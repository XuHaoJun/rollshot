mod common;

use common::{
    paint_sticky_footer, paint_sticky_header, paint_sticky_horizontal_band, paint_sticky_sidebar,
    Side,
};
use image::{Rgba, RgbaImage};
use rollshot_core::{StitchConfig, StitchOutcome, Stitcher};

fn make_scroll_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([240, 240, 240, 255]));
    for y in 0..height {
        for x in 0..width {
            if (x / 4 + y / 6) % 2 == 0 {
                img.put_pixel(
                    x,
                    y,
                    Rgba([30, ((x * 7) % 200) as u8, ((y * 11) % 200) as u8, 255]),
                );
            }
        }
    }
    img
}

fn crop(canvas: &RgbaImage, y: u32, h: u32) -> RgbaImage {
    image::imageops::crop_imm(canvas, 0, y, canvas.width(), h).to_image()
}

fn drive_vertical(
    stitcher: &mut Stitcher,
    canvas: &RgbaImage,
    frame_h: u32,
    step: u32,
    paint: impl Fn(&mut RgbaImage),
) {
    let mut y = 0;
    while y + frame_h <= canvas.height() {
        let mut f = crop(canvas, y, frame_h);
        paint(&mut f);
        stitcher.push_frame(f);
        y += step;
    }
}

#[test]
fn sticky_left_sidebar_does_not_duplicate_in_output() {
    let canvas = make_scroll_canvas(120, 600);
    let mut stitcher = Stitcher::new(StitchConfig::default());
    drive_vertical(&mut stitcher, &canvas, 160, 40, |f| {
        paint_sticky_sidebar(f, Side::Left, 12)
    });

    let stitched = stitcher.full_image().expect("stitched output exists");
    let first_frame_pixel = stitched.get_pixel(0, 0);
    assert_ne!(
        first_frame_pixel,
        &Rgba([240, 240, 240, 255]),
        "first frame's sidebar must be preserved"
    );
    let later_pixel = stitched.get_pixel(0, stitched.height() - 1);
    let gray = later_pixel[0];
    assert!(
        gray == 100 || gray == 140,
        "left-edge pixel at canvas bottom = {gray:?}"
    );
    assert_eq!(later_pixel[0], later_pixel[1], "bg should be gray (R==G)");
    assert_eq!(later_pixel[1], later_pixel[2], "bg should be gray (G==B)");
}

#[test]
fn sticky_right_sidebar_does_not_duplicate_in_output() {
    let canvas = make_scroll_canvas(120, 600);
    let mut stitcher = Stitcher::new(StitchConfig::default());
    drive_vertical(&mut stitcher, &canvas, 160, 40, |f| {
        paint_sticky_sidebar(f, Side::Right, 12)
    });

    let stitched = stitcher.full_image().expect("stitched output exists");
    let w = stitched.width();
    let later_pixel = stitched.get_pixel(w - 1, stitched.height() - 1);
    let gray = later_pixel[0];
    assert!(gray == 100 || gray == 140, "right-edge bg gray = {gray:?}");
}

#[test]
fn sticky_footer_does_not_duplicate_in_output() {
    let canvas = make_scroll_canvas(120, 600);
    let mut stitcher = Stitcher::new(StitchConfig::default());
    drive_vertical(&mut stitcher, &canvas, 160, 40, |f| {
        paint_sticky_footer(f, 12)
    });

    let stitched = stitcher.full_image().expect("stitched output exists");
    let later_pixel = stitched.get_pixel(stitched.width() / 2, stitched.height() - 1);
    let gray = later_pixel[0];
    assert!(gray == 110 || gray == 150, "footer-edge bg gray = {gray:?}");
}

#[test]
fn sticky_header_output_is_clean_after_first_frame() {
    // Use the rich common canvas at 320px width so NCC template matching
    // reliably finds dy≈80. The simple 120px canvas struggles with the
    // bright full-width sticky header confusing the matcher.
    let canvas = common::make_scroll_canvas(320, 1400);
    let mut config = StitchConfig::default();
    config.verifier.downsample_max_mad = 40.0 / 255.0;
    config.verifier.full_res_max_mad = 30.0 / 255.0;
    let mut stitcher = Stitcher::new(config);

    let mut first = common::crop_frame(&canvas, 0, 320);
    paint_sticky_header(&mut first, 12);
    assert!(matches!(
        stitcher.push_frame(first),
        StitchOutcome::FirstFrame
    ));

    let mut scrolled = common::crop_frame(&canvas, 70, 320);
    paint_sticky_header(&mut scrolled, 12);
    let outcome = stitcher.push_frame(scrolled);
    assert!(
        matches!(outcome, StitchOutcome::Appended { .. }),
        "expected Appended, got {outcome:?}"
    );

    let stitched = stitcher.full_image().expect("stitched output exists");
    let h = stitched.height();
    assert!(
        h > 200,
        "stitched image should be taller than 200px, got {h}"
    );
    let mid_pixel = stitched.get_pixel(50, 216);
    assert!(
        mid_pixel[0] != mid_pixel[1] || mid_pixel[1] != mid_pixel[2],
        "row 216 should be scrollable content, got {mid_pixel:?}"
    );
}

