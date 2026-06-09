// Public API consumed by later tasks; allow dead_code until the call sites land.
#![allow(dead_code)]

pub mod actions;

use iced::widget::image::Handle as ImageHandle;
use image::RgbaImage;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const SUCCESS_MESSAGE_DURATION: Duration = Duration::from_secs(4);

// ---------------------------------------------------------------------------
// Document
// ---------------------------------------------------------------------------

pub struct ResultDocument {
    pub source_image: RgbaImage,
    pub saved_path: Option<PathBuf>,
}

impl ResultDocument {
    pub fn saved(image: RgbaImage, path: PathBuf) -> Self {
        Self {
            source_image: image,
            saved_path: Some(path),
        }
    }

    pub fn unsaved(image: RgbaImage) -> Self {
        Self {
            source_image: image,
            saved_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Inline message
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineMessage {
    Success { text: String, expires_at: Instant },
    Error(String),
}

impl InlineMessage {
    pub fn text(&self) -> &str {
        match self {
            InlineMessage::Success { text, .. } => text,
            InlineMessage::Error(text) => text,
        }
    }

    fn success(text: String) -> Self {
        InlineMessage::Success {
            text,
            expires_at: Instant::now() + SUCCESS_MESSAGE_DURATION,
        }
    }
}

// ---------------------------------------------------------------------------
// Close decision
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    Close,
    ConfirmDiscard,
}

pub fn close_decision(document: &ResultDocument) -> CloseDecision {
    if document.saved_path.is_some() {
        CloseDecision::Close
    } else {
        CloseDecision::ConfirmDiscard
    }
}

// ---------------------------------------------------------------------------
// Workspace
// ---------------------------------------------------------------------------

pub struct ResultWorkspace {
    pub document: ResultDocument,
    pub message: Option<InlineMessage>,
    pub confirming_discard: bool,
    /// Iced image handle built once from the source RGBA pixels.
    pub image_handle: ImageHandle,
}

impl ResultWorkspace {
    pub fn new(document: ResultDocument, initial_error: Option<String>) -> Self {
        let image_handle = ImageHandle::from_rgba(
            document.source_image.width(),
            document.source_image.height(),
            document.source_image.as_raw().clone(),
        );

        let message = if let Some(err) = initial_error {
            Some(InlineMessage::Error(err))
        } else {
            document
                .saved_path
                .as_deref()
                .map(|path| InlineMessage::success(format!("Saved to {}", path.display())))
        };

        Self {
            document,
            message,
            confirming_discard: false,
            image_handle,
        }
    }

    pub fn message_text(&self) -> Option<String> {
        self.message.as_ref().map(|m| m.text().to_owned())
    }

    /// Apply the result of a save-as dialog + write.
    ///
    /// - `Ok(Some(path))` — user chose a path and the write succeeded.
    /// - `Ok(None)` — user cancelled the dialog; no change.
    /// - `Err(e)` — write failed; show a persistent error.
    pub fn apply_save_as(&mut self, result: Result<Option<PathBuf>, String>) {
        match result {
            Ok(Some(path)) => {
                let text = format!("Saved to {}", path.display());
                self.document.saved_path = Some(path);
                self.message = Some(InlineMessage::success(text));
            }
            Ok(None) => {
                // User cancelled — no change, no error.
            }
            Err(e) => {
                self.message = Some(InlineMessage::Error(e));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Message enum (update handler implemented in Task 5)
// ---------------------------------------------------------------------------

/// Messages produced by the Result Workspace UI.
///
/// The `update` handler that dispatches these is implemented in Task 5.
#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum Message {
    /// User requested window close (e.g. pressed Esc or window close button).
    RequestClose,
    /// User confirmed they want to discard unsaved changes.
    ConfirmDiscard,
    /// User chose to keep the window open despite unsaved changes.
    KeepUnsaved,
    /// User dismissed the inline success/error banner.
    DismissMessage,
    /// User pressed "Copy to clipboard".
    Copy,
    /// Background clipboard write completed.
    CopyFinished(Result<(), String>),
    /// User pressed "Save As…".
    SaveAs,
    /// The async file-picker returned (None = cancelled).
    SavePathChosen(Option<PathBuf>),
    /// Background PNG write completed.
    SaveFinished(Result<PathBuf, String>),
    /// User pressed "Reveal in Finder / Files".
    Reveal,
    /// Background reveal command completed.
    RevealFinished(Result<(), String>),
    /// Subscription tick for expiring success messages.
    Tick(Instant),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    use std::path::Path;

    fn image() -> RgbaImage {
        RgbaImage::from_pixel(2, 2, Rgba([100, 150, 200, 255]))
    }

    #[test]
    fn saved_document_closes_immediately() {
        let document = ResultDocument::saved(image(), PathBuf::from("/tmp/result.png"));
        assert_eq!(close_decision(&document), CloseDecision::Close);
    }

    #[test]
    fn unsaved_document_requests_discard_confirmation() {
        let document = ResultDocument::unsaved(image());
        assert_eq!(close_decision(&document), CloseDecision::ConfirmDiscard);
    }

    #[test]
    fn save_as_success_updates_saved_path_and_message() {
        let mut state = ResultWorkspace::new(ResultDocument::unsaved(image()), None);
        state.apply_save_as(Ok(Some(PathBuf::from("/tmp/result.png"))));
        assert_eq!(
            state.document.saved_path.as_deref(),
            Some(Path::new("/tmp/result.png"))
        );
        assert!(matches!(state.message, Some(InlineMessage::Success { .. })));
    }

    #[test]
    fn saved_workspace_starts_with_saved_path_message() {
        let path = PathBuf::from("/tmp/result.png");
        let state = ResultWorkspace::new(ResultDocument::saved(image(), path.clone()), None);
        assert_eq!(
            state.message_text(),
            Some(format!("Saved to {}", path.display()))
        );
    }

    #[test]
    fn unsaved_workspace_with_initial_error_has_error_message() {
        let err = "disk full".to_string();
        let state = ResultWorkspace::new(ResultDocument::unsaved(image()), Some(err.clone()));
        assert!(matches!(&state.message, Some(InlineMessage::Error(e)) if e == &err));
    }

    #[test]
    fn save_as_cancel_leaves_no_change() {
        let mut state = ResultWorkspace::new(ResultDocument::unsaved(image()), None);
        state.apply_save_as(Ok(None));
        assert!(state.document.saved_path.is_none());
        assert!(state.message.is_none());
    }

    #[test]
    fn save_as_error_sets_persistent_error_and_no_path() {
        let mut state = ResultWorkspace::new(ResultDocument::unsaved(image()), None);
        state.apply_save_as(Err("write failed".to_string()));
        assert!(state.document.saved_path.is_none());
        assert!(matches!(&state.message, Some(InlineMessage::Error(e)) if e == "write failed"));
    }
}
