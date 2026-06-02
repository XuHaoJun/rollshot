//! Guard tests for the P6 lazy-load robustness work. These must hold in EVERY
//! phase (they are the misfire floor + monotonicity smoke that gate loosening
//! the verifier):
//!
//! - `repeated_rows_still_not_falsely_appended`: a deliberately ambiguous
//!   repeated pattern must never be confidently mis-appended just because the
//!   verifier got more tolerant.
//! - `clean_scroll_unchanged`: a clean (non-dynamic) scroll still stitches.
//!
//! The lazy-load behaviour itself is covered where it can be tested stably:
//! the robust verifier's localized-change acceptance is unit-tested in
//! `verifier.rs` (Phase B); routine feature offset recovery is unit-tested in
//! `feature_matcher.rs` (Phase C); the first-frame stale anchor is the
//! integration case in `reanchor_stale_first_frame.rs`; and the unrecoverable
//! mid-capture escape (③ re-anchor) is added in Phase D below.

mod common;

use common::{crop_frame, make_repeated_rows, make_scroll_canvas};
use rollshot_core::{StitchConfig, StitchOutcome, Stitcher};

const W: u32 = 720;
const CANVAS_H: u32 = 2600;
const FRAME_H: u32 = 600;
const STEP: u32 = 160;

fn cfg() -> StitchConfig {
    let mut c = StitchConfig::default();
    c.max_search_ratio = 0.75; // small synthetic frames, see golden_fixtures.rs
    c
}

/// Misfire floor: a repeated pattern must NOT be falsely accepted just because
/// the verifier got more tolerant. (Defense is the matcher second-best margin;
/// this test locks that the verifier change does not open a hole.)
#[test]
fn repeated_rows_still_not_falsely_appended() {
    let canvas = make_repeated_rows(W, CANVAS_H);
    let mut s = Stitcher::new(cfg());
    assert_eq!(
        s.push_frame(crop_frame(&canvas, 0, FRAME_H)),
        StitchOutcome::FirstFrame
    );
    // Repeated rows are deliberately ambiguous; an aliased "append" would be a
    // misfire. Accept Duplicate/NoMatch/NoProgress, but never a confident
    // wrong-offset Appended with a large dy.
    for i in 1..5u32 {
        if let StitchOutcome::Appended { estimate, .. } =
            s.push_frame(crop_frame(&canvas, i * STEP, FRAME_H))
        {
            assert!(
                estimate.dy.unsigned_abs() <= STEP + 8,
                "aliased misfire: dy={} far from true step {STEP}",
                estimate.dy
            );
        }
    }
}

/// Monotonicity smoke: a clean (non-dynamic) scroll still stitches normally.
#[test]
fn clean_scroll_unchanged() {
    let canvas = make_scroll_canvas(W, CANVAS_H);
    let mut s = Stitcher::new(cfg());
    assert_eq!(
        s.push_frame(crop_frame(&canvas, 0, FRAME_H)),
        StitchOutcome::FirstFrame
    );
    let mut appended = 0;
    for i in 1..6u32 {
        if matches!(
            s.push_frame(crop_frame(&canvas, i * STEP, FRAME_H)),
            StitchOutcome::Appended { .. }
        ) {
            appended += 1;
        }
    }
    assert!(
        appended >= 4,
        "clean scroll regressed: only {appended} appends"
    );
}
