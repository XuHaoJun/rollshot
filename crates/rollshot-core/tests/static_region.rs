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
fn sticky_header_masked_in_vertical_up_scroll_appends() {
    // Vertical-UP scroll: each new frame sees content higher on the page than
    // the previous, so the stitcher uses Top append (prepend). The sticky
    // header at frame y=[0, 12) therefore lands INSIDE each prepended slice
    // (top slice_px rows of the new frame). Without v0.2.1's static mask the
    // header would repeat every slice_px rows down the top of the stitched
    // canvas.
    //
    // We push enough frames (≥4 successful appends) so the detector locks
    // (default min_observations = 3 — locked after the 3rd observe call, the
    // 4th and later pushes append with the locked mask). We use the rich 320px
    // common canvas so NCC template matching reliably finds the motion, and
    // we loosen the verifier MAD thresholds because the painted header pattern
    // (red / dark-blue contrast) inflates overlap MAD even though the matcher's
    // content_roi already excludes the header band.
    let canvas = common::make_scroll_canvas(320, 1400);
    let mut config = StitchConfig::default();
    config.verifier.downsample_max_mad = 40.0 / 255.0;
    config.verifier.full_res_max_mad = 30.0 / 255.0;
    let mut stitcher = Stitcher::new(config);

    let frame_h: u32 = 320;
    let step: u32 = 70;
    let mut y = canvas.height() - frame_h;
    let mut appended = 0u32;
    loop {
        let mut f = common::crop_frame(&canvas, y, frame_h);
        paint_sticky_header(&mut f, 12);
        match stitcher.push_frame(f) {
            StitchOutcome::FirstFrame => {}
            StitchOutcome::Appended { .. } => appended += 1,
            other => panic!("unexpected outcome at y={y}: {other:?}"),
        }
        if y < step {
            break;
        }
        y -= step;
    }
    assert!(
        appended >= 4,
        "need ≥4 Top appends so detector locks before the last push; got {appended}"
    );

    let stitched = stitcher.full_image().expect("stitched output exists");

    // The most recently prepended slice sits at canvas y=0..slice_px. With
    // detector locked and top.thickness == 12, canvas y=0..12 must be the
    // sampled bg color, uniformly across every x. paint_sticky_header alternates
    // between (200, 60, 60) and (30, 30, 90); a successful mask replaces them
    // with one flat color.
    let row1: Vec<_> = (0..stitched.width())
        .map(|x| *stitched.get_pixel(x, 1))
        .collect();
    let first = row1[0];
    assert!(
        row1.iter().all(|p| p == &first),
        "row 1 inside the masked header zone should be one flat bg color across x; \
         first variant {first:?} vs others"
    );
    let header_red = Rgba([200, 60, 60, 255]);
    let header_dark_blue = Rgba([30, 30, 90, 255]);
    assert_ne!(
        first, header_red,
        "masked pixel should not be raw paint_sticky_header red"
    );
    assert_ne!(
        first, header_dark_blue,
        "masked pixel should not be raw paint_sticky_header dark-blue"
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
