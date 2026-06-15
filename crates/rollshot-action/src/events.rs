//! Privacy-safe burst coalescing for the semantic event stream. Consecutive
//! `TypingActivity` / `ScrollActivity` events within `window` ms collapse into
//! a single representative event (earliest timestamp). `Click` and Enter/Tab
//! pass through unchanged and end any in-progress activity run. The privacy
//! boundary is the `SemanticAction` shape itself — there is no field able to
//! carry typed text or raw key codes.

use crate::models::{Millis, SemanticAction, TimedSemanticAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivityKind {
    Typing,
    Scroll,
}

fn activity_kind(action: &SemanticAction) -> Option<ActivityKind> {
    match action {
        SemanticAction::TypingActivity => Some(ActivityKind::Typing),
        SemanticAction::ScrollActivity => Some(ActivityKind::Scroll),
        SemanticAction::Click { .. } | SemanticAction::SemanticKey(_) => None,
    }
}

pub struct EventAggregator {
    window: Millis,
    out: Vec<TimedSemanticAction>,
    last_kind: Option<ActivityKind>,
    last_at: Millis,
}

impl EventAggregator {
    pub fn new(coalesce_window_ms: Millis) -> Self {
        Self {
            window: coalesce_window_ms,
            out: Vec::new(),
            last_kind: None,
            last_at: 0,
        }
    }

    pub fn push(&mut self, ev: TimedSemanticAction) {
        match activity_kind(&ev.action) {
            Some(kind)
                if self.last_kind == Some(kind)
                    && ev.at_ms.saturating_sub(self.last_at) <= self.window =>
            {
                // Fold into the in-progress run; slide the window anchor.
                self.last_at = ev.at_ms;
            }
            Some(kind) => {
                self.out.push(ev);
                self.last_kind = Some(kind);
                self.last_at = ev.at_ms;
            }
            None => {
                self.out.push(ev);
                self.last_kind = None;
                self.last_at = ev.at_ms;
            }
        }
    }

    /// Take all coalesced events accumulated so far. Run state persists, so a
    /// run split across drains still coalesces.
    pub fn drain(&mut self) -> Vec<TimedSemanticAction> {
        std::mem::take(&mut self.out)
    }
}

impl Default for EventAggregator {
    fn default() -> Self {
        Self::new(120)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MouseButton, SemanticAction, SemanticKey, TimedSemanticAction};

    fn ev(action: SemanticAction, at_ms: u64) -> TimedSemanticAction {
        TimedSemanticAction { action, at_ms }
    }

    #[test]
    fn consecutive_typing_within_window_coalesces_to_one() {
        let mut agg = EventAggregator::new(120);
        for t in [0u64, 50, 100, 150, 200] {
            agg.push(ev(SemanticAction::TypingActivity, t));
        }
        let out = agg.drain();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].at_ms, 0, "keep the earliest timestamp of the run");
    }

    #[test]
    fn a_gap_larger_than_the_window_breaks_the_run() {
        let mut agg = EventAggregator::new(120);
        agg.push(ev(SemanticAction::TypingActivity, 0));
        agg.push(ev(SemanticAction::TypingActivity, 500));
        assert_eq!(agg.drain().len(), 2);
    }

    #[test]
    fn enter_breaks_a_typing_run() {
        let mut agg = EventAggregator::new(120);
        agg.push(ev(SemanticAction::TypingActivity, 0));
        agg.push(ev(SemanticAction::TypingActivity, 50));
        agg.push(ev(SemanticAction::SemanticKey(SemanticKey::Enter), 60));
        agg.push(ev(SemanticAction::TypingActivity, 70));
        let out = agg.drain();
        let kinds: Vec<_> = out.iter().map(|e| e.action).collect();
        assert_eq!(
            kinds,
            vec![
                SemanticAction::TypingActivity,
                SemanticAction::SemanticKey(SemanticKey::Enter),
                SemanticAction::TypingActivity,
            ]
        );
    }

    #[test]
    fn clicks_pass_through_and_break_runs() {
        let mut agg = EventAggregator::new(120);
        agg.push(ev(SemanticAction::TypingActivity, 0));
        agg.push(ev(
            SemanticAction::Click {
                button: MouseButton::Left,
                position: None,
            },
            10,
        ));
        agg.push(ev(SemanticAction::TypingActivity, 20));
        assert_eq!(agg.drain().len(), 3);
    }

    #[test]
    fn scroll_and_typing_runs_are_independent() {
        let mut agg = EventAggregator::new(120);
        agg.push(ev(SemanticAction::ScrollActivity, 0));
        agg.push(ev(SemanticAction::ScrollActivity, 40));
        agg.push(ev(SemanticAction::TypingActivity, 60)); // different kind -> new event
        agg.push(ev(SemanticAction::TypingActivity, 90));
        let out = agg.drain();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].action, SemanticAction::ScrollActivity);
        assert_eq!(out[1].action, SemanticAction::TypingActivity);
    }

    #[test]
    fn aggregated_events_never_carry_text_or_raw_codes() {
        let json = serde_json::to_string(&ev(SemanticAction::TypingActivity, 5)).unwrap();
        assert_eq!(json, r#"{"action":"typing-activity","at_ms":5}"#);
        for forbidden in ["text", "unicode", "keycode", "key_code", "device"] {
            assert!(!json.contains(forbidden), "leaked {forbidden}: {json}");
        }
    }
}
