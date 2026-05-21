mod common;

use common::{
    crop_frame, crop_frame_xy, make_akaze_fallback_canvas, make_repeated_rows,
    make_scroll_canvas, make_wide_canvas, paint_sticky_header,
};
use image::{Rgba, RgbaImage};
use rollshot_core::{
    AkazeConfig, AppendDirection, MatchMethod, NoMatchReason, ScrollAxis, StitchConfig,
    StitchOutcome, Stitcher, VerifierConfig,
};

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
    assert_eq!(stats.total_width, 320);
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
        StitchOutcome::NoMatch {
            reason,
            best_estimate,
        } => {
            assert_eq!(reason, NoMatchReason::DimensionMismatch);
            assert!(best_estimate.is_none());
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
    assert_eq!(
        stitcher.push_frame(first.clone()),
        StitchOutcome::FirstFrame
    );
    assert_eq!(stitcher.push_frame(first.clone()), StitchOutcome::Duplicate);

    let full = stitcher.full_image().expect("image stored");
    assert_eq!(full.dimensions(), (320, 320));
    assert_eq!(stitcher.stats().frame_count, 1);
}

#[test]
fn normal_scroll_appends_bottom_and_locks_vertical_axis() {
    let canvas = make_scroll_canvas(320, 1200);
    let first = crop_frame(&canvas, 0, 320);
    let scrolled = crop_frame(&canvas, 80, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended {
            direction,
            added,
            estimate,
        } => {
            assert_eq!(direction, AppendDirection::Bottom);
            assert_eq!(estimate.axis, ScrollAxis::Vertical);
            assert!((76..=84).contains(&added), "added = {added}");
            assert!(estimate.confidence < StitchConfig::default().accept_confidence);
        }
        other => panic!("expected Appended, got {other:?}"),
    }

    let full = stitcher.full_image().expect("stitched image");
    assert!(full.height() > 320);
    let stats = stitcher.stats();
    assert_eq!(stats.frame_count, 2);
    assert_eq!(stats.total_height, full.height());
    assert_eq!(stats.total_width, 320);
}

#[test]
fn small_scroll_below_min_append_reports_no_progress() {
    let canvas = make_scroll_canvas(320, 1000);
    let first = crop_frame(&canvas, 0, 320);
    let nudged = crop_frame(&canvas, 16, 320);

    let mut config = StitchConfig::default();
    config.min_append = 64;
    let mut stitcher = Stitcher::new(config);
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);
    match stitcher.push_frame(nudged) {
        StitchOutcome::NoProgress { estimate } => {
            let est = estimate.expect("nudged frame should still produce an estimate");
            assert!(est.dy.abs() < 64, "dy = {}", est.dy);
        }
        other => panic!("expected NoProgress, got {other:?}"),
    }

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
        StitchOutcome::NoMatch { reason, .. } => {
            assert!(
                reason == NoMatchReason::LowConfidence
                    || reason == NoMatchReason::AkazeDisabled
            );
        }
        other => panic!("expected NoMatch on white frame, got {other:?}"),
    }

    let stats_after_bad = stitcher.stats();
    assert_eq!(stats_after_bad.frame_count, 1);
    assert_eq!(stats_after_bad.total_height, 320);

    match stitcher.push_frame(recovered) {
        StitchOutcome::Appended {
            added, direction, ..
        } => {
            assert_eq!(direction, AppendDirection::Bottom);
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

    // The painted sticky header creates a strong checkerboard that the default
    // verifier thresholds reject. Use a slightly more lenient verifier so the
    // test proves sticky headers don't dominate *motion estimation* rather than
    // testing verifier strictness.
    let mut config = StitchConfig::default();
    {
        let mut v = VerifierConfig::default();
        v.downsample_max_mad = 40.0 / 255.0;
        v.full_res_max_mad = 30.0 / 255.0;
        config.verifier = v;
    };
    let mut stitcher = Stitcher::new(config);
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended {
            added, direction, ..
        } => {
            assert_eq!(direction, AppendDirection::Bottom);
            assert!((66..=74).contains(&added), "added = {added}");
        }
        other => panic!("expected Appended with sticky header, got {other:?}"),
    }
}

#[test]
fn vertical_up_scroll_prepends_top() {
    let canvas = make_scroll_canvas(320, 1400);
    let first = crop_frame(&canvas, 800, 320);
    let scrolled = crop_frame(&canvas, 720, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended {
            direction,
            added,
            estimate,
        } => {
            assert_eq!(direction, AppendDirection::Top);
            assert_eq!(estimate.axis, ScrollAxis::Vertical);
            assert!(estimate.dy < 0, "dy = {}", estimate.dy);
            assert!((76..=84).contains(&added), "added = {added}");
        }
        other => panic!("expected top append, got {other:?}"),
    }

    assert_eq!(stitcher.stats().total_width, 320);
    assert!(stitcher.stats().total_height > 320);
}

