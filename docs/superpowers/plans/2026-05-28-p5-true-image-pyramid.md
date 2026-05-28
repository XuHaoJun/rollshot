# P5 True Image Pyramid Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an always-on Gaussian image pyramid candidate path to rollshot-core and use it as the default large-motion recovery mechanism.

**Architecture:** `PreparedFrame` gains a lazy pyramid cache built from the existing grayscale buffer. `estimate_motion` adds `pyramid_candidates` between coarse and template candidates, then continues to use the existing ranker and `PixelOverlapVerifier`. P5 initially removes the legacy relaxed coarse recovery path; verification decides whether the removal is valid before the branch is complete.

**Tech Stack:** Rust, `image`, `rayon`, existing `rollshot-core` matcher/tests/bench harness, Python bench compare scripts.

---

## Source Documents

- Spec: `docs/superpowers/specs/2026-05-28-p5-true-image-pyramid-design.md`
- Roadmap source: `docs/stitching-rollshot-optimizations-2.md`
- Current matcher: `crates/rollshot-core/src/matcher.rs`

## Files To Modify

- Modify `crates/rollshot-core/src/types.rs`
  - Add `MatchMethod::Pyramid`.
  - Keep existing enum tests passing after adding the variant.

- Modify `crates/rollshot-core/src/metrics.rs`
  - Add `pyramid_us` and `pyramid_candidates` to `StitchMetrics`.
  - Keep defaults at zero.
  - Update metrics default test.

- Modify `crates/rollshot-core/src/matcher.rs`
  - Add pyramid constants, data structures, construction helpers, search helpers, and tests.
  - Extend `PreparedFrame` with a lazy pyramid cache.
  - Add `pyramid_candidates` to the regular matcher path.
  - Remove `relaxed_coarse_candidate` after pyramid recovery tests pass.

- Modify `crates/rollshot-core/benches/stitch_sequences.rs`
  - Emit pyramid metrics in frame JSONL.
  - Teach `match_method_str` about `MatchMethod::Pyramid`.

- Modify `scripts/bench/summarize.py`
  - Include `p50_pyramid_us` in aggregation and stage table.

- Modify `scripts/bench/compare.py`
  - Include `p50_pyramid_us` in aggregation and regression reporting.

- Modify `scripts/bench/test_summarize.py`
  - Add `pyramid_us` to synthetic frame records used by summarize/compare tests.

- Modify `crates/rollshot-core/tests/metrics_population.rs`
  - Assert first-frame/duplicate skipped-path pyramid metrics stay zero.
  - Allow appended matcher work to include pyramid timing/candidates.

- Modify `crates/rollshot-core/tests/stitcher.rs`
  - Rename the relaxed-coarse fast-scroll test to pyramid recovery and assert `MatchMethod::Pyramid` when relaxed coarse is removed.

## Task 1: Capture Before Benchmark JSONL

**Files:**
- Create: `bench-results/runs/p5-pyramid/before.jsonl`

- [ ] **Step 1: Capture the before benchmark before code changes**

Run:

```bash
rtk mkdir -p bench-results/runs/p5-pyramid
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --out bench-results/runs/p5-pyramid/before.jsonl
```

Expected:

```text
bench-results/runs/p5-pyramid/before.jsonl
```

exists and contains JSONL records with `kind:"frame"` and `kind:"summary"`.

- [ ] **Step 2: Verify the file is populated**

Run:

```bash
rtk test -s bench-results/runs/p5-pyramid/before.jsonl
rtk python3 scripts/bench/summarize.py bench-results/runs/p5-pyramid/before.jsonl
```

Expected: command exits successfully and prints a bench summary.

- [ ] **Step 3: Commit the before benchmark artifact only if this repository normally tracks bench run outputs**

Check:

```bash
rtk git check-ignore -v bench-results/runs/p5-pyramid/before.jsonl
```

Expected: if the file is ignored, do not stage it. If the file is not ignored and prior bench runs are tracked in this repo, stage it with later implementation commits.

## Task 2: Add Public Method And Metrics Plumbing

**Files:**
- Modify: `crates/rollshot-core/src/types.rs`
- Modify: `crates/rollshot-core/src/metrics.rs`
- Modify: `crates/rollshot-core/benches/stitch_sequences.rs`
- Modify: `scripts/bench/summarize.py`
- Modify: `scripts/bench/compare.py`
- Modify: `scripts/bench/test_summarize.py`
- Modify: `crates/rollshot-core/tests/metrics_population.rs`

- [ ] **Step 1: Write failing metrics/default tests**

In `crates/rollshot-core/src/metrics.rs`, update `metrics_default_is_zero`:

```rust
#[test]
fn metrics_default_is_zero() {
    let m = StitchMetrics::default();
    assert_eq!(m.total_us, 0);
    assert_eq!(m.outcome, StitchOutcomeKind::None);
    assert!(m.no_match_reason.is_none());
    assert_eq!(m.coarse_us, 0);
    assert_eq!(m.pyramid_us, 0);
    assert_eq!(m.pyramid_candidates, 0);
    assert_eq!(m.append_us, 0);
}
```

