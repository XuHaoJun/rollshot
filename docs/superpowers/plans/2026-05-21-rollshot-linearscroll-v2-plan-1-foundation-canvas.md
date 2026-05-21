# Rollshot LinearScroll v2 — Plan 1: Foundation + Canvas

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `rollshot-core`'s dy-only stitching model with the v0.2 type vocabulary (`MotionEstimate`, `ScrollAxis`, `AppendDirection`, `MatchMethod`, `MatchStrategy`, richer `StitchOutcome`/`NoMatchReason`), a generic 2D overlap verifier, and a four-direction `LinearCanvas` — without yet introducing horizontal matching or AKAZE. The existing template matcher keeps running as the lone candidate generator, wrapped into the new types so the CLI still produces vertical long screenshots after this plan lands.

**Architecture:** New modules sit between the existing template matcher and the `Stitcher`: `overlap.rs` computes generic prev/curr overlap rectangles from `(dx, dy)`; `verifier.rs` runs grayscale MAD against that overlap; `canvas.rs` owns the stitched `RgbaImage` and exposes append/prepend for all four directions while enforcing the single-axis invariant; `axis.rs` classifies candidates as Vertical/Horizontal/Ambiguous and validates them against a locked axis. `Stitcher` is refactored to feed the wrapped v0.1 matcher result through `axis → verifier → canvas` and emits the new `StitchOutcome` variants. AKAZE-related types are deferred to Plan 3; horizontal matching is deferred to Plan 2.

**Tech Stack:** Rust 2021, `image` 0.25 (`RgbaImage`), standard library only, deterministic synthetic fixtures for tests. No new dependencies in this plan.

---

## File Structure

- Modify: `crates/rollshot-core/src/lib.rs`
  New module decls (`overlap`, `verifier`, `canvas`, `axis`) and re-exports for the v0.2 public types.
- Replace: `crates/rollshot-core/src/types.rs`
  All v0.2 public types: `ScrollAxis`, `AppendDirection`, `MatchMethod`, `MatchStrategy`, `MotionCandidate`, `MotionEstimate`, `OverlapRegion`, `NoMatchReason`, `StitchOutcome`, `StitchConfig`, `VerifierConfig`, `StitchStats`.
- Create: `crates/rollshot-core/src/overlap.rs`
  `compute_overlap(prev_w, prev_h, curr_w, curr_h, dx, dy) -> Option<OverlapRegion>` plus unit tests for all four directions and edge cases.
- Create: `crates/rollshot-core/src/verifier.rs`
  `PixelOverlapVerifier::verify(prev, curr, candidate, config) -> VerifierOutcome` doing downsampled grayscale MAD + full-resolution sample-band MAD on the overlap region returned by `compute_overlap`.
- Create: `crates/rollshot-core/src/canvas.rs`
  `LinearCanvas` owning the stitched image and a locked `Option<ScrollAxis>`; `append(direction, frame, MotionEstimate)` with one branch per direction; width/height invariants per axis.
- Create: `crates/rollshot-core/src/axis.rs`
  `classify_axis(dx, dy, ratio_threshold) -> AxisClassification`, `validate_with_lock(locked, candidate, cross_axis_tolerance) -> AxisValidation`.
- Modify: `crates/rollshot-core/src/matcher.rs`
  Add `estimate_motion(prev, curr, last_dy, config) -> Option<MotionCandidate>` that wraps the existing `estimate_offset` and reports `MatchMethod::Template` candidates. Keep `estimate_offset` and the existing module-private helpers (Plan 2 evolves them).
- Modify: `crates/rollshot-core/src/stitcher.rs`
  Refactor to drive `matcher::estimate_motion → axis → verifier → LinearCanvas` and emit v0.2 outcomes. Stop using `image_ext::append_below`.
- Delete: `crates/rollshot-core/src/image_ext.rs`
  Orphaned once `Stitcher` switches to `LinearCanvas`.
- Modify: `crates/rollshot-core/tests/stitcher.rs`
  Update existing integration tests to the new `StitchOutcome` shape (no behavioral change, just destructuring).
- Create: `crates/rollshot-core/tests/canvas.rs`
  Plan-1 completion-criterion test suite: feed synthetic `MotionEstimate`s into a `LinearCanvas` and prove all four append directions stitch correctly.
- Create: `crates/rollshot-core/tests/verifier.rs`
  Plan-1 completion-criterion test suite: pass/reject behavior of `PixelOverlapVerifier` against synthetic frames with known motion.
- Modify: `crates/rollshot-cli/src/cmd_capture.rs`
  Update the `StitchOutcome` match arms (`NoProgress` is no longer a unit variant; new `AxisChanged` variant must be handled).
- Modify: `crates/rollshot-cli/src/cmd_stitch_folder.rs`
  Same outcome-match update as `cmd_capture.rs`.

---

## Task 1: Replace `types.rs` with the v0.2 Type Vocabulary

**Files:**
- Replace: `crates/rollshot-core/src/types.rs`

- [ ] **Step 1: Replace `types.rs` with the v0.2 type set**

Replace the entire contents of `crates/rollshot-core/src/types.rs` with:

```rust
//! Public v0.2 stitching types.
//!
//! `dx` and `dy` describe the current frame's top-left position relative to
//! the previous accepted frame in content coordinates:
//! - `dy > 0` means current frame sees lower content -> append `Bottom`.
//! - `dy < 0` means current frame sees higher content -> append `Top`.
//! - `dx > 0` means current frame sees rightward content -> append `Right`.
//! - `dx < 0` means current frame sees leftward content -> append `Left`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAxis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendDirection {
    Bottom,
    Top,
    Right,
    Left,
}

impl AppendDirection {
    pub fn axis(self) -> ScrollAxis {
        match self {
            AppendDirection::Bottom | AppendDirection::Top => ScrollAxis::Vertical,
            AppendDirection::Right | AppendDirection::Left => ScrollAxis::Horizontal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMethod {
    Template,
    Coarse,
    Edge,
    Akaze,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchStrategy {
    AutoHybrid,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionCandidate {
    pub dx: i32,
    pub dy: i32,
    pub method: MatchMethod,
    pub score: f32,
    pub second_best_score: Option<f32>,
    pub inliers: Option<usize>,
    pub raw_matches: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlapRegion {
    pub prev_x: u32,
    pub prev_y: u32,
    pub curr_x: u32,
    pub curr_y: u32,
    pub width: u32,
    pub height: u32,
}

impl OverlapRegion {
    pub fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionEstimate {
    pub dx: i32,
    pub dy: i32,
    pub axis: ScrollAxis,
    pub direction: AppendDirection,
    pub confidence: f32,
    pub method: MatchMethod,
    pub overlap: OverlapRegion,
    pub inliers: Option<usize>,
    pub raw_matches: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoMatchReason {
    LowConfidence,
    AmbiguousAxis,
    /// The candidate's cross-axis movement exceeded `max_cross_axis_px` while a
    /// scroll axis was already locked. Plan 1 cannot reach this through real
    /// frames (matcher always returns `dx = 0`); Plan 2's horizontal matching
    /// exercises it.
    CrossAxisTooLarge,
    InsufficientOverlap,
    OverlapVerificationFailed,
    NotEnoughFeatures,
    MotionTooSmall,
    DimensionMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StitchOutcome {
    FirstFrame,
    Appended {
        direction: AppendDirection,
        added: u32,
        estimate: MotionEstimate,
    },
    NoProgress {
        estimate: Option<MotionEstimate>,
    },
    Duplicate,
    NoMatch {
        reason: NoMatchReason,
        best_estimate: Option<MotionEstimate>,
    },
    AxisChanged {
        previous_axis: ScrollAxis,
        new_axis: ScrollAxis,
        estimate: MotionEstimate,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StitchStats {
    pub frame_count: u32,
    pub total_height: u32,
    pub total_width: u32,
    pub last_append: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VerifierConfig {
    /// Maximum mean absolute difference of the downsampled overlap, normalized to [0, 1].
    pub downsample_max_mad: f32,
    /// Maximum mean absolute difference of the full-resolution sample band, normalized to [0, 1].
    pub full_res_max_mad: f32,
    /// Linear downsample step used for the cheap overlap pass.
    pub downsample_step: u32,
    /// Height (or width, for horizontal motion) of the full-resolution sample band, in pixels.
    pub sample_band: u32,
}

impl Default for VerifierConfig {
    fn default() -> Self {
        Self {
            downsample_max_mad: 24.0 / 255.0,
            full_res_max_mad: 18.0 / 255.0,
            downsample_step: 4,
            sample_band: 160,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StitchConfig {
    pub strategy: MatchStrategy,
    pub min_overlap: u32,
    pub min_append: u32,
    pub duplicate_threshold: f32,
    pub accept_confidence: f32,
    pub axis_ratio_threshold: f32,
    pub max_cross_axis_px: i32,
    pub second_best_margin: f32,
    pub max_search_ratio: f32,
    pub match_width: u32,
    pub verifier: VerifierConfig,
}

impl Default for StitchConfig {
    fn default() -> Self {
        Self {
            strategy: MatchStrategy::AutoHybrid,
            min_overlap: 64,
            min_append: 8,
            duplicate_threshold: 0.01,
            accept_confidence: 0.15,
            axis_ratio_threshold: 1.5,
            max_cross_axis_px: 6,
            second_best_margin: 0.015,
            max_search_ratio: 0.75,
            match_width: 512,
            verifier: VerifierConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_picks_auto_hybrid() {
        let cfg = StitchConfig::default();
        assert_eq!(cfg.strategy, MatchStrategy::AutoHybrid);
        assert_eq!(cfg.min_overlap, 64);
        assert_eq!(cfg.axis_ratio_threshold, 1.5);
        assert_eq!(cfg.max_cross_axis_px, 6);
        assert_eq!(cfg.verifier.downsample_step, 4);
    }

    #[test]
    fn append_direction_axis_mapping() {
        assert_eq!(AppendDirection::Bottom.axis(), ScrollAxis::Vertical);
        assert_eq!(AppendDirection::Top.axis(), ScrollAxis::Vertical);
        assert_eq!(AppendDirection::Right.axis(), ScrollAxis::Horizontal);
        assert_eq!(AppendDirection::Left.axis(), ScrollAxis::Horizontal);
    }

    #[test]
    fn overlap_region_area_is_width_times_height() {
        let r = OverlapRegion {
            prev_x: 0,
            prev_y: 10,
            curr_x: 0,
            curr_y: 0,
            width: 100,
            height: 50,
        };
        assert_eq!(r.area(), 5000);
    }

    #[test]
    fn stitch_outcome_variants_are_distinct() {
        let dummy = MotionEstimate {
            dx: 0,
            dy: 12,
            axis: ScrollAxis::Vertical,
            direction: AppendDirection::Bottom,
            confidence: 0.05,
            method: MatchMethod::Template,
            overlap: OverlapRegion {
                prev_x: 0,
                prev_y: 12,
                curr_x: 0,
                curr_y: 0,
                width: 100,
                height: 88,
            },
            inliers: None,
            raw_matches: None,
        };
        let appended = StitchOutcome::Appended {
            direction: AppendDirection::Bottom,
            added: 12,
            estimate: dummy,
        };
        let no_match = StitchOutcome::NoMatch {
            reason: NoMatchReason::LowConfidence,
            best_estimate: Some(dummy),
        };
        let no_progress = StitchOutcome::NoProgress { estimate: None };
        assert_ne!(appended, StitchOutcome::FirstFrame);
        assert_ne!(no_match, no_progress);
        assert_ne!(no_match, StitchOutcome::Duplicate);
    }
}
```

