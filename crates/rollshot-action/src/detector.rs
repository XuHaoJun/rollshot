//! Deterministic visual step detector. Streaming state machine over downsampled
//! luma frames: detects frame-to-frame motion, waits for a stable settle, and
//! emits a candidate only when the settled state differs meaningfully from the
//! rolling baseline. Cursor-only motion stays below the area threshold and
//! animation that returns to baseline never produces a new stable state, so
//! neither creates a step. Semantic events (click, typing, scroll) open
//! sessions that classify the settle into Click, Typing, Scroll, or
//! UiChanged.

use crate::diagnostics::TARGET_DETECTOR;
use crate::frame_store::AnalysisFrame;
use crate::metrics::{changed_area_ratio, masked_luma_diff, LumaPlane};
use crate::models::{
    CandidateKind, DetectReason, FrameId, Millis, SemanticAction, TimedSemanticAction,
};

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

/// Frame-to-frame motion test: meaningful diff AND meaningful changed area.
fn motion(a: &LumaPlane, b: &LumaPlane, config: &DetectorConfig) -> bool {
    masked_luma_diff(a, b, None) > config.diff_threshold
        && changed_area_ratio(a, b, None, config.per_sample_threshold) > config.area_threshold
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
    // event sessions
    click_open_until: Option<Millis>,
    in_typing: bool,
    typing_last_at: Millis,
    typing_force_end: bool,
    in_scroll: bool,
    scroll_last_at: Millis,
    pre_scroll_baseline: Option<LumaPlane>,
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
            click_open_until: None,
            in_typing: false,
            typing_last_at: 0,
            typing_force_end: false,
            in_scroll: false,
            scroll_last_at: 0,
            pre_scroll_baseline: None,
        }
    }

    fn cooldown_ok(&self, at_ms: Millis) -> bool {
        match self.last_candidate_ms {
            Some(prev) => at_ms.saturating_sub(prev) >= self.config.cooldown_ms,
            None => true,
        }
    }

    fn meaningful_vs_baseline(&self, luma: &LumaPlane) -> bool {
        match &self.baseline {
            Some(b) => motion(b, luma, &self.config),
            None => true,
        }
    }

    fn click_consume(&mut self, at_ms: Millis) -> bool {
        match self.click_open_until {
            Some(until) if at_ms <= until => {
                self.click_open_until = None;
                true
            }
            _ => false,
        }
    }

    /// Observe a privacy-filtered semantic event. Opens click windows and
    /// typing/scroll sessions; never inspects key values or text.
    pub fn observe_event(&mut self, ev: TimedSemanticAction) {
        match ev.action {
            SemanticAction::Click { .. } => {
                self.click_open_until = Some(ev.at_ms.saturating_add(self.config.click_window_ms));
            }
            SemanticAction::TypingActivity => {
                self.in_typing = true;
                self.typing_last_at = ev.at_ms;
            }
            SemanticAction::SemanticKey(_) => {
                if self.in_typing {
                    self.typing_last_at = ev.at_ms;
                    self.typing_force_end = true;
                }
            }
            SemanticAction::ScrollActivity => {
                if !self.in_scroll {
                    self.in_scroll = true;
                    self.pre_scroll_baseline = self.baseline.clone();
                }
                self.scroll_last_at = ev.at_ms;
            }
        }
    }

    pub fn observe_frame(&mut self, frame: &AnalysisFrame) -> Option<CandidateMarker> {
        let luma = &frame.luma;
        self.last_frame = Some((frame.id, frame.at_ms));

        if self.baseline.is_none() {
            self.baseline = Some(luma.clone());
            self.prev = Some(luma.clone());
            return None;
        }

        // A change in analysis-plane dimensions (e.g. a mid-recording region
        // resize) makes the masked diff fall back to "no motion", so visual
        // detection is degraded until the baseline is re-established. Surface it
        // once at the transition; re-baseline handling is owned by the app
        // integration (Plan 2).
        if let Some(prev) = &self.prev {
            if prev.width != luma.width || prev.height != luma.height {
                tracing::debug!(
                    target: TARGET_DETECTOR,
                    prev_w = prev.width,
                    prev_h = prev.height,
                    new_w = luma.width,
                    new_h = luma.height,
                    "analysis frame dimensions changed; visual diff degraded until re-baseline"
                );
            }
        }

        // --- movement bookkeeping (runs every frame) ---
        let changed = match &self.prev {
            Some(prev) => motion(prev, luma, &self.config),
            None => false,
        };
        let mut settled_this_frame = false;
        if changed {
            self.moving = true;
            self.saw_change = true;
            self.stable_count = 0;
        } else if self.moving {
            self.stable_count += 1;
            if self.stable_count >= self.config.stable_frames {
                settled_this_frame = true;
                self.moving = false;
            }
        }
        self.prev = Some(luma.clone());

        // --- candidate decision (priority: typing > scroll > generic settle) ---

        // 1. Typing burst ends on Enter/Tab or a long enough pause.
        if self.in_typing
            && (self.typing_force_end
                || frame.at_ms.saturating_sub(self.typing_last_at) >= self.config.typing_pause_ms)
        {
            self.in_typing = false;
            self.typing_force_end = false;
            self.saw_change = false;
            self.baseline = Some(luma.clone());
            if self.cooldown_ok(frame.at_ms) {
                self.last_candidate_ms = Some(frame.at_ms);
                return Some(CandidateMarker {
                    kind: CandidateKind::Typing,
                    reason: DetectReason::TypingSettled,
                    at_ms: frame.at_ms,
                    center_id: frame.id,
                });
            }
            return None;
        }

        // 2. Scroll ends after a settled dwell; compare to the pre-scroll state.
        if self.in_scroll
            && frame.at_ms.saturating_sub(self.scroll_last_at) >= self.config.scroll_dwell_ms
            && !self.moving
        {
            let meaningful = match &self.pre_scroll_baseline {
                Some(b) => motion(b, luma, &self.config),
                None => self.meaningful_vs_baseline(luma),
            };
            self.in_scroll = false;
            self.saw_change = false;
            self.baseline = Some(luma.clone());
            if meaningful && self.cooldown_ok(frame.at_ms) {
                self.last_candidate_ms = Some(frame.at_ms);
                return Some(CandidateMarker {
                    kind: CandidateKind::Scroll,
                    reason: DetectReason::ScrollSettled,
                    at_ms: frame.at_ms,
                    center_id: frame.id,
                });
            }
            return None;
        }

        // 3. Generic settle. Suppressed while a typing/scroll session owns the
        // change; otherwise becomes a Click (if within a click window) or a
        // plain visual change.
        if settled_this_frame && !self.in_typing && !self.in_scroll {
            let meaningful = self.meaningful_vs_baseline(luma);
            self.baseline = Some(luma.clone());
            self.saw_change = false;
            if meaningful && self.cooldown_ok(frame.at_ms) {
                self.last_candidate_ms = Some(frame.at_ms);
                let (kind, reason) = if self.click_consume(frame.at_ms) {
                    (CandidateKind::Click, DetectReason::ClickConfirmed)
                } else {
                    (CandidateKind::UiChanged, DetectReason::VisualChange)
                };
                return Some(CandidateMarker {
                    kind,
                    reason,
                    at_ms: frame.at_ms,
                    center_id: frame.id,
                });
            }
            return None;
        }

        // Settle suppressed by an open session: still advance the baseline so
        // the session end compares against the latest stable state.
        if settled_this_frame {
            self.baseline = Some(luma.clone());
            self.saw_change = false;
        }

        None
    }

    pub fn finish(&mut self) -> Option<CandidateMarker> {
        // An open typing burst closes into one step at recording finish.
        if self.in_typing {
            self.in_typing = false;
            self.typing_force_end = false;
            let (id, at) = self.last_frame?;
            if self.cooldown_ok(at) {
                self.last_candidate_ms = Some(at);
                return Some(CandidateMarker {
                    kind: CandidateKind::Typing,
                    reason: DetectReason::TypingSettled,
                    at_ms: at,
                    center_id: id,
                });
            }
            return None;
        }
        // An open scroll session closes into one step at recording finish, even
        // if its settle-dwell never elapsed, when the final state differs
        // meaningfully from the pre-scroll baseline. Typing takes priority above
        // when both are open, mirroring `observe_frame`.
        if self.in_scroll {
            self.in_scroll = false;
            self.saw_change = false;
            let (Some(luma), Some((id, at))) = (self.prev.clone(), self.last_frame) else {
                return None;
            };
            let meaningful = match &self.pre_scroll_baseline {
                Some(b) => motion(b, &luma, &self.config),
                None => self.meaningful_vs_baseline(&luma),
            };
            if meaningful && self.cooldown_ok(at) {
                self.last_candidate_ms = Some(at);
                return Some(CandidateMarker {
                    kind: CandidateKind::Scroll,
                    reason: DetectReason::ScrollSettled,
                    at_ms: at,
                    center_id: id,
                });
            }
            return None;
        }
        // A visual change still in progress flushes if it differs from baseline.
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
    use crate::models::{
        CandidateKind, DetectReason, MouseButton, SemanticAction, SemanticKey, TimedSemanticAction,
    };

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

    fn ev(action: SemanticAction, at: u64) -> TimedSemanticAction {
        TimedSemanticAction { action, at_ms: at }
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

    #[test]
    fn click_then_visual_settle_is_a_confirmed_click() {
        let mut det = Detector::new(cfg());
        det.observe_frame(&af(0, 0, uniform(0.0)));
        det.observe_event(ev(
            SemanticAction::Click {
                button: MouseButton::Left,
                position: None,
            },
            100,
        ));
        let frames = [
            af(1, 150, quadrant(0.0, 255.0)),
            af(2, 250, quadrant(0.0, 255.0)),
            af(3, 350, quadrant(0.0, 255.0)), // settle within click window [100, 700]
        ];
        let markers: Vec<_> = frames.iter().filter_map(|f| det.observe_frame(f)).collect();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].kind, CandidateKind::Click);
        assert_eq!(markers[0].reason, DetectReason::ClickConfirmed);
    }

    #[test]
    fn click_without_visual_change_is_not_a_step() {
        let mut det = Detector::new(cfg());
        det.observe_frame(&af(0, 0, uniform(0.0)));
        det.observe_event(ev(
            SemanticAction::Click {
                button: MouseButton::Left,
                position: None,
            },
            100,
        ));
        let frames = [af(1, 150, uniform(0.0)), af(2, 250, uniform(0.0))];
        let markers: Vec<_> = frames.iter().filter_map(|f| det.observe_frame(f)).collect();
        assert!(markers.is_empty());
    }

    #[test]
    fn typing_burst_merges_into_one_step_ending_on_pause() {
        let mut det = Detector::new(cfg());
        det.observe_event(ev(SemanticAction::TypingActivity, 0));
        det.observe_frame(&af(0, 0, uniform(0.0)));
        det.observe_event(ev(SemanticAction::TypingActivity, 100));
        det.observe_frame(&af(1, 100, quadrant(0.0, 255.0)));
        det.observe_event(ev(SemanticAction::TypingActivity, 200));
        let mut markers = Vec::new();
        for f in [
            af(2, 200, quadrant(0.0, 255.0)),
            af(3, 300, quadrant(0.0, 255.0)), // settle, suppressed (in typing)
            af(4, 1000, quadrant(0.0, 255.0)), // pause >= 700ms from last typing -> Typing step
        ] {
            if let Some(m) = det.observe_frame(&f) {
                markers.push(m);
            }
        }
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].kind, CandidateKind::Typing);
        assert_eq!(markers[0].reason, DetectReason::TypingSettled);
    }

    #[test]
    fn enter_ends_a_typing_burst() {
        let mut det = Detector::new(cfg());
        det.observe_event(ev(SemanticAction::TypingActivity, 0));
        det.observe_frame(&af(0, 0, uniform(0.0)));
        det.observe_event(ev(SemanticAction::TypingActivity, 50));
        det.observe_frame(&af(1, 50, quadrant(0.0, 255.0)));
        det.observe_event(ev(SemanticAction::SemanticKey(SemanticKey::Enter), 60));
        let m = det.observe_frame(&af(2, 100, quadrant(0.0, 255.0)));
        assert_eq!(m.map(|m| m.kind), Some(CandidateKind::Typing));
    }

    #[test]
    fn scroll_emits_one_step_only_after_settle_with_meaningful_change() {
        let mut det = Detector::new(cfg());
        det.observe_frame(&af(0, 0, uniform(0.0))); // pre-scroll baseline A
        det.observe_event(ev(SemanticAction::ScrollActivity, 100));
        det.observe_frame(&af(1, 100, quadrant(0.0, 100.0))); // moving
        det.observe_event(ev(SemanticAction::ScrollActivity, 200));
        det.observe_frame(&af(2, 200, quadrant(0.0, 200.0))); // moving
        det.observe_event(ev(SemanticAction::ScrollActivity, 300));
        let mut markers = Vec::new();
        for f in [
            af(3, 300, quadrant(0.0, 255.0)),  // moving
            af(4, 400, quadrant(0.0, 255.0)),  // stable 1
            af(5, 500, quadrant(0.0, 255.0)),  // stable 2 -> settle, suppressed (in scroll)
            af(6, 1000, quadrant(0.0, 255.0)), // dwell >= 600ms past last scroll -> Scroll step
        ] {
            if let Some(m) = det.observe_frame(&f) {
                markers.push(m);
            }
        }
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].kind, CandidateKind::Scroll);
        assert_eq!(markers[0].reason, DetectReason::ScrollSettled);
    }

    #[test]
    fn drag_collapses_to_one_step_at_the_stable_end_state() {
        let mut det = Detector::new(cfg());
        det.observe_frame(&af(0, 0, uniform(0.0)));
        det.observe_event(ev(
            SemanticAction::Click {
                button: MouseButton::Left,
                position: None,
            },
            50,
        ));
        let mut markers = Vec::new();
        for f in [
            af(1, 100, quadrant(0.0, 50.0)), // drag motion
            af(2, 200, quadrant(0.0, 100.0)),
            af(3, 300, quadrant(0.0, 150.0)),
            af(4, 400, quadrant(0.0, 200.0)),
            af(5, 500, quadrant(0.0, 200.0)), // stable 1
            af(6, 600, quadrant(0.0, 200.0)), // stable 2 -> settle (within click window) -> one step
        ] {
            if let Some(m) = det.observe_frame(&f) {
                markers.push(m);
            }
        }
        assert_eq!(markers.len(), 1, "drag must not create intermediate steps");
        assert_eq!(markers[0].kind, CandidateKind::Click);
        assert_eq!(markers[0].center_id, 6);
    }

    #[test]
    fn tab_ends_a_typing_burst() {
        let mut det = Detector::new(cfg());
        det.observe_event(ev(SemanticAction::TypingActivity, 0));
        det.observe_frame(&af(0, 0, uniform(0.0)));
        det.observe_event(ev(SemanticAction::TypingActivity, 50));
        det.observe_frame(&af(1, 50, quadrant(0.0, 255.0)));
        det.observe_event(ev(SemanticAction::SemanticKey(SemanticKey::Tab), 60));
        let m = det.observe_frame(&af(2, 100, quadrant(0.0, 255.0)));
        assert_eq!(m.map(|m| m.kind), Some(CandidateKind::Typing));
    }

    #[test]
    fn typing_burst_closes_on_finish_when_no_pause_occurs() {
        let mut det = Detector::new(cfg());
        det.observe_event(ev(SemanticAction::TypingActivity, 0));
        det.observe_frame(&af(0, 0, uniform(0.0)));
        det.observe_event(ev(SemanticAction::TypingActivity, 100));
        det.observe_frame(&af(1, 100, quadrant(0.0, 255.0)));
        // No terminating pause / Enter / Tab; recording ends -> finish flushes Typing.
        let m = det.finish();
        assert_eq!(m.map(|m| m.kind), Some(CandidateKind::Typing));
    }

    #[test]
    fn scroll_burst_closes_on_finish_when_dwell_does_not_elapse() {
        // User scrolls to a new state, then ends recording before scroll_dwell_ms
        // elapses past the last scroll event: finish() must still flush one Scroll
        // step (not a generic UiChanged) when the final state differs from the
        // pre-scroll baseline.
        let mut det = Detector::new(cfg());
        det.observe_frame(&af(0, 0, uniform(0.0))); // pre-scroll baseline A
        det.observe_event(ev(SemanticAction::ScrollActivity, 100));
        det.observe_frame(&af(1, 100, quadrant(0.0, 100.0))); // moving
        det.observe_event(ev(SemanticAction::ScrollActivity, 200));
        det.observe_frame(&af(2, 200, quadrant(0.0, 200.0))); // moving
        det.observe_event(ev(SemanticAction::ScrollActivity, 300));
        det.observe_frame(&af(3, 300, quadrant(0.0, 255.0))); // moving
        det.observe_frame(&af(4, 400, quadrant(0.0, 255.0))); // dwell not elapsed (400-300 < 600)
        let m = det.finish();
        assert_eq!(m.map(|m| m.kind), Some(CandidateKind::Scroll));
    }

    #[test]
    fn scroll_returning_to_baseline_emits_nothing_on_finish() {
        // Scrolling that ends back at the pre-scroll state is not a step.
        let mut det = Detector::new(cfg());
        det.observe_frame(&af(0, 0, uniform(0.0))); // pre-scroll baseline A
        det.observe_event(ev(SemanticAction::ScrollActivity, 100));
        det.observe_frame(&af(1, 100, quadrant(0.0, 255.0))); // moving (B)
        det.observe_event(ev(SemanticAction::ScrollActivity, 200));
        det.observe_frame(&af(2, 200, uniform(0.0))); // back to A
        assert!(det.finish().is_none());
    }
}
