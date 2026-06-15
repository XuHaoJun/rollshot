//! Deterministic visual step detector. Streaming state machine over downsampled
//! luma frames: detects frame-to-frame motion, waits for a stable settle, and
//! emits a candidate only when the settled state differs meaningfully from the
//! rolling baseline. Cursor-only motion stays below the area threshold and
//! animation that returns to baseline never produces a new stable state, so
//! neither creates a step. Event-aware classification is added in the next
//! task; this core emits `UiChanged` / `VisualChange`.

use crate::frame_store::AnalysisFrame;
use crate::metrics::{changed_area_ratio, masked_luma_diff, LumaPlane};
use crate::models::{CandidateKind, DetectReason, FrameId, Millis};

#[derive(Debug, Clone)]
pub struct DetectorConfig {
    /// Normalized luma diff above which two frames are "different".
    pub diff_threshold: f32,
    /// Changed-area ratio above which a difference is "meaningful".
    pub area_threshold: f32,
    /// Per-sample luma delta (0..255) counted as a changed sample.
    pub per_sample_threshold: f32,
    /// Minimum ms between successive candidates (debounce).
    pub cooldown_ms: Millis,
    /// Window after a click in which a settle is attributed to the click.
    pub click_window_ms: Millis,
    /// Idle gap that ends a typing burst.
    pub typing_pause_ms: Millis,
    /// Dwell after scroll input before a scroll candidate may form.
    pub scroll_dwell_ms: Millis,
    /// Consecutive low-diff frames required to call the view "settled".
    pub stable_frames: u32,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            diff_threshold: 0.012,
            area_threshold: 0.04,
            per_sample_threshold: 12.0,
            cooldown_ms: 400,
            click_window_ms: 600,
            typing_pause_ms: 700,
            scroll_dwell_ms: 600,
            stable_frames: 2,
        }
    }
}

/// A detected candidate, centered on the settled keyframe. Carries no
/// coordinates, key values, or text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateMarker {
    pub kind: CandidateKind,
    pub reason: DetectReason,
    pub at_ms: Millis,
    pub center_id: FrameId,
}

pub struct Detector {
    config: DetectorConfig,
    prev: Option<LumaPlane>,
    baseline: Option<LumaPlane>,
    moving: bool,
    stable_count: u32,
    saw_change: bool,
    last_candidate_ms: Option<Millis>,
    last_frame: Option<(FrameId, Millis)>,
}

impl Detector {
    pub fn new(config: DetectorConfig) -> Self {
        Self {
            config,
            prev: None,
            baseline: None,
            moving: false,
            stable_count: 0,
            saw_change: false,
            last_candidate_ms: None,
            last_frame: None,
        }
    }

    fn cooldown_ok(&self, at_ms: Millis) -> bool {
        match self.last_candidate_ms {
            Some(prev) => at_ms.saturating_sub(prev) >= self.config.cooldown_ms,
            None => true,
        }
    }

    /// True if `luma` differs meaningfully (diff + area) from the rolling
    /// baseline.
    fn meaningful_vs_baseline(&self, luma: &LumaPlane) -> bool {
        match &self.baseline {
            Some(b) => {
                masked_luma_diff(b, luma, None) > self.config.diff_threshold
                    && changed_area_ratio(b, luma, None, self.config.per_sample_threshold)
                        > self.config.area_threshold
            }
            None => true,
        }
    }

    /// Observe one analysis frame; returns a candidate if one settles here.
    pub fn observe_frame(&mut self, frame: &AnalysisFrame) -> Option<CandidateMarker> {
        let luma = &frame.luma;
        self.last_frame = Some((frame.id, frame.at_ms));

        // Initialize the baseline on the first frame.
        if self.baseline.is_none() {
            self.baseline = Some(luma.clone());
            self.prev = Some(luma.clone());
            return None;
        }

        let changed = match &self.prev {
            Some(prev) => {
                masked_luma_diff(prev, luma, None) > self.config.diff_threshold
                    && changed_area_ratio(prev, luma, None, self.config.per_sample_threshold)
                        > self.config.area_threshold
            }
            None => false,
        };

        let mut marker = None;

        if changed {
            self.moving = true;
            self.saw_change = true;
            self.stable_count = 0;
        } else if self.moving {
            self.stable_count += 1;
            if self.stable_count >= self.config.stable_frames {
                // Settled. Emit only if meaningfully different from baseline.
                if self.meaningful_vs_baseline(luma)
                    && self.saw_change
                    && self.cooldown_ok(frame.at_ms)
                {
                    self.last_candidate_ms = Some(frame.at_ms);
                    marker = Some(CandidateMarker {
                        kind: CandidateKind::UiChanged,
                        reason: DetectReason::VisualChange,
                        at_ms: frame.at_ms,
                        center_id: frame.id,
                    });
                }
                self.moving = false;
                self.saw_change = false;
                self.baseline = Some(luma.clone());
            }
        }

        self.prev = Some(luma.clone());
        marker
    }

