mod common;

use common::{
    crop_frame, crop_frame_xy, make_repeated_rows, make_scroll_canvas, make_wide_canvas,
    paint_sticky_header,
};
use image::{Rgba, RgbaImage};
use rollshot_core::{
    AppendDirection, MatchMethod, NoMatchReason, ScrollAxis, StitchConfig, StitchOutcome, Stitcher,
    VerifierConfig,
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

    let full = stitcher.full_image().expect("first frame stored").clone();
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

    let full = stitcher.full_image().expect("image stored").clone();
    assert_eq!(full.dimensions(), (320, 320));
    assert_eq!(stitcher.stats().frame_count, 1);
}

#[test]
fn fast_scroll_beyond_default_search_ratio_recovers_via_relaxed_pass() {
    // The default `max_search_ratio` (0.4) only reaches ~128 px on a
    // 320-tall frame. A 200 px scroll lands outside that envelope, so
    // every regular matcher misses. The relaxed coarse pass widens the
    // ratio to ~0.85 (≈272 px), which must recover the offset.
    let canvas = make_scroll_canvas(320, 1200);
    let first = crop_frame(&canvas, 0, 320);
    let scrolled = crop_frame(&canvas, 200, 320);

    let config = StitchConfig::default();
    let mut stitcher = Stitcher::new(config);
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended {
            direction,
            added,
            estimate,
        } => {
            assert_eq!(direction, AppendDirection::Bottom);
            assert_eq!(estimate.axis, ScrollAxis::Vertical);
            assert!(
                (192..=208).contains(&added),
                "added = {added} (expected ~200 via relaxed coarse)"
            );
        }
        other => panic!("expected Appended via relaxed coarse, got {other:?}"),
    }
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

    let full = stitcher.full_image().expect("stitched image").clone();
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
                    || reason == NoMatchReason::FeatureFallbackDisabled
                    || reason == NoMatchReason::NotEnoughFeatures
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
fn scroll_back_after_reverse_direction_miss_can_reconnect_to_last_good_anchor() {
    let canvas = make_scroll_canvas(320, 1800);
    let first = crop_frame(&canvas, 0, 320);
    let appended = crop_frame(&canvas, 96, 320);
    let reverse = crop_frame(&canvas, 32, 320);
    let reconnected = crop_frame(&canvas, 192, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(appended) {
        StitchOutcome::Appended { direction, .. } => {
            assert_eq!(direction, AppendDirection::Bottom);
        }
        other => panic!("expected initial bottom append, got {other:?}"),
    }

    match stitcher.push_frame(reverse) {
        StitchOutcome::NoMatch { reason, .. } => {
            assert_eq!(reason, NoMatchReason::ReverseDirection);
        }
        other => panic!("expected reverse-direction miss, got {other:?}"),
    }

    let stats_after_miss = stitcher.stats();
    assert_eq!(stats_after_miss.frame_count, 2);

    match stitcher.push_frame(reconnected) {
        StitchOutcome::Appended {
            direction, added, ..
        } => {
            assert_eq!(direction, AppendDirection::Bottom);
            assert!((92..=100).contains(&added), "added = {added}");
        }
        other => panic!("expected append after reconnecting to anchor, got {other:?}"),
    }

    assert_eq!(stitcher.stats().frame_count, 3);
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

#[test]
fn fast_hnsw_fallback_recovers_repeated_grid_with_sparse_features() {
    let canvas = common::make_feature_fallback_canvas(320, 900);
    let first = crop_frame(&canvas, 0, 320);
    let scrolled = crop_frame(&canvas, 88, 320);

    let mut config = StitchConfig::default();
    config.second_best_margin = 0.25;
    let mut stitcher = Stitcher::new(config);

    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);
    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended {
            direction,
            added,
            estimate,
        } => {
            assert_eq!(direction, AppendDirection::Bottom);
            assert_eq!(estimate.method, MatchMethod::FastHnsw);
            assert!((84..=92).contains(&added), "added = {added}");
            assert!(estimate.inliers.unwrap_or(0) >= 16);
            assert!(estimate.raw_matches.unwrap_or(0) >= 24);
        }
        other => panic!("expected FAST+KNN append, got {other:?}"),
    }
}

#[test]
fn fast_hnsw_attempt_with_blank_frames_reports_not_enough_features() {
    let prev = RgbaImage::from_pixel(220, 220, Rgba([250, 250, 250, 255]));
    let curr = RgbaImage::from_pixel(220, 220, Rgba([220, 180, 240, 255]));

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(prev), StitchOutcome::FirstFrame);
    match stitcher.push_frame(curr) {
        StitchOutcome::NoMatch {
            reason: NoMatchReason::NotEnoughFeatures,
            best_estimate: None,
        } => {}
        other => panic!("expected NotEnoughFeatures, got {other:?}"),
    }
}

#[test]
fn fast_hnsw_candidate_rejected_by_verifier_preserves_best_estimate() {
    let canvas = common::make_feature_fallback_canvas(360, 760);
    let prev = crop_frame(&canvas, 0, 240);
    let mut curr = crop_frame(&canvas, 72, 240);
    // Corrupt most of the frame except a thin top strip so verifier rejects.
    for y in 40..240 {
        for x in 0..240 {
            let v = ((x * 41 + y * 67) % 255) as u8;
            curr.put_pixel(x, y, Rgba([v, 255 - v, v / 2, 255]));
        }
    }

    let mut config = StitchConfig::default();
    config.second_best_margin = 0.25;
    let mut stitcher = Stitcher::new(config);
    assert_eq!(stitcher.push_frame(prev), StitchOutcome::FirstFrame);

    match stitcher.push_frame(curr) {
        StitchOutcome::NoMatch {
            reason: NoMatchReason::FeatureLowInliers,
            best_estimate: Some(estimate),
        } => {
            assert_eq!(estimate.method, MatchMethod::FastHnsw);
            assert!((estimate.dy - 72).abs() <= 8, "dy = {}", estimate.dy);
        }
        other => panic!("expected FeatureLowInliers with best_estimate, got {other:?}"),
    }
}

#[test]
fn appended_advances_anchor_to_latest_frame() {
    let canvas = make_scroll_canvas(320, 1200);
    let f0 = crop_frame(&canvas, 0, 320);
    let f1 = crop_frame(&canvas, 80, 320);
    let f2 = crop_frame(&canvas, 160, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(f0), StitchOutcome::FirstFrame);

    match stitcher.push_frame(f1) {
        StitchOutcome::Appended { added, .. } => {
            assert!((76..=84).contains(&added), "f0->f1 added={added}");
        }
        other => panic!("expected Appended, got {other:?}"),
    }

    // If the anchor advanced to f1, f1->f2 is a +80 scroll (added ~80). If it
    // had wrongly stayed at f0, f0->f2 is +160 and `added` would be ~160 (or a
    // NoMatch) — either way the 76..=84 assert below fails.
    match stitcher.push_frame(f2) {
        StitchOutcome::Appended {
            added, direction, ..
        } => {
            assert_eq!(direction, AppendDirection::Bottom);
            assert!(
                (76..=84).contains(&added),
                "f1->f2 added={added} (anchor did not advance to f1)"
            );
        }
        other => panic!("expected Appended, got {other:?}"),
    }
}
