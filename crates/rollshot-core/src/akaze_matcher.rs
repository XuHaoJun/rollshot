use image::RgbaImage;

use crate::types::{AkazeConfig, MotionCandidate};

#[cfg(feature = "akaze")]
use crate::types::MatchMethod;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AkazeCandidateOutcome {
    Disabled,
    NotEnoughFeatures { prev: usize, curr: usize },
    NotEnoughMatches { raw_matches: usize },
    Candidates(Vec<MotionCandidate>),
}

#[cfg(not(feature = "akaze"))]
pub(crate) fn akaze_candidates(
    _prev: &RgbaImage,
    _curr: &RgbaImage,
    _config: &AkazeConfig,
) -> AkazeCandidateOutcome {
    AkazeCandidateOutcome::Disabled
}

#[cfg(feature = "akaze")]
fn akaze_score(inlier_ratio: f32, residual_px: f32) -> f32 {
    let ratio_term = 1.0 - inlier_ratio.clamp(0.0, 1.0);
    let residual_term = (residual_px / 4.0).clamp(0.0, 1.0);
    (ratio_term * 0.08 + residual_term * 0.04).clamp(0.0, 1.0)
}

#[cfg(feature = "akaze")]
pub(crate) fn akaze_candidates(
    prev: &RgbaImage,
    curr: &RgbaImage,
    config: &AkazeConfig,
) -> AkazeCandidateOutcome {
    use akaze::{Akaze, KeyPoint};
    use bitarray::{BitArray, Hamming};
    use space::{Knn, LinearKnn};
    use std::collections::BTreeMap;

    const TRANSLATION_BUCKET_PX: f32 = 4.0;

    fn matching(a: &[BitArray<64>], b: &[BitArray<64>]) -> Vec<Option<usize>> {
        if b.len() < 2 {
            return vec![None; a.len()];
        }

        let knn_b = LinearKnn {
            metric: Hamming,
            iter: b.iter(),
        };

        (0..a.len())
            .map(|a_idx| {
                let neighbors = knn_b.knn(&a[a_idx], 2);
                if neighbors.len() < 2 {
                    return None;
                }
                if neighbors[0].distance + 24 < neighbors[1].distance {
                    Some(neighbors[0].index)
                } else {
                    None
                }
            })
            .collect()
    }

    fn symmetric_matching(a: &[BitArray<64>], b: &[BitArray<64>]) -> Vec<[usize; 2]> {
        let forward = matching(a, b);
        let reverse = matching(b, a);

        forward
            .into_iter()
            .enumerate()
            .filter_map(|(a_idx, b_idx)| {
                b_idx
                    .map(|b_idx| [a_idx, b_idx])
                    .filter(|&[a_idx, b_idx]| reverse[b_idx] == Some(a_idx))
            })
            .collect()
    }

    fn median(mut values: Vec<f32>) -> f32 {
        values.sort_by(f32::total_cmp);
        let mid = values.len() / 2;
        if values.len() % 2 == 0 {
            (values[mid - 1] + values[mid]) * 0.5
        } else {
            values[mid]
        }
    }

    fn median_residual(translations: &[(f32, f32)], dx: f32, dy: f32) -> f32 {
        let residuals = translations
            .iter()
            .map(|(tx, ty)| ((tx - dx).powi(2) + (ty - dy).powi(2)).sqrt())
            .collect();
        median(residuals)
    }

    fn dominant_translation(
        prev_keypoints: &[KeyPoint],
        curr_keypoints: &[KeyPoint],
        matches: &[[usize; 2]],
    ) -> Option<(i32, i32, usize, f32)> {
        let translations: Vec<(f32, f32)> = matches
            .iter()
            .map(|&[prev_idx, curr_idx]| {
                let (px, py) = prev_keypoints[prev_idx].point;
                let (cx, cy) = curr_keypoints[curr_idx].point;
                (px - cx, py - cy)
            })
            .collect();

        let mut buckets: BTreeMap<(i32, i32), Vec<(f32, f32)>> = BTreeMap::new();
        for &(dx, dy) in &translations {
            let key = (
                (dx / TRANSLATION_BUCKET_PX).round() as i32,
                (dy / TRANSLATION_BUCKET_PX).round() as i32,
            );
            buckets.entry(key).or_default().push((dx, dy));
        }

        let bucket = buckets.into_values().max_by_key(Vec::len)?;
        let dx = median(bucket.iter().map(|(dx, _)| *dx).collect());
        let dy = median(bucket.iter().map(|(_, dy)| *dy).collect());
        let inliers: Vec<(f32, f32)> = translations
            .into_iter()
            .filter(|(tx, ty)| {
                let residual = ((tx - dx).powi(2) + (ty - dy).powi(2)).sqrt();
                residual <= TRANSLATION_BUCKET_PX
            })
            .collect();
        let residual = median_residual(&inliers, dx, dy);

        Some((
            dx.round() as i32,
            dy.round() as i32,
            inliers.len(),
            residual,
        ))
    }

    if !config.enabled {
        return AkazeCandidateOutcome::Disabled;
    }

    let mut extractor = Akaze::new(config.detector_threshold);
    extractor.maximum_features = config.max_features;

    let prev_image = image::DynamicImage::ImageLuma8(image::imageops::grayscale(prev));
    let curr_image = image::DynamicImage::ImageLuma8(image::imageops::grayscale(curr));
    let (prev_keypoints, prev_descriptors) = extractor.extract(&prev_image);
    let (curr_keypoints, curr_descriptors) = extractor.extract(&curr_image);

    if prev_descriptors.len() < 2 || curr_descriptors.len() < 2 {
        return AkazeCandidateOutcome::NotEnoughFeatures {
            prev: prev_descriptors.len(),
            curr: curr_descriptors.len(),
        };
    }

    let matches = symmetric_matching(&prev_descriptors, &curr_descriptors);
    let raw_matches = matches.len();
    if raw_matches < config.min_raw_matches {
        return AkazeCandidateOutcome::NotEnoughMatches { raw_matches };
    }

    let Some((dx, dy, inliers, residual_px)) =
        dominant_translation(&prev_keypoints, &curr_keypoints, &matches)
    else {
        return AkazeCandidateOutcome::NotEnoughMatches { raw_matches };
    };

    let inlier_ratio = inliers as f32 / raw_matches as f32;
    if inliers < config.min_inliers || inlier_ratio < config.min_inlier_ratio {
        return AkazeCandidateOutcome::NotEnoughMatches { raw_matches };
    }

    AkazeCandidateOutcome::Candidates(vec![MotionCandidate {
        dx,
        dy,
        method: MatchMethod::Akaze,
        score: akaze_score(inlier_ratio, residual_px),
        second_best_score: None,
        inliers: Some(inliers),
        raw_matches: Some(raw_matches),
    }])
}

