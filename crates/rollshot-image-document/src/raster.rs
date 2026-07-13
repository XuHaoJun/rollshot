//! Minimal anti-aliased software rasterizer for flattened output.

use image::RgbaImage;

use crate::geometry::{ImagePoint, ImageRect, Rgba8};
use crate::two_point::{point_in_triangle, segment_distance};

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
    let blend =
        |src: u8, dst: u8| -> u8 { (src as f32 * a + dst as f32 * (1.0 - a)).round() as u8 };
    let out_a = a + (dst.0[3] as f32 / 255.0) * (1.0 - a);
    dst.0 = [
        blend(color.r, dst.0[0]),
        blend(color.g, dst.0[1]),
        blend(color.b, dst.0[2]),
        (out_a * 255.0).round() as u8,
    ];
}

/// Solid rectangle fill. Edges snap to whole pixels (crisp redactions); the
/// blend at alpha 255 replaces pixels exactly.
pub(crate) fn fill_rect(img: &mut RgbaImage, rect: ImageRect, color: Rgba8) {
    let x0 = rect.x.round() as i32;
    let y0 = rect.y.round() as i32;
    let x1 = (rect.x + rect.width).round() as i32;
    let y1 = (rect.y + rect.height).round() as i32;
    for y in y0..y1 {
        for x in x0..x1 {
            blend_px(img, x, y, color, 1.0);
        }
    }
}

/// Anti-aliased filled circle: per-pixel coverage from distance to center.
pub(crate) fn fill_circle(img: &mut RgbaImage, center: ImagePoint, radius: f32, color: Rgba8) {
    let x0 = (center.x - radius - 1.0).floor() as i32;
    let y0 = (center.y - radius - 1.0).floor() as i32;
    let x1 = (center.x + radius + 1.0).ceil() as i32;
    let y1 = (center.y + radius + 1.0).ceil() as i32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = ImagePoint::new(x as f32 + 0.5, y as f32 + 0.5).distance(center);
            let coverage = (radius + 0.5 - d).clamp(0.0, 1.0);
            blend_px(img, x, y, color, coverage);
        }
    }
}

/// Anti-aliased ring (circle outline) of `width` centered on `radius`.
pub(crate) fn stroke_circle(
    img: &mut RgbaImage,
    center: ImagePoint,
    radius: f32,
    width: f32,
    color: Rgba8,
) {
    let outer = radius + width / 2.0;
    let inner = radius - width / 2.0;
    let x0 = (center.x - outer - 1.0).floor() as i32;
    let y0 = (center.y - outer - 1.0).floor() as i32;
    let x1 = (center.x + outer + 1.0).ceil() as i32;
    let y1 = (center.y + outer + 1.0).ceil() as i32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = ImagePoint::new(x as f32 + 0.5, y as f32 + 0.5).distance(center);
            let coverage =
                ((outer + 0.5 - d).clamp(0.0, 1.0)) * ((d - inner + 0.5).clamp(0.0, 1.0));
            blend_px(img, x, y, color, coverage);
        }
    }
}

pub(crate) fn stroke_line(
    img: &mut RgbaImage,
    start: ImagePoint,
    end: ImagePoint,
    width: f32,
    color: Rgba8,
) {
    if img.width() == 0 || img.height() == 0 {
        return;
    }
    let radius = width / 2.0;
    let bounds = ImageRect::from_corners(start, end).expanded(radius + 1.0);
    let max_x = i32::try_from(img.width() - 1).unwrap_or(i32::MAX);
    let max_y = i32::try_from(img.height() - 1).unwrap_or(i32::MAX);
    let x0 = (bounds.x.floor() as i32).max(0);
    let y0 = (bounds.y.floor() as i32).max(0);
    let x1 = ((bounds.x + bounds.width).ceil() as i32).min(max_x);
    let y1 = ((bounds.y + bounds.height).ceil() as i32).min(max_y);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let sample = ImagePoint::new(x as f32 + 0.5, y as f32 + 0.5);
            let coverage = (radius + 0.5 - segment_distance(sample, start, end)).clamp(0.0, 1.0);
            blend_px(img, x, y, color, coverage);
        }
    }
}

/// Anti-aliased filled triangle via 4×4 supersampled coverage.
pub(crate) fn fill_triangle(img: &mut RgbaImage, t: &[ImagePoint; 3], color: Rgba8) {
    let xs = [t[0].x, t[1].x, t[2].x];
    let ys = [t[0].y, t[1].y, t[2].y];
    let x0 = xs.iter().cloned().fold(f32::MAX, f32::min).floor() as i32;
    let y0 = ys.iter().cloned().fold(f32::MAX, f32::min).floor() as i32;
    let x1 = xs.iter().cloned().fold(f32::MIN, f32::max).ceil() as i32;
    let y1 = ys.iter().cloned().fold(f32::MIN, f32::max).ceil() as i32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let mut hits = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    let sample = ImagePoint::new(
                        x as f32 + (sx as f32 + 0.5) / 4.0,
                        y as f32 + (sy as f32 + 0.5) / 4.0,
                    );
                    if point_in_triangle(sample, *t) {
                        hits += 1;
                    }
                }
            }
            if hits > 0 {
                blend_px(img, x, y, color, hits as f32 / 16.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn huge_finite_line_width_scans_only_image_pixels() {
        let mut image = RgbaImage::from_pixel(2, 2, Rgba([0, 0, 0, 0]));
        let color = Rgba8::new(10, 20, 30, 255);

        stroke_line(
            &mut image,
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(1.0, 1.0),
            f32::MAX,
            color,
        );

        assert!(image.pixels().all(|pixel| pixel.0 == [10, 20, 30, 255]));
    }
}
