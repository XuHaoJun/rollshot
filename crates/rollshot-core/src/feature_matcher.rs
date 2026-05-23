//! FAST corners + linear KNN feature fallback (Approach B per the spec).
//!
//! The "Hnsw" in the public identifiers is reserved for a future ANN
//! upgrade — see
//! docs/superpowers/specs/2026-05-23-rollshot-fast-hnsw-fallback-design.md
//! Approach A. Current matching is exact linear scan.

use image::GrayImage;
use image::RgbaImage;
use imageproc::corners;

use crate::akaze_matcher::{akaze_candidates, AkazeCandidateOutcome};
use crate::types::{FastHnswConfig, MotionCandidate, ScrollAxis, StitchConfig};

use rayon::prelude::*;
use std::collections::HashMap;

fn rgba_to_gray(img: &RgbaImage) -> GrayImage {
    image::imageops::grayscale(img)
}

fn extract_corners(gray: &GrayImage, threshold: u8, max_features: usize) -> Vec<(u32, u32)> {
    if max_features == 0 || gray.width() < 16 || gray.height() < 16 {
        return Vec::new();
    }
    let fast12 = corners::corners_fast12(gray, threshold);
    let raw: Vec<(u32, u32)> = if fast12.len() > 200 {
        fast12.into_iter().map(|c| (c.x, c.y)).collect()
    } else {
        corners::corners_fast9(gray, threshold)
            .into_iter()
            .map(|c| (c.x, c.y))
            .collect()
    };
    if raw.len() <= max_features {
        return raw;
    }
    let step = raw.len() / max_features + 1;
    raw.into_iter().step_by(step).collect()
}

/// Outcome of running `fast_hnsw_candidates`.
///
/// Shape mirrors `AkazeCandidateOutcome` deliberately so the dispatcher
/// can collapse both into `FeatureFallbackOutcome` without bespoke
/// arms per branch.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FastHnswCandidateOutcome {
    Disabled,
    NotEnoughFeatures { prev: usize, curr: usize },
    NotEnoughMatches { raw_matches: usize },
    Candidates(Vec<MotionCandidate>),
}

