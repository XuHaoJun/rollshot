mod common;

use common::{crop_frame_xy, make_scroll_canvas};
use image::Rgba;
use rollshot_core::{NoMatchReason, StitchConfig, StitchOutcome, Stitcher, VerifierConfig};

#[test]
fn verifier_accepts_matching_vertical_motion() {
    let canvas = make_scroll_canvas(320, 1000);
    let first = crop_frame_xy(&canvas, 0, 0, 320, 320);
    let second = crop_frame_xy(&canvas, 0, 80, 320, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);
    match stitcher.push_frame(second) {
        StitchOutcome::Appended { estimate, .. } => {
            assert!(estimate.overlap.height > 0);
            assert!(estimate.overlap.width > 0);
        }
        other => panic!("expected Appended after verified motion, got {other:?}"),
    }
}

#[test]
fn verifier_rejects_when_pixels_disagree() {
    let canvas = make_scroll_canvas(320, 1000);
    let first = crop_frame_xy(&canvas, 0, 0, 320, 320);
    let mut second = crop_frame_xy(&canvas, 0, 80, 320, 320);
    for y in 160..320 {
        for x in 0..320 {
            second.put_pixel(x, y, Rgba([255, 0, 255, 255]));
        }
    }

    let mut stricter = StitchConfig::default();
    let mut verifier = VerifierConfig::default();
    verifier.downsample_max_mad = 0.02;
    verifier.full_res_max_mad = 0.02;
    stricter.verifier = verifier;

    let mut stitcher = Stitcher::new(stricter);
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);
    match stitcher.push_frame(second) {
        StitchOutcome::NoMatch { reason, .. } => {
            assert!(
                matches!(
                    reason,
                    NoMatchReason::OverlapVerificationFailed
                        | NoMatchReason::LowConfidence
                        | NoMatchReason::AkazeDisabled
                        | NoMatchReason::AkazeLowInliers
                        | NoMatchReason::FeatureFallbackDisabled
                        | NoMatchReason::FeatureLowInliers
                ),
                "unexpected reason: {reason:?}"
            );
        }
        other => panic!("expected verifier rejection, got {other:?}"),
    }
}

#[test]
fn verifier_rejects_when_overlap_is_too_small() {
    let canvas = make_scroll_canvas(320, 1000);
    let first = crop_frame_xy(&canvas, 0, 0, 320, 320);
    let second = crop_frame_xy(&canvas, 0, 80, 320, 320);

    let mut strict = StitchConfig::default();
    strict.min_overlap = 4096;
    let mut stitcher = Stitcher::new(strict);
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);
    match stitcher.push_frame(second) {
        StitchOutcome::NoMatch { reason, .. } => {
            assert!(
                matches!(
                    reason,
                    NoMatchReason::InsufficientOverlap
                        | NoMatchReason::LowConfidence
                        | NoMatchReason::AkazeDisabled
                        | NoMatchReason::AkazeLowInliers
                        | NoMatchReason::FeatureFallbackDisabled
                        | NoMatchReason::FeatureLowInliers
                ),
                "unexpected reason: {reason:?}"
            );
        }
        other => panic!("expected InsufficientOverlap-like rejection, got {other:?}"),
    }
}
