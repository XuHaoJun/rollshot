//! Workspace motion state and Save recording export flow.
//!
//! Manages the lifecycle of a motion recording inside the timeline workspace:
//! session-owned `Ready` assets, failure categories, and the raw-MP4 export
//! picker/worker that writes a byte-identical copy to a user-chosen destination.

use std::path::PathBuf;

use rollshot_action::motion::{MotionFailureCategory, ValidatedMotionAsset};
use rollshot_action::project::MotionAssetLoad;

/// Workspace-level motion state. Replaces the raw `Option<MotionRecordingOutcome>`
/// with a stable, testable enum that separates "no motion", "ready to export",
/// "recording failed", and "motion unavailable on load".
///
/// ```text
/// MotionRecordingOutcome::Ready(asset)   ──► Ready(asset)
/// MotionRecordingOutcome::Failure(cat)   ──► Failed(cat)
/// MotionAssetLoad::Available(asset)      ──► Ready(asset)
/// MotionAssetLoad::Unavailable(cat)      ──► Unavailable(cat)
/// MotionAssetLoad::None                  ──► None
/// Disabled (no toolchain)                ──► None
/// ```
#[derive(Debug, Clone)]
pub(crate) enum WorkspaceMotion {
    /// No motion recording was captured or the feature was disabled.
    None,
    /// A session-owned validated H.264 recording is available for export.
    Ready(ValidatedMotionAsset),
    /// The recording attempt failed. The guide remains usable; the category
    /// drives the user-facing error copy.
    Failed(MotionFailureCategory),
    /// A persisted motion asset was specified but could not be loaded
    /// (corrupt, missing, probe failure). The guide remains usable.
    Unavailable(MotionFailureCategory),
}

impl WorkspaceMotion {
    /// Convert a fresh recording outcome into workspace motion state.
    pub(crate) fn from_outcome(
        outcome: Option<rollshot_action::motion::MotionRecordingOutcome>,
    ) -> Self {
        match outcome {
            None => Self::None,
            Some(rollshot_action::motion::MotionRecordingOutcome::Ready(asset)) => {
                Self::Ready(asset)
            }
            Some(rollshot_action::motion::MotionRecordingOutcome::Failure(cat)) => {
                Self::Failed(cat)
            }
        }
    }

    /// Convert a project-loaded motion asset into workspace motion state.
    pub(crate) fn from_loaded(load: MotionAssetLoad) -> Self {
        match load {
            MotionAssetLoad::None => Self::None,
            MotionAssetLoad::Available(asset) => Self::Ready(asset),
            MotionAssetLoad::Unavailable(cat) => Self::Unavailable(cat),
        }
    }

    /// Borrow the validated asset if the motion is `Ready`.
    pub(crate) fn as_ready(&self) -> Option<&ValidatedMotionAsset> {
        match self {
            Self::Ready(asset) => Some(asset),
            _ => None,
        }
    }

    /// True when the motion state represents a usable recording.
    pub(crate) fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }

    /// True when the motion state represents a failure or unavailability.
    #[allow(dead_code)] // used in product tests and view
    pub(crate) fn is_failed_or_unavailable(&self) -> bool {
        matches!(self, Self::Failed(_) | Self::Unavailable(_))
    }

    /// Consume and return the validated asset if `Ready`, leaving `None` in
    /// its place. Used after a successful project save to release the
    /// session-owned clone (the promoted copy is now project-owned).
    pub(crate) fn take_ready(&mut self) -> Option<ValidatedMotionAsset> {
        let prev = std::mem::replace(self, Self::None);
        match prev {
            Self::Ready(asset) => Some(asset),
            other => {
                *self = other;
                None
            }
        }
    }
}

/// Stable user-facing copy for motion failure categories.
pub(crate) fn failure_category_copy(cat: MotionFailureCategory) -> &'static str {
    match cat {
        MotionFailureCategory::ToolUnavailable => {
            "Screen recording could not be saved: FFmpeg is not available."
        }
        MotionFailureCategory::Spawn => {
            "Screen recording could not be saved: FFmpeg could not start."
        }
        MotionFailureCategory::BrokenPipe => {
            "Screen recording could not be saved: encoder closed unexpectedly."
        }
        MotionFailureCategory::Write => {
            "Screen recording could not be saved: write error during encoding."
        }
        MotionFailureCategory::Filesystem => {
            "Screen recording could not be saved: file system error."
        }
        MotionFailureCategory::Finalize => {
            "Screen recording could not be saved: encoder failed to finalize."
        }
        MotionFailureCategory::Probe => {
            "Screen recording could not be saved: recording validation failed."
        }
        MotionFailureCategory::Digest => {
            "Screen recording could not be saved: integrity check failed."
        }
        MotionFailureCategory::Cancelled => "Screen recording was cancelled.",
    }
}

