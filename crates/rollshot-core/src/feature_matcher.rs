//! FAST corners + linear KNN feature fallback (Approach B per the spec).
//!
//! The "Hnsw" in the public identifiers is reserved for a future ANN
//! upgrade — see
//! docs/superpowers/specs/2026-05-23-rollshot-fast-hnsw-fallback-design.md
//! Approach A. Current matching is exact linear scan.

use image::RgbaImage;

use crate::akaze_matcher::{akaze_candidates, AkazeCandidateOutcome};
use crate::types::{
    FastHnswConfig, MotionCandidate, NoMatchReason, ScrollAxis, StitchConfig,
};

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
}