- [ ] **Step 2: Update `lib.rs` re-exports to the v0.2 surface**

Replace `crates/rollshot-core/src/lib.rs` with:

```rust
mod duplicate;
mod image_ext;
mod matcher;
mod stitcher;
mod types;

pub use stitcher::Stitcher;
pub use types::{
    AppendDirection, MatchMethod, MatchStrategy, MotionCandidate, MotionEstimate, NoMatchReason,
    OverlapRegion, ScrollAxis, StitchConfig, StitchOutcome, StitchStats, VerifierConfig,
};
```

> The new `overlap`, `verifier`, `canvas`, `axis` modules are added in later tasks; their declarations join `lib.rs` then.

- [ ] **Step 3: Run the type unit tests**

Run: `rtk cargo test -p rollshot-core --lib types::tests -- --nocapture`
Expected: PASS for `default_config_picks_auto_hybrid`, `append_direction_axis_mapping`, `overlap_region_area_is_width_times_height`, `stitch_outcome_variants_are_distinct`.

> The workspace as a whole will not compile yet — `stitcher.rs`, `matcher.rs`, and the CLI still reference the v0.1 `OffsetEstimate`/`MatchAlgorithm`/old `StitchOutcome` shape. Test only the `types` module here. Compile-level fixes land in tasks 6, 7, and 8.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-core/src/types.rs crates/rollshot-core/src/lib.rs
git commit -m "feat(core): introduce v0.2 motion/canvas type vocabulary"
```

---

## Task 2: Generic 2D Overlap Rectangle (`overlap.rs`)

**Files:**
- Create: `crates/rollshot-core/src/overlap.rs`
- Modify: `crates/rollshot-core/src/lib.rs`

- [ ] **Step 1: Add module declaration + stub `compute_overlap` alongside failing tests**

Edit `crates/rollshot-core/src/lib.rs` and insert `mod overlap;` alphabetically:

```rust
mod duplicate;
mod image_ext;
mod matcher;
mod overlap;
mod stitcher;
mod types;
```

Create `crates/rollshot-core/src/overlap.rs` with the function signature stubbed via `unimplemented!()` and the full test suite already in place:

```rust
//! Generic 2D overlap rectangle math, independent of any matcher.

use crate::types::OverlapRegion;

/// Computes the rectangular overlap between `prev` and `curr` frames given a
/// candidate motion `(dx, dy)` where the current frame's top-left sits at
/// `(dx, dy)` in the previous frame's content coordinate space.
///
/// Returns `None` when there is no positive-area overlap.
pub fn compute_overlap(
    prev_w: u32,
    prev_h: u32,
    curr_w: u32,
    curr_h: u32,
    dx: i32,
    dy: i32,
) -> Option<OverlapRegion> {
    let _ = (prev_w, prev_h, curr_w, curr_h, dx, dy);
    unimplemented!("Task 2 Step 3 fills this in")
}

#[cfg(test)]
mod tests {
    use super::compute_overlap;

    #[test]
    fn vertical_down_overlap_lives_at_bottom_of_prev_and_top_of_curr() {
        let r = compute_overlap(100, 100, 100, 100, 0, 30).expect("overlap exists");
        assert_eq!(r.prev_x, 0);
        assert_eq!(r.prev_y, 30);
        assert_eq!(r.curr_x, 0);
        assert_eq!(r.curr_y, 0);
        assert_eq!(r.width, 100);
        assert_eq!(r.height, 70);
    }

    #[test]
    fn vertical_up_overlap_lives_at_top_of_prev_and_bottom_of_curr() {
        let r = compute_overlap(100, 100, 100, 100, 0, -25).expect("overlap exists");
        assert_eq!(r.prev_x, 0);
        assert_eq!(r.prev_y, 0);
        assert_eq!(r.curr_x, 0);
        assert_eq!(r.curr_y, 25);
        assert_eq!(r.width, 100);
        assert_eq!(r.height, 75);
    }

    #[test]
    fn horizontal_right_overlap_lives_at_right_of_prev_and_left_of_curr() {
        let r = compute_overlap(120, 80, 120, 80, 40, 0).expect("overlap exists");
        assert_eq!(r.prev_x, 40);
        assert_eq!(r.prev_y, 0);
        assert_eq!(r.curr_x, 0);
        assert_eq!(r.curr_y, 0);
        assert_eq!(r.width, 80);
        assert_eq!(r.height, 80);
    }

    #[test]
    fn horizontal_left_overlap_lives_at_left_of_prev_and_right_of_curr() {
        let r = compute_overlap(120, 80, 120, 80, -50, 0).expect("overlap exists");
        assert_eq!(r.prev_x, 0);
        assert_eq!(r.prev_y, 0);
        assert_eq!(r.curr_x, 50);
        assert_eq!(r.curr_y, 0);
        assert_eq!(r.width, 70);
        assert_eq!(r.height, 80);
    }

    #[test]
    fn motion_larger_than_frame_yields_no_overlap() {
        assert!(compute_overlap(100, 100, 100, 100, 0, 200).is_none());
        assert!(compute_overlap(100, 100, 100, 100, 0, -200).is_none());
        assert!(compute_overlap(100, 100, 100, 100, 200, 0).is_none());
        assert!(compute_overlap(100, 100, 100, 100, -200, 0).is_none());
    }

    #[test]
    fn zero_motion_returns_full_frame_overlap() {
        let r = compute_overlap(80, 60, 80, 60, 0, 0).expect("overlap exists");
        assert_eq!(r.width, 80);
        assert_eq!(r.height, 60);
        assert_eq!((r.prev_x, r.prev_y, r.curr_x, r.curr_y), (0, 0, 0, 0));
    }

    #[test]
    fn diagonal_motion_returns_inner_rectangle() {
        let r = compute_overlap(100, 100, 100, 100, 20, 30).expect("overlap exists");
        assert_eq!(r.prev_x, 20);
        assert_eq!(r.prev_y, 30);
        assert_eq!(r.curr_x, 0);
        assert_eq!(r.curr_y, 0);
        assert_eq!(r.width, 80);
        assert_eq!(r.height, 70);
    }
}
```

- [ ] **Step 2: Run tests to confirm RED**

Run: `rtk cargo test -p rollshot-core --lib overlap::tests`
Expected: FAIL — every test panics with `not implemented: Task 2 Step 3 fills this in`.

- [ ] **Step 3: Implement `compute_overlap`**

Replace the stubbed body in `crates/rollshot-core/src/overlap.rs` with the real implementation:

```rust
pub fn compute_overlap(
    prev_w: u32,
    prev_h: u32,
    curr_w: u32,
    curr_h: u32,
    dx: i32,
    dy: i32,
) -> Option<OverlapRegion> {
    let prev_w_i = prev_w as i32;
    let prev_h_i = prev_h as i32;
    let curr_w_i = curr_w as i32;
    let curr_h_i = curr_h as i32;

    let x_lo = dx.max(0);
    let y_lo = dy.max(0);
    let x_hi = (dx + curr_w_i).min(prev_w_i);
    let y_hi = (dy + curr_h_i).min(prev_h_i);

    if x_hi <= x_lo || y_hi <= y_lo {
        return None;
    }

    let width = (x_hi - x_lo) as u32;
    let height = (y_hi - y_lo) as u32;
    let prev_x = x_lo as u32;
    let prev_y = y_lo as u32;
    let curr_x = (x_lo - dx) as u32;
    let curr_y = (y_lo - dy) as u32;

    Some(OverlapRegion {
        prev_x,
        prev_y,
        curr_x,
        curr_y,
        width,
        height,
    })
}
```

- [ ] **Step 4: Run tests to confirm GREEN**

Run: `rtk cargo test -p rollshot-core --lib overlap::tests`
Expected: PASS — all seven test cases.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/overlap.rs crates/rollshot-core/src/lib.rs
git commit -m "feat(core): add generic 2D overlap rectangle helper"
```

---

## Task 3: Pixel Overlap Verifier (`verifier.rs`)

**Files:**
- Create: `crates/rollshot-core/src/verifier.rs`
- Modify: `crates/rollshot-core/src/lib.rs`

- [ ] **Step 1: Add module declaration + stub `verify` alongside failing tests**

Edit `crates/rollshot-core/src/lib.rs`, insert `mod verifier;` alphabetically:

```rust
mod duplicate;
mod image_ext;
mod matcher;
mod overlap;
mod stitcher;
mod types;
mod verifier;
```

Create `crates/rollshot-core/src/verifier.rs` with the public types in place, `verify` stubbed via `unimplemented!()`, and the full test suite:

