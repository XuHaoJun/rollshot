//! The cross-platform semantic-input seam. `rollshot-action` depends only on
//! this trait; platform implementations live in the P0b crates and push
//! privacy-filtered, burst-aggregated actions. `VisualOnlySource` is the no-op
//! used when no semantic source is available — P0a always uses it.

use crate::diagnostics::TARGET_INPUT;
use crate::models::{CaptureRegion, DegradedReason, InputCapability, TimedSemanticAction};
use crate::recorder::ActionRecorder;

pub trait SemanticInputSource: Send {
    /// Begin observing input for `region`. On `Err`, the caller falls back to
    /// `InputCapability::VisualOnly { reason }` and recording continues.
    fn start(&mut self, region: CaptureRegion) -> Result<InputCapability, DegradedReason>;
    /// Drain semantic actions observed since the last poll. Never returns raw
    /// key codes, typed text, device names, or device paths.
    fn poll(&mut self) -> Vec<TimedSemanticAction>;
    /// Disable the source and release any native resources.
    fn stop(&mut self);
}

/// No-op source: produces no semantic events and always reports visual-only.
/// P0a uses `DegradedReason::SourceStartFailed` ("no platform source wired");
/// in P0b the app constructs it with the real fallback reason when a platform
/// source fails.
#[derive(Debug, Clone, Copy)]
pub struct VisualOnlySource {
    reason: DegradedReason,
}

impl VisualOnlySource {
    pub fn new(reason: DegradedReason) -> Self {
        Self { reason }
    }
}

impl Default for VisualOnlySource {
    fn default() -> Self {
        Self {
            reason: DegradedReason::SourceStartFailed,
        }
    }
}

impl SemanticInputSource for VisualOnlySource {
    fn start(&mut self, _region: CaptureRegion) -> Result<InputCapability, DegradedReason> {
        Ok(InputCapability::VisualOnly {
            reason: self.reason,
        })
    }

    fn poll(&mut self) -> Vec<TimedSemanticAction> {
        Vec::new()
    }

    fn stop(&mut self) {}
}

/// A started semantic-input source paired with the capability it resolved to.
///
/// Owns the single start → fallback → poll → stop lifecycle so the app and the
/// overlay share one fallback semantics. On the source's start failure the
/// source is *replaced* by a started [`VisualOnlySource`] carrying the real
/// `reason`, so later polls drain the no-op source (never a half-started one)
/// and the capability reports the actual degradation cause rather than a
/// generic placeholder.
pub struct StartedSemanticInput {
    source: Box<dyn SemanticInputSource>,
    capability: InputCapability,
}

impl StartedSemanticInput {
    /// Start observing `region`. On the source's `Err(reason)`, swap to a
    /// started `VisualOnlySource { reason }` and report `VisualOnly { reason }`
    /// so recording continues in visual-only mode.
    pub fn start(mut source: Box<dyn SemanticInputSource>, region: CaptureRegion) -> Self {
        match source.start(region) {
            Ok(capability) => {
                tracing::info!(target: TARGET_INPUT, ?capability, "semantic input started");
                Self { source, capability }
            }
            Err(reason) => {
                tracing::warn!(
                    target: TARGET_INPUT,
                    ?reason,
                    "semantic input degraded to visual-only"
                );
                let mut fallback = VisualOnlySource::new(reason);
                // `VisualOnlySource::start` never errors.
                let capability = fallback
                    .start(region)
                    .unwrap_or(InputCapability::VisualOnly { reason });
                Self {
                    source: Box::new(fallback),
                    capability,
                }
            }
        }
    }

    /// The capability resolved at start.
    pub fn capability(&self) -> InputCapability {
        self.capability
    }

    /// Drain the source and forward each privacy-filtered action into `recorder`.
    pub fn poll_into(&mut self, recorder: &mut ActionRecorder) {
        for action in self.source.poll() {
            recorder.ingest_event(action);
        }
    }

