use image::RgbaImage;
use std::borrow::Cow;
use std::path::{Path, PathBuf};

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
    image
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(|e| format!("failed to write PNG: {e}"))?;
    Ok(path.to_path_buf())
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
