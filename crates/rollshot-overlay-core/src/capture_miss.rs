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
    ReverseDirection,
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
        StitchOutcome::NoMatch {
            reason,
            best_estimate,
        } => match reason {
            rollshot_core::NoMatchReason::ReverseDirection => {
                StitchProgressSignal::ReverseDirection
            }
            _ => StitchProgressSignal::Missed {
                edge: best_estimate
                    .map(|estimate| captured_edge_from_direction(estimate.direction))
                    .unwrap_or(CapturedEdge::Unknown),
            },
        },
        StitchOutcome::AxisChanged { estimate, .. } => StitchProgressSignal::Missed {
            edge: captured_edge_from_direction(estimate.direction),
        },
        StitchOutcome::Duplicate | StitchOutcome::NoProgress { .. } => StitchProgressSignal::Idle,
    }
}

/// Two-miss recovery gate for capture-miss tracking.
///
/// ```text
/// Stitching { consecutive_misses }
///   -- second consecutive genuine miss -->
/// Paused { captured_edge }
///   -- reliable match against frozen last_good -->
/// Stitching { consecutive_misses: 0 }
/// ```
#[derive(Debug)]
pub struct CaptureMissTracker {
    active: bool,
    captured_edge: CapturedEdge,
    consecutive_misses: u8,
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
            captured_edge: CapturedEdge::Unknown,
            consecutive_misses: 0,
            last_warning_at: None,
            throttle,
        }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn update(&mut self, signal: StitchProgressSignal, now: Instant) -> CaptureMissState {
        match signal {
            StitchProgressSignal::Accepted { edge } => {
                self.active = false;
                self.consecutive_misses = 0;
                self.captured_edge = edge;
                self.last_warning_at = None;
                CaptureMissState::default()
            }
            StitchProgressSignal::Missed { edge: _ } => {
                self.consecutive_misses += 1;
                if self.consecutive_misses >= 2 {
                    self.active = true;
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
                        edge: self.captured_edge,
                        affordance: PreviewRecoveryAffordance {
                            active: true,
                            edge: self.captured_edge,
                            processing: false,
                        },
                    }
                } else {
                    CaptureMissState::default()
                }
            }
            StitchProgressSignal::ReverseDirection => self.state(),
            StitchProgressSignal::Idle => {
                self.consecutive_misses = 0;
                self.state()
            }
        }
    }

    pub fn update_recovery(&mut self, recovered: bool, now: Instant) -> CaptureMissState {
        if recovered {
            self.active = false;
            self.consecutive_misses = 0;
            self.captured_edge = CapturedEdge::Unknown;
            self.last_warning_at = None;
            CaptureMissState::default()
        } else {
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
                edge: self.captured_edge,
                affordance: PreviewRecoveryAffordance {
                    active: true,
                    edge: self.captured_edge,
                    processing: false,
                },
            }
        }
    }

    pub fn state(&self) -> CaptureMissState {
        CaptureMissState {
            active: self.active,
            warn: false,
            edge: self.captured_edge,
            affordance: PreviewRecoveryAffordance {
                active: self.active,
                edge: self.captured_edge,
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
    fn second_consecutive_genuine_miss_enters_paused_state() {
        let mut gate = CaptureMissTracker::new(Duration::from_secs(3));
        gate.update(
            StitchProgressSignal::Accepted {
                edge: CapturedEdge::Bottom,
            },
            t(0),
        );
        // The miss-signal edge is intentionally `Unknown`; the paused edge below
        // must come from the last *accepted* append (`captured_edge`), proving the
        // gate sources the guide edge from progress, not from the failed frame.
        assert!(
            !gate
                .update(
                    StitchProgressSignal::Missed {
                        edge: CapturedEdge::Unknown
                    },
                    t(10)
                )
                .active
        );
        let paused = gate.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Unknown,
            },
            t(20),
        );
        assert!(paused.active);
        assert!(paused.warn);
        assert_eq!(paused.edge, CapturedEdge::Bottom);
    }

    #[test]
    fn reverse_direction_is_neutral_and_preserves_miss_count() {
        let mut gate = CaptureMissTracker::new(Duration::from_secs(3));
        gate.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Unknown,
            },
            t(0),
        );
        let state = gate.update(StitchProgressSignal::ReverseDirection, t(10));
        assert!(!state.active);
        assert!(
            gate.update(
                StitchProgressSignal::Missed {
                    edge: CapturedEdge::Unknown
                },
                t(20)
            )
            .active
        );
    }

    #[test]
    fn paused_gate_clears_only_after_recovery() {
        let mut gate = CaptureMissTracker::new(Duration::from_secs(3));
        gate.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Unknown,
            },
            t(0),
        );
        gate.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Unknown,
            },
            t(10),
        );
        assert!(gate.update_recovery(false, t(20)).active);
        assert!(!gate.update_recovery(true, t(30)).active);
    }

    #[test]
    fn warning_is_throttled_in_paused_state() {
        let mut gate = CaptureMissTracker::new(Duration::from_millis(3000));
        gate.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Unknown,
            },
            t(0),
        );
        gate.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(10),
        );

        let throttled = gate.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(1000),
        );
        assert!(throttled.active);
        assert!(!throttled.warn);
    }

    #[test]
    fn warning_pulses_again_after_throttle_window() {
        let mut gate = CaptureMissTracker::new(Duration::from_millis(3000));
        gate.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Unknown,
            },
            t(0),
        );
        gate.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(10),
        );

        let pulsed = gate.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(3011),
        );
        assert!(pulsed.active);
        assert!(pulsed.warn);
    }

    #[test]
    fn miss_after_recovery_warns_immediately() {
        let mut gate = CaptureMissTracker::new(Duration::from_millis(3000));
        gate.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Unknown,
            },
            t(0),
        );
        gate.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(10),
        );
        gate.update_recovery(true, t(100));

        // Fresh miss cycle: first miss doesn't activate.
        assert!(
            !gate
                .update(
                    StitchProgressSignal::Missed {
                        edge: CapturedEdge::Unknown
                    },
                    t(200)
                )
                .active
        );
        // Second miss activates and warns immediately (throttle was cleared).
        let state = gate.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(300),
        );
        assert!(state.active);
        assert!(state.warn);
    }

    #[test]
    fn accepted_resets_miss_count_and_updates_captured_edge() {
        let mut gate = CaptureMissTracker::new(Duration::from_secs(3));
        gate.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Unknown,
            },
            t(0),
        );
        gate.update(
            StitchProgressSignal::Accepted {
                edge: CapturedEdge::Top,
            },
            t(10),
        );
        // Next miss cycle: first miss doesn't activate (count was reset).
        assert!(
            !gate
                .update(
                    StitchProgressSignal::Missed {
                        edge: CapturedEdge::Unknown
                    },
                    t(20)
                )
                .active
        );
    }

    #[test]
    fn idle_resets_miss_count() {
        let mut gate = CaptureMissTracker::new(Duration::from_secs(3));
        gate.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Unknown,
            },
            t(0),
        );
        gate.update(StitchProgressSignal::Idle, t(10));
        // Next miss cycle: first miss doesn't activate.
        assert!(
            !gate
                .update(
                    StitchProgressSignal::Missed {
                        edge: CapturedEdge::Unknown
                    },
                    t(20)
                )
                .active
        );
    }

    #[test]
    fn active_accessor_reflects_state() {
        let mut gate = CaptureMissTracker::new(Duration::from_secs(3));
        assert!(!gate.active());
        gate.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Unknown,
            },
            t(0),
        );
        assert!(!gate.active());
        gate.update(
            StitchProgressSignal::Missed {
                edge: CapturedEdge::Bottom,
            },
            t(10),
        );
        assert!(gate.active());
    }

    #[test]
    fn reverse_direction_outcome_maps_to_reverse_signal() {
        let outcome = StitchOutcome::NoMatch {
            reason: rollshot_core::NoMatchReason::ReverseDirection,
            best_estimate: None,
        };
        assert_eq!(
            progress_signal_from_outcome(&outcome),
            StitchProgressSignal::ReverseDirection
        );
    }

    #[test]
    fn other_no_match_outcomes_map_to_missed() {
        let outcome = StitchOutcome::NoMatch {
            reason: rollshot_core::NoMatchReason::OverlapVerificationFailed,
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
    fn axis_changed_outcome_maps_to_missed() {
        let outcome = StitchOutcome::AxisChanged {
            previous_axis: rollshot_core::ScrollAxis::Vertical,
            new_axis: rollshot_core::ScrollAxis::Horizontal,
            estimate: rollshot_core::MotionEstimate {
                dx: 0,
                dy: 50,
                axis: rollshot_core::ScrollAxis::Horizontal,
                direction: AppendDirection::Bottom,
                confidence: 0.9,
                method: rollshot_core::MatchMethod::Template,
                overlap: rollshot_core::OverlapRegion {
                    prev_x: 0,
                    prev_y: 0,
                    curr_x: 0,
                    curr_y: 50,
                    width: 100,
                    height: 100,
                },
                inliers: None,
                raw_matches: None,
            },
        };
        let signal = progress_signal_from_outcome(&outcome);
        match signal {
            StitchProgressSignal::Missed { edge } => {
                assert_eq!(edge, CapturedEdge::Bottom);
            }
            _ => panic!("expected Missed, got {:?}", signal),
        }
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
