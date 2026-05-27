# P2 PreparedFrame Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop recomputing the `last_good` (prev) frame's grayscale, coarse samples, and edge projections on every frame by caching them in a `PreparedFrame` that is carried forward on a successful append — a pure caching refactor with byte-identical output.

**Architecture:** Introduce `PreparedFrame` (RGBA + signature + eager grayscale + lazy `OnceLock` coarse/projections) in `matcher.rs`. Rewrite `estimate_motion` and its candidate-gathering helpers to read from two `PreparedFrame`s instead of recomputing from `&RgbaImage`. `Stitcher` holds `last_good: Option<PreparedFrame>`, builds the curr `PreparedFrame` only after the duplicate gate, and moves it into `last_good` only on `Appended`.

**Tech Stack:** Rust, `rollshot-core` crate, `image::RgbaImage`, `rayon`, `std::sync::OnceLock`. Bench harness: `cargo bench -p rollshot-core --bench stitch_sequences` + `scripts/bench/compare.py`.

**Spec:** `docs/superpowers/specs/2026-05-27-p2-prepared-frame-cache-design.md` (live for this workflow).

---

## File Structure

- Modify: `crates/rollshot-core/src/matcher.rs` — add `PreparedFrame` (struct + impl + builders co-located with the private `SearchAxis`/builder fns); rewrite `estimate_motion`, `coarse_candidates`, `edge_projection_candidates`, `edge_projection_axis`, `relaxed_coarse_candidate`, and the test-only `estimate_motion_with_budget`; add equivalence unit tests.
- Modify: `crates/rollshot-core/src/stitcher.rs` — replace `last_good_frame`/`last_good_signature` with `last_good: Option<PreparedFrame>`; build curr after the dup gate; carry it forward on append.
- Modify: `crates/rollshot-core/tests/stitcher.rs` — add two anchor-advancement behavioral tests.
- No changes to `lib.rs` (`PreparedFrame` stays `pub(crate)`), the bench harness, CLI, or app.

---

## Task 0: Capture benchmark baseline (do this BEFORE any code change)

The current branch `p2-prepared-frame-cache` has only the spec commit on top of `main`, so its code is identical to `main`. Capture the baseline now; after Task 1 the code diverges and a clean baseline is impossible.

- [ ] **Step 1: Capture baseline JSONL**

Run:
```bash
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter --repeats 3 \
    --out bench-results/runs/p2-prepared-frame/before.jsonl
```
Expected: command completes; `bench-results/runs/p2-prepared-frame/before.jsonl` exists with rows for the three fixtures. (This path is gitignored — do not commit it.)

- [ ] **Step 2: Record the baseline commit**

Run: `git rev-parse --short HEAD`
Expected: a short SHA (the spec commit, e.g. `6c47dcc`). Note it — it becomes the `before.short_commit` in the Task 4 compare report.

No commit in this task (the JSONL is a gitignored artifact).

---

## Task 1: Add `PreparedFrame` with cached gray/coarse/projection

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs` (imports near line 1; new struct/impl before `fn to_grayscale` at line 894; test `use` block at line 1033; new tests in the `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing equivalence tests**

In `crates/rollshot-core/src/matcher.rs`, extend the test module's `use super::{...}` block (currently at lines 1033-1037) to add the items the new tests need. Replace:

```rust
    use super::{
        coarse_axis_offsets, coarse_sample_dimensions, content_roi, estimate_motion,
        estimate_motion_with_budget, refinement_offsets, template_refine_radius,
        MotionSearchOutcome, SearchBudget, COARSE_AXIS_STRIDE, COARSE_DOWNSAMPLE_STEP,
    };
```

with:

```rust
    use super::{
        coarse_axis_offsets, coarse_sample_dimensions, content_roi, coarse_samples,
        edge_projection, estimate_motion, estimate_motion_with_budget, refinement_offsets,
        template_refine_radius, to_grayscale, MotionSearchOutcome, PreparedFrame, SearchAxis,
        SearchBudget, COARSE_AXIS_STRIDE, COARSE_DOWNSAMPLE_STEP,
    };
```

Then add these tests inside the same `mod tests` block (e.g. right after `content_roi_skips_borders`):

