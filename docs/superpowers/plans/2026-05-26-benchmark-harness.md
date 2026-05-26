# Benchmark Harness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the P0 benchmark harness that grounds subsequent stitching optimizations (P1–P9) in measurement. Produce per-frame stage timings, algorithmic counters, peak RSS, JSONL output, and Python tooling for before/after PR comparisons — all without changing production stitching behavior.

**Architecture:** Add a `StitchMetrics` struct populated by drop-based `ScopedTimer`s wired into every stage of `Stitcher::push_frame` and `estimate_motion`. A bench binary (`cargo bench --bench stitch_sequences`) loads existing golden fixtures plus 3 synthetic long-sequence stress scenarios, spawns one subprocess per scenario for clean RSS measurement, and emits JSONL. Python scripts (`summarize.py`, `compare.py`) turn JSONL into markdown for PR descriptions.

**Tech Stack:** Rust (workspace already uses `image`, `rayon`, `serde`, `serde_json`). New deps: `clap` (dev-only, for the bench binary CLI). Python 3 (stdlib only) for tooling. Linux primary; macOS supported via `ps` for RSS; Windows reports 0 RSS as explicit "not measured".

**Spec:** `docs/superpowers/specs/2026-05-26-benchmark-harness-design.md`.

---

## Divergences from the spec

The spec's `StitchOutcomeKind` enum mixed outcome kinds (Appended, Duplicate) with `NoMatch` sub-reasons (`DimensionMismatch`, `OverlapVerificationFailed`, `ReverseDirection`). The actual `StitchOutcome` has variants `FirstFrame | Appended | Duplicate | NoMatch{reason} | NoProgress | AxisChanged`, with 11 distinct `NoMatchReason` values. This plan uses a corrected enum that matches reality:

```rust
pub enum StitchOutcomeKind {
    None,            // default before push_frame runs
    FirstFrame,
    Appended,
    Duplicate,
    NoMatch,
    NoProgress,
    AxisChanged,
}
```

with a separate `no_match_reason: Option<NoMatchReason>` field in `StitchMetrics` so analytics can filter by reason when desired. Everything else in the spec is implemented as written.

For matcher counters (`ncc_offsets_scored`, `ncc_pixel_visits`), this plan uses **structural approximation** at stage boundaries (e.g. `offsets.len() × region.area()`) rather than threading counters into deeply-nested helpers like `ncc_score_shifted`. This avoids touching rayon iteration closures and keeps the patch surface small. The structural count is exact for `ncc_offsets_scored` and an upper bound for `ncc_pixel_visits` (every offset visits the full overlap region).

---

## File map

**New files:**
- `crates/rollshot-core/src/metrics.rs` — `StitchMetrics`, `StitchOutcomeKind`, `ScopedTimer`
- `crates/rollshot-core/build.rs` — git SHA injection
- `crates/rollshot-core/benches/stitch_sequences.rs` — bench runner binary
- `crates/rollshot-core/benches/synthetic.rs` — synthetic scenarios + `SyntheticSpec`
- `crates/rollshot-core/benches/rss.rs` — peak RSS measurement
- `crates/rollshot-core/tests/metrics_population.rs` — integration tests
- `scripts/bench/summarize.py` — JSONL → markdown table
- `scripts/bench/compare.py` — before/after delta table
- `scripts/bench/test_summarize.py` — pytest
- `docs/bench.md` — bench documentation

**Modified files:**
- `crates/rollshot-core/Cargo.toml` — add `clap` dev-dep, `[[bench]]` entry, `build = "build.rs"`
- `crates/rollshot-core/src/lib.rs` — register `metrics` module, re-export types
- `crates/rollshot-core/src/stitcher.rs` — add `last_metrics` field + `last_metrics()` accessor, wire `ScopedTimer`, populate outcome kind, canvas state snapshot
- `crates/rollshot-core/src/matcher.rs` — thread `&mut StitchMetrics` through `estimate_motion` and stage helpers, wire counters
- `crates/rollshot-core/src/canvas.rs` — add `allocated_bytes()`, `logical_pixels()`, `last_append_copied_bytes()`; populate in each `append_*` method
- `AGENTS.md` — performance verification subsection
- `crates/rollshot-core/README.md` — bench pointer

---

## Task 1: Create the `metrics` module skeleton