```rust
//! Pixel-overlap verifier shared by every motion candidate.

use image::RgbaImage;

use crate::types::{MotionCandidate, OverlapRegion, VerifierConfig};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VerifierOutcome {
    /// Verification passed; `overlap` is the rectangle examined and the score
    /// is a normalized confidence contribution (lower = better, in [0, 1]).
    Pass {
        overlap: OverlapRegion,
        score: f32,
    },
    /// Verification failed because the overlap did not exist or was too small.
    InsufficientOverlap,
    /// Verification failed because the overlap pixels disagreed too strongly.
    OverlapDisagreement { downsample_mad: f32, full_mad: f32 },
}

pub struct PixelOverlapVerifier<'a> {
    config: &'a VerifierConfig,
    min_overlap_area: u64,
}

impl<'a> PixelOverlapVerifier<'a> {
    pub fn new(config: &'a VerifierConfig, min_overlap: u32) -> Self {
        Self {
            config,
            min_overlap_area: u64::from(min_overlap) * u64::from(min_overlap),
        }
    }

    pub fn verify(
        &self,
        prev: &RgbaImage,
        curr: &RgbaImage,
        candidate: &MotionCandidate,
    ) -> VerifierOutcome {
        let _ = (prev, curr, candidate, self.config, self.min_overlap_area);
        unimplemented!("Task 3 Step 3 fills this in")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{imageops, Rgba, RgbaImage};

    fn textured(width: u32, height: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(width, height, Rgba([240, 240, 240, 255]));
        for y in 0..height {
            for x in 0..width {
                if (x / 4 + y / 6) % 2 == 0 {
                    img.put_pixel(x, y, Rgba([30, ((x * 7) % 200) as u8, ((y * 11) % 200) as u8, 255]));
                }
            }
        }
        img
    }

    fn crop(canvas: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
        imageops::crop_imm(canvas, x, y, w, h).to_image()
    }

    fn candidate(dx: i32, dy: i32) -> MotionCandidate {
        MotionCandidate {
            dx,
            dy,
            method: MatchMethod::Template,
            score: 0.0,
            second_best_score: None,
            inliers: None,
            raw_matches: None,
        }
    }

    use crate::types::MatchMethod;

    #[test]
    fn matching_frames_with_known_motion_pass() {
        let canvas = textured(160, 320);
        let prev = crop(&canvas, 0, 0, 160, 160);
        let curr = crop(&canvas, 0, 40, 160, 160);
        let cfg = VerifierConfig::default();
        let verifier = PixelOverlapVerifier::new(&cfg, 64);
        match verifier.verify(&prev, &curr, &candidate(0, 40)) {
            VerifierOutcome::Pass { overlap, score } => {
                assert_eq!(overlap.height, 120);
                assert!(score < cfg.full_res_max_mad);
            }
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[test]
    fn unrelated_frames_fail_verification() {
        let prev = textured(160, 160);
        let curr = RgbaImage::from_pixel(160, 160, Rgba([255, 255, 255, 255]));
        let cfg = VerifierConfig::default();
        let verifier = PixelOverlapVerifier::new(&cfg, 64);
        let outcome = verifier.verify(&prev, &curr, &candidate(0, 20));
        assert!(matches!(
            outcome,
            VerifierOutcome::OverlapDisagreement { .. }
        ));
    }

    #[test]
    fn motion_with_no_overlap_returns_insufficient_overlap() {
        let prev = textured(64, 64);
        let curr = textured(64, 64);
        let cfg = VerifierConfig::default();
        let verifier = PixelOverlapVerifier::new(&cfg, 32);
        let outcome = verifier.verify(&prev, &curr, &candidate(0, 200));
        assert_eq!(outcome, VerifierOutcome::InsufficientOverlap);
    }

    #[test]
    fn overlap_below_min_overlap_area_returns_insufficient_overlap() {
        let canvas = textured(120, 240);
        let prev = crop(&canvas, 0, 0, 120, 120);
        let curr = crop(&canvas, 0, 115, 120, 120);
        let cfg = VerifierConfig::default();
        // min_overlap = 80 means area threshold 6400; overlap is 120*5 = 600.
        let verifier = PixelOverlapVerifier::new(&cfg, 80);
        let outcome = verifier.verify(&prev, &curr, &candidate(0, 115));
        assert_eq!(outcome, VerifierOutcome::InsufficientOverlap);
    }

    #[test]
    fn horizontal_right_motion_passes_with_aligned_crops() {
        let canvas = textured(320, 160);
        let prev = crop(&canvas, 0, 0, 160, 160);
        let curr = crop(&canvas, 40, 0, 160, 160);
        let cfg = VerifierConfig::default();
        let verifier = PixelOverlapVerifier::new(&cfg, 64);
        match verifier.verify(&prev, &curr, &candidate(40, 0)) {
            VerifierOutcome::Pass { overlap, .. } => {
                assert_eq!(overlap.width, 120);
                assert_eq!(overlap.height, 160);
            }
            other => panic!("expected Pass, got {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Run tests to confirm RED**

Run: `rtk cargo test -p rollshot-core --lib verifier::tests`
Expected: FAIL — every test panics with `not implemented: Task 3 Step 3 fills this in`.

- [ ] **Step 3: Implement `verify` and its grayscale-MAD helpers**

In `crates/rollshot-core/src/verifier.rs`, replace the `use` lines to add `Rgba` and the overlap import, replace the stubbed `verify` body with the real implementation, and add the three private helpers below the `impl` block.

First, replace the top of the file (`use image::RgbaImage;` and `use crate::types::...`) with:

```rust
use image::{Rgba, RgbaImage};

use crate::overlap::compute_overlap;
use crate::types::{MotionCandidate, OverlapRegion, VerifierConfig};
```

Then replace the stubbed `verify` body:

```rust
    pub fn verify(
        &self,
        prev: &RgbaImage,
        curr: &RgbaImage,
        candidate: &MotionCandidate,
    ) -> VerifierOutcome {
        let region = match compute_overlap(
            prev.width(),
            prev.height(),
            curr.width(),
            curr.height(),
            candidate.dx,
            candidate.dy,
        ) {
            Some(r) => r,
            None => return VerifierOutcome::InsufficientOverlap,
        };

        if region.area() < self.min_overlap_area {
            return VerifierOutcome::InsufficientOverlap;
        }

        let downsample_mad = downsampled_mad(prev, curr, region, self.config.downsample_step);
        if !downsample_mad.is_finite() || downsample_mad > self.config.downsample_max_mad {
            return VerifierOutcome::OverlapDisagreement {
                downsample_mad,
                full_mad: f32::NAN,
            };
        }

        let full_mad = sample_band_mad(prev, curr, region, self.config.sample_band);
        if !full_mad.is_finite() || full_mad > self.config.full_res_max_mad {
            return VerifierOutcome::OverlapDisagreement {
                downsample_mad,
                full_mad,
            };
        }

        let score = full_mad.clamp(0.0, 1.0);
        VerifierOutcome::Pass {
            overlap: region,
            score,
        }
    }
```

Then add these three private helpers immediately after the `impl PixelOverlapVerifier` block (before `#[cfg(test)]`):

```rust
fn pixel_gray(img: &RgbaImage, x: u32, y: u32) -> f32 {
    let Rgba([r, g, b, _]) = *img.get_pixel(x, y);
    0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
}

fn downsampled_mad(prev: &RgbaImage, curr: &RgbaImage, r: OverlapRegion, step: u32) -> f32 {
    let step = step.max(1);
    let mut sum = 0.0f32;
    let mut count = 0u32;
    let mut row = 0u32;
    while row < r.height {
        let mut col = 0u32;
        while col < r.width {
            let p = pixel_gray(prev, r.prev_x + col, r.prev_y + row);
            let c = pixel_gray(curr, r.curr_x + col, r.curr_y + row);
            sum += (p - c).abs();
            count += 1;
            col += step;
        }
        row += step;
    }
    if count == 0 {
        return f32::INFINITY;
    }
    sum / (count as f32 * 255.0)
}

fn sample_band_mad(prev: &RgbaImage, curr: &RgbaImage, r: OverlapRegion, sample_band: u32) -> f32 {
    if r.width == 0 || r.height == 0 {
        return f32::INFINITY;
    }
    let band_h = sample_band.min(r.height).max(1);
    let band_w = sample_band.min(r.width).max(1);
    // Walk the trailing band along the longer axis so vertical motion samples
    // the bottom rows of the overlap and horizontal motion samples its right
    // columns. This mirrors how scrolling content presents new pixels.
    let (use_h, use_w) = if r.height >= r.width {
        (band_h, r.width)
    } else {
        (r.height, band_w)
    };
    let row_start = r.height - use_h;
    let col_start = r.width - use_w;

    let mut sum = 0.0f32;
    let mut count = 0u32;
    for row in 0..use_h {
        for col in 0..use_w {
            let p = pixel_gray(prev, r.prev_x + col_start + col, r.prev_y + row_start + row);
            let c = pixel_gray(curr, r.curr_x + col_start + col, r.curr_y + row_start + row);
            sum += (p - c).abs();
            count += 1;
        }
    }
    if count == 0 {
        return f32::INFINITY;
    }
    sum / (count as f32 * 255.0)
}
```

- [ ] **Step 4: Run tests to confirm GREEN**

Run: `rtk cargo test -p rollshot-core --lib verifier::tests`
Expected: PASS — five test cases.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/verifier.rs crates/rollshot-core/src/lib.rs
git commit -m "feat(core): add generic pixel overlap verifier"
```

---

## Task 4: `LinearCanvas` with Four-Direction Append

**Files:**
- Create: `crates/rollshot-core/src/canvas.rs`
- Modify: `crates/rollshot-core/src/lib.rs`

- [ ] **Step 1: Add module declaration + re-export, then stub `append` alongside failing tests**

Edit `crates/rollshot-core/src/lib.rs`:

```rust
mod canvas;
mod duplicate;
mod image_ext;
mod matcher;
mod overlap;
mod stitcher;
mod types;
mod verifier;

pub use canvas::{CanvasAppendError, LinearCanvas};
pub use stitcher::Stitcher;
pub use types::{
    AppendDirection, MatchMethod, MatchStrategy, MotionCandidate, MotionEstimate, NoMatchReason,
    OverlapRegion, ScrollAxis, StitchConfig, StitchOutcome, StitchStats, VerifierConfig,
};
```

Create `crates/rollshot-core/src/canvas.rs` with the public types in place, the `append` body stubbed via `unimplemented!()`, and the full test suite (nine tests):

```rust
//! Single-axis stitched canvas that can grow in four directions.

use image::RgbaImage;