    /// Stop observing and release the source's resources.
    pub fn stop(&mut self) {
        self.source.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorConfig;
    use crate::frame_store::StoreConfig;
    use crate::models::{
        CaptureRegion, DegradedReason, InputCapability, MouseButton, SemanticAction,
    };
    use crate::recorder::ActionRecorder;

    fn region() -> CaptureRegion {
        CaptureRegion {
            x: 0,
            y: 0,
            width: 100,
            height: 80,
        }
    }

    #[test]
    fn visual_only_source_starts_visual_only_and_polls_empty() {
        let mut src = VisualOnlySource::new(DegradedReason::SourceStartFailed);
        let cap = src.start(region()).expect("visual-only start never errors");
        assert_eq!(
            cap,
            InputCapability::VisualOnly {
                reason: DegradedReason::SourceStartFailed
            }
        );
        assert!(src.poll().is_empty());
        src.stop();
        assert!(src.poll().is_empty());
    }

    #[test]
    fn visual_only_source_preserves_p0b_fallback_reason() {
        let mut src = VisualOnlySource::new(DegradedReason::PermissionDenied);
        let cap = src.start(region()).unwrap();
        assert_eq!(
            cap,
            InputCapability::VisualOnly {
                reason: DegradedReason::PermissionDenied
            }
        );
    }

    #[test]
    fn semantic_input_source_is_object_safe_and_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Box<dyn SemanticInputSource>>();
        let _boxed: Box<dyn SemanticInputSource> =
            Box::new(VisualOnlySource::new(DegradedReason::SourceStartFailed));
    }

    /// Fails to start with a chosen reason and panics if ever polled — so a test
    /// can prove the failed source was swapped out rather than kept and drained.
    struct FailingSource(DegradedReason);
    impl SemanticInputSource for FailingSource {
        fn start(&mut self, _r: CaptureRegion) -> Result<InputCapability, DegradedReason> {
            Err(self.0)
        }
        fn poll(&mut self) -> Vec<TimedSemanticAction> {
            panic!("a failed source must be swapped out and never polled");
        }
        fn stop(&mut self) {}
    }

    /// Starts with semantic events and yields one queued action once.
    #[derive(Default)]
    struct OneClickSource {
        drained: bool,
    }
    impl SemanticInputSource for OneClickSource {
        fn start(&mut self, _r: CaptureRegion) -> Result<InputCapability, DegradedReason> {
            Ok(InputCapability::SemanticEvents)
        }
        fn poll(&mut self) -> Vec<TimedSemanticAction> {
            if self.drained {
                Vec::new()
            } else {
                self.drained = true;
                vec![TimedSemanticAction {
                    action: SemanticAction::Click {
                        button: MouseButton::Left,
                        position: None,
                    },
                    at_ms: 10,
                }]
            }
        }
        fn stop(&mut self) {}
    }

    fn test_recorder() -> ActionRecorder {
        ActionRecorder::new(region(), StoreConfig::default(), DetectorConfig::default())
    }

    #[test]
    fn start_failure_falls_back_to_visual_only_preserving_reason() {
        let started = StartedSemanticInput::start(
            Box::new(FailingSource(DegradedReason::PermissionDenied)),
            region(),
        );
        assert_eq!(
            started.capability(),
            InputCapability::VisualOnly {
                reason: DegradedReason::PermissionDenied
            },
            "the real start reason must be preserved, not collapsed to SourceStartFailed"
        );
    }

    #[test]
    fn start_failure_swaps_out_the_failed_source() {
        // `FailingSource::poll` panics; the only way `poll_into` stays quiet is
        // if `start` replaced the failed source with a started VisualOnlySource.
        let mut started = StartedSemanticInput::start(
            Box::new(FailingSource(DegradedReason::NoInputDevice)),
            region(),
        );
        let mut recorder = test_recorder();
        started.poll_into(&mut recorder); // must not panic
        started.stop();
    }

    #[test]
    fn successful_start_reports_semantic_events_and_polls() {
        let mut started = StartedSemanticInput::start(Box::<OneClickSource>::default(), region());
        assert_eq!(started.capability(), InputCapability::SemanticEvents);
        let mut recorder = test_recorder();
        started.poll_into(&mut recorder); // forwards one click
        started.poll_into(&mut recorder); // no-op
        started.stop();
    }
}
