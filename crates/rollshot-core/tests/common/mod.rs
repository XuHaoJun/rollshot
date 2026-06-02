#![allow(dead_code)]

use image::{imageops, Rgba, RgbaImage};

/// Builds a tall, deterministic canvas with stripes, color blocks and column
/// patterns. The texture is rich enough that NCC template matching picks a
/// confident offset on any viewport-sized crop.
pub fn make_scroll_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([245, 245, 245, 255]));

    for y in (0..height).step_by(36) {
        let accent = ((y / 3) % 180) as u8;
        for x in 24..width.saturating_sub(24) {
            let stripe = if (x / 7 + y / 11) % 2 == 0 { 220 } else { 180 };
            img.put_pixel(x, y, Rgba([accent, stripe, 80, 255]));
            if y + 1 < height {
                img.put_pixel(x, y + 1, Rgba([30, 30, 30, 255]));
            }
        }
    }

    for block in 0..10u32 {
        let y0 = 30 + block * 80;
        let block_h = 34 + (block % 3) * 8;
        let color = [
            ((40u16 + block as u16 * 17) % 200) as u8,
            ((90u16 + block as u16 * 11) % 200) as u8,
            ((140u16 + block as u16 * 19) % 200) as u8,
            255,
        ];
        for y in y0..(y0 + block_h).min(height) {
            for x in 30..width.saturating_sub(30) {
                if x % (9 + block % 5) == 0 || y % (7 + block % 4) == 0 {
                    img.put_pixel(x, y, Rgba(color));
                }
            }
        }
    }

    for col in [42u32, 96, 154, 211, 268] {
        if col >= width {
            continue;
        }
        for y in 20..height.saturating_sub(20) {
            if (y / 13) % 3 != 0 {
                img.put_pixel(col, y, Rgba([20, 20, 20, 255]));
            }
        }
    }

    img
}

#[derive(Debug, Clone, Copy)]
pub enum Side {
    Left,
    Right,
}

pub fn paint_sticky_sidebar(frame: &mut image::RgbaImage, side: Side, width: u32) {
    let h = frame.height();
    let w = frame.width();
    let x_start = match side {
        Side::Left => 0,
        Side::Right => w.saturating_sub(width),
    };
    for y in 0..h {
        for x in x_start..(x_start + width).min(w) {
            let v = if (y / 7) % 2 == 0 { 100 } else { 140 };
            frame.put_pixel(x, y, image::Rgba([v, v, v, 255]));
        }
    }
}

pub fn paint_sticky_footer(frame: &mut image::RgbaImage, height: u32) {
    let h = frame.height();
    let w = frame.width();
    let y_start = h.saturating_sub(height);
    for y in y_start..h {
        for x in 0..w {
            let v = if (x / 9) % 2 == 0 { 110 } else { 150 };
            frame.put_pixel(x, y, image::Rgba([v, v, v, 255]));
        }
    }
}

pub fn paint_decorative_bottom_border(frame: &mut image::RgbaImage, color: image::Rgba<u8>) {
    let h = frame.height();
    if h == 0 {
        return;
    }
    let w = frame.width();
    for x in 0..w {
        frame.put_pixel(x, h - 1, color);
    }
}

pub fn paint_sticky_horizontal_band(frame: &mut image::RgbaImage, top_h: u32, bottom_h: u32) {
    let h = frame.height();
    let w = frame.width();
    for y in 0..top_h.min(h) {
        for x in 0..w {
            let v = if (x / 5) % 2 == 0 { 90 } else { 130 };
            frame.put_pixel(x, y, image::Rgba([v, v, v, 255]));
        }
    }
    let bottom_start = h.saturating_sub(bottom_h);
    for y in bottom_start..h {
        for x in 0..w {
            let v = if (x / 5) % 2 == 0 { 95 } else { 135 };
            frame.put_pixel(x, y, image::Rgba([v, v, v, 255]));
        }
    }
}

pub fn paint_sidebar_icon_at(
    frame: &mut image::RgbaImage,
    sidebar_width: u32,
    icon_y: u32,
    icon_h: u32,
    color: image::Rgba<u8>,
) {
    let h = frame.height();
    let w = frame.width();
    let y0 = icon_y.min(h);
    let y1 = (icon_y + icon_h).min(h);
    let x1 = sidebar_width.min(w);
    for y in y0..y1 {
        for x in 0..x1 {
            frame.put_pixel(x, y, color);
        }
    }
}

/// Crops a viewport-sized frame from the canvas (vertically).
pub fn crop_frame(canvas: &RgbaImage, y: u32, height: u32) -> RgbaImage {
    imageops::crop_imm(canvas, 0, y, canvas.width(), height).to_image()
}

