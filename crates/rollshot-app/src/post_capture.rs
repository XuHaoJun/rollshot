use std::path::PathBuf;

use crate::diagnostics::TARGET_APP;
use crate::storage::Platform;

#[cfg(target_os = "linux")]
use crate::result_workspace::{run as run_workspace, ResultDocument};

/// The intent behind a capture session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapturePurpose {
    /// Normal capture: auto-save and present the result workspace / thumbnail.
    Present,
    /// OCR capture: recognize text and copy to clipboard.
    Ocr { graphical_feedback: bool },
}

/// Dispatch decision after capture completion: whether there is post-capture
/// work and, for OCR, which side-effect path to follow.
#[derive(Debug, Clone)]
pub enum PurposeCompletion {
    Cancelled,
    Present(rollshot_iced_overlay::CaptureResult),
    Ocr {
        image: image::RgbaImage,
        graphical_feedback: bool,
    },
}

/// Select the completion path based on the capture purpose and result.
pub fn select_completion(
    purpose: CapturePurpose,
    result: Option<rollshot_iced_overlay::CaptureResult>,
) -> PurposeCompletion {
    match result {
        None => PurposeCompletion::Cancelled,
        Some(cr) => match purpose {
            CapturePurpose::Present => PurposeCompletion::Present(cr),
            CapturePurpose::Ocr { graphical_feedback } => PurposeCompletion::Ocr {
                image: cr.image,
                graphical_feedback,
            },
        },
    }
}

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
    let (platform, disposition) = match &presentation {
        Presentation::LinuxSavedWorkspace(_) => ("linux", "saved"),
        Presentation::LinuxUnsavedWorkspace(_) => ("linux", "unsaved"),
        Presentation::MacosSavedThumbnail(_) => ("macos", "saved"),
        Presentation::MacosUnsavedWorkspace(_) => ("macos", "unsaved"),
    };
    tracing::info!(
        target: TARGET_APP,
        platform,
        disposition,
        "presentation selected"
    );
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
    fn cancelled_ocr_has_no_post_capture_work() {
        assert!(matches!(
            select_completion(
                CapturePurpose::Ocr {
                    graphical_feedback: false
                },
                None
            ),
            PurposeCompletion::Cancelled
        ));
    }

    #[test]
    fn cancelled_present_has_no_post_capture_work() {
        assert!(matches!(
            select_completion(CapturePurpose::Present, None),
            PurposeCompletion::Cancelled
        ));
    }

    #[test]
    fn present_purpose_preserves_result() {
        let cr = capture_result();
        match select_completion(CapturePurpose::Present, Some(cr.clone())) {
            PurposeCompletion::Present(r) => {
                assert_eq!(r.image.dimensions(), (1, 1));
            }
            other => panic!("expected Present, got {other:?}"),
        }
    }

    #[test]
    fn ocr_purpose_extracts_image_and_feedback_flag() {
        let cr = capture_result();
        match select_completion(
            CapturePurpose::Ocr {
                graphical_feedback: true,
            },
            Some(cr),
        ) {
            PurposeCompletion::Ocr {
                image,
                graphical_feedback,
            } => {
                assert_eq!(image.dimensions(), (1, 1));
                assert!(graphical_feedback);
            }
            other => panic!("expected Ocr, got {other:?}"),
        }
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

    #[test]
    fn presentation_event_omits_saved_path_and_raw_error() {
        let saved_path = "/home/noah/Desktop/private-capture.png";
        let raw_error = "output directory does not exist: /home/noah/Secret";
        let log = crate::diagnostics::capture_test_logs(|| {
            select_presentation(Platform::Linux, Ok(PathBuf::from(saved_path)));
            select_presentation(Platform::Macos, Err(raw_error.to_string()));
        });

        assert!(!log.contains(saved_path), "log = {log}");
        assert!(!log.contains(raw_error), "log = {log}");
        assert!(log.contains("disposition=\"saved\""), "log = {log}");
        assert!(log.contains("disposition=\"unsaved\""), "log = {log}");
    }
}
