use super::scoring::ExpectedRect;
use ab_glyph::{FontRef, PxScale};
use image::{Rgba, RgbaImage};
use imageproc::drawing::draw_text_mut;

pub(crate) struct RenderedFixture {
    pub image: RgbaImage,
    pub expected: Vec<ExpectedRect>,
}

const FONT_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../rollshot-image-document/assets/fonts/DejaVuSans.ttf"
));

fn font() -> FontRef<'static> {
    FontRef::try_from_slice(FONT_BYTES).expect("DejaVuSans font loads")
}

fn fill(img: &mut RgbaImage, color: [u8; 4]) {
    for px in img.pixels_mut() {
        *px = Rgba(color);
    }
}

fn draw_filled_rect(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: [u8; 4]) {
    let (iw, ih) = img.dimensions();
    for yy in y..(y + h).min(ih) {
        for xx in x..(x + w).min(iw) {
            img.put_pixel(xx, yy, Rgba(color));
        }
    }
}

fn draw_label(
    img: &mut RgbaImage,
    font: &FontRef<'static>,
    text: &str,
    x: u32,
    y: u32,
    px: f32,
    color: [u8; 4],
) {
    draw_text_mut(
        img,
        Rgba(color),
        x as i32,
        y as i32,
        PxScale::from(px),
        font,
        text,
    );
}

const PAGE_BG: [u8; 4] = [245, 245, 245, 255];
const CHROME_BG: [u8; 4] = [60, 63, 70, 255];
const FIELD_BG: [u8; 4] = [255, 255, 255, 255];
const TEXT_DARK: [u8; 4] = [20, 20, 20, 255];

/// A browser chrome with a single URL field carrying obviously-fake text.
pub(crate) fn render_url_bar() -> RenderedFixture {
    let font = font();
    let mut img = RgbaImage::new(800, 200);
    fill(&mut img, PAGE_BG);
    // toolbar
    draw_filled_rect(&mut img, 0, 0, 800, 56, CHROME_BG);
    // url field
    let (fx, fy, fw, fh) = (120u32, 14u32, 600u32, 28u32);
    draw_filled_rect(&mut img, fx, fy, fw, fh, FIELD_BG);
    draw_label(
        &mut img,
        &font,
        "https://example.com/u/secret-12345",
        fx + 8,
        fy + 4,
        20.0,
        TEXT_DARK,
    );
    RenderedFixture {
        image: img,
        expected: vec![ExpectedRect {
            x: fx as f32,
            y: fy as f32,
            width: fw as f32,
            height: fh as f32,
            label: "url_bar".into(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_bar_fixture_is_well_formed() {
        let f = render_url_bar();
        assert_eq!(f.image.dimensions(), (800, 200));
        assert_eq!(f.expected.len(), 1);
        let r = &f.expected[0];
        assert_eq!(r.label, "url_bar");
        // expected rect lies inside the image bounds
        assert!(r.x >= 0.0 && r.y >= 0.0);
        assert!(r.x + r.width <= 800.0 && r.y + r.height <= 200.0);
        // the bar region is not the same flat color as the page background
        let bg = f.image.get_pixel(5, 180);
        let bar = f
            .image
            .get_pixel((r.x + r.width / 2.0) as u32, (r.y + r.height / 2.0) as u32);
        assert_ne!(bg, bar);
    }
}
