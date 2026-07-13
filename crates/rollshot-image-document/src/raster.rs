//! Minimal anti-aliased software rasterizer for flattened output.

use image::RgbaImage;

use crate::annotation::ShapeKind;
use crate::geometry::{ImagePoint, ImageRect, Rgba8};
use crate::two_point::point_in_triangle;

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
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length = (dx * dx + dy * dy).sqrt();
    if length == 0.0 {
        return;
    }
    let (ux, uy) = (dx / length, dy / length);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let rel_x = x as f32 + 0.5 - start.x;
            let rel_y = y as f32 + 0.5 - start.y;
            // Butt caps: the stroke ends flush at both endpoints, matching the
            // live iced canvas (`Stroke::default()` uses `LineCap::Butt`) and
            // the reviewed geometry where the arrow tip is exactly `end`. A
            // clamped-projection distance would instead round-cap the stroke,
            // painting a semicircle past each endpoint (a nub beyond the tip).
            let along = rel_x * ux + rel_y * uy;
            let perp = (rel_x * uy - rel_y * ux).abs();
            let perp_coverage = (radius + 0.5 - perp).clamp(0.0, 1.0);
            let cap_coverage = (along.min(length - along) + 0.5).clamp(0.0, 1.0);
            blend_px(img, x, y, color, perp_coverage * cap_coverage);
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

const BOX_SAMPLE_OFFSETS: [f32; 4] = [0.125, 0.375, 0.625, 0.875];

fn point_in_shape(kind: ShapeKind, bounds: ImageRect, point: ImagePoint) -> bool {
    match kind {
        ShapeKind::Rectangle => bounds.contains(point),
        ShapeKind::Ellipse => {
            let cx = bounds.x + bounds.width / 2.0;
            let cy = bounds.y + bounds.height / 2.0;
            let rx = bounds.width / 2.0;
            let ry = bounds.height / 2.0;
            if rx <= 0.0 || ry <= 0.0 {
                return false;
            }
            let dx = (point.x - cx) / rx;
            let dy = (point.y - cy) / ry;
            dx * dx + dy * dy <= 1.0
        }
    }
}

