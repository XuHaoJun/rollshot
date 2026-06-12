use std::path::PathBuf;

use crate::diagnostics::TARGET_APP;
use crate::storage::Platform;

#[cfg(target_os = "linux")]
use crate::result_workspace::{run as run_workspace, ResultDocument};

/// What the caller should present to the user after a capture completes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presentation {
    LinuxSavedWorkspace(PathBuf),
    LinuxUnsavedWorkspace(String),
    MacosSavedThumbnail(PathBuf),
    MacosUnsavedWorkspace(String),
}

/// Whether a capture session produced a result or was cancelled.
#[allow(dead_code)]
pub enum CaptureCompletion {
    Present(rollshot_iced_overlay::CaptureResult),
    Cancelled,
}

/// Convert the raw overlay return into a `CaptureCompletion`.
#[allow(dead_code)]
pub fn capture_completion(
    result: Option<rollshot_iced_overlay::CaptureResult>,
) -> CaptureCompletion {
    match result {
        Some(cr) => {
            tracing::info!(target: TARGET_APP, "capture present");
            CaptureCompletion::Present(cr)
        }
        None => {
            tracing::info!(target: TARGET_APP, "capture cancelled");
            CaptureCompletion::Cancelled
        }
    }
}

/// Select which presentation to show based on the platform and auto-save outcome.
pub fn select_presentation(platform: Platform, auto_save: Result<PathBuf, String>) -> Presentation {
    let presentation = match (platform, auto_save) {
        (Platform::Linux, Ok(path)) => Presentation::LinuxSavedWorkspace(path),
        (Platform::Linux, Err(msg)) => Presentation::LinuxUnsavedWorkspace(msg),
        (Platform::Macos, Ok(path)) => Presentation::MacosSavedThumbnail(path),
        (Platform::Macos, Err(msg)) => Presentation::MacosUnsavedWorkspace(msg),
    };
    tracing::info!(target: TARGET_APP, ?presentation, "presentation selected");
    presentation
}

/// Linux end-to-end: auto-save, then launch the Result Workspace.
#[cfg(target_os = "linux")]
pub fn handle_linux_capture(result: rollshot_iced_overlay::CaptureResult) -> Result<(), String> {
    match select_presentation(
        Platform::Linux,
        crate::storage::auto_save(&result.image, Platform::Linux),
    ) {
        Presentation::LinuxSavedWorkspace(path) => {
            run_workspace(ResultDocument::saved(result.image, path), None)
        }
        Presentation::LinuxUnsavedWorkspace(error) => {
            run_workspace(ResultDocument::unsaved(result.image), Some(error))
        }
        _ => unreachable!("Linux policy returned a macOS presentation"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture_result() -> rollshot_iced_overlay::CaptureResult {
        rollshot_iced_overlay::CaptureResult {
            image: image::RgbaImage::new(1, 1),
            stats: None,
        }
    }

    #[test]
    fn platform_policy_selects_the_required_presentations() {
        assert_eq!(
            select_presentation(Platform::Linux, Ok(PathBuf::from("/tmp/a.png"))),
            Presentation::LinuxSavedWorkspace(PathBuf::from("/tmp/a.png"))
        );
        assert_eq!(
            select_presentation(Platform::Macos, Ok(PathBuf::from("/tmp/a.png"))),
            Presentation::MacosSavedThumbnail(PathBuf::from("/tmp/a.png"))
        );
        assert_eq!(
            select_presentation(Platform::Linux, Err("disk full".to_string())),
            Presentation::LinuxUnsavedWorkspace("disk full".to_string())
        );
        assert_eq!(
            select_presentation(Platform::Macos, Err("disk full".to_string())),
            Presentation::MacosUnsavedWorkspace("disk full".to_string())
        );
    }

    #[test]
    fn cancelled_linux_capture_has_no_post_capture_presentation() {
        assert!(matches!(
            capture_completion(None),
            CaptureCompletion::Cancelled
        ));
    }

    #[test]
    fn completed_capture_wraps_result() {
        assert!(matches!(
            capture_completion(Some(capture_result())),
            CaptureCompletion::Present(_)
        ));
    }
}
