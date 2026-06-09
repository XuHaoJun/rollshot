use image::RgbaImage;
use std::path::{Path, PathBuf};

pub enum SaveOutcome {
    #[allow(dead_code)]
    Saved(PathBuf),
    Cancelled,
}

pub fn prompt_save_path() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Save stitched PNG")
        .set_file_name("rollshot.png")
        .add_filter("PNG image", &["png"])
        .save_file()
}

pub fn write_png(image: &RgbaImage, path: &Path) -> Result<(), String> {
    image
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(|e| format!("failed to write PNG: {e}"))
}

pub fn save_as(image: &RgbaImage) -> Result<SaveOutcome, String> {
    match prompt_save_path() {
        Some(path) => {
            write_png(image, &path)?;
            Ok(SaveOutcome::Saved(path))
        }
        None => Ok(SaveOutcome::Cancelled),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn write_png_roundtrips() {
        let img = RgbaImage::from_pixel(4, 4, Rgba([10, 20, 30, 255]));
        let dir = std::env::temp_dir().join("rollshot-test-write-png");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.png");

        write_png(&img, &path).unwrap();

        let loaded = image::open(&path).unwrap().to_rgba8();
        assert_eq!(loaded.dimensions(), (4, 4));
        assert_eq!(loaded.get_pixel(0, 0), &Rgba([10, 20, 30, 255]));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
