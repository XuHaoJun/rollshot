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

/// Crops a viewport-sized frame from the canvas.
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
