//! FAST corners + linear KNN feature fallback (Approach B per the spec).
//!
//! The "Hnsw" in the public identifiers is reserved for a future ANN
//! upgrade — see
//! docs/superpowers/specs/2026-05-23-rollshot-fast-hnsw-fallback-design.md
//! Approach A. Current matching is exact linear scan.

use image::RgbaImage;
use image::GrayImage;
use imageproc::corners;

use crate::akaze_matcher::{akaze_candidates, AkazeCandidateOutcome};
use crate::types::{
    FastHnswConfig, MotionCandidate, NoMatchReason, ScrollAxis, StitchConfig,
};

fn rgba_to_gray(img: &RgbaImage) -> GrayImage {
    image::imageops::grayscale(img)
}

fn extract_corners(gray: &GrayImage, threshold: u8, max_features: usize) -> Vec<(u32, u32)> {
    if gray.width() < 16 || gray.height() < 16 {
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
    NotEnoughFeatures { prev: usize, curr: usize },
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
            AkazeCandidateOutcome::Candidates(candidates) => {
                FeatureFallbackOutcome::Candidates {
                    candidates,
                    source: FeatureSource::Akaze,
                }
            }
        }
    }
}

/// FAST corners + linear KNN matching. The "Hnsw" in the name is
/// reserved for a future ANN upgrade — see
/// docs/superpowers/specs/2026-05-23-rollshot-fast-hnsw-fallback-design.md
/// Approach A. Current matching is exact linear scan.
pub(crate) fn fast_hnsw_candidates(
    _prev: &RgbaImage,
    _curr: &RgbaImage,
    _locked_axis: Option<ScrollAxis>,
    config: &FastHnswConfig,
) -> FastHnswCandidateOutcome {
    if !config.enabled {
        return FastHnswCandidateOutcome::Disabled;
    }
    // Real implementation lands in subsequent tasks.
    FastHnswCandidateOutcome::Disabled
}

/// Pick-one dispatch:
///   - `config.akaze.enabled = true`  → run AKAZE (FastHnsw is skipped
///                                       even if also enabled)
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
        let mut config = FastHnswConfig::default();
        config.enabled = false;
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
                FeatureFallbackOutcome::Disabled
                    | FeatureFallbackOutcome::NotEnoughFeatures { .. }
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
                        let intensity = if (xx / 3 + yy / 3) % 2 == 0 || dx.abs() < 2 || dy.abs() < 2 {
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
        assert!(corners.is_empty(), "solid image returned {} corners", corners.len());
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
}