use crate::types::{AppendDirection, ScrollAxis};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasAppendError {
    /// The append direction's axis does not match the canvas's locked axis.
    AxisMismatch {
        locked: ScrollAxis,
        attempted: ScrollAxis,
    },
    /// The frame's perpendicular dimension does not match the canvas.
    DimensionMismatch { canvas: u32, frame: u32 },
    /// `slice_px` is zero -- nothing to append.
    EmptyAppend,
}

pub struct LinearCanvas {
    image: RgbaImage,
    axis: Option<ScrollAxis>,
}

impl LinearCanvas {
    pub fn new(first_frame: RgbaImage) -> Self {
        Self {
            image: first_frame,
            axis: None,
        }
    }

    pub fn image(&self) -> &RgbaImage {
        &self.image
    }

    pub fn into_image(self) -> RgbaImage {
        self.image
    }

    pub fn axis(&self) -> Option<ScrollAxis> {
        self.axis
    }

    pub fn width(&self) -> u32 {
        self.image.width()
    }

    pub fn height(&self) -> u32 {
        self.image.height()
    }

    /// Appends `slice_px` new pixels from `frame` in the given `direction`.
    ///
    /// `slice_px` is the number of new rows (Bottom/Top) or columns
    /// (Right/Left) the canvas should gain. The caller is expected to derive
    /// it from a verified `MotionEstimate` (`dy` for vertical, `dx` for
    /// horizontal). The non-stitching dimension of `frame` must equal the
    /// canvas's current matching dimension.
    pub fn append(
        &mut self,
        direction: AppendDirection,
        frame: &RgbaImage,
        slice_px: u32,
    ) -> Result<u32, CanvasAppendError> {
        let _ = (direction, frame, slice_px);
        unimplemented!("Task 4 Step 3 fills this in")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn solid(width: u32, height: u32, color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba(color))
    }

    #[test]
    fn append_bottom_adds_slice_below() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Bottom, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.height(), 6);
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.image().get_pixel(0, 5), &Rgba([200, 0, 0, 255]));
        assert_eq!(canvas.axis(), Some(ScrollAxis::Vertical));
    }

    #[test]
    fn prepend_top_adds_slice_above() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [0, 200, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Top, &frame, 3).unwrap();
        assert_eq!(added, 3);
        assert_eq!(canvas.height(), 7);
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([0, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 3), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.axis(), Some(ScrollAxis::Vertical));
    }

    #[test]
    fn append_right_adds_slice_to_the_right() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [0, 0, 200, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Right, &frame, 2).unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.width(), 6);
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.image().get_pixel(5, 0), &Rgba([0, 0, 200, 255]));
        assert_eq!(canvas.axis(), Some(ScrollAxis::Horizontal));
    }

    #[test]
    fn prepend_left_adds_slice_to_the_left() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [200, 200, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Left, &frame, 3).unwrap();
        assert_eq!(added, 3);
        assert_eq!(canvas.width(), 7);
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([200, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(3, 0), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.axis(), Some(ScrollAxis::Horizontal));
    }

    #[test]
    fn axis_lock_rejects_perpendicular_direction() {
        let mut canvas = LinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
        let frame = solid(4, 4, [1, 1, 1, 255]);
        canvas.append(AppendDirection::Bottom, &frame, 1).unwrap();
        let err = canvas
            .append(AppendDirection::Right, &frame, 1)
            .unwrap_err();
        assert_eq!(
            err,
            CanvasAppendError::AxisMismatch {
                locked: ScrollAxis::Vertical,
                attempted: ScrollAxis::Horizontal,
            }
        );
    }

    #[test]
    fn dimension_mismatch_is_reported() {
        let mut canvas = LinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
        let frame = solid(6, 4, [1, 1, 1, 255]);
        let err = canvas
            .append(AppendDirection::Bottom, &frame, 1)
            .unwrap_err();
        assert_eq!(
            err,
            CanvasAppendError::DimensionMismatch {
                canvas: 4,
                frame: 6,
            }
        );
    }

    #[test]
    fn dimension_mismatch_in_horizontal_mode_is_reported() {
        let mut canvas = LinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
        let frame = solid(4, 6, [1, 1, 1, 255]);
        let err = canvas
            .append(AppendDirection::Right, &frame, 1)
            .unwrap_err();
        assert_eq!(
            err,
            CanvasAppendError::DimensionMismatch {
                canvas: 4,
                frame: 6,
            }
        );
    }

    #[test]
    fn zero_slice_px_is_rejected() {
        let mut canvas = LinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
        let frame = solid(4, 4, [1, 1, 1, 255]);
        let err = canvas
            .append(AppendDirection::Bottom, &frame, 0)
            .unwrap_err();
        assert_eq!(err, CanvasAppendError::EmptyAppend);
    }

    #[test]
    fn slice_larger_than_frame_is_clamped_to_frame_size() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [0, 200, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas.append(AppendDirection::Bottom, &frame, 99).unwrap();
        assert_eq!(added, 4);
        assert_eq!(canvas.height(), 8);
    }
}
```

- [ ] **Step 2: Run tests to confirm RED**

Run: `rtk cargo test -p rollshot-core --lib canvas::tests`
Expected: FAIL — every test panics with `not implemented: Task 4 Step 3 fills this in`.

- [ ] **Step 3: Implement `append` and the four direction helpers**

In `crates/rollshot-core/src/canvas.rs`, replace the top use line `use image::RgbaImage;` with the wider import needed by the helpers:

```rust
use image::{GenericImage, GenericImageView, RgbaImage};
```

Replace the stubbed `append` body with the real implementation:

```rust
    pub fn append(
        &mut self,
        direction: AppendDirection,
        frame: &RgbaImage,
        slice_px: u32,
    ) -> Result<u32, CanvasAppendError> {
        let target_axis = direction.axis();
        if let Some(locked) = self.axis {
            if locked != target_axis {
                return Err(CanvasAppendError::AxisMismatch {
                    locked,
                    attempted: target_axis,
                });
            }
        }

        match target_axis {
            ScrollAxis::Vertical => {
                if frame.width() != self.image.width() {
                    return Err(CanvasAppendError::DimensionMismatch {
                        canvas: self.image.width(),
                        frame: frame.width(),
                    });
                }
            }
            ScrollAxis::Horizontal => {
                if frame.height() != self.image.height() {
                    return Err(CanvasAppendError::DimensionMismatch {
                        canvas: self.image.height(),
                        frame: frame.height(),
                    });
                }
            }
        }

        if slice_px == 0 {
            return Err(CanvasAppendError::EmptyAppend);
        }

        let added = match direction {
            AppendDirection::Bottom => self.append_bottom(frame, slice_px),
            AppendDirection::Top => self.prepend_top(frame, slice_px),
            AppendDirection::Right => self.append_right(frame, slice_px),
            AppendDirection::Left => self.prepend_left(frame, slice_px),
        };

        self.axis = Some(target_axis);
        Ok(added)
    }
```

Then add these four private helpers inside the `impl LinearCanvas` block (immediately below `append`):

```rust
    fn append_bottom(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let slice_px = slice_px.min(frame.height());
        let overlap = frame.height() - slice_px;
        let slice = frame
            .view(0, overlap, frame.width(), slice_px)
            .to_image();
        let mut combined = RgbaImage::new(self.image.width(), self.image.height() + slice_px);
        combined.copy_from(&self.image, 0, 0).expect("copy base");
        combined
            .copy_from(&slice, 0, self.image.height())
            .expect("copy slice");
        self.image = combined;
        slice_px
    }

    fn prepend_top(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let slice_px = slice_px.min(frame.height());
        let slice = frame.view(0, 0, frame.width(), slice_px).to_image();
        let mut combined = RgbaImage::new(self.image.width(), self.image.height() + slice_px);
        combined.copy_from(&slice, 0, 0).expect("copy slice");
        combined
            .copy_from(&self.image, 0, slice_px)
            .expect("copy base");
        self.image = combined;
        slice_px
    }

    fn append_right(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let slice_px = slice_px.min(frame.width());
        let overlap = frame.width() - slice_px;
        let slice = frame
            .view(overlap, 0, slice_px, frame.height())
            .to_image();
        let mut combined = RgbaImage::new(self.image.width() + slice_px, self.image.height());
        combined.copy_from(&self.image, 0, 0).expect("copy base");
        combined
            .copy_from(&slice, self.image.width(), 0)
            .expect("copy slice");
        self.image = combined;
        slice_px
    }

    fn prepend_left(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let slice_px = slice_px.min(frame.width());
        let slice = frame.view(0, 0, slice_px, frame.height()).to_image();
        let mut combined = RgbaImage::new(self.image.width() + slice_px, self.image.height());
        combined.copy_from(&slice, 0, 0).expect("copy slice");
        combined
            .copy_from(&self.image, slice_px, 0)
            .expect("copy base");
        self.image = combined;
        slice_px
    }
```

- [ ] **Step 4: Run tests to confirm GREEN**

Run: `rtk cargo test -p rollshot-core --lib canvas::tests`
Expected: PASS — nine test cases.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/canvas.rs crates/rollshot-core/src/lib.rs
git commit -m "feat(core): add LinearCanvas with four-direction append"
```

---

## Task 5: Axis Detection and Locking (`axis.rs`)

**Files:**
- Create: `crates/rollshot-core/src/axis.rs`
- Modify: `crates/rollshot-core/src/lib.rs`

- [ ] **Step 1: Add module declaration + stub `classify_axis` and `validate_with_lock` alongside failing tests**

Edit `crates/rollshot-core/src/lib.rs`, insert `mod axis;` alphabetically (before `canvas`):

```rust
mod axis;
mod canvas;
mod duplicate;
mod image_ext;
mod matcher;
mod overlap;
mod stitcher;
mod types;
mod verifier;
```

Create `crates/rollshot-core/src/axis.rs` with the public types in place, both functions stubbed via `unimplemented!()`, and the full test suite:

```rust
//! Axis detection and single-axis lock validation.

use crate::types::{AppendDirection, ScrollAxis};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisClassification {
    Vertical { direction: AppendDirection },
    Horizontal { direction: AppendDirection },
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisValidation {
    /// Candidate matches the locked axis and stays within `max_cross_axis_px`.
    OnAxis { direction: AppendDirection },
    /// Cross-axis movement exceeded `max_cross_axis_px` while a locked axis was set.
    CrossAxisTooLarge,
    /// Candidate is reliable on the opposite axis from the lock.
    AxisChanged { new_axis: ScrollAxis },
}

/// Classifies a `(dx, dy)` candidate using the rollshot axis-ratio rule.
pub fn classify_axis(dx: i32, dy: i32, ratio_threshold: f32) -> AxisClassification {
    let _ = (dx, dy, ratio_threshold);
    unimplemented!("Task 5 Step 3 fills this in")
}

/// Validates a candidate against a locked axis. Cross-axis movement above the
/// tolerance is treated as a real axis change rather than noise.
pub fn validate_with_lock(
    locked: ScrollAxis,
    dx: i32,
    dy: i32,
    max_cross_axis_px: i32,
) -> AxisValidation {
    let _ = (locked, dx, dy, max_cross_axis_px);
    unimplemented!("Task 5 Step 3 fills this in")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_down_motion_classifies_bottom() {
        assert_eq!(
            classify_axis(0, 30, 1.5),
            AxisClassification::Vertical {
                direction: AppendDirection::Bottom
            }
        );
    }

    #[test]
    fn vertical_up_motion_classifies_top() {
        assert_eq!(
            classify_axis(0, -30, 1.5),
            AxisClassification::Vertical {
                direction: AppendDirection::Top
            }
        );
    }

    #[test]
    fn horizontal_right_motion_classifies_right() {
        assert_eq!(
            classify_axis(40, 0, 1.5),
            AxisClassification::Horizontal {
                direction: AppendDirection::Right
            }
        );
    }

    #[test]
    fn horizontal_left_motion_classifies_left() {
        assert_eq!(
            classify_axis(-40, 0, 1.5),
            AxisClassification::Horizontal {
                direction: AppendDirection::Left
            }
        );
    }

    #[test]
    fn diagonal_motion_within_ratio_is_ambiguous() {
        // dx = 20, dy = 25; neither beats the other by 1.5x.
        assert_eq!(
            classify_axis(20, 25, 1.5),
            AxisClassification::Ambiguous
        );
    }

    #[test]
    fn zero_motion_is_ambiguous() {
        assert_eq!(classify_axis(0, 0, 1.5), AxisClassification::Ambiguous);
    }

    #[test]
    fn vertical_lock_accepts_small_cross_axis() {
        let v = validate_with_lock(ScrollAxis::Vertical, 3, 40, 6);
        assert_eq!(
            v,
            AxisValidation::OnAxis {
                direction: AppendDirection::Bottom
            }
        );
    }

    #[test]
    fn vertical_lock_flags_too_large_cross_axis_as_noise() {
        let v = validate_with_lock(ScrollAxis::Vertical, 12, 40, 6);
        assert_eq!(v, AxisValidation::CrossAxisTooLarge);
    }

    #[test]
    fn vertical_lock_reports_axis_change_when_horizontal_dominates() {
        let v = validate_with_lock(ScrollAxis::Vertical, 60, 4, 6);
        assert_eq!(
            v,
            AxisValidation::AxisChanged {
                new_axis: ScrollAxis::Horizontal
            }
        );
    }

    #[test]
    fn horizontal_lock_accepts_left_motion() {
        let v = validate_with_lock(ScrollAxis::Horizontal, -30, 2, 6);
        assert_eq!(
            v,
            AxisValidation::OnAxis {
                direction: AppendDirection::Left
            }
        );
    }
}
```

- [ ] **Step 2: Run tests to confirm RED**

Run: `rtk cargo test -p rollshot-core --lib axis::tests`
Expected: FAIL — every test panics with `not implemented: Task 5 Step 3 fills this in`.

- [ ] **Step 3: Implement `classify_axis` and `validate_with_lock`**

In `crates/rollshot-core/src/axis.rs`, replace the stubbed `classify_axis` body with:

```rust
pub fn classify_axis(dx: i32, dy: i32, ratio_threshold: f32) -> AxisClassification {
    let adx = dx.unsigned_abs() as f32;
    let ady = dy.unsigned_abs() as f32;

    if adx == 0.0 && ady == 0.0 {
        return AxisClassification::Ambiguous;
    }

    if ady > adx * ratio_threshold {
        let direction = if dy >= 0 {
            AppendDirection::Bottom
        } else {
            AppendDirection::Top
        };
        return AxisClassification::Vertical { direction };
    }

    if adx > ady * ratio_threshold {
        let direction = if dx >= 0 {
            AppendDirection::Right
        } else {
            AppendDirection::Left
        };
        return AxisClassification::Horizontal { direction };
    }

    AxisClassification::Ambiguous
}
```

Then replace the stubbed `validate_with_lock` body with:

```rust
pub fn validate_with_lock(
    locked: ScrollAxis,
    dx: i32,
    dy: i32,
    max_cross_axis_px: i32,
) -> AxisValidation {
    let cross = match locked {
        ScrollAxis::Vertical => dx.abs(),
        ScrollAxis::Horizontal => dy.abs(),
    };

    if cross <= max_cross_axis_px {
        let direction = match locked {
            ScrollAxis::Vertical => {
                if dy >= 0 {
                    AppendDirection::Bottom
                } else {
                    AppendDirection::Top
                }
            }
            ScrollAxis::Horizontal => {
                if dx >= 0 {
                    AppendDirection::Right
                } else {
                    AppendDirection::Left
                }
            }
        };
        return AxisValidation::OnAxis { direction };
    }

    let main = match locked {
        ScrollAxis::Vertical => dy.abs(),
        ScrollAxis::Horizontal => dx.abs(),
    };

    if cross > main {
        let new_axis = match locked {
            ScrollAxis::Vertical => ScrollAxis::Horizontal,
            ScrollAxis::Horizontal => ScrollAxis::Vertical,
        };
        AxisValidation::AxisChanged { new_axis }
    } else {
        AxisValidation::CrossAxisTooLarge
    }
}
```

- [ ] **Step 4: Run tests to confirm GREEN**

Run: `rtk cargo test -p rollshot-core --lib axis::tests`
Expected: PASS — ten test cases.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/axis.rs crates/rollshot-core/src/lib.rs
git commit -m "feat(core): add axis classification and lock validation"
```

---

## Task 6: Wrap the v0.1 Template Matcher into a `MotionCandidate`

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`

> The v0.1 `estimate_offset` keeps producing dy-only template estimates. This task adds an outward-facing `estimate_motion` shim that produces a `MotionCandidate` so `Stitcher` (refactored next task) and Plan 2 can both consume it uniformly.

- [ ] **Step 1: Update `matcher.rs` to import the new v0.2 types and add `estimate_motion`**

At the top of `crates/rollshot-core/src/matcher.rs`, replace the existing first three lines:

```rust
use image::{Rgba, RgbaImage};

use crate::types::{OffsetEstimate, StitchConfig};
```

with:

```rust
use image::{Rgba, RgbaImage};

use crate::types::{MatchMethod, MotionCandidate, StitchConfig};
```

- [ ] **Step 2: Remove `OffsetEstimate` from `estimate_offset`'s signature**

`OffsetEstimate` no longer exists. Replace the `estimate_offset` function (the docs and the function body, lines spanning the doc comment through `}`) with an internal-only version that returns a low-level result, then add the public `estimate_motion`. Replace the whole `estimate_offset` definition with the following:

```rust
/// Internal vertical-template result.
///
/// `confidence` follows the rollshot v0.1 convention: lower is better, and a
/// caller-facing accept threshold of `StitchConfig::accept_confidence` decides
/// whether the candidate is usable. `f32::INFINITY` means the inputs were not
/// usable at all (dimension mismatch, ROI empty, second-best margin too tight,
/// verification disagreement).
struct VerticalTemplateEstimate {
    dy: i32,
    confidence: f32,
    second_best_score: Option<f32>,
}

fn estimate_vertical_template(
    prev: &RgbaImage,
    curr: &RgbaImage,
    last_offset: i32,
    config: &StitchConfig,
) -> VerticalTemplateEstimate {
    let no_match = VerticalTemplateEstimate {
        dy: 0,
        confidence: f32::INFINITY,
        second_best_score: None,
    };

    if prev.dimensions() != curr.dimensions() {
        return no_match;
    }

    let width = prev.width();
    let height = prev.height();
    if height < 100 || width < 50 {
        return no_match;
    }

    let roi = content_roi(width, height);
    let match_region = match_width_region(roi, config.match_width);
    if roi.h < TEMPLATE_MIN_HEIGHT * 2 || match_region.w < 40 {
        return no_match;
    }

    let template_h = (roi.h / 3).max(TEMPLATE_MIN_HEIGHT).min(roi.h - 1);
    let search_start = roi.y as i32;
    let search_end = (roi.y + roi.h - template_h) as i32;
    if search_end <= search_start {
        return no_match;
    }

    let prev_gray = to_grayscale(prev);
    let curr_gray = to_grayscale(curr);

    let max_offset = (height as i32 - config.min_overlap as i32)
        .max(0)
        .min(search_end - search_start);
    let predict = last_offset.clamp(0, max_offset);

    let mut best_offset = 0i32;
    let mut best_score = f32::MIN;
    let mut second_score = f32::MIN;

    for offset in predict_iter(max_offset, predict) {
        let search_y = search_start + offset;

        let curr_template = Region {
            y: roi.y,
            h: template_h,
            ..match_region
        };
        let prev_template = Region {
            y: search_y as u32,
            ..curr_template
        };
        let score = ncc_score_region(&prev_gray, &curr_gray, width, prev_template, curr_template);

        if score > best_score {
            second_score = best_score;
            best_score = score;
            best_offset = offset;
        } else if score > second_score {
            second_score = score;
        }
    }

    if !best_score.is_finite() || best_score <= 0.0 {
        return no_match;
    }

    if second_score.is_finite() && best_score - second_score < config.second_best_margin {
        return no_match;
    }

    let overlap_h = height.saturating_sub(best_offset as u32);
    let overlap_region = Region {
        y: 0,
        h: overlap_h,
        ..match_region
    };
    let verify = overlap_mean_abs_diff(
        &prev_gray,
        &curr_gray,
        width,
        overlap_region,
        best_offset as u32,
    );

    if !verify.is_finite() || verify > VERIFY_MAX_NORMALIZED_DIFF {
        return no_match;
    }

    let confidence = (1.0 - best_score.clamp(0.0, 1.0)) + verify * 0.5;
    VerticalTemplateEstimate {
        dy: best_offset,
        confidence,
        second_best_score: if second_score.is_finite() {
            Some(second_score)
        } else {
            None
        },
    }
}

/// Public v0.2 entrypoint. Produces a single vertical `MotionCandidate` from
/// the template matcher. Plan 2 evolves this into a multi-candidate hybrid
/// generator; Plan 1 ships only the vertical template path.
///
/// Returns `None` when the template path could not produce a usable estimate
/// at all. Callers downstream still run the candidate through the pixel
/// overlap verifier.
pub fn estimate_motion(
    prev: &RgbaImage,
    curr: &RgbaImage,
    last_offset: i32,
    config: &StitchConfig,
) -> Option<MotionCandidate> {
    let raw = estimate_vertical_template(prev, curr, last_offset, config);
    if !raw.confidence.is_finite() {
        return None;
    }
    Some(MotionCandidate {
        dx: 0,
        dy: raw.dy,
        method: MatchMethod::Template,
        score: raw.confidence,
        second_best_score: raw.second_best_score,
        inliers: None,
        raw_matches: None,
    })
}
```