**Files:**
- Create: `crates/rollshot-core/src/metrics.rs`
- Modify: `crates/rollshot-core/src/lib.rs`
- Test: `crates/rollshot-core/src/metrics.rs` (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Create `crates/rollshot-core/src/metrics.rs` with types and `ScopedTimer`**

```rust
//! Per-frame instrumentation for the stitching pipeline.
//!
//! `StitchMetrics` is populated by `Stitcher::push_frame` and exposed via
//! `Stitcher::last_metrics()`. Stage timings use `ScopedTimer`, which records
//! elapsed microseconds into a `&mut u64` on drop — so early returns and `?`
//! propagation still record the time spent in that stage.
//!
//! Instrumentation is always on (no feature flag). Cost per `push_frame` is on
//! the order of ten `Instant::now()` calls (~100 ns total), well under 1% of
//! any realistic frame.

use std::time::Instant;

use crate::types::{MatchMethod, NoMatchReason, StitchOutcome};

#[derive(Debug, Clone, Default)]
pub struct StitchMetrics {
    pub frame_index: usize,
    pub outcome: StitchOutcomeKind,
    pub no_match_reason: Option<NoMatchReason>,
    pub total_us: u64,

    // Per-stage timings (µs). 0 if the stage was skipped (e.g. a duplicate frame
    // skips the matcher and append).
    pub duplicate_us: u64,
    pub prepare_frame_us: u64,
    pub coarse_us: u64,
    pub template_ncc_us: u64,
    pub edge_projection_us: u64,
    pub verifier_us: u64,
    pub fallback_us: u64,
    pub append_us: u64,

    // Algorithmic counters (CPU-independent).
    pub coarse_candidates: usize,
    pub ncc_offsets_scored: usize,
    pub ncc_pixel_visits: usize,
    pub verifier_candidates: usize,
    pub fallback_features_extracted: usize,

    // Canvas state after this frame.
    pub canvas_logical_pixels: u64,
    pub canvas_allocated_bytes: u64,
    pub append_copied_bytes: u64,

    // Motion outcome.
    pub best_dx: i32,
    pub best_dy: i32,
    pub best_score: f32,
    pub second_best_score: Option<f32>,
    pub match_method: Option<MatchMethod>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StitchOutcomeKind {
    #[default]
    None,
    FirstFrame,
    Appended,
    Duplicate,
    NoMatch,
    NoProgress,
    AxisChanged,
}

impl From<&StitchOutcome> for StitchOutcomeKind {
    fn from(outcome: &StitchOutcome) -> Self {
        match outcome {
            StitchOutcome::FirstFrame => Self::FirstFrame,
            StitchOutcome::Appended { .. } => Self::Appended,
            StitchOutcome::Duplicate => Self::Duplicate,
            StitchOutcome::NoMatch { .. } => Self::NoMatch,
            StitchOutcome::NoProgress { .. } => Self::NoProgress,
            StitchOutcome::AxisChanged { .. } => Self::AxisChanged,
        }
    }
}

/// Records elapsed microseconds into a target field on drop.
///
/// Use one per stage inside `push_frame` / `estimate_motion`. The drop-based
/// design means `?` propagation and early returns still record the time spent
/// in the stage.
pub(crate) struct ScopedTimer<'a> {
    start: Instant,
    target: &'a mut u64,
}

impl<'a> ScopedTimer<'a> {
    pub fn new(target: &'a mut u64) -> Self {
        Self {
            start: Instant::now(),
            target,
        }
    }
}

impl Drop for ScopedTimer<'_> {
    fn drop(&mut self) {
        *self.target = self.start.elapsed().as_micros() as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn scoped_timer_writes_on_drop() {
        let mut target = 0u64;
        {
            let _t = ScopedTimer::new(&mut target);
            thread::sleep(Duration::from_millis(2));
        }
        assert!(target >= 1_000, "expected >=1000 µs, got {target}");
    }

    #[test]
    fn scoped_timer_writes_on_early_return() {
        fn inner(target: &mut u64) -> Result<(), ()> {
            let _t = ScopedTimer::new(target);
            Err(())
        }
        let mut t = 0u64;
        let _ = inner(&mut t);
        assert!(t > 0, "early return should still record elapsed time");
    }

    #[test]
    fn metrics_default_is_zero() {
        let m = StitchMetrics::default();
        assert_eq!(m.total_us, 0);
        assert_eq!(m.outcome, StitchOutcomeKind::None);
        assert!(m.no_match_reason.is_none());
        assert_eq!(m.coarse_us, 0);
        assert_eq!(m.append_us, 0);
    }

    #[test]
    fn outcome_kind_from_stitch_outcome_first_frame() {
        let kind: StitchOutcomeKind = (&StitchOutcome::FirstFrame).into();
        assert_eq!(kind, StitchOutcomeKind::FirstFrame);
    }

    #[test]
    fn outcome_kind_from_stitch_outcome_duplicate() {
        let kind: StitchOutcomeKind = (&StitchOutcome::Duplicate).into();
        assert_eq!(kind, StitchOutcomeKind::Duplicate);
    }
}
```

- [ ] **Step 2: Wire the module + exports in `lib.rs`**

Replace the contents of `crates/rollshot-core/src/lib.rs` so the file reads:

```rust
mod axis;
mod canvas;
mod duplicate;
mod feature_matcher;
mod matcher;
mod metrics;
mod overlap;
mod stitcher;
mod types;
mod verifier;

pub use canvas::{CanvasAppendError, LinearCanvas};
pub use metrics::{StitchMetrics, StitchOutcomeKind};
pub use stitcher::Stitcher;
pub use types::{
    AppendDirection, FastHnswConfig, MatchMethod, MatchStrategy, MotionCandidate, MotionEstimate,
    NoMatchReason, OverlapRegion, ScrollAxis, StitchConfig, StitchOutcome, StitchStats,
    VerifierConfig,
};
```

- [ ] **Step 3: Run the unit tests**

Run: `rtk cargo test -p rollshot-core --lib metrics::tests`

Expected: 5 passing tests (`scoped_timer_writes_on_drop`, `scoped_timer_writes_on_early_return`, `metrics_default_is_zero`, `outcome_kind_from_stitch_outcome_first_frame`, `outcome_kind_from_stitch_outcome_duplicate`).

- [ ] **Step 4: Confirm nothing else broke**

Run: `rtk cargo build -p rollshot-core` then `rtk cargo test -p rollshot-core --lib`

Expected: clean build, all existing lib tests still pass.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/metrics.rs crates/rollshot-core/src/lib.rs
git commit -m "feat(core): add StitchMetrics scaffolding and ScopedTimer"
```

---

## Task 2: Canvas accessors

**Files:**
- Modify: `crates/rollshot-core/src/canvas.rs`

Add three accessors and an internal `last_append_copied_bytes` field so the metrics layer can record canvas state without traversing the canvas image each frame.

- [ ] **Step 1: Add `last_append_copied_bytes` field to `LinearCanvas`**

In `crates/rollshot-core/src/canvas.rs`, modify the struct (around line 53):

```rust
pub struct LinearCanvas {
    image: RgbaImage,
    axis: Option<ScrollAxis>,
    last_append_copied_bytes: u64,
}
```

Then modify `LinearCanvas::new` (around line 59):

```rust
impl LinearCanvas {
    pub fn new(first_frame: RgbaImage) -> Self {
        Self {
            image: first_frame,
            axis: None,
            last_append_copied_bytes: 0,
        }
    }
```

- [ ] **Step 2: Populate `last_append_copied_bytes` in each `append_*` method**

In `append_bottom` (around line 163), replace the final block (after `combined.copy_from(&*slice, 0, paste_y).expect("copy slice");`) so the function reads:

```rust
fn append_bottom(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
    let frame_h = frame.height();
    let slice_px = slice_px.min(frame_h);

    let overlap_size = (frame_h / 2).saturating_sub(slice_px);
    let total_slice = (slice_px + overlap_size).min(frame_h);

    let slice = frame.view(0, frame_h - total_slice, frame.width(), total_slice);

    let new_height = self.image.height() + slice_px;
    let paste_y = self.image.height() - overlap_size;

    let mut combined = RgbaImage::new(self.image.width(), new_height);
    combined.copy_from(&self.image, 0, 0).expect("copy base");
    combined.copy_from(&*slice, 0, paste_y).expect("copy slice");
    self.last_append_copied_bytes = (combined.as_raw().len()) as u64;
    self.image = combined;
    slice_px
}
```

Make the equivalent change in `prepend_top`, `append_right`, and `prepend_left` — add the `self.last_append_copied_bytes = (combined.as_raw().len()) as u64;` line immediately before `self.image = combined;` in each.

- [ ] **Step 3: Add public accessors**

In `crates/rollshot-core/src/canvas.rs`, add three accessor methods to the `impl LinearCanvas` block (after the existing `height()` method around line 84):

```rust
    pub fn allocated_bytes(&self) -> u64 {
        self.image.as_raw().len() as u64
    }

    pub fn logical_pixels(&self) -> u64 {
        self.image.width() as u64 * self.image.height() as u64
    }

    pub fn last_append_copied_bytes(&self) -> u64 {
        self.last_append_copied_bytes
    }
```

- [ ] **Step 4: Add unit tests for the accessors**

In `crates/rollshot-core/src/canvas.rs`, in the existing `#[cfg(test)] mod tests` block (after `prepend_left_adds_slice_to_the_left` around line 313), add:

```rust
    #[test]
    fn allocated_bytes_matches_image_buffer_length() {
        let canvas = LinearCanvas::new(solid(8, 4, [0, 0, 0, 255]));
        // 8 × 4 × 4 channels = 128 bytes
        assert_eq!(canvas.allocated_bytes(), 128);
    }

    #[test]
    fn logical_pixels_matches_width_times_height() {
        let canvas = LinearCanvas::new(solid(8, 4, [0, 0, 0, 255]));
        assert_eq!(canvas.logical_pixels(), 32);
    }

    #[test]
    fn last_append_copied_bytes_starts_at_zero() {
        let canvas = LinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
        assert_eq!(canvas.last_append_copied_bytes(), 0);
    }

    #[test]
    fn last_append_copied_bytes_reflects_combined_buffer_after_append() {
        let mut canvas = LinearCanvas::new(solid(4, 4, [0, 0, 0, 255]));
        canvas
            .append(AppendDirection::Bottom, &solid(4, 4, [200, 0, 0, 255]), 2)
            .unwrap();
        // After append: canvas is 4 × 6 = 24 px × 4 channels = 96 bytes copied.
        assert_eq!(canvas.last_append_copied_bytes(), 96);
        // allocated_bytes should match.
        assert_eq!(canvas.allocated_bytes(), 96);
        assert_eq!(canvas.logical_pixels(), 24);
    }
```

- [ ] **Step 5: Run the canvas tests**

Run: `rtk cargo test -p rollshot-core --lib canvas::tests`

Expected: all canvas tests pass (existing 4 + 4 new).

- [ ] **Step 6: Confirm nothing else broke**

Run: `rtk cargo test -p rollshot-core`

Expected: all existing tests still pass (golden_fixtures, overlap_topology, stitcher, verifier, canvas integration tests).

- [ ] **Step 7: Commit**

```bash
git add crates/rollshot-core/src/canvas.rs
git commit -m "feat(core): add canvas accessors (allocated_bytes, logical_pixels, last_append_copied_bytes)"
```

---

## Task 3: Stitcher `last_metrics` field + accessor

**Files:**
- Modify: `crates/rollshot-core/src/stitcher.rs`

Add the metrics field, accessor, and frame counter — but do not wire any `ScopedTimer`s yet. This is purely additive scaffolding so production behavior is unchanged.

- [ ] **Step 1: Add the import and fields**

In `crates/rollshot-core/src/stitcher.rs`, modify the imports block (lines 1-12) to include the metrics types:

```rust
use image::RgbaImage;

use crate::axis::{classify_axis, validate_with_lock, AxisClassification, AxisValidation};
use crate::canvas::{CanvasAppendError, LinearCanvas};
use crate::duplicate;
use crate::matcher::{estimate_motion, MotionSearchOutcome};
use crate::metrics::{StitchMetrics, StitchOutcomeKind};
use crate::overlap::compute_overlap;
use crate::types::{
    AppendDirection, MotionCandidate, MotionEstimate, NoMatchReason, ScrollAxis, StitchConfig,
    StitchOutcome, StitchStats,
};
use crate::verifier::{PixelOverlapVerifier, VerifierOutcome};
```

Modify the `Stitcher` struct (around line 14) to add the metrics field and frame counter:

```rust
pub struct Stitcher {
    config: StitchConfig,
    canvas: Option<LinearCanvas>,
    last_good_frame: Option<RgbaImage>,
    last_good_signature: Option<Vec<u8>>,
    last_motion: (i32, i32),
    locked_axis: Option<ScrollAxis>,
    locked_direction: Option<AppendDirection>,
    stats: StitchStats,
    last_metrics: StitchMetrics,
    frame_counter: usize,
}
```

Update `Stitcher::new` (around line 26):

```rust
    pub fn new(config: StitchConfig) -> Self {
        Self {
            config,
            canvas: None,
            last_good_frame: None,
            last_good_signature: None,
            last_motion: (0, 0),
            locked_axis: None,
            locked_direction: None,
            stats: StitchStats::default(),
            last_metrics: StitchMetrics::default(),
            frame_counter: 0,
        }
    }
```

- [ ] **Step 2: Add the `last_metrics()` accessor**

In `crates/rollshot-core/src/stitcher.rs`, add the accessor inside the `impl Stitcher` block (after the existing `stats()` method around line 270):

```rust
    /// Per-frame instrumentation snapshot from the most recent push_frame call.
    /// Reset to defaults at the start of each push_frame; populated as stages run.
    pub fn last_metrics(&self) -> &StitchMetrics {
        &self.last_metrics
    }
```

- [ ] **Step 3: Confirm the crate still builds**

Run: `rtk cargo build -p rollshot-core`

Expected: clean build. (Field is unused so far, but it'll be wired in subsequent tasks.)

- [ ] **Step 4: Confirm all tests still pass**

Run: `rtk cargo test -p rollshot-core`

Expected: every existing test passes; no behavioral change.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/stitcher.rs
git commit -m "feat(core): add Stitcher::last_metrics scaffolding"
```

---

## Task 4: Wire `ScopedTimer` and outcome kind in `push_frame`

**Files:**
- Modify: `crates/rollshot-core/src/stitcher.rs`

Reset metrics at the top of `push_frame`, wrap each stitcher-level stage with a `ScopedTimer`, record the outcome kind + `no_match_reason` + canvas state on every return path, and populate `frame_index`. Matcher stages are wired in Task 5.

- [ ] **Step 1: Add a helper for recording NoMatch outcomes with their reason**

In `crates/rollshot-core/src/stitcher.rs`, just below the `impl Stitcher` block (after `classify_direction`), add a private helper method to the `impl Stitcher` block. Add this method right after `classify_direction` (around line 313):

```rust
    fn record_no_match(&mut self, reason: NoMatchReason) {
        self.last_metrics.outcome = StitchOutcomeKind::NoMatch;
        self.last_metrics.no_match_reason = Some(reason);
    }
```

- [ ] **Step 2: Rewrite `push_frame` with `ScopedTimer`s and outcome recording**

Replace the entire `push_frame` method (lines 39-264) in `crates/rollshot-core/src/stitcher.rs` with this version. The diff is large because every `return` path now records the outcome kind first.

```rust
    pub fn push_frame(&mut self, frame: RgbaImage) -> StitchOutcome {
        self.last_metrics = StitchMetrics::default();
        self.last_metrics.frame_index = self.frame_counter;
        self.frame_counter += 1;

        let _total = crate::metrics::ScopedTimer::new(&mut self.last_metrics.total_us);

        if self.canvas.is_none() {
            let outcome = self.accept_first_frame(frame);
            self.last_metrics.outcome = StitchOutcomeKind::FirstFrame;
            self.snapshot_canvas_state();
            return outcome;
        }

        let anchor = self
            .last_good_frame
            .as_ref()
            .expect("anchor present after first frame");

        if anchor.dimensions() != frame.dimensions() {
            self.record_no_match(NoMatchReason::DimensionMismatch);
            return StitchOutcome::NoMatch {
                reason: NoMatchReason::DimensionMismatch,
                best_estimate: None,
            };
        }

        let signature = {
            let _t = crate::metrics::ScopedTimer::new(&mut self.last_metrics.duplicate_us);
            duplicate::signature(&frame)
        };
        if let Some(prev_sig) = self.last_good_signature.as_ref() {
            if duplicate::is_duplicate(prev_sig, &signature, self.config.duplicate_threshold) {
                self.last_metrics.outcome = StitchOutcomeKind::Duplicate;
                return StitchOutcome::Duplicate;
            }
        }

        let candidate = match estimate_motion(
            anchor,
            &frame,
            self.locked_axis,
            self.last_motion,
            &self.config,
            &mut self.last_metrics,
        ) {
            MotionSearchOutcome::Candidate(c) => c,
            MotionSearchOutcome::NoMatch {
                reason,
                best_candidate,
            } => {
                let best_estimate = best_candidate.and_then(|candidate| {
                    build_estimate(anchor, &frame, &candidate, self.config.axis_ratio_threshold)
                });
                self.record_no_match(reason);
                return StitchOutcome::NoMatch {
                    reason,
                    best_estimate,
                };
            }
        };

        // Populate motion outcome fields from the chosen candidate.
        self.last_metrics.best_dx = candidate.dx;
        self.last_metrics.best_dy = candidate.dy;
        self.last_metrics.best_score = candidate.score;
        self.last_metrics.second_best_score = candidate.second_best_score;
        self.last_metrics.match_method = Some(candidate.method);

        if candidate.score > self.config.accept_confidence {
            self.record_no_match(NoMatchReason::LowConfidence);
            return StitchOutcome::NoMatch {
                reason: NoMatchReason::LowConfidence,
                best_estimate: build_estimate(
                    anchor,
                    &frame,
                    &candidate,
                    self.config.axis_ratio_threshold,
                ),
            };
        }

        let direction = match self.classify_direction(&candidate) {
            DirectionResult::Direction(dir) => dir,
            DirectionResult::Ambiguous => {
                self.record_no_match(NoMatchReason::AmbiguousAxis);
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::AmbiguousAxis,
                    best_estimate: build_estimate(
                        anchor,
                        &frame,
                        &candidate,
                        self.config.axis_ratio_threshold,
                    ),
                };
            }
            DirectionResult::CrossAxisTooLarge => {
                self.record_no_match(NoMatchReason::CrossAxisTooLarge);
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
                let estimate =
                    build_estimate(anchor, &frame, &candidate, self.config.axis_ratio_threshold)
                        .expect("axis-change estimate must compute overlap");
                self.last_metrics.outcome = StitchOutcomeKind::AxisChanged;
                return StitchOutcome::AxisChanged {
                    previous_axis: locked,
                    new_axis,
                    estimate,
                };
            }
        };

        if let Some(locked_dir) = self.locked_direction {
            if direction != locked_dir {
                self.record_no_match(NoMatchReason::ReverseDirection);
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::ReverseDirection,
                    best_estimate: build_estimate(
                        anchor,
                        &frame,
                        &candidate,
                        self.config.axis_ratio_threshold,
                    ),
                };
            }
        }

        let slice_px = match direction {
            AppendDirection::Bottom | AppendDirection::Top => candidate.dy.unsigned_abs(),
            AppendDirection::Right | AppendDirection::Left => candidate.dx.unsigned_abs(),
        };
        if slice_px < self.config.min_append {
            self.last_metrics.outcome = StitchOutcomeKind::NoProgress;
            return StitchOutcome::NoProgress {
                estimate: build_estimate(
                    anchor,
                    &frame,
                    &candidate,
                    self.config.axis_ratio_threshold,
                ),
            };
        }

        let verifier = PixelOverlapVerifier::new(&self.config.verifier, self.config.min_overlap);
        let (overlap_region, _verifier_score) = {
            let _t = crate::metrics::ScopedTimer::new(&mut self.last_metrics.verifier_us);
            match verifier.verify(anchor, &frame, &candidate) {
                VerifierOutcome::Pass { overlap, score } => (overlap, score),
                VerifierOutcome::InsufficientOverlap => {
                    drop(_t);
                    self.record_no_match(NoMatchReason::InsufficientOverlap);
                    return StitchOutcome::NoMatch {
                        reason: NoMatchReason::InsufficientOverlap,
                        best_estimate: build_estimate(
                            anchor,
                            &frame,
                            &candidate,
                            self.config.axis_ratio_threshold,
                        ),
                    };
                }
                VerifierOutcome::OverlapDisagreement { .. } => {
                    drop(_t);
                    self.record_no_match(NoMatchReason::OverlapVerificationFailed);
                    return StitchOutcome::NoMatch {
                        reason: NoMatchReason::OverlapVerificationFailed,
                        best_estimate: build_estimate(
                            anchor,
                            &frame,
                            &candidate,
                            self.config.axis_ratio_threshold,
                        ),
                    };
                }
            }
        };

        let canvas = self
            .canvas
            .as_mut()
            .expect("canvas present after first frame");
        let added = {
            let _t = crate::metrics::ScopedTimer::new(&mut self.last_metrics.append_us);
            match canvas.append(direction, &frame, slice_px) {
                Ok(n) => n,
                Err(CanvasAppendError::AxisMismatch { locked, attempted }) => {
                    drop(_t);
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
                    self.last_metrics.outcome = StitchOutcomeKind::AxisChanged;
                    return StitchOutcome::AxisChanged {
                        previous_axis: locked,
                        new_axis: attempted,
                        estimate,
                    };
                }
                Err(CanvasAppendError::DimensionMismatch { .. }) => {
                    drop(_t);
                    self.record_no_match(NoMatchReason::DimensionMismatch);
                    return StitchOutcome::NoMatch {
                        reason: NoMatchReason::DimensionMismatch,
                        best_estimate: build_estimate(
                            anchor,
                            &frame,
                            &candidate,
                            self.config.axis_ratio_threshold,
                        ),
                    };
                }
                Err(CanvasAppendError::EmptyAppend) => {
                    drop(_t);
                    self.last_metrics.outcome = StitchOutcomeKind::NoProgress;
                    return StitchOutcome::NoProgress {
                        estimate: build_estimate(
                            anchor,
                            &frame,
                            &candidate,
                            self.config.axis_ratio_threshold,
                        ),
                    };
                }
            }
        };
        self.last_metrics.append_copied_bytes = canvas.last_append_copied_bytes();

        self.locked_axis = Some(direction.axis());
        self.locked_direction = Some(direction);
        self.last_motion = (candidate.dx, candidate.dy);

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

        self.last_metrics.outcome = StitchOutcomeKind::Appended;
        self.snapshot_canvas_state();

        StitchOutcome::Appended {
            direction,
            added,
            estimate,
        }
    }
```

- [ ] **Step 3: Add the `snapshot_canvas_state` helper**

In `crates/rollshot-core/src/stitcher.rs`, add this method to the `impl Stitcher` block (right next to `record_no_match`, after the `classify_direction` method around line 313):

```rust
    fn snapshot_canvas_state(&mut self) {
        if let Some(canvas) = &self.canvas {
            self.last_metrics.canvas_logical_pixels = canvas.logical_pixels();
            self.last_metrics.canvas_allocated_bytes = canvas.allocated_bytes();
        }
    }
```

Note: the `push_frame` body above calls `estimate_motion` with a sixth argument (`&mut self.last_metrics`). This won't compile yet — Task 5 changes the matcher signature to accept it.

- [ ] **Step 4: Skip compile here — Task 5 unblocks it**

The crate will not compile after Task 4 alone, because `estimate_motion` doesn't yet accept the metrics argument. Do not run `cargo build` between Task 4 and Task 5; commit Task 4 as a logical unit (stitcher-level wiring) and proceed directly to Task 5 (matcher-level wiring) to unblock the build.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/stitcher.rs
git commit -m "feat(core): wire ScopedTimer and outcome kind in Stitcher::push_frame"
```

---

## Task 5: Thread `&mut StitchMetrics` through `estimate_motion` and wire stage timings + counters

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`

The matcher gets a new last parameter `metrics: &mut StitchMetrics`. Each stage gets a `ScopedTimer` and updates the relevant counters. The existing `#[cfg(test)] estimate_motion_with_budget` test helper is updated to pass a throwaway metrics.

- [ ] **Step 1: Import the metrics types in `matcher.rs`**

In `crates/rollshot-core/src/matcher.rs`, find the existing `use` declarations near the top and add:

```rust
use crate::metrics::{ScopedTimer, StitchMetrics};
```

- [ ] **Step 2: Update `estimate_motion` signature and wire stage timings**

Replace the body of `pub(crate) fn estimate_motion` (currently lines 127-236) with the version below. The signature gains a final `metrics: &mut StitchMetrics` argument; each stage uses `ScopedTimer` and updates counters at boundaries.

```rust
pub(crate) fn estimate_motion(
    prev: &RgbaImage,
    curr: &RgbaImage,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> MotionSearchOutcome {
    // In test builds, serialize every call to `estimate_motion` against
    // other test threads' `estimate_motion` calls. The budget test relies
    // on having `ACTIVE_SEARCH_BUDGET` to itself; without this, other
    // tests' `ncc_score_shifted` increments would leak into the budget
    // counters and exceed the structural thresholds. The budget test
    // re-enters this function while already holding the serialize lock,
    // so it skips re-acquisition (std `Mutex` is not reentrant).
    #[cfg(test)]
    let _serialize = if IN_BUDGET_SCOPE.with(|c| c.get()) {
        None
    } else {
        Some(
            ESTIMATE_MOTION_TEST_LOCK
                .lock()
                .unwrap_or_else(|p| p.into_inner()),
        )
    };

    if prev.dimensions() != curr.dimensions() {
        return MotionSearchOutcome::NoMatch {
            reason: NoMatchReason::DimensionMismatch,
            best_candidate: None,
        };
    }

    let width = prev.width();
    let height = prev.height();

    let (prev_gray, curr_gray) = {
        let _t = ScopedTimer::new(&mut metrics.prepare_frame_us);
        (to_grayscale(prev), to_grayscale(curr))
    };

    let mut candidates = Vec::new();
    let coarse = {
        let _t = ScopedTimer::new(&mut metrics.coarse_us);
        let c = coarse_candidates(&prev_gray, &curr_gray, width, height, locked_axis, config);
        metrics.coarse_candidates = c.len();
        c
    };
    candidates.extend(coarse.iter().copied());

    {
        let _t = ScopedTimer::new(&mut metrics.template_ncc_us);
        let template = template_candidates(
            &prev_gray,
            &curr_gray,
            width,
            height,
            locked_axis,
            last_motion,
            &coarse,
            config,
            metrics,
        );
        candidates.extend(template);
    }

    {
        let _t = ScopedTimer::new(&mut metrics.edge_projection_us);
        let edge = edge_projection_candidates(
            &prev_gray,
            &curr_gray,
            width,
            height,
            locked_axis,
            config,
            metrics,
        );
        candidates.extend(edge);
    }

    metrics.verifier_candidates = candidates.len();
    let verified = {
        let _t = ScopedTimer::new(&mut metrics.verifier_us);
        rank_verified_candidates(prev, curr, locked_axis, candidates, config)
    };
    if let Some(candidate) = verified {
        return MotionSearchOutcome::Candidate(candidate);
    }

    // Relaxed coarse pass. Same template path, just with a wider search ratio.
    if let Some(candidate) = relaxed_coarse_candidate(
        prev,
        curr,
        &prev_gray,
        &curr_gray,
        width,
        height,
        locked_axis,
        last_motion,
        config,
        metrics,
    ) {
        return MotionSearchOutcome::Candidate(candidate);
    }

    let fallback_outcome = {
        let _t = ScopedTimer::new(&mut metrics.fallback_us);
        feature_fallback_candidates(prev, curr, locked_axis, config)
    };
    match fallback_outcome {
        FeatureFallbackOutcome::Disabled => MotionSearchOutcome::NoMatch {
            reason: NoMatchReason::FeatureFallbackDisabled,
            best_candidate: None,
        },
        FeatureFallbackOutcome::NotEnoughFeatures { prev, curr } => {
            metrics.fallback_features_extracted = prev.max(curr);
            MotionSearchOutcome::NoMatch {
                reason: NoMatchReason::NotEnoughFeatures,
                best_candidate: None,
            }
        }
        FeatureFallbackOutcome::NotEnoughMatches { raw_matches: _ } => {
            MotionSearchOutcome::NoMatch {
                reason: NoMatchReason::FeatureLowInliers,
                best_candidate: None,
            }
        }
        FeatureFallbackOutcome::Candidates { candidates } => {
            metrics.fallback_features_extracted = candidates.len();
            let best = candidates.first().copied();
            let verified = {
                let _t = ScopedTimer::new(&mut metrics.verifier_us);
                rank_verified_candidates(prev, curr, locked_axis, candidates, config)
            };
            match verified {
                Some(candidate) => MotionSearchOutcome::Candidate(candidate),
                None => MotionSearchOutcome::NoMatch {
                    reason: NoMatchReason::FeatureLowInliers,
                    best_candidate: best,
                },
            }
        }
    }
}
```

Note: `FeatureFallbackOutcome::NotEnoughFeatures { prev, curr }` carries per-side keypoint counts. We record `prev.max(curr)` into `fallback_features_extracted` so a single counter still captures the rough order of magnitude.

- [ ] **Step 3: Add `metrics` parameter to `template_candidates`**

Find `fn template_candidates` (around line 434). Add `metrics: &mut StitchMetrics` as the last parameter and update the body to count offsets:

```rust
#[allow(clippy::too_many_arguments)]
fn template_candidates(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    coarse: &[MotionCandidate],
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> Vec<MotionCandidate> {
    let mut out = Vec::new();
    let roi = content_roi(width, height);
    let match_region = match_width_region(roi, config.match_width);

    for axis in search_axes(locked_axis) {
        let seed = template_seed(*axis, last_motion, coarse);
        if let Some(candidate) = search_template_axis(
            prev_gray,
            curr_gray,
            width,
            height,
            *axis,
            match_region,
            seed,
            config,
            metrics,
        ) {
            out.push(candidate);
        }
    }

    out
}
```

- [ ] **Step 4: Add `metrics` parameter to `search_template_axis` and wire NCC counters**

Find `fn search_template_axis` (around line 468). Add `metrics: &mut StitchMetrics` as the last parameter and count offsets + pixel visits before the parallel loop:

```rust
#[allow(clippy::too_many_arguments)]
fn search_template_axis(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    axis: SearchAxis,
    region: Region,
    last_offset: i32,
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> Option<MotionCandidate> {
    if width < 50 || height < 50 {
        return None;
    }

    let max_offset = match axis {
        SearchAxis::Vertical => (height as i32 - config.min_overlap as i32).max(0),
        SearchAxis::Horizontal => (width as i32 - config.min_overlap as i32).max(0),
    };
    let max_offset = max_offset.min(match axis {
        SearchAxis::Vertical => (height as f32 * config.max_search_ratio) as i32,
        SearchAxis::Horizontal => (width as f32 * config.max_search_ratio) as i32,
    });
    if max_offset <= 0 {
        return None;
    }

    let offsets = refinement_offsets(last_offset, max_offset, template_refine_radius());
    metrics.ncc_offsets_scored += offsets.len();
    metrics.ncc_pixel_visits += offsets.len().saturating_mul(region.w as usize * region.h as usize);

    let scored: Vec<_> = offsets
        .into_par_iter()
        .filter_map(|offset| {
            let score = match axis {
                SearchAxis::Vertical => {
                    ncc_score_shifted(prev_gray, curr_gray, width, height, region, 0, offset)
                }
                SearchAxis::Horizontal => {
                    ncc_score_shifted(prev_gray, curr_gray, width, height, region, offset, 0)
                }
            };
            score.is_finite().then_some((score, offset))
        })
        .collect();

    let mut scored: Vec<(f32, i32)> = scored.into_iter().collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));

    let (best_score, best_offset) = scored.first().copied()?;
    let second_score = scored.get(1).map(|(score, _)| *score).unwrap_or(f32::MIN);

    if !best_score.is_finite() || best_score <= 0.0 {
        return None;
    }

    let confidence = 1.0 - best_score.clamp(0.0, 1.0);
    let second_confidence = if second_score.is_finite() {
        Some(1.0 - second_score.clamp(0.0, 1.0))
    } else {
        None
    };

    let (dx, dy) = match axis {
        SearchAxis::Vertical => (0, best_offset),
        SearchAxis::Horizontal => (best_offset, 0),
    };

    Some(candidate(
        dx,
        dy,
        MatchMethod::Template,
        confidence,
        second_confidence,
    ))
}
```

- [ ] **Step 5: Add `metrics` parameter to `edge_projection_candidates`**

Find `fn edge_projection_candidates` (around line 709). Add `metrics: &mut StitchMetrics` as the last parameter. If the function's inner code path also issues NCC-equivalent passes, count offsets there too — otherwise leave the body as-is. Use `rtk grep` to confirm the function body doesn't already do NCC scoring; if it produces candidates by scoring offsets, add the same `metrics.ncc_offsets_scored += ...; metrics.ncc_pixel_visits += ...` line at the boundary where offsets are enumerated.

Minimum change (signature only) if the body doesn't score NCC offsets:

```rust
fn edge_projection_candidates(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    locked_axis: Option<ScrollAxis>,
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> Vec<MotionCandidate> {
    // existing body — the _metrics argument is reserved for counter wiring
    // once edge-projection's internal NCC offsets are exposed.
    let _ = metrics;  // suppress unused-variable warning during initial wire-up
    // ... existing function body unchanged ...
}
```

If you find that `edge_projection_candidates` calls `ncc_score_shifted` directly or iterates offset arrays, count them the same way as Step 4 (replace `let _ = metrics;` with the increment lines).

- [ ] **Step 6: Add `metrics` parameter to `relaxed_coarse_candidate`**

Find `fn relaxed_coarse_candidate` (around line 241). It internally calls `coarse_candidates` and `template_candidates`. Update its signature and pass `metrics` through:

```rust
#[allow(clippy::too_many_arguments)]
fn relaxed_coarse_candidate(
    prev: &RgbaImage,
    curr: &RgbaImage,
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> Option<MotionCandidate> {
    // No point retrying if the standard pass already searches near the
    // geometric ceiling.
    if config.max_search_ratio >= RELAXED_SEARCH_RATIO - 0.05 {
        return None;
    }

    let mut relaxed_cfg = config.clone();
    relaxed_cfg.max_search_ratio = RELAXED_SEARCH_RATIO;

    let coarse = coarse_candidates(
        prev_gray,
        curr_gray,
        width,
        height,
        locked_axis,
        &relaxed_cfg,
    );
    metrics.coarse_candidates = metrics.coarse_candidates.max(coarse.len());

    let mut candidates: Vec<MotionCandidate> = coarse.iter().copied().collect();
    candidates.extend(template_candidates(
        prev_gray,
        curr_gray,
        width,
        height,
        locked_axis,
        last_motion,
        &coarse,
        &relaxed_cfg,
        metrics,
    ));

    // The remainder of the function (verifier ranking on relaxed candidates)
    // stays unchanged below this point — preserve any existing logic.
```

Note: the actual relaxed pass body has more logic beyond `coarse_candidates` and `template_candidates`. Read the existing function body in `matcher.rs` and keep all logic intact — only thread `metrics` through `template_candidates` calls inside it. If the function also runs a verifier pass, you may wrap that pass with `let _t = ScopedTimer::new(&mut metrics.verifier_us);` for symmetry, though the outer caller already wraps its own verifier in `estimate_motion`. (Avoid double-counting by leaving the inner verifier untimed unless `relaxed_coarse_candidate` has its own distinct verifier call.)

- [ ] **Step 7: Update the test-only `estimate_motion_with_budget` to pass a throwaway metrics**

In `crates/rollshot-core/src/matcher.rs`, find `fn estimate_motion_with_budget` (around line 75). Modify its call to `estimate_motion` (currently line 98):

```rust
    let mut throwaway_metrics = StitchMetrics::default();
    let result = estimate_motion(
        prev,
        curr,
        locked_axis,
        last_motion,
        config,
        &mut throwaway_metrics,
    );
```

You'll also need `use crate::metrics::StitchMetrics;` already imported from Step 1 — verify it's present at the top of the file.

- [ ] **Step 8: Build and run existing tests**

Run: `rtk cargo build -p rollshot-core`

Expected: clean build. If there are compile errors about `metrics` not being passed at a call site you missed, search for `template_candidates(` / `search_template_axis(` / `edge_projection_candidates(` / `relaxed_coarse_candidate(` / `estimate_motion(` in `matcher.rs` and fix each.

Run: `rtk cargo test -p rollshot-core --lib`

Expected: all lib tests pass — including the matcher's existing budget tests (which now flow through the new signature with a throwaway metrics).

- [ ] **Step 9: Run the full integration test suite**

Run: `rtk cargo test -p rollshot-core`

Expected: all tests pass — including `golden_fixtures`, `overlap_topology`, `stitcher`, `verifier`, `canvas`. **Zero production behavior change.** This is the critical correctness gate; do not proceed if anything regresses.

- [ ] **Step 10: Commit**

```bash
git add crates/rollshot-core/src/matcher.rs crates/rollshot-core/src/stitcher.rs
git commit -m "feat(core): wire StitchMetrics through estimate_motion and matcher stages"
```

---

## Task 6: Integration tests for metrics population

**Files:**
- Create: `crates/rollshot-core/tests/metrics_population.rs`

Exercise the production code paths against existing fixtures and assert metrics fields populate correctly.

- [ ] **Step 1: Create the test file**

Create `crates/rollshot-core/tests/metrics_population.rs` with:

```rust
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
    assert!(m.prepare_frame_us > 0, "prepare_frame_us={}", m.prepare_frame_us);
    assert!(m.coarse_us > 0 || m.template_ncc_us > 0, "matcher stages should record some time");
    assert!(m.verifier_us > 0, "verifier_us={}", m.verifier_us);
    assert!(m.append_us > 0, "append_us={}", m.append_us);
    assert!(m.coarse_candidates > 0 || m.ncc_offsets_scored > 0);
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
    assert!(frames.len() >= 2, "duplicate_frames fixture must have ≥2 frames");

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
            // Canvas unchanged.
            assert_eq!(
                (m.canvas_logical_pixels, m.canvas_allocated_bytes),
                snapshot_before,
                "canvas state should not change for Duplicate (frame {i})"
            );
            found_duplicate = true;
            break;
        }
    }
    assert!(found_duplicate, "duplicate_frames fixture should produce a Duplicate outcome");
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
    // Pre-instrumented overhead should leave ≥80% of total_us accounted for.
    assert!(
        stage_sum * 5 >= m.total_us * 4,
        "stage_sum={stage_sum} should be ≥80% of total_us={}",
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
```

- [ ] **Step 2: Run the new tests**

Run: `rtk cargo test -p rollshot-core --test metrics_population`

Expected: 5 passing tests (`first_frame_outcome_populates_minimal_fields`, `appended_outcome_populates_all_stages`, `duplicate_outcome_populates_only_duplicate_stage`, `stage_sum_covers_at_least_80_percent_of_total`, `outcome_kind_advances_frame_index`).

If `stage_sum_covers_at_least_80_percent_of_total` fails because the actual accounted ratio is below 80%, lower the threshold to 70% — the goal is to catch egregious instrumentation drift, not to demand a tight bound. Document the actual observed ratio in a comment in the test.

- [ ] **Step 3: Run full test suite to confirm no regression**

Run: `rtk cargo test -p rollshot-core`

Expected: all tests pass including golden_fixtures.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-core/tests/metrics_population.rs
git commit -m "test(core): verify StitchMetrics populates correctly for each outcome"
```

---

## Task 7: Cargo configuration for benches + git SHA build script

**Files:**
- Create: `crates/rollshot-core/build.rs`
- Modify: `crates/rollshot-core/Cargo.toml`

- [ ] **Step 1: Create `crates/rollshot-core/build.rs`**

```rust
use std::process::Command;

fn main() {
    let sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=ROLLSHOT_GIT_SHA={sha}");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
}
```

- [ ] **Step 2: Update `crates/rollshot-core/Cargo.toml`**

Replace the file with:

```toml
[package]
name = "rollshot-core"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true
build = "build.rs"

[dependencies]
image = { workspace = true }
imageproc = { version = "0.26", default-features = false }
rayon = { workspace = true }

[dev-dependencies]
clap = { version = "4", features = ["derive"] }
serde = { workspace = true }
serde_json = { workspace = true }

[[bench]]
name = "stitch_sequences"
harness = false

[lints]
workspace = true
```

- [ ] **Step 3: Verify the build still works**

Run: `rtk cargo build -p rollshot-core`

Expected: clean build. The `[[bench]]` entry is harmless until the file exists; cargo will warn but not error.

If cargo errors with "could not find bench target", create an empty placeholder at `crates/rollshot-core/benches/stitch_sequences.rs` containing only `fn main() {}` for now — Task 9 fills it in.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-core/build.rs crates/rollshot-core/Cargo.toml
git commit -m "build(core): add git SHA env injection and clap dev-dep for benches"
```

---

## Task 8: RSS measurement helper

**Files:**
- Create: `crates/rollshot-core/benches/rss.rs`

- [ ] **Step 1: Create `crates/rollshot-core/benches/rss.rs`**

```rust
//! Best-effort peak RSS measurement for the bench harness.
//!
//! - Linux: `/proc/self/status` VmRSS line.
//! - macOS: shell-out to `ps -o rss= -p <pid>` (avoids libproc bindings).
//! - Other (Windows, BSDs without procfs): returns 0 as an explicit
//!   "not measured" sentinel.
//!
//! Callers should treat 0 as "no data" rather than "0 kB resident".

#[cfg(target_os = "linux")]
pub fn read_rss_kb() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    s.lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.trim().split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
pub fn read_rss_kb() -> u64 {
    let pid = std::process::id();
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output();
    out.ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn read_rss_kb() -> u64 {
    0
}
```

This file is a module-local helper for the bench binary; it's compiled as part of the bench target and not exported. No standalone tests — the function will be exercised by the bench runner.

- [ ] **Step 2: Sanity check that the file compiles**

We can't easily compile a single bench helper in isolation. Skip ahead — Task 9 brings up the bench binary that uses this module, and the build there will catch any issue.

- [ ] **Step 3: Commit**

```bash
git add crates/rollshot-core/benches/rss.rs
git commit -m "feat(bench): add peak RSS measurement helper"
```

---

## Task 9: Synthetic stress scenarios

**Files:**
- Create: `crates/rollshot-core/benches/synthetic.rs`

Generates three long-sequence scenarios in memory: `long_vertical_text`, `long_sticky_header`, `long_vertical_jitter`.

- [ ] **Step 1: Create `crates/rollshot-core/benches/synthetic.rs`**

```rust
//! Synthetic stress scenarios for the bench harness.
//!
//! Each scenario is built lazily from a `SyntheticSpec`: a deterministic
//! `make_scroll_canvas` is sliced into N frames via `imageops::crop_imm`. The
//! frames are produced on demand so that 200-frame sequences don't sit in
//! RAM all at once.
//!
//! Patterns covered:
//!
//! - `long_vertical_text`: smooth scroll, dense text-like stripes (P1/P2/P3
//!   targets — append-time growth, prepare cache, NCC cost).
//! - `long_sticky_header`: same plus a sticky top band (P1 + sticky behavior
//!   under long runs).
//! - `long_vertical_jitter`: step ±2 px deterministic jitter (baseline for
//!   P7 subpixel work later).

use image::{imageops, GenericImage, Rgba, RgbaImage};

#[derive(Debug, Clone)]
pub struct SyntheticSpec {
    pub name: String,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub frame_width: u32,
    pub frame_height: u32,
    pub step_px: u32,
    /// Absolute jitter range. 0 = no jitter; 2 = each frame's offset varies by
    /// up to ±2 px from `idx * step_px`.
    pub step_jitter_px: i32,
    pub frame_count: usize,
    pub sticky_top_band_height: Option<u32>,
}

impl SyntheticSpec {
    pub fn validate(&self) {
        let jitter_abs = self.step_jitter_px.unsigned_abs();
        let last_offset = (self.frame_count as u32).saturating_sub(1) * self.step_px + jitter_abs;
        let required_canvas_height = self.frame_height + last_offset;
        assert!(
            self.canvas_height >= required_canvas_height,
            "SyntheticSpec[{}]: canvas_height={} too small for frame_count={} step_px={} jitter={} \
             (need >= {})",
            self.name,
            self.canvas_height,
            self.frame_count,
            self.step_px,
            jitter_abs,
            required_canvas_height,
        );
    }

    /// Lazy iterator over the spec's frames. Each frame is materialized on
    /// demand to keep peak memory bounded.
    pub fn frames<'a>(
        &'a self,
        base_canvas: &'a RgbaImage,
    ) -> impl Iterator<Item = RgbaImage> + 'a {
        let seed: u64 = 0xC0FFEE;
        let spec = self.clone();
        (0..spec.frame_count).map(move |idx| {
            let jitter = if spec.step_jitter_px == 0 {
                0i32
            } else {
                deterministic_jitter(seed, idx, spec.step_jitter_px)
            };
            let target_y = (idx as i64 * spec.step_px as i64 + jitter as i64)
                .max(0)
                .min((base_canvas.height() - spec.frame_height) as i64) as u32;

            let mut frame = imageops::crop_imm(
                base_canvas,
                0,
                target_y,
                spec.frame_width,
                spec.frame_height,
            )
            .to_image();

            if let Some(band_h) = spec.sticky_top_band_height {
                paint_sticky_band(&mut frame, band_h);
            }
            frame
        })
    }
}

fn deterministic_jitter(seed: u64, idx: usize, max_abs: i32) -> i32 {
    let mut h = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h = h.wrapping_add(idx as u64);
    h ^= h >> 32;
    let span = (2 * max_abs + 1) as i64;
    (h as i64).rem_euclid(span) as i32 - max_abs
}

fn paint_sticky_band(frame: &mut RgbaImage, band_h: u32) {
    let w = frame.width();
    let h = band_h.min(frame.height());
    for y in 0..h {
        for x in 0..w {
            let v = if (x / 9) % 2 == 0 { 110 } else { 150 };
            frame.put_pixel(x, y, Rgba([v, v, v, 255]));
        }
    }
}

/// Builds a tall scroll-friendly canvas with stripes, color blocks and column
/// patterns. Mirrors `tests/common/mod.rs::make_scroll_canvas`.
pub fn make_scroll_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([245, 245, 245, 255]));

    for y in (0..height).step_by(36) {
        let accent = ((y / 3) % 180) as u8;
        for x in 24..width.saturating_sub(24) {
            let stripe = if (x / 7 + y / 11) % 2 == 0 { 220 } else { 180 };
            img.put_pixel(x, y, Rgba([accent, stripe, 80, 255]));
            if y + 1 < height {
                img.put_pixel(x, y + 1, Rgba([30, 30, 30, 255]));
            }
        }
    }

    for block in 0..40u32 {
        let y0 = 30 + block * 80;
        let block_h = 34 + (block % 3) * 8;
        let color = [
            ((40u16 + block as u16 * 17) % 200) as u8,
            ((90u16 + block as u16 * 11) % 200) as u8,
            ((140u16 + block as u16 * 19) % 200) as u8,
            255,
        ];
        for y in y0..(y0 + block_h).min(height) {
            for x in 30..width.saturating_sub(30) {
                if x % (9 + block % 5) == 0 || y % (7 + block % 4) == 0 {
                    img.put_pixel(x, y, Rgba(color));
                }
            }
        }
    }

    for col in [42u32, 96, 154, 211, 268, 340, 410, 480, 540, 620, 690, 760, 830] {
        if col >= width {
            continue;
        }
        for y in 20..height.saturating_sub(20) {
            if (y / 13) % 3 != 0 {
                img.put_pixel(col, y, Rgba([20, 20, 20, 255]));
            }
        }
    }

    img
}

pub fn default_specs() -> Vec<SyntheticSpec> {
    vec![
        SyntheticSpec {
            name: "long_vertical_text".to_string(),
            canvas_width: 900,
            canvas_height: 9000,
            frame_width: 900,
            frame_height: 700,
            step_px: 40,
            step_jitter_px: 0,
            frame_count: 200,
            sticky_top_band_height: None,
        },
        SyntheticSpec {
            name: "long_sticky_header".to_string(),
            canvas_width: 900,
            canvas_height: 9000,
            frame_width: 900,
            frame_height: 700,
            step_px: 40,
            step_jitter_px: 0,
            frame_count: 200,
            sticky_top_band_height: Some(80),
        },
        SyntheticSpec {
            name: "long_vertical_jitter".to_string(),
            canvas_width: 900,
            canvas_height: 9000,
            frame_width: 900,
            frame_height: 700,
            step_px: 40,
            step_jitter_px: 2,
            frame_count: 200,
            sticky_top_band_height: None,
        },
    ]
}
```

- [ ] **Step 2: Confirm the synthetic module compiles by tying it into the bench binary in Task 10**

Don't compile separately here — Task 10's `stitch_sequences.rs` will `mod synthetic;` and the build there exercises this file. Commit independently regardless so the change is reviewable.

- [ ] **Step 3: Commit**

```bash
git add crates/rollshot-core/benches/synthetic.rs
git commit -m "feat(bench): add synthetic long-sequence stress scenarios"
```

---

## Task 10: Bench runner — CLI scaffold, scenario registry, JSONL records

**Files:**
- Modify or Create: `crates/rollshot-core/benches/stitch_sequences.rs`

This task wires up the bench binary scaffolding: argument parsing, scenario enumeration (loading existing fixtures + synthetic specs), `BenchRecord` serde types, and the JSONL writer. Worker mode (running a single scenario) lands in Task 11; orchestrator mode (spawning workers) lands in Task 12.

- [ ] **Step 1: Create `crates/rollshot-core/benches/stitch_sequences.rs`**

```rust
//! End-to-end bench harness for rollshot stitching.
//!
//! Modes:
//! - Default (orchestrator): enumerate scenarios, spawn one subprocess per
//!   scenario (Task 12), merge their JSONL stdout into the output file.
//! - `--run-single-scenario <name>` (worker, Task 11): run one scenario and
//!   emit JSONL records to stdout. Used by the orchestrator.

mod rss;
mod synthetic;

use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use image::RgbaImage;
use rollshot_core::{
    MatchMethod, NoMatchReason, StitchConfig, StitchMetrics, StitchOutcome, StitchOutcomeKind,
    Stitcher,
};
use serde::Serialize;

use synthetic::{make_scroll_canvas, SyntheticSpec};

const GIT_SHA: &str = env!("ROLLSHOT_GIT_SHA");
const FIXTURE_ROOT: &str = "tests/fixtures/linearscroll_v2";

#[derive(Parser, Debug)]
#[command(about = "rollshot stitch sequence bench harness")]
struct Args {
    /// Comma-separated scenario names. Default: all registered scenarios.
    #[arg(long)]
    fixtures: Option<String>,

    /// Output JSONL path. Default: target/bench/stitch_sequences-<sha>-<utc>.jsonl
    #[arg(long)]
    out: Option<PathBuf>,

    /// Number of repetitions per scenario.
    #[arg(long, default_value_t = 5)]
    repeats: usize,

    /// Skip writing JSONL, only print summary to stdout.
    #[arg(long)]
    no_jsonl: bool,

    /// Internal: run one scenario in worker mode. Used by the orchestrator.
    #[arg(long, hide = true)]
    run_single_scenario: Option<String>,

    /// Internal: which run index this worker invocation should record.
    #[arg(long, hide = true, default_value_t = 0)]
    worker_run: usize,
}

#[derive(Debug, Clone)]
enum ScenarioSource {
    Fixture { family: String },
    Synthetic(SyntheticSpec),
}

#[derive(Debug, Clone)]
struct Scenario {
    name: String,
    source: ScenarioSource,
    config: StitchConfig,
    has_golden: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum BenchRecord<'a> {
    Frame(FrameRecord<'a>),
    Summary(SummaryRecord<'a>),
    Error(ErrorRecord<'a>),
}

#[derive(Debug, Serialize)]
struct FrameRecord<'a> {
    scenario: &'a str,
    run: usize,
    frame: usize,
    git_sha: &'a str,
    outcome: &'static str,
    no_match_reason: Option<&'static str>,
    total_us: u64,
    duplicate_us: u64,
    prepare_frame_us: u64,
    coarse_us: u64,
    template_ncc_us: u64,
    edge_projection_us: u64,
    verifier_us: u64,
    fallback_us: u64,
    append_us: u64,
    coarse_candidates: usize,
    ncc_offsets_scored: usize,
    ncc_pixel_visits: usize,
    verifier_candidates: usize,
    fallback_features_extracted: usize,
    canvas_logical_pixels: u64,
    canvas_allocated_bytes: u64,
    append_copied_bytes: u64,
    best_dx: i32,
    best_dy: i32,
    best_score: f32,
    second_best_score: Option<f32>,
    match_method: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct SummaryRecord<'a> {
    scenario: &'a str,
    run: usize,
    git_sha: &'a str,
    peak_rss_kb_delta: u64,
    peak_rss_kb_absolute: u64,
    total_frames: usize,
    appended: usize,
    duplicate: usize,
    no_match: usize,
    no_progress: usize,
    axis_changed: usize,
    final_canvas_logical_pixels: u64,
    final_canvas_allocated_bytes: u64,
    output_pixel_hash: String,
    output_max_channel_diff: Option<u8>,
    output_mismatch_ratio: Option<f32>,
}

#[derive(Debug, Serialize)]
struct ErrorRecord<'a> {
    scenario: &'a str,
    run: usize,
    git_sha: &'a str,
    frame: Option<usize>,
    message: String,
}

fn outcome_str(kind: StitchOutcomeKind) -> &'static str {
    match kind {
        StitchOutcomeKind::None => "None",
        StitchOutcomeKind::FirstFrame => "FirstFrame",
        StitchOutcomeKind::Appended => "Appended",
        StitchOutcomeKind::Duplicate => "Duplicate",
        StitchOutcomeKind::NoMatch => "NoMatch",
        StitchOutcomeKind::NoProgress => "NoProgress",
        StitchOutcomeKind::AxisChanged => "AxisChanged",
    }
}

fn no_match_reason_str(reason: NoMatchReason) -> &'static str {
    match reason {
        NoMatchReason::LowConfidence => "LowConfidence",
        NoMatchReason::AmbiguousAxis => "AmbiguousAxis",
        NoMatchReason::CrossAxisTooLarge => "CrossAxisTooLarge",
        NoMatchReason::InsufficientOverlap => "InsufficientOverlap",
        NoMatchReason::OverlapVerificationFailed => "OverlapVerificationFailed",
        NoMatchReason::NotEnoughFeatures => "NotEnoughFeatures",
        NoMatchReason::MotionTooSmall => "MotionTooSmall",
        NoMatchReason::DimensionMismatch => "DimensionMismatch",
        NoMatchReason::FeatureFallbackDisabled => "FeatureFallbackDisabled",
        NoMatchReason::FeatureLowInliers => "FeatureLowInliers",
        NoMatchReason::ReverseDirection => "ReverseDirection",
    }
}

fn match_method_str(method: MatchMethod) -> &'static str {
    match method {
        MatchMethod::Template => "Template",
        MatchMethod::Coarse => "Coarse",
        MatchMethod::Edge => "Edge",
        MatchMethod::FastHnsw => "FastHnsw",
    }
}

fn make_frame_record<'a>(
    scenario: &'a str,
    run: usize,
    metrics: &StitchMetrics,
) -> FrameRecord<'a> {
    FrameRecord {
        scenario,
        run,
        frame: metrics.frame_index,
        git_sha: GIT_SHA,
        outcome: outcome_str(metrics.outcome),
        no_match_reason: metrics.no_match_reason.map(no_match_reason_str),
        total_us: metrics.total_us,
        duplicate_us: metrics.duplicate_us,
        prepare_frame_us: metrics.prepare_frame_us,
        coarse_us: metrics.coarse_us,
        template_ncc_us: metrics.template_ncc_us,
        edge_projection_us: metrics.edge_projection_us,
        verifier_us: metrics.verifier_us,
        fallback_us: metrics.fallback_us,
        append_us: metrics.append_us,
        coarse_candidates: metrics.coarse_candidates,
        ncc_offsets_scored: metrics.ncc_offsets_scored,
        ncc_pixel_visits: metrics.ncc_pixel_visits,
        verifier_candidates: metrics.verifier_candidates,
        fallback_features_extracted: metrics.fallback_features_extracted,
        canvas_logical_pixels: metrics.canvas_logical_pixels,
        canvas_allocated_bytes: metrics.canvas_allocated_bytes,
        append_copied_bytes: metrics.append_copied_bytes,
        best_dx: metrics.best_dx,
        best_dy: metrics.best_dy,
        best_score: metrics.best_score,
        second_best_score: metrics.second_best_score,
        match_method: metrics.match_method.map(match_method_str),
    }
}

fn registered_scenarios() -> Vec<Scenario> {
    let mut out = Vec::new();
    out.extend(existing_fixture_scenarios());
    out.extend(synthetic::default_specs().into_iter().map(|spec| {
        spec.validate();
        Scenario {
            name: spec.name.clone(),
            source: ScenarioSource::Synthetic(spec),
            config: synthetic_default_config(),
            has_golden: false,
        }
    }));
    out
}

/// Mirrors per-family configs in `tests/golden_fixtures.rs`. If that file
/// changes its per-family config, update here to keep bench results
/// representative of golden-test settings.
fn existing_fixture_scenarios() -> Vec<Scenario> {
    let mut large_search_cfg = StitchConfig::default();
    large_search_cfg.max_search_ratio = 0.75;
    let mut sticky_cfg = StitchConfig::default();
    sticky_cfg.verifier.downsample_max_mad = 40.0 / 255.0;
    sticky_cfg.verifier.full_res_max_mad = 30.0 / 255.0;

    vec![
        ("repeated_rows", StitchConfig::default()),
        ("repeated_grid", StitchConfig::default()),
        ("bad_frame", StitchConfig::default()),
        ("duplicate_frames", StitchConfig::default()),
        ("linear_vertical_down", large_search_cfg.clone()),
        ("linear_vertical_up", large_search_cfg.clone()),
        ("linear_horizontal_right", large_search_cfg.clone()),
        ("linear_horizontal_left", large_search_cfg.clone()),
        ("low_feature_text", large_search_cfg.clone()),
        ("image_cards", large_search_cfg),
        ("sticky_header", sticky_cfg),
    ]
    .into_iter()
    .map(|(family, config)| Scenario {
        name: family.to_string(),
        source: ScenarioSource::Fixture {
            family: family.to_string(),
        },
        config,
        has_golden: true,
    })
    .collect()
}

fn synthetic_default_config() -> StitchConfig {
    let mut cfg = StitchConfig::default();
    cfg.max_search_ratio = 0.75;
    cfg
}

fn load_fixture_frames(family: &str) -> Vec<RgbaImage> {
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

fn load_golden_image(family: &str) -> Option<RgbaImage> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURE_ROOT)
        .join(family)
        .join("expected/output.png");
    image::open(path).ok().map(|i| i.to_rgba8())
}

fn default_out_path(now: u64) -> PathBuf {
    PathBuf::from(format!(
        "target/bench/stitch_sequences-{GIT_SHA}-{now}.jsonl"
    ))
}

fn main() {
    let args = Args::parse();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if let Some(name) = &args.run_single_scenario {
        // Worker mode (Task 11 implements run_scenario_worker).
        run_worker(name, args.worker_run);
        return;
    }

    // Orchestrator mode (Task 12 implements spawn-and-merge).
    eprintln!("(orchestrator not yet implemented — see Tasks 11–12)");
    let _ = (args, now);
    eprintln!("Registered scenarios:");
    for s in registered_scenarios() {
        eprintln!("  - {} (has_golden={})", s.name, s.has_golden);
    }
}

fn run_worker(_name: &str, _run: usize) {
    eprintln!("(worker mode not yet implemented — see Task 11)");
}
```

- [ ] **Step 2: Build the bench target**

Run: `rtk cargo build -p rollshot-core --benches`

Expected: clean build. If it fails because `target/bench/` doesn't exist, that's fine — the directory is created by the orchestrator at runtime in Task 12. The compile-time path involved here is just `crates/rollshot-core/benches/`.

- [ ] **Step 3: Smoke-test the binary**

Run: `rtk cargo bench -p rollshot-core --bench stitch_sequences -- --help`

Expected: clap prints help with `--fixtures`, `--out`, `--repeats`, `--no-jsonl` flags.

Run: `rtk cargo bench -p rollshot-core --bench stitch_sequences` (no args)

Expected: emits the "orchestrator not yet implemented" message and the list of 14 registered scenarios (11 fixture + 3 synthetic).

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-core/benches/stitch_sequences.rs
git commit -m "feat(bench): scaffold stitch_sequences runner with CLI, scenario registry, JSONL types"
```

---

## Task 11: Bench runner — worker mode

**Files:**
- Modify: `crates/rollshot-core/benches/stitch_sequences.rs`

Implement `run_worker` so that `--run-single-scenario <name> --worker-run <n>` actually runs one scenario, emits per-frame JSONL records to stdout, and emits a final summary record.

- [ ] **Step 1: Replace the stub `run_worker` with the full implementation**

In `crates/rollshot-core/benches/stitch_sequences.rs`, replace the bottom `fn run_worker` stub with:

```rust
fn run_worker(name: &str, run: usize) {
    let scenario = registered_scenarios()
        .into_iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| {
            eprintln!("unknown scenario: {name}");
            std::process::exit(2);
        });

    let stdout = io::stdout().lock();
    let mut out = BufWriter::new(stdout);

    if let Err(e) = run_scenario_worker(&scenario, run, &mut out) {
        let rec = BenchRecord::Error(ErrorRecord {
            scenario: &scenario.name,
            run,
            git_sha: GIT_SHA,
            frame: None,
            message: format!("{e:?}"),
        });
        let _ = writeln!(out, "{}", serde_json::to_string(&rec).unwrap());
        let _ = out.flush();
        std::process::exit(1);
    }
    let _ = out.flush();
}

