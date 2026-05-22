//! v0.3 overlap-and-overwrite topology integration tests.
//!
//! The four `pure_scroll_*_byte_identical_to_v0_2` tests assert that the
//! stitched canvas equals the original source verbatim. For pure-scroll
//! fixtures (no per-frame variation, no sticky UI), this is algebraically
//! equivalent to byte-identity with v0.2's stitched output, since v0.2's
//! minimal-slice append over a pure scroll also reconstructs the source.
//! Any drift indicates a bug in the new overlap-and-overwrite slice math.

mod common;

use common::{
    crop_frame, crop_frame_xy, make_scroll_canvas, make_wide_canvas,
    paint_decorative_bottom_border, paint_sidebar_icon_at, paint_sticky_footer,
    paint_sticky_header, paint_sticky_horizontal_band,
};
use image::{Rgba, RgbaImage};
use rollshot_core::{AppendDirection, LinearCanvas, StitchConfig, StitchOutcome, Stitcher};

#[test]
fn pure_scroll_down_byte_identical_to_v0_2() {
    let source = make_scroll_canvas(320, 1400);
    let frame_w = 320u32;
    let frame_h = 320u32;
    let step = 70u32;

    let first = crop_frame_xy(&source, 0, 0, frame_w, frame_h);
    let mut canvas = LinearCanvas::new(first);
    let mut expected_h = frame_h;

    let mut y = step;
    while y + frame_h <= source.height() && expected_h < 700 {
        let f = crop_frame_xy(&source, 0, y, frame_w, frame_h);
        let added = canvas
            .append(AppendDirection::Bottom, &f, step)
            .expect("append");
        assert_eq!(added, step);
        expected_h += step;
        y += step;
    }

    let stitched = canvas.image();
    assert_eq!(stitched.height(), expected_h);
    for cy in 0..stitched.height() {
        for cx in 0..stitched.width() {
            assert_eq!(
                stitched.get_pixel(cx, cy),
                source.get_pixel(cx, cy),
                "scroll-down pixel mismatch at ({cx}, {cy})",
            );
        }
    }
}

#[test]
fn pure_scroll_up_byte_identical_to_v0_2() {
    let source = make_scroll_canvas(320, 1400);
    let frame_w = 320u32;
    let frame_h = 320u32;
    let step = 70u32;
    let initial_y_start = 700u32;

    let first = crop_frame_xy(&source, 0, initial_y_start, frame_w, frame_h);
    let mut canvas = LinearCanvas::new(first);
    let mut top_y = initial_y_start;

    let mut prepended = 0u32;
    while top_y >= step && prepended < 700 {
        top_y -= step;
        let f = crop_frame_xy(&source, 0, top_y, frame_w, frame_h);
        let added = canvas
            .append(AppendDirection::Top, &f, step)
            .expect("prepend");
        assert_eq!(added, step);
        prepended += step;
    }

    let stitched = canvas.image();
    let bottom_y = initial_y_start + frame_h;
    let expected_h = bottom_y - top_y;
    assert_eq!(stitched.height(), expected_h);

    for cy in 0..stitched.height() {
        for cx in 0..stitched.width() {
            assert_eq!(
                stitched.get_pixel(cx, cy),
                source.get_pixel(cx, top_y + cy),
                "scroll-up pixel mismatch at canvas ({cx}, {cy}) -> source ({cx}, {})",
                top_y + cy,
            );
        }
    }
}

#[test]
fn pure_scroll_right_byte_identical_to_v0_2() {
    let source = make_wide_canvas(1400, 320);
    let frame_w = 320u32;
    let frame_h = 320u32;
    let step = 70u32;

    let first = crop_frame_xy(&source, 0, 0, frame_w, frame_h);
    let mut canvas = LinearCanvas::new(first);
    let mut expected_w = frame_w;

    let mut x = step;
    while x + frame_w <= source.width() && expected_w < 700 {
        let f = crop_frame_xy(&source, x, 0, frame_w, frame_h);
        let added = canvas
            .append(AppendDirection::Right, &f, step)
            .expect("append");
        assert_eq!(added, step);
        expected_w += step;
        x += step;
    }

    let stitched = canvas.image();
    assert_eq!(stitched.width(), expected_w);
    for cy in 0..stitched.height() {
        for cx in 0..stitched.width() {
            assert_eq!(
                stitched.get_pixel(cx, cy),
                source.get_pixel(cx, cy),
                "scroll-right pixel mismatch at ({cx}, {cy})",
            );
        }
    }
}