#[test]
fn horizontal_right_scroll_appends_right() {
    let canvas = make_wide_canvas(1400, 320);
    let first = crop_frame_xy(&canvas, 0, 0, 320, 320);
    let scrolled = crop_frame_xy(&canvas, 80, 0, 320, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended {
            direction,
            added,
            estimate,
        } => {
            assert_eq!(direction, AppendDirection::Right);
            assert_eq!(estimate.axis, ScrollAxis::Horizontal);
            assert!(estimate.dx > 0, "dx = {}", estimate.dx);
            assert!((76..=84).contains(&added), "added = {added}");
        }
        other => panic!("expected right append, got {other:?}"),
    }

    assert!(stitcher.stats().total_width > 320);
    assert_eq!(stitcher.stats().total_height, 320);
}

#[test]
fn horizontal_left_scroll_prepends_left() {
    let canvas = make_wide_canvas(1400, 320);
    let first = crop_frame_xy(&canvas, 800, 0, 320, 320);
    let scrolled = crop_frame_xy(&canvas, 720, 0, 320, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended {
            direction,
            added,
            estimate,
        } => {
            assert_eq!(direction, AppendDirection::Left);
            assert_eq!(estimate.axis, ScrollAxis::Horizontal);
            assert!(estimate.dx < 0, "dx = {}", estimate.dx);
            assert!((76..=84).contains(&added), "added = {added}");
        }
        other => panic!("expected left append, got {other:?}"),
    }

    assert!(stitcher.stats().total_width > 320);
    assert_eq!(stitcher.stats().total_height, 320);
}

#[test]
fn horizontal_after_vertical_lock_is_rejected_as_axis_change() {
    let canvas = make_scroll_canvas(900, 1200);
    let first = crop_frame_xy(&canvas, 200, 0, 320, 320);
    let down = crop_frame_xy(&canvas, 200, 80, 320, 320);
    let right = crop_frame_xy(&canvas, 280, 80, 320, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);
    assert!(matches!(
        stitcher.push_frame(down),
        StitchOutcome::Appended {
            direction: AppendDirection::Bottom,
            ..
        }
    ));

    match stitcher.push_frame(right) {
        StitchOutcome::AxisChanged {
            previous_axis: ScrollAxis::Vertical,
            new_axis: ScrollAxis::Horizontal,
            ..
        } => {}
        other => panic!("expected horizontal frame rejected after vertical lock, got {other:?}"),
    }

    assert_eq!(stitcher.stats().frame_count, 2);
}

#[test]
fn repeated_rows_do_not_append_without_clear_match() {
    let canvas = make_repeated_rows(320, 1000);
    let first = crop_frame(&canvas, 0, 320);
    let repeated = crop_frame(&canvas, 32, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(repeated) {
        StitchOutcome::Duplicate => {}
        StitchOutcome::NoMatch { reason, .. } => {
            assert!(matches!(
                reason,
                NoMatchReason::LowConfidence
                    | NoMatchReason::AmbiguousAxis
                    | NoMatchReason::OverlapVerificationFailed
            ));
        }
        other => panic!("expected repeated rows to be rejected, got {other:?}"),
    }

    assert_eq!(stitcher.stats().frame_count, 1);
    assert_eq!(stitcher.stats().total_height, 320);
}

#[cfg(feature = "akaze")]
#[test]
fn akaze_fallback_appends_when_template_is_ambiguous() {
    let canvas = make_akaze_fallback_canvas(320, 900);
    let first = crop_frame(&canvas, 0, 320);
    let scrolled = crop_frame(&canvas, 88, 320);

    let mut config = StitchConfig::default();
    config.second_best_margin = 0.25;
    {
        let mut a = AkazeConfig::default();
        a.enabled = true;
        a.detector_threshold = 0.0005;
        a.min_raw_matches = 8;
        a.min_inliers = 6;
        a.min_inlier_ratio = 0.25;
        config.akaze = a;
    }
    let mut stitcher = Stitcher::new(config);

    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);
    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended {
            direction,
            added,
            estimate,
        } => {
            assert_eq!(direction, AppendDirection::Bottom);
            assert_eq!(estimate.method, MatchMethod::Akaze);
            assert!((84..=92).contains(&added), "added = {added}");
            assert!(estimate.inliers.unwrap_or(0) >= 6);
            assert!(estimate.raw_matches.unwrap_or(0) >= 8);
        }
        other => panic!("expected AKAZE append, got {other:?}"),
    }
}