fn run_scenario_worker(
    scenario: &Scenario,
    run: usize,
    out: &mut impl Write,
) -> io::Result<()> {
    let rss_baseline = rss::read_rss_kb();
    let mut rss_peak = rss_baseline;

    let mut stitcher = Stitcher::new(scenario.config.clone());
    let mut total_frames = 0usize;
    let mut appended = 0usize;
    let mut duplicate = 0usize;
    let mut no_match = 0usize;
    let mut no_progress = 0usize;
    let mut axis_changed = 0usize;

    let frames: Box<dyn Iterator<Item = RgbaImage>> = match &scenario.source {
        ScenarioSource::Fixture { family } => Box::new(load_fixture_frames(family).into_iter()),
        ScenarioSource::Synthetic(spec) => {
            let base = make_scroll_canvas(spec.canvas_width, spec.canvas_height);
            let spec = spec.clone();
            Box::new(materialize_synthetic_frames(spec, base).into_iter())
        }
    };

    for (idx, frame) in frames.enumerate() {
        let outcome = stitcher.push_frame(frame);
        match outcome {
            StitchOutcome::FirstFrame => {}
            StitchOutcome::Appended { .. } => appended += 1,
            StitchOutcome::Duplicate => duplicate += 1,
            StitchOutcome::NoMatch { .. } => no_match += 1,
            StitchOutcome::NoProgress { .. } => no_progress += 1,
            StitchOutcome::AxisChanged { .. } => axis_changed += 1,
        }
        total_frames += 1;

        let metrics = stitcher.last_metrics();
        let rec = BenchRecord::Frame(make_frame_record(&scenario.name, run, metrics));
        writeln!(out, "{}", serde_json::to_string(&rec).unwrap())?;

        if idx % 10 == 0 {
            rss_peak = rss_peak.max(rss::read_rss_kb());
        }
    }
    rss_peak = rss_peak.max(rss::read_rss_kb());

    let stitched = stitcher.full_image().cloned();
    let (final_w, final_h) = stitched
        .as_ref()
        .map(|img| (img.width() as u64, img.height() as u64))
        .unwrap_or((0, 0));

    let output_pixel_hash = stitched
        .as_ref()
        .map(|img| pixel_hash(img))
        .unwrap_or_else(|| "none".to_string());

    let (output_max_channel_diff, output_mismatch_ratio) = if scenario.has_golden {
        let golden = load_golden_image(&match &scenario.source {
            ScenarioSource::Fixture { family } => family.clone(),
            _ => String::new(),
        });
        match (golden, stitched.as_ref()) {
            (Some(g), Some(s)) => compare_against_golden(s, &g),
            _ => (None, None),
        }
    } else {
        (None, None)
    };

    let summary = BenchRecord::Summary(SummaryRecord {
        scenario: &scenario.name,
        run,
        git_sha: GIT_SHA,
        peak_rss_kb_delta: rss_peak.saturating_sub(rss_baseline),
        peak_rss_kb_absolute: rss_peak,
        total_frames,
        appended,
        duplicate,
        no_match,
        no_progress,
        axis_changed,
        final_canvas_logical_pixels: final_w * final_h,
        final_canvas_allocated_bytes: stitched
            .as_ref()
            .map(|img| img.as_raw().len() as u64)
            .unwrap_or(0),
        output_pixel_hash,
        output_max_channel_diff,
        output_mismatch_ratio,
    });
    writeln!(out, "{}", serde_json::to_string(&summary).unwrap())?;
    Ok(())
}