> Also delete the v0.1 module-private constant `SECOND_BEST_MIN_MARGIN` from the top of `matcher.rs`. Its usage above now reads `config.second_best_margin` instead.

To delete `SECOND_BEST_MIN_MARGIN`, remove its line. After your edits, the constants section at the top of the file should be:

```rust
const TOP_IGNORE_RATIO: f32 = 0.12;
const BOTTOM_IGNORE_RATIO: f32 = 0.08;
const SIDE_IGNORE_RATIO: f32 = 0.04;
const MIN_IGNORE_PX: u32 = 24;
const TEMPLATE_MIN_HEIGHT: u32 = 48;
const VERIFY_MAX_NORMALIZED_DIFF: f32 = 18.0 / 255.0;
```

- [ ] **Step 3: Update the existing matcher unit tests in `matcher.rs`**

The existing tests at the bottom of `matcher.rs` reference `estimate_offset`, `OffsetEstimate`, `MatchAlgorithm`, and `accept_diff`. Replace the entire `#[cfg(test)] mod tests { ... }` block at the bottom of `matcher.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::{content_roi, estimate_motion};
    use crate::types::{MatchMethod, StitchConfig};
    use image::{imageops, Rgba, RgbaImage};

    fn make_textured_canvas(width: u32, height: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(width, height, Rgba([245, 245, 245, 255]));
        for y in (0..height).step_by(11) {
            let accent = ((y / 3) % 180) as u8;
            for x in 8..width.saturating_sub(8) {
                let stripe = if (x / 5 + y / 7) % 2 == 0 { 220 } else { 180 };
                img.put_pixel(x, y, Rgba([accent, stripe, 80, 255]));
                if y + 1 < height {
                    img.put_pixel(x, y + 1, Rgba([30, 30, 30, 255]));
                }
            }
        }
        for col in [21, 47, 73, 99, 125] {
            if col >= width {
                continue;
            }
            for y in 12..height.saturating_sub(12) {
                if (y / 13) % 3 != 0 {
                    img.put_pixel(col, y, Rgba([20, 20, 20, 255]));
                }
            }
        }
        img
    }

    fn crop(canvas: &RgbaImage, y: u32, h: u32) -> RgbaImage {
        imageops::crop_imm(canvas, 0, y, canvas.width(), h).to_image()
    }

    #[test]
    fn content_roi_skips_borders() {
        let roi = content_roi(320, 320);
        assert!(roi.x >= 24);
        assert!(roi.y >= 24);
        assert!(roi.w < 320);
        assert!(roi.h < 320);
    }

    #[test]
    fn estimate_motion_respects_min_overlap() {
        let canvas = make_textured_canvas(320, 800);
        let prev = crop(&canvas, 0, 320);
        let curr = crop(&canvas, 120, 320);
        let config = StitchConfig {
            min_overlap: 280,
            ..StitchConfig::default()
        };
        let candidate = estimate_motion(&prev, &curr, 0, &config).expect("template candidate");
        assert!(candidate.dy <= 40, "dy = {} exceeds bounded search", candidate.dy);
    }

    #[test]
    fn estimate_motion_finds_known_scroll() {
        let canvas = make_textured_canvas(160, 600);
        let prev = crop(&canvas, 0, 160);
        let curr = crop(&canvas, 40, 160);
        let candidate = estimate_motion(&prev, &curr, 0, &StitchConfig::default())
            .expect("template candidate");
        assert_eq!(candidate.method, MatchMethod::Template);
        assert_eq!(candidate.dx, 0);
        assert!(
            (candidate.dy - 40).abs() <= 2,
            "dy = {} (expected ~40)",
            candidate.dy
        );
    }

    #[test]
    fn estimate_motion_returns_none_for_unrelated_frames() {
        let prev = make_textured_canvas(160, 160);
        let curr = RgbaImage::from_pixel(160, 160, Rgba([255, 255, 255, 255]));
        assert!(estimate_motion(&prev, &curr, 0, &StitchConfig::default()).is_none());
    }

    #[test]
    fn estimate_motion_returns_none_for_dimension_mismatch() {
        let prev = make_textured_canvas(160, 160);
        let curr = make_textured_canvas(160, 200);
        assert!(estimate_motion(&prev, &curr, 0, &StitchConfig::default()).is_none());
    }
}
```

- [ ] **Step 4: Run the matcher tests**

Run: `rtk cargo test -p rollshot-core --lib matcher::tests`
Expected: PASS — five test cases.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/matcher.rs
git commit -m "feat(core): expose template matcher as MotionCandidate generator"
```

---

## Task 7: Refactor `Stitcher` onto `LinearCanvas` + Verifier + Axis Lock

**Files:**
- Modify: `crates/rollshot-core/src/stitcher.rs`
- Delete: `crates/rollshot-core/src/image_ext.rs`
- Modify: `crates/rollshot-core/src/lib.rs`
- Modify: `crates/rollshot-core/tests/stitcher.rs`

- [ ] **Step 1: Replace `stitcher.rs` with the v0.2 state machine**

Replace the entire contents of `crates/rollshot-core/src/stitcher.rs` with:

```rust
use image::RgbaImage;

use crate::axis::{classify_axis, validate_with_lock, AxisClassification, AxisValidation};
use crate::canvas::{CanvasAppendError, LinearCanvas};
use crate::duplicate;
use crate::matcher::estimate_motion;
use crate::overlap::compute_overlap;
use crate::types::{
    AppendDirection, MotionCandidate, MotionEstimate, NoMatchReason, ScrollAxis, StitchConfig,
    StitchOutcome, StitchStats,
};
use crate::verifier::{PixelOverlapVerifier, VerifierOutcome};

pub struct Stitcher {
    config: StitchConfig,
    canvas: Option<LinearCanvas>,
    last_good_frame: Option<RgbaImage>,
    last_good_signature: Option<Vec<u8>>,
    last_offset: i32,
    locked_axis: Option<ScrollAxis>,
    stats: StitchStats,
}

impl Stitcher {
    pub fn new(config: StitchConfig) -> Self {
        Self {
            config,
            canvas: None,
            last_good_frame: None,
            last_good_signature: None,
            last_offset: 0,
            locked_axis: None,
            stats: StitchStats::default(),
        }
    }

    /// Feeds a frame through the v0.2 stitching pipeline.
    ///
    /// State machine:
    ///
    /// ```text
    ///                  push_frame(frame)
    ///                         |
    ///              canvas empty? --yes--> accept_first_frame --> FirstFrame
    ///                         | no
    ///                         v
    ///              dims == anchor? --no--> NoMatch { DimensionMismatch }
    ///                         | yes
    ///                         v
    ///              duplicate(anchor, frame)? --yes--> Duplicate
    ///                         | no
    ///                         v
    ///              estimate_motion(anchor, frame) --None--> NoMatch { LowConfidence }
    ///                         | Some(c)
    ///                         v
    ///              c.score > accept_confidence? --yes--> NoMatch { LowConfidence }
    ///                         | no
    ///                         v
    ///              classify_direction(c):
    ///                Ambiguous              -> NoMatch { AmbiguousAxis }
    ///                CrossAxisTooLarge      -> NoMatch { CrossAxisTooLarge }
    ///                AxisChanged            -> AxisChanged
    ///                Direction(dir)         v
    ///                         |
    ///                         v
    ///              slice_px < min_append? --yes--> NoProgress { estimate }
    ///                         | no
    ///                         v
    ///              verifier.verify(anchor, frame, c):
    ///                InsufficientOverlap    -> NoMatch { InsufficientOverlap }
    ///                OverlapDisagreement    -> NoMatch { OverlapVerificationFailed }
    ///                Pass(overlap, _)       v
    ///                         |
    ///                         v
    ///              canvas.append(dir, frame, slice_px):
    ///                AxisMismatch           -> AxisChanged (defensive)
    ///                DimensionMismatch      -> NoMatch { DimensionMismatch }
    ///                EmptyAppend            -> NoProgress { estimate }
    ///                Ok(added)              -> Appended { dir, added, estimate }
    /// ```
    ///
    /// Bad frames never poison the anchor: any branch ending in `NoMatch`,
    /// `NoProgress`, `Duplicate`, or `AxisChanged` leaves `last_good_frame`,
    /// `last_good_signature`, and `last_offset` unchanged.
    pub fn push_frame(&mut self, frame: RgbaImage) -> StitchOutcome {
        if self.canvas.is_none() {
            return self.accept_first_frame(frame);
        }

        let anchor = self
            .last_good_frame
            .as_ref()
            .expect("anchor present after first frame");

        if anchor.dimensions() != frame.dimensions() {
            return StitchOutcome::NoMatch {
                reason: NoMatchReason::DimensionMismatch,
                best_estimate: None,
            };
        }

        let signature = duplicate::signature(&frame);
        if let Some(prev_sig) = self.last_good_signature.as_ref() {
            if duplicate::is_duplicate(prev_sig, &signature, self.config.duplicate_threshold) {
                return StitchOutcome::Duplicate;
            }
        }

        let candidate = match estimate_motion(anchor, &frame, self.last_offset, &self.config) {
            Some(c) => c,
            None => {
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::LowConfidence,
                    best_estimate: None,
                };
            }
        };

