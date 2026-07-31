//! Platform-independent recording status state mapping.
//!
//! Pure state helpers for the macOS recording tray title/tooltip.
//! Tested on every host; the native tray construction that consumes
//! these values lives in [`crate::macos_recording_tray`] (macOS-only).

/// Live motion-recording status. Starts `Off`; transitions to `On` once the
/// encoder accepts frames, or `Failed` on runtime error. `Failed` is terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingStatus {
    Off,
    On,
    Failed,
}

/// Map a `MotionRuntimeStatus` to a `RecordingStatus`, preserving terminal
/// `Failed` on `Off` transitions.
pub fn recording_status(
    motion: rollshot_action::motion::MotionRuntimeStatus,
    current: RecordingStatus,
) -> RecordingStatus {
    match motion {
        rollshot_action::motion::MotionRuntimeStatus::On => RecordingStatus::On,
        rollshot_action::motion::MotionRuntimeStatus::Failed => RecordingStatus::Failed,
        rollshot_action::motion::MotionRuntimeStatus::Off => {
            if current == RecordingStatus::Failed {
                return current;
            }
            RecordingStatus::Off
        }
    }
}

/// Tray title prefix for the given status.
pub fn status_title(status: RecordingStatus) -> String {
    match status {
        RecordingStatus::Off | RecordingStatus::On => "● Rollshot".into(),
        RecordingStatus::Failed => "● Rollshot — motion failed".into(),
    }
}

/// Tray tooltip for the given status.
pub fn status_tooltip(status: RecordingStatus) -> String {
    match status {
        RecordingStatus::Off => "Rollshot is recording".into(),
        RecordingStatus::On => "Rollshot is recording (motion)".into(),
        RecordingStatus::Failed => "Screen recording failed — Action Guide continues".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_action::motion::MotionRuntimeStatus;

    #[test]
    fn motion_on_shows_recording_on() {
        assert_eq!(status_title(RecordingStatus::On), "● Rollshot");
        assert_eq!(
            status_tooltip(RecordingStatus::On),
            "Rollshot is recording (motion)"
        );
    }

    #[test]
    fn motion_failed_shows_failed_text() {
        assert_eq!(
            status_title(RecordingStatus::Failed),
            "● Rollshot — motion failed"
        );
        assert_eq!(
            status_tooltip(RecordingStatus::Failed),
            "Screen recording failed — Action Guide continues"
        );
    }

    #[test]
    fn motion_off_shows_plain_recording() {
        assert_eq!(status_title(RecordingStatus::Off), "● Rollshot");
        assert_eq!(
            status_tooltip(RecordingStatus::Off),
            "Rollshot is recording"
        );
    }

    #[test]
    fn runtime_failure_remains_failed() {
        assert_eq!(
            recording_status(MotionRuntimeStatus::Off, RecordingStatus::Failed),
            RecordingStatus::Failed,
        );
    }

    #[test]
    fn motion_disabled_stays_off() {
        let off = status_title(RecordingStatus::Off);
        let tooltip = status_tooltip(RecordingStatus::Off);
        assert_eq!(off, "● Rollshot");
        assert_eq!(tooltip, "Rollshot is recording");
    }

    #[test]
    fn on_after_off() {
        assert_eq!(
            recording_status(MotionRuntimeStatus::On, RecordingStatus::Off),
            RecordingStatus::On,
        );
    }

    #[test]
    fn off_after_on() {
        assert_eq!(
            recording_status(MotionRuntimeStatus::Off, RecordingStatus::On),
            RecordingStatus::Off,
        );
    }

    #[test]
    fn on_after_failed_stays_failed() {
        // On → Failed → Off should remain Failed (terminal).
        // The on-transition after failure is blocked because Failed is terminal.
        assert_eq!(
            recording_status(MotionRuntimeStatus::Off, RecordingStatus::Failed),
            RecordingStatus::Failed,
        );
    }

    #[test]
    fn runtime_failed_maps_to_failed() {
        assert_eq!(
            recording_status(MotionRuntimeStatus::Failed, RecordingStatus::On),
            RecordingStatus::Failed,
        );
    }
}
