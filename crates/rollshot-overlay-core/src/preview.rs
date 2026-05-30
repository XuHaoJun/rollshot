use image::RgbaImage;

/// Fixed preview width (matches wayscrollshot's PREVIEW_MAX_WIDTH). Keeps the
/// per-frame preview texture small enough to upload stably on the
/// iced_layershell/wgpu path.
pub const PREVIEW_WIDTH: u32 = 280;
/// Cap on the preview height: the preview grows up to this, then follows the
/// bottom of the stitch.
pub const PREVIEW_MAX_HEIGHT: u32 = 480;

/// Build a wayscrollshot-style preview that grows, then follows the bottom.
///
/// Scales `image` to `width`, then takes the bottom `min(scaled_height,
/// max_height)` rows. While the stitch is short the result is short (the
/// preview visibly grows with scroll); once it would exceed `max_height` the
/// result stays bounded and tracks the latest (bottom) content.
pub fn preview_viewport(image: &RgbaImage, width: u32, max_height: u32) -> RgbaImage {
    let width = width.max(1);
    let max_height = max_height.max(1);
    let scale = width as f32 / image.width().max(1) as f32;
    let scaled_height = ((image.height() as f32 * scale).round() as u32).max(1);
    if image.width() == width && image.height() == scaled_height {
        let out_height = image.height().min(max_height);
        let src_y = image.height() - out_height;
        return image::imageops::crop_imm(image, 0, src_y, width, out_height).to_image();
    }

    let scaled = image::imageops::resize(
        image,
        width,
        scaled_height,
        image::imageops::FilterType::Triangle,
    );
    let out_height = scaled.height().min(max_height);
    let src_y = scaled.height() - out_height;
    image::imageops::crop_imm(&scaled, 0, src_y, width, out_height).to_image()
}

#[cfg(test)]
mod tests {
    use super::preview_viewport;
    use image::{Rgba, RgbaImage};

    #[test]
    fn grows_to_content_below_cap() {
        // Stitch shorter than the cap: result height is the scaled content, not
        // padded to the cap — so the preview visibly grows with scroll.
        let image = RgbaImage::from_pixel(1920, 1080, Rgba([12, 34, 56, 255]));
        let view = preview_viewport(&image, 960, 2_000);
        // 1920->960 halves width; 1080->540 < 2000 cap, so no clamp.
        assert_eq!((view.width(), view.height()), (960, 540));
    }

    #[test]
    fn caps_and_follows_bottom_for_tall_canvas() {
        let mut image = RgbaImage::new(960, 6_000);
        for y in 0..image.height() {
            for x in 0..image.width() {
                image.put_pixel(x, y, Rgba([(y % 251) as u8, (x % 251) as u8, 99, 255]));
            }
        }
        let view = preview_viewport(&image, 960, 540);
        // Capped at 540 tall, showing the bottom: first row is source row 6000-540.
        assert_eq!((view.width(), view.height()), (960, 540));
        assert_eq!(
            view.get_pixel(0, 0).0,
            [((6_000 - 540) % 251) as u8, 0, 99, 255]
        );
    }
}