In `crates/rollshot-core/tests/metrics_population.rs`, update skipped-stage assertions:

```rust
assert_eq!(m.coarse_us, 0);
assert_eq!(m.pyramid_us, 0);
assert_eq!(m.pyramid_candidates, 0);
assert_eq!(m.template_ncc_us, 0);
```

Update the appended matcher assertion:

```rust
assert!(
    m.coarse_us > 0 || m.pyramid_us > 0 || m.template_ncc_us > 0,
    "matcher stages should record some time"
);
assert!(
    m.coarse_candidates > 0 || m.pyramid_candidates > 0 || m.ncc_offsets_scored > 0
);
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-core metrics_default_is_zero first_frame_outcome_populates_minimal_fields appended_outcome_populates_all_stages duplicate_outcome_populates_only_duplicate_stage
```

Expected: FAIL with missing `pyramid_us` / `pyramid_candidates` fields.

- [ ] **Step 3: Add the metrics fields**

In `crates/rollshot-core/src/metrics.rs`, add fields to `StitchMetrics`:

```rust
pub pyramid_us: u64,
```

immediately after `coarse_us`, and:

```rust
pub pyramid_candidates: usize,
```

immediately after `coarse_candidates`.

- [ ] **Step 4: Add `MatchMethod::Pyramid`**

In `crates/rollshot-core/src/types.rs`, update `MatchMethod`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MatchMethod {
    Template,
    Coarse,
    Pyramid,
    Edge,
    /// FAST corners + linear KNN matching. The "Hnsw" in the name is
    /// reserved for a future ANN upgrade — see
    /// docs/superpowers/specs/2026-05-23-rollshot-fast-hnsw-fallback-design.md
    /// Approach A. Current matching is exact linear scan.
    FastHnsw,
}
```

- [ ] **Step 5: Update bench JSONL records**

In `crates/rollshot-core/benches/stitch_sequences.rs`, add fields to `FrameRecord`:

```rust
pyramid_us: u64,
pyramid_candidates: usize,
```

Place `pyramid_us` after `coarse_us` and `pyramid_candidates` after `coarse_candidates`.

Update `match_method_str`:

```rust
fn match_method_str(method: MatchMethod) -> &'static str {
    match method {
        MatchMethod::Template => "Template",
        MatchMethod::Coarse => "Coarse",
        MatchMethod::Pyramid => "Pyramid",
        MatchMethod::Edge => "Edge",
        MatchMethod::FastHnsw => "FastHnsw",
        _ => "Unknown",
    }
}
```

Update `make_frame_record`:

```rust
pyramid_us: metrics.pyramid_us,
```

after `coarse_us`, and:

```rust
pyramid_candidates: metrics.pyramid_candidates,
```

after `coarse_candidates`.

- [ ] **Step 6: Update bench summary and compare scripts**

In `scripts/bench/summarize.py`, add to aggregation:

```python
"p50_pyramid_us": quantile([r.get("pyramid_us", 0) for r in recs], 0.50),
```

after `p50_coarse_us`.

Change the stage table header:

```python
"| scenario | prepare | coarse | pyramid | ncc | edge | verifier | fallback | append |"
```

Change the separator:

```python
"|---|---:|---:|---:|---:|---:|---:|---:|---:|"
```

Change the row format to include pyramid:

```python
f"| {scn} | {m['p50_prepare_us']:,} | {m['p50_coarse_us']:,} | "
f"{m['p50_pyramid_us']:,} | {m['p50_ncc_us']:,} | {m['p50_edge_us']:,} | "
f"{m['p50_verifier_us']:,} | {m['p50_fallback_us']:,} | {m['p50_append_us']:,} |"
```

In `scripts/bench/compare.py`, add to aggregation:

```python
"p50_pyramid_us": quantile([r.get("pyramid_us", 0) for r in recs], 0.50),
```

after `p50_coarse_us`.

Add a regression section near the existing coarse/NCC sections:

```python
all_regressions.extend(section("Pyramid (p50)", "p50_pyramid_us", "p50 pyramid"))
```

- [ ] **Step 7: Update Python bench script tests**

In `scripts/bench/test_summarize.py`, update `_frame_record` to include:

```python
"pyramid_us": 0,
"pyramid_candidates": 0,
```

Set `pyramid_us` to a nonzero value in one summarize test record:

```python
"pyramid_us": 25,
```

Assert the rendered output contains the new column:

```python
assert "| scenario | prepare | coarse | pyramid | ncc | edge | verifier | fallback | append |" in result
```

- [ ] **Step 8: Run focused verification**

Run:

```bash
rtk cargo test -p rollshot-core metrics_default_is_zero first_frame_outcome_populates_minimal_fields appended_outcome_populates_all_stages duplicate_outcome_populates_only_duplicate_stage
rtk python3 -m pytest scripts/bench/test_summarize.py
```

Expected: PASS.

- [ ] **Step 9: Commit metrics plumbing**

Run:

```bash
rtk git add crates/rollshot-core/src/types.rs crates/rollshot-core/src/metrics.rs crates/rollshot-core/benches/stitch_sequences.rs scripts/bench/summarize.py scripts/bench/compare.py scripts/bench/test_summarize.py crates/rollshot-core/tests/metrics_population.rs
rtk git commit -m "feat(core): add pyramid metrics plumbing"
```

## Task 3: Add Pyramid Construction

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`

