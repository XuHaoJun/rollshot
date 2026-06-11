//! Minimal anti-aliased software rasterizer for flattened output.

use image::RgbaImage;

use crate::geometry::Rgba8;

/// Source-over blend of `color` at `coverage` (0..=1) into pixel (x, y).
/// Out-of-bounds coordinates are ignored.
pub(crate) fn blend_px(img: &mut RgbaImage, x: i32, y: i32, color: Rgba8, coverage: f32) {
    if x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 {
        return;
    }
    let a = (color.a as f32 / 255.0) * coverage.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let dst = img.get_pixel_mut(x as u32, y as u32);
    let blend = |src: u8, dst: u8| -> u8 {
        (src as f32 * a + dst as f32 * (1.0 - a)).round() as u8
    };
    let out_a = a + (dst.0[3] as f32 / 255.0) * (1.0 - a);
    dst.0 = [
        blend(color.r, dst.0[0]),
        blend(color.g, dst.0[1]),
        blend(color.b, dst.0[2]),
        (out_a * 255.0).round() as u8,
    ];
}