/// Overlays a constant header band on a frame, simulating a sticky UI header.
pub fn paint_sticky_header(frame: &mut RgbaImage, header_h: u32) {
    let header_h = header_h.min(frame.height());
    for y in 0..header_h {
        for x in 0..frame.width() {
            let on = ((x / 4) + (y / 3)) % 2 == 0;
            let color = if on {
                Rgba([200, 60, 60, 255])
            } else {
                Rgba([30, 30, 90, 255])
            };
            frame.put_pixel(x, y, color);
        }
    }
}

/// Builds a wide deterministic canvas suitable for horizontal scroll fixtures.
/// The texture differs from `make_scroll_canvas` so column shifts produce
/// distinct grayscale patterns.
pub fn make_wide_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([240, 240, 240, 255]));

    for x in (0..width).step_by(36) {
        let accent = ((x / 3) % 180) as u8;
        for y in 24..height.saturating_sub(24) {
            let stripe = if (x / 7 + y / 11) % 2 == 0 { 220 } else { 180 };
            img.put_pixel(x, y, Rgba([stripe, accent, 80, 255]));
            if x + 1 < width {
                img.put_pixel(x + 1, y, Rgba([30, 30, 30, 255]));
            }
        }
    }

    for row in [42u32, 96, 154, 211, 268] {
        if row >= height {
            continue;
        }
        for x in 20..width.saturating_sub(20) {
            if (x / 13) % 3 != 0 {
                img.put_pixel(x, row, Rgba([20, 20, 20, 255]));
            }
        }
    }

    img
}

/// Crops a horizontal viewport-sized frame from the canvas.
pub fn crop_frame_xy(canvas: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
    imageops::crop_imm(canvas, x, y, w, h).to_image()
}

/// Builds deliberately ambiguous repeated rows. Multiple offsets look equally
/// plausible, so Plan 2 should reject them without the feature fallback.
pub fn make_repeated_rows(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([250, 250, 250, 255]));
    for y in 0..height {
        let band = (y / 16) % 2;
        let color = if band == 0 {
            Rgba([40, 40, 40, 255])
        } else {
            Rgba([210, 210, 210, 255])
        };
        for x in 0..width {
            img.put_pixel(x, y, color);
        }
    }
    img
}

/// Builds mostly repeated content with sparse unique corners. Template and edge
/// projections see many plausible offsets, while the feature fallback can vote
/// on the sparse corners.
pub fn make_feature_fallback_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = make_repeated_rows(width, height);

    for i in 0..80u32 {
        let x = 20 + ((i * 43) % width.saturating_sub(40).max(1));
        let y = 20 + ((i * 61) % height.saturating_sub(40).max(1));
        let color = Rgba([
            (20 + (i * 19) % 180) as u8,
            (30 + (i * 23) % 160) as u8,
            (40 + (i * 29) % 150) as u8,
            255,
        ]);
        for yy in y..(y + 9).min(height) {
            for xx in x..(x + 9).min(width) {
                if xx == x || yy == y || xx + 1 == x + 9 || yy + 1 == y + 9 || xx == x + yy - y {
                    img.put_pixel(xx, yy, color);
                }
            }
        }
    }

    img
}

/// A tall page with richly-textured text-like rows everywhere plus one large
/// product-image block spanning rows `[img_y0, img_y1)`. `image_loaded`
/// toggles whether that block is the real textured photo or a flat lazy-load
/// placeholder. Used to reproduce load-once lazy-load mutation between frames.
pub fn lazy_load_page(
    width: u32,
    height: u32,
    img_y0: u32,
    img_y1: u32,
    image_loaded: bool,
) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([250, 250, 250, 255]));
    for y in 0..height {
        if y >= img_y0 && y < img_y1 {
            continue;
        }
        let line = (y / 22) % 4;
        if line == 0 {
            for x in 30..width.saturating_sub(30) {
                if (x / 6 + y / 3) % 2 == 0 {
                    img.put_pixel(x, y, Rgba([40, 40, 40, 255]));
                }
            }
        } else if line == 1 && y % 22 < 3 {
            for x in 40..width.saturating_sub(120) {
                img.put_pixel(x, y, Rgba([70, 90, 160, 255]));
            }
        }
    }
    for y in img_y0..img_y1.min(height) {
        for x in 24..width.saturating_sub(24) {
            let px = if image_loaded {
                let r = (60 + ((x * 2 + y) % 160)) as u8;
                let g = (40 + ((x + y * 3) % 180)) as u8;
                let b = (90 + ((x * 3 + y * 2) % 150)) as u8;
                Rgba([r, g, b, 255])
            } else {
                Rgba([225, 225, 225, 255])
            };
            img.put_pixel(x, y, px);
        }
    }
    img
}
