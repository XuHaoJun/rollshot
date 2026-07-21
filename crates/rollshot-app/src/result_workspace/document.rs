use std::path::{Path, PathBuf};

use image::RgbaImage;
use rollshot_image_document::ImageDocument;

pub(crate) const UNSAVED_LABEL: &str = "Unsaved capture";

/// Origin semantics:
///
/// Origin          | Save policy                    | Default export name        | Close can lose source
/// ----------------+--------------------------------+----------------------------+----------------------
/// UnsavedCapture  | Save As; no durable source yet | Rollshot <timestamp>.png   | yes, until exported
/// SavedCapture    | Save As; source is auto-save   | <source file name>         | no
/// Imported        | Save As only; source read-only | <stem>-annotated.png       | no
pub enum DocumentOrigin {
    UnsavedCapture,
    SavedCapture(PathBuf),
    Imported(crate::image_import::ImportedSource),
}

/// The Result Workspace document: the image document plus durable-path
/// identity (spec §7). `origin` tracks the document's creation source and
/// never changes because of annotation export; `last_export_path` is the most
/// recent successful Save As, with `last_export_is_safe` recording whether it
/// was flattened while secure redactions existed.
pub struct ResultDocument {
    pub image: ImageDocument,
    pub(crate) origin: DocumentOrigin,
    pub last_export_path: Option<PathBuf>,
    pub last_export_is_safe: bool,
}

impl ResultDocument {
    pub fn saved(image: RgbaImage, path: PathBuf) -> Self {
        Self::with_origin(image, DocumentOrigin::SavedCapture(path))
    }

    pub fn unsaved(image: RgbaImage) -> Self {
        Self::with_origin(image, DocumentOrigin::UnsavedCapture)
    }

    pub(crate) fn imported(image: RgbaImage, source: crate::image_import::ImportedSource) -> Self {
        Self::with_origin(image, DocumentOrigin::Imported(source))
    }

    fn with_origin(image: RgbaImage, origin: DocumentOrigin) -> Self {
        Self {
            image: ImageDocument::new(image),
            origin,
            last_export_path: None,
            last_export_is_safe: false,
        }
    }

    pub fn source_path(&self) -> Option<&Path> {
        match &self.origin {
            DocumentOrigin::SavedCapture(path) => Some(path),
            DocumentOrigin::Imported(source) => Some(source.display_path()),
            DocumentOrigin::UnsavedCapture => None,
        }
    }

    pub(crate) fn imported_source(&self) -> Option<&crate::image_import::ImportedSource> {
        match &self.origin {
            DocumentOrigin::Imported(source) => Some(source),
            DocumentOrigin::UnsavedCapture | DocumentOrigin::SavedCapture(_) => None,
        }
    }

    pub(crate) fn is_imported(&self) -> bool {
        matches!(&self.origin, DocumentOrigin::Imported(_))
    }

    pub(crate) fn default_save_dir(&self) -> Option<PathBuf> {
        self.imported_source()
            .map(|source| source.default_export_dir())
    }

    pub(crate) fn default_save_name(&self) -> String {
        match &self.origin {
            DocumentOrigin::Imported(source) => {
                let stem = source
                    .display_path()
                    .file_stem()
                    .map(|stem| stem.to_string_lossy())
                    .filter(|stem| !stem.is_empty())
                    .unwrap_or_else(|| "Rollshot".into());
                format!("{stem}-annotated.png")
            }
            DocumentOrigin::SavedCapture(path) => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(capture_default_save_name),
            DocumentOrigin::UnsavedCapture => capture_default_save_name(),
        }
    }

    pub(crate) fn origin_status(&self, dirty: bool) -> Option<&'static str> {
        self.is_imported().then_some(if dirty {
            "Imported • Unsaved edits"
        } else {
            "Imported"
        })
    }

    /// Reveal opens the latest durable output, preferring the annotated
    /// export over the original (spec §7).
    pub fn reveal_path(&self) -> Option<&Path> {
        self.last_export_path.as_deref().or(self.source_path())
    }

    pub fn display_name(&self) -> String {
        self.source_path()
            .or(self.last_export_path.as_deref())
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| UNSAVED_LABEL.to_string())
    }
}

fn capture_default_save_name() -> String {
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H.%M.%S");
    format!("Rollshot {timestamp}.png")
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
    let lose_capture = document.source_path().is_none() && document.last_export_path.is_none();
    if lose_capture || annotations_dirty {
        CloseDecision::Confirm(DiscardPrompt {
            lose_capture,
            lose_edits: annotations_dirty,
        })
    } else {
        CloseDecision::Close
    }
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
            CloseDecision::Confirm(DiscardPrompt {
                lose_capture: true,
                lose_edits: false
            })
        );
    }

    #[test]
    fn dirty_annotations_confirm_edit_loss_even_when_saved() {
        let d = ResultDocument::saved(image(), PathBuf::from("/tmp/a.png"));
        assert_eq!(
            close_decision(&d, true),
            CloseDecision::Confirm(DiscardPrompt {
                lose_capture: false,
                lose_edits: true
            })
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
            DiscardPrompt {
                lose_capture: true,
                lose_edits: false
            }
            .text(),
            "Discard unsaved capture?"
        );
        assert_eq!(
            DiscardPrompt {
                lose_capture: false,
                lose_edits: true
            }
            .text(),
            "Discard annotation edits?"
        );
        assert_eq!(
            DiscardPrompt {
                lose_capture: true,
                lose_edits: true
            }
            .text(),
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

    fn imported_document() -> (tempfile::TempDir, ResultDocument) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("screen.jpg");
        image()
            .save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
        let imported = crate::image_import::load(&path).unwrap();
        let document = ResultDocument::imported(imported.pixels, imported.source);
        (dir, document)
    }

    #[test]
    fn imported_origin_is_clean_durable_and_uses_annotated_png_name() {
        let (_dir, document) = imported_document();

        assert!(document.is_imported());
        assert_eq!(document.display_name(), "screen.jpg");
        assert_eq!(document.default_save_name(), "screen-annotated.png");
        assert_eq!(document.origin_status(false), Some("Imported"));
        assert_eq!(
            document.origin_status(true),
            Some("Imported • Unsaved edits")
        );
        assert_eq!(close_decision(&document, false), CloseDecision::Close);
        assert_eq!(
            close_decision(&document, true),
            CloseDecision::Confirm(DiscardPrompt {
                lose_capture: false,
                lose_edits: true,
            })
        );
    }

    #[test]
    fn imported_reveal_prefers_latest_export() {
        let (_dir, mut document) = imported_document();
        let source = document.source_path().unwrap().to_path_buf();
        assert_eq!(document.reveal_path(), Some(source.as_path()));

        document.last_export_path = Some(PathBuf::from("/tmp/export.png"));
        assert_eq!(document.reveal_path(), Some(Path::new("/tmp/export.png")));
    }

    #[test]
    fn existing_origins_keep_their_display_names() {
        let saved = ResultDocument::saved(image(), PathBuf::from("/tmp/result.png"));
        assert_eq!(saved.display_name(), "result.png");
        let unsaved = ResultDocument::unsaved(image());
        assert_eq!(unsaved.display_name(), UNSAVED_LABEL);
    }
}