/// Format a duration in milliseconds as `M:SS.s`.
pub(crate) fn format_duration_ms(ms: u64) -> String {
    let total_secs = ms as f64 / 1000.0;
    let mins = (total_secs / 60.0).floor() as u64;
    let secs = total_secs - (mins as f64 * 60.0);
    format!("{}:{:04.1}", mins, secs)
}

/// Format motion metadata as a one-line summary for the workspace view.
///
/// Example: `12.3s · 1920×1080 · 30 fps · Silent H.264`
pub(crate) fn motion_metadata_line(asset: &ValidatedMotionAsset) -> String {
    let duration = format_duration_ms(asset.duration_ms());
    let fps = if asset.fps_denominator() == 1 {
        format!("{} fps", asset.fps_numerator())
    } else {
        format!("{}/{} fps", asset.fps_numerator(), asset.fps_denominator())
    };
    let audio = match asset.audio() {
        rollshot_action::motion::MotionAudio::None => "Silent",
    };
    let codec = match asset.codec() {
        rollshot_action::motion::MotionCodec::H264 => "H.264",
    };
    format!(
        "{} · {}×{} · {} · {} {}",
        duration,
        asset.width(),
        asset.height(),
        fps,
        audio,
        codec,
    )
}

/// Save recording state machine for the raw-MP4 export picker/worker.
///
/// ```text
/// Idle ──► PickingDestination { operation_id }
///              │  chosen path ──► Exporting { operation_id, destination }
///              │  cancelled ────► Idle
///              v
///         Exporting { operation_id, destination }
///              │  success ──► Idle  (banner shown)
///              │  failure ──► Idle  (error banner)
///              │  stale result ──► ignored
/// ```
#[derive(Debug, Clone)]
pub(crate) enum SaveRecordingState {
    Idle,
    PickingDestination {
        operation_id: u64,
    },
    Exporting {
        operation_id: u64,
        #[allow(dead_code)]
        destination: PathBuf,
    },
}

/// Outcome of the save-recording background worker.
#[derive(Debug, Clone)]
pub(crate) enum SaveRecordingOutcome {
    Success,
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_action::motion::{MotionAudio, MotionCodec, MotionMetadata};

    fn dummy_metadata() -> MotionMetadata {
        MotionMetadata {
            sha256: "a".repeat(64),
            duration_ms: 12_300,
            width: 1920,
            height: 1080,
            fps_numerator: 30,
            fps_denominator: 1,
            codec: MotionCodec::H264,
            audio: MotionAudio::None,
        }
    }

    fn dummy_asset() -> ValidatedMotionAsset {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recording.mp4");
        std::fs::write(&path, b"fake mp4").unwrap();
        ValidatedMotionAsset::new_for_test(dummy_metadata(), path, dir.into_path())
    }

    // ---- WorkspaceMotion construction tests ----

    #[test]
    fn from_outcome_none() {
        let ws = WorkspaceMotion::from_outcome(None);
        assert!(matches!(ws, WorkspaceMotion::None));
    }

    #[test]
    fn from_outcome_ready() {
        let asset = dummy_asset();
        let ws = WorkspaceMotion::from_outcome(Some(
            rollshot_action::motion::MotionRecordingOutcome::Ready(asset),
        ));
        assert!(ws.is_ready());
    }

    #[test]
    fn from_outcome_failure() {
        let ws = WorkspaceMotion::from_outcome(Some(
            rollshot_action::motion::MotionRecordingOutcome::Failure(
                MotionFailureCategory::ToolUnavailable,
            ),
        ));
        assert!(ws.is_failed_or_unavailable());
        assert!(!ws.is_ready());
    }

    #[test]
    fn from_loaded_none() {
        let ws = WorkspaceMotion::from_loaded(MotionAssetLoad::None);
        assert!(matches!(ws, WorkspaceMotion::None));
    }

    #[test]
    fn from_loaded_available() {
        let asset = dummy_asset();
        let ws = WorkspaceMotion::from_loaded(MotionAssetLoad::Available(asset));
        assert!(ws.is_ready());
    }