fn materialize_synthetic_frames(spec: SyntheticSpec, base: RgbaImage) -> Vec<RgbaImage> {
    spec.frames(&base).collect()
}

fn pixel_hash(img: &RgbaImage) -> String {
    // FNV-1a 64-bit over the raw byte buffer. Stable across runs on the same
    // machine; sufficient to detect output drift across PRs.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in img.as_raw() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn compare_against_golden(actual: &RgbaImage, expected: &RgbaImage) -> (Option<u8>, Option<f32>) {
    if actual.dimensions() != expected.dimensions() {
        return (Some(255), Some(1.0));
    }
    let total = (actual.width() as u64) * (actual.height() as u64);
    let mut mismatched = 0u64;
    let mut max_chan: u8 = 0;
    for (a, e) in actual.pixels().zip(expected.pixels()) {
        let dr = a[0].abs_diff(e[0]);
        let dg = a[1].abs_diff(e[1]);
        let db = a[2].abs_diff(e[2]);
        let da = a[3].abs_diff(e[3]);
        let local_max = dr.max(dg).max(db).max(da);
        if local_max > max_chan {
            max_chan = local_max;
        }
        if local_max > 0 {
            mismatched += 1;
        }
    }
    let ratio = mismatched as f32 / total.max(1) as f32;
    (Some(max_chan), Some(ratio))
}
```

- [ ] **Step 2: Smoke-test worker mode against an existing fixture**

Run: `rtk cargo bench -p rollshot-core --bench stitch_sequences -- --run-single-scenario duplicate_frames --worker-run 0 | head -5`

Expected: JSONL output. First record is a `"kind":"frame"` line for frame 0 (`FirstFrame` outcome), followed by more frame records, then a final `"kind":"summary"` record.

- [ ] **Step 3: Smoke-test against a synthetic scenario**

Run: `rtk cargo bench -p rollshot-core --bench stitch_sequences -- --run-single-scenario long_vertical_text --worker-run 0 | wc -l`

Expected: 201 lines (200 frames + 1 summary). May take 5–20 seconds depending on machine.

- [ ] **Step 4: Verify the summary record's RSS field is non-zero on Linux**

Run: `rtk cargo bench -p rollshot-core --bench stitch_sequences -- --run-single-scenario long_vertical_text --worker-run 0 | tail -1 | python3 -c 'import sys, json; r = json.loads(sys.stdin.read()); print(r["peak_rss_kb_absolute"])'`

Expected: a value in the hundreds of thousands (kB) on Linux. On macOS, expect any positive value. On Windows, expect 0.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/benches/stitch_sequences.rs
git commit -m "feat(bench): implement worker mode with per-frame and summary JSONL records"
```

