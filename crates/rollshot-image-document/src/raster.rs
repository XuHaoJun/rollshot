//! Minimal anti-aliased software rasterizer for flattened output.

use image::RgbaImage;

use crate::geometry::{ImagePoint, ImageRect, Rgba8};

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

fn edge(a: ImagePoint, b: ImagePoint, p: ImagePoint) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

fn point_in_triangle(p: ImagePoint, t: &[ImagePoint; 3]) -> bool {
    let d1 = edge(t[0], t[1], p);
    let d2 = edge(t[1], t[2], p);
    let d3 = edge(t[2], t[0], p);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
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
                    if point_in_triangle(sample, t) {
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