#[test]
fn pure_scroll_left_byte_identical_to_v0_2() {
    let source = make_wide_canvas(1400, 320);
    let frame_w = 320u32;
    let frame_h = 320u32;
    let step = 70u32;
    let initial_x_start = 700u32;

    let first = crop_frame_xy(&source, initial_x_start, 0, frame_w, frame_h);
    let mut canvas = LinearCanvas::new(first);
    let mut left_x = initial_x_start;

    let mut prepended = 0u32;
    while left_x >= step && prepended < 700 {
        left_x -= step;
        let f = crop_frame_xy(&source, left_x, 0, frame_w, frame_h);
        let added = canvas
            .append(AppendDirection::Left, &f, step)
            .expect("prepend");
        assert_eq!(added, step);
        prepended += step;
    }

    let stitched = canvas.image();
    let right_x = initial_x_start + frame_w;
    let expected_w = right_x - left_x;
    assert_eq!(stitched.width(), expected_w);

    for cy in 0..stitched.height() {
        for cx in 0..stitched.width() {
            assert_eq!(
                stitched.get_pixel(cx, cy),
                source.get_pixel(left_x + cx, cy),
                "scroll-left pixel mismatch at canvas ({cx}, {cy}) -> source ({}, {cy})",
                left_x + cx,
            );
        }
    }
}

#[test]
fn first_frame_preserved_verbatim() {
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;
    let mut stitcher = Stitcher::new(StitchConfig::default());

    let mut first = crop_frame(&source, 0, frame_h);
    paint_sticky_header(&mut first, 12);
    let expected_first = first.clone();

    let outcome = stitcher.push_frame(first);
    assert!(matches!(outcome, StitchOutcome::FirstFrame));

    let mut y = step;
    let mut appended = 0u32;
    while y + frame_h <= source.height() && appended < 3 {
        let mut f = crop_frame(&source, y, frame_h);
        paint_sticky_header(&mut f, 12);
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended += 1;
        }
        y += step;
    }
    assert!(appended >= 2, "need >=2 appends; got {appended}");

    let stitched = stitcher
        .full_image()
        .expect("stitched output exists")
        .clone();

    let preserved_until = frame_h / 2;
    for y in 0..preserved_until {
        for x in 0..stitched.width() {
            assert_eq!(
                stitched.get_pixel(x, y),
                expected_first.get_pixel(x, y),
                "first-frame pixel changed at ({x}, {y}); should be preserved before y={preserved_until}",
            );
        }
    }
}

#[test]
fn sticky_header_appears_only_at_canvas_top() {
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;

    let mut config = StitchConfig::default();
    config.verifier.downsample_max_mad = 40.0 / 255.0;
    config.verifier.full_res_max_mad = 30.0 / 255.0;
    let mut stitcher = Stitcher::new(config);

    let mut y = 0;
    let mut appended = 0u32;
    while y + frame_h <= source.height() {
        let mut f = crop_frame(&source, y, frame_h);
        paint_sticky_header(&mut f, 12);
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended += 1;
        }
        y += step;
    }
    assert!(appended >= 3, "need >=3 appends; got {appended}");

    let stitched = stitcher.full_image().expect("stitched");

    let header_red = Rgba([200, 60, 60, 255]);
    let header_dark_blue = Rgba([30, 30, 90, 255]);
    let mut saw_header_at_top = false;
    for x in 0..stitched.width() {
        let p = *stitched.get_pixel(x, 0);
        if p == header_red || p == header_dark_blue {
            saw_header_at_top = true;
            break;
        }
    }
    assert!(
        saw_header_at_top,
        "frame 1's header must remain at canvas top"
    );

    let probe_y = frame_h + step + 1;
    if probe_y < stitched.height() {
        for x in 0..stitched.width() {
            let p = *stitched.get_pixel(x, probe_y);
            assert!(
                p != header_red && p != header_dark_blue,
                "header color leaked to canvas y={probe_y} at x={x}: {p:?}",
            );
        }
    }
}

