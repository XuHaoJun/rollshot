# Core Large-Scroll Stitching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let `rollshot-core` stitch near-page scroll jumps that still have at least the configured verifiable overlap.

**Architecture:** Keep the current matcher pipeline unchanged: normal candidates, relaxed large-scroll recovery, then feature fallback. Refactor only the relaxed recovery window so it uses geometry-derived per-axis limits (`frame_dim - min_overlap`) instead of a fixed 85% ratio, and keep all existing verifier gates.

**Tech Stack:** Rust, `rollshot-core`, `image::RgbaImage`, existing `Stitcher`, matcher, and integration tests.

---

## File Structure

- Modify `crates/rollshot-core/tests/stitcher.rs`
  - Add a terminal-like synthetic canvas helper local to this integration test.
  - Add one strict positive regression test for an 800 px jump in a 900 px viewport.
  - Add one strict negative regression test for a jump that leaves less than `min_overlap`.
- Modify `crates/rollshot-core/src/matcher.rs`
  - Replace the fixed relaxed search ratio with per-axis relaxed limits derived from `min_overlap`.
  - Add small private helpers for coarse/template candidates with explicit pixel limits.
  - Preserve the existing public API and existing normal matcher behavior.

No other files should change for implementation. Do not edit macOS capture code, overlay UI, app UI text, or public type definitions.

---

### Task 0: Capture Baseline Stitching Performance

**Files:**
- Verify only: `crates/rollshot-core/benches/stitch_sequences.rs`

- [ ] **Step 1: Run the baseline benchmark before code edits**

Run:

```bash
rtk cargo bench -p rollshot-core --bench stitch_sequences -- --out bench-results/runs/core-large-scroll/before.jsonl
```

Expected:

- Command exits successfully.
- `bench-results/runs/core-large-scroll/before.jsonl` exists.
- Do not stage `bench-results/runs/core-large-scroll/before.jsonl`; `bench-results/runs/` is a local gitignored artifact.

---

### Task 1: Add Strict Failing Large-Scroll Tests

**Files:**
- Modify: `crates/rollshot-core/tests/stitcher.rs`
- Test: `crates/rollshot-core/tests/stitcher.rs`

- [ ] **Step 1: Add a terminal-like canvas helper**

In `crates/rollshot-core/tests/stitcher.rs`, after `duplicate_frame_returns_duplicate_without_growing`, add this helper:

```rust
fn make_terminal_like_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([12, 14, 18, 255]));

    for y in 0..height {
        let row = y / 18;
        let row_bg = if row % 2 == 0 { 18 } else { 22 };
        for x in 0..width {
            img.put_pixel(x, y, Rgba([row_bg, row_bg + 2, row_bg + 6, 255]));
        }

        if y % 18 == 13 {
            let prompt_w = 16 + (row % 11) * 3;
            for x in 18..(18 + prompt_w).min(width.saturating_sub(18)) {
                img.put_pixel(x, y, Rgba([80, 190, 120, 255]));
            }
        }

        if y % 18 == 14 {
            let text_start = 48 + (row % 7) * 5;
            let text_end = (text_start + 180 + (row % 17) * 9).min(width.saturating_sub(24));
            for x in text_start..text_end {
                if (x + row * 13) % 9 < 6 {
                    let tone = 130 + ((row * 19 + x / 5) % 90) as u8;
                    img.put_pixel(x, y, Rgba([tone, tone, tone, 255]));
                }
            }
        }

        if y % 72 == 3 {
            let color = [
                (60 + (row * 23) % 160) as u8,
                (70 + (row * 31) % 150) as u8,
                (90 + (row * 37) % 130) as u8,
                255,
            ];
            for x in 30..width.saturating_sub(30) {
                if (x / 4 + row) % 3 != 0 {
                    img.put_pixel(x, y, Rgba(color));
                }
            }
        }
    }

    img
}
```