    /// Flush a final candidate if recording ends mid-change on a state that
    /// still differs from baseline.
    pub fn finish(&mut self) -> Option<CandidateMarker> {
        if self.moving && self.saw_change {
            let (Some(luma), Some((id, at))) = (self.prev.clone(), self.last_frame) else {
                return None;
            };
            if self.meaningful_vs_baseline(&luma) && self.cooldown_ok(at) {
                self.moving = false;
                self.saw_change = false;
                self.last_candidate_ms = Some(at);
                return Some(CandidateMarker {
                    kind: CandidateKind::UiChanged,
                    reason: DetectReason::VisualChange,
                    at_ms: at,
                    center_id: id,
                });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_store::AnalysisFrame;
    use crate::metrics::LumaPlane;
    use crate::models::{CandidateKind, DetectReason};

    fn cfg() -> DetectorConfig {
        DetectorConfig {
            diff_threshold: 0.01,
            area_threshold: 0.05,
            per_sample_threshold: 12.0,
            cooldown_ms: 0,
            click_window_ms: 600,
            typing_pause_ms: 700,
            scroll_dwell_ms: 600,
            stable_frames: 2,
        }
    }

    fn uniform(v: f32) -> LumaPlane {
        LumaPlane {
            width: 8,
            height: 8,
            samples: vec![v; 64],
        }
    }
    fn quadrant(base: f32, q: f32) -> LumaPlane {
        let mut s = vec![base; 64];
        for y in 0..4 {
            for x in 0..4 {
                s[y * 8 + x] = q;
            }
        }
        LumaPlane {
            width: 8,
            height: 8,
            samples: s,
        }
    }
    fn one_pixel(base: f32, p: f32) -> LumaPlane {
        let mut s = vec![base; 64];
        s[0] = p;
        LumaPlane {
            width: 8,
            height: 8,
            samples: s,
        }
    }
    fn af(id: u64, at: u64, luma: LumaPlane) -> AnalysisFrame {
        AnalysisFrame {
            id,
            at_ms: at,
            luma,
        }
    }

    /// Feed frames, collect every emitted marker (does not call finish()).
    fn run(det: &mut Detector, frames: Vec<AnalysisFrame>) -> Vec<CandidateMarker> {
        frames.iter().filter_map(|f| det.observe_frame(f)).collect()
    }

    #[test]
    fn change_then_settle_emits_one_ui_changed_candidate() {
        let mut det = Detector::new(cfg());
        let frames = vec![
            af(0, 0, uniform(0.0)),           // baseline
            af(1, 100, quadrant(0.0, 255.0)), // change begins (moving)
            af(2, 200, quadrant(0.0, 255.0)), // stable 1
            af(3, 300, quadrant(0.0, 255.0)), // stable 2 -> settle -> candidate
        ];
        let markers = run(&mut det, frames);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].kind, CandidateKind::UiChanged);
        assert_eq!(markers[0].reason, DetectReason::VisualChange);
        assert_eq!(markers[0].center_id, 3);
        assert_eq!(markers[0].at_ms, 300);
    }

    #[test]
    fn identical_frames_emit_no_candidate() {
        let mut det = Detector::new(cfg());
        let frames = (0..6u64).map(|i| af(i, i * 100, uniform(20.0))).collect();
        assert!(run(&mut det, frames).is_empty());
    }

    #[test]
    fn tiny_localized_change_is_below_area_threshold_and_emits_nothing() {
        // A blinking caret / small cursor: 1 of 64 samples flips each frame.
        let mut det = Detector::new(cfg());
        let frames = vec![
            af(0, 0, uniform(0.0)),
            af(1, 100, one_pixel(0.0, 255.0)),
            af(2, 200, uniform(0.0)),
            af(3, 300, one_pixel(0.0, 255.0)),
            af(4, 400, uniform(0.0)),
        ];
        assert!(run(&mut det, frames).is_empty());
    }

    #[test]
    fn oscillation_returning_to_baseline_emits_nothing_even_on_finish() {
        // Spinner-like A<->B that never settles and ends back at baseline A.
        let mut det = Detector::new(cfg());
        let frames = vec![
            af(0, 0, uniform(0.0)),           // A baseline
            af(1, 100, quadrant(0.0, 255.0)), // B
            af(2, 200, uniform(0.0)),         // A
            af(3, 300, quadrant(0.0, 255.0)), // B
            af(4, 400, uniform(0.0)),         // A (ends on baseline)
        ];
        let mut markers = run(&mut det, frames);
        if let Some(m) = det.finish() {
            markers.push(m);
        }
        assert!(
            markers.is_empty(),
            "oscillation back to baseline is not a step"
        );
    }
}
