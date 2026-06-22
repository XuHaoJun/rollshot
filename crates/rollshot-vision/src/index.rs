//! `VisualIndex` — built once per automation run; holds the source image and
//! a cached grayscale (the only precompute SP1 needs, for NCC). Manifest-driven
//! lazy precompute is deferred to SP2.

use crate::VisionError;

#[derive(Debug)]
pub struct VisualIndex {
    image: image::RgbaImage,
    width: u32,
    height: u32,
    gray: image::GrayImage,
}

impl VisualIndex {
    pub fn build(image: image::RgbaImage) -> Result<Self, VisionError> {
        let (width, height) = image.dimensions();
        if width == 0 || height == 0 {
            return Err(VisionError::EmptyImage);
        }
        let gray = image::imageops::grayscale(&image);
        Ok(Self {
            image,
            width,
            height,
            gray,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn image(&self) -> &image::RgbaImage {
        &self.image
    }

    pub(crate) fn gray(&self) -> &image::GrayImage {
        &self.gray
    }
}

#[cfg(test)]
mod tests {
    use super::VisualIndex;
    use crate::VisionError;

    fn solid(w: u32, h: u32, lum: u8) -> image::RgbaImage {
        image::RgbaImage::from_pixel(w, h, image::Rgba([lum, lum, lum, 255]))
    }

    #[test]
    fn build_rejects_empty_image() {
        let e = VisualIndex::build(image::RgbaImage::new(0, 0)).unwrap_err();
        assert_eq!(e, VisionError::EmptyImage);
    }

    #[test]
    fn build_caches_grayscale_with_right_dims() {
        let idx = VisualIndex::build(solid(8, 4, 200)).unwrap();
        assert_eq!((idx.width(), idx.height()), (8, 4));
        assert_eq!(idx.gray().dimensions(), (8, 4));
        // Grayscale of a mid-grey RGBA is ~ that grey.
        assert!(idx.gray().get_pixel(0, 0).0[0] > 150);
    }
}