- [ ] **Step 2: Add the positive strict regression test**

In the same file, immediately after the helper, add:

```rust
#[test]
fn large_scroll_beyond_old_relaxed_ratio_appends_when_overlap_verifies() {
    const W: u32 = 720;
    const FRAME_H: u32 = 900;
    const OFFSET: u32 = 800;
    const EXPECTED_OVERLAP: u32 = FRAME_H - OFFSET;

    let canvas = make_terminal_like_canvas(W, 2200);
    let first = crop_frame(&canvas, 0, FRAME_H);
    let scrolled = crop_frame(&canvas, OFFSET, FRAME_H);

    let mut config = StitchConfig::default();
    config.fast_hnsw.enabled = false;
    assert_eq!(config.min_overlap, 64);
    assert_eq!(
        (FRAME_H as f32 * 0.85) as u32,
        765,
        "test must stay beyond the old fixed relaxed search ceiling"
    );
    assert!(
        OFFSET > (FRAME_H as f32 * 0.85) as u32,
        "OFFSET={OFFSET} must exceed old 85% ceiling"
    );
    assert!(
        EXPECTED_OVERLAP >= config.min_overlap,
        "test must leave enough configured overlap"
    );

    let mut stitcher = Stitcher::new(config.clone());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended {
            direction,
            added,
            estimate,
        } => {
            assert_eq!(direction, AppendDirection::Bottom);
            assert_eq!(estimate.axis, ScrollAxis::Vertical);
            assert_eq!(estimate.dx, 0);
            assert!(
                (OFFSET - 2..=OFFSET + 2).contains(&added),
                "added = {added}, expected {OFFSET}"
            );
            assert!(
                (OFFSET as i32 - 2..=OFFSET as i32 + 2).contains(&estimate.dy),
                "estimate.dy = {}, expected {OFFSET}",
                estimate.dy
            );
            assert!(
                (EXPECTED_OVERLAP - 2..=EXPECTED_OVERLAP + 2).contains(&estimate.overlap.height),
                "overlap height {} not close to expected {}",
                estimate.overlap.height,
                EXPECTED_OVERLAP
            );
            assert_eq!(estimate.overlap.width, W);
            assert_eq!(estimate.overlap.curr_y, 0);
            assert!(
                (OFFSET - 2..=OFFSET + 2).contains(&estimate.overlap.prev_y),
                "overlap prev_y = {}, expected {OFFSET}",
                estimate.overlap.prev_y
            );
        }
        other => panic!("expected Appended for verifiable large scroll, got {other:?}"),
    }

    let stats = stitcher.stats();
    assert_eq!(stats.frame_count, 2);
    assert!(
        (FRAME_H + OFFSET - 2..=FRAME_H + OFFSET + 2).contains(&stats.total_height),
        "total_height = {}, expected about {}",
        stats.total_height,
        FRAME_H + OFFSET
    );
}
```

- [ ] **Step 3: Add the negative strict regression test**

After the positive test, add:

