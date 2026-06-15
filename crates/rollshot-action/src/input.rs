//! The cross-platform semantic-input seam. `rollshot-action` depends only on
//! this trait; platform implementations live in the P0b crates and push
//! privacy-filtered, burst-aggregated actions. `VisualOnlySource` is the no-op
//! used when no semantic source is available — P0a always uses it.

use crate::models::{CaptureRegion, DegradedReason, InputCapability, TimedSemanticAction};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CaptureRegion, DegradedReason, InputCapability};

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
}
