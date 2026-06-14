// Dead-code allowed: this module is consumed by later tasks in the
// feat/secure-redaction-sharing branch.
#![allow(dead_code)]

use std::path::Path;

use rollshot_image_document::Annotation;

use super::document::ResultDocument;

pub(crate) const RETAINED_ORIGINAL_DISCLOSURE: &str =
    "Unredacted original remains saved. Safe exports are flattened.";
pub(crate) const SAFE_EXPORT_OVERWRITE_ERROR: &str =
    "Safe export cannot overwrite the unredacted original. Choose another location.";
pub(crate) const COPY_SAFE_SUCCESS: &str = "Copied safe flattened image";
pub(crate) const SAVE_SAFE_SUCCESS: &str = "Saved safe flattened image";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnredactedAction {
    CopyOriginal,
    RevealOriginal,
}

impl UnredactedAction {
    pub(crate) fn prompt(self) -> &'static str {
        match self {
            Self::CopyOriginal => {
                "Copy the unredacted original? This will expose content hidden by redactions."
            }
            Self::RevealOriginal => {
                "Reveal the unredacted original? This file contains content hidden by redactions."
            }
        }
    }

    pub(crate) fn confirm_label(self) -> &'static str {
        match self {
            Self::CopyOriginal => "Copy Unredacted Original",
            Self::RevealOriginal => "Reveal Unredacted Original",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevealAction<'a> {
    Disabled,
    Immediate { label: &'static str, path: &'a Path },
    ConfirmUnredacted(&'a Path),
}

impl RevealAction<'_> {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Reveal",
            Self::Immediate { label, .. } => label,
            Self::ConfirmUnredacted(_) => "Reveal Unredacted Original\u{2026}",
        }
    }
}

pub(crate) fn has_secure_redactions(document: &ResultDocument) -> bool {
    document
        .image
        .annotations()
        .iter()
        .any(|annotation| matches!(annotation, Annotation::OpaqueRedaction { .. }))
}

pub(crate) fn copy_label(document: &ResultDocument) -> &'static str {
    if has_secure_redactions(document) {
        "Copy Safe Image"
    } else {
        "Copy"
    }
}

pub(crate) fn save_label(document: &ResultDocument) -> &'static str {
    if has_secure_redactions(document) {
        "Save Safe Image As"
    } else {
        "Save As"
    }
}

pub(crate) fn copy_original_label(document: &ResultDocument) -> &'static str {
    if has_secure_redactions(document) {
        "Copy Unredacted Original\u{2026}"
    } else {
        "Copy Original"
    }
}

pub(crate) fn retained_original_disclosure(document: &ResultDocument) -> Option<&'static str> {
    (has_secure_redactions(document) && document.source_path.is_some())
        .then_some(RETAINED_ORIGINAL_DISCLOSURE)
}

pub(crate) fn reveal_action(document: &ResultDocument) -> RevealAction<'_> {
    if has_secure_redactions(document) {
        if document.last_export_is_safe {
            if let Some(path) = document.last_export_path.as_deref() {
                return RevealAction::Immediate {
                    label: "Reveal Last Safe Export",
                    path,
                };
            }
        }
        return document
            .source_path
            .as_deref()
            .map(RevealAction::ConfirmUnredacted)
            .unwrap_or(RevealAction::Disabled);
    }

    document
        .reveal_path()
        .map(|path| RevealAction::Immediate {
            label: "Reveal",
            path,
        })
        .unwrap_or(RevealAction::Disabled)
}

pub(crate) fn default_save_name(document: &ResultDocument) -> String {
    let base = super::document::default_save_name(document);
    if !has_secure_redactions(document) {
        return base;
    }

    let path = Path::new(&base);
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy())
        .unwrap_or_else(|| base.as_str().into());
    match path
        .extension()
        .map(|extension| extension.to_string_lossy())
    {
        Some(extension) => format!("{stem}-redacted.{extension}"),
        None => format!("{stem}-redacted"),
    }
}

