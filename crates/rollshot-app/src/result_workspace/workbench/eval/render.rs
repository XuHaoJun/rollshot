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

const ROW_BG: [u8; 4] = [255, 255, 255, 255];
const ROW_ALT: [u8; 4] = [232, 235, 240, 255];
const ACCENT: [u8; 4] = [70, 110, 200, 255];

fn rows(
    img: &mut RgbaImage,
    font: &FontRef<'static>,
    x: u32,
    top: u32,
    labels: &[&str],
    rh: u32,
) -> Vec<ExpectedRect> {
    let mut out = Vec::new();
    for (i, text) in labels.iter().enumerate() {
        let y = top + i as u32 * (rh + 8);
        let bg = if i % 2 == 0 { ROW_BG } else { ROW_ALT };
        draw_filled_rect(img, x, y, 360, rh, bg);
        draw_label(img, font, text, x + 8, y + 4, 18.0, TEXT_DARK);
        out.push(ExpectedRect {
            x: x as f32,
            y: y as f32,
            width: 360.0,
            height: rh as f32,
            label: format!("row_{i}"),
        });
    }
    out
}

pub(crate) fn render_bookmarks() -> RenderedFixture {
    let font = font();
    let mut img = RgbaImage::new(420, 220);
    fill(&mut img, PAGE_BG);
    draw_filled_rect(&mut img, 0, 0, 420, 30, ACCENT);
    let expected = rows(
        &mut img,
        &font,
        20,
        50,
        &[
            "Bookmark: secret-project-roadmap",
            "Bookmark: payroll-q3-internal",
            "Bookmark: vpn-admin-console",
        ],
        28,
    );
    RenderedFixture {
        image: img,
        expected,
    }
}

pub(crate) fn render_desktop_folders() -> RenderedFixture {
    let font = font();
    let mut img = RgbaImage::new(480, 360);
    fill(&mut img, [40, 80, 60, 255]);
    let mut expected = Vec::new();
    let names = ["Taxes 2025", "Client NDA", "Passwords", "HR Cases"];
    for (i, name) in names.iter().enumerate() {
        let col = (i % 2) as u32;
        let row = (i / 2) as u32;
        let x = 40 + col * 220;
        let y = 40 + row * 150;
        draw_filled_rect(&mut img, x, y, 80, 64, [230, 200, 120, 255]);
        draw_filled_rect(&mut img, x, y + 70, 180, 26, [0, 0, 0, 160]);
        draw_label(
            &mut img,
            &font,
            name,
            x + 4,
            y + 72,
            18.0,
            [255, 255, 255, 255],
        );
        expected.push(ExpectedRect {
            x: x as f32,
            y: (y + 70) as f32,
            width: 180.0,
            height: 26.0,
            label: format!("folder_{i}"),
        });
    }
    RenderedFixture {
        image: img,
        expected,
    }
}

pub(crate) fn render_emails() -> RenderedFixture {
    let font = font();
    let mut img = RgbaImage::new(420, 200);
    fill(&mut img, PAGE_BG);
    let expected = rows(
        &mut img,
        &font,
        20,
        20,
        &[
            "ada.fake@example.com",
            "grace.test@example.org",
            "alan.sample@example.net",
        ],
        28,
    );
    RenderedFixture {
        image: img,
        expected,
    }
}

pub(crate) fn render_names() -> RenderedFixture {
    let font = font();
    let mut img = RgbaImage::new(420, 200);
    fill(&mut img, PAGE_BG);
    let expected = rows(
        &mut img,
        &font,
        20,
        20,
        &[
            "Name: Ada Placeholder",
            "Name: Grace Sample",
            "Name: Alan Example",
        ],
        28,
    );
    RenderedFixture {
        image: img,
        expected,
    }
}

pub(crate) fn render_account_ids() -> RenderedFixture {
    let font = font();
    let mut img = RgbaImage::new(420, 200);
    fill(&mut img, PAGE_BG);
    let expected = rows(
        &mut img,
        &font,
        20,
        20,
        &[
            "Account: ACME-0000-1111",
            "Account: ACME-2222-3333",
            "Account: ACME-4444-5555",
        ],
        28,
    );
    RenderedFixture {
        image: img,
        expected,
    }
}

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
    fn all_five_remaining_fixtures_are_well_formed() {
        let cases: Vec<(RenderedFixture, usize)> = vec![
            (render_bookmarks(), 3),
            (render_desktop_folders(), 4),
            (render_emails(), 3),
            (render_names(), 3),
            (render_account_ids(), 3),
        ];
        for (f, want) in cases {
            let (w, h) = f.image.dimensions();
            assert!(w > 0 && h > 0);
            assert_eq!(f.expected.len(), want);
            for r in &f.expected {
                assert!(r.x >= 0.0 && r.y >= 0.0);
                assert!(r.x + r.width <= w as f32 && r.y + r.height <= h as f32);
                assert!(!r.label.is_empty());
            }
        }
    }

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
