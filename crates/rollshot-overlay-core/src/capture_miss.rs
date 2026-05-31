use std::time::{Duration, Instant};

use rollshot_core::{AppendDirection, StitchOutcome};

pub const CAPTURE_MISS_WARNING: &str =
    "Scrolling too fast. Scroll back to the captured edge and try again.";

/// One warning toast at most per this window (R1: single source of truth).
pub const CAPTURE_MISS_THROTTLE: Duration = Duration::from_millis(3000);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StitchProgressSignal {
    Accepted { edge: CapturedEdge },
    Missed { edge: CapturedEdge },
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapturedEdge {
    Top,
    Bottom,
    Left,
    Right,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreviewRecoveryAffordance {
    pub active: bool,
    pub edge: CapturedEdge,
    pub processing: bool,
}

/// `Default` is the inactive state: not active, not warning, `Unknown` edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CaptureMissState {
    pub active: bool,
    pub warn: bool,
    pub edge: CapturedEdge,
    pub affordance: PreviewRecoveryAffordance,
}

/// Convert a stitch append direction into the captured edge the user must
/// scroll back toward. Shared by both capture paths (R2).
pub fn captured_edge_from_direction(direction: AppendDirection) -> CapturedEdge {
    match direction {
        AppendDirection::Top => CapturedEdge::Top,
        AppendDirection::Bottom => CapturedEdge::Bottom,
        AppendDirection::Left => CapturedEdge::Left,
        AppendDirection::Right => CapturedEdge::Right,
    }
}

/// Map a `StitchOutcome` to the progress signal that drives the tracker.
/// Shared by `session.rs` (webview) and `driver.rs` (native) (R2).
pub fn progress_signal_from_outcome(outcome: &StitchOutcome) -> StitchProgressSignal {
    match outcome {
        StitchOutcome::FirstFrame => StitchProgressSignal::Accepted {
            edge: CapturedEdge::Unknown,
        },
        StitchOutcome::Appended { direction, .. } => StitchProgressSignal::Accepted {
            edge: captured_edge_from_direction(*direction),
        },
        StitchOutcome::NoMatch { best_estimate, .. } => StitchProgressSignal::Missed {
            edge: best_estimate
                .map(|estimate| captured_edge_from_direction(estimate.direction))
                .unwrap_or(CapturedEdge::Unknown),
        },
        StitchOutcome::AxisChanged { estimate, .. } => StitchProgressSignal::Missed {
            edge: captured_edge_from_direction(estimate.direction),
        },
        StitchOutcome::Duplicate | StitchOutcome::NoProgress { .. } => StitchProgressSignal::Idle,
    }
}

#[derive(Debug)]
pub struct CaptureMissTracker {
    active: bool,
    edge: CapturedEdge,
    last_warning_at: Option<Instant>,
    throttle: Duration,
}

impl Default for CaptureMissTracker {
    fn default() -> Self {
        Self::new(CAPTURE_MISS_THROTTLE)
    }
}

impl CaptureMissTracker {
    pub fn new(throttle: Duration) -> Self {
        Self {
            active: false,
            edge: CapturedEdge::Unknown,
            last_warning_at: None,
            throttle,
        }
    }

    pub fn update(&mut self, signal: StitchProgressSignal, now: Instant) -> CaptureMissState {
        match signal {
            StitchProgressSignal::Accepted { .. } => {
                self.active = false;
                self.edge = CapturedEdge::Unknown;
                // R7: forget the last warning time so a miss right after recovery
                // warns immediately rather than being throttled by the old pulse.
                self.last_warning_at = None;
                CaptureMissState::default()
            }
            StitchProgressSignal::Missed { edge } => {
                self.active = true;
                self.edge = edge;
                let warn = match self.last_warning_at {
                    Some(last) => now.duration_since(last) >= self.throttle,
                    None => true,
                };
                if warn {
                    self.last_warning_at = Some(now);
                }
                CaptureMissState {
                    active: true,
                    warn,
                    edge,
                    affordance: PreviewRecoveryAffordance {
                        active: true,
                        edge,
                        processing: false,
                    },
                }
            }
            StitchProgressSignal::Idle => self.state(),
        }
    }

    pub fn state(&self) -> CaptureMissState {
        CaptureMissState {
            active: self.active,
            warn: false,
            edge: self.edge,
            affordance: PreviewRecoveryAffordance {
                active: self.active,
                edge: self.edge,
                processing: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(ms: u64) -> Instant {
        Instant::now() + Duration::from_millis(ms)
    }

    #[test]
    fn missed_enters_active_state_and_warns() {
        let mut tracker = CaptureMissTracker::new(Duration::from_millis(3000));

        let state = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(0),
        );

        assert!(state.active);
        assert!(state.warn);
        assert_eq!(state.edge, CapturedEdge::Bottom);
        assert!(state.affordance.active);
        assert_eq!(state.affordance.edge, CapturedEdge::Bottom);
    }

    #[test]
    fn repeated_misses_are_throttled_but_stay_active() {
        let mut tracker = CaptureMissTracker::new(Duration::from_millis(3000));
        let _ = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(0),
        );

        let state = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(1000),
        );

        assert!(state.active);
        assert!(!state.warn);
    }

    #[test]
    fn missed_warns_again_after_throttle_window() {
        let mut tracker = CaptureMissTracker::new(Duration::from_millis(3000));
        let _ = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(0),
        );

        let state = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(3001),
        );

        assert!(state.active);
        assert!(state.warn);
    }

    #[test]
    fn idle_does_not_create_or_clear_miss_state() {
        let mut tracker = CaptureMissTracker::new(Duration::from_millis(3000));
        let idle = tracker.update(StitchProgressSignal::Idle, t(0));
        assert!(!idle.active);
        assert!(!idle.warn);

        let _ = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Top,
            },
            t(10),
        );
        let idle_after_miss = tracker.update(StitchProgressSignal::Idle, t(20));
        assert!(idle_after_miss.active);
        assert!(!idle_after_miss.warn);
        assert_eq!(idle_after_miss.edge, CapturedEdge::Top);
    }

    #[test]
    fn accepted_clears_active_miss_state() {
        let mut tracker = CaptureMissTracker::new(Duration::from_millis(3000));
        let _ = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(0),
        );

        let state = tracker.update(
            StitchProgressSignal::Accepted {
                edge: CapturedEdge::Bottom,
            },
            t(10),
        );

        assert!(!state.active);
        assert!(!state.warn);
        assert_eq!(state.edge, CapturedEdge::Unknown);
        assert!(!state.affordance.active);
    }

    // R7: a fresh miss right after a successful reconnect must warn again, not
    // be silently throttled by the pre-recovery `last_warning_at`.
    #[test]
    fn miss_after_recovery_warns_immediately() {
        let mut tracker = CaptureMissTracker::new(Duration::from_millis(3000));
        let _ = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(0),
        );
        let _ = tracker.update(
            StitchProgressSignal::Accepted {
                edge: CapturedEdge::Bottom,
            },
            t(100),
        );

        let state = tracker.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(200),
        );

        assert!(state.active);
        assert!(
            state.warn,
            "miss after recovery must warn within throttle window"
        );
    }

    // R2: the shared outcome->signal converter, exercised here so both capture
    // paths inherit the coverage instead of each re-testing the mapping.
    #[test]
    fn no_match_outcome_maps_to_missed_signal() {
        let outcome = StitchOutcome::NoMatch {
            reason: rollshot_core::NoMatchReason::ReverseDirection,
            best_estimate: None,
        };
        assert_eq!(
            progress_signal_from_outcome(&outcome),
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Unknown
            }
        );
    }

    #[test]
    fn duplicate_outcome_maps_to_idle_signal() {
        assert_eq!(
            progress_signal_from_outcome(&StitchOutcome::Duplicate),
            StitchProgressSignal::Idle
        );
    }

    #[test]
    fn appended_outcome_maps_accepted_edge_from_direction() {
        assert_eq!(
            captured_edge_from_direction(AppendDirection::Bottom),
            CapturedEdge::Bottom
        );
    }
}
