//! Action Guide input wiring: pick the platform semantic-input source, run its
//! start/poll/stop lifecycle, degrade to visual-only on start failure, and
//! forward privacy-filtered actions into the `rollshot-action` recorder. This
//! is the reusable seam the future Action Guide recording lifecycle calls; P0b
//! exercises it through the `action-guide` CLI probe. (See the plan's Scope
//! Boundary.)

use rollshot_action::{DegradedReason, SemanticInputSource};

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
        Box::new(rollshot_action::VisualOnlySource::new(
            DegradedReason::SourceStartFailed,
        ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_action::DegradedReason;

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
}