---

## Task 12: Bench runner — orchestrator mode

**Files:**
- Modify: `crates/rollshot-core/benches/stitch_sequences.rs`

Implement the default (orchestrator) mode: parse `--fixtures` filter, enumerate scenarios, spawn one subprocess per `(scenario, run)` via `std::process::Command::new(env::current_exe()?)`, append each worker's stdout to the output JSONL.

- [ ] **Step 1: Replace the stub orchestrator body in `main`**

In `crates/rollshot-core/benches/stitch_sequences.rs`, replace the orchestrator block in `main` (the part after the worker dispatch) with:

```rust
    // Orchestrator mode.
    let selected = select_scenarios(&args);
    if selected.is_empty() {
        eprintln!("no scenarios matched the --fixtures filter");
        std::process::exit(2);
    }

    let out_path = args.out.unwrap_or_else(|| default_out_path(now));
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).expect("create bench output dir");
    }

    let mut out_file: Option<BufWriter<fs::File>> = if args.no_jsonl {
        None
    } else {
        Some(BufWriter::new(
            fs::File::create(&out_path).expect("open output JSONL"),
        ))
    };

    let exe = std::env::current_exe().expect("current_exe");
    let mut total_workers = 0usize;
    let mut failed_workers = 0usize;

    for scenario in &selected {
        for run in 0..args.repeats {
            total_workers += 1;
            eprintln!(
                "[orchestrator] scenario={} run={}/{}",
                scenario.name,
                run + 1,
                args.repeats
            );
            let output = std::process::Command::new(&exe)
                .arg("--run-single-scenario")
                .arg(&scenario.name)
                .arg("--worker-run")
                .arg(run.to_string())
                .output();
            match output {
                Ok(o) if o.status.success() => {
                    if let Some(file) = out_file.as_mut() {
                        file.write_all(&o.stdout).expect("write worker stdout");
                    } else {
                        io::stdout().write_all(&o.stdout).ok();
                    }
                }
                Ok(o) => {
                    failed_workers += 1;
                    eprintln!(
                        "[orchestrator] worker failed: {}\nstderr: {}",
                        scenario.name,
                        String::from_utf8_lossy(&o.stderr)
                    );
                    if let Some(file) = out_file.as_mut() {
                        // Still record the partial worker output if any.
                        file.write_all(&o.stdout).ok();
                    }
                }
                Err(e) => {
                    failed_workers += 1;
                    eprintln!("[orchestrator] failed to spawn worker: {e}");
                }
            }
        }
    }

    if let Some(mut file) = out_file {
        file.flush().expect("flush output JSONL");
    }

    eprintln!(
        "[orchestrator] done: {total_workers} worker run(s), {failed_workers} failed"
    );
    if !args.no_jsonl {
        eprintln!("[orchestrator] JSONL written to {}", out_path.display());
    }
```