```rust
    #[test]
    fn prepared_frame_signature_matches_old_signature() {
        let img = make_textured_canvas(160, 200);
        let prep = PreparedFrame::new(img.clone());
        assert_eq!(prep.signature(), crate::duplicate::signature(&img).as_slice());
    }

    #[test]
    fn prepared_frame_gray_matches_old_to_grayscale() {
        let img = make_textured_canvas(160, 200);
        let prep = PreparedFrame::new(img.clone());
        assert_eq!(prep.gray(), to_grayscale(&img).as_slice());
    }

    #[test]
    fn prepared_frame_coarse_matches_old_coarse_samples() {
        let img = make_textured_canvas(160, 200);
        let gray = to_grayscale(&img);
        let expected = coarse_samples(&gray, 160, 200, COARSE_DOWNSAMPLE_STEP);
        let prep = PreparedFrame::new(img.clone());
        assert_eq!(prep.coarse(), expected.as_slice());
        // Lazy OnceLock build is idempotent.
        assert_eq!(prep.coarse(), expected.as_slice());
        assert_eq!(
            prep.coarse_dims,
            coarse_sample_dimensions(160, 200, COARSE_DOWNSAMPLE_STEP)
        );
    }

    #[test]
    fn prepared_frame_projection_matches_old_edge_projection() {
        let img = make_textured_canvas(160, 200);
        let gray = to_grayscale(&img);
        let prep = PreparedFrame::new(img.clone());
        assert_eq!(
            prep.projection(SearchAxis::Vertical),
            edge_projection(&gray, 160, 200, SearchAxis::Vertical).as_slice()
        );
        assert_eq!(
            prep.projection(SearchAxis::Horizontal),
            edge_projection(&gray, 160, 200, SearchAxis::Horizontal).as_slice()
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `rtk cargo test -p rollshot-core --lib matcher::tests::prepared_frame 2>&1 | head -40`
Expected: compile error — `cannot find type PreparedFrame`, `cannot find function to_grayscale`/`coarse_samples`/`edge_projection` in `super` exports, etc.

- [ ] **Step 3: Add the `OnceLock` import**

In `crates/rollshot-core/src/matcher.rs`, add the import after the existing `use` lines at the top (after line 9):

```rust
use std::sync::OnceLock;
```

- [ ] **Step 4: Add the `PreparedFrame` struct and impl**

In `crates/rollshot-core/src/matcher.rs`, insert this block immediately **before** `fn to_grayscale(img: &RgbaImage) -> Vec<f32> {` (currently line 894):

```rust
/// A frame plus its derived matcher inputs. Grayscale is built eagerly; coarse
/// samples and edge projections are built lazily on first use and cached, so a
/// frame carried forward as `last_good` does not recompute them next round.
pub(crate) struct PreparedFrame {
    rgba: RgbaImage,
    width: u32,
    height: u32,
    signature: Vec<u8>,
    gray: Vec<f32>,
    coarse_dims: (u32, u32),
    coarse: OnceLock<Vec<f32>>,
    proj_v: OnceLock<Vec<f32>>,
    proj_h: OnceLock<Vec<f32>>,
}

impl PreparedFrame {
    /// Build from an owned frame, computing the duplicate signature internally.
    pub(crate) fn new(rgba: RgbaImage) -> Self {
        let signature = crate::duplicate::signature(&rgba);
        Self::from_parts(rgba, signature)
    }

    /// Build from an owned frame whose duplicate signature was already computed
    /// (e.g. by the stitcher's duplicate gate), avoiding a second pass.
    pub(crate) fn from_parts(rgba: RgbaImage, signature: Vec<u8>) -> Self {
        let width = rgba.width();
        let height = rgba.height();
        let gray = to_grayscale(&rgba);
        let coarse_dims = coarse_sample_dimensions(width, height, COARSE_DOWNSAMPLE_STEP);
        Self {
            rgba,
            width,
            height,
            signature,
            gray,
            coarse_dims,
            coarse: OnceLock::new(),
            proj_v: OnceLock::new(),
            proj_h: OnceLock::new(),
        }
    }

    pub(crate) fn rgba(&self) -> &RgbaImage {
        &self.rgba
    }

    pub(crate) fn signature(&self) -> &[u8] {
        &self.signature
    }

    pub(crate) fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn gray(&self) -> &[f32] {
        &self.gray
    }

    fn coarse(&self) -> &[f32] {
        self.coarse
            .get_or_init(|| coarse_samples(&self.gray, self.width, self.height, COARSE_DOWNSAMPLE_STEP))
    }

    fn projection(&self, axis: SearchAxis) -> &[f32] {
        match axis {
            SearchAxis::Vertical => self
                .proj_v
                .get_or_init(|| edge_projection(&self.gray, self.width, self.height, SearchAxis::Vertical)),
            SearchAxis::Horizontal => self
                .proj_h
                .get_or_init(|| edge_projection(&self.gray, self.width, self.height, SearchAxis::Horizontal)),
        }
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `rtk cargo test -p rollshot-core --lib matcher::tests::prepared_frame 2>&1 | tail -20`
Expected: 4 tests pass (`prepared_frame_signature_matches_old_signature`, `..._gray_matches_old_to_grayscale`, `..._coarse_matches_old_coarse_samples`, `..._projection_matches_old_edge_projection`).

Note: `estimate_motion` does not yet consume `PreparedFrame`, so the rest of the matcher tests still compile against the old signature. They are migrated in Task 2.

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-core/src/matcher.rs
git commit -m "$(cat <<'EOF'
feat(core): add PreparedFrame with cached gray/coarse/projection

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: `estimate_motion` consumes `PreparedFrame`

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs` (`estimate_motion` 136-278; `relaxed_coarse_candidate` 282-337; `coarse_candidates` 616-657; `edge_projection_candidates` 763-784; `edge_projection_axis` 786-826; `estimate_motion_with_budget` 76-114; test call sites)

This is a refactor; the existing matcher tests (migrated in Step 6) are the safety net. Output must not change.

- [ ] **Step 1: Rewrite `estimate_motion` signature and grayscale/dispatch block**

Replace the signature (lines 136-143) — change `prev`/`curr` types:

```rust
pub(crate) fn estimate_motion(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> MotionSearchOutcome {
```

Then replace the dimension-check + grayscale + coarse block (lines 162-180) with:

```rust
    if prev.dimensions() != curr.dimensions() {
        return MotionSearchOutcome::NoMatch {
            reason: NoMatchReason::DimensionMismatch,
            best_candidate: None,
        };
    }

    let (width, height) = prev.dimensions();

    let mut candidates = Vec::new();
    let coarse = {
        let _t = ScopedTimer::new(&mut metrics.coarse_us);
        coarse_candidates(prev, curr, locked_axis, config)
    };
```

(This removes the `prepare_frame_us` timer and the `prev_gray`/`curr_gray` locals; prepare timing now lives in the stitcher, Task 3.)

- [ ] **Step 2: Update the remaining helper calls inside `estimate_motion`**

In the same function, update each call that used `&prev_gray`/`&curr_gray`/`prev`/`curr`:

- `template_candidates(...)` (lines 184-194): change the first two args from `&prev_gray, &curr_gray` to `prev.gray(), curr.gray()`.
- `edge_projection_candidates(...)` (lines 198-206): replace the whole call with `edge_projection_candidates(prev, curr, locked_axis, config, metrics)`.
- `rank_verified_candidates(prev, curr, ...)` (line 213): change to `rank_verified_candidates(prev.rgba(), curr.rgba(), locked_axis, candidates, config)`.
- `relaxed_coarse_candidate(...)` (lines 224-235): replace the whole call with `relaxed_coarse_candidate(prev, curr, locked_axis, last_motion, config, metrics)`.
- `feature_fallback_candidates(prev, curr, ...)` (line 241): change to `feature_fallback_candidates(prev.rgba(), curr.rgba(), locked_axis, config)`.
- `rank_verified_candidates(prev, curr, ...)` (line 267): change to `rank_verified_candidates(prev.rgba(), curr.rgba(), locked_axis, candidates, config)`.

- [ ] **Step 3: Rewrite `coarse_candidates`**

Replace `coarse_candidates` (lines 616-657) with:

```rust
fn coarse_candidates(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<ScrollAxis>,
    config: &StitchConfig,
) -> Vec<MotionCandidate> {
    let (width, height) = prev.dimensions();
    let step = COARSE_DOWNSAMPLE_STEP as i32;
    let (sample_w, sample_h) = prev.coarse_dims;
    let prev_samples = prev.coarse();
    let curr_samples = curr.coarse();
    let max_dx = ((width as f32 * config.max_search_ratio) as i32 / step).max(0);
    let max_dy = ((height as f32 * config.max_search_ratio) as i32 / step).max(0);

    let mut out = Vec::new();
    for axis in search_axes(locked_axis) {
        let max_offset = match axis {
            SearchAxis::Vertical => max_dy,
            SearchAxis::Horizontal => max_dx,
        };
        if let Some(candidate) =
            coarse_axis_candidate(prev_samples, curr_samples, sample_w, sample_h, *axis, max_offset)
        {
            out.push(candidate);
        }
    }

    out.into_iter()
        .map(|mut candidate| {
            candidate.dx *= step;
            candidate.dy *= step;
            candidate
        })
        .filter(|candidate| candidate_matches_axis(candidate.dx, candidate.dy, locked_axis, config))
        .collect()
}
```

- [ ] **Step 4: Rewrite `edge_projection_candidates` and `edge_projection_axis`**

Replace `edge_projection_candidates` (lines 763-784) with:

```rust
fn edge_projection_candidates(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<ScrollAxis>,
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> Vec<MotionCandidate> {
    let _ = metrics; // reserved for edge-stage counters; timing is captured by the outer ScopedTimer.
    let (width, height) = prev.dimensions();
    let mut out = Vec::new();

    for axis in search_axes(locked_axis) {
        if let Some(candidate) = edge_projection_axis(
            prev.projection(*axis),
            curr.projection(*axis),
            width,
            height,
            *axis,
            config,
        ) {
            out.push(candidate);
        }
    }

    out
}
```

Replace `edge_projection_axis` (lines 786-826) with (takes prebuilt projections instead of building them):

```rust
fn edge_projection_axis(
    prev_proj: &[f32],
    curr_proj: &[f32],
    width: u32,
    height: u32,
    axis: SearchAxis,
    config: &StitchConfig,
) -> Option<MotionCandidate> {
    let max_offset = match axis {
        SearchAxis::Vertical => (height as f32 * config.max_search_ratio) as i32,
        SearchAxis::Horizontal => (width as f32 * config.max_search_ratio) as i32,
    };
    if max_offset <= 0 {
        return None;
    }

    let mut scored = Vec::new();
    for offset in signed_predict_iter(max_offset, 0) {
        let score = projection_mad(prev_proj, curr_proj, offset, EDGE_PROJECTION_STEP as usize);
        if score.is_finite() {
            scored.push((score, offset));
        }
    }

    scored.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (best, offset) = *scored.first()?;
    let second = scored.get(1).map(|(score, _)| *score);
    let (dx, dy) = match axis {
        SearchAxis::Vertical => (0, offset),
        SearchAxis::Horizontal => (offset, 0),
    };

    Some(candidate(dx, dy, MatchMethod::Edge, best, second))
}
```

(The standalone `fn edge_projection` at line 828 is unchanged; it is now called only by `PreparedFrame::projection`.)

- [ ] **Step 5: Rewrite `relaxed_coarse_candidate`**

Replace `relaxed_coarse_candidate` including its `#[allow(clippy::too_many_arguments)]` attribute (lines 282-337) with (now 6 args, so the attribute is dropped):

```rust
fn relaxed_coarse_candidate(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
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
    let (width, height) = prev.dimensions();

    let mut relaxed_cfg = config.clone();
    relaxed_cfg.max_search_ratio = RELAXED_SEARCH_RATIO;

    let coarse = coarse_candidates(prev, curr, locked_axis, &relaxed_cfg);
    metrics.coarse_candidates = metrics.coarse_candidates.max(coarse.len());
    if coarse.is_empty() {
        return None;
    }

    // Coarse is stride-8 in sample space (32 px in pixel space) — too coarse
    // to pass the verifier on its own. Use it to seed a relaxed template
    // refinement, which lands on a single-pixel offset that the verifier can
    // accept on the same min_overlap budget.
    let mut candidates = coarse.clone();
    candidates.extend(template_candidates(
        prev.gray(),
        curr.gray(),
        width,
        height,
        locked_axis,
        last_motion,
        &coarse,
        &relaxed_cfg,
        metrics,
    ));

    metrics.verifier_candidates += candidates.len();
    let _t = ScopedTimer::new(&mut metrics.verifier_us);
    rank_verified_candidates(prev.rgba(), curr.rgba(), locked_axis, candidates, config)
}
```

- [ ] **Step 6: Migrate the test-only `estimate_motion_with_budget` and direct test call sites**

Replace the body of `estimate_motion_with_budget` (lines 76-114) so it keeps `&RgbaImage` params (the two budget call sites stay unchanged) but builds `PreparedFrame`s before calling `estimate_motion`:

```rust
#[cfg(test)]
fn estimate_motion_with_budget(
    prev: &RgbaImage,
    curr: &RgbaImage,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    config: &StitchConfig,
    budget: &mut SearchBudget,
) -> MotionSearchOutcome {
    // Hold the serialization lock for the entire scope (set Some → run →
    // take None) so no concurrent test can slip in NCC calls between
    // `estimate_motion` returning and the budget being taken.
    let _serialize = ESTIMATE_MOTION_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    IN_BUDGET_SCOPE.with(|c| c.set(true));
    let _restore = InBudgetScopeGuard;
    {
        let mut active = ACTIVE_SEARCH_BUDGET
            .lock()
            .expect("search budget mutex poisoned");
        assert!(active.is_none(), "nested search budgets are not supported");
        *active = Some(SearchBudget::default());
    }
    let prev_prepared = PreparedFrame::new(prev.clone());
    let curr_prepared = PreparedFrame::new(curr.clone());
    let mut throwaway_metrics = StitchMetrics::default();
    let result = estimate_motion(
        &prev_prepared,
        &curr_prepared,
        locked_axis,
        last_motion,
        config,
        &mut throwaway_metrics,
    );
    *budget = ACTIVE_SEARCH_BUDGET
        .lock()
        .expect("search budget mutex poisoned")
        .take()
        .unwrap_or_default();
    result
}
```

Then add a `prep` helper inside the `mod tests` block (e.g. next to `unwrap_candidate` at line 1142):

```rust
    fn prep(img: &RgbaImage) -> PreparedFrame {
        PreparedFrame::new(img.clone())
    }
```

Then update the 11 direct `estimate_motion(&prev, &curr, ...)` test call sites to wrap the two image args with `prep`. The call sites are at lines 1167, 1187, 1208, 1225, 1243, 1266, 1289, 1312, 1330, 1348, 1461. In each, change the first two arguments:

- from `&prev,` / `&curr,` to `&prep(&prev),` / `&prep(&curr),`

(The `estimate_motion_with_budget` call sites at lines 1398 and 1473 are NOT changed — they still pass `&prev, &curr` because the wrapper keeps `&RgbaImage` params.)

- [ ] **Step 7: Run the full matcher test suite**

Run: `rtk cargo test -p rollshot-core --lib matcher 2>&1 | tail -30`
Expected: all matcher tests pass — including `estimate_motion_finds_known_scroll`, `estimate_motion_finds_vertical_up_scroll`, `repeated_grid_is_rejected_by_second_best_margin`, `large_pair_stays_within_structural_search_budget`, and the four `prepared_frame_*` tests. Same offsets/outcomes as before (behavior unchanged).

- [ ] **Step 8: Commit**

```bash
git add crates/rollshot-core/src/matcher.rs
git commit -m "$(cat <<'EOF'
refactor(core): estimate_motion consumes PreparedFrame

Read prev/curr gray, coarse samples, and edge projections from PreparedFrame
caches instead of recomputing per call. No behavior change.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Cache `last_good` PreparedFrame across frames in `Stitcher`

**Files:**
- Modify: `crates/rollshot-core/src/stitcher.rs` (imports 1-13; struct 15-26; `new` 29-42; `push_frame_inner` 58-342; `accept_first_frame` 358-371)
- Modify: `crates/rollshot-core/tests/stitcher.rs` (add two tests)

- [ ] **Step 1: Update imports and struct fields**

In `crates/rollshot-core/src/stitcher.rs`, change the matcher import (line 6) from:

```rust
use crate::matcher::{estimate_motion, MotionSearchOutcome};
```

to:

```rust
use crate::matcher::{estimate_motion, MotionSearchOutcome, PreparedFrame};
```

Replace the two `last_good_*` fields in `struct Stitcher` (lines 18-19):

```rust
    last_good_frame: Option<RgbaImage>,
    last_good_signature: Option<Vec<u8>>,
```

with:

```rust
    last_good: Option<PreparedFrame>,
```

In `Stitcher::new` (lines 32-34), replace:

```rust
            canvas: None,
            last_good_frame: None,
            last_good_signature: None,
```

with:

```rust
            canvas: None,
            last_good: None,
```

- [ ] **Step 2: Update `accept_first_frame`**

In `accept_first_frame` (lines 367-369), replace:

```rust
        self.last_good_signature = Some(duplicate::signature(&frame));
        self.last_good_frame = Some(frame.clone());
        self.canvas = Some(StripCanvas::new(frame));
```

with:

```rust
        self.last_good = Some(PreparedFrame::new(frame.clone()));
        self.canvas = Some(StripCanvas::new(frame));
```

- [ ] **Step 3: Rewrite `push_frame_inner` to use the cached anchor and build curr after the dup gate**

Replace the entire body of `push_frame_inner` (lines 58-342) with the following. The changes from the original: `anchor` is a `&PreparedFrame`; the duplicate gate reads `anchor.signature()`; the curr `PreparedFrame` is built (timed under `prepare_frame_us`) only after passing the dup gate; every `anchor`/`&frame` argument to `build_estimate`, `verifier.verify`, and `canvas.append` becomes `anchor.rgba()`/`curr.rgba()`; on `Appended` the curr `PreparedFrame` is moved into `self.last_good`.

```rust
    fn push_frame_inner(&mut self, frame: RgbaImage) -> StitchOutcome {
        if self.canvas.is_none() {
            let outcome = self.accept_first_frame(frame);
            self.last_metrics.outcome = StitchOutcomeKind::FirstFrame;
            return outcome;
        }

        let anchor = self
            .last_good
            .as_ref()
            .expect("anchor present after first frame");

        if anchor.dimensions() != frame.dimensions() {
            self.last_metrics
                .set_no_match(NoMatchReason::DimensionMismatch);
            return StitchOutcome::NoMatch {
                reason: NoMatchReason::DimensionMismatch,
                best_estimate: None,
            };
        }

        let signature = {
            let _t = crate::metrics::ScopedTimer::new(&mut self.last_metrics.duplicate_us);
            duplicate::signature(&frame)
        };
        if duplicate::is_duplicate(anchor.signature(), &signature, self.config.duplicate_threshold) {
            self.last_metrics.outcome = StitchOutcomeKind::Duplicate;
            return StitchOutcome::Duplicate;
        }

        let curr = {
            let _t = crate::metrics::ScopedTimer::new(&mut self.last_metrics.prepare_frame_us);
            PreparedFrame::from_parts(frame, signature)
        };

        let candidate = match estimate_motion(
            anchor,
            &curr,
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
                    build_estimate(
                        anchor.rgba(),
                        curr.rgba(),
                        &candidate,
                        self.config.axis_ratio_threshold,
                    )
                });
                self.last_metrics.set_no_match(reason);
                return StitchOutcome::NoMatch {
                    reason,
                    best_estimate,
                };
            }
        };

        self.last_metrics.best_dx = candidate.dx;
        self.last_metrics.best_dy = candidate.dy;
        self.last_metrics.best_score = candidate.score;
        self.last_metrics.second_best_score = candidate.second_best_score;
        self.last_metrics.match_method = Some(candidate.method);

        if candidate.score > self.config.accept_confidence {
            self.last_metrics.set_no_match(NoMatchReason::LowConfidence);
            return StitchOutcome::NoMatch {
                reason: NoMatchReason::LowConfidence,
                best_estimate: build_estimate(
                    anchor.rgba(),
                    curr.rgba(),
                    &candidate,
                    self.config.axis_ratio_threshold,
                ),
            };
        }

        let direction = match self.classify_direction(&candidate) {
            DirectionResult::Direction(dir) => dir,
            DirectionResult::Ambiguous => {
                self.last_metrics.set_no_match(NoMatchReason::AmbiguousAxis);
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::AmbiguousAxis,
                    best_estimate: build_estimate(
                        anchor.rgba(),
                        curr.rgba(),
                        &candidate,
                        self.config.axis_ratio_threshold,
                    ),
                };
            }
            DirectionResult::CrossAxisTooLarge => {
                self.last_metrics
                    .set_no_match(NoMatchReason::CrossAxisTooLarge);
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::CrossAxisTooLarge,
                    best_estimate: build_estimate(
                        anchor.rgba(),
                        curr.rgba(),
                        &candidate,
                        self.config.axis_ratio_threshold,
                    ),
                };
            }
            DirectionResult::AxisChanged { new_axis, locked } => {
                let estimate = build_estimate(
                    anchor.rgba(),
                    curr.rgba(),
                    &candidate,
                    self.config.axis_ratio_threshold,
                )
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
                self.last_metrics
                    .set_no_match(NoMatchReason::ReverseDirection);
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::ReverseDirection,
                    best_estimate: build_estimate(
                        anchor.rgba(),
                        curr.rgba(),
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
                    anchor.rgba(),
                    curr.rgba(),
                    &candidate,
                    self.config.axis_ratio_threshold,
                ),
            };
        }

        let verifier = PixelOverlapVerifier::new(&self.config.verifier, self.config.min_overlap);
        let (overlap_region, _verifier_score) = {
            let _t = crate::metrics::ScopedTimer::new(&mut self.last_metrics.verifier_us);
            match verifier.verify(anchor.rgba(), curr.rgba(), &candidate) {
                VerifierOutcome::Pass { overlap, score } => (overlap, score),
                VerifierOutcome::InsufficientOverlap => {
                    drop(_t);
                    self.last_metrics
                        .set_no_match(NoMatchReason::InsufficientOverlap);
                    return StitchOutcome::NoMatch {
                        reason: NoMatchReason::InsufficientOverlap,
                        best_estimate: build_estimate(
                            anchor.rgba(),
                            curr.rgba(),
                            &candidate,
                            self.config.axis_ratio_threshold,
                        ),
                    };
                }
                VerifierOutcome::OverlapDisagreement { .. } => {
                    drop(_t);
                    self.last_metrics
                        .set_no_match(NoMatchReason::OverlapVerificationFailed);
                    return StitchOutcome::NoMatch {
                        reason: NoMatchReason::OverlapVerificationFailed,
                        best_estimate: build_estimate(
                            anchor.rgba(),
                            curr.rgba(),
                            &candidate,
                            self.config.axis_ratio_threshold,
                        ),
                    };
                }
            }
        };

        // Run the append in its own scope so the canvas borrow (and the append
        // timer) are released before the error match. That lets the error arms
        // borrow `anchor`/`self.last_metrics` again and compute `build_estimate`
        // lazily — only on the rejection paths that actually need it.
        let append_result = {
            let _t = crate::metrics::ScopedTimer::new(&mut self.last_metrics.append_us);
            let canvas = self
                .canvas
                .as_mut()
                .expect("canvas present after first frame");
            canvas.append(direction, curr.rgba(), slice_px)
        };
        let added = match append_result {
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
                self.last_metrics.outcome = StitchOutcomeKind::AxisChanged;
                return StitchOutcome::AxisChanged {
                    previous_axis: locked,
                    new_axis: attempted,
                    estimate,
                };
            }
            Err(CanvasAppendError::DimensionMismatch { .. }) => {
                self.last_metrics
                    .set_no_match(NoMatchReason::DimensionMismatch);
                return StitchOutcome::NoMatch {
                    reason: NoMatchReason::DimensionMismatch,
                    best_estimate: build_estimate(
                        anchor.rgba(),
                        curr.rgba(),
                        &candidate,
                        self.config.axis_ratio_threshold,
                    ),
                };
            }
            Err(CanvasAppendError::EmptyAppend) => {
                self.last_metrics.outcome = StitchOutcomeKind::NoProgress;
                return StitchOutcome::NoProgress {
                    estimate: build_estimate(
                        anchor.rgba(),
                        curr.rgba(),
                        &candidate,
                        self.config.axis_ratio_threshold,
                    ),
                };
            }
        };
        let (canvas_height, canvas_width, append_copied_bytes) = {
            let canvas = self
                .canvas
                .as_ref()
                .expect("canvas present after first frame");
            (
                canvas.height(),
                canvas.width(),
                canvas.last_append_copied_bytes(),
            )
        };
        self.last_metrics.append_copied_bytes = append_copied_bytes;

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

        self.last_good = Some(curr);
        self.stats.frame_count += 1;
        self.stats.total_height = canvas_height;
        self.stats.total_width = canvas_width;
        self.stats.last_append = added;

        self.last_metrics.outcome = StitchOutcomeKind::Appended;

        StitchOutcome::Appended {
            direction,
            added,
            estimate,
        }
    }
