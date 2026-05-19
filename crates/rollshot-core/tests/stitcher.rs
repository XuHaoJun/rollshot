mod common;

use common::{crop_frame, make_scroll_canvas, paint_sticky_header};
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

#[test]
fn normal_scroll_appends_expected_pixels() {
    let canvas = make_scroll_canvas(320, 1200);
    let first = crop_frame(&canvas, 0, 320);
    let scrolled = crop_frame(&canvas, 80, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended { added } => {
            assert!((76..=84).contains(&added), "added = {added}");
        }
        other => panic!("expected Appended, got {other:?}"),
    }

    let full = stitcher.full_image().expect("stitched image");
    assert!(full.height() > 320);
    let stats = stitcher.stats();
    assert_eq!(stats.frame_count, 2);
    assert_eq!(stats.total_height, full.height());
}

#[test]
fn small_scroll_below_min_append_reports_no_progress() {
    let canvas = make_scroll_canvas(320, 1000);
    let first = crop_frame(&canvas, 0, 320);
    let nudged = crop_frame(&canvas, 16, 320);

    let mut stitcher = Stitcher::new(StitchConfig {
        min_append: 64,
        ..StitchConfig::default()
    });
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);
    assert_eq!(stitcher.push_frame(nudged), StitchOutcome::NoProgress);

    let full = stitcher.full_image().expect("stitched image");
    assert_eq!(full.height(), 320);
}

#[test]
fn bad_frame_returns_no_match_and_preserves_anchor() {
    let canvas = make_scroll_canvas(320, 1200);
    let first = crop_frame(&canvas, 0, 320);
    let bad = RgbaImage::from_pixel(320, 320, Rgba([255, 255, 255, 255]));
    let recovered = crop_frame(&canvas, 96, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(bad) {
        StitchOutcome::NoMatch { confidence } => {
            assert!(confidence > StitchConfig::default().accept_diff);
        }
        other => panic!("expected NoMatch on white frame, got {other:?}"),
    }

    let stats_after_bad = stitcher.stats();
    assert_eq!(stats_after_bad.frame_count, 1);
    assert_eq!(stats_after_bad.total_height, 320);

    match stitcher.push_frame(recovered) {
        StitchOutcome::Appended { added } => {
            assert!((92..=100).contains(&added), "added = {added}");
        }
        other => panic!("expected Appended after recovery, got {other:?}"),
    }

    let stats_after_recover = stitcher.stats();
    assert_eq!(stats_after_recover.frame_count, 2);
}

#[test]
fn sticky_header_frames_still_append_expected_amount() {
    let canvas = make_scroll_canvas(320, 1400);
    let mut first = crop_frame(&canvas, 0, 320);
    let mut scrolled = crop_frame(&canvas, 70, 320);

    paint_sticky_header(&mut first, 36);
    paint_sticky_header(&mut scrolled, 36);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended { added } => {
            assert!((66..=74).contains(&added), "added = {added}");
        }
        other => panic!("expected Appended with sticky header, got {other:?}"),
    }
}