#[test]
fn sticky_footer_only_at_canvas_bottom() {
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;
    let mut stitcher = Stitcher::new(StitchConfig::default());

    let mut y = 0;
    let mut appended = 0u32;
    while y + frame_h <= source.height() {
        let mut f = crop_frame(&source, y, frame_h);
        paint_sticky_footer(&mut f, 12);
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended += 1;
        }
        y += step;
    }
    assert!(appended >= 3, "need >=3 appends; got {appended}");

    let stitched = stitcher.full_image().expect("stitched");
    let h = stitched.height();

    let bottom = *stitched.get_pixel(stitched.width() / 2, h - 1);
    assert!(
        bottom[0] == 110 || bottom[0] == 150,
        "bottom row should be a footer color; got {bottom:?}",
    );
    assert_eq!(bottom[0], bottom[1]);
    assert_eq!(bottom[1], bottom[2]);

    let probe_y = frame_h;
    if probe_y < h {
        let mut saw_footer = false;
        for x in 0..stitched.width() {
            let p = *stitched.get_pixel(x, probe_y);
            if p[0] == p[1] && p[1] == p[2] && (p[0] == 110 || p[0] == 150) {
                saw_footer = true;
                break;
            }
        }
        assert!(
            !saw_footer,
            "footer leaked to canvas y={probe_y} in the middle of the canvas",
        );
    }
}

#[test]
fn decorative_1px_bottom_border_only_at_canvas_bottom() {
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;
    let border_color = Rgba([160, 160, 160, 255]);
    let mut stitcher = Stitcher::new(StitchConfig::default());

    let mut y = 0;
    let mut appended = 0u32;
    while y + frame_h <= source.height() {
        let mut f = crop_frame(&source, y, frame_h);
        paint_decorative_bottom_border(&mut f, border_color);
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended += 1;
        }
        y += step;
    }
    assert!(appended >= 4, "need >=4 appends; got {appended}");

    let stitched = stitcher.full_image().expect("stitched");
    let h = stitched.height();
    let w = stitched.width();

    for x in 0..w {
        let p = *stitched.get_pixel(x, h - 1);
        assert_eq!(p, border_color, "bottom row must be border at x={x}");
    }

    let mut border_seen_at_y: Vec<u32> = Vec::new();
    for y in 0..h - 1 {
        for x in 0..w {
            if *stitched.get_pixel(x, y) == border_color {
                border_seen_at_y.push(y);
                break;
            }
        }
    }
    assert!(
        border_seen_at_y.is_empty(),
        "decorative border found at unexpected canvas rows: {border_seen_at_y:?}",
    );
}

#[test]
fn sticky_header_after_scroll_up_appears_only_once() {
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;
    let mut config = StitchConfig::default();
    config.verifier.downsample_max_mad = 40.0 / 255.0;
    config.verifier.full_res_max_mad = 30.0 / 255.0;
    let mut stitcher = Stitcher::new(config);

    let mut y = source.height() - frame_h;
    let mut appended = 0u32;
    loop {
        let mut f = crop_frame(&source, y, frame_h);
        paint_sticky_header(&mut f, 12);
        match stitcher.push_frame(f) {
            StitchOutcome::FirstFrame => {}
            StitchOutcome::Appended { .. } => appended += 1,
            _ => {}
        }
        if y < step {
            break;
        }
        y -= step;
    }
    assert!(appended >= 3, "need >=3 prepends; got {appended}");

    let stitched = stitcher.full_image().expect("stitched");

    let header_red = Rgba([200, 60, 60, 255]);
    let header_dark_blue = Rgba([30, 30, 90, 255]);
    let mut saw_header = false;
    for x in 0..stitched.width() {
        let p = *stitched.get_pixel(x, 0);
        if p == header_red || p == header_dark_blue {
            saw_header = true;
            break;
        }
    }
    assert!(
        saw_header,
        "header must appear at canvas top after scroll-up"
    );

    let probe_start = step;
    for y in probe_start..stitched.height() {
        let mut saw = false;
        for x in 0..stitched.width() {
            let p = *stitched.get_pixel(x, y);
            if p == header_red || p == header_dark_blue {
                saw = true;
                break;
            }
        }
        assert!(
            !saw,
            "header leaked to canvas y={y} after scroll-up prepend",
        );
    }
}

