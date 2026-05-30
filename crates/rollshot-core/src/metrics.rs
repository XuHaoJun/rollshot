//! Per-frame instrumentation for the stitching pipeline.
//!
//! `StitchMetrics` is populated by `Stitcher::push_frame` and exposed via
//! `Stitcher::last_metrics()`. Stage timings use `ScopedTimer`, which adds
//! elapsed microseconds into a `&mut u64` on drop — so early returns and `?`
//! propagation still record the time spent in that stage, and a stage that
//! runs more than once per frame (e.g. the verifier across the primary,
//! relaxed, and fallback passes) accumulates rather than overwrites.
//!
//! Instrumentation is always on (no feature flag). Cost per `push_frame` is on
//! the order of ten `Instant::now()` calls (~100 ns total), well under 1% of
//! any realistic frame.

use std::time::Instant;

use crate::types::{MatchMethod, NoMatchReason, StitchOutcome};

#[derive(Debug, Clone, Default)]
pub struct StitchMetrics {
    pub frame_index: usize,
    pub outcome: StitchOutcomeKind,
    pub no_match_reason: Option<NoMatchReason>,
    pub total_us: u64,

    // Per-stage timings (µs). 0 if the stage was skipped (e.g. a duplicate frame
    // skips the matcher and append).
    pub duplicate_us: u64,
    pub prepare_frame_us: u64,
    pub coarse_us: u64,
    pub pyramid_us: u64,
    pub template_ncc_us: u64,
    pub edge_projection_us: u64,
    pub verifier_us: u64,
    pub fallback_us: u64,
    pub append_us: u64,

    // Algorithmic counters (CPU-independent).
    pub coarse_candidates: usize,
    pub pyramid_candidates: usize,
    pub ncc_offsets_scored: usize,
    pub ncc_pixel_visits: usize,
    pub verifier_candidates: usize,
    pub fallback_features_extracted: usize,

    // Canvas state after this frame.
    pub canvas_logical_pixels: u64,
    pub canvas_allocated_bytes: u64,
    pub append_copied_bytes: u64,

    // Motion outcome.
    pub best_dx: i32,
    pub best_dy: i32,
    pub best_score: f32,
    pub second_best_score: Option<f32>,
    pub match_method: Option<MatchMethod>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StitchOutcomeKind {
    #[default]
    None,
    FirstFrame,
    Appended,
    Duplicate,
    NoMatch,
    NoProgress,
    AxisChanged,
}

impl StitchMetrics {
    /// Marks this frame as a `NoMatch` outcome with the given reason.
    pub(crate) fn set_no_match(&mut self, reason: NoMatchReason) {
        self.outcome = StitchOutcomeKind::NoMatch;
        self.no_match_reason = Some(reason);
    }
}

impl From<&StitchOutcome> for StitchOutcomeKind {
    fn from(outcome: &StitchOutcome) -> Self {
        match outcome {
            StitchOutcome::FirstFrame => Self::FirstFrame,
            StitchOutcome::Appended { .. } => Self::Appended,
            StitchOutcome::Duplicate => Self::Duplicate,
            StitchOutcome::NoMatch { .. } => Self::NoMatch,
            StitchOutcome::NoProgress { .. } => Self::NoProgress,
            StitchOutcome::AxisChanged { .. } => Self::AxisChanged,
        }
    }
}

/// Adds elapsed microseconds to a target field on drop.
///
/// Use one per stage inside `push_frame` / `estimate_motion`. The drop-based
/// design means `?` propagation and early returns still record the time spent
/// in the stage. Because it accumulates (`+=`) rather than overwrites, a stage
/// timed more than once in a single frame sums correctly.
pub(crate) struct ScopedTimer<'a> {
    start: Instant,
    target: &'a mut u64,
}

impl<'a> ScopedTimer<'a> {
    pub fn new(target: &'a mut u64) -> Self {
        Self {
            start: Instant::now(),
            target,
        }
    }
}

impl Drop for ScopedTimer<'_> {
    fn drop(&mut self) {
        *self.target = self
            .target
            .saturating_add(self.start.elapsed().as_micros() as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn scoped_timer_writes_on_drop() {
        let mut target = 0u64;
        {
            let _t = ScopedTimer::new(&mut target);
            thread::sleep(Duration::from_millis(2));
        }
        assert!(target >= 1_000, "expected >=1000 µs, got {target}");
    }

    #[test]
    fn scoped_timer_accumulates_across_drops() {
        let mut target = 0u64;
        for _ in 0..3 {
            let _t = ScopedTimer::new(&mut target);
            thread::sleep(Duration::from_millis(1));
        }
        // Three ~1ms stints add up rather than overwriting; the last drop
        // alone would be ~1000 µs, so anything well above that proves the +=.
        assert!(
            target >= 1_500,
            "expected accumulated total >= 1500 µs across 3 timers, got {target}"
        );
    }

    #[test]
    fn scoped_timer_writes_on_early_return() {
        fn inner(target: &mut u64) -> Result<(), ()> {
            let _t = ScopedTimer::new(target);
            thread::sleep(Duration::from_millis(2));
            Err(())
        }
        let mut t = 0u64;
        let _ = inner(&mut t);
        assert!(t > 0, "early return should still record elapsed time");
    }

    #[test]
    fn metrics_default_is_zero() {
        let m = StitchMetrics::default();
        assert_eq!(m.total_us, 0);
        assert_eq!(m.outcome, StitchOutcomeKind::None);
        assert!(m.no_match_reason.is_none());
        assert_eq!(m.coarse_us, 0);
        assert_eq!(m.pyramid_us, 0);
        assert_eq!(m.pyramid_candidates, 0);
        assert_eq!(m.append_us, 0);
    }

    #[test]
    fn outcome_kind_from_stitch_outcome_first_frame() {
        let kind: StitchOutcomeKind = (&StitchOutcome::FirstFrame).into();
        assert_eq!(kind, StitchOutcomeKind::FirstFrame);
    }

    #[test]
    fn outcome_kind_from_stitch_outcome_duplicate() {
        let kind: StitchOutcomeKind = (&StitchOutcome::Duplicate).into();
        assert_eq!(kind, StitchOutcomeKind::Duplicate);
    }
}