- [ ] **Step 1: Write failing construction tests**

In `crates/rollshot-core/src/matcher.rs`, add these names to the test module `use super::{ ... }` list:

```rust
build_frame_pyramid, gaussian_downsample_2x, FramePyramid, PyramidLevel,
PYRAMID_MAX_LEVELS, PYRAMID_MIN_LEVEL_SIDE,
```

Add tests:

```rust
#[test]
fn pyramid_downsample_dimensions_are_correct() {
    let gray = vec![10.0; 5 * 3];
    let down = gaussian_downsample_2x(&gray, 5, 3);
    assert_eq!(down.width, 3);
    assert_eq!(down.height, 2);
    assert_eq!(down.gray.len(), 6);
}

#[test]
fn pyramid_gaussian_downsample_is_deterministic() {
    let gray = vec![
        0.0, 10.0, 20.0, 30.0, 40.0,
        50.0, 60.0, 70.0, 80.0, 90.0,
        100.0, 110.0, 120.0, 130.0, 140.0,
        150.0, 160.0, 170.0, 180.0, 190.0,
        200.0, 210.0, 220.0, 230.0, 240.0,
    ];
    let a = gaussian_downsample_2x(&gray, 5, 5);
    let b = gaussian_downsample_2x(&gray, 5, 5);
    assert_eq!(a.width, 3);
    assert_eq!(a.height, 3);
    assert_eq!(a.gray, b.gray);
    assert!(
        a.gray.iter().all(|v| v.is_finite() && (0.0..=240.0).contains(v)),
        "downsampled values should stay in input luminance range: {:?}",
        a.gray
    );
}

#[test]
fn frame_pyramid_respects_level_limits() {
    let img = make_textured_canvas(640, 480);
    let prep = PreparedFrame::new(img);
    let pyramid = build_frame_pyramid(prep.gray(), 640, 480);
    assert!(!pyramid.levels.is_empty());
    assert!(pyramid.levels.len() <= PYRAMID_MAX_LEVELS as usize);
    assert_eq!(pyramid.levels[0].scale_log2, 0);
    assert_eq!(pyramid.levels[0].width, 640);
    assert_eq!(pyramid.levels[0].height, 480);
    for level in pyramid.levels.iter().skip(1) {
        assert!(
            level.width >= PYRAMID_MIN_LEVEL_SIDE || level.height >= PYRAMID_MIN_LEVEL_SIDE,
            "last constructed level can approach the side threshold, got {}x{}",
            level.width,
            level.height
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-core pyramid_downsample_dimensions_are_correct pyramid_gaussian_downsample_is_deterministic frame_pyramid_respects_level_limits
```

Expected: FAIL with missing pyramid symbols.

- [ ] **Step 3: Add constants and data structures**

In `crates/rollshot-core/src/matcher.rs`, near existing matcher constants, add:

```rust
const PYRAMID_MAX_LEVELS: u8 = 4;
const PYRAMID_MIN_LEVEL_SIDE: u32 = 96;
const PYRAMID_KERNEL: [f32; 5] = [1.0, 4.0, 6.0, 4.0, 1.0];
const PYRAMID_KERNEL_SUM: f32 = 16.0;

#[derive(Debug, Clone)]
struct PyramidLevel {
    scale_log2: u8,
    width: u32,
    height: u32,
    gray: Vec<f32>,
}

#[derive(Debug, Clone)]
struct FramePyramid {
    levels: Vec<PyramidLevel>,
}
```

- [ ] **Step 4: Add Gaussian downsample helpers**

Add below `coarse_samples`:

