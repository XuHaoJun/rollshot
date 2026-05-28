//! Verifies that `Stitcher::last_metrics()` is populated correctly for each
//! `StitchOutcome` variant. Uses the existing golden fixture frames as input.

use std::fs;
use std::path::Path;

use image::RgbaImage;
use rollshot_core::{StitchConfig, StitchOutcome, StitchOutcomeKind, Stitcher};

const FIXTURE_ROOT: &str = "tests/fixtures/linearscroll_v2";

fn load_frames(family: &str) -> Vec<RgbaImage> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURE_ROOT)
        .join(family)
        .join("frames");
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .expect("read frames dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("png"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|p| image::open(p).expect("decode").to_rgba8())
        .collect()
}

/// Build a frame filled with a vertical gradient so the matcher can find
/// reliable vertical dy motion between two instances shifted by `shift_y`.
fn make_vertical_gradient_frame(width: u32, height: u32, offset_y: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |_x, y| {
        let v = ((y + offset_y) % 256) as u8;
        image::Rgba([v, v, v, 255])
    })
}

/// Build a rich-texture frame by stacking horizontal text-like stripes and
/// vertical column markers, then shift it by `(offset_x, offset_y)`.
/// The pattern has high frequency content in both axes so the edge-projection
/// and coarse matchers can detect sub-frame translations reliably.
fn make_rich_texture_frame(width: u32, height: u32, offset_x: i32, offset_y: i32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        // Wrap-around shift.
        let px = ((x as i32 + offset_x).rem_euclid(width as i32)) as u32;
        let py = ((y as i32 + offset_y).rem_euclid(height as i32)) as u32;
        // Hash-like deterministic value for texture variety (LCG-style mix).
        let v = (px
            .wrapping_mul(1664525)
            .wrapping_add(py.wrapping_mul(1013904223))
            & 0xFF) as u8;
        // Add fine-grained stripe structure so edge detectors fire.
        let stripe = if (px / 4 + py / 4) % 2 == 0 {
            v
        } else {
            255 - v
        };
        image::Rgba([stripe, stripe, stripe, 255])
    })
}

#[test]
fn first_frame_outcome_populates_minimal_fields() {
    let mut large_search_cfg = StitchConfig::default();
    large_search_cfg.max_search_ratio = 0.75;
    let mut stitcher = Stitcher::new(large_search_cfg);
    let frames = load_frames("linear_vertical_down");
    stitcher.push_frame(frames[0].clone());
    let m = stitcher.last_metrics();
    assert_eq!(m.outcome, StitchOutcomeKind::FirstFrame);
    assert_eq!(m.frame_index, 0);
    assert!(m.total_us > 0);
    assert_eq!(m.duplicate_us, 0);
    assert_eq!(m.coarse_us, 0);
    assert_eq!(m.pyramid_us, 0);
    assert_eq!(m.pyramid_candidates, 0);
    assert_eq!(m.template_ncc_us, 0);
    assert_eq!(m.verifier_us, 0);
    assert_eq!(m.append_us, 0);
    assert!(m.canvas_logical_pixels > 0);
    assert!(m.canvas_allocated_bytes > 0);
    assert_eq!(m.append_copied_bytes, 0);
}

#[test]
fn appended_outcome_populates_all_stages() {
    let mut large_search_cfg = StitchConfig::default();
    large_search_cfg.max_search_ratio = 0.75;
    let mut stitcher = Stitcher::new(large_search_cfg);
    let frames = load_frames("linear_vertical_down");
    assert!(frames.len() >= 2, "fixture must have at least two frames");

    stitcher.push_frame(frames[0].clone());
    let outcome = stitcher.push_frame(frames[1].clone());
    assert!(
        matches!(outcome, StitchOutcome::Appended { .. }),
        "expected Appended, got {outcome:?}"
    );

    let m = stitcher.last_metrics();
    assert_eq!(m.outcome, StitchOutcomeKind::Appended);
    assert_eq!(m.frame_index, 1);
    assert!(m.no_match_reason.is_none());
    assert!(
        m.prepare_frame_us > 0,
        "prepare_frame_us={}",
        m.prepare_frame_us
    );
    assert!(
        m.coarse_us > 0 || m.pyramid_us > 0 || m.template_ncc_us > 0,
        "matcher stages should record some time"
    );
    assert!(m.verifier_us > 0, "verifier_us={}", m.verifier_us);
    assert!(m.append_us > 0, "append_us={}", m.append_us);
    assert!(m.coarse_candidates > 0 || m.pyramid_candidates > 0 || m.ncc_offsets_scored > 0);
    assert!(m.canvas_logical_pixels > 0);
    assert!(m.canvas_allocated_bytes > 0);
    assert!(m.append_copied_bytes > 0);
    assert_ne!(m.best_dy, 0, "linear_vertical_down should have non-zero dy");
    assert!(m.match_method.is_some());
}