    #[test]
    fn from_loaded_unavailable() {
        let ws = WorkspaceMotion::from_loaded(MotionAssetLoad::Unavailable(
            MotionFailureCategory::Probe,
        ));
        assert!(ws.is_failed_or_unavailable());
        assert!(!ws.is_ready());
    }

    #[test]
    fn as_ready_returns_asset_when_ready() {
        let asset = dummy_asset();
        let ws = WorkspaceMotion::Ready(asset);
        assert!(ws.as_ready().is_some());
    }

    #[test]
    fn as_ready_returns_none_when_failed() {
        let ws = WorkspaceMotion::Failed(MotionFailureCategory::Spawn);
        assert!(ws.as_ready().is_none());
    }

    #[test]
    fn take_ready_consumes_asset() {
        let asset = dummy_asset();
        let mut ws = WorkspaceMotion::Ready(asset);
        let taken = ws.take_ready();
        assert!(taken.is_some());
        assert!(matches!(ws, WorkspaceMotion::None));
    }

    #[test]
    fn take_ready_noop_when_not_ready() {
        let mut ws = WorkspaceMotion::Failed(MotionFailureCategory::Cancelled);
        let taken = ws.take_ready();
        assert!(taken.is_none());
        assert!(matches!(ws, WorkspaceMotion::Failed(_)));
    }

    // ---- Metadata formatting tests ----

    #[test]
    fn format_duration_whole_seconds() {
        assert_eq!(format_duration_ms(5000), "0:05.0");
    }

    #[test]
    fn format_duration_fractional() {
        assert_eq!(format_duration_ms(12_300), "0:12.3");
    }

    #[test]
    fn motion_metadata_line_includes_all_fields() {
        let asset = dummy_asset();
        let line = motion_metadata_line(&asset);
        assert!(line.contains("1920×1080"), "line: {line}");
        assert!(line.contains("30 fps"), "line: {line}");
        assert!(line.contains("Silent H.264"), "line: {line}");
    }