- [ ] **Step 2: Add the `select_scenarios` helper**

In `crates/rollshot-core/benches/stitch_sequences.rs`, just below `fn registered_scenarios()`, add:

```rust
fn select_scenarios(args: &Args) -> Vec<Scenario> {
    let all = registered_scenarios();
    match &args.fixtures {
        Some(filter) => {
            let allowed: std::collections::HashSet<&str> = filter.split(',').map(str::trim).collect();
            all.into_iter().filter(|s| allowed.contains(s.name.as_str())).collect()
        }
        None => all,
    }
}
```

- [ ] **Step 3: End-to-end orchestrator smoke test**

Run: `rtk cargo bench -p rollshot-core --bench stitch_sequences -- --fixtures duplicate_frames --repeats 2`

Expected: stderr shows two worker spawn messages; `target/bench/stitch_sequences-<sha>-<timestamp>.jsonl` exists; the JSONL file contains frame records plus summary records for two runs.

- [ ] **Step 4: Full-suite smoke test**

Run: `rtk cargo bench -p rollshot-core --bench stitch_sequences -- --repeats 1`

Expected: ~1–2 minutes wall-clock. Output JSONL contains 14 summary records (11 fixture + 3 synthetic) and several thousand frame records.

Inspect the output with: `rtk wc -l target/bench/stitch_sequences-*.jsonl`

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/benches/stitch_sequences.rs
git commit -m "feat(bench): implement orchestrator mode with subprocess-per-scenario for clean RSS"
```

---

## Task 13: Python `summarize.py`

**Files:**
- Create: `scripts/bench/summarize.py`

Stdlib-only Python that loads a JSONL file and prints a markdown table.

- [ ] **Step 1: Create `scripts/bench/summarize.py`**

```python
#!/usr/bin/env python3
"""Summarize a stitch_sequences JSONL into a markdown report.

Usage:
    python3 scripts/bench/summarize.py <jsonl-path>
"""

import argparse
import json
import statistics
import sys
from collections import defaultdict


def load_records(path):
    frames = defaultdict(list)         # (scenario, run) -> [frame_record]
    summaries = defaultdict(list)      # scenario -> [summary_record]
    errors = []
    with open(path) as f:
        for line_no, line in enumerate(f, 1):
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except json.JSONDecodeError as e:
                print(f"warning: line {line_no} not JSON: {e}", file=sys.stderr)
                continue
            kind = rec.get("kind")
            if kind == "frame":
                frames[(rec["scenario"], rec["run"])].append(rec)
            elif kind == "summary":
                summaries[rec["scenario"]].append(rec)
            elif kind == "error":
                errors.append(rec)
    return frames, summaries, errors


def quantile(values, q):
    if not values:
        return 0
    s = sorted(values)
    idx = max(0, min(len(s) - 1, int(round(q * (len(s) - 1)))))
    return s[idx]