#[test]
fn duplicate_outcome_populates_only_duplicate_stage() {
    let mut stitcher = Stitcher::new(StitchConfig::default());
    let frames = load_frames("duplicate_frames");
    assert!(
        frames.len() >= 2,
        "duplicate_frames fixture must have >=2 frames"
    );

    stitcher.push_frame(frames[0].clone());
    let snapshot_before = {
        let m = stitcher.last_metrics();
        (m.canvas_logical_pixels, m.canvas_allocated_bytes)
    };

    // Find a frame that triggers Duplicate.
    let mut found_duplicate = false;
    for (i, frame) in frames.iter().enumerate().skip(1) {
        let outcome = stitcher.push_frame(frame.clone());
        if matches!(outcome, StitchOutcome::Duplicate) {
            let m = stitcher.last_metrics();
            assert_eq!(m.outcome, StitchOutcomeKind::Duplicate);
            assert!(m.duplicate_us > 0, "duplicate_us={}", m.duplicate_us);
            assert_eq!(m.prepare_frame_us, 0);
            assert_eq!(m.coarse_us, 0);
            assert_eq!(m.template_ncc_us, 0);
            assert_eq!(m.verifier_us, 0);
            assert_eq!(m.append_us, 0);
            assert_eq!(m.append_copied_bytes, 0);
            // The Duplicate path appends nothing, so canvas state is snapshot
            // unchanged from the first frame.
            assert_eq!(
                (m.canvas_logical_pixels, m.canvas_allocated_bytes),
                snapshot_before,
                "canvas state should not change for Duplicate (frame {i})"
            );
            found_duplicate = true;
            break;
        }
    }
    assert!(
        found_duplicate,
        "duplicate_frames fixture should produce a Duplicate outcome"
    );
}

#[test]
fn stage_sum_covers_at_least_80_percent_of_total() {
    let mut large_search_cfg = StitchConfig::default();
    large_search_cfg.max_search_ratio = 0.75;
    let mut stitcher = Stitcher::new(large_search_cfg);
    let frames = load_frames("linear_vertical_down");
    stitcher.push_frame(frames[0].clone());
    stitcher.push_frame(frames[1].clone());
    let m = stitcher.last_metrics();
    assert_eq!(m.outcome, StitchOutcomeKind::Appended);

    let stage_sum = m.duplicate_us
        + m.prepare_frame_us
        + m.coarse_us
        + m.pyramid_us
        + m.template_ncc_us
        + m.edge_projection_us
        + m.verifier_us
        + m.fallback_us
        + m.append_us;

    assert!(
        stage_sum <= m.total_us,
        "stage_sum={stage_sum} should be <= total_us={}",
        m.total_us
    );
    // Pre-instrumented overhead should leave >=80% of total_us accounted for.
    assert!(
        stage_sum * 5 >= m.total_us * 4,
        "stage_sum={stage_sum} should be >=80% of total_us={}",
        m.total_us
    );
}

#[test]
fn outcome_kind_advances_frame_index() {
    let mut stitcher = Stitcher::new(StitchConfig::default());
    let frames = load_frames("duplicate_frames");
    stitcher.push_frame(frames[0].clone());
    assert_eq!(stitcher.last_metrics().frame_index, 0);
    stitcher.push_frame(frames[0].clone()); // duplicate
    assert_eq!(stitcher.last_metrics().frame_index, 1);
}

#[test]
fn no_match_outcome_records_dimension_mismatch() {
    use rollshot_core::NoMatchReason;
    let mut stitcher = Stitcher::new(StitchConfig::default());
    let frame1 = RgbaImage::from_pixel(200, 200, image::Rgba([200, 200, 200, 255]));
    let frame2 = RgbaImage::from_pixel(220, 200, image::Rgba([200, 200, 200, 255]));
    stitcher.push_frame(frame1);
    let outcome = stitcher.push_frame(frame2);
    assert!(
        matches!(
            outcome,
            StitchOutcome::NoMatch {
                reason: NoMatchReason::DimensionMismatch,
                ..
            }
        ),
        "expected NoMatch{{DimensionMismatch}}, got {outcome:?}"
    );
    let m = stitcher.last_metrics();
    assert_eq!(m.outcome, StitchOutcomeKind::NoMatch);
    assert_eq!(m.no_match_reason, Some(NoMatchReason::DimensionMismatch));
    // Dimension mismatch returns before duplicate detection, so duplicate_us
    // stays zero.
    assert_eq!(m.duplicate_us, 0);
}

