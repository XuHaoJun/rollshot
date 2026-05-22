mod common;

use common::{crop_frame_xy, make_scroll_canvas, make_wide_canvas};
use rollshot_core::{AppendDirection, LinearCanvas};

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
