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

pub(crate) fn ocr_redaction_masks(
    document: &rollshot_image_document::ImageDocument,
) -> Vec<rollshot_image_document::ImageRect> {
    document
        .annotations()
        .iter()
        .filter_map(|annotation| match annotation {
            Annotation::OpaqueRedaction { bounds, .. } => Some(*bounds),
            _ => None,
        })
        .collect()
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
    (has_secure_redactions(document) && document.source_path().is_some())
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
            .source_path()
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
    let base = document.default_save_name();
    if !has_secure_redactions(document) || document.is_imported() {
        return base;
    }
    add_redacted_suffix(&base)
}

fn add_redacted_suffix(base: &str) -> String {
    let path = Path::new(base);
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy())
        .unwrap_or_else(|| base.into());
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
    let Some(source) = document.source_path() else {
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

pub(crate) const IMPORTED_SOURCE_READ_ONLY_ERROR: &str =
    "Imported source is read-only. Choose another export location.";
pub(crate) const DESTINATION_VERIFICATION_ERROR: &str =
    "Rollshot could not verify the export destination. Choose another location.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExportDestinationError {
    ImportedSourceReadOnly,
    UnsafeRedactionSource,
    VerificationFailed,
}

impl ExportDestinationError {
    pub(crate) fn message(&self) -> &'static str {
        match self {
            Self::ImportedSourceReadOnly => IMPORTED_SOURCE_READ_ONLY_ERROR,
            Self::UnsafeRedactionSource => SAFE_EXPORT_OVERWRITE_ERROR,
            Self::VerificationFailed => DESTINATION_VERIFICATION_ERROR,
        }
    }
}

fn paths_resolve_equal(source: &Path, destination: &Path) -> bool {
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

pub(crate) fn validate_export_destination(
    document: &ResultDocument,
    destination: &Path,
) -> Result<(), ExportDestinationError> {
    if let Some(source) = document.imported_source() {
        return match source.destination_matches(destination) {
            Ok(true) => Err(ExportDestinationError::ImportedSourceReadOnly),
            Ok(false) => Ok(()),
            Err(_) => Err(ExportDestinationError::VerificationFailed),
        };
    }

    if has_secure_redactions(document)
        && document
            .source_path()
            .is_some_and(|source| paths_resolve_equal(source, destination))
    {
        return Err(ExportDestinationError::UnsafeRedactionSource);
    }
    Ok(())
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

    #[test]
    fn opaque_redaction_is_only_secure_sharing_classification() {
        let mut document = saved();

        document
            .image
            .add_shape(
                rollshot_image_document::ShapeKind::Rectangle,
                ImageRect {
                    x: 0.0,
                    y: 0.0,
                    width: 3.0,
                    height: 3.0,
                },
            )
            .unwrap();
        assert!(
            !has_secure_redactions(&document),
            "ordinary Shape must not trigger secure-sharing classification"
        );

        document
            .image
            .add_shape_with_style(
                rollshot_image_document::ShapeKind::Rectangle,
                ImageRect {
                    x: 0.0,
                    y: 0.0,
                    width: 3.0,
                    height: 3.0,
                },
                rollshot_image_document::StrokeStyle::default(),
                Some(rollshot_image_document::Rgb8::new(0, 0, 0)),
            )
            .unwrap();
        assert!(
            !has_secure_redactions(&document),
            "black-filled Shape must not trigger secure-sharing classification"
        );

        let rid = add_redaction(&mut document);
        assert!(
            has_secure_redactions(&document),
            "OpaqueRedaction must trigger secure-sharing classification"
        );

        document.image.delete_annotation(rid).unwrap();
        assert!(
            !has_secure_redactions(&document),
            "removing OpaqueRedaction must clear secure-sharing classification"
        );
    }

    #[test]
    fn pixelate_alone_is_not_secure() {
        let mut document = saved();
        let original_bytes = document.image.source().as_raw().to_vec();
        document
            .image
            .add_pixelate(
                ImageRect {
                    x: 0.0,
                    y: 0.0,
                    width: 2.0,
                    height: 2.0,
                },
                rollshot_image_document::pixelate::DEFAULT_PIXELATE_BLOCK_SIZE,
            )
            .unwrap();

        assert!(!has_secure_redactions(&document));
        assert_eq!(copy_label(&document), "Copy");
        assert!(ocr_redaction_masks(&document.image).is_empty());
        assert_eq!(
            document.image.source().as_raw(),
            original_bytes.as_slice(),
            "source bytes must not change from adding a pixelate annotation"
        );
    }

    #[test]
    fn pixelate_beside_redaction_only_redaction_is_secure() {
        let mut document = saved();
        document
            .image
            .add_pixelate(
                ImageRect {
                    x: 0.0,
                    y: 0.0,
                    width: 2.0,
                    height: 2.0,
                },
                rollshot_image_document::pixelate::DEFAULT_PIXELATE_BLOCK_SIZE,
            )
            .unwrap();
        let rid = add_redaction(&mut document);

        assert!(
            has_secure_redactions(&document),
            "OpaqueRedaction beside Pixelate must trigger secure classification"
        );
        assert_eq!(copy_label(&document), "Copy Safe Image");

        let masks = ocr_redaction_masks(&document.image);
        assert_eq!(masks.len(), 1, "only OpaqueRedaction produces a mask");
        assert_eq!(
            masks[0],
            ImageRect {
                x: 0.0,
                y: 0.0,
                width: 2.0,
                height: 2.0,
            }
        );

        document.image.delete_annotation(rid).unwrap();
        assert!(
            !has_secure_redactions(&document),
            "removing OpaqueRedaction clears secure classification even with Pixelate present"
        );
    }

    #[test]
    fn imported_redaction_keeps_annotated_export_name() {
        let dir = tempfile::tempdir().unwrap();
        let source_path = dir.path().join("screen.jpg");
        image()
            .save_with_format(&source_path, image::ImageFormat::Png)
            .unwrap();
        let imported = crate::image_import::load(&source_path).unwrap();
        let mut document = ResultDocument::imported(imported.pixels, imported.source);
        assert_eq!(default_save_name(&document), "screen-annotated.png");
        add_redaction(&mut document);
        assert_eq!(default_save_name(&document), "screen-annotated.png");
    }

    #[cfg(unix)]
    #[test]
    fn imported_source_is_rejected_with_or_without_redactions() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let source_path = dir.path().join("source.png");
        image()
            .save_with_format(&source_path, image::ImageFormat::Png)
            .unwrap();
        let imported = crate::image_import::load(&source_path).unwrap();
        let mut document = ResultDocument::imported(imported.pixels, imported.source);

        assert_eq!(
            validate_export_destination(&document, &source_path).unwrap_err(),
            ExportDestinationError::ImportedSourceReadOnly
        );

        let alias = dir.path().join("alias.png");
        symlink(&source_path, &alias).unwrap();
        assert_eq!(
            validate_export_destination(&document, &alias).unwrap_err(),
            ExportDestinationError::ImportedSourceReadOnly
        );

        add_redaction(&mut document);
        assert_eq!(
            validate_export_destination(&document, &source_path).unwrap_err(),
            ExportDestinationError::ImportedSourceReadOnly
        );
        assert!(validate_export_destination(&document, &dir.path().join("safe.png")).is_ok());
    }
}