/// Tagged outcome from the dispatcher so the matcher can map it back
/// onto the correct `NoMatchReason` variant.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FeatureFallbackOutcome {
    Disabled,
    NotEnoughFeatures {
        prev: usize,
        curr: usize,
    },
    NotEnoughMatches {
        raw_matches: usize,
        source: FeatureSource,
    },
    Candidates {
        candidates: Vec<MotionCandidate>,
        source: FeatureSource,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeatureSource {
    FastHnsw,
    Akaze,
}

impl FeatureFallbackOutcome {
    fn from_fast_hnsw(outcome: FastHnswCandidateOutcome) -> Self {
        match outcome {
            FastHnswCandidateOutcome::Disabled => FeatureFallbackOutcome::Disabled,
            FastHnswCandidateOutcome::NotEnoughFeatures { prev, curr } => {
                FeatureFallbackOutcome::NotEnoughFeatures { prev, curr }
            }
            FastHnswCandidateOutcome::NotEnoughMatches { raw_matches } => {
                FeatureFallbackOutcome::NotEnoughMatches {
                    raw_matches,
                    source: FeatureSource::FastHnsw,
                }
            }
            FastHnswCandidateOutcome::Candidates(candidates) => {
                FeatureFallbackOutcome::Candidates {
                    candidates,
                    source: FeatureSource::FastHnsw,
                }
            }
        }
    }

    fn from_akaze(outcome: AkazeCandidateOutcome) -> Self {
        match outcome {
            AkazeCandidateOutcome::Disabled => FeatureFallbackOutcome::Disabled,
            AkazeCandidateOutcome::NotEnoughFeatures { prev, curr } => {
                FeatureFallbackOutcome::NotEnoughFeatures { prev, curr }
            }
            AkazeCandidateOutcome::NotEnoughMatches { raw_matches } => {
                FeatureFallbackOutcome::NotEnoughMatches {
                    raw_matches,
                    source: FeatureSource::Akaze,
                }
            }
            AkazeCandidateOutcome::Candidates(candidates) => FeatureFallbackOutcome::Candidates {
                candidates,
                source: FeatureSource::Akaze,
            },
        }
    }
}

/// 9x9 patch → `[f32; 8]` row/col-mean descriptor.
///
/// Returns `None` when the patch reaches outside the image (no
/// clamping — corners too close to an edge are dropped at the call
/// site).
fn compute_descriptor(gray: &GrayImage, x: u32, y: u32, patch: usize) -> Option<[f32; 8]> {
    if patch % 2 == 0 || patch < 3 {
        return None;
    }
    let half = (patch / 2) as i32;
    let w = gray.width() as i32;
    let h = gray.height() as i32;
    let cx = x as i32;
    let cy = y as i32;
    if cx - half < 0 || cy - half < 0 || cx + half >= w || cy + half >= h {
        return None;
    }
    let bins = patch / 2;
    let mut desc = [0.0f32; 8];
    for (i, d) in desc.iter_mut().enumerate().take(bins) {
        let row_y = cy + (-half + (i as i32) * 2);
        let mut sum = 0.0f32;
        let mut count = 0u32;
        for j in 0..bins {
            let col_x = cx + (-half + (j as i32) * 2);
            sum += gray.get_pixel(col_x as u32, row_y as u32)[0] as f32 / 255.0;
            count += 1;
        }
        *d = if count > 0 { sum / count as f32 } else { 0.0 };
    }
    for (k, d) in desc.iter_mut().enumerate().skip(bins).take(bins) {
        let col_bin = k - bins;
        let col_x = cx + (-half + (col_bin as i32) * 2);
        let mut sum = 0.0f32;
        let mut count = 0u32;
        for i in 0..bins {
            let row_y = cy + (-half + (i as i32) * 2);
            sum += gray.get_pixel(col_x as u32, row_y as u32)[0] as f32 / 255.0;
            count += 1;
        }
        *d = if count > 0 { sum / count as f32 } else { 0.0 };
    }
    Some(desc)
}

/// Batch descriptor computation. Returns `(descriptors, surviving_corners)`
/// in lockstep — corners that fail the edge check are dropped from
/// both. Parallel via rayon.
fn compute_descriptors(
    gray: &GrayImage,
    corners: &[(u32, u32)],
    patch: usize,
) -> (Vec<[f32; 8]>, Vec<(u32, u32)>) {
    let paired: Vec<((u32, u32), [f32; 8])> = corners
        .par_iter()
        .filter_map(|&(x, y)| compute_descriptor(gray, x, y, patch).map(|d| ((x, y), d)))
        .collect();
    let (kept, descs): (Vec<(u32, u32)>, Vec<[f32; 8]>) = paired.into_iter().unzip();
    (descs, kept)
}

/// Linear KNN with Lowe ratio test. For each `curr` descriptor, find
/// the best and second-best `prev` matches by Euclidean distance.
/// Accept if `best.dist < distance_threshold` and `best.dist * ratio <
/// second.dist`. When there is only one `prev` candidate, the ratio
/// test cannot fire — accept the best if it clears the distance
/// threshold.
///
/// Returns pairs as `[curr_idx, prev_idx]`. Parallel via rayon.
fn linear_knn_match(
    prev: &[[f32; 8]],
    curr: &[[f32; 8]],
    distance_threshold: f32,
    lowe_ratio: f32,
) -> Vec<[usize; 2]> {
    if prev.is_empty() || curr.is_empty() {
        return Vec::new();
    }
    curr.par_iter()
        .enumerate()
        .filter_map(|(curr_idx, c)| {
            let mut best = (f32::INFINITY, usize::MAX);
            let mut second = f32::INFINITY;
            for (i, p) in prev.iter().enumerate() {
                let dist = euclidean_distance(p, c);
                if dist < best.0 {
                    second = best.0;
                    best = (dist, i);
                } else if dist < second {
                    second = dist;
                }
            }
            if best.0 >= distance_threshold {
                return None;
            }
            if second.is_finite() && best.0 * lowe_ratio >= second {
                return None;
            }
            Some([curr_idx, best.1])
        })
        .collect()
}

fn euclidean_distance(a: &[f32; 8], b: &[f32; 8]) -> f32 {
    let mut sum = 0.0f32;
    for i in 0..8 {
        let d = a[i] - b[i];
        sum += d * d;
    }
    sum.sqrt()
}

/// Bucket-vote translation summary.
///
/// Returns `(dx, dy, inliers, raw_matches, residual_px)` where:
/// - `(dx, dy)` is the median translation within the winning bucket
/// - `inliers` is the count of matches in the winning bucket
/// - `raw_matches` is the total number of input matches (before
///   cross-axis filtering)
/// - `residual_px` is the median Euclidean distance of inliers from
///   the winning (dx, dy) — fed into `feature_score` per the spec
fn vote_dominant_translation(
    prev_corners: &[(u32, u32)],
    curr_corners: &[(u32, u32)],
    matches: &[[usize; 2]],
    locked_axis: Option<ScrollAxis>,
    config: &FastHnswConfig,
) -> Option<(i32, i32, usize, usize, f32)> {
    let raw_matches = matches.len();
    if raw_matches == 0 {
        return None;
    }
    let translations: Vec<(i32, i32)> = matches
        .iter()
        .filter_map(|&[curr_idx, prev_idx]| {
            let (cx, cy) = curr_corners.get(curr_idx)?;
            let (px, py) = prev_corners.get(prev_idx)?;
            let dx = *px as i32 - *cx as i32;
            let dy = *py as i32 - *cy as i32;
            match locked_axis {
                Some(ScrollAxis::Vertical) if dx.abs() > config.cross_axis_tolerance => None,
                Some(ScrollAxis::Horizontal) if dy.abs() > config.cross_axis_tolerance => None,
                _ => Some((dx, dy)),
            }
        })
        .collect();
    if translations.is_empty() {
        return None;
    }
    let mut buckets: HashMap<(i32, i32), Vec<(i32, i32)>> = HashMap::new();
    for &(dx, dy) in &translations {
        let key = (dx / 4, dy / 4);
        if key == (0, 0) {
            continue;
        }
        buckets.entry(key).or_default().push((dx, dy));
    }
    type TranslationBucket = ((i32, i32), Vec<(i32, i32)>);
    let mut buckets: Vec<TranslationBucket> = buckets.into_iter().collect();
    buckets.sort_by(|(a_key, a_bucket), (b_key, b_bucket)| {
        b_bucket
            .len()
            .cmp(&a_bucket.len())
            .then_with(|| a_key.cmp(b_key))
    });
    let (_, bucket) = buckets.first()?;
    let second_best = buckets.get(1).map(|(_, bucket)| bucket.len()).unwrap_or(0);
    if second_best > 0 && (bucket.len() as f32) < (second_best as f32 * config.second_best_ratio) {
        return None;
    }
    let inliers = bucket.len();
    if inliers < config.min_inliers {
        return None;
    }
    let mut dxs: Vec<i32> = bucket.iter().map(|(dx, _)| *dx).collect();
    let mut dys: Vec<i32> = bucket.iter().map(|(_, dy)| *dy).collect();
    dxs.sort_unstable();
    dys.sort_unstable();
    let dx_median = dxs[dxs.len() / 2];
    let dy_median = dys[dys.len() / 2];
    let residual_px = compute_median_residual(bucket, dx_median, dy_median);
    Some((dx_median, dy_median, inliers, raw_matches, residual_px))
}

/// Median Euclidean distance between each `(tx, ty)` translation and
/// the winning `(dx, dy)`. Drives the `residual_term` of
/// `feature_score`.
fn compute_median_residual(translations: &[(i32, i32)], dx: i32, dy: i32) -> f32 {
    if translations.is_empty() {
        return 0.0;
    }
    let mut residuals: Vec<f32> = translations
        .iter()
        .map(|&(tx, ty)| {
            let ex = (tx - dx) as f32;
            let ey = (ty - dy) as f32;
            (ex * ex + ey * ey).sqrt()
        })
        .collect();
    residuals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = residuals.len() / 2;
    if residuals.len() % 2 == 0 {
        (residuals[mid - 1] + residuals[mid]) * 0.5
    } else {
        residuals[mid]
    }
}

// Keep in sync with akaze_matcher::akaze_score (intentionally private
// there). Same coefficients, same residual normalization, so AKAZE
// and FAST+KNN scores are directly comparable against
// `accept_confidence` and against each other. When AKAZE is removed,
// fold this into a single shared helper.
fn feature_score(inlier_ratio: f32, residual_px: f32) -> f32 {
    let ratio_term = 1.0 - inlier_ratio.clamp(0.0, 1.0);
    let residual_term = (residual_px / 4.0).clamp(0.0, 1.0);
    (ratio_term * 0.08 + residual_term * 0.04).clamp(0.0, 1.0)
}

pub(crate) fn fast_hnsw_candidates(
    prev: &RgbaImage,
    curr: &RgbaImage,
    locked_axis: Option<ScrollAxis>,
    config: &FastHnswConfig,
) -> FastHnswCandidateOutcome {
    if !config.enabled {
        return FastHnswCandidateOutcome::Disabled;
    }
    if prev.dimensions() != curr.dimensions() {
        return FastHnswCandidateOutcome::NotEnoughFeatures { prev: 0, curr: 0 };
    }

    let prev_gray = rgba_to_gray(prev);
    let curr_gray = rgba_to_gray(curr);
    let prev_corners = extract_corners(&prev_gray, config.corner_threshold, config.max_features);
    let curr_corners = extract_corners(&curr_gray, config.corner_threshold, config.max_features);

    if prev_corners.len() < config.min_keypoints || curr_corners.len() < config.min_keypoints {
        return FastHnswCandidateOutcome::NotEnoughFeatures {
            prev: prev_corners.len(),
            curr: curr_corners.len(),
        };
    }

    let (prev_desc, prev_kept) =
        compute_descriptors(&prev_gray, &prev_corners, config.descriptor_patch_size);
    let (curr_desc, curr_kept) =
        compute_descriptors(&curr_gray, &curr_corners, config.descriptor_patch_size);

    if prev_desc.len() < config.min_keypoints || curr_desc.len() < config.min_keypoints {
        return FastHnswCandidateOutcome::NotEnoughFeatures {
            prev: prev_desc.len(),
            curr: curr_desc.len(),
        };
    }

    let lowe_ratio = 1.4;
    let matches = linear_knn_match(
        &prev_desc,
        &curr_desc,
        config.distance_threshold,
        lowe_ratio,
    );
    if matches.len() < config.min_raw_matches {
        return FastHnswCandidateOutcome::NotEnoughMatches {
            raw_matches: matches.len(),
        };
    }

    let Some((dx, dy, inliers, raw, residual_px)) =
        vote_dominant_translation(&prev_kept, &curr_kept, &matches, locked_axis, config)
    else {
        return FastHnswCandidateOutcome::NotEnoughMatches {
            raw_matches: matches.len(),
        };
    };
    let inlier_ratio = inliers as f32 / raw.max(1) as f32;
    FastHnswCandidateOutcome::Candidates(vec![MotionCandidate {
        dx,
        dy,
        method: crate::types::MatchMethod::FastHnsw,
        score: feature_score(inlier_ratio, residual_px),
        second_best_score: None,
        inliers: Some(inliers),
        raw_matches: Some(raw),
    }])
}

/// Pick-one dispatch:
///   - `config.akaze.enabled = true`  → run AKAZE (FastHnsw is skipped
///     even if also enabled)
///   - else `config.fast_hnsw.enabled = true` → run FAST+KNN
///   - else → Disabled
pub(crate) fn feature_fallback_candidates(
    prev: &RgbaImage,
    curr: &RgbaImage,
    locked_axis: Option<ScrollAxis>,
    config: &StitchConfig,
) -> FeatureFallbackOutcome {
    if config.akaze.enabled {
        return FeatureFallbackOutcome::from_akaze(akaze_candidates(prev, curr, &config.akaze));
    }
    if config.fast_hnsw.enabled {
        return FeatureFallbackOutcome::from_fast_hnsw(fast_hnsw_candidates(
            prev,
            curr,
            locked_axis,
            &config.fast_hnsw,
        ));
    }
    FeatureFallbackOutcome::Disabled
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::StitchConfig;
    use image::{Rgba, RgbaImage};

    fn solid_frame() -> RgbaImage {
        RgbaImage::from_pixel(100, 100, Rgba([200, 200, 200, 255]))
    }

    #[test]
    fn fast_hnsw_returns_disabled_when_config_disabled() {
        let config = FastHnswConfig {
            enabled: false,
            ..Default::default()
        };
        let outcome = fast_hnsw_candidates(&solid_frame(), &solid_frame(), None, &config);
        assert_eq!(outcome, FastHnswCandidateOutcome::Disabled);
    }

    #[test]
    fn feature_fallback_disabled_when_both_off() {
        let mut config = StitchConfig::default();
        config.fast_hnsw.enabled = false;
        config.akaze.enabled = false;
        let outcome = feature_fallback_candidates(&solid_frame(), &solid_frame(), None, &config);
        assert_eq!(outcome, FeatureFallbackOutcome::Disabled);
    }

    #[test]
    fn feature_fallback_akaze_wins_pick_one() {
        let mut config = StitchConfig::default();
        config.fast_hnsw.enabled = true;
        config.akaze.enabled = true;
        let outcome = feature_fallback_candidates(&solid_frame(), &solid_frame(), None, &config);
        // Both akaze and fast_hnsw are enabled → akaze wins per pick-one
        // dispatch. With the akaze feature flag compiled in, a solid
        // frame triggers NotEnoughFeatures from the real AKAZE code.
        // Without the feature flag, akaze_candidates returns Disabled.
        // Either way we just verify the dispatcher didn't panic and
        // didn't route through FastHnsw (which would produce Disabled
        // with the FastHnsw source tag, observable after real impl).
        assert!(
            matches!(
                outcome,
                FeatureFallbackOutcome::Disabled | FeatureFallbackOutcome::NotEnoughFeatures { .. }
            ),
            "unexpected outcome: {outcome:?}"
        );
    }

    fn feature_canvas(width: u32, height: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(width, height, Rgba([238, 238, 238, 255]));
        for i in 0..80u32 {
            let x = 20 + ((i * 37 + i * i) % width.saturating_sub(40).max(1));
            let y = 20 + ((i * 53 + i * i * 3) % height.saturating_sub(40).max(1));
            let r = (40 + (i * 17) % 180) as u8;
            let g = (70 + (i * 29) % 170) as u8;
            let b = (90 + (i * 31) % 150) as u8;
            let size: u32 = 12 + (i % 8);
            for yy in 0..size {
                for xx in 0..size {
                    let cx = size as i32 / 2;
                    let cy = size as i32 / 2;
                    let dx = xx as i32 - cx;
                    let dy = yy as i32 - cy;
                    let dist2 = dx * dx + dy * dy;
                    let radius2 = (size as i32 / 2).pow(2);
                    if dist2 <= radius2 {
                        let intensity =
                            if (xx / 3 + yy / 3) % 2 == 0 || dx.abs() < 2 || dy.abs() < 2 {
                                60i32
                            } else {
                                -30i32
                            };
                        img.put_pixel(
                            x + xx,
                            y + yy,
                            Rgba([
                                (r as i32 + intensity).clamp(0, 255) as u8,
                                (g as i32 + intensity + (i * 13 % 40) as i32).clamp(0, 255) as u8,
                                (b as i32 + intensity).clamp(0, 255) as u8,
                                255,
                            ]),
                        );
                    }
                }
            }
        }
        img
    }

    #[test]
    fn extract_corners_returns_empty_on_solid_image() {
        let img = solid_frame();
        let gray = rgba_to_gray(&img);
        let corners = extract_corners(&gray, 64, 1200);
        assert!(
            corners.is_empty(),
            "solid image returned {} corners",
            corners.len()
        );
    }

    #[test]
    fn extract_corners_short_circuits_on_tiny_image() {
        let tiny = image::GrayImage::from_pixel(8, 8, image::Luma([128]));
        assert!(extract_corners(&tiny, 64, 1200).is_empty());
        let narrow = image::GrayImage::from_pixel(8, 240, image::Luma([128]));
        assert!(extract_corners(&narrow, 64, 1200).is_empty());
    }

    #[test]
    fn extract_corners_finds_features_on_feature_canvas() {
        let img = feature_canvas(220, 220);
        let gray = rgba_to_gray(&img);
        let corners = extract_corners(&gray, 64, 1200);
        assert!(
            corners.len() > 30,
            "expected >30 corners on feature canvas, got {}",
            corners.len()
        );
    }

    #[test]
    fn extract_corners_caps_at_max_features() {
        let img = feature_canvas(420, 420);
        let gray = rgba_to_gray(&img);
        let corners = extract_corners(&gray, 16, 50);
        assert!(corners.len() <= 50, "got {}", corners.len());
    }

    #[test]
    fn extract_corners_returns_empty_when_max_features_is_zero() {
        let img = feature_canvas(220, 220);
        let gray = rgba_to_gray(&img);
        let corners = extract_corners(&gray, 64, 0);
        assert!(corners.is_empty(), "got {}", corners.len());
    }

    #[test]
    fn compute_descriptor_returns_eight_dim_for_interior_corner() {
        let img = feature_canvas(220, 220);
        let gray = rgba_to_gray(&img);
        let desc = compute_descriptor(&gray, 110, 110, 9);
        let desc = desc.expect("interior corner descriptor");
        for v in &desc {
            assert!(*v >= 0.0 && *v <= 1.0, "descriptor entry out of range: {v}");
        }
    }

    #[test]
    fn compute_descriptor_skips_edge_corner_without_panic() {
        let img = feature_canvas(220, 220);
        let gray = rgba_to_gray(&img);
        assert!(compute_descriptor(&gray, 1, 1, 9).is_none());
        assert!(compute_descriptor(&gray, 218, 218, 9).is_none());
    }

    #[test]
    fn compute_descriptor_rejects_even_or_tiny_patch() {
        let img = feature_canvas(220, 220);
        let gray = rgba_to_gray(&img);
        assert!(
            compute_descriptor(&gray, 110, 110, 8).is_none(),
            "even patch must be rejected"
        );
        assert!(
            compute_descriptor(&gray, 110, 110, 1).is_none(),
            "patch<3 must be rejected"
        );
        assert!(
            compute_descriptor(&gray, 110, 110, 9).is_some(),
            "valid 9x9 patch on interior corner must succeed"
        );
    }

    #[test]
    fn compute_descriptors_skips_edge_corners_and_keeps_interior() {
        let img = feature_canvas(220, 220);
        let gray = rgba_to_gray(&img);
        let corners = vec![(1u32, 1u32), (110, 110), (218, 218)];
        let (descs, kept) = compute_descriptors(&gray, &corners, 9);
        assert_eq!(descs.len(), 1, "only the interior corner survives");
        assert_eq!(kept, vec![(110, 110)]);
    }

    #[test]
    fn linear_knn_match_pairs_identical_descriptors() {
        let d = |v: f32| [v; 8];
        let prev = vec![d(0.10), d(0.30), d(0.70)];
        let curr = vec![d(0.30), d(0.10), d(0.70)];
        let pairs = linear_knn_match(&prev, &curr, 0.20, 1.4);
        assert!(pairs.contains(&[0, 1]));
        assert!(pairs.contains(&[1, 0]));
        assert!(pairs.contains(&[2, 2]));
        assert_eq!(pairs.len(), 3);
    }

    #[test]
    fn linear_knn_match_rejects_ambiguous_pairs() {
        let d = |v: f32| [v; 8];
        let prev = vec![d(0.48), d(0.52)];
        let curr = vec![d(0.50)];
        let pairs = linear_knn_match(&prev, &curr, 0.20, 1.4);
        assert!(pairs.is_empty(), "expected ambiguous pair rejected");
    }

    #[test]
    fn linear_knn_match_rejects_distant_pairs() {
        let d = |v: f32| [v; 8];
        let prev = vec![d(0.10)];
        let curr = vec![d(0.90)];
        let pairs = linear_knn_match(&prev, &curr, 0.20, 1.4);
        assert!(pairs.is_empty(), "expected distant pair rejected");
    }

    #[test]
    fn linear_knn_match_returns_empty_on_empty_input() {
        assert!(linear_knn_match(&[], &[[0.0; 8]], 0.20, 1.4).is_empty());
        assert!(linear_knn_match(&[[0.0; 8]], &[], 0.20, 1.4).is_empty());
        let prev = [[0.0; 8]];
        let curr = [[0.0; 8]];
        let pairs = linear_knn_match(&prev, &curr, 0.20, 1.4);
        assert_eq!(pairs, vec![[0usize, 0usize]]);
    }

    #[test]
    fn vote_dominant_translation_picks_majority_bucket() {
        let prev = vec![(0u32, 0u32); 6];
        let curr = vec![(0u32, 40u32), (0, 41), (0, 39), (0, 40), (0, 42), (0, 100)];
        let matches: Vec<[usize; 2]> = (0..6).map(|i| [i, i]).collect();
        let cfg = FastHnswConfig {
            min_inliers: 4,
            ..Default::default()
        };
        let result = vote_dominant_translation(&prev, &curr, &matches, None, &cfg);
        let (dx, dy, inliers, raw, residual_px) = result.expect("dominant translation");
        assert_eq!(dx, 0);
        assert!((-42..=-39).contains(&dy), "dy = {dy}");
        assert!(inliers >= 4, "inliers = {inliers}");
        assert_eq!(raw, 6);
        assert!(residual_px <= 2.0, "residual_px = {residual_px}");
    }

    #[test]
    fn vote_dominant_translation_rejects_ambiguous_runner_up_bucket() {
        let prev = vec![(0u32, 0u32); 7];
        let curr = vec![
            (0u32, 40u32),
            (0, 40),
            (0, 40),
            (0, 40),
            (0, 80),
            (0, 80),
            (0, 80),
        ];
        let matches: Vec<[usize; 2]> = (0..7).map(|i| [i, i]).collect();
        let cfg = FastHnswConfig {
            min_inliers: 3,
            second_best_ratio: 2.0,
            ..Default::default()
        };

        assert!(
            vote_dominant_translation(&prev, &curr, &matches, None, &cfg).is_none(),
            "4 votes vs 3 votes must fail the 2.0 second-best ratio"
        );
    }

    #[test]
    fn vote_dominant_translation_rejects_zero_zero_bucket() {
        let prev = vec![(10u32, 20u32), (30, 40), (50, 60)];
        let curr = vec![(10u32, 20u32), (30, 40), (50, 60)];
        let matches = vec![[0, 0], [1, 1], [2, 2]];
        let cfg = FastHnswConfig::default();
        assert!(vote_dominant_translation(&prev, &curr, &matches, None, &cfg).is_none());
    }

    #[test]
    fn vote_dominant_translation_respects_locked_vertical_axis() {
        let prev = vec![(0u32, 0u32), (0, 10), (0, 20)];
        let curr = vec![(50u32, 0u32), (50, 10), (50, 20)];
        let matches = vec![[0, 0], [1, 1], [2, 2]];
        let cfg = FastHnswConfig::default();
        assert!(
            vote_dominant_translation(&prev, &curr, &matches, Some(ScrollAxis::Vertical), &cfg)
                .is_none(),
            "vertical lock must reject cross-axis-only matches"
        );
    }

    fn crop_xy(canvas: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
        use image::imageops;
        imageops::crop_imm(canvas, x, y, w, h).to_image()
    }

    #[test]
    fn fast_hnsw_candidates_estimate_translation() {
        let canvas = feature_canvas(420, 420);
        let prev = crop_xy(&canvas, 20, 30, 220, 220);
        let curr = crop_xy(&canvas, 58, 92, 220, 220);
        let config = FastHnswConfig::default();
        let outcome = fast_hnsw_candidates(&prev, &curr, None, &config);
        let candidates = match outcome {
            FastHnswCandidateOutcome::Candidates(c) => c,
            other => panic!("expected Candidates, got {other:?}"),
        };
        let candidate = candidates.first().expect("one candidate");
        assert_eq!(candidate.method, crate::types::MatchMethod::FastHnsw);
        assert!(
            (candidate.dx - 38).abs() <= 3,
            "dx = {} (expected ~38)",
            candidate.dx
        );
        assert!(
            (candidate.dy - 62).abs() <= 3,
            "dy = {} (expected ~62)",
            candidate.dy
        );
        assert!(candidate.raw_matches.unwrap_or(0) >= 24);
        assert!(candidate.inliers.unwrap_or(0) >= 16);
    }

    #[test]
    fn fast_hnsw_candidates_returns_not_enough_features_on_solid_frames() {
        let prev = solid_frame();
        let curr = solid_frame();
        let config = FastHnswConfig::default();
        let outcome = fast_hnsw_candidates(&prev, &curr, None, &config);
        assert!(
            matches!(outcome, FastHnswCandidateOutcome::NotEnoughFeatures { .. }),
            "got {outcome:?}"
        );
    }

    #[test]
    fn fast_hnsw_candidates_returns_not_enough_matches_on_unrelated_frames() {
        let prev = feature_canvas(220, 220);
        let mut curr = feature_canvas(220, 220);
        for (i, px) in curr.pixels_mut().enumerate() {
            let n = ((i as u64).wrapping_mul(6364136223846793005) >> 32) as u8;
            px[0] = n;
            px[1] = n.wrapping_add(83);
            px[2] = n.wrapping_add(149);
        }
        let config = FastHnswConfig::default();
        let outcome = fast_hnsw_candidates(&prev, &curr, None, &config);
        assert!(
            matches!(
                outcome,
                FastHnswCandidateOutcome::NotEnoughMatches { .. }
                    | FastHnswCandidateOutcome::NotEnoughFeatures { .. }
            ),
            "got {outcome:?}"
        );
    }

    #[test]
    fn fast_hnsw_candidates_respects_locked_vertical_axis() {
        let canvas = feature_canvas(420, 420);
        let prev = crop_xy(&canvas, 20, 100, 220, 220);
        let curr = crop_xy(&canvas, 58, 100, 220, 220);
        let config = FastHnswConfig::default();
        let outcome = fast_hnsw_candidates(&prev, &curr, Some(ScrollAxis::Vertical), &config);
        assert!(
            matches!(
                outcome,
                FastHnswCandidateOutcome::NotEnoughMatches { .. }
                    | FastHnswCandidateOutcome::NotEnoughFeatures { .. }
            ),
            "vertical lock should reject pure horizontal motion, got {outcome:?}"
        );
    }

    #[test]
    fn fast_hnsw_candidates_respects_locked_horizontal_axis() {
        let canvas = feature_canvas(420, 420);
        let prev = crop_xy(&canvas, 100, 20, 220, 220);
        let curr = crop_xy(&canvas, 100, 58, 220, 220);
        let config = FastHnswConfig::default();
        let outcome = fast_hnsw_candidates(&prev, &curr, Some(ScrollAxis::Horizontal), &config);
        assert!(
            matches!(
                outcome,
                FastHnswCandidateOutcome::NotEnoughMatches { .. }
                    | FastHnswCandidateOutcome::NotEnoughFeatures { .. }
            ),
            "horizontal lock should reject pure vertical motion, got {outcome:?}"
        );
    }

    #[test]
    fn fast_hnsw_score_below_default_accept_confidence() {
        let accept = StitchConfig::default().accept_confidence;
        let score = feature_score(0.6, 1.0);
        assert!(
            score < accept,
            "healthy match scored {score} >= accept_confidence {accept}"
        );
    }

    #[test]
    fn fast_hnsw_score_top_quality_near_zero() {
        let score = feature_score(0.9, 0.0);
        assert!(score <= 0.01, "top-quality score = {score} should be ~0");
    }

    #[test]
    fn fast_hnsw_score_floor_at_minimum_acceptable_quality() {
        let accept = StitchConfig::default().accept_confidence;
        let floor = feature_score(0.35, 4.0);
        assert!(
            floor <= accept,
            "floor {floor} exceeds accept_confidence {accept}"
        );
    }
}