```rust
fn clamp_coord(value: i32, max_exclusive: u32) -> u32 {
    value.clamp(0, max_exclusive.saturating_sub(1) as i32) as u32
}

fn gaussian_downsample_2x(gray: &[f32], width: u32, height: u32) -> PyramidLevel {
    let out_w = width.div_ceil(2).max(1);
    let out_h = height.div_ceil(2).max(1);
    let mut blurred_h = vec![0.0f32; (width * height) as usize];

    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0f32;
            for (k, weight) in PYRAMID_KERNEL.iter().enumerate() {
                let xx = clamp_coord(x as i32 + k as i32 - 2, width);
                sum += gray[(y * width + xx) as usize] * *weight;
            }
            blurred_h[(y * width + x) as usize] = sum / PYRAMID_KERNEL_SUM;
        }
    }

    let mut out = Vec::with_capacity((out_w * out_h) as usize);
    for oy in 0..out_h {
        let src_y = (oy * 2).min(height - 1);
        for ox in 0..out_w {
            let src_x = (ox * 2).min(width - 1);
            let mut sum = 0.0f32;
            for (k, weight) in PYRAMID_KERNEL.iter().enumerate() {
                let yy = clamp_coord(src_y as i32 + k as i32 - 2, height);
                sum += blurred_h[(yy * width + src_x) as usize] * *weight;
            }
            out.push(sum / PYRAMID_KERNEL_SUM);
        }
    }

    PyramidLevel {
        scale_log2: 0,
        width: out_w,
        height: out_h,
        gray: out,
    }
}

fn build_frame_pyramid(gray: &[f32], width: u32, height: u32) -> FramePyramid {
    let mut levels = vec![PyramidLevel {
        scale_log2: 0,
        width,
        height,
        gray: gray.to_vec(),
    }];

    while levels.len() < PYRAMID_MAX_LEVELS as usize {
        let prev = levels.last().expect("pyramid always has level 0");
        if prev.width <= PYRAMID_MIN_LEVEL_SIDE || prev.height <= PYRAMID_MIN_LEVEL_SIDE {
            break;
        }

        let mut next = gaussian_downsample_2x(&prev.gray, prev.width, prev.height);
        next.scale_log2 = prev.scale_log2 + 1;
        levels.push(next);
    }

    FramePyramid { levels }
}
```

- [ ] **Step 5: Extend `PreparedFrame`**

Add a field:

```rust
pyramid: OnceLock<FramePyramid>,
```

Initialize it in `PreparedFrame::from_parts`:

```rust
pyramid: OnceLock::new(),
```

Add a method:

```rust
fn pyramid(&self) -> &FramePyramid {
    self.pyramid
        .get_or_init(|| build_frame_pyramid(&self.gray, self.width, self.height))
}
```

- [ ] **Step 6: Run focused construction tests**

Run:

```bash
rtk cargo test -p rollshot-core pyramid_downsample_dimensions_are_correct pyramid_gaussian_downsample_is_deterministic frame_pyramid_respects_level_limits prepared_frame_coarse_matches_old_coarse_samples prepared_frame_projection_matches_old_edge_projection
```

Expected: PASS.

- [ ] **Step 7: Commit pyramid construction**

Run:

```bash
rtk git add crates/rollshot-core/src/matcher.rs
rtk git commit -m "feat(core): add lazy frame pyramid construction"
```

## Task 4: Add Pyramid Candidate Search

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`

- [ ] **Step 1: Write failing search tests**

Add these names to the test module import list:

```rust
pyramid_axis_candidate, pyramid_candidates_for_axes, pyramid_mad,
PYRAMID_REFINE_RADIUS,
```

Add tests:

```rust
#[test]
fn pyramid_large_jump_finds_correct_candidate() {
    let canvas = make_aperiodic_canvas(480, 1200);
    let prev = crop(&canvas, 0, 320);
    let curr = crop(&canvas, 210, 320);
    let prev_prep = PreparedFrame::new(prev);
    let curr_prep = PreparedFrame::new(curr);
    let config = StitchConfig::default();
    let mut metrics = StitchMetrics::default();

    let candidates = pyramid_candidates_for_axes(
        &prev_prep,
        &curr_prep,
        None,
        &[SearchAxis::Vertical],
        &config,
        &mut metrics,
    );

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].dx, 0);
    assert!(
        (candidates[0].dy - 210).abs() <= PYRAMID_REFINE_RADIUS + 2,
        "dy = {} (expected around 210)",
        candidates[0].dy
    );
    assert_eq!(candidates[0].method, crate::types::MatchMethod::Pyramid);
    assert!(candidates[0].score <= config.accept_confidence);
    assert!(metrics.pyramid_us > 0);
    assert_eq!(metrics.pyramid_candidates, 1);
}

#[test]
fn pyramid_score_contract_matches_ranker() {
    let canvas = make_aperiodic_canvas(360, 900);
    let prev = crop(&canvas, 0, 240);
    let curr = crop(&canvas, 96, 240);
    let prev_prep = PreparedFrame::new(prev);
    let curr_prep = PreparedFrame::new(curr);
    let config = StitchConfig::default();
    let mut metrics = StitchMetrics::default();

    let candidate = pyramid_candidates_for_axes(
        &prev_prep,
        &curr_prep,
        None,
        &[SearchAxis::Vertical],
        &config,
        &mut metrics,
    )
    .pop()
    .expect("pyramid candidate");

    assert!(candidate.score >= 0.0);
    assert!(candidate.score <= 1.0);
    if let Some(second) = candidate.second_best_score {
        assert!(second >= 0.0);
        assert!(second <= 1.0);
        assert!(second >= candidate.score);
    }
}

