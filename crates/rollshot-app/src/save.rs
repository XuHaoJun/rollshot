use std::path::{Path, PathBuf};

use image::{ImageFormat, RgbaImage};

/// Open a native "Save stitched PNG" dialog, mirroring the Tauri app's
/// `promptSaveStitchedPng` (default name `rollshot.png`, PNG filter). Returns
/// `None` when the user cancels. Runs on the main thread after the iced event
/// loop has exited, so the modal panel has the run loop to itself.
pub fn prompt_save_path() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Save stitched PNG")
        .set_file_name("rollshot.png")
        .add_filter("PNG image", &["png"])
        .save_file()
}

/// Write the stitched capture to `path` as PNG.
pub fn write_png(image: &RgbaImage, path: &Path) -> Result<(), String> {
    image
        .save_with_format(path, ImageFormat::Png)
        .map_err(|err| format!("failed to write PNG to {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::write_png;
    use image::{Rgba, RgbaImage};

    #[test]
    fn write_png_creates_a_decodable_file() {
        let image = RgbaImage::from_pixel(3, 2, Rgba([10, 20, 30, 255]));
        let path = std::env::temp_dir().join("rollshot_app_write_png_test.png");
        let _ = std::fs::remove_file(&path);

        write_png(&image, &path).expect("write succeeds");

        let decoded = image::open(&path).expect("written file decodes as an image");
        assert_eq!(decoded.width(), 3);
        assert_eq!(decoded.height(), 2);

        std::fs::remove_file(&path).ok();
    }
}
