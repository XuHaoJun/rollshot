mod common;

use common::{crop_frame, make_scroll_canvas};
use image::{Rgba, RgbaImage};
use rollshot_core::{RecoveryProbeResult, StitchConfig, StitchOutcome, Stitcher};

#[test]
fn preserving_anchor_push_never_reanchors_after_repeated_misses() {
    let canvas = make_scroll_canvas(320, 1800);
    let anchor = crop_frame(&canvas, 96, 320);
    let next = crop_frame(&canvas, 192, 320);
    let bad = RgbaImage::from_pixel(320, 320, Rgba([255, 255, 255, 255]));
    let mut stitcher = Stitcher::new(StitchConfig::default());

    assert_eq!(
        stitcher.push_frame_preserving_anchor(crop_frame(&canvas, 0, 320)),
        StitchOutcome::FirstFrame
    );
    assert!(matches!(
        stitcher.push_frame_preserving_anchor(anchor),
        StitchOutcome::Appended { .. }
    ));
    let before = stitcher.stats();

    for _ in 0..4 {
        assert!(matches!(
            stitcher.push_frame_preserving_anchor(bad.clone()),
            StitchOutcome::NoMatch { .. }
        ));
    }

    assert_eq!(stitcher.stats(), before);
    assert!(matches!(
        stitcher.push_frame_preserving_anchor(next),
        StitchOutcome::Appended { .. }
    ));
}

#[test]
fn recovery_probe_accepts_duplicate_and_reverse_overlap_without_mutation() {
    let canvas = make_scroll_canvas(320, 1800);
    let anchor = crop_frame(&canvas, 96, 320);
    let reverse_overlap = crop_frame(&canvas, 32, 320);
    let mut stitcher = Stitcher::new(StitchConfig::default());
    stitcher.push_frame(crop_frame(&canvas, 0, 320));
    stitcher.push_frame(anchor.clone());
    let stats = stitcher.stats();
    let metrics_frame_index = stitcher.last_metrics().frame_index;
    let metrics_outcome = stitcher.last_metrics().outcome;

    assert_eq!(
        stitcher.probe_recovery(&anchor),
        RecoveryProbeResult::Recovered
    );
    assert_eq!(
        stitcher.probe_recovery(&reverse_overlap),
        RecoveryProbeResult::Recovered
    );
    assert_eq!(stitcher.stats(), stats);
    assert_eq!(stitcher.last_metrics().frame_index, metrics_frame_index);
    assert_eq!(stitcher.last_metrics().outcome, metrics_outcome);
}

#[test]
fn recovery_probe_rejects_unrelated_and_dimension_mismatched_frames() {
    let canvas = make_scroll_canvas(320, 1800);
    let mut stitcher = Stitcher::new(StitchConfig::default());
    stitcher.push_frame(crop_frame(&canvas, 0, 320));
    stitcher.push_frame(crop_frame(&canvas, 96, 320));

    let unrelated = RgbaImage::from_pixel(320, 320, Rgba([255, 255, 255, 255]));
    let wrong_size = RgbaImage::from_pixel(200, 320, Rgba([255, 255, 255, 255]));
    assert_eq!(
        stitcher.probe_recovery(&unrelated),
        RecoveryProbeResult::Missed
    );
    assert_eq!(
        stitcher.probe_recovery(&wrong_size),
        RecoveryProbeResult::Missed
    );
}

// Parity guard: a frame the normal push path would accept as an on-axis
// append must probe as `Recovered`, and a genuine non-overlapping frame the
// push path rejects must probe as `Missed`. Pins probe vs push so the shared
// evaluation core cannot drift apart. Uses two independent stitchers so the
// push-side acceptance does not perturb the probe-side anchor.
#[test]
fn probe_recovery_agrees_with_push_accept_reject() {
    let canvas = make_scroll_canvas(320, 1800);
    let forward = crop_frame(&canvas, 96, 320);
    let unrelated = RgbaImage::from_pixel(320, 320, Rgba([255, 255, 255, 255]));

    let mut pushed = Stitcher::new(StitchConfig::default());
    pushed.push_frame(crop_frame(&canvas, 0, 320));
    let mut probed = Stitcher::new(StitchConfig::default());
    probed.push_frame(crop_frame(&canvas, 0, 320));

    assert!(matches!(
        pushed.push_frame(forward.clone()),
        StitchOutcome::Appended { .. }
    ));
    assert_eq!(
        probed.probe_recovery(&forward),
        RecoveryProbeResult::Recovered
    );

    assert!(matches!(
        pushed.push_frame(unrelated.clone()),
        StitchOutcome::NoMatch { .. }
    ));
    assert_eq!(
        probed.probe_recovery(&unrelated),
        RecoveryProbeResult::Missed
    );
}