#[test]
fn no_progress_outcome_recorded_for_low_motion_fixture() {
    // Construct synthetic frames with sub-threshold vertical motion to force
    // NoProgress. `min_append` defaults to 8; a 3-pixel shift is below that.
    let mut cfg = StitchConfig::default();
    cfg.max_search_ratio = 0.75;
    let mut stitcher = Stitcher::new(cfg);

    // Frame 0: base vertical gradient (320x320).
    let frame0 = make_vertical_gradient_frame(320, 320, 0);
    // Frame 1: shifted down by 3 px -- below the default min_append of 8.
    let frame1 = make_vertical_gradient_frame(320, 320, 3);

    stitcher.push_frame(frame0);
    let outcome = stitcher.push_frame(frame1);

    if matches!(outcome, StitchOutcome::NoProgress { .. }) {
        let m = stitcher.last_metrics();
        assert_eq!(m.outcome, StitchOutcomeKind::NoProgress);
        assert!(
            m.no_match_reason.is_none(),
            "NoProgress should not set no_match_reason, got {:?}",
            m.no_match_reason
        );
        return;
    }

    // If the synthetic shift doesn't resolve to exactly 3 px (e.g. edge
    // effects push it past min_append), scan the repeated_rows fixture as a
    // fallback across up to 10 frames.
    let mut stitcher2 = Stitcher::new(StitchConfig::default());
    let frames = load_frames("repeated_rows");
    assert!(
        frames.len() >= 2,
        "repeated_rows fixture must have >=2 frames"
    );
    stitcher2.push_frame(frames[0].clone());
    for frame in frames.iter().skip(1).take(10) {
        let o = stitcher2.push_frame(frame.clone());
        if matches!(o, StitchOutcome::NoProgress { .. }) {
            let m = stitcher2.last_metrics();
            assert_eq!(m.outcome, StitchOutcomeKind::NoProgress);
            assert!(
                m.no_match_reason.is_none(),
                "NoProgress should not set no_match_reason, got {:?}",
                m.no_match_reason
            );
            return;
        }
    }

    panic!(
        "synthetic sub-threshold frames produced {outcome:?} instead of NoProgress, \
         and repeated_rows did not produce NoProgress within 10 frames; \
         choose a different fixture or adjust the shift amount"
    );
}

#[test]
fn axis_changed_outcome_recorded_when_axis_lock_breaks() {
    // Establish a horizontal axis lock using a rich-texture pattern, then
    // push a vertically-shifted frame of the same content to produce a
    // dominant dy that trips AxisChanged (Horizontal -> Vertical).
    let mut cfg = StitchConfig::default();
    cfg.max_search_ratio = 0.75;
    let mut stitcher = Stitcher::new(cfg);

    const W: u32 = 320;
    const H: u32 = 320;
    // Shift well above min_append=8 and max_cross_axis_px=6.
    const SHIFT: i32 = 60;

    // Frame 0 + 1: same texture shifted right by SHIFT px to lock Horizontal.
    let hf0 = make_rich_texture_frame(W, H, 0, 0);
    let hf1 = make_rich_texture_frame(W, H, SHIFT, 0);
    stitcher.push_frame(hf0);
    let lock_outcome = stitcher.push_frame(hf1);
    assert!(
        matches!(lock_outcome, StitchOutcome::Appended { .. }),
        "expected horizontal lock Appended, got {lock_outcome:?}"
    );

    // Frame 2: same texture with the same x-offset as hf1 but additionally
    // shifted down by SHIFT px -- so compared to hf1 the motion is dy=SHIFT,
    // dx=0, which should break the Horizontal lock and return AxisChanged.
    let vf = make_rich_texture_frame(W, H, SHIFT, SHIFT);
    let outcome = stitcher.push_frame(vf);
    assert!(
        matches!(outcome, StitchOutcome::AxisChanged { .. }),
        "expected AxisChanged after locking horizontal and pushing vertical-dominant frame, got {outcome:?}"
    );
    let m = stitcher.last_metrics();
    assert_eq!(m.outcome, StitchOutcomeKind::AxisChanged);
}