#[test]
fn bidirectional_scroll_down_then_up_canvas_consistent() {
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;
    let mut stitcher = Stitcher::new(StitchConfig::default());

    let mut anchor = 500u32;
    let mut appended_total = 0u32;

    let mut f = crop_frame(&source, anchor, frame_h);
    paint_sticky_footer(&mut f, 8);
    stitcher.push_frame(f);

    for _ in 0..3 {
        anchor += step;
        let mut f = crop_frame(&source, anchor, frame_h);
        paint_sticky_footer(&mut f, 8);
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended_total += 1;
        }
    }

    let mut up_anchor = 500u32;
    for _ in 0..3 {
        if up_anchor < step {
            break;
        }
        up_anchor -= step;
        let mut f = crop_frame(&source, up_anchor, frame_h);
        paint_sticky_footer(&mut f, 8);
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended_total += 1;
        }
    }

    assert!(
        appended_total >= 4,
        "need >=4 appends across both directions; got {appended_total}",
    );

    let stitched = stitcher.full_image().expect("stitched");
    assert!(
        stitched.height() > frame_h,
        "canvas height should exceed single frame_h after bidirectional scroll",
    );

    let h = stitched.height();
    let bottom = *stitched.get_pixel(stitched.width() / 2, h - 1);
    assert!(
        bottom[0] == 110 || bottom[0] == 150,
        "canvas bottom should still be a footer color after bidirectional scroll; got {bottom:?}",
    );
}

#[test]
fn solid_sidebar_renders_as_continuous_column() {
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;
    let sidebar_color = Rgba([50, 60, 70, 255]);
    let sidebar_w = 12u32;
    let mut stitcher = Stitcher::new(StitchConfig::default());

    let mut y = 0;
    let mut appended = 0u32;
    while y + frame_h <= source.height() {
        let mut f = crop_frame(&source, y, frame_h);
        for fy in 0..f.height() {
            for fx in 0..sidebar_w.min(f.width()) {
                f.put_pixel(fx, fy, sidebar_color);
            }
        }
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended += 1;
        }
        y += step;
    }
    assert!(appended >= 3, "need >=3 appends; got {appended}");

    let stitched = stitcher.full_image().expect("stitched");
    for cy in 0..stitched.height() {
        for cx in 0..sidebar_w {
            assert_eq!(
                stitched.get_pixel(cx, cy),
                &sidebar_color,
                "solid sidebar should be continuous at ({cx}, {cy})",
            );
        }
    }
}

#[test]
fn top_anchored_sidebar_icon_preserved_from_first_frame() {
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;
    let sidebar_w = 12u32;
    let icon_color = Rgba([255, 128, 0, 255]);
    let icon_y = 20u32;
    let icon_h = 20u32;
    let mut stitcher = Stitcher::new(StitchConfig::default());

    let mut y = 0;
    let mut appended = 0u32;
    while y + frame_h <= source.height() {
        let mut f = crop_frame(&source, y, frame_h);
        for fy in 0..f.height() {
            for fx in 0..sidebar_w.min(f.width()) {
                f.put_pixel(fx, fy, Rgba([50, 60, 70, 255]));
            }
        }
        paint_sidebar_icon_at(&mut f, sidebar_w, icon_y, icon_h, icon_color);
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended += 1;
        }
        y += step;
    }
    assert!(appended >= 3, "need >=3 appends; got {appended}");

    let stitched = stitcher.full_image().expect("stitched");

    for cy in icon_y..icon_y + icon_h {
        for cx in 0..sidebar_w {
            assert_eq!(
                stitched.get_pixel(cx, cy),
                &icon_color,
                "icon missing at ({cx}, {cy})",
            );
        }
    }

    for cy in 0..stitched.height() {
        if cy >= icon_y && cy < icon_y + icon_h {
            continue;
        }
        for cx in 0..sidebar_w {
            assert!(
                stitched.get_pixel(cx, cy) != &icon_color,
                "icon leaked to ({cx}, {cy}) — top-anchored icon should appear only at frame 1's position",
            );
        }
    }
}