#[test]
fn first_frame_keeps_sticky_pixels_verbatim() {
    let canvas = make_scroll_canvas(120, 600);
    let mut stitcher = Stitcher::new(StitchConfig::default());

    let mut first = crop(&canvas, 0, 160);
    paint_sticky_sidebar(&mut first, Side::Left, 8);
    let expected_first = first.clone();
    let outcome = stitcher.push_frame(first);
    assert!(matches!(outcome, StitchOutcome::FirstFrame));

    drive_vertical(&mut stitcher, &canvas, 160, 40, |f| {
        paint_sticky_sidebar(f, Side::Left, 8)
    });

    let stitched = stitcher.full_image().expect("stitched output");
    for y in 0..160 {
        for x in 0..120 {
            assert_eq!(
                stitched.get_pixel(x, y),
                expected_first.get_pixel(x, y),
                "first-frame pixel mismatch at ({x}, {y})"
            );
        }
    }
}

fn make_wide_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([240, 240, 240, 255]));
    for x in 0..width {
        for y in 0..height {
            if (x / 4 + y / 6) % 2 == 0 {
                img.put_pixel(
                    x,
                    y,
                    Rgba([((x * 7) % 200) as u8, 30, ((y * 11) % 200) as u8, 255]),
                );
            }
        }
    }
    img
}

fn crop_xy(canvas: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
    image::imageops::crop_imm(canvas, x, y, w, h).to_image()
}

fn drive_horizontal(
    stitcher: &mut Stitcher,
    canvas: &RgbaImage,
    frame_w: u32,
    step: u32,
    paint: impl Fn(&mut RgbaImage),
) {
    let mut x = 0;
    while x + frame_w <= canvas.width() {
        let mut f = crop_xy(canvas, x, 0, frame_w, canvas.height());
        paint(&mut f);
        stitcher.push_frame(f);
        x += step;
    }
}

#[test]
fn horizontal_scroll_with_sticky_top_band() {
    let canvas = make_wide_canvas(600, 120);
    let mut stitcher = Stitcher::new(StitchConfig::default());
    drive_horizontal(&mut stitcher, &canvas, 160, 40, |f| {
        paint_sticky_horizontal_band(f, 10, 0)
    });

    let stitched = stitcher.full_image().expect("stitched output");
    let later_pixel = stitched.get_pixel(250, 0);
    let gray = later_pixel[0];
    assert!(gray == 90 || gray == 130, "top-band bg gray = {gray:?}");
    assert_eq!(later_pixel[0], later_pixel[1]);
    assert_eq!(later_pixel[1], later_pixel[2]);
}

#[test]
fn horizontal_scroll_with_sticky_bottom_band() {
    let canvas = make_wide_canvas(600, 120);
    let mut stitcher = Stitcher::new(StitchConfig::default());
    drive_horizontal(&mut stitcher, &canvas, 160, 40, |f| {
        paint_sticky_horizontal_band(f, 0, 8)
    });

    let stitched = stitcher.full_image().expect("stitched output");
    let h = stitched.height();
    let later_pixel = stitched.get_pixel(250, h - 1);
    let gray = later_pixel[0];
    assert!(gray == 95 || gray == 135, "bottom-band bg gray = {gray:?}");
}

fn disabled_config() -> StitchConfig {
    let mut cfg = StitchConfig::default();
    cfg.static_region.enabled = false;
    cfg
}

#[test]
fn detector_disabled_via_config_reproduces_legacy_pixel_for_pixel() {
    let canvas = make_scroll_canvas(120, 600);

    let mut s_on = Stitcher::new(StitchConfig::default());
    let mut s_off = Stitcher::new(disabled_config());

    drive_vertical(&mut s_on, &canvas, 160, 40, |f| {
        paint_sticky_sidebar(f, Side::Left, 12)
    });
    drive_vertical(&mut s_off, &canvas, 160, 40, |f| {
        paint_sticky_sidebar(f, Side::Left, 12)
    });

    let on = s_on.full_image().expect("on output");
    let off = s_off.full_image().expect("off output");
    assert_eq!(on.dimensions(), off.dimensions());

    let mut differs = false;
    for y in 160..on.height() {
        for x in 0..12 {
            if on.get_pixel(x, y) != off.get_pixel(x, y) {
                differs = true;
                break;
            }
        }
        if differs {
            break;
        }
    }
    assert!(
        differs,
        "with detector ON some sidebar pixel in appended slices must differ from v0.2"
    );
}

#[test]
fn no_sticky_baseline_output_byte_identical_to_disabled_config() {
    let canvas = make_scroll_canvas(120, 600);

    let mut s_on = Stitcher::new(StitchConfig::default());
    let mut s_off = Stitcher::new(disabled_config());

    drive_vertical(&mut s_on, &canvas, 160, 40, |_| {});
    drive_vertical(&mut s_off, &canvas, 160, 40, |_| {});

    let on = s_on.full_image().expect("on output");
    let off = s_off.full_image().expect("off output");
    assert_eq!(on.dimensions(), off.dimensions());
    assert_eq!(
        on.as_raw(),
        off.as_raw(),
        "default config must be byte-identical to disabled on pure-scroll input"
    );
}