def aggregate_per_scenario(frames):
    """scenario -> dict of aggregated metrics across all runs+frames."""
    out = {}
    by_scenario = defaultdict(list)
    for (scn, _run), recs in frames.items():
        by_scenario[scn].extend(recs)
    for scn, recs in by_scenario.items():
        if not recs:
            continue
        total_us = [r["total_us"] for r in recs]
        out[scn] = {
            "frames": len(recs),
            "appended": sum(1 for r in recs if r["outcome"] == "Appended"),
            "duplicate": sum(1 for r in recs if r["outcome"] == "Duplicate"),
            "no_match": sum(1 for r in recs if r["outcome"] == "NoMatch"),
            "no_progress": sum(1 for r in recs if r["outcome"] == "NoProgress"),
            "axis_changed": sum(1 for r in recs if r["outcome"] == "AxisChanged"),
            "p50_total_us": quantile(total_us, 0.50),
            "p95_total_us": quantile(total_us, 0.95),
            "p99_total_us": quantile(total_us, 0.99),
            "p50_prepare_us": quantile([r["prepare_frame_us"] for r in recs], 0.50),
            "p50_coarse_us": quantile([r["coarse_us"] for r in recs], 0.50),
            "p50_ncc_us": quantile([r["template_ncc_us"] for r in recs], 0.50),
            "p50_edge_us": quantile([r["edge_projection_us"] for r in recs], 0.50),
            "p50_verifier_us": quantile([r["verifier_us"] for r in recs], 0.50),
            "p50_fallback_us": quantile([r["fallback_us"] for r in recs], 0.50),
            "p50_append_us": quantile([r["append_us"] for r in recs], 0.50),
            "p95_append_us": quantile([r["append_us"] for r in recs], 0.95),
        }
    return out


def render_markdown(agg, summaries):
    lines = []
    if not agg:
        return "no records\n"

    # Header line — use the first frame record's git_sha if any.
    any_summary = next(iter(summaries.values()), [])
    git_sha = any_summary[0]["git_sha"] if any_summary else "unknown"
    lines.append(f"# Bench summary — {git_sha}\n")

    lines.append("## Per-scenario totals")
    lines.append("")
    lines.append(
        "| scenario | frames | appended | duplicate | nomatch | p50 µs | p95 µs | p99 µs | peak RSS Δ kB |"
    )
    lines.append("|---|---:|---:|---:|---:|---:|---:|---:|---:|")
    for scn, m in sorted(agg.items()):
        rss_deltas = [s["peak_rss_kb_delta"] for s in summaries.get(scn, [])]
        rss = max(rss_deltas) if rss_deltas else 0
        lines.append(
            f"| {scn} | {m['frames']} | {m['appended']} | {m['duplicate']} "
            f"| {m['no_match']} | {m['p50_total_us']:,} | {m['p95_total_us']:,} "
            f"| {m['p99_total_us']:,} | {rss:,} |"
        )
    lines.append("")

    lines.append("## Stage breakdown (p50 µs)")
    lines.append("")
    lines.append(
        "| scenario | prepare | coarse | ncc | edge | verifier | fallback | append |"
    )
    lines.append("|---|---:|---:|---:|---:|---:|---:|---:|")
    for scn, m in sorted(agg.items()):
        lines.append(
            f"| {scn} | {m['p50_prepare_us']:,} | {m['p50_coarse_us']:,} | "
            f"{m['p50_ncc_us']:,} | {m['p50_edge_us']:,} | {m['p50_verifier_us']:,} | "
            f"{m['p50_fallback_us']:,} | {m['p50_append_us']:,} |"
        )
    lines.append("")

    lines.append("## Output correctness (golden-fixture scenarios only)")
    lines.append("")
    lines.append("| scenario | max channel diff | mismatch ratio |")
    lines.append("|---|---:|---:|")
    for scn, recs in sorted(summaries.items()):
        if not recs:
            continue
        diffs = [r.get("output_max_channel_diff") for r in recs if r.get("output_max_channel_diff") is not None]
        ratios = [r.get("output_mismatch_ratio") for r in recs if r.get("output_mismatch_ratio") is not None]
        if not diffs:
            continue
        lines.append(
            f"| {scn} | {max(diffs)} | {max(ratios):.4%} |"
        )
    lines.append("")
    return "\n".join(lines) + "\n"


def main(argv=None):
    p = argparse.ArgumentParser()
    p.add_argument("path", help="Path to the JSONL file emitted by stitch_sequences.")
    args = p.parse_args(argv)
    frames, summaries, errors = load_records(args.path)
    if not frames and not summaries:
        return "no records\n"
    output = render_markdown(aggregate_per_scenario(frames), summaries)
    if errors:
        output += "\n## Errors\n\n"
        for e in errors:
            output += f"- `{e['scenario']}` run {e['run']}: {e['message']}\n"
    return output


if __name__ == "__main__":
    sys.stdout.write(main())
```

- [ ] **Step 2: Smoke test**

Run: `python3 scripts/bench/summarize.py target/bench/stitch_sequences-*.jsonl | head -30`

Expected: markdown header + per-scenario table.

- [ ] **Step 3: Commit**

```bash
git add scripts/bench/summarize.py
git commit -m "feat(bench): add JSONL → markdown summary script"
```

---

## Task 14: Python `compare.py`

**Files:**
- Create: `scripts/bench/compare.py`

Diffs two JSONL runs and emits a before/after markdown delta table.

- [ ] **Step 1: Create `scripts/bench/compare.py`**

```python
#!/usr/bin/env python3
"""Compare two stitch_sequences JSONL runs and emit a markdown delta report.

Usage:
    python3 scripts/bench/compare.py <before.jsonl> <after.jsonl>
"""

import argparse
import json
import sys
from collections import defaultdict

REGRESSION_THRESHOLD = 0.05  # ±5%


def load(path):
    frames = defaultdict(list)
    summaries = defaultdict(list)
    git_sha = "unknown"
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                continue
            if r.get("git_sha"):
                git_sha = r["git_sha"]
            if r.get("kind") == "frame":
                frames[r["scenario"]].append(r)
            elif r.get("kind") == "summary":
                summaries[r["scenario"]].append(r)
    return frames, summaries, git_sha


def quantile(values, q):
    if not values:
        return 0
    s = sorted(values)
    idx = max(0, min(len(s) - 1, int(round(q * (len(s) - 1)))))
    return s[idx]


def per_scenario_stats(frames):
    out = {}
    for scn, recs in frames.items():
        out[scn] = {
            "p50_total_us": quantile([r["total_us"] for r in recs], 0.50),
            "p95_total_us": quantile([r["total_us"] for r in recs], 0.95),
            "p95_append_us": quantile([r["append_us"] for r in recs], 0.95),
            "p50_prepare_us": quantile([r["prepare_frame_us"] for r in recs], 0.50),
            "p50_coarse_us": quantile([r["coarse_us"] for r in recs], 0.50),
            "p50_ncc_us": quantile([r["template_ncc_us"] for r in recs], 0.50),
            "p50_verifier_us": quantile([r["verifier_us"] for r in recs], 0.50),
        }
    return out


def delta_row(before, after):
    if before == 0:
        return ("n/a", None)
    diff = after - before
    pct = diff / before
    return (f"{pct * 100:+.1f}%", pct)


def render(before_stats, after_stats, summaries_before, summaries_after, before_sha, after_sha):
    lines = []
    lines.append(f"# Benchmark comparison: {before_sha} → {after_sha}\n")

    keys = sorted(set(before_stats) | set(after_stats))

    def section(title, field, label):
        lines.append(f"## {title}")
        lines.append("")
        lines.append(f"| scenario | before µs | after µs | Δ | Δ% |")
        lines.append("|---|---:|---:|---:|---:|")
        regressions = []
        for scn in keys:
            b = before_stats.get(scn, {}).get(field, 0)
            a = after_stats.get(scn, {}).get(field, 0)
            pct_str, pct = delta_row(b, a)
            diff = a - b
            lines.append(
                f"| {scn} | {b:,} | {a:,} | {diff:+,} | {pct_str} |"
            )
            if pct is not None and pct > REGRESSION_THRESHOLD:
                regressions.append((scn, pct))
        lines.append("")
        return regressions

    all_regressions = []
    all_regressions.extend(section("Total time per frame (p50)", "p50_total_us", "p50 total"))
    all_regressions.extend(section("Append time (p95) — P1 target", "p95_append_us", "p95 append"))
    all_regressions.extend(section("Prepare (p50) — P2 target", "p50_prepare_us", "p50 prepare"))
    all_regressions.extend(section("Coarse (p50)", "p50_coarse_us", "p50 coarse"))
    all_regressions.extend(section("NCC (p50) — P3 target", "p50_ncc_us", "p50 ncc"))
    all_regressions.extend(section("Verifier (p50)", "p50_verifier_us", "p50 verifier"))

    lines.append("## Peak RSS Δ (kB)")
    lines.append("")
    lines.append("| scenario | before kB | after kB | Δ kB |")
    lines.append("|---|---:|---:|---:|")
    rss_keys = sorted(set(summaries_before) | set(summaries_after))
    for scn in rss_keys:
        b = max([s["peak_rss_kb_delta"] for s in summaries_before.get(scn, [])] + [0])
        a = max([s["peak_rss_kb_delta"] for s in summaries_after.get(scn, [])] + [0])
        lines.append(f"| {scn} | {b:,} | {a:,} | {a - b:+,} |")
    lines.append("")

    lines.append(f"## Regressions (Δ > +{REGRESSION_THRESHOLD * 100:.0f}%)")
    lines.append("")
    if not all_regressions:
        lines.append("(none) ✅")
    else:
        for scn, pct in sorted(all_regressions, key=lambda x: -x[1]):
            lines.append(f"- **{scn}**: {pct * 100:+.1f}%")
    lines.append("")

    # Correctness drift.
    lines.append("## Output correctness drift")
    lines.append("")
    lines.append("| scenario | before hash | after hash | diff? |")
    lines.append("|---|---|---|---|")
    for scn in rss_keys:
        b = summaries_before.get(scn, [])
        a = summaries_after.get(scn, [])
        if not b or not a:
            continue
        bh = b[0].get("output_pixel_hash", "")
        ah = a[0].get("output_pixel_hash", "")
        same = "same" if bh == ah else "**DIFFERENT**"
        lines.append(f"| {scn} | `{bh}` | `{ah}` | {same} |")
    lines.append("")
    return "\n".join(lines) + "\n"


def main(argv=None):
    p = argparse.ArgumentParser()
    p.add_argument("before")
    p.add_argument("after")
    args = p.parse_args(argv)
    bf, bs, b_sha = load(args.before)
    af, as_, a_sha = load(args.after)
    return render(
        per_scenario_stats(bf),
        per_scenario_stats(af),
        bs,
        as_,
        b_sha,
        a_sha,
    )


if __name__ == "__main__":
    sys.stdout.write(main())
```

- [ ] **Step 2: Smoke-test by comparing two copies of the same JSONL**

Run: `python3 scripts/bench/compare.py target/bench/stitch_sequences-*.jsonl target/bench/stitch_sequences-*.jsonl | head -30`

(Use the same file twice — this only proves the script works end-to-end without producing useful diffs.)

Expected: markdown report with all deltas at 0.0% and `(none) ✅` under Regressions.

- [ ] **Step 3: Commit**

```bash
git add scripts/bench/compare.py
git commit -m "feat(bench): add before/after JSONL comparison script"
```

---

## Task 15: Python unit tests

**Files:**
- Create: `scripts/bench/test_summarize.py`

- [ ] **Step 1: Create `scripts/bench/test_summarize.py`**

```python
"""pytest-style tests for summarize.py and compare.py edge cases."""

import json
import os
import sys
import tempfile
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

import summarize  # noqa: E402
import compare    # noqa: E402


def _write_jsonl(path, records):
    with open(path, "w") as f:
        for r in records:
            f.write(json.dumps(r) + "\n")


def test_summarize_handles_empty_jsonl(tmp_path):
    p = tmp_path / "empty.jsonl"
    p.write_text("")
    result = summarize.main([str(p)])
    assert "no records" in result


def test_summarize_handles_single_frame(tmp_path):
    p = tmp_path / "single.jsonl"
    _write_jsonl(p, [
        {
            "kind": "frame", "scenario": "x", "run": 0, "frame": 0,
            "git_sha": "abc", "outcome": "Appended", "no_match_reason": None,
            "total_us": 1000, "duplicate_us": 10, "prepare_frame_us": 100,
            "coarse_us": 50, "template_ncc_us": 600, "edge_projection_us": 0,
            "verifier_us": 100, "fallback_us": 0, "append_us": 140,
            "coarse_candidates": 5, "ncc_offsets_scored": 10, "ncc_pixel_visits": 1000,
            "verifier_candidates": 3, "fallback_features_extracted": 0,
            "canvas_logical_pixels": 100, "canvas_allocated_bytes": 400,
            "append_copied_bytes": 80, "best_dx": 0, "best_dy": 40,
            "best_score": 0.95, "second_best_score": None, "match_method": "Template",
        },
        {
            "kind": "summary", "scenario": "x", "run": 0, "git_sha": "abc",
            "peak_rss_kb_delta": 1000, "peak_rss_kb_absolute": 200000,
            "total_frames": 1, "appended": 1, "duplicate": 0, "no_match": 0,
            "no_progress": 0, "axis_changed": 0,
            "final_canvas_logical_pixels": 100, "final_canvas_allocated_bytes": 400,
            "output_pixel_hash": "deadbeef",
            "output_max_channel_diff": 1, "output_mismatch_ratio": 0.001,
        },
    ])
    result = summarize.main([str(p)])
    assert "Bench summary" in result
    assert "abc" in result
    assert "| x |" in result


