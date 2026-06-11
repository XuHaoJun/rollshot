use std::path::PathBuf;

use image::RgbaImage;

pub(crate) const UNSAVED_LABEL: &str = "Unsaved capture";
pub(crate) const DISCARD_PROMPT: &str = "Discard unsaved capture?";

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

pub(crate) fn default_save_name(document: &ResultDocument) -> String {
    document
        .saved_path
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
    use image::Rgba;

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
}
