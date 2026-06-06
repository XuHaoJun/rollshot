use rollshot_capture::CaptureMode;

use crate::coords::LogicalRect;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CaptureWorkflowKind {
    Scrolling,
    Screenshot,
}

impl From<CaptureMode> for CaptureWorkflowKind {
    fn from(mode: CaptureMode) -> Self {
        match mode {
            CaptureMode::Scrolling => CaptureWorkflowKind::Scrolling,
            CaptureMode::Screenshot => CaptureWorkflowKind::Screenshot,
        }
    }
}

pub enum CaptureWorkflow {
    Scrolling(ScrollingWorkflow),
    Screenshot(ScreenshotWorkflow),
}

pub struct ScrollingWorkflow {
    pub crop_confirmed: bool,
    pub preview: Option<iced::widget::image::Handle>,
}

pub struct ScreenshotWorkflow {
    pub frozen_handle: iced::widget::image::Handle,
}

pub struct OverlaySession {
    pub active_mode: CaptureMode,
    pub workflow: CaptureWorkflow,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OverlayEffect {
    None,
    StartScrollingCapture { crop: LogicalRect },
    FinishScrolling,
    FinishScreenshot { crop: LogicalRect },
    Cancel,
}

impl OverlaySession {
    pub fn new_scrolling() -> Self {
        Self {
            active_mode: CaptureMode::Scrolling,
            workflow: CaptureWorkflow::Scrolling(ScrollingWorkflow {
                crop_confirmed: false,
                preview: None,
            }),
        }
    }

    pub fn new_screenshot(frozen_handle: iced::widget::image::Handle) -> Self {
        Self {
            active_mode: CaptureMode::Screenshot,
            workflow: CaptureWorkflow::Screenshot(ScreenshotWorkflow { frozen_handle }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    #[test]
    fn new_scrolling_session_has_scrolling_mode() {
        let session = OverlaySession::new_scrolling();
        assert_eq!(session.active_mode, CaptureMode::Scrolling);
        assert!(matches!(session.workflow, CaptureWorkflow::Scrolling(_)));
    }

    #[test]
    fn new_screenshot_session_has_screenshot_mode() {
        let img = RgbaImage::new(100, 100);
        let handle =
            iced::widget::image::Handle::from_rgba(img.width(), img.height(), img.to_vec());
        let session = OverlaySession::new_screenshot(handle);
        assert_eq!(session.active_mode, CaptureMode::Screenshot);
        assert!(matches!(session.workflow, CaptureWorkflow::Screenshot(_)));
    }

    #[test]
    fn capture_workflow_kind_from_capture_mode() {
        assert_eq!(
            CaptureWorkflowKind::from(CaptureMode::Scrolling),
            CaptureWorkflowKind::Scrolling
        );
        assert_eq!(
            CaptureWorkflowKind::from(CaptureMode::Screenshot),
            CaptureWorkflowKind::Screenshot
        );
    }

    #[test]
    fn overlay_effect_none_is_default() {
        let effect = OverlayEffect::None;
        assert_eq!(effect, OverlayEffect::None);
    }

    #[test]
    fn scrolling_workflow_starts_unconfirmed() {
        let session = OverlaySession::new_scrolling();
        if let CaptureWorkflow::Scrolling(ref wf) = session.workflow {
            assert!(!wf.crop_confirmed);
            assert!(wf.preview.is_none());
        } else {
            panic!("expected scrolling workflow");
        }
    }

    #[test]
    fn screenshot_workflow_owns_frozen_handle() {
        let img = RgbaImage::new(200, 150);
        let handle =
            iced::widget::image::Handle::from_rgba(img.width(), img.height(), img.to_vec());
        let session = OverlaySession::new_screenshot(handle);
        if let CaptureWorkflow::Screenshot(ref wf) = session.workflow {
            match &wf.frozen_handle {
                iced::widget::image::Handle::Rgba { width, height, .. } => {
                    assert_eq!(*width, 200);
                    assert_eq!(*height, 150);
                }
                other => panic!("expected Rgba handle, got {other:?}"),
            }
        } else {
            panic!("expected screenshot workflow");
        }
    }

    #[test]
    fn start_scrolling_capture_emits_correct_effect() {
        let crop = LogicalRect {
            x: 10.0,
            y: 20.0,
            width: 300.0,
            height: 400.0,
        };
        let effect = OverlayEffect::StartScrollingCapture { crop };
        match effect {
            OverlayEffect::StartScrollingCapture { crop: c } => {
                assert_eq!(c.x, 10.0);
                assert_eq!(c.y, 20.0);
                assert_eq!(c.width, 300.0);
                assert_eq!(c.height, 400.0);
            }
            _ => panic!("expected StartScrollingCapture"),
        }
    }

    #[test]
    fn finish_screenshot_emits_correct_effect() {
        let crop = LogicalRect {
            x: 5.0,
            y: 10.0,
            width: 100.0,
            height: 200.0,
        };
        let effect = OverlayEffect::FinishScreenshot { crop };
        match effect {
            OverlayEffect::FinishScreenshot { crop: c } => {
                assert_eq!(c.x, 5.0);
                assert_eq!(c.width, 100.0);
            }
            _ => panic!("expected FinishScreenshot"),
        }
    }

    #[test]
    fn session_construction_accepts_either_workflow_without_platform_resources() {
        let scrolling = OverlaySession::new_scrolling();
        assert_eq!(scrolling.active_mode, CaptureMode::Scrolling);

        let img = RgbaImage::new(50, 50);
        let handle =
            iced::widget::image::Handle::from_rgba(img.width(), img.height(), img.to_vec());
        let screenshot = OverlaySession::new_screenshot(handle);
        assert_eq!(screenshot.active_mode, CaptureMode::Screenshot);
    }
}
