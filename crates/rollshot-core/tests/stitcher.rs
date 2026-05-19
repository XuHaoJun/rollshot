mod common;

use common::{crop_frame, make_scroll_canvas};
use image::{Rgba, RgbaImage};
use rollshot_core::{StitchConfig, StitchOutcome, Stitcher};

#[test]
fn first_frame_initializes_stitched_image() {
    let canvas = make_scroll_canvas(320, 1000);
    let first = crop_frame(&canvas, 0, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());

    assert_eq!(
        stitcher.push_frame(first.clone()),
        StitchOutcome::FirstFrame
    );

    let full = stitcher.full_image().expect("first frame stored");
    assert_eq!(full.dimensions(), (320, 320));

    let stats = stitcher.stats();
    assert_eq!(stats.frame_count, 1);
    assert_eq!(stats.total_height, 320);
    assert_eq!(stats.last_append, 320);
}

#[test]
fn dimension_mismatch_returns_no_match() {
    let canvas = make_scroll_canvas(320, 1000);
    let first = crop_frame(&canvas, 0, 320);
    let wrong_size = RgbaImage::from_pixel(200, 320, Rgba([255, 255, 255, 255]));

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(wrong_size) {
        StitchOutcome::NoMatch { confidence } => {
            assert!(!confidence.is_finite(), "confidence = {confidence}");
        }
        other => panic!("expected NoMatch, got {other:?}"),
    }

    let stats = stitcher.stats();
    assert_eq!(stats.frame_count, 1);
    assert_eq!(stats.total_height, 320);
}

#[test]
fn duplicate_frame_returns_duplicate_without_growing() {
    let canvas = make_scroll_canvas(320, 1000);
    let first = crop_frame(&canvas, 0, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first.clone()), StitchOutcome::FirstFrame);
    assert_eq!(stitcher.push_frame(first.clone()), StitchOutcome::Duplicate);

    let full = stitcher.full_image().expect("image stored");
    assert_eq!(full.dimensions(), (320, 320));
    assert_eq!(stitcher.stats().frame_count, 1);
}