#[test]
fn pyramid_does_not_accept_repeated_grid_alias() {
    let canvas = make_repeated_grid(256, 640);
    let prev = crop_xy(&canvas, 0, 0, 192, 192);
    let curr = crop_xy(&canvas, 0, 48, 192, 192);
    let config = StitchConfig::default();

    let outcome = estimate_motion(
        &prep(&prev),
        &prep(&curr),
        None,
        (0, 0),
        &config,
        &mut StitchMetrics::default(),
    );

    assert!(
        matches!(outcome, MotionSearchOutcome::NoMatch { .. }),
        "repeated grid must not be accepted through pyramid: {outcome:?}"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-core pyramid_large_jump_finds_correct_candidate pyramid_score_contract_matches_ranker pyramid_does_not_accept_repeated_grid_alias
```

Expected: FAIL with missing pyramid search helpers.

- [ ] **Step 3: Add pyramid search constants**

Near the pyramid constants:

```rust
const PYRAMID_REFINE_RADIUS: i32 = 4;
```

- [ ] **Step 4: Add pyramid MAD and scoring helpers**

Add below `coarse_mad`:

```rust
fn pyramid_mad(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    dx: i32,
    dy: i32,
) -> f32 {
    coarse_mad(prev_gray, curr_gray, width, height, dx, dy, 1)
}

fn pyramid_confidence(raw_mad: f32) -> f32 {
    raw_mad.clamp(0.0, 1.0)
}
```

This works because `coarse_mad` already divides by `255.0`.

- [ ] **Step 5: Add pyramid axis search helpers**

Add below `coarse_axis_candidate`:

```rust
fn pyramid_axis_candidate(
    prev: &FramePyramid,
    curr: &FramePyramid,
    axis: SearchAxis,
    config: &StitchConfig,
) -> Option<MotionCandidate> {
    let top_idx = prev.levels.len().min(curr.levels.len()).checked_sub(1)?;
    let top = &prev.levels[top_idx];
    let curr_top = &curr.levels[top_idx];
    if top.width != curr_top.width || top.height != curr_top.height {
        return None;
    }

    let max_offset = match axis {
        SearchAxis::Vertical => top.height.saturating_sub(config.min_overlap >> top.scale_log2) as i32,
        SearchAxis::Horizontal => top.width.saturating_sub(config.min_overlap >> top.scale_log2) as i32,
    }
    .max(0);
    if max_offset <= 0 {
        return None;
    }

    let mut offset = pyramid_best_offset_at_level(top, curr_top, axis, 0, max_offset)?;

    for level_idx in (0..top_idx).rev() {
        let level = &prev.levels[level_idx];
        let curr_level = &curr.levels[level_idx];
        let max_offset = match axis {
            SearchAxis::Vertical => level.height.saturating_sub(config.min_overlap >> level.scale_log2) as i32,
            SearchAxis::Horizontal => level.width.saturating_sub(config.min_overlap >> level.scale_log2) as i32,
        }
        .max(0);
        offset = (offset * 2).clamp(-max_offset, max_offset);
        offset = pyramid_best_offset_at_level(
            level,
            curr_level,
            axis,
            offset,
            PYRAMID_REFINE_RADIUS.min(max_offset),
        )?;
    }

    let full = &prev.levels[0];
    let curr_full = &curr.levels[0];
    let offsets = refinement_offsets(offset, match axis {
        SearchAxis::Vertical => full.height.saturating_sub(config.min_overlap) as i32,
        SearchAxis::Horizontal => full.width.saturating_sub(config.min_overlap) as i32,
    }, PYRAMID_REFINE_RADIUS);

    let mut scored = Vec::new();
    for refined in offsets {
        let (dx, dy) = match axis {
            SearchAxis::Vertical => (0, refined),
            SearchAxis::Horizontal => (refined, 0),
        };
        let score = pyramid_mad(&full.gray, &curr_full.gray, full.width, full.height, dx, dy);
        if score.is_finite() {
            scored.push((score, dx, dy));
        }
    }
    scored.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let (best, dx, dy) = *scored.first()?;
    let second = scored.get(1).map(|(score, _, _)| pyramid_confidence(*score));

    Some(candidate(
        dx,
        dy,
        MatchMethod::Pyramid,
        pyramid_confidence(best),
        second,
    ))
}

fn pyramid_best_offset_at_level(
    prev: &PyramidLevel,
    curr: &PyramidLevel,
    axis: SearchAxis,
    seed: i32,
    radius_or_max: i32,
) -> Option<i32> {
    let max_abs = match axis {
        SearchAxis::Vertical => prev.height.saturating_sub(1) as i32,
        SearchAxis::Horizontal => prev.width.saturating_sub(1) as i32,
    };
    let offsets = if seed == 0 {
        signed_predict_iter(radius_or_max.min(max_abs), 0)
    } else {
        refinement_offsets(seed, max_abs, radius_or_max)
    };

    offsets
        .into_iter()
        .filter(|offset| *offset != 0)
        .filter_map(|offset| {
            let (dx, dy) = match axis {
                SearchAxis::Vertical => (0, offset),
                SearchAxis::Horizontal => (offset, 0),
            };
            let score = pyramid_mad(&prev.gray, &curr.gray, prev.width, prev.height, dx, dy);
            score.is_finite().then_some((score, offset))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(_, offset)| offset)
}
```

After adding this code, clean up the duplicated max-offset expressions into a helper:

```rust
fn pyramid_max_axis_offset(level: &PyramidLevel, axis: SearchAxis, min_overlap: u32) -> i32 {
    let scaled_min_overlap = (min_overlap >> level.scale_log2).max(1);
    match axis {
        SearchAxis::Vertical => level.height.saturating_sub(scaled_min_overlap) as i32,
        SearchAxis::Horizontal => level.width.saturating_sub(scaled_min_overlap) as i32,
    }
}
```

- [ ] **Step 6: Add candidate collection helpers**

Add:

```rust
fn pyramid_candidates(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<ScrollAxis>,
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> Vec<MotionCandidate> {
    let locked_axes;
    let axes = match locked_axis {
        Some(axis) => {
            locked_axes = [SearchAxis::from_scroll_axis(axis)];
            &locked_axes[..]
        }
        None => dual_search_axes(),
    };
    pyramid_candidates_for_axes(prev, curr, locked_axis, axes, config, metrics)
}

fn pyramid_candidates_for_axes(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<ScrollAxis>,
    axes: &[SearchAxis],
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> Vec<MotionCandidate> {
    let _timer = ScopedTimer::new(&mut metrics.pyramid_us);
    let prev_pyramid = prev.pyramid();
    let curr_pyramid = curr.pyramid();
    let out: Vec<_> = axes
        .iter()
        .filter_map(|axis| pyramid_axis_candidate(prev_pyramid, curr_pyramid, *axis, config))
        .filter(|candidate| candidate_matches_axis(candidate.dx, candidate.dy, locked_axis, config))
        .collect();
    metrics.pyramid_candidates = metrics.pyramid_candidates.max(out.len());
    out
}
```

- [ ] **Step 7: Run focused search tests**

Run:

```bash
rtk cargo test -p rollshot-core pyramid_large_jump_finds_correct_candidate pyramid_score_contract_matches_ranker pyramid_does_not_accept_repeated_grid_alias
```

Expected: PASS. If `pyramid_does_not_accept_repeated_grid_alias` fails because pyramid is not yet integrated into `estimate_motion`, keep the test in place and complete Task 5 before re-running it.

- [ ] **Step 8: Commit pyramid candidate search**

Run:

```bash
rtk git add crates/rollshot-core/src/matcher.rs
rtk git commit -m "feat(core): add pyramid motion candidates"
```

## Task 5: Integrate Pyramid Into Matcher

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`
- Modify: `crates/rollshot-core/tests/stitcher.rs`

- [ ] **Step 1: Write failing integration tests**

In `crates/rollshot-core/src/matcher.rs`, add:

```rust
#[test]
fn pyramid_candidate_passes_existing_verifier() {
    let canvas = make_aperiodic_canvas(420, 1000);
    let prev = crop(&canvas, 0, 280);
    let curr = crop(&canvas, 170, 280);
    let config = StitchConfig::default();
    let mut metrics = StitchMetrics::default();

    let candidate = unwrap_candidate(estimate_motion(
        &prep(&prev),
        &prep(&curr),
        None,
        (0, 0),
        &config,
        &mut metrics,
    ));

    assert_eq!(candidate.method, crate::types::MatchMethod::Pyramid);
    assert_eq!(candidate.dx, 0);
    assert!(
        (candidate.dy - 170).abs() <= 3,
        "dy = {} (expected around 170)",
        candidate.dy
    );
    assert!(metrics.pyramid_us > 0);
    assert!(metrics.pyramid_candidates > 0);
}
```

In `crates/rollshot-core/tests/stitcher.rs`, replace `fast_scroll_beyond_default_search_ratio_recovers_via_relaxed_pass` with:

```rust
#[test]
fn fast_scroll_beyond_default_search_ratio_recovers_via_pyramid() {
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
            assert_eq!(estimate.method, MatchMethod::Pyramid);
            assert!(
                (192..=208).contains(&added),
                "added = {added} (expected ~200 via pyramid)"
            );
        }
        other => panic!("expected Appended via pyramid, got {other:?}"),
    }
}
```

Ensure the test imports `MatchMethod`.

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-core pyramid_candidate_passes_existing_verifier fast_scroll_beyond_default_search_ratio_recovers_via_pyramid
```

Expected: FAIL because `estimate_motion` does not use pyramid candidates yet.

- [ ] **Step 3: Insert pyramid candidates into `estimate_motion` regular path**

In `estimate_motion`, after coarse candidates are collected:

```rust
let pyramid = pyramid_candidates(prev, curr, locked_axis, config, metrics);
candidates.extend(pyramid.iter().copied());
```

The regular path should read:

```rust
let mut candidates = Vec::new();
let coarse = {
    let _t = ScopedTimer::new(&mut metrics.coarse_us);
    coarse_candidates(prev, curr, locked_axis, config)
};
metrics.coarse_candidates = coarse.len();
candidates.extend(coarse.iter().copied());
let pyramid = pyramid_candidates(prev, curr, locked_axis, config, metrics);
candidates.extend(pyramid.iter().copied());
let template_start = std::time::Instant::now();
let template_result = template_candidates(
    prev,
    curr,
    locked_axis,
    last_motion,
    &coarse,
    &pyramid,
    config,
    metrics,
);
```

- [ ] **Step 4: Update template seed signatures**

Change:

```rust
fn template_seed(axis: SearchAxis, last_motion: (i32, i32), coarse: &[MotionCandidate]) -> i32
```

to:

```rust
fn template_seed(
    axis: SearchAxis,
    last_motion: (i32, i32),
    coarse: &[MotionCandidate],
    pyramid: &[MotionCandidate],
) -> i32
```

Use this body:

```rust
let predicted = predicted_offset(axis, last_motion);
if predicted != 0 {
    return predicted;
}

pyramid
    .iter()
    .chain(coarse.iter())
    .find_map(|candidate| match axis {
        SearchAxis::Vertical if candidate.dx == 0 => Some(candidate.dy),
        SearchAxis::Horizontal if candidate.dy == 0 => Some(candidate.dx),
        _ => None,
    })
    .unwrap_or(predicted)
```

Update `template_candidates` and `template_candidates_for_axes` to accept `pyramid: &[MotionCandidate]` and pass it to `template_seed`.

- [ ] **Step 5: Keep axis fast path unchanged**

Do not add pyramid to `axis_fast_path_candidate`. The axis fast path is for steady locked-axis frames and should stay cheaper than the full regular path. Suspicious fast-path frames still fall back to the regular path, where pyramid runs.

- [ ] **Step 6: Run focused integration tests**

Run:

```bash
rtk cargo test -p rollshot-core pyramid_candidate_passes_existing_verifier fast_scroll_beyond_default_search_ratio_recovers_via_pyramid repeated_grid_is_rejected_by_second_best_margin repeated_rows_do_not_append_without_clear_match
```

Expected: PASS.

- [ ] **Step 7: Commit matcher integration**

Run:

```bash
rtk git add crates/rollshot-core/src/matcher.rs crates/rollshot-core/tests/stitcher.rs
rtk git commit -m "feat(core): use pyramid candidates in matcher"
```

## Task 6: Remove Relaxed Coarse

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`
- Modify: `crates/rollshot-core/tests/stitcher.rs`

- [ ] **Step 1: Remove relaxed coarse from `estimate_motion`**

Delete this block from `estimate_motion`:

```rust
if let Some(candidate) =
    relaxed_coarse_candidate(prev, curr, locked_axis, last_motion, config, metrics)
{
    return MotionSearchOutcome::Candidate(candidate);
}
```

- [ ] **Step 2: Delete the relaxed coarse helper**

Delete:

```rust
const RELAXED_SEARCH_RATIO: f32 = 0.85;
```

and the full `relaxed_coarse_candidate` function.

- [ ] **Step 3: Remove stale comments that describe relaxed coarse as a recovery path**

Search:

```bash
rtk rg -n "relaxed|RELAXED_SEARCH_RATIO|relaxed_coarse" crates/rollshot-core/src/matcher.rs crates/rollshot-core/tests/stitcher.rs docs/superpowers/plans/2026-05-28-p5-true-image-pyramid.md
```

Expected after source edits: only this plan mentions relaxed coarse in the searched files.

- [ ] **Step 4: Run relaxed-coarse replacement tests**

Run:

```bash
rtk cargo test -p rollshot-core fast_scroll_beyond_default_search_ratio_recovers_via_pyramid pyramid_candidate_passes_existing_verifier estimate_motion_finds_known_scroll estimate_motion_finds_vertical_up_scroll estimate_motion_finds_horizontal_right_scroll estimate_motion_finds_horizontal_left_scroll
```

Expected: PASS.

- [ ] **Step 5: Run ambiguity and low-feature guards**

Run:

```bash
rtk cargo test -p rollshot-core repeated_grid_is_rejected_by_second_best_margin repeated_rows_do_not_append_without_clear_match fast_hnsw_fallback_recovers_repeated_grid_with_sparse_features
rtk cargo test -p rollshot-core --test golden_fixtures
```

Expected: PASS.

- [ ] **Step 6: Stop for design review if pyramid cannot replace relaxed coarse**

If Step 4 or Step 5 fails only because a specific large-motion fixture no longer recovers, stop execution and report:

```text
P5 pyramid did not cover relaxed coarse replacement.
Include the exact failing command from Step 4 or Step 5.
Include the exact failing test or fixture name from output.
Include the relevant failure line showing the observed candidate or outcome.
```

Do not restore relaxed coarse in this task. The approved spec allows temporary
retention only with named evidence, so that retention needs a short plan/spec
amendment before source code changes continue.

- [ ] **Step 7: Commit relaxed coarse decision**

After Step 4 and Step 5 pass with relaxed coarse removed:

```bash
rtk git add crates/rollshot-core/src/matcher.rs crates/rollshot-core/tests/stitcher.rs
rtk git commit -m "refactor(core): replace relaxed coarse with pyramid recovery"
```

## Task 7: Update Structural Budgets And Bench Scripts

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`
- Modify: `scripts/bench/compare.py`
- Modify: `scripts/bench/summarize.py`
- Modify: `scripts/bench/test_summarize.py`

- [ ] **Step 1: Run current structural budget test**

Run:

```bash
rtk cargo test -p rollshot-core large_pair_stays_within_structural_search_budget -- --nocapture
```

Expected: PASS or FAIL with printed budget values.

- [ ] **Step 2: Update budget assertions to include pyramid work**

In `large_pair_stays_within_structural_search_budget`, keep these existing bounds:

```rust
assert!(
    budget.coarse_score_calls <= 4096,
    "coarse_score_calls = {}",
    budget.coarse_score_calls
);
assert!(
    budget.full_res_ncc_calls <= 768,
    "full_res_ncc_calls = {}",
    budget.full_res_ncc_calls
);
assert!(
    budget.full_res_ncc_pixel_visits <= 200_000_000,
    "full_res_ncc_pixel_visits = {}",
    budget.full_res_ncc_pixel_visits
);
```

Do not add a pyramid-specific budget counter unless Task 4 showed pyramid scoring is structurally unbounded. Pyramid top-level full-range search is acceptable because it runs on downsampled levels.

- [ ] **Step 3: Verify bench scripts still pass**

Run:

```bash
rtk python3 -m pytest scripts/bench/test_summarize.py
```

Expected: PASS.

- [ ] **Step 4: Commit budget/script adjustments**

Run:

```bash
rtk git add crates/rollshot-core/src/matcher.rs scripts/bench/compare.py scripts/bench/summarize.py scripts/bench/test_summarize.py
rtk git commit -m "test(core): cover pyramid structural budgets"
```

## Task 8: Full Verification And After Benchmark

**Files:**
- Create: `bench-results/runs/p5-pyramid/after.jsonl`
- Read: `bench-results/runs/p5-pyramid/before.jsonl`

- [ ] **Step 1: Run rollshot-core tests**

Run:

```bash
rtk cargo test -p rollshot-core
```

Expected: PASS.

- [ ] **Step 2: Run Rust formatting**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 3: Run clippy**

Run:

```bash
rtk cargo clippy -p rollshot-core --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Capture after benchmark**

Run:

```bash
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --out bench-results/runs/p5-pyramid/after.jsonl
```

Expected:

```text
bench-results/runs/p5-pyramid/after.jsonl
```

exists and contains JSONL records with pyramid fields in frame rows.

- [ ] **Step 5: Compare before and after**

Run:

```bash
rtk python3 scripts/bench/compare.py \
    bench-results/runs/p5-pyramid/before.jsonl \
    bench-results/runs/p5-pyramid/after.jsonl
```

Expected: comparison report completes. Review output for:

- no increase in wrong appends;
- no golden correctness regression;
- fast-scroll scenarios still append;
- p50/p95 total latency does not materially regress on steady scroll;
- pyramid time is visible as a separate stage.

- [ ] **Step 6: Inspect changed files**

Run:

```bash
rtk git status --short
rtk git diff --stat
```

Expected: only planned files and benchmark outputs changed.

- [ ] **Step 7: Final implementation commit**

Run:

```bash
rtk git add crates/rollshot-core/src/types.rs crates/rollshot-core/src/metrics.rs crates/rollshot-core/src/matcher.rs crates/rollshot-core/benches/stitch_sequences.rs crates/rollshot-core/tests/metrics_population.rs crates/rollshot-core/tests/stitcher.rs scripts/bench/summarize.py scripts/bench/compare.py scripts/bench/test_summarize.py
rtk git commit -m "perf(core): add true image pyramid matching"
```

If there is nothing left to commit because earlier tasks committed every change, skip this step and record the final commit list in the handoff.

## Completion Handoff

When all tasks are complete, report:

- final commit hashes;
- whether `relaxed_coarse_candidate` was removed;
- before/after JSONL paths;
- compare report path or summary;
- verification command results.
