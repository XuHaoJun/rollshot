mod common;

use common::{crop_frame, crop_frame_xy, make_scroll_canvas, make_wide_canvas, paint_sticky_header};
use image::Rgba;
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
    assert!(saw_header_at_top, "frame 1's header must remain at canvas top");

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
