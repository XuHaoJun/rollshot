use image::RgbaImage;
use std::borrow::Cow;
use std::path::{Path, PathBuf};

use crate::diagnostics::TARGET_SAVE;

/// Copy the full-resolution image to the system clipboard.
pub fn copy_image(image: &RgbaImage) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("clipboard error: {e}"))?;
    let image_data = arboard::ImageData {
        width: image.width() as usize,
        height: image.height() as usize,
        bytes: Cow::from(image.as_raw().as_slice()),
    };
    clipboard
        .set_image(image_data)
        .map_err(|e| format!("clipboard write error: {e}"))
}

pub fn copy_text(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("clipboard error: {e}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("clipboard write error: {e}"))
}

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

/// Open the directory containing `path` in the platform file manager.
///
/// - macOS: `open -R <path>` (reveals the file itself in Finder).
/// - Linux: prefers the freedesktop `org.freedesktop.FileManager1` D-Bus
///   `ShowItems` method (which selects the file); falls back to `xdg-open
///   <parent>` (opens the containing directory).
pub fn reveal(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map_err(|e| format!("open -R failed: {e}"))?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        reveal_with_fallback(
            || reveal_with_file_manager1(path),
            || reveal_with_xdg_open(path),
        )
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        Err("reveal is not supported on this platform".to_string())
    }
}

/// Try `primary`; if it fails, try `fallback` and combine both error messages.
#[cfg(any(target_os = "linux", test))]
fn reveal_with_fallback(
    primary: impl FnOnce() -> Result<(), String>,
    fallback: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    match primary() {
        Ok(()) => Ok(()),
        Err(primary_error) => fallback().map_err(|fallback_error| {
            format!("{primary_error}; fallback failed: {fallback_error}")
        }),
    }
}

#[cfg(target_os = "linux")]
fn reveal_with_file_manager1(path: &Path) -> Result<(), String> {
    let uri = url::Url::from_file_path(path)
        .map_err(|_| format!("cannot convert path to file URI: {}", path.display()))?
        .to_string();
    let connection =
        zbus::blocking::Connection::session().map_err(|e| format!("D-Bus session failed: {e}"))?;
    let proxy = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.FileManager1",
        "/org/freedesktop/FileManager1",
        "org.freedesktop.FileManager1",
    )
    .map_err(|e| format!("FileManager1 proxy failed: {e}"))?;
    proxy
        .call::<_, _, ()>("ShowItems", &(vec![uri], ""))
        .map_err(|e| format!("FileManager1 ShowItems failed: {e}"))
}

#[cfg(target_os = "linux")]
fn reveal_with_xdg_open(path: &Path) -> Result<(), String> {
    let parent = path.parent().unwrap_or(path);
    std::process::Command::new("xdg-open")
        .arg(parent)
        .spawn()
        .map_err(|e| format!("xdg-open failed: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_with_fallback_skips_fallback_after_primary_success() {
        let mut fallback_called = false;
        let result = reveal_with_fallback(
            || Ok(()),
            || {
                fallback_called = true;
                Ok(())
            },
        );
        assert_eq!(result, Ok(()));
        assert!(!fallback_called);
    }

    #[test]
    fn reveal_with_fallback_runs_fallback_after_primary_failure() {
        let mut fallback_called = false;
        let result = reveal_with_fallback(
            || Err("D-Bus unavailable".to_string()),
            || {
                fallback_called = true;
                Ok(())
            },
        );
        assert_eq!(result, Ok(()));
        assert!(fallback_called);
    }

    #[test]
    fn reveal_with_fallback_reports_both_failures() {
        let result = reveal_with_fallback(
            || Err("D-Bus unavailable".to_string()),
            || Err("xdg-open unavailable".to_string()),
        )
        .expect_err("both operations failed");
        assert!(result.contains("D-Bus unavailable"));
        assert!(result.contains("xdg-open unavailable"));
    }
}