```rust
#[test]
fn large_scroll_below_min_overlap_does_not_append_or_grow_canvas() {
    const W: u32 = 720;
    const FRAME_H: u32 = 900;

    let mut config = StitchConfig::default();
    config.fast_hnsw.enabled = false;
    let offset = FRAME_H - config.min_overlap + 1;
    assert_eq!(FRAME_H - offset, config.min_overlap - 1);

    let canvas = make_terminal_like_canvas(W, 2200);
    let first = crop_frame(&canvas, 0, FRAME_H);
    let scrolled = crop_frame(&canvas, offset, FRAME_H);

    let mut stitcher = Stitcher::new(config.clone());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);
    let before_stats = stitcher.stats();
    let before_dims = stitcher
        .full_image()
        .expect("first frame should be stored")
        .dimensions();

    match stitcher.push_frame(scrolled) {
        StitchOutcome::NoMatch { best_estimate, .. } => {
            if let Some(estimate) = best_estimate {
                assert!(
                    estimate.overlap.height >= config.min_overlap,
                    "best estimate exposed below-floor overlap: {} < {}",
                    estimate.overlap.height,
                    config.min_overlap
                );
            }
        }
        StitchOutcome::AxisChanged { .. } => panic!("below-min-overlap vertical scroll changed axis"),
        StitchOutcome::Appended { .. } => {
            panic!("below-min-overlap large scroll must not append")
        }
        StitchOutcome::NoProgress { .. } => {
            panic!("below-min-overlap large scroll must not report no progress")
        }
        StitchOutcome::Duplicate | StitchOutcome::FirstFrame => {
            panic!("unexpected outcome for below-min-overlap large scroll")
        }
    }

    let after_stats = stitcher.stats();
    assert_eq!(after_stats.frame_count, before_stats.frame_count);
    assert_eq!(after_stats.total_height, before_stats.total_height);
    assert_eq!(after_stats.total_width, before_stats.total_width);
    assert_eq!(after_stats.last_append, before_stats.last_append);
    let after_dims = stitcher
        .full_image()
        .expect("canvas should still be stored")
        .dimensions();
    assert_eq!(after_dims, before_dims);
}
```

- [ ] **Step 4: Run the focused tests and verify the positive test fails**

Run:

```bash
rtk cargo test -p rollshot-core large_scroll -- --nocapture
```

Expected before implementation:

- `large_scroll_beyond_old_relaxed_ratio_appends_when_overlap_verifies` fails because `OFFSET = 800` is beyond the old relaxed `0.85 * 900 = 765` ceiling with feature fallback disabled.
- `large_scroll_below_min_overlap_does_not_append_or_grow_canvas` passes and proves the verifier floor still prevents canvas growth.

- [ ] **Step 5: Commit the failing tests**

Run:

```bash
rtk git add crates/rollshot-core/tests/stitcher.rs
rtk git commit -m "test(core): cover verifiable near-page scroll jumps"
```

---

### Task 2: Replace Fixed Relaxed Ratio With Geometry-Derived Limits

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`
- Test: `crates/rollshot-core/tests/stitcher.rs`

- [ ] **Step 1: Replace `RELAXED_SEARCH_RATIO` with explicit-limit helpers**

In `crates/rollshot-core/src/matcher.rs`, replace:

```rust
const RELAXED_SEARCH_RATIO: f32 = 0.85;
```

with:

```rust
fn relaxed_search_limits(
    prev: &PreparedFrame,
    config: &StitchConfig,
) -> Option<(i32, i32)> {
    let (width, height) = prev.dimensions();
    let max_dx = overlap_limited_offset(width, config.min_overlap);
    let max_dy = overlap_limited_offset(height, config.min_overlap);
    if max_dx <= 0 && max_dy <= 0 {
        return None;
    }

    let (current_max_dx, current_max_dy) = normal_search_limits(prev, config);
    let tolerance = COARSE_DOWNSAMPLE_STEP as i32;
    if current_max_dx + tolerance >= max_dx && current_max_dy + tolerance >= max_dy {
        return None;
    }

    Some((max_dx, max_dy))
}

fn normal_search_limits(prev: &PreparedFrame, config: &StitchConfig) -> (i32, i32) {
    let (width, height) = prev.dimensions();
    let ratio_dx = ratio_limited_offset(width, config.max_search_ratio);
    let ratio_dy = ratio_limited_offset(height, config.max_search_ratio);
    let overlap_dx = overlap_limited_offset(width, config.min_overlap);
    let overlap_dy = overlap_limited_offset(height, config.min_overlap);
    (ratio_dx.min(overlap_dx), ratio_dy.min(overlap_dy))
}

fn ratio_limited_offset(dim: u32, ratio: f32) -> i32 {
    ((dim as f32 * ratio) as i32).max(0)
}