```

- [ ] **Step 4: Build to verify the borrow checker is satisfied**

Run: `rtk cargo build -p rollshot-core 2>&1 | tail -20`
Expected: builds cleanly. (Confirms `anchor` borrow of `self.last_good` ends before `self.last_good = Some(curr)` on the success path, and `duplicate` is still used so no unused-import warning.)

- [ ] **Step 5: Write the anchor-advancement behavioral test**

The "NoMatch preserves the anchor" invariant is **already covered** by the existing
`bad_frame_returns_no_match_and_preserves_anchor` test (first → white frame =
NoMatch with `frame_count` still 1 → recovered frame appends). That test must stay
green; do not duplicate it.

Add one new test for the "Appended advances the anchor to the latest frame"
invariant. Use **80px** scroll steps — the value the existing
`normal_scroll_appends_bottom_and_locks_vertical_axis` test proves appends with
`added` in `76..=84` under default config (a 40px step is below that proven point
and risks `NoProgress` depending on the default `min_append`).

In `crates/rollshot-core/tests/stitcher.rs`, add at the end of the file (uses the
already-imported `make_scroll_canvas`, `crop_frame`, `StitchOutcome`, `Stitcher`,
`StitchConfig`, `AppendDirection`):

```rust
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
```

- [ ] **Step 6: Run the stitcher tests**

Run: `rtk cargo test -p rollshot-core --test stitcher 2>&1 | tail -30`
Expected: all stitcher integration tests pass, including the new `appended_advances_anchor_to_latest_frame`, the existing `bad_frame_returns_no_match_and_preserves_anchor` (the NoMatch-preserves-anchor invariant), and `duplicate_frame_returns_duplicate_without_growing`.

- [ ] **Step 7: Commit**

```bash
git add crates/rollshot-core/src/stitcher.rs crates/rollshot-core/tests/stitcher.rs
git commit -m "$(cat <<'EOF'
perf(core): cache last_good PreparedFrame across frames

