use image::RgbaImage;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved(PathBuf),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputOutcome {
    Saved(PathBuf),
    Copied,
    Cancelled,
    Error(String),
}

pub trait OutputService {
    fn save_as(&mut self, image: &RgbaImage) -> Result<SaveOutcome, String>;
    fn copy(&mut self, image: &RgbaImage) -> Result<(), String>;
}

pub fn perform_output<S: OutputService>(
    service: &mut S,
    action: crate::workspace::OutputAction,
    image: &RgbaImage,
) -> OutputOutcome {
    match action {
        crate::workspace::OutputAction::Save => match service.save_as(image) {
            Ok(SaveOutcome::Saved(path)) => OutputOutcome::Saved(path),
            Ok(SaveOutcome::Cancelled) => OutputOutcome::Cancelled,
            Err(e) => OutputOutcome::Error(e),
        },
        crate::workspace::OutputAction::Copy => match service.copy(image) {
            Ok(()) => OutputOutcome::Copied,
            Err(e) => OutputOutcome::Error(e),
        },
    }
}

pub struct ArboardOutput;

impl ArboardOutput {
    pub fn new() -> Self {
        Self
    }
}

impl OutputService for ArboardOutput {
    fn save_as(&mut self, _image: &RgbaImage) -> Result<SaveOutcome, String> {
        Err("async save not implemented here".to_string())
    }

    fn copy(&mut self, image: &RgbaImage) -> Result<(), String> {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        let data = arboard::ImageData {
            width: image.width() as usize,
            height: image.height() as usize,
            bytes: std::borrow::Cow::Borrowed(image.as_raw()),
        };
        clipboard.set_image(data).map_err(|e| e.to_string())?;
        Ok(())
    }
}

pub fn outcome_to_phase_decision(
    outcome: &OutputOutcome,
    current_phase: crate::workspace::WorkspacePhase,
) -> WorkspaceTransition {
    match outcome {
        OutputOutcome::Saved(_) | OutputOutcome::Copied => WorkspaceTransition::Exit,
        OutputOutcome::Cancelled => match current_phase {
            crate::workspace::WorkspacePhase::ResultReview => {
                WorkspaceTransition::StayInResultReview
            }
            _ => WorkspaceTransition::EnterResultReview,
        },
        OutputOutcome::Error(_) => match current_phase {
            crate::workspace::WorkspacePhase::ResultReview => {
                WorkspaceTransition::StayInResultReview
            }
            _ => WorkspaceTransition::EnterResultReview,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTransition {
    Exit,
    EnterResultReview,
    StayInResultReview,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{OutputAction, WorkspacePhase};
    use image::{Rgba, RgbaImage};

    fn image() -> RgbaImage {
        RgbaImage::from_pixel(1200, 2400, Rgba([100, 150, 200, 255]))
    }

    struct FakeOutput {
        save_result: Result<SaveOutcome, String>,
        copy_result: Result<(), String>,
        pub copied_dimensions: Option<(u32, u32)>,
    }

    impl FakeOutput {
        fn default() -> Self {
            Self {
                save_result: Ok(SaveOutcome::Saved(PathBuf::from("/tmp/test.png"))),
                copy_result: Ok(()),
                copied_dimensions: None,
            }
        }

        fn save_cancelled() -> Self {
            Self {
                save_result: Ok(SaveOutcome::Cancelled),
                copy_result: Ok(()),
                copied_dimensions: None,
            }
        }

        fn save_error() -> Self {
            Self {
                save_result: Err("disk full".to_string()),
                copy_result: Ok(()),
                copied_dimensions: None,
            }
        }

        fn copy_error() -> Self {
            Self {
                save_result: Ok(SaveOutcome::Saved(PathBuf::from("/tmp/test.png"))),
                copy_result: Err("clipboard unavailable".to_string()),
                copied_dimensions: None,
            }
        }
    }

    impl OutputService for FakeOutput {
        fn save_as(&mut self, _image: &RgbaImage) -> Result<SaveOutcome, String> {
            self.save_result.clone()
        }

        fn copy(&mut self, image: &RgbaImage) -> Result<(), String> {
            self.copied_dimensions = Some((image.width(), image.height()));
            self.copy_result.clone()
        }
    }

    #[test]
    fn cancelled_save_keeps_result_review() {
        let mut output = FakeOutput::save_cancelled();
        assert_eq!(
            perform_output(&mut output, OutputAction::Save, &image()),
            OutputOutcome::Cancelled
        );
    }

    #[test]
    fn copy_receives_full_resolution_rgba() {
        let mut output = FakeOutput::default();
        perform_output(&mut output, OutputAction::Copy, &image());
        assert_eq!(output.copied_dimensions, Some((1200, 2400)));
    }

    #[test]
    fn successful_save_returns_path() {
        let mut output = FakeOutput::default();
        assert_eq!(
            perform_output(&mut output, OutputAction::Save, &image()),
            OutputOutcome::Saved(PathBuf::from("/tmp/test.png"))
        );
    }

    #[test]
    fn save_error_returns_error_outcome() {
        let mut output = FakeOutput::save_error();
        assert_eq!(
            perform_output(&mut output, OutputAction::Save, &image()),
            OutputOutcome::Error("disk full".to_string())
        );
    }

    #[test]
    fn copy_error_returns_error_outcome() {
        let mut output = FakeOutput::copy_error();
        assert_eq!(
            perform_output(&mut output, OutputAction::Copy, &image()),
            OutputOutcome::Error("clipboard unavailable".to_string())
        );
    }

    #[test]
    fn successful_copy_returns_copied() {
        let mut output = FakeOutput::default();
        assert_eq!(
            perform_output(&mut output, OutputAction::Copy, &image()),
            OutputOutcome::Copied
        );
    }

    #[test]
    fn cancel_during_result_review_stays_in_result_review() {
        let transition =
            outcome_to_phase_decision(&OutputOutcome::Cancelled, WorkspacePhase::ResultReview);
        assert_eq!(transition, WorkspaceTransition::StayInResultReview);
    }

    #[test]
    fn cancel_during_scrolling_enters_result_review() {
        let transition =
            outcome_to_phase_decision(&OutputOutcome::Cancelled, WorkspacePhase::ScrollingCapture);
        assert_eq!(transition, WorkspaceTransition::EnterResultReview);
    }

    #[test]
    fn successful_output_exits() {
        let transition = outcome_to_phase_decision(
            &OutputOutcome::Saved(PathBuf::from("/tmp/test.png")),
            WorkspacePhase::ResultReview,
        );
        assert_eq!(transition, WorkspaceTransition::Exit);

        let transition =
            outcome_to_phase_decision(&OutputOutcome::Copied, WorkspacePhase::ResultReview);
        assert_eq!(transition, WorkspaceTransition::Exit);
    }

    #[test]
    fn error_during_result_review_stays_in_result_review() {
        let transition = outcome_to_phase_decision(
            &OutputOutcome::Error("fail".to_string()),
            WorkspacePhase::ResultReview,
        );
        assert_eq!(transition, WorkspaceTransition::StayInResultReview);
    }

    #[test]
    fn error_during_selected_enters_result_review() {
        let transition = outcome_to_phase_decision(
            &OutputOutcome::Error("fail".to_string()),
            WorkspacePhase::Selected,
        );
        assert_eq!(transition, WorkspaceTransition::EnterResultReview);
    }
}