/// Filled box shape (Rectangle or Ellipse) with 4×4 AA coverage.
pub(crate) fn fill_box_shape(
    img: &mut RgbaImage,
    kind: ShapeKind,
    bounds: ImageRect,
    color: Rgba8,
) {
    let x0 = bounds.x.floor() as i32;
    let y0 = bounds.y.floor() as i32;
    let x1 = (bounds.x + bounds.width).ceil() as i32;
    let y1 = (bounds.y + bounds.height).ceil() as i32;
    for y in y0..y1 {
        for x in x0..x1 {
            let mut hits = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    let sample = ImagePoint::new(
                        x as f32 + BOX_SAMPLE_OFFSETS[sx],
                        y as f32 + BOX_SAMPLE_OFFSETS[sy],
                    );
                    if point_in_shape(kind, bounds, sample) {
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

/// Stroked box shape (Rectangle or Ellipse) with 4×4 AA coverage and
/// per-pixel interior early-out.
pub(crate) fn stroke_box_shape(
    img: &mut RgbaImage,
    kind: ShapeKind,
    bounds: ImageRect,
    width: f32,
    color: Rgba8,
) {
    let half = width / 2.0;
    let outer = bounds.expanded(half);
    let x0 = outer.x.floor() as i32;
    let y0 = outer.y.floor() as i32;
    let x1 = (outer.x + outer.width).ceil() as i32;
    let y1 = (outer.y + outer.height).ceil() as i32;

    // Inner contracted region: pixels whose centers are strictly inside this
    // are fully interior and can be skipped.
    let inner = ImageRect {
        x: bounds.x + half,
        y: bounds.y + half,
        width: (bounds.width - width).max(0.0),
        height: (bounds.height - width).max(0.0),
    };
    let inner_ellipse = match kind {
        ShapeKind::Ellipse => {
            let cx = bounds.x + bounds.width / 2.0;
            let cy = bounds.y + bounds.height / 2.0;
            let irx = (bounds.width / 2.0 - half).max(0.0);
            let iry = (bounds.height / 2.0 - half).max(0.0);
            if irx > 0.0 && iry > 0.0 {
                Some((cx, cy, irx, iry))
            } else {
                None
            }
        }
        _ => None,
    };

    for y in y0..y1 {
        for x in x0..x1 {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;

            // Early-out: skip pixels whose centers are well inside the interior.
            match kind {
                ShapeKind::Rectangle => {
                    if inner.width > 0.0
                        && inner.height > 0.0
                        && px > inner.x
                        && px < inner.x + inner.width
                        && py > inner.y
                        && py < inner.y + inner.height
                    {
                        continue;
                    }
                }
                ShapeKind::Ellipse => {
                    if let Some((cx, cy, irx, iry)) = inner_ellipse {
                        let dx = (px - cx) / irx;
                        let dy = (py - cy) / iry;
                        if dx * dx + dy * dy < 1.0 {
                            continue;
                        }
                    }
                }
            }

            let mut hits = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    let sample = ImagePoint::new(
                        x as f32 + BOX_SAMPLE_OFFSETS[sx],
                        y as f32 + BOX_SAMPLE_OFFSETS[sy],
                    );
                    let in_outer = point_in_shape(kind, bounds.expanded(half), sample);
                    let in_inner = if half > 0.0 {
                        point_in_shape(kind, bounds.expanded(-half), sample)
                    } else {
                        false
                    };
                    if in_outer && !in_inner {
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

        // A full-span segment with an enormous width: the scan region must be
        // clamped to the image, and every pixel is covered along the segment.
        stroke_line(
            &mut image,
            ImagePoint::new(0.0, 1.0),
            ImagePoint::new(2.0, 1.0),
            f32::MAX,
            color,
        );

        assert!(image.pixels().all(|pixel| pixel.0 == [10, 20, 30, 255]));
    }

    #[test]
    fn line_uses_butt_caps_and_does_not_paint_past_endpoints() {
        // Horizontal segment (10,10)->(30,10), width 8 (radius 4). A pixel 3px
        // beyond `end` sits within the round-cap radius but outside the butt
        // cap, so butt-cap rasterization must leave it untouched.
        let mut image = RgbaImage::from_pixel(40, 20, Rgba([0, 0, 0, 255]));
        let color = Rgba8::new(200, 50, 50, 255);
        stroke_line(
            &mut image,
            ImagePoint::new(10.0, 10.0),
            ImagePoint::new(30.0, 10.0),
            8.0,
            color,
        );
        assert_ne!(
            image.get_pixel(20, 10).0,
            [0, 0, 0, 255],
            "mid-segment pixel must be painted"
        );
        assert_eq!(
            image.get_pixel(33, 10).0,
            [0, 0, 0, 255],
            "pixel beyond the endpoint must stay unpainted with butt caps"
        );
    }

    #[test]
    fn fill_box_shape_rectangle_fills_interior() {
        let mut image = RgbaImage::from_pixel(40, 40, Rgba([0, 0, 0, 0]));
        let color = Rgba8::new(255, 0, 0, 255);
        let bounds = ImageRect {
            x: 5.0,
            y: 5.0,
            width: 30.0,
            height: 30.0,
        };
        fill_box_shape(&mut image, ShapeKind::Rectangle, bounds, color);
        // Interior pixel should be fully painted
        assert_eq!(image.get_pixel(20, 20).0, [255, 0, 0, 255]);
        // Outside pixel should be untouched
        assert_eq!(image.get_pixel(2, 2).0, [0, 0, 0, 0]);
    }

    #[test]
    fn fill_box_shape_ellipse_center_filled_corner_transparent() {
        let mut image = RgbaImage::from_pixel(40, 40, Rgba([0, 0, 0, 0]));
        let color = Rgba8::new(0, 255, 0, 255);
        let bounds = ImageRect {
            x: 5.0,
            y: 5.0,
            width: 30.0,
            height: 30.0,
        };
        fill_box_shape(&mut image, ShapeKind::Ellipse, bounds, color);
        // Center should be filled
        assert!(image.get_pixel(20, 20).0[3] > 200);
        // Corner (outside ellipse) should be transparent
        assert_eq!(image.get_pixel(6, 6).0[3], 0);
    }

    #[test]
    fn stroke_box_shape_rectangle_strokes_outline() {
        let mut image = RgbaImage::from_pixel(40, 40, Rgba([0, 0, 0, 0]));
        let color = Rgba8::new(0, 0, 255, 255);
        let bounds = ImageRect {
            x: 5.0,
            y: 5.0,
            width: 30.0,
            height: 30.0,
        };
        stroke_box_shape(&mut image, ShapeKind::Rectangle, bounds, 2.0, color);
        // Edge pixel should be painted
        assert!(image.get_pixel(5, 20).0[3] > 0);
        // Center should be untouched (well inside the stroke)
        assert_eq!(image.get_pixel(20, 20).0[3], 0);
    }

    #[test]
    fn stroke_box_shape_ellipse_stroke_with_interior_early_out() {
        let mut image = RgbaImage::from_pixel(60, 60, Rgba([0, 0, 0, 0]));
        let color = Rgba8::new(255, 255, 0, 255);
        let bounds = ImageRect {
            x: 5.0,
            y: 5.0,
            width: 50.0,
            height: 50.0,
        };
        stroke_box_shape(&mut image, ShapeKind::Ellipse, bounds, 3.0, color);
        // Edge of ellipse should be painted
        assert!(image.get_pixel(30, 5).0[3] > 0);
        // Center should be untouched
        assert_eq!(image.get_pixel(30, 30).0[3], 0);
    }

    #[test]
    fn fill_box_shape_clips_at_image_edges() {
        let mut image = RgbaImage::from_pixel(20, 20, Rgba([0, 0, 0, 0]));
        let color = Rgba8::new(255, 0, 0, 255);
        let bounds = ImageRect {
            x: -5.0,
            y: -5.0,
            width: 30.0,
            height: 30.0,
        };
        fill_box_shape(&mut image, ShapeKind::Rectangle, bounds, color);
        // Pixel at (0,0) should be painted (inside the rect)
        assert!(image.get_pixel(0, 0).0[3] > 0);
    }
}