#[test]
fn bottom_anchored_sidebar_icon_only_at_canvas_bottom() {
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;
    let sidebar_w = 12u32;
    let icon_color = Rgba([0, 200, 200, 255]);
    let icon_h = 20u32;
    let icon_y = frame_h - icon_h;
    let mut stitcher = Stitcher::new(StitchConfig::default());

    let mut y = 0;
    let mut appended = 0u32;
    while y + frame_h <= source.height() {
        let mut f = crop_frame(&source, y, frame_h);
        for fy in 0..f.height() {
            for fx in 0..sidebar_w.min(f.width()) {
                f.put_pixel(fx, fy, Rgba([50, 60, 70, 255]));
            }
        }
        paint_sidebar_icon_at(&mut f, sidebar_w, icon_y, icon_h, icon_color);
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended += 1;
        }
        y += step;
    }
    assert!(appended >= 3, "need >=3 appends; got {appended}");

    let stitched = stitcher.full_image().expect("stitched");
    let h = stitched.height();

    for cy in h - icon_h..h {
        for cx in 0..sidebar_w {
            assert_eq!(
                stitched.get_pixel(cx, cy),
                &icon_color,
                "icon missing at ({cx}, {cy}) at canvas bottom",
            );
        }
    }

    for cy in 0..h - icon_h {
        for cx in 0..sidebar_w {
            assert!(
                stitched.get_pixel(cx, cy) != &icon_color,
                "icon leaked to ({cx}, {cy}) — bottom-anchored should only appear at canvas bottom",
            );
        }
    }
}

fn drive_horizontal_right(
    canvas: &RgbaImage,
    frame_w: u32,
    step: u32,
    mut paint: impl FnMut(&mut RgbaImage),
) -> RgbaImage {
    let mut stitcher = Stitcher::new(StitchConfig::default());
    let mut x = 0;
    while x + frame_w <= canvas.width() {
        let mut f = crop_frame_xy(canvas, x, 0, frame_w, canvas.height());
        paint(&mut f);
        stitcher.push_frame(f);
        x += step;
    }
    stitcher
        .full_image()
        .expect("stitched output exists")
        .clone()
}

#[test]
fn horizontal_scroll_with_sticky_top_band() {
    let source = make_wide_canvas(1400, 320);
    let frame_w = 320u32;
    let step = 70u32;

    let stitched = drive_horizontal_right(&source, frame_w, step, |f| {
        paint_sticky_horizontal_band(f, 10, 0);
    });

    for x in 0..stitched.width() {
        let p = *stitched.get_pixel(x, 0);
        assert!(
            (p[0] == 90 || p[0] == 130) && p[0] == p[1] && p[1] == p[2],
            "top band missing at ({x}, 0): {p:?}",
        );
    }
}

#[test]
fn horizontal_scroll_with_sticky_bottom_band() {
    let source = make_wide_canvas(1400, 320);
    let frame_w = 320u32;
    let step = 70u32;

    let stitched = drive_horizontal_right(&source, frame_w, step, |f| {
        paint_sticky_horizontal_band(f, 0, 8);
    });

    let h = stitched.height();
    for x in 0..stitched.width() {
        let p = *stitched.get_pixel(x, h - 1);
        assert!(
            (p[0] == 95 || p[0] == 135) && p[0] == p[1] && p[1] == p[2],
            "bottom band missing at ({x}, h-1): {p:?}",
        );
    }
}

#[test]
fn motion_larger_than_half_frame_falls_back_to_v0_2_behavior() {
    let source = make_scroll_canvas(320, 1400);
    let frame_w = 320u32;
    let frame_h = 320u32;
    let step = 200u32;

    let first = crop_frame_xy(&source, 0, 0, frame_w, frame_h);
    let mut canvas = LinearCanvas::new(first);
    let mut expected_h = frame_h;

    let mut y = step;
    while y + frame_h <= source.height() && expected_h < 1000 {
        let f = crop_frame_xy(&source, 0, y, frame_w, frame_h);
        let added = canvas
            .append(AppendDirection::Bottom, &f, step)
            .expect("append");
        assert_eq!(added, step);
        expected_h += step;
        y += step;
    }

    assert!(
        expected_h > frame_h,
        "need at least one large-motion append; got expected_h = {expected_h}",
    );

    let stitched = canvas.image();
    assert_eq!(stitched.height(), expected_h);
    for cy in 0..stitched.height() {
        for cx in 0..stitched.width() {
            assert_eq!(
                stitched.get_pixel(cx, cy),
                source.get_pixel(cx, cy),
                "large-motion fallback byte-equivalence mismatch at ({cx}, {cy})",
            );
        }
    }
}
