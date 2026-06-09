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
/// - Linux: `xdg-open <parent>` (opens the containing directory).
pub fn reveal(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map_err(|e| format!("open -R failed: {e}"))?;
    }

    #[cfg(target_os = "linux")]
    {
        // MVP: open the containing folder without selecting the file. Spec §11.3
        // prefers the freedesktop `org.freedesktop.FileManager1` D-Bus `ShowItems`
        // method (which selects the file) with this `xdg-open` as the fallback.
        // TODO(§11.3): add the D-Bus `ShowItems` tier before this branch ships.
        let parent = path.parent().unwrap_or(path);
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|e| format!("xdg-open failed: {e}"))?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        return Err("reveal is not supported on this platform".to_string());
    }

    Ok(())
}
