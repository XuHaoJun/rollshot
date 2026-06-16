//! Action Guide input wiring: pick the platform semantic-input source, run its
//! start/poll/stop lifecycle, degrade to visual-only on start failure, and
//! forward privacy-filtered actions into the `rollshot-action` recorder. This
//! is the reusable seam the future Action Guide recording lifecycle calls; P0b
//! exercises it through the `action-guide` CLI probe. (See the plan's Scope
//! Boundary.)

use rollshot_action::{
    ActionRecorder, CaptureRegion, DegradedReason, InputCapability, SemanticInputSource,
    VisualOnlySource,
};

const TARGET: &str = "rollshot::action::app_input";

/// Construct the platform-appropriate semantic input source. On unsupported
/// hosts (or when no platform source is compiled in) this is a
/// `VisualOnlySource` reporting `SourceStartFailed`.
pub fn create_input_source() -> Box<dyn SemanticInputSource> {
    #[cfg(target_os = "linux")]
    {
        Box::new(rollshot_linux_input::EvdevInputSource::new())
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(rollshot_macos_input::MacosInputSource::new())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Box::new(VisualOnlySource::new(DegradedReason::SourceStartFailed))
    }
}

/// Persistent advisory shown while recording/reviewing in visual-only mode.
/// Non-fatal: recording, detection, review, and export remain available
/// (spec §Recording State And Warning).
pub fn degraded_advisory(_reason: DegradedReason) -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Input Monitoring is unavailable. Using visual-only step detection. \
         Open System Settings to grant Input Monitoring."
    }
    #[cfg(not(target_os = "macos"))]
    {
        "Input events unavailable. Using visual-only step detection. See the \
         README to grant temporary input-device access."
    }
}

/// Owns the active input source and the resolved capability for one recording.
pub struct ActionInputSession {
    source: Box<dyn SemanticInputSource>,
    capability: InputCapability,
}

impl ActionInputSession {
    pub fn new(source: Box<dyn SemanticInputSource>) -> Self {
        Self {
            source,
            // Until `start`, treat as not-yet-started visual-only.
            capability: InputCapability::VisualOnly {
                reason: DegradedReason::SourceStartFailed,
            },
        }
    }

    /// Start observing. On the source's `Err(reason)`, swap to a started
    /// `VisualOnlySource{reason}` so recording continues (spec §Session
    /// Lifecycle: semantic-input failure stays Recording, capability=VisualOnly).
    pub fn start(&mut self, region: CaptureRegion) -> InputCapability {
        match self.source.start(region) {
            Ok(cap) => {
                tracing::info!(target: TARGET, ?cap, "input source started");
                self.capability = cap;
            }
            Err(reason) => {
                tracing::warn!(target: TARGET, ?reason, "input source degraded to visual-only");
                let mut fallback = VisualOnlySource::new(reason);
                // VisualOnlySource::start never errors.
                let cap = fallback
                    .start(region)
                    .unwrap_or(InputCapability::VisualOnly { reason });
                self.source = Box::new(fallback);
                self.capability = cap;
            }
        }
        self.capability
    }

    #[allow(dead_code)]
    pub fn capability(&self) -> InputCapability {
        self.capability
    }

    /// Drain the source and forward each action into the recorder.
    pub fn poll_into(&mut self, recorder: &mut ActionRecorder) {
        for action in self.source.poll() {
            recorder.ingest_event(action);
        }
    }

    pub fn stop(&mut self) {
        self.source.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_action::{
        CaptureRegion, DegradedReason, InputCapability, MouseButton, SemanticAction,
        SemanticInputSource, TimedSemanticAction,
    };

    fn region() -> CaptureRegion {
        CaptureRegion {
            x: 0,
            y: 0,
            width: 100,
            height: 80,
        }
    }

    /// A fake that fails to start with a chosen reason.
    struct FailingSource(DegradedReason);
    impl SemanticInputSource for FailingSource {
        fn start(&mut self, _r: CaptureRegion) -> Result<InputCapability, DegradedReason> {
            Err(self.0)
        }
        fn poll(&mut self) -> Vec<TimedSemanticAction> {
            Vec::new()
        }
        fn stop(&mut self) {}
    }

    /// A fake that starts SemanticEvents and yields one queued action once.
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

    #[test]
    fn start_failure_falls_back_to_visual_only_with_reason() {
        let mut session =
            ActionInputSession::new(Box::new(FailingSource(DegradedReason::PermissionDenied)));
        let cap = session.start(region());
        assert_eq!(
            cap,
            InputCapability::VisualOnly {
                reason: DegradedReason::PermissionDenied
            }
        );
        assert_eq!(session.capability(), cap);
        // A degraded session still polls (the swapped VisualOnlySource yields nothing).
        // Build a recorder to forward into.
        let mut recorder = test_recorder();
        session.poll_into(&mut recorder); // must not panic
        session.stop();
    }

    #[test]
    fn successful_start_reports_semantic_events_and_forwards_actions() {
        let mut session = ActionInputSession::new(Box::<OneClickSource>::default());
        assert_eq!(session.start(region()), InputCapability::SemanticEvents);
        let mut recorder = test_recorder();
        // First poll forwards the one click; second is a no-op.
        session.poll_into(&mut recorder);
        session.poll_into(&mut recorder);
        session.stop();
        // The recorder consumed the event without panicking; detailed candidate
        // assertions belong to rollshot-action's own tests.
    }

    #[test]
    fn advisory_text_is_platform_appropriate_and_non_fatal() {
        let linux = degraded_advisory(DegradedReason::PermissionDenied);
        assert!(linux.to_lowercase().contains("visual-only"));
        // The macOS-vs-Linux split is chosen at compile time; just assert the
        // string is non-empty and mentions the visual-only fallback.
        assert!(!degraded_advisory(DegradedReason::NoInputDevice).is_empty());
        assert!(!degraded_advisory(DegradedReason::SourceStartFailed).is_empty());
        assert!(!degraded_advisory(DegradedReason::RuntimeFailure).is_empty());
    }

    fn test_recorder() -> rollshot_action::ActionRecorder {
        rollshot_action::ActionRecorder::new(
            region(),
            rollshot_action::StoreConfig::default(),
            rollshot_action::DetectorConfig::default(),
        )
    }
}
