mod common;

use common::{crop_frame, lazy_load_page, make_repeated_rows, make_scroll_canvas};
use rollshot_core::{AppendDirection, StitchConfig, StitchOutcome, Stitcher};

const W: u32 = 720;
const CANVAS_H: u32 = 2600;
const FRAME_H: u32 = 600;
const IMG_Y0: u32 = 480;
const IMG_Y1: u32 = 1180;
const STEP: u32 = 160;

fn cfg() -> StitchConfig {
    let mut c = StitchConfig::default();
    c.max_search_ratio = 0.75; // small synthetic frames, see golden_fixtures.rs
    c
}

fn good_bottom(outcome: StitchOutcome) -> bool {
    matches!(
        outcome,
        StitchOutcome::Appended { direction: AppendDirection::Bottom, estimate, .. }
            if (120..=200).contains(&estimate.dy)
    )
}

/// ① + overlap-overwrite: a lazy-loaded image at the bottom of a GOOD anchor
/// (mid-capture) must not veto the correct offset; capture keeps progressing.
#[test]
fn mid_capture_lazy_load_keeps_stitching() {
    let loaded = lazy_load_page(W, CANVAS_H, IMG_Y0, IMG_Y1, true);
    let placeholder = lazy_load_page(W, CANVAS_H, IMG_Y0, IMG_Y1, false);
    let mut s = Stitcher::new(cfg());

    // Frames 0,1 from loaded page → establishes a good vertical-locked anchor.
    assert_eq!(s.push_frame(crop_frame(&loaded, 0, FRAME_H)), StitchOutcome::FirstFrame);
    let _ = s.push_frame(crop_frame(&loaded, STEP, FRAME_H));

    // Frame 2: imagine the image just below loaded in THIS frame only as a
    // placeholder (the anchor=frame1 has it loaded). Use the placeholder page
    // for frame 2 → overlap (anchor bottom vs frame2 top) disagrees locally.
    let mut good = 0;
    for i in 2..7u32 {
        let page = if i == 2 { &placeholder } else { &loaded };
        let outcome = s.push_frame(crop_frame(page, i * STEP, FRAME_H));
        if good_bottom(outcome) {
            good += 1;
        }
    }
    assert!(good >= 3, "mid-capture lazy-load stalled: only {good} good Bottom appends");
}

/// ②: a LARGE changed region defeats template; routine feature consensus must
/// still recover the offset (then ① verifies it).
#[test]
fn large_lazy_region_recovered_by_feature_consensus() {
    // Image block spans most of the frame so template NCC peak is dragged off.
    let big0 = 150;
    let big1 = 560;
    let loaded = lazy_load_page(W, CANVAS_H, big0, big1, true);
    let placeholder = lazy_load_page(W, CANVAS_H, big0, big1, false);
    let mut s = Stitcher::new(cfg());
    assert_eq!(s.push_frame(crop_frame(&loaded, 0, FRAME_H)), StitchOutcome::FirstFrame);
    let _ = s.push_frame(crop_frame(&loaded, STEP, FRAME_H));
    let outcome = s.push_frame(crop_frame(&placeholder, 2 * STEP, FRAME_H));
    assert!(
        good_bottom(outcome),
        "feature consensus did not recover the offset under a large changed region"
    );
}

/// Misfire floor: a repeated pattern must NOT be falsely accepted just because
/// the verifier got more tolerant. (Defense is the matcher second-best margin;
/// this test locks that the verifier change does not open a hole.)
#[test]
fn repeated_rows_still_not_falsely_appended() {
    let canvas = make_repeated_rows(W, CANVAS_H);
    let mut s = Stitcher::new(cfg());
    assert_eq!(s.push_frame(crop_frame(&canvas, 0, FRAME_H)), StitchOutcome::FirstFrame);
    // Repeated rows are deliberately ambiguous; an aliased "append" would be a
    // misfire. Accept Duplicate/NoMatch/NoProgress, but never a confident
    // wrong-offset Appended with a large dy.
    for i in 1..5u32 {
        match s.push_frame(crop_frame(&canvas, i * STEP, FRAME_H)) {
            StitchOutcome::Appended { estimate, .. } => {
                assert!(
                    estimate.dy.unsigned_abs() <= STEP + 8,
                    "aliased misfire: dy={} far from true step {STEP}",
                    estimate.dy
                );
            }
            _ => {}
        }
    }
}

/// Monotonicity smoke: a clean (non-dynamic) scroll still stitches normally.
#[test]
fn clean_scroll_unchanged() {
    let canvas = make_scroll_canvas(W, CANVAS_H);
    let mut s = Stitcher::new(cfg());
    assert_eq!(s.push_frame(crop_frame(&canvas, 0, FRAME_H)), StitchOutcome::FirstFrame);
    let mut appended = 0;
    for i in 1..6u32 {
        if matches!(s.push_frame(crop_frame(&canvas, i * STEP, FRAME_H)), StitchOutcome::Appended { .. }) {
            appended += 1;
        }
    }
    assert!(appended >= 4, "clean scroll regressed: only {appended} appends");
}