        if candidate.score > self.config.accept_confidence {
            return StitchOutcome::NoMatch {
                reason: NoMatchReason::LowConfidence,
                best_estimate: build_estimate(anchor, &frame, &candidate, self.config.axis_ratio_threshold),
            };
        }

        let direction = match self.classify_direction(&candidate) {
            DirectionResult::Direction(dir) => dir,
            DirectionResult::Ambiguous => {
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::AmbiguousAxis,
                    best_estimate: build_estimate(anchor, &frame, &candidate, self.config.axis_ratio_threshold),
                };
            }
            DirectionResult::CrossAxisTooLarge => {
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::CrossAxisTooLarge,
                    best_estimate: build_estimate(
                        anchor,
                        &frame,
                        &candidate,
                        self.config.axis_ratio_threshold,
                    ),
                };
            }
            DirectionResult::AxisChanged { new_axis, locked } => {
                let estimate = build_estimate(anchor, &frame, &candidate, self.config.axis_ratio_threshold)
                    .expect("axis-change estimate must compute overlap");
                return StitchOutcome::AxisChanged {
                    previous_axis: locked,
                    new_axis,
                    estimate,
                };
            }
        };

        let slice_px = match direction {
            AppendDirection::Bottom | AppendDirection::Top => candidate.dy.unsigned_abs(),
            AppendDirection::Right | AppendDirection::Left => candidate.dx.unsigned_abs(),
        };
        if slice_px < self.config.min_append {
            return StitchOutcome::NoProgress {
                estimate: build_estimate(anchor, &frame, &candidate, self.config.axis_ratio_threshold),
            };
        }

        let verifier = PixelOverlapVerifier::new(&self.config.verifier, self.config.min_overlap);
        let (overlap_region, _verifier_score) = match verifier.verify(anchor, &frame, &candidate) {
            VerifierOutcome::Pass { overlap, score } => (overlap, score),
            VerifierOutcome::InsufficientOverlap => {
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::InsufficientOverlap,
                    best_estimate: build_estimate(anchor, &frame, &candidate, self.config.axis_ratio_threshold),
                };
            }
            VerifierOutcome::OverlapDisagreement { .. } => {
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::OverlapVerificationFailed,
                    best_estimate: build_estimate(anchor, &frame, &candidate, self.config.axis_ratio_threshold),
                };
            }
        };

        let canvas = self
            .canvas
            .as_mut()
            .expect("canvas present after first frame");
        let added = match canvas.append(direction, &frame, slice_px) {
            Ok(n) => n,
            Err(CanvasAppendError::AxisMismatch { locked, attempted }) => {
                let estimate = MotionEstimate {
                    dx: candidate.dx,
                    dy: candidate.dy,
                    axis: attempted,
                    direction,
                    confidence: candidate.score,
                    method: candidate.method,
                    overlap: overlap_region,
                    inliers: candidate.inliers,
                    raw_matches: candidate.raw_matches,
                };
                return StitchOutcome::AxisChanged {
                    previous_axis: locked,
                    new_axis: attempted,
                    estimate,
                };
            }
            Err(CanvasAppendError::DimensionMismatch { .. }) => {
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::DimensionMismatch,
                    best_estimate: build_estimate(anchor, &frame, &candidate, self.config.axis_ratio_threshold),
                };
            }
            Err(CanvasAppendError::EmptyAppend) => {
                return StitchOutcome::NoProgress {
                    estimate: build_estimate(anchor, &frame, &candidate, self.config.axis_ratio_threshold),
                };
            }
        };

        self.locked_axis = Some(direction.axis());
        self.last_offset = candidate.dy;

        let estimate = MotionEstimate {
            dx: candidate.dx,
            dy: candidate.dy,
            axis: direction.axis(),
            direction,
            confidence: candidate.score,
            method: candidate.method,
            overlap: overlap_region,
            inliers: candidate.inliers,
            raw_matches: candidate.raw_matches,
        };

        self.last_good_signature = Some(signature);
        self.last_good_frame = Some(frame);
        self.stats.frame_count += 1;
        self.stats.total_height = canvas.height();
        self.stats.total_width = canvas.width();
        self.stats.last_append = added;

        StitchOutcome::Appended {
            direction,
            added,
            estimate,
        }
    }

    pub fn full_image(&self) -> Option<&RgbaImage> {
        self.canvas.as_ref().map(LinearCanvas::image)
    }

    pub fn stats(&self) -> StitchStats {
        self.stats
    }

    fn accept_first_frame(&mut self, frame: RgbaImage) -> StitchOutcome {
        let height = frame.height();
        let width = frame.width();
        self.stats = StitchStats {
            frame_count: 1,
            total_height: height,
            total_width: width,
            last_append: height,
        };
        self.last_good_signature = Some(duplicate::signature(&frame));
        self.last_good_frame = Some(frame.clone());
        self.canvas = Some(LinearCanvas::new(frame));
        StitchOutcome::FirstFrame
    }

    fn classify_direction(&self, candidate: &MotionCandidate) -> DirectionResult {
        match self.locked_axis {
            None => match classify_axis(
                candidate.dx,
                candidate.dy,
                self.config.axis_ratio_threshold,
            ) {
                AxisClassification::Vertical { direction }
                | AxisClassification::Horizontal { direction } => {
                    DirectionResult::Direction(direction)
                }
                AxisClassification::Ambiguous => DirectionResult::Ambiguous,
            },
            Some(locked) => match validate_with_lock(
                locked,
                candidate.dx,
                candidate.dy,
                self.config.max_cross_axis_px,
            ) {
                AxisValidation::OnAxis { direction } => DirectionResult::Direction(direction),
                AxisValidation::CrossAxisTooLarge => DirectionResult::CrossAxisTooLarge,
                AxisValidation::AxisChanged { new_axis } => {
                    DirectionResult::AxisChanged { new_axis, locked }
                }
            },
        }
    }
}

enum DirectionResult {
    Direction(AppendDirection),
    Ambiguous,
    CrossAxisTooLarge,
    AxisChanged {
        new_axis: ScrollAxis,
        locked: ScrollAxis,
    },
}

fn build_estimate(
    prev: &RgbaImage,
    curr: &RgbaImage,
    candidate: &MotionCandidate,
    axis_ratio_threshold: f32,
) -> Option<MotionEstimate> {
    let overlap = compute_overlap(
        prev.width(),
        prev.height(),
        curr.width(),
        curr.height(),
        candidate.dx,
        candidate.dy,
    )?;
    // Diagnostic-only direction tag. Reuses `classify_axis` so the rejection-
    // path direction stays consistent with the accept-path classifier. If the
    // candidate is `Ambiguous`, fall back to a sign-based vertical default
    // since `MotionEstimate` cannot carry an "ambiguous" direction.
    let direction = match classify_axis(candidate.dx, candidate.dy, axis_ratio_threshold) {
        AxisClassification::Vertical { direction }
        | AxisClassification::Horizontal { direction } => direction,
        AxisClassification::Ambiguous => {
            if candidate.dy >= 0 {
                AppendDirection::Bottom
            } else {
                AppendDirection::Top
            }
        }
    };
    Some(MotionEstimate {
        dx: candidate.dx,
        dy: candidate.dy,
        axis: direction.axis(),
        direction,
        confidence: candidate.score,
        method: candidate.method,
        overlap,
        inliers: candidate.inliers,
        raw_matches: candidate.raw_matches,
    })
}
```

- [ ] **Step 2: Delete the orphaned `image_ext.rs` (file only — leave staging for Step 5)**

Run:

```bash
rm crates/rollshot-core/src/image_ext.rs
```

> Use a plain `rm` here, not `git rm`. Step 5 stages the deletion alongside the other changes in a single commit so the workspace doesn't sit in a partially-staged state between steps.

Then edit `crates/rollshot-core/src/lib.rs` and remove the `mod image_ext;` line. The new module list should be:

```rust
mod axis;
mod canvas;
mod duplicate;
mod matcher;
mod overlap;
mod stitcher;
mod types;
mod verifier;

pub use canvas::{CanvasAppendError, LinearCanvas};
pub use stitcher::Stitcher;
pub use types::{
    AppendDirection, MatchMethod, MatchStrategy, MotionCandidate, MotionEstimate, NoMatchReason,
    OverlapRegion, ScrollAxis, StitchConfig, StitchOutcome, StitchStats, VerifierConfig,
};
```

- [ ] **Step 3: Update the existing integration tests in `tests/stitcher.rs`**

Replace the entire contents of `crates/rollshot-core/tests/stitcher.rs` with:

```rust
mod common;