Stitcher holds last_good as a PreparedFrame and builds the curr frame only
after the duplicate gate, moving it into last_good on append. Prev gray/coarse
are no longer recomputed each frame. Output unchanged.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Verify equivalence, lint, and benchmark

**Files:** none modified except the committed compare report (after user acceptance).

- [ ] **Step 1: Full test suite (golden byte-identity)**

Run: `rtk cargo test -p rollshot-core 2>&1 | tail -30`
Expected: every test passes, including `tests/golden_fixtures.rs` (golden outputs are byte-identical — this is the core correctness gate for the refactor).

- [ ] **Step 2: Format check**

Run: `rtk cargo fmt --check`
Expected: no output (formatting clean). If it reports diffs, run `rtk cargo fmt` and amend.

- [ ] **Step 3: Clippy**

Run: `rtk cargo clippy -p rollshot-core --all-targets -- -D warnings 2>&1 | tail -20`
Expected: no warnings. (Watch for: unused `duplicate` import, dead `edge_projection`/`coarse_samples`/`to_grayscale` — all should still be used via `PreparedFrame`; an `#[allow(clippy::too_many_arguments)]` that is no longer needed on `relaxed_coarse_candidate`.)

- [ ] **Step 4: Capture the "after" benchmark**

Run:
```bash
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter --repeats 3 \
    --out bench-results/runs/p2-prepared-frame/after.jsonl
```
Expected: `bench-results/runs/p2-prepared-frame/after.jsonl` exists.