#[cfg(all(test, feature = "akaze"))]
mod tests {
    use image::{imageops, Rgba, RgbaImage};

    use crate::akaze_matcher::{akaze_candidates, AkazeCandidateOutcome};
    use crate::types::{AkazeConfig, MatchMethod};

    fn feature_canvas(width: u32, height: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(width, height, Rgba([238, 238, 238, 255]));
        for i in 0..80u32 {
            let x = 20 + ((i * 37 + i * i) % (width.saturating_sub(40).max(1)));
            let y = 20 + ((i * 53 + i * i * 3) % (height.saturating_sub(40).max(1)));
            let r = (40 + (i * 17) % 180) as u8;
            let g = (70 + (i * 29) % 170) as u8;
            let b = (90 + (i * 31) % 150) as u8;
            let size: u32 = 12 + (i % 8) as u32;
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

    fn crop_xy(canvas: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
        imageops::crop_imm(canvas, x, y, w, h).to_image()
    }

    fn test_config() -> AkazeConfig {
        AkazeConfig {
            enabled: true,
            max_features: 800,
            detector_threshold: 0.0005,
            min_raw_matches: 8,
            min_inliers: 6,
            min_inlier_ratio: 0.25,
        }
    }

    #[test]
    fn akaze_candidates_estimate_translation() {
        let canvas = feature_canvas(420, 420);
        let prev = crop_xy(&canvas, 20, 30, 220, 220);
        let curr = crop_xy(&canvas, 58, 92, 220, 220);

        let outcome = akaze_candidates(&prev, &curr, &test_config());
        let candidates = match outcome {
            AkazeCandidateOutcome::Candidates(candidates) => candidates,
            other => panic!("expected AKAZE candidates, got {other:?}"),
        };

        let candidate = candidates.first().expect("one candidate");
        assert_eq!(candidate.method, MatchMethod::Akaze);
        assert!((candidate.dx - 38).abs() <= 3, "dx = {}", candidate.dx);
        assert!((candidate.dy - 62).abs() <= 3, "dy = {}", candidate.dy);
        assert!(candidate.raw_matches.unwrap_or(0) >= 8);
        assert!(candidate.inliers.unwrap_or(0) >= 6);
    }

    #[test]
    fn solid_frames_report_not_enough_features() {
        let prev = RgbaImage::from_pixel(220, 220, Rgba([250, 250, 250, 255]));
        let curr = RgbaImage::from_pixel(220, 220, Rgba([250, 250, 250, 255]));

        let outcome = akaze_candidates(&prev, &curr, &test_config());

        assert!(matches!(
            outcome,
            AkazeCandidateOutcome::NotEnoughFeatures { .. }
        ));
    }

    #[test]
    fn akaze_score_passes_default_accept_confidence() {
        use crate::types::StitchConfig;

        let accept = StitchConfig::default().accept_confidence;

        // Floor case: minimum allowed inlier ratio + worst-case residual.
        let floor = super::akaze_score(0.35, 4.0);
        assert!(
            floor <= accept,
            "floor {floor} exceeds accept_confidence {accept}"
        );

        // Median quality: realistic AKAZE outcome should score well below the gate.
        let mid = super::akaze_score(0.6, 1.0);
        assert!(mid < floor, "mid {mid} >= floor {floor}");

        // Top quality: near-perfect AKAZE result scores near zero.
        let top = super::akaze_score(0.9, 0.0);
        assert!(top < mid, "top {top} >= mid {mid}");
        assert!(top <= 0.01, "top {top} > 0.01");
    }
}