fn overlap_limited_offset(dim: u32, min_overlap: u32) -> i32 {
    dim.saturating_sub(min_overlap) as i32
}
```

- [ ] **Step 2: Add coarse candidate helper with explicit pixel limits**

Replace the current `coarse_candidates_for_axes` function with:

```rust
fn coarse_candidates_for_axes(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<ScrollAxis>,
    axes: &[SearchAxis],
    config: &StitchConfig,
) -> Vec<MotionCandidate> {
    let (max_dx, max_dy) = normal_search_limits(prev, config);
    coarse_candidates_for_axes_with_limits(prev, curr, locked_axis, axes, max_dx, max_dy, config)
}

fn coarse_candidates_for_axes_with_limits(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<ScrollAxis>,
    axes: &[SearchAxis],
    max_dx_px: i32,
    max_dy_px: i32,
    config: &StitchConfig,
) -> Vec<MotionCandidate> {
    let step = COARSE_DOWNSAMPLE_STEP as i32;
    let (sample_w, sample_h) = prev.coarse_dims;
    let prev_samples = prev.coarse();
    let curr_samples = curr.coarse();
    let max_dx = (max_dx_px / step).max(0);
    let max_dy = (max_dy_px / step).max(0);

    let mut out = Vec::new();
    for axis in axes {
        let max_offset = match axis {
            SearchAxis::Vertical => max_dy,
            SearchAxis::Horizontal => max_dx,
        };
        if let Some(candidate) = coarse_axis_candidate(
            prev_samples,
            curr_samples,
            sample_w,
            sample_h,
            *axis,
            max_offset,
        ) {
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

- [ ] **Step 3: Add template candidate helper with explicit pixel limits**

Replace `template_candidates_for_axes` and `search_template_axis` with this refactor:

```rust
fn template_candidates_for_axes(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    last_motion: (i32, i32),
    coarse: &[MotionCandidate],
    axes: &[SearchAxis],
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> Vec<MotionCandidate> {
    let (max_dx, max_dy) = normal_search_limits(prev, config);
    template_candidates_for_axes_with_limits(
        prev,
        curr,
        last_motion,
        coarse,
        axes,
        max_dx,
        max_dy,
        config,
        metrics,
    )
}

fn template_candidates_for_axes_with_limits(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    last_motion: (i32, i32),
    coarse: &[MotionCandidate],
    axes: &[SearchAxis],
    max_dx_px: i32,
    max_dy_px: i32,
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> Vec<MotionCandidate> {
    let mut out = Vec::new();
    let (width, height) = prev.dimensions();
    let roi = content_roi(width, height);
    let match_region = match_width_region(roi, config.match_width);

    for axis in axes {
        let seed = template_seed(*axis, last_motion, coarse);
        let max_offset = match axis {
            SearchAxis::Vertical => max_dy_px,
            SearchAxis::Horizontal => max_dx_px,
        };
        if let Some(candidate) = search_template_axis_with_limit(
            prev,
            curr,
            *axis,
            match_region,
            seed,
            max_offset,
            metrics,
        ) {
            out.push(candidate);
        }
    }

    out
}

fn search_template_axis_with_limit(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    axis: SearchAxis,
    region: Region,
    last_offset: i32,
    max_offset: i32,
    metrics: &mut StitchMetrics,
) -> Option<MotionCandidate> {
    let (width, height) = prev.dimensions();
    if width < 50 || height < 50 {
        return None;
    }

    let max_offset = max_offset.max(0);
    if max_offset <= 0 {
        return None;
    }

    let offsets = refinement_offsets(last_offset, max_offset, template_refine_radius());
    metrics.ncc_offsets_scored += offsets.len();
    metrics.ncc_pixel_visits += offsets
        .len()
        .saturating_mul(region.w as usize * region.h as usize);
    let scored: Vec<_> = offsets
        .into_par_iter()
        .filter_map(|offset| {
            let score = match axis {
                SearchAxis::Vertical => fast_ncc_score_shifted(prev, curr, region, 0, offset),
                SearchAxis::Horizontal => fast_ncc_score_shifted(prev, curr, region, offset, 0),
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

- [ ] **Step 4: Update relaxed recovery to use explicit limits**

Replace the body of `relaxed_coarse_candidate` with:

```rust
fn relaxed_coarse_candidate(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> Option<MotionCandidate> {
    let (max_dx, max_dy) = relaxed_search_limits(prev, config)?;

    let coarse = coarse_candidates_for_axes_with_limits(
        prev,
        curr,
        locked_axis,
        dual_search_axes(),
        max_dx,
        max_dy,
        config,
    );
    metrics.coarse_candidates = metrics.coarse_candidates.max(coarse.len());
    if coarse.is_empty() {
        return None;
    }

    // Coarse is stride-8 in sample space (32 px in pixel space) — too coarse
    // to pass the verifier on its own. Use it to seed a relaxed template
    // refinement, which lands on a single-pixel offset that the verifier can
    // accept on the same min_overlap budget.
    let mut candidates = coarse.clone();
    candidates.extend(template_candidates_for_axes_with_limits(
        prev,
        curr,
        last_motion,
        &coarse,
        dual_search_axes(),
        max_dx,
        max_dy,
        config,
        metrics,
    ));

    metrics.verifier_candidates += candidates.len();
    let _t = ScopedTimer::new(&mut metrics.verifier_us);
    rank_verified_candidates(prev.rgba(), curr.rgba(), locked_axis, candidates, config)
}
```

- [ ] **Step 5: Update the existing fast-scroll regression comment**

In `crates/rollshot-core/tests/stitcher.rs`, update the comment inside
`fast_scroll_beyond_default_search_ratio_recovers_via_relaxed_pass` from the
old fixed-ratio wording to:

```rust
    // The default `max_search_ratio` (0.4) only reaches ~128 px on a
    // 320-tall frame. A 200 px scroll lands outside that envelope, so
    // every regular matcher misses. The relaxed coarse pass widens the
    // search to the geometry-derived overlap ceiling (height - min_overlap),
    // which must recover the offset.
```

- [ ] **Step 6: Run the focused tests and verify they pass**

Run:

```bash
rtk cargo test -p rollshot-core large_scroll -- --nocapture
```

Expected:

- `large_scroll_beyond_old_relaxed_ratio_appends_when_overlap_verifies` passes.
- `large_scroll_below_min_overlap_does_not_append_or_grow_canvas` passes.

- [ ] **Step 7: Run existing matcher/stitcher regression tests**

Run:

```bash
rtk cargo test -p rollshot-core fast_scroll_beyond_default_search_ratio_recovers_via_relaxed_pass -- --nocapture
rtk cargo test -p rollshot-core estimate_motion_respects_min_overlap -- --nocapture
rtk cargo test -p rollshot-core large_pair_stays_within_structural_search_budget -- --nocapture
```

Expected:

- All three tests pass.
- If `large_pair_stays_within_structural_search_budget` exceeds its budget, do not loosen the assertion immediately. First inspect candidate counts and ensure relaxed recovery is still only reached after normal verification fails.

- [ ] **Step 8: Commit the matcher implementation**

Run:

```bash
rtk git add crates/rollshot-core/src/matcher.rs crates/rollshot-core/tests/stitcher.rs
rtk git commit -m "fix(core): search verifiable near-page scroll jumps"
```

---

### Task 3: Full Verification and Review

**Files:**
- Verify: `crates/rollshot-core/src/matcher.rs`
- Verify: `crates/rollshot-core/tests/stitcher.rs`

- [ ] **Step 1: Run all core tests**

Run:

```bash
rtk cargo test -p rollshot-core
```

Expected:

- All `rollshot-core` unit and integration tests pass.

- [ ] **Step 2: Run formatting check**

Run:

```bash
rtk cargo fmt --check
```

Expected:

- Formatting check passes.

- [ ] **Step 3: Run clippy because matcher behavior is shared core logic**

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected:

- Clippy passes without warnings.

- [ ] **Step 4: Run the after benchmark and compare against the baseline**

Run:

```bash
rtk cargo bench -p rollshot-core --bench stitch_sequences -- --out bench-results/runs/core-large-scroll/after.jsonl
rtk python3 scripts/bench/compare.py bench-results/runs/core-large-scroll/before.jsonl bench-results/runs/core-large-scroll/after.jsonl
```

Expected:

- Benchmark command exits successfully.
- Compare command prints before/after metrics.
- If the comparison shows a material regression in matcher/verifier-heavy scenarios, inspect candidate counts before proceeding. Do not hide a regression by omitting the benchmark output from the final handoff.
- Do not stage `bench-results/runs/core-large-scroll/*.jsonl`; raw run artifacts are local and gitignored.

- [ ] **Step 5: Inspect the final diff**

Run:

```bash
rtk git diff --stat HEAD~2..HEAD
rtk git diff HEAD~2..HEAD -- crates/rollshot-core/src/matcher.rs crates/rollshot-core/tests/stitcher.rs
```

Expected:

- Diff touches only `crates/rollshot-core/src/matcher.rs` and `crates/rollshot-core/tests/stitcher.rs`.
- No macOS backend, overlay UI, app UI text, public API, or config default changes.
- Tests disable `fast_hnsw` in the new large-scroll cases so they specifically cover relaxed coarse/template recovery.

- [ ] **Step 6: Commit verification notes if any code changes were needed**

If verification required additional code or test changes, commit them:

```bash
rtk git add crates/rollshot-core/src/matcher.rs crates/rollshot-core/tests/stitcher.rs
rtk git commit -m "fix(core): finalize large-scroll stitching recovery"
```

If no files changed after Task 2, do not create an empty commit.

---

## Completion Criteria

- The new positive near-page scroll test fails before implementation and passes after implementation.
- The new negative below-min-overlap test proves no append/no-progress and no canvas growth.
- Existing fast-scroll and min-overlap tests still pass.
- `rtk cargo test -p rollshot-core` passes.
- `rtk cargo fmt --check` passes.
- `rtk cargo clippy --workspace --all-targets -- -D warnings` passes, or any failure is documented with the exact reason if unrelated to this change.
- `rtk cargo bench -p rollshot-core --bench stitch_sequences` has before/after JSONL runs under `bench-results/runs/core-large-scroll/`, and `scripts/bench/compare.py` output is summarized in the implementation handoff.

---

## Engineering Review Addendum

### Step 0: Scope Challenge

- Goal vs steps alignment: accepted as-is. Every task supports core-only near-page large-scroll stitching.
- Existing code reused: the plan reuses `Stitcher`, `estimate_motion`, `rank_verified_candidates`, `PixelOverlapVerifier`, `coarse_axis_candidate`, and `template_candidates` rather than adding a new matcher.
- Minimum viable plan: Tasks 0-3 are required because this touches `rollshot-core` stitching paths and project rules require before/after benchmark numbers. No task can be deferred without either losing strict coverage or skipping required verification.
- Complexity check: 0 new files, 2 modified files, 4 tasks. No scope reduction triggered.
- Search check: no new framework, runtime, infrastructure, concurrency pattern, or external dependency is introduced.
- Distribution check: no new artifact is introduced; no build/publish task is needed.

### Auto Decisions Applied During Review

Auto decision D1 — Preserve normal-path min-overlap limits
Context: The draft helper refactor risked using only `max_search_ratio` for the normal template path in `crates/rollshot-core/src/matcher.rs`.
ELI10: The normal matcher already refuses to search offsets that cannot leave enough overlap for verification. If the refactor loses that cap, it can waste work and risk changing existing matcher behavior. The fix is to keep a shared `normal_search_limits` helper and let only relaxed recovery expand to the geometry ceiling.
Stakes if we pick wrong: Existing min-overlap behavior and performance-sensitive matcher tests can regress.
Recommendation: D1A because it preserves established behavior while enabling the new relaxed path.
Completeness: A=10/10, B=6/10
Pros / cons:
A) Add `normal_search_limits` and use it for normal coarse/template paths (recommended)
  ✅ Preserves verifier floor semantics and keeps the diff explicit.
  ❌ Adds two small helper functions.
B) Use ratio-only limits everywhere
  ✅ Smaller implementation snippet.
  ❌ Lets normal search explore unverifiable offsets and risks `estimate_motion_respects_min_overlap`.
Net: This is a small explicit helper in exchange for preserving existing matcher contracts.

Auto decision D2 — Make overlap assertions exact enough to catch wrong offsets
Context: The draft positive test only asserted overlap height was at least `min_overlap`.
ELI10: A test can pass while still accepting the wrong large offset if it only checks a floor. The known offset leaves a known overlap, so the test should verify that number. This makes the test catch off-by-one and wrong-candidate failures.
Stakes if we pick wrong: A loose test could bless a large-scroll implementation that appends the wrong slice.
Recommendation: D2A because strict numeric assertions match the user's explicit test requirement.
Completeness: A=10/10, B=7/10
Pros / cons:
A) Assert expected overlap height and overlap coordinates within tolerance (recommended)
  ✅ Catches wrong-offset and wrong-overlap bugs directly.
  ❌ Slightly more test code.
B) Keep only `>= min_overlap`
  ✅ Shorter test.
  ❌ Does not prove the selected large-scroll candidate is the intended one.
Net: The extra assertions are cheap and materially improve the regression test.

Auto decision D3 — Require the below-floor case to remain `NoMatch`
Context: The draft negative test allowed `AxisChanged` even though the synthetic input is a vertical same-axis scroll.
ELI10: If a vertical scroll below the overlap floor comes back as an axis change, something unexpected happened in classification. The test should not normalize that. It should reject append/no-progress and treat axis-change as a failure.
Stakes if we pick wrong: A real matcher regression could be hidden behind a broad negative-test match arm.
Recommendation: D3A because it keeps the negative case specific and stricter.
Completeness: A=10/10, B=8/10
Pros / cons:
A) Panic on `AxisChanged` in the below-floor vertical case (recommended)
  ✅ Catches unexpected classification behavior.
  ❌ Slightly narrower test expectation.
B) Allow `AxisChanged` if canvas does not grow
  ✅ More tolerant of internal behavior.
  ❌ Hides a surprising result for a same-axis test fixture.
Net: Specific synthetic tests should fail loudly when the matcher chooses a surprising path.

Auto decision D4 — Add required before/after benchmarks
Context: Project rules require performance verification for changes touching `rollshot-core` stitching paths.
ELI10: Expanding search can cost CPU. Even if the code is correct, it might make every difficult frame slower. A before/after benchmark gives evidence instead of relying on intuition.
Stakes if we pick wrong: A correctness fix could ship with an unnoticed matcher/verifier performance regression.
Recommendation: D4A because it satisfies the repo rule and makes the performance tradeoff visible.
Completeness: A=10/10, B=6/10
Pros / cons:
A) Add baseline, after, and compare benchmark steps (recommended)
  ✅ Provides concrete performance evidence for the hot path.
  ❌ Adds runtime to execution.
B) Keep only tests/fmt/clippy
  ✅ Faster execution.
  ❌ Violates the repo's core stitching verification rule.
Net: Benchmarking is required discipline for this crate, not optional polish.

### Test Coverage Table

| Task / behavior | Unit | Integ | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| Task 1 / terminal-like near-page scroll beyond old 85% ceiling appends | — | ✓ | — | no |
| Task 1 / below-`min_overlap` near-page scroll does not append or grow canvas | — | ✓ | — | no |
| Task 2 / normal fast-scroll relaxed recovery still works | — | ✓ | — | no |
| Task 2 / `estimate_motion` still respects high `min_overlap` | ✓ | — | — | no |
| Task 2 / matcher structural search budget remains bounded | ✓ | — | — | no |
| Task 3 / full `rollshot-core` regression suite | ✓ | ✓ | — | no |
| Task 3 / benchmark before/after comparison | — | — | smoke | no |

### NOT in Scope

- macOS event-delta handling: Rollshot stitches frames and does not need scroll-wheel units for this core fix.
- Overlay/UI wording: the requested scope is core stitching only.
- Lowering `StitchConfig::min_overlap`: that weakens verification instead of making large valid offsets searchable.
- New matcher family: existing coarse/template recovery can solve this with a smaller diff.
- Public API/config changes: the behavior can be fixed privately inside `matcher.rs`.

### What Already Exists

- `relaxed_coarse_candidate`: already provides the recovery slot for fast scrolls; the plan widens its search ceiling instead of adding a new stage.
- `PixelOverlapVerifier`: already enforces the overlap floor and pixel agreement; the plan keeps it as the safety gate.
- `rank_verified_candidates`: already filters confidence, second-best ambiguity, axis lock, and verifier results; the plan continues routing relaxed candidates through it.
- `fast_scroll_beyond_default_search_ratio_recovers_via_relaxed_pass`: already covers moderate fast-scroll recovery; the plan keeps and updates it.
- `large_pair_stays_within_structural_search_budget`: already guards matcher search growth; the plan explicitly reruns it.
- `docs/bench.md` workflow and `scripts/bench/compare.py`: already define before/after benchmark collection; the plan reuses them.

### Failure Modes

| New codepath / behavior | Realistic failure | Test covers it | Handling exists | User-visible result |
|---|---|---|---|---|
| Relaxed geometry-derived search | Picks an offset beyond verifiable overlap | Task 1 Step 3 | `PixelOverlapVerifier::InsufficientOverlap` through `rank_verified_candidates` | No append; capture may report stitch miss |
| Relaxed geometry-derived search | Picks wrong repeated terminal-like offset | Task 1 Step 2 strict offset/overlap assertions | `rank_verified_candidates` second-best + verifier gates | No append or test failure before shipping |
| Normal matcher helper refactor | Normal path searches too far and regresses high `min_overlap` behavior | Task 2 Step 7 `estimate_motion_respects_min_overlap` | `normal_search_limits` caps ratio by overlap floor | No silent below-floor append |
| Expanded relaxed candidate pool | Matcher/verifier work grows materially | Task 2 Step 7 budget test and Task 3 Step 4 benchmark compare | Structural budget assertions plus benchmark review | Performance regression blocks handoff |

Critical gaps: none after the D1-D4 edits.

### Worktree / Subagent Parallelization Strategy

Sequential execution, no parallelization opportunity. All tasks touch or depend on `crates/rollshot-core/`, and Task 2 depends on Task 1's failing tests. Task 3 depends on the implementation commits.

### Review Completion Summary

Plan reviewed:           `docs/superpowers/plans/2026-07-05-core-large-scroll-stitching.md`
Tasks in plan:           4
Files Create/Modify:     0 create / 2 modify

- Step 0: Scope Challenge   — accepted as-is
- Architecture Review:        2 issues
- Plan Structure + Code Q:    1 issue
- Test Review:                table produced, 2 gaps fixed
- Performance Review:         1 issue
- NOT in scope:               written
- What already exists:        written
- Failure modes:              0 critical gaps flagged
- Parallelization:            sequential execution, no parallelization opportunity
- Unresolved decisions:       0

Plan is locked in after these auto edits. Run `superpowers:executing-plans` for sequential implementation.
