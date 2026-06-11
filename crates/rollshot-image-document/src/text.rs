//! Text shaping, measurement, and rasterization via cosmic-text. The vendored
//! DejaVu fonts are the deterministic baseline; system fonts provide fallback
//! coverage (CJK etc.). Both the plate geometry (`shapes.rs`) and flattened
//! glyph raster use THIS module, so measured layout and drawn output agree.

use std::sync::{Mutex, OnceLock};

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, Weight};
use image::RgbaImage;

use crate::geometry::{ImagePoint, Rgba8};
use crate::raster::blend_px;
use crate::style;

struct TextSystem {
    fonts: FontSystem,
    cache: SwashCache,
}

fn system() -> &'static Mutex<TextSystem> {
    static SYSTEM: OnceLock<Mutex<TextSystem>> = OnceLock::new();
    SYSTEM.get_or_init(|| {
        let mut fonts = FontSystem::new();
        fonts
            .db_mut()
            .load_font_data(style::FONT_REGULAR_BYTES.to_vec());
        fonts
            .db_mut()
            .load_font_data(style::FONT_BOLD_BYTES.to_vec());
        Mutex::new(TextSystem {
            fonts,
            cache: SwashCache::new(),
        })
    })
}

fn attrs(bold: bool) -> Attrs<'static> {
    let attrs = Attrs::new().family(Family::Name(style::FONT_FAMILY_NAME));
    if bold {
        attrs.weight(Weight::BOLD)
    } else {
        attrs
    }
}

fn shaped_buffer(fonts: &mut FontSystem, _text: &str, px: f32) -> Buffer {
    let metrics = Metrics::new(px, px * style::TEXT_LINE_HEIGHT);
    let mut buffer = Buffer::new(fonts, metrics);
    buffer.set_size(fonts, None, None);
    buffer
}

/// Measure a text block (lines split on `\n`, no soft wrapping).
/// Returns `(max_line_width, total_height)` in image pixels.
pub fn measure_block(text: &str, px: f32, bold: bool) -> (f32, f32) {
    let mut sys = system().lock().expect("text system poisoned");
    let TextSystem { fonts, .. } = &mut *sys;
    let mut buffer = shaped_buffer(fonts, text, px);
    buffer.set_text(fonts, text, &attrs(bold), Shaping::Advanced, None);
    buffer.shape_until_scroll(fonts, false);

    let mut width: f32 = 0.0;
    let mut lines: usize = 0;
    for run in buffer.layout_runs() {
        width = width.max(run.line_w);
        lines += 1;
    }
    (width, lines.max(1) as f32 * px * style::TEXT_LINE_HEIGHT)
}

/// Rasterize a text block onto `img` with its top-left at `top_left`.
pub(crate) fn draw_block(
    img: &mut RgbaImage,
    top_left: ImagePoint,
    text: &str,
    px: f32,
    bold: bool,
    color: Rgba8,
) {
    let mut sys = system().lock().expect("text system poisoned");
    let TextSystem { fonts, cache } = &mut *sys;
    let mut buffer = shaped_buffer(fonts, text, px);
    buffer.set_text(fonts, text, &attrs(bold), Shaping::Advanced, None);
    buffer.shape_until_scroll(fonts, false);

    let base = cosmic_text::Color::rgba(color.r, color.g, color.b, color.a);
    let (ox, oy) = (top_left.x.round() as i32, top_left.y.round() as i32);
    buffer.draw(fonts, cache, base, |x, y, w, h, c| {
        let alpha = (c.0 >> 24) as u8;
        if alpha == 0 {
            return;
        }
        let px_color = Rgba8::new(c.r(), c.g(), c.b(), alpha);
        for dy in 0..h as i32 {
            for dx in 0..w as i32 {
                blend_px(img, ox + x + dx, oy + y + dy, px_color, 1.0);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{ImagePoint, Rgba8};
    use image::RgbaImage;

    #[test]
    fn measure_is_positive_and_grows_with_text() {
        let (w1, h1) = measure_block("1", 20.0, true);
        let (w2, _) = measure_block("10", 20.0, true);
        assert!(w1 > 0.0 && h1 > 0.0);
        assert!(w2 > w1);
    }

    #[test]
    fn multiline_is_taller_and_width_is_max_line() {
        let (w1, h1) = measure_block("hello", 18.0, false);
        let (w2, h2) = measure_block("hello\nhi", 18.0, false);
        assert!(h2 > h1);
        assert!((w2 - w1).abs() < 1.0, "width should match the longest line");
    }

    #[test]
    fn draw_block_blends_pixels_into_image() {
        let mut img = RgbaImage::from_pixel(60, 40, image::Rgba([0, 0, 0, 255]));
        draw_block(
            &mut img,
            ImagePoint::new(2.0, 2.0),
            "Hi",
            18.0,
            false,
            Rgba8::new(255, 255, 255, 255),
        );
        let changed = img.pixels().filter(|p| p.0 != [0, 0, 0, 255]).count();
        assert!(changed > 10, "expected glyph pixels, got {changed}");
    }
}