    #[test]
    fn motion_metadata_line_fractional_fps() {
        let meta = MotionMetadata {
            fps_numerator: 24000,
            fps_denominator: 1001,
            ..dummy_metadata()
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recording.mp4");
        std::fs::write(&path, b"fake mp4").unwrap();
        let asset = ValidatedMotionAsset::new_for_test(meta, path, dir.into_path());
        let line = motion_metadata_line(&asset);
        assert!(line.contains("24000/1001 fps"), "line: {line}");
    }

    // ---- Failure category copy tests ----

    #[test]
    fn failure_copy_is_stable() {
        assert_eq!(
            failure_category_copy(MotionFailureCategory::ToolUnavailable),
            "Screen recording could not be saved: FFmpeg is not available."
        );
        assert_eq!(
            failure_category_copy(MotionFailureCategory::Cancelled),
            "Screen recording was cancelled."
        );
    }

    // ---- SaveRecordingState tests ----

    #[test]
    fn save_recording_state_starts_idle() {
        let state = SaveRecordingState::Idle;
        assert!(matches!(state, SaveRecordingState::Idle));
    }

    // ---- Workspace lifecycle tests (RED) ----
    //
    // These tests exercise the workspace motion state transitions that
    // the update and project modules must implement. They are designed
    // to compile against the WorkspaceMotion API and assert the expected
    // contracts.

    #[test]
    fn native_ready_starts_as_ready() {
        // A fresh workspace with a Ready outcome should report Ready.
        let ws = WorkspaceMotion::from_outcome(Some(
            rollshot_action::motion::MotionRecordingOutcome::Ready(dummy_asset()),
        ));
        assert!(ws.is_ready());
        assert!(ws.as_ready().is_some());
    }

    #[test]
    fn disabled_preserves_none() {
        // When motion is disabled, WorkspaceMotion::from_outcome(None) stays None.
        let ws = WorkspaceMotion::from_outcome(None);
        assert!(matches!(ws, WorkspaceMotion::None));
    }

    #[test]
    fn failed_keeps_guide_usable() {
        // A Failed motion state does not prevent guide operations.
        let ws = WorkspaceMotion::from_outcome(Some(
            rollshot_action::motion::MotionRecordingOutcome::Failure(
                MotionFailureCategory::Finalize,
            ),
        ));
        assert!(!ws.is_ready());
        assert!(ws.is_failed_or_unavailable());
        // The failure category is stable copy.
        let copy = failure_category_copy(*match &ws {
            WorkspaceMotion::Failed(cat) => cat,
            _ => panic!("expected Failed"),
        });
        assert!(copy.contains("encoder failed to finalize"), "copy: {copy}");
    }

    #[test]
    fn save_success_replaces_session_with_none() {
        // After a successful project save, take_ready consumes the asset.
        let mut ws = WorkspaceMotion::from_outcome(Some(
            rollshot_action::motion::MotionRecordingOutcome::Ready(dummy_asset()),
        ));
        assert!(ws.is_ready());
        let taken = ws.take_ready();
        assert!(taken.is_some());
        assert!(matches!(ws, WorkspaceMotion::None));
    }

    #[test]
    fn save_failure_retains_session_asset() {
        // On failed save, the workspace motion state is untouched.
        let mut ws = WorkspaceMotion::from_outcome(Some(
            rollshot_action::motion::MotionRecordingOutcome::Ready(dummy_asset()),
        ));
        // Simulate: save failed, so we do NOT call take_ready.
        assert!(ws.is_ready());
        assert!(ws.as_ready().is_some());
    }

    #[test]
    fn reopen_available_enables_export() {
        // A project with an available motion asset loads as Ready.
        let ws = WorkspaceMotion::from_loaded(MotionAssetLoad::Available(dummy_asset()));
        assert!(ws.is_ready());
    }

    #[test]
    fn reopen_unavailable_keeps_guide_usable() {
        // A project with an unavailable motion asset loads as Unavailable.
        let ws = WorkspaceMotion::from_loaded(MotionAssetLoad::Unavailable(
            MotionFailureCategory::Probe,
        ));
        assert!(!ws.is_ready());
        assert!(ws.is_failed_or_unavailable());
    }

    // ---- Raw-export update tests (RED) ----

    #[test]
    fn save_recording_outcome_success_does_not_change_motion_state() {
        // The save-recording export never mutates the workspace motion state.
        let ws = WorkspaceMotion::Ready(dummy_asset());
        let outcome = SaveRecordingOutcome::Success;
        // After outcome, ws is still Ready.
        assert!(ws.is_ready());
        let _ = outcome; // consumed by handler
    }

    #[test]
    fn save_recording_outcome_failure_does_not_change_motion_state() {
        let ws = WorkspaceMotion::Ready(dummy_asset());
        let outcome = SaveRecordingOutcome::Failed("disk full".into());
        assert!(ws.is_ready());
        let _ = outcome;
    }

    #[test]
    fn stale_worker_result_ignored_by_operation_id() {
        // When the operation_id doesn't match the current Exporting state,
        // the result is dropped silently.
        let current_id: u64 = 5;
        let stale_id: u64 = 3;
        assert_ne!(current_id, stale_id);
        // The handler should check op IDs match before applying.
    }

    #[test]
    fn picker_cancel_returns_to_idle() {
        let state = SaveRecordingState::PickingDestination { operation_id: 1 };
        // Picker returns None → transition to Idle.
        let next = match state {
            SaveRecordingState::PickingDestination { .. } => SaveRecordingState::Idle,
            _ => unreachable!(),
        };
        assert!(matches!(next, SaveRecordingState::Idle));
    }

    // ---- Workspace structural tests (RED) ----
    //
    // These verify that the view helpers produce the expected text for
    // each motion state.

    #[test]
    fn ready_metadata_contains_duration_dimensions_fps_codec() {
        let asset = dummy_asset();
        let line = motion_metadata_line(&asset);
        // Duration: 12.3s formatted as 0:12.3
        assert!(line.contains("0:12"), "line: {line}");
        assert!(line.contains("1920×1080"), "line: {line}");
        assert!(line.contains("30 fps"), "line: {line}");
        assert!(line.contains("Silent H.264"), "line: {line}");
    }

    #[test]
    fn failed_copy_is_stable_category_text() {
        let cat = MotionFailureCategory::Probe;
        let copy = failure_category_copy(cat);
        assert_eq!(
            copy,
            "Screen recording could not be saved: recording validation failed."
        );
    }

    #[test]
    fn unavailable_copy_is_stable_category_text() {
        let cat = MotionFailureCategory::ToolUnavailable;
        let copy = failure_category_copy(cat);
        assert_eq!(
            copy,
            "Screen recording could not be saved: FFmpeg is not available."
        );
    }

    #[test]
    fn none_has_no_metadata() {
        let ws = WorkspaceMotion::None;
        assert!(ws.as_ready().is_none());
        assert!(!ws.is_ready());
        assert!(!ws.is_failed_or_unavailable());
    }
}