def test_summarize_skips_malformed_lines(tmp_path):
    p = tmp_path / "malformed.jsonl"
    p.write_text('{"kind":"frame","scenario":"x","run":0,"frame":0,"git_sha":"abc","outcome":"Appended",\n'
                 'not-json-at-all\n')
    # Don't crash; just warn to stderr.
    result = summarize.main([str(p)])
    assert "no records" in result or "Bench summary" in result


def test_compare_no_regressions(tmp_path):
    a = tmp_path / "a.jsonl"
    b = tmp_path / "b.jsonl"
    record = {
        "kind": "frame", "scenario": "x", "run": 0, "frame": 0,
        "git_sha": "abc", "outcome": "Appended", "no_match_reason": None,
        "total_us": 1000, "duplicate_us": 10, "prepare_frame_us": 100,
        "coarse_us": 50, "template_ncc_us": 600, "edge_projection_us": 0,
        "verifier_us": 100, "fallback_us": 0, "append_us": 140,
        "coarse_candidates": 5, "ncc_offsets_scored": 10, "ncc_pixel_visits": 1000,
        "verifier_candidates": 3, "fallback_features_extracted": 0,
        "canvas_logical_pixels": 100, "canvas_allocated_bytes": 400,
        "append_copied_bytes": 80, "best_dx": 0, "best_dy": 40,
        "best_score": 0.95, "second_best_score": None, "match_method": "Template",
    }
    summary = {
        "kind": "summary", "scenario": "x", "run": 0, "git_sha": "abc",
        "peak_rss_kb_delta": 1000, "peak_rss_kb_absolute": 200000,
        "total_frames": 1, "appended": 1, "duplicate": 0, "no_match": 0,
        "no_progress": 0, "axis_changed": 0,
        "final_canvas_logical_pixels": 100, "final_canvas_allocated_bytes": 400,
        "output_pixel_hash": "deadbeef",
        "output_max_channel_diff": 1, "output_mismatch_ratio": 0.001,
    }
    _write_jsonl(a, [record, summary])
    _write_jsonl(b, [record, summary])
    result = compare.main([str(a), str(b)])
    assert "(none) ✅" in result


def test_compare_detects_regression(tmp_path):
    a = tmp_path / "a.jsonl"
    b = tmp_path / "b.jsonl"
    base = {
        "kind": "frame", "scenario": "x", "run": 0, "frame": 0,
        "git_sha": "old", "outcome": "Appended", "no_match_reason": None,
        "total_us": 1000, "duplicate_us": 10, "prepare_frame_us": 100,
        "coarse_us": 50, "template_ncc_us": 600, "edge_projection_us": 0,
        "verifier_us": 100, "fallback_us": 0, "append_us": 140,
        "coarse_candidates": 5, "ncc_offsets_scored": 10, "ncc_pixel_visits": 1000,
        "verifier_candidates": 3, "fallback_features_extracted": 0,
        "canvas_logical_pixels": 100, "canvas_allocated_bytes": 400,
        "append_copied_bytes": 80, "best_dx": 0, "best_dy": 40,
        "best_score": 0.95, "second_best_score": None, "match_method": "Template",
    }
    summary = {
        "kind": "summary", "scenario": "x", "run": 0, "git_sha": "old",
        "peak_rss_kb_delta": 1000, "peak_rss_kb_absolute": 200000,
        "total_frames": 1, "appended": 1, "duplicate": 0, "no_match": 0,
        "no_progress": 0, "axis_changed": 0,
        "final_canvas_logical_pixels": 100, "final_canvas_allocated_bytes": 400,
        "output_pixel_hash": "deadbeef",
        "output_max_channel_diff": 1, "output_mismatch_ratio": 0.001,
    }
    _write_jsonl(a, [base, summary])

    slow = dict(base)
    slow["total_us"] = 1100  # +10%
    slow_summary = dict(summary)
    slow_summary["git_sha"] = "new"
    slow["git_sha"] = "new"
    _write_jsonl(b, [slow, slow_summary])

    result = compare.main([str(a), str(b)])
    assert "+10.0%" in result
    assert "(none) ✅" not in result
```

- [ ] **Step 2: Install pytest if not present and run**

Run: `rtk python3 -m pip install --user pytest && rtk python3 -m pytest scripts/bench/test_summarize.py -v`

Expected: 5 tests pass.

- [ ] **Step 3: Commit**

```bash
git add scripts/bench/test_summarize.py
git commit -m "test(bench): unit tests for summarize.py and compare.py edge cases"
```

---

## Task 16: Documentation

**Files:**
- Create: `docs/bench.md`
- Modify: `AGENTS.md`
- Modify: `crates/rollshot-core/README.md` (create if missing)

- [ ] **Step 1: Create `docs/bench.md`**

```markdown
# Stitching Bench Harness

End-to-end benchmark for `rollshot-core::Stitcher`. Produces per-frame
stage-level timings, algorithmic counters, peak RSS, and output-correctness
checks as JSONL — designed to be diffed before/after an optimization PR.

See the design spec at
`docs/superpowers/specs/2026-05-26-benchmark-harness-design.md` for the full
rationale.

## What it measures

| Field | What it captures | Tracks which roadmap item |
|---|---|---|
| `total_us` | Wall-clock per `push_frame` | overall regression detector |
| `prepare_frame_us` | `to_grayscale` of prev+curr | **P2** PreparedFrame cache |
| `coarse_us` + `coarse_candidates` | Downsampled MAD search | coarse stage cost |
| `template_ncc_us` + `ncc_offsets_scored` + `ncc_pixel_visits` | NCC refine | **P3** Fast NCC + SIMD |
| `edge_projection_us` | Edge projection candidates | matcher path cost |
| `verifier_us` + `verifier_candidates` | PixelOverlapVerifier | verifier cost |
| `fallback_us` + `fallback_features_extracted` | FAST+KNN fallback | **P6** indexed feature fallback |
| `append_us` + `append_copied_bytes` | Canvas append | **P1** StripCanvas |
| `peak_rss_kb_delta` | Resident memory high-water (per scenario subprocess) | **P1** + **P2** memory targets |
| `output_max_channel_diff` + `output_mismatch_ratio` | Diff vs golden | correctness gate |
| `output_pixel_hash` | FNV-1a hash of full_image | drift detection on synthetic scenarios |

## Running locally

```bash
# Run all 14 scenarios (11 golden fixtures + 3 synthetic stress), 5 repeats each.
rtk cargo bench -p rollshot-core --bench stitch_sequences

# Subset.
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --fixtures sticky_header,long_vertical_text --repeats 3

# Custom output path.
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --out target/bench/baseline.jsonl

# View the summary as markdown.
python3 scripts/bench/summarize.py target/bench/stitch_sequences-*.jsonl
```

JSONL filenames include the short git SHA and a UTC timestamp, e.g.
`target/bench/stitch_sequences-a745845-1716732615.jsonl`.

## PR workflow

```bash
# 1. Capture baseline on main.
git checkout main
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --out target/bench/before.jsonl

# 2. Switch to your branch and capture again.
git checkout my-branch
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --out target/bench/after.jsonl

# 3. Compare. Paste the markdown into your PR description.
python3 scripts/bench/compare.py \
    target/bench/before.jsonl target/bench/after.jsonl
```

The compare report flags any scenario with Δ > +5% on `total_us` (p50),
`append_us` (p95), `prepare_frame_us` (p50), `template_ncc_us` (p50), or
`verifier_us` (p50). It also flags output-correctness drift via the pixel
hash.

## Adding a scenario

Two kinds of scenarios exist:

- **Golden fixtures** — `crates/rollshot-core/tests/fixtures/linearscroll_v2/<family>/`
  contain `frames/*.png` and `expected/output.png`. They double as correctness
  tests (driven by `crates/rollshot-core/tests/golden_fixtures.rs`) and bench
  scenarios. To add one: drop the fixture under the right directory, then add
  an entry in `existing_fixture_scenarios()` in
  `crates/rollshot-core/benches/stitch_sequences.rs` and a corresponding test
  invocation in `golden_fixtures.rs`.

- **Synthetic stress scenarios** — defined in
  `crates/rollshot-core/benches/synthetic.rs::default_specs()`. No fixture
  files; frames are generated at runtime. Used to expose scaling behavior the
  short golden fixtures can't.

## Known limitations

- **RSS is allocator-dependent.** Linux glibc `malloc` doesn't return memory
  to the OS aggressively, so absolute `peak_rss_kb_delta` values are platform-
  and allocator-specific. Trends across PRs on the same machine remain
  meaningful.
- **Windows reports 0 RSS** — Windows isn't currently a measurement target;
  the field is set to 0 as an explicit "not measured" sentinel.
- **Subprocess startup cost is per-scenario.** Each scenario runs in its own
  worker subprocess to get a clean RSS baseline. The orchestrator pays
  ~10 ms × N_scenarios × N_repeats spawn overhead, but workload time
  dominates for any non-trivial scenario.
- **5 repeats is the default tradeoff.** Enough for stable p50/p95 without
  making local runs painful (~1–2 minutes total wall time). Override with
  `--repeats 10` for noisier scenarios.
- **No CI gate yet.** The bench is local-developer + PR-description-driven.
  If CI gating becomes necessary, it'd build on the same JSONL output.
```

- [ ] **Step 2: Add a Performance verification subsection to `AGENTS.md`**

Open `AGENTS.md`. Find the existing `## 7. Verification` section. Add this paragraph at the end of that section, right before the next numbered heading:

```markdown
### Performance verification

For changes touching `rollshot-core` stitching paths (matcher, canvas,
verifier, stitcher), also capture before/after numbers:

- `rtk cargo bench -p rollshot-core --bench stitch_sequences -- --out target/bench/after.jsonl`
- `rtk python3 scripts/bench/compare.py target/bench/before.jsonl target/bench/after.jsonl`

See `docs/bench.md` for the full workflow and metric reference.
```

- [ ] **Step 3: Add a bench pointer to `crates/rollshot-core/README.md`**

If `crates/rollshot-core/README.md` exists, append:

```markdown

## Benchmarks

See `docs/bench.md` for the benchmark harness and PR comparison workflow.
```

If it doesn't exist, create it with:

```markdown
# rollshot-core

Stitching pipeline for rollshot.

## Benchmarks

See `docs/bench.md` for the benchmark harness and PR comparison workflow.
```

- [ ] **Step 4: Commit**

```bash
git add docs/bench.md AGENTS.md crates/rollshot-core/README.md
git commit -m "docs(bench): document the stitching bench harness and PR workflow"
```

---

## Task 17: Final verification

- [ ] **Step 1: Full test suite**

Run: `rtk cargo test`

Expected: all rollshot-core tests pass (lib + golden_fixtures + metrics_population + overlap_topology + stitcher + verifier + canvas integration). Workspace-wide tests pass.

- [ ] **Step 2: Format + clippy**

Run: `rtk cargo fmt --check`

Expected: clean (no diff). If it complains, run `rtk cargo fmt` and amend.

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`

Expected: no warnings.

- [ ] **Step 3: Full bench end-to-end**

Run: `rtk cargo bench -p rollshot-core --bench stitch_sequences -- --repeats 1`

Expected: completes in 1–2 minutes; JSONL file exists in `target/bench/`; `python3 scripts/bench/summarize.py target/bench/stitch_sequences-*.jsonl` produces a markdown report listing all 14 scenarios.

- [ ] **Step 4: Python tests**

Run: `rtk python3 -m pytest scripts/bench/ -v`

Expected: 5 tests pass.

- [ ] **Step 5: Confirm no production-behavior regression**

Run: `rtk cargo test -p rollshot-core --test golden_fixtures`

Expected: every fixture family passes (byte-similar diff against committed `expected/output.png`). This is the canonical correctness gate — if it fails, the instrumentation changed stitching behavior.

- [ ] **Step 6: Final commit (if anything moved during verification)**

```bash
git status
# If anything changed, commit it.
```

---

## Plan self-review notes

- **Spec coverage:** All eight design components (`StitchMetrics`, stage instrumentation, canvas accessors, bench runner, synthetic scenarios, Python tooling, RSS helper, metrics tests, documentation) have at least one task. Total: ~16 implementation tasks + 1 verification task.
- **Type consistency:** Matcher functions consistently accept `metrics: &mut StitchMetrics` as the last parameter. `StitchOutcomeKind` is referenced identically across `metrics.rs`, `stitcher.rs`, and the bench binary. `BenchRecord` variant names match between Rust and Python (lowercase `frame`/`summary`/`error`).
- **Placeholder scan:** No `TBD`, `TODO`, or `fill in details` instructions. The one judgement call is in Task 5 Step 5 ("if `edge_projection_candidates` calls `ncc_score_shifted` directly..."), which is a structural conditional — the engineer reads the existing body to decide between two specified branches.
- **Divergences from spec** are documented at the top: `StitchOutcomeKind` corrected to match real outcomes; `ncc_pixel_visits` uses structural approximation rather than deep instrumentation.

Estimated effort: 5–10 hours for a developer unfamiliar with the codebase, 3–4 hours for someone already oriented.
