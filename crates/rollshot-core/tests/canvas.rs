mod common;

use common::{crop_frame_xy, make_scroll_canvas, make_wide_canvas};
use rollshot_core::{AppendDirection, LinearCanvas, ScrollAxis};

#[test]
fn vertical_down_pipeline_grows_canvas_downward() {
    let canvas_src = make_scroll_canvas(320, 1200);
    let f0 = crop_frame_xy(&canvas_src, 0, 0, 320, 320);
    let f1 = crop_frame_xy(&canvas_src, 0, 80, 320, 320);
    let f2 = crop_frame_xy(&canvas_src, 0, 160, 320, 320);

    let mut canvas = LinearCanvas::new(f0);
    assert_eq!(canvas.height(), 320);

    let added1 = canvas.append(AppendDirection::Bottom, &f1, 80).unwrap();
    assert_eq!(added1, 80);
    assert_eq!(canvas.axis(), Some(ScrollAxis::Vertical));
    assert_eq!(canvas.height(), 400);
    assert_eq!(canvas.width(), 320);

    let added2 = canvas.append(AppendDirection::Bottom, &f2, 80).unwrap();
    assert_eq!(added2, 80);
    assert_eq!(canvas.height(), 480);
}

#[test]
fn vertical_up_pipeline_grows_canvas_upward() {
    let canvas_src = make_scroll_canvas(320, 1200);
    let f0 = crop_frame_xy(&canvas_src, 0, 800, 320, 320);
    let f1 = crop_frame_xy(&canvas_src, 0, 720, 320, 320);
    let f2 = crop_frame_xy(&canvas_src, 0, 640, 320, 320);

    let mut canvas = LinearCanvas::new(f0);
    let added1 = canvas.append(AppendDirection::Top, &f1, 80).unwrap();
    assert_eq!(added1, 80);
    assert_eq!(canvas.axis(), Some(ScrollAxis::Vertical));

    let added2 = canvas.append(AppendDirection::Top, &f2, 80).unwrap();
    assert_eq!(added2, 80);
    assert_eq!(canvas.height(), 480);
    assert_eq!(canvas.width(), 320);
}

#[test]
fn horizontal_right_pipeline_grows_canvas_rightward() {
    let canvas_src = make_wide_canvas(1200, 320);
    let f0 = crop_frame_xy(&canvas_src, 0, 0, 320, 320);
    let f1 = crop_frame_xy(&canvas_src, 80, 0, 320, 320);
    let f2 = crop_frame_xy(&canvas_src, 160, 0, 320, 320);

    let mut canvas = LinearCanvas::new(f0);
    let added1 = canvas.append(AppendDirection::Right, &f1, 80).unwrap();
    assert_eq!(added1, 80);
    assert_eq!(canvas.axis(), Some(ScrollAxis::Horizontal));

    let added2 = canvas.append(AppendDirection::Right, &f2, 80).unwrap();
    assert_eq!(added2, 80);
    assert_eq!(canvas.width(), 480);
    assert_eq!(canvas.height(), 320);
}

#[test]
fn horizontal_left_pipeline_grows_canvas_leftward() {
    let canvas_src = make_wide_canvas(1200, 320);
    let f0 = crop_frame_xy(&canvas_src, 800, 0, 320, 320);
    let f1 = crop_frame_xy(&canvas_src, 720, 0, 320, 320);
    let f2 = crop_frame_xy(&canvas_src, 640, 0, 320, 320);

    let mut canvas = LinearCanvas::new(f0);
    let added1 = canvas.append(AppendDirection::Left, &f1, 80).unwrap();
    assert_eq!(added1, 80);
    assert_eq!(canvas.axis(), Some(ScrollAxis::Horizontal));

    let added2 = canvas.append(AppendDirection::Left, &f2, 80).unwrap();
    assert_eq!(added2, 80);
    assert_eq!(canvas.width(), 480);
    assert_eq!(canvas.height(), 320);
}

#[test]
fn switching_axis_mid_stitch_is_rejected() {
    let canvas_src = make_scroll_canvas(320, 800);
    let f0 = crop_frame_xy(&canvas_src, 0, 0, 320, 320);
    let f1 = crop_frame_xy(&canvas_src, 0, 80, 320, 320);

    let mut canvas = LinearCanvas::new(f0);
    canvas.append(AppendDirection::Bottom, &f1, 80).unwrap();

    let wide = make_wide_canvas(1200, 320);
    let frame_right = crop_frame_xy(&wide, 80, 0, 320, 320);
    let err = canvas
        .append(AppendDirection::Right, &frame_right, 80)
        .unwrap_err();
    use rollshot_core::CanvasAppendError;
    assert_eq!(
        err,
        CanvasAppendError::AxisMismatch {
            locked: ScrollAxis::Vertical,
            attempted: ScrollAxis::Horizontal,
        }
    );
}