use common::{crop_frame, make_scroll_canvas, paint_sticky_header};
use image::{Rgba, RgbaImage};
use rollshot_core::{
    AppendDirection, NoMatchReason, ScrollAxis, StitchConfig, StitchOutcome, Stitcher,
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
        StitchOutcome::NoMatch { reason, best_estimate } => {
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
        StitchOutcome::Appended { direction, added, estimate } => {
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

    let mut stitcher = Stitcher::new(StitchConfig {
        min_append: 64,
        ..StitchConfig::default()
    });
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
            assert_eq!(reason, NoMatchReason::LowConfidence);
        }
        other => panic!("expected NoMatch on white frame, got {other:?}"),
    }

    let stats_after_bad = stitcher.stats();
    assert_eq!(stats_after_bad.frame_count, 1);
    assert_eq!(stats_after_bad.total_height, 320);

    match stitcher.push_frame(recovered) {
        StitchOutcome::Appended { added, direction, .. } => {
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

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended { added, direction, .. } => {
            assert_eq!(direction, AppendDirection::Bottom);
            assert!((66..=74).contains(&added), "added = {added}");
        }
        other => panic!("expected Appended with sticky header, got {other:?}"),
    }
}
```

- [ ] **Step 4: Run the core test suite**

Run: `rtk cargo test -p rollshot-core`
Expected: PASS for every unit test in `types`, `overlap`, `verifier`, `canvas`, `axis`, `matcher`, and the integration tests in `tests/stitcher.rs`. (The CLI crate may still fail to build at this point — fixed in the next task.)

If `cargo test -p rollshot-core` fails because something in `crates/rollshot-cli` doesn't compile (e.g. `cargo` decides to build the workspace dependency graph), restrict it with `--no-default-features` or run only the core's tests via `--tests`:

`rtk cargo test -p rollshot-core --lib --tests`

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/stitcher.rs crates/rollshot-core/src/lib.rs crates/rollshot-core/tests/stitcher.rs
git rm crates/rollshot-core/src/image_ext.rs
git commit -m "feat(core): drive Stitcher through LinearCanvas + verifier + axis lock"
```

---

## Task 8: Update CLI Consumers for the v0.2 `StitchOutcome`

**Files:**
- Modify: `crates/rollshot-cli/src/cmd_capture.rs`
- Modify: `crates/rollshot-cli/src/cmd_stitch_folder.rs`

> The CLI doesn't read any of the new fields; it only counts categories. But `NoProgress` is no longer a unit variant and `AxisChanged` is a new variant, so the existing `match` is no longer exhaustive.

- [ ] **Step 1: Update `cmd_capture.rs` outcome match**

Find this block in `crates/rollshot-cli/src/cmd_capture.rs`:

```rust
match stitcher.push_frame(frame.image) {
    StitchOutcome::FirstFrame => {}
    StitchOutcome::Appended { .. } => appended += 1,
    StitchOutcome::Duplicate => duplicates += 1,
    StitchOutcome::NoMatch { .. } => no_match += 1,
    StitchOutcome::NoProgress => no_progress += 1,
}
```

Replace with:

```rust
match stitcher.push_frame(frame.image) {
    StitchOutcome::FirstFrame => {}
    StitchOutcome::Appended { .. } => appended += 1,
    StitchOutcome::Duplicate => duplicates += 1,
    StitchOutcome::NoMatch { .. } => no_match += 1,
    StitchOutcome::NoProgress { .. } => no_progress += 1,
    StitchOutcome::AxisChanged { .. } => no_match += 1,
}
```

- [ ] **Step 2: Update `cmd_stitch_folder.rs` outcome match**

Find this block in `crates/rollshot-cli/src/cmd_stitch_folder.rs`:

```rust
match stitcher.push_frame(frame) {
    StitchOutcome::FirstFrame => {}
    StitchOutcome::Appended { .. } => appended += 1,
    StitchOutcome::Duplicate => duplicates += 1,
    StitchOutcome::NoMatch { .. } => no_match += 1,
    StitchOutcome::NoProgress => no_progress += 1,
}
```

Replace with:

```rust
match stitcher.push_frame(frame) {
    StitchOutcome::FirstFrame => {}
    StitchOutcome::Appended { .. } => appended += 1,
    StitchOutcome::Duplicate => duplicates += 1,
    StitchOutcome::NoMatch { .. } => no_match += 1,
    StitchOutcome::NoProgress { .. } => no_progress += 1,
    StitchOutcome::AxisChanged { .. } => no_match += 1,
}
```

> Counting `AxisChanged` under `no_match` is intentional for Plan 1: there is no horizontal matcher yet, so this branch can only fire when the verifier wraps a noisy template estimate. Plan 2/3 reconsider how to expose this in the CLI summary.

- [ ] **Step 3: Run the full workspace test suite**

Run: `rtk cargo test --workspace`
Expected: PASS — every test in core + CLI + capture crates.

- [ ] **Step 4: Run lints**

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-cli/src/cmd_capture.rs crates/rollshot-cli/src/cmd_stitch_folder.rs
git commit -m "feat(cli): handle v0.2 StitchOutcome variants (NoProgress, AxisChanged)"
```

---

## Task 9: Plan-1 Completion-Criterion Integration Tests

> Plan-1 success criterion (from the spec): "core tests can directly feed known motion estimates into the canvas/verifier and prove all four append directions work." Unit tests inside `canvas.rs` and `verifier.rs` already cover the modules. This task adds explicit integration-level fixtures that exercise the full pipeline (synthetic 2D canvas → crop frames at known offsets → run through `compute_overlap` + `PixelOverlapVerifier` + `LinearCanvas`) for all four append directions.

**Files:**
- Create: `crates/rollshot-core/tests/canvas.rs`
- Create: `crates/rollshot-core/tests/verifier.rs`
- Modify: `crates/rollshot-core/tests/common/mod.rs`

- [ ] **Step 1: Extend the shared `common/mod.rs` with a wide-and-tall canvas helper**

Append to `crates/rollshot-core/tests/common/mod.rs`:

```rust
/// Builds a wide deterministic canvas suitable for horizontal scroll fixtures.
/// The texture differs from `make_scroll_canvas` so column shifts produce
/// distinct grayscale patterns.
pub fn make_wide_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([240, 240, 240, 255]));

    for x in (0..width).step_by(36) {
        let accent = ((x / 3) % 180) as u8;
        for y in 24..height.saturating_sub(24) {
            let stripe = if (x / 7 + y / 11) % 2 == 0 { 220 } else { 180 };
            img.put_pixel(x, y, Rgba([stripe, accent, 80, 255]));
            if x + 1 < width {
                img.put_pixel(x + 1, y, Rgba([30, 30, 30, 255]));
            }
        }
    }

    for row in [42u32, 96, 154, 211, 268] {
        if row >= height {
            continue;
        }
        for x in 20..width.saturating_sub(20) {
            if (x / 13) % 3 != 0 {
                img.put_pixel(x, row, Rgba([20, 20, 20, 255]));
            }
        }
    }

    img
}

/// Crops a horizontal viewport-sized frame from the canvas.
pub fn crop_frame_xy(canvas: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
    imageops::crop_imm(canvas, x, y, w, h).to_image()
}
```

- [ ] **Step 2: Create `tests/canvas.rs` with four-direction append integration tests**

Create `crates/rollshot-core/tests/canvas.rs`:

```rust
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
```

- [ ] **Step 3: Create `tests/verifier.rs` with full-pipeline verifier integration tests**

> The verifier is module-private. To exercise it via an integration test crate, route through the public `Stitcher` API: a verified `Appended` outcome implies the verifier accepted the candidate. Failure paths are checked via crafted inputs that force `OverlapVerificationFailed` / `InsufficientOverlap`.

Create `crates/rollshot-core/tests/verifier.rs`:

```rust
mod common;

use common::{crop_frame_xy, make_scroll_canvas};
use image::{Rgba, RgbaImage};
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
    // Corrupt the bottom half of the "scrolled" frame so the overlap pixels
    // disagree but the template matcher still finds a vertical candidate.
    let mut second = crop_frame_xy(&canvas, 0, 80, 320, 320);
    for y in 160..320 {
        for x in 0..320 {
            second.put_pixel(x, y, Rgba([255, 0, 255, 255]));
        }
    }

    let stricter = StitchConfig {
        verifier: VerifierConfig {
            downsample_max_mad: 0.02,
            full_res_max_mad: 0.02,
            ..VerifierConfig::default()
        },
        ..StitchConfig::default()
    };

    let mut stitcher = Stitcher::new(stricter);
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);
    match stitcher.push_frame(second) {
        StitchOutcome::NoMatch { reason, .. } => {
            assert!(matches!(
                reason,
                NoMatchReason::OverlapVerificationFailed | NoMatchReason::LowConfidence
            ));
        }
        other => panic!("expected verifier rejection, got {other:?}"),
    }
}

#[test]
fn verifier_rejects_when_overlap_is_too_small() {
    let canvas = make_scroll_canvas(320, 1000);
    let first = crop_frame_xy(&canvas, 0, 0, 320, 320);
    let second = crop_frame_xy(&canvas, 0, 80, 320, 320);

    // Force `min_overlap` so high that no candidate can satisfy it.
    let strict = StitchConfig {
        min_overlap: 4096,
        ..StitchConfig::default()
    };
    let mut stitcher = Stitcher::new(strict);
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);
    match stitcher.push_frame(second) {
        StitchOutcome::NoMatch { reason, .. } => {
            assert!(matches!(
                reason,
                NoMatchReason::InsufficientOverlap | NoMatchReason::LowConfidence
            ));
        }
        other => panic!("expected InsufficientOverlap-like rejection, got {other:?}"),
    }
}
```

- [ ] **Step 4: Run the new integration suites**

Run: `rtk cargo test -p rollshot-core --test canvas --test verifier`
Expected: PASS — five canvas tests + three verifier tests.

- [ ] **Step 5: Run full verification (fmt + clippy + workspace tests)**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk cargo test --workspace
```

Expected: all pass, no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-core/tests/canvas.rs crates/rollshot-core/tests/verifier.rs crates/rollshot-core/tests/common/mod.rs
git commit -m "test(core): cover four-direction canvas append + verifier outcomes"
```

---

## Done When

- `cargo fmt --check` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo test --workspace` passes — including `tests/stitcher.rs`, `tests/canvas.rs`, `tests/verifier.rs`, and every `*::tests` unit suite under `rollshot-core`.
- `rollshot stitch-folder` still produces a vertical long screenshot on its existing v0.1 fixtures.
- `LinearCanvas` integration tests demonstrate all four append directions (`Bottom`, `Top`, `Right`, `Left`).
- The public core surface exports `AppendDirection`, `MatchMethod`, `MatchStrategy`, `MotionCandidate`, `MotionEstimate`, `NoMatchReason`, `OverlapRegion`, `ScrollAxis`, `StitchConfig`, `StitchOutcome`, `StitchStats`, `VerifierConfig`, `LinearCanvas`, `CanvasAppendError`, `Stitcher`.
- `OffsetEstimate`, `MatchAlgorithm`, `append_below`, and `image_ext.rs` no longer exist in the repo.

## Out of Scope (Deferred to Plan 2)

- Horizontal template / coarse / edge matching (Plan 1 ships only the wrapped v0.1 vertical template).
- `CoarseDownscaled2DMatcher`, `AxisAwareTemplateMatcher`, `EdgeOrColumnMatcher`.
- `AutoHybridMatcher` orchestration (the type `MatchStrategy::AutoHybrid` exists as a label but routes through the single template path).
- Multi-candidate ranking, second-best margin rejection in a hybrid setting.

## Out of Scope (Deferred to Plan 3)

- `AkazeConfig`, the `akaze` Cargo feature, AKAZE keypoint extraction, descriptor matching, translation voting.
- Golden fixture directories (`tests/fixtures/linear_vertical_down/...`).
- `MatchMethod::Akaze` ever being produced (the enum variant exists; nothing emits it yet).
- Debug match report (`--debug-match-report`, `--dump-overlap-debug`, `--disable-akaze`).
- CI workflow updates.
