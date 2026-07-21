use image::RgbaImage;
use std::path::{Path, PathBuf};

use crate::diagnostics::TARGET_SAVE;

/// Show an async save-file dialog and return the chosen path, or `None` if the
/// user cancelled.
pub async fn prompt_save_as(default_dir: PathBuf, default_name: String) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_directory(default_dir)
        .set_file_name(default_name)
        .add_filter("PNG", &["png"])
        .save_file()
        .await
        .map(|h| h.path().to_path_buf())
}

/// Write `image` as PNG to `path`. Returns the path on success.
///
/// This is a user-chosen overwrite path (picked via the save dialog), so a
/// normal overwrite write is correct — unlike auto-save, no exclusive-create
/// semantics are needed.
pub fn write_save_as(image: &RgbaImage, path: &Path) -> Result<PathBuf, String> {
    let width = image.width();
    let height = image.height();
    tracing::info!(
        target: TARGET_SAVE,
        width,
        height,
        destination = "save_as",
        "save start"
    );
    if let Err(error) = image.save_with_format(path, image::ImageFormat::Png) {
        let category = crate::storage::classify_save_error(&error.to_string());
        tracing::error!(
            target: TARGET_SAVE,
            category,
            destination = "save_as",
            "save failure"
        );
        return Err(format!("failed to write PNG: {error}"));
    }
    let encoded_bytes = std::fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    tracing::info!(
        target: TARGET_SAVE,
        width,
        height,
        encoded_bytes,
        destination = "save_as",
        "save success"
    );
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn save_as_emits_start_and_success_without_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private.png");
        let log = crate::diagnostics::capture_test_logs(|| {
            write_save_as(&RgbaImage::new(2, 3), &path).unwrap();
        });

        assert!(log.contains("save start"), "log = {log}");
        assert!(log.contains("save success"), "log = {log}");
        assert!(
            !log.contains(path.to_string_lossy().as_ref()),
            "log = {log}"
        );
    }

    #[test]
    fn save_as_emits_failure_without_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing").join("private.png");
        let log = crate::diagnostics::capture_test_logs(|| {
            assert!(write_save_as(&RgbaImage::new(2, 3), &path).is_err());
        });

        assert!(log.contains("save failure"), "log = {log}");
        assert!(
            !log.contains(path.to_string_lossy().as_ref()),
            "log = {log}"
        );
    }

    fn png_chunk_types(bytes: &[u8]) -> Vec<[u8; 4]> {
        let mut offset = 8;
        let mut chunks = Vec::new();
        while offset + 12 <= bytes.len() {
            let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            let chunk_type: [u8; 4] = bytes[offset + 4..offset + 8].try_into().unwrap();
            chunks.push(chunk_type);
            offset += 12 + length;
            if chunk_type == *b"IEND" {
                break;
            }
        }
        chunks
    }

    #[test]
    fn save_as_png_contains_only_flattened_pixels_and_core_png_chunks() {
        use rollshot_image_document::{ImageDocument, ImageRect};

        let mut document =
            ImageDocument::new(RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255])));
        document
            .add_redaction(ImageRect {
                x: 0.0,
                y: 0.0,
                width: 2.0,
                height: 2.0,
            })
            .unwrap();
        let flattened = document.flatten();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("safe.png");
        write_save_as(&flattened, &path).unwrap();

        let decoded = image::open(&path).unwrap().to_rgba8();
        assert_eq!(decoded.as_raw(), flattened.as_raw());
        assert_eq!(decoded.get_pixel(0, 0).0, [0, 0, 0, 255]);
        assert_ne!(
            decoded.get_pixel(0, 0).0,
            document.source().get_pixel(0, 0).0
        );

        let chunks = png_chunk_types(&std::fs::read(path).unwrap());
        assert!(chunks
            .iter()
            .all(|chunk| matches!(chunk.as_slice(), b"IHDR" | b"IDAT" | b"IEND")));
    }
}

pub(crate) fn normalize_png_destination(mut path: PathBuf) -> Result<PathBuf, String> {
    match path.extension() {
        None => {
            path.set_extension("png");
            Ok(path)
        }
        Some(extension)
            if extension
                .to_str()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("png")) =>
        {
            Ok(path)
        }
        Some(_) => Err("Rollshot exports PNG files. Choose a .png filename.".to_string()),
    }
}

/// Open the directory containing `path` in the platform file manager.
///
/// Delegates to [`crate::platform_actions::reveal`].
pub fn reveal(path: &Path) -> Result<(), String> {
    crate::platform_actions::reveal(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_delegates_to_platform_actions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.png");
        std::fs::write(&path, b"test").unwrap();
        // reveal delegates to platform_actions; on CI it may fail
        // due to no file manager, but it must not panic.
        let _ = reveal(&path);
    }

    #[test]
    fn png_destination_normalizes_missing_extension_and_rejects_other_extensions() {
        assert_eq!(
            normalize_png_destination(PathBuf::from("/tmp/result")).unwrap(),
            PathBuf::from("/tmp/result.png")
        );
        assert_eq!(
            normalize_png_destination(PathBuf::from("/tmp/result.PNG")).unwrap(),
            PathBuf::from("/tmp/result.PNG")
        );
        assert_eq!(
            normalize_png_destination(PathBuf::from("/tmp/result.jpg")).unwrap_err(),
            "Rollshot exports PNG files. Choose a .png filename."
        );
    }
}