pub(crate) fn safe_export_overwrites_source(document: &ResultDocument, destination: &Path) -> bool {
    if !has_secure_redactions(document) {
        return false;
    }
    let Some(source) = document.source_path.as_deref() else {
        return false;
    };
    if source == destination {
        return true;
    }
    match (
        std::fs::canonicalize(source),
        std::fs::canonicalize(destination),
    ) {
        (Ok(source), Ok(destination)) => source == destination,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use rollshot_image_document::ImageRect;
    use std::path::{Path, PathBuf};

    fn image() -> RgbaImage {
        RgbaImage::from_pixel(4, 4, Rgba([10, 20, 30, 255]))
    }

    fn saved() -> ResultDocument {
        ResultDocument::saved(image(), PathBuf::from("/tmp/original.png"))
    }

    fn add_redaction(document: &mut ResultDocument) -> rollshot_image_document::AnnotationId {
        document
            .image
            .add_redaction(ImageRect {
                x: 0.0,
                y: 0.0,
                width: 2.0,
                height: 2.0,
            })
            .unwrap()
    }

    #[test]
    fn derived_redaction_state_drives_labels_and_disclosure() {
        let mut document = saved();
        assert!(!has_secure_redactions(&document));
        assert_eq!(copy_label(&document), "Copy");
        assert_eq!(save_label(&document), "Save As");
        assert_eq!(copy_original_label(&document), "Copy Original");
        assert_eq!(retained_original_disclosure(&document), None);

        let id = add_redaction(&mut document);
        assert!(has_secure_redactions(&document));
        assert_eq!(copy_label(&document), "Copy Safe Image");
        assert_eq!(save_label(&document), "Save Safe Image As");
        assert_eq!(
            copy_original_label(&document),
            "Copy Unredacted Original\u{2026}"
        );
        assert_eq!(
            retained_original_disclosure(&document),
            Some(RETAINED_ORIGINAL_DISCLOSURE)
        );

        document.image.undo();
        assert!(!has_secure_redactions(&document));
        document.image.redo();
        assert!(has_secure_redactions(&document));
        document.image.delete_annotation(id).unwrap();
        assert!(!has_secure_redactions(&document));
    }

    #[test]
    fn unsaved_redacted_document_has_no_retained_original_disclosure() {
        let mut document = ResultDocument::unsaved(image());
        add_redaction(&mut document);
        assert_eq!(retained_original_disclosure(&document), None);
    }

    #[test]
    fn reveal_policy_prefers_last_safe_export_then_warns_for_original() {
        let mut document = saved();
        add_redaction(&mut document);
        assert_eq!(
            reveal_action(&document),
            RevealAction::ConfirmUnredacted(Path::new("/tmp/original.png"))
        );

        document.last_export_path = Some(PathBuf::from("/tmp/safe.png"));
        document.last_export_is_safe = true;
        assert_eq!(
            reveal_action(&document),
            RevealAction::Immediate {
                label: "Reveal Last Safe Export",
                path: Path::new("/tmp/safe.png"),
            }
        );
    }

    #[test]
    fn safe_filename_inserts_redacted_before_extension() {
        let mut document = saved();
        add_redaction(&mut document);
        assert_eq!(default_save_name(&document), "original-redacted.png");

        let document = ResultDocument::unsaved(image());
        assert!(!default_save_name(&document).contains("-redacted"));
    }

    #[test]
    fn source_path_is_rejected_only_for_safe_exports() {
        let mut document = saved();
        let source = Path::new("/tmp/original.png");
        assert!(!safe_export_overwrites_source(&document, source));
        add_redaction(&mut document);
        assert!(safe_export_overwrites_source(&document, source));
        assert!(!safe_export_overwrites_source(
            &document,
            Path::new("/tmp/other.png")
        ));
    }

    #[test]
    fn canonical_source_alias_is_rejected_for_safe_export() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("original.png");
        std::fs::write(&source, b"source").unwrap();
        let mut document = ResultDocument::saved(image(), source.clone());
        add_redaction(&mut document);

        let alias = dir.path().join(".").join("original.png");
        assert_ne!(source.to_string_lossy(), alias.to_string_lossy());
        assert!(safe_export_overwrites_source(&document, &alias));
    }

    #[test]
    fn reveal_labels_cover_disabled_general_original_and_last_safe_export() {
        let unsaved = ResultDocument::unsaved(image());
        assert_eq!(reveal_action(&unsaved).label(), "Reveal");

        let saved_doc = saved();
        assert_eq!(reveal_action(&saved_doc).label(), "Reveal");

        let mut redacted = saved();
        add_redaction(&mut redacted);
        assert_eq!(
            reveal_action(&redacted).label(),
            "Reveal Unredacted Original\u{2026}"
        );

        redacted.last_export_path = Some(PathBuf::from("/tmp/safe.png"));
        redacted.last_export_is_safe = true;
        assert_eq!(reveal_action(&redacted).label(), "Reveal Last Safe Export");
    }

    #[test]
    fn user_facing_policy_copy_never_says_secure() {
        let mut document = saved();
        add_redaction(&mut document);
        for text in [
            RETAINED_ORIGINAL_DISCLOSURE,
            SAFE_EXPORT_OVERWRITE_ERROR,
            COPY_SAFE_SUCCESS,
            SAVE_SAFE_SUCCESS,
            copy_label(&document),
            save_label(&document),
            copy_original_label(&document),
            reveal_action(&document).label(),
            UnredactedAction::CopyOriginal.prompt(),
            UnredactedAction::CopyOriginal.confirm_label(),
            UnredactedAction::RevealOriginal.prompt(),
            UnredactedAction::RevealOriginal.confirm_label(),
        ] {
            assert!(!text.to_ascii_lowercase().contains("secure"), "{text}");
        }
    }
}
