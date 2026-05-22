//! Public v0.2 stitching types.
//!
//! `dx` and `dy` describe the current frame's top-left position relative to
//! the previous accepted frame in content coordinates:
//! - `dy > 0` means current frame sees lower content -> append `Bottom`.
//! - `dy < 0` means current frame sees higher content -> append `Top`.
//! - `dx > 0` means current frame sees rightward content -> append `Right`.
//! - `dx < 0` means current frame sees leftward content -> append `Left`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAxis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendDirection {
    Bottom,
    Top,
    Right,
    Left,
}

impl AppendDirection {
    pub fn axis(self) -> ScrollAxis {
        match self {
            AppendDirection::Bottom | AppendDirection::Top => ScrollAxis::Vertical,
            AppendDirection::Right | AppendDirection::Left => ScrollAxis::Horizontal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMethod {
    Template,
    Coarse,
    Edge,
    Akaze,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchStrategy {
    AutoHybrid,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionCandidate {
    pub dx: i32,
    pub dy: i32,
    pub method: MatchMethod,
    pub score: f32,
    pub second_best_score: Option<f32>,
    pub inliers: Option<usize>,
    pub raw_matches: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlapRegion {
    pub prev_x: u32,
    pub prev_y: u32,
    pub curr_x: u32,
    pub curr_y: u32,
    pub width: u32,
    pub height: u32,
}

impl OverlapRegion {
    pub fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionEstimate {
    pub dx: i32,
    pub dy: i32,
    pub axis: ScrollAxis,
    pub direction: AppendDirection,
    pub confidence: f32,
    pub method: MatchMethod,
    pub overlap: OverlapRegion,
    pub inliers: Option<usize>,
    pub raw_matches: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoMatchReason {
    LowConfidence,
    AmbiguousAxis,
    /// The candidate's cross-axis movement exceeded `max_cross_axis_px` while a
    /// scroll axis was already locked. Plan 1 cannot reach this through real
    /// frames (matcher always returns `dx = 0`); Plan 2's horizontal matching
    /// exercises it.
    CrossAxisTooLarge,
    InsufficientOverlap,
    OverlapVerificationFailed,
    NotEnoughFeatures,
    MotionTooSmall,
    DimensionMismatch,
    AkazeDisabled,
    AkazeLowInliers,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StitchOutcome {
    FirstFrame,
    Appended {
        direction: AppendDirection,
        added: u32,
        estimate: MotionEstimate,
    },
    NoProgress {
        estimate: Option<MotionEstimate>,
    },
    Duplicate,
    NoMatch {
        reason: NoMatchReason,
        best_estimate: Option<MotionEstimate>,
    },
    AxisChanged {
        previous_axis: ScrollAxis,
        new_axis: ScrollAxis,
        estimate: MotionEstimate,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StitchStats {
    pub frame_count: u32,
    pub total_height: u32,
    pub total_width: u32,
    pub last_append: u32,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct VerifierConfig {
    /// Maximum mean absolute difference of the downsampled overlap, normalized to [0, 1].
    pub downsample_max_mad: f32,
    /// Maximum mean absolute difference of the full-resolution sample band, normalized to [0, 1].
    pub full_res_max_mad: f32,
    /// Linear downsample step used for the cheap overlap pass.
    pub downsample_step: u32,
    /// Height (or width, for horizontal motion) of the full-resolution sample band, in pixels.
    pub sample_band: u32,
}

impl Default for VerifierConfig {
    fn default() -> Self {
        Self {
            downsample_max_mad: 24.0 / 255.0,
            full_res_max_mad: 18.0 / 255.0,
            downsample_step: 4,
            sample_band: 160,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct AkazeConfig {
    pub enabled: bool,
    pub max_features: usize,
    pub detector_threshold: f64,
    pub min_raw_matches: usize,
    pub min_inliers: usize,
    pub min_inlier_ratio: f32,
}

impl Default for AkazeConfig {
    fn default() -> Self {
        Self {
            enabled: cfg!(feature = "akaze"),
            max_features: 1200,
            detector_threshold: 0.001,
            min_raw_matches: 24,
            min_inliers: 16,
            min_inlier_ratio: 0.35,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct StitchConfig {
    pub strategy: MatchStrategy,
    pub min_overlap: u32,
    pub min_append: u32,
    pub duplicate_threshold: f32,
    pub accept_confidence: f32,
    pub axis_ratio_threshold: f32,
    pub max_cross_axis_px: i32,
    pub second_best_margin: f32,
    pub max_search_ratio: f32,
    pub match_width: u32,
    pub akaze: AkazeConfig,
    pub verifier: VerifierConfig,
}

impl Default for StitchConfig {
    fn default() -> Self {
        Self {
            strategy: MatchStrategy::AutoHybrid,
            min_overlap: 64,
            min_append: 8,
            duplicate_threshold: 0.01,
            accept_confidence: 0.15,
            axis_ratio_threshold: 1.5,
            max_cross_axis_px: 6,
            second_best_margin: 0.001,
            max_search_ratio: 0.75,
            match_width: 512,
            akaze: AkazeConfig::default(),
            verifier: VerifierConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_picks_auto_hybrid() {
        let cfg = StitchConfig::default();
        assert_eq!(cfg.strategy, MatchStrategy::AutoHybrid);
        assert_eq!(cfg.min_overlap, 64);
        assert_eq!(cfg.axis_ratio_threshold, 1.5);
        assert_eq!(cfg.max_cross_axis_px, 6);
        assert_eq!(cfg.verifier.downsample_step, 4);
    }

    #[test]
    fn append_direction_axis_mapping() {
        assert_eq!(AppendDirection::Bottom.axis(), ScrollAxis::Vertical);
        assert_eq!(AppendDirection::Top.axis(), ScrollAxis::Vertical);
        assert_eq!(AppendDirection::Right.axis(), ScrollAxis::Horizontal);
        assert_eq!(AppendDirection::Left.axis(), ScrollAxis::Horizontal);
    }

    #[test]
    fn overlap_region_area_is_width_times_height() {
        let r = OverlapRegion {
            prev_x: 0,
            prev_y: 10,
            curr_x: 0,
            curr_y: 0,
            width: 100,
            height: 50,
        };
        assert_eq!(r.area(), 5000);
    }

    #[test]
    fn stitch_outcome_variants_are_distinct() {
        let dummy = MotionEstimate {
            dx: 0,
            dy: 12,
            axis: ScrollAxis::Vertical,
            direction: AppendDirection::Bottom,
            confidence: 0.05,
            method: MatchMethod::Template,
            overlap: OverlapRegion {
                prev_x: 0,
                prev_y: 12,
                curr_x: 0,
                curr_y: 0,
                width: 100,
                height: 88,
            },
            inliers: None,
            raw_matches: None,
        };
        let appended = StitchOutcome::Appended {
            direction: AppendDirection::Bottom,
            added: 12,
            estimate: dummy,
        };
        let no_match = StitchOutcome::NoMatch {
            reason: NoMatchReason::LowConfidence,
            best_estimate: Some(dummy),
        };
        let no_progress = StitchOutcome::NoProgress { estimate: None };
        assert_ne!(appended, StitchOutcome::FirstFrame);
        assert_ne!(no_match, no_progress);
        assert_ne!(no_match, StitchOutcome::Duplicate);
    }

    #[test]
    fn akaze_defaults_follow_compile_feature() {
        let cfg = StitchConfig::default();

        #[cfg(feature = "akaze")]
        assert!(cfg.akaze.enabled);

        #[cfg(not(feature = "akaze"))]
        assert!(!cfg.akaze.enabled);

        assert_eq!(cfg.akaze.max_features, 1200);
        assert_eq!(cfg.akaze.detector_threshold, 0.001);
        assert_eq!(cfg.akaze.min_raw_matches, 24);
        assert_eq!(cfg.akaze.min_inliers, 16);
        assert_eq!(cfg.akaze.min_inlier_ratio, 0.35);
    }
}
