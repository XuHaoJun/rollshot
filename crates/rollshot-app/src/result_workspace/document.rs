use std::path::{Path, PathBuf};

use image::RgbaImage;
use rollshot_image_document::ImageDocument;

pub(crate) const UNSAVED_LABEL: &str = "Unsaved capture";

/// The Result Workspace document: the image document plus durable-path
/// identity (spec §7). `source_path` is the original auto-saved capture and
/// never changes because of annotation export; `last_export_path` is the most
/// recent successful annotated Save As.
pub struct ResultDocument {
    pub image: ImageDocument,
    pub source_path: Option<PathBuf>,
    pub last_export_path: Option<PathBuf>,
}

impl ResultDocument {
    pub fn saved(image: RgbaImage, path: PathBuf) -> Self {
        Self {
            image: ImageDocument::new(image),
            source_path: Some(path),
            last_export_path: None,
        }
    }

    pub fn unsaved(image: RgbaImage) -> Self {
        Self {
            image: ImageDocument::new(image),
            source_path: None,
            last_export_path: None,
        }
    }

    /// Reveal opens the latest durable output, preferring the annotated
    /// export over the original (spec §7).
    pub fn reveal_path(&self) -> Option<&Path> {
        self.last_export_path.as_deref().or(self.source_path.as_deref())
    }

    pub fn display_name(&self) -> String {
        self.source_path
            .as_deref()
            .or(self.last_export_path.as_deref())
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| UNSAVED_LABEL.to_string())
    }
}

// ---------------------------------------------------------------------------
// Close decision
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscardPrompt {
    pub lose_capture: bool,
    pub lose_edits: bool,
}

impl DiscardPrompt {
    pub fn text(&self) -> &'static str {
        match (self.lose_capture, self.lose_edits) {
            (true, true) => "Discard unsaved capture and annotation edits?",
            (true, false) => "Discard unsaved capture?",
            _ => "Discard annotation edits?",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    Close,
    Confirm(DiscardPrompt),
}

/// Spec §12.3: confirm when the capture has no durable file at all, or when
/// annotation edits are dirty relative to the last successful Save As.
pub fn close_decision(document: &ResultDocument, annotations_dirty: bool) -> CloseDecision {
    let lose_capture = document.source_path.is_none() && document.last_export_path.is_none();
    if lose_capture || annotations_dirty {
        CloseDecision::Confirm(DiscardPrompt { lose_capture, lose_edits: annotations_dirty })
    } else {
        CloseDecision::Close
    }
}

pub(crate) fn default_save_name(document: &ResultDocument) -> String {
    document
        .source_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            let timestamp = chrono::Local::now().format("%Y-%m-%d %H.%M.%S");
            format!("Rollshot {timestamp}.png")
        })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use std::path::PathBuf;

    fn image() -> RgbaImage {
        RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255]))
    }

    #[test]
    fn clean_saved_document_closes_immediately() {
        let d = ResultDocument::saved(image(), PathBuf::from("/tmp/a.png"));
        assert_eq!(close_decision(&d, false), CloseDecision::Close);
    }

    #[test]
    fn unsaved_capture_without_export_confirms_capture_loss() {
        let d = ResultDocument::unsaved(image());
        assert_eq!(
            close_decision(&d, false),
            CloseDecision::Confirm(DiscardPrompt { lose_capture: true, lose_edits: false })
        );
    }

    #[test]
    fn dirty_annotations_confirm_edit_loss_even_when_saved() {
        let d = ResultDocument::saved(image(), PathBuf::from("/tmp/a.png"));
        assert_eq!(
            close_decision(&d, true),
            CloseDecision::Confirm(DiscardPrompt { lose_capture: false, lose_edits: true })
        );
    }

    #[test]
    fn unsaved_capture_with_successful_export_closes_when_clean() {
        let mut d = ResultDocument::unsaved(image());
        d.last_export_path = Some(PathBuf::from("/tmp/out.png"));
        assert_eq!(close_decision(&d, false), CloseDecision::Close);
    }

    #[test]
    fn prompt_text_distinguishes_capture_edits_and_both() {
        assert_eq!(
            DiscardPrompt { lose_capture: true, lose_edits: false }.text(),
            "Discard unsaved capture?"
        );
        assert_eq!(
            DiscardPrompt { lose_capture: false, lose_edits: true }.text(),
            "Discard annotation edits?"
        );
        assert_eq!(
            DiscardPrompt { lose_capture: true, lose_edits: true }.text(),
            "Discard unsaved capture and annotation edits?"
        );
    }

    #[test]
    fn reveal_path_prefers_export_then_source() {
        let mut d = ResultDocument::saved(image(), PathBuf::from("/tmp/src.png"));
        assert_eq!(d.reveal_path(), Some(Path::new("/tmp/src.png")));
        d.last_export_path = Some(PathBuf::from("/tmp/out.png"));
        assert_eq!(d.reveal_path(), Some(Path::new("/tmp/out.png")));
        assert!(ResultDocument::unsaved(image()).reveal_path().is_none());
    }
}