- [ ] **Step 5: Compare (preview, not committed)**

Run:
```bash
rtk python3 scripts/bench/compare.py \
    bench-results/runs/p2-prepared-frame/before.jsonl \
    bench-results/runs/p2-prepared-frame/after.jsonl
```
Expected:
- `output_pixel_hash` is **same** for all three fixtures (no correctness drift) — this is mandatory.
- `prepare_frame_us` (p50) drops (target ~30–50%).
- `coarse_us` (p50) drops modestly.
- `append_us` (p95) and `peak_rss` not regressed.

If the pixel hash differs on any fixture, STOP — the refactor changed output; debug before proceeding (use superpowers:systematic-debugging).

- [ ] **Step 6: Present results to the user and get acceptance**

Show the compare table. Do NOT write the committed report until the user accepts the numbers. Once accepted, proceed to Step 7.

- [ ] **Step 7: Write and commit the accepted compare report**

Substitute `<BEFORE_SHA>` with the short SHA recorded in Task 0 Step 2, and `<AFTER_SHA>` with `git rev-parse --short HEAD`:

```bash
rtk python3 scripts/bench/compare.py \
    --include-frontmatter \
    --benchmark-id 2026-05-27-p2-prepared-frame \
    --benchmark-scope p2-prepared-frame \
    --roadmap-item P2 \
    --status user_accepted \
    --date 2026-05-27 \
    --command "rtk cargo bench -p rollshot-core --bench stitch_sequences -- --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter --repeats 3 --out bench-results/runs/p2-prepared-frame/after.jsonl" \
    --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter \
    --repeats 3 \
    bench-results/runs/p2-prepared-frame/before.jsonl \
    bench-results/runs/p2-prepared-frame/after.jsonl \
    > bench-results/compare/2026-05-27-p2-prepared-frame-compare.md
```

Then commit only the markdown report:

```bash
git add bench-results/compare/2026-05-27-p2-prepared-frame-compare.md
git commit -m "$(cat <<'EOF'
docs(bench): add P2 PreparedFrame compare report

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Acceptance checklist (verify before declaring done)

- [ ] Golden fixtures byte-identical (`output_pixel_hash` same on all three fixtures).
- [ ] All `rollshot-core` tests pass; `fmt --check` clean; `clippy -D warnings` clean.
- [ ] `prepare_frame_us` (p50) down ~30–50%; `coarse_us` (p50) down; `append_us`/`peak_rss` not regressed.
- [ ] `Duplicate` returns before building the curr `PreparedFrame` (gray not built on dup) — confirmed by the early-return placement in Task 3 Step 3.
- [ ] `NoMatch` leaves `last_good` unchanged (existing `bad_frame_returns_no_match_and_preserves_anchor` stays green).
- [ ] `Appended` advances `last_good` to the new frame (`appended_advances_anchor_to_latest_frame`).
- [ ] PixelOverlapVerifier still the final gate; ReverseDirection still rejected (existing tests).
- [ ] Feature-flag-free; `Stitcher` remains `Send + Sync` (`cargo build -p rollshot-core` clean; `OnceLock` used).
