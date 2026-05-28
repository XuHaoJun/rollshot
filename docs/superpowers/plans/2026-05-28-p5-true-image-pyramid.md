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
    let mut scratch = vec![0.0f32; 5 * 3];
    let down = gaussian_downsample_2x(&gray, 5, 3, &mut scratch);
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
    let mut scratch_a = vec![0.0f32; 25];
    let mut scratch_b = vec![0.0f32; 25];
    let a = gaussian_downsample_2x(&gray, 5, 5, &mut scratch_a);
    let b = gaussian_downsample_2x(&gray, 5, 5, &mut scratch_b);
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
    // `FramePyramid` stores ONLY downsampled levels 1..N. Level 0 is the
    // caller's grayscale buffer, referenced (not cloned) by the search code.
    assert!(
        !pyramid.levels.is_empty(),
        "640x480 must produce at least one downsampled level"
    );
    assert!(pyramid.levels.len() <= (PYRAMID_MAX_LEVELS as usize - 1));
    // First cached level is scale_log2=1, i.e. half-resolution.
    assert_eq!(pyramid.levels[0].scale_log2, 1);
    assert_eq!(pyramid.levels[0].width, 320);
    assert_eq!(pyramid.levels[0].height, 240);
    for (i, level) in pyramid.levels.iter().enumerate() {
        assert_eq!(level.scale_log2, (i + 1) as u8);
    }
}

#[test]
fn prepared_frame_pyramid_is_cached() {
    let img = make_textured_canvas(640, 480);
    let prep = PreparedFrame::new(img);
    let first = prep.pyramid() as *const FramePyramid;
    let second = prep.pyramid() as *const FramePyramid;
    assert_eq!(first, second, "pyramid OnceLock must return the same instance");
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

// Separable Gaussian downsample. `scratch` must hold at least `width * height`
// f32s; the caller reuses it across pyramid levels so we don't reallocate
// ~33 MB per level at 4K. Rows of the horizontal blur and decimation are
// embarrassingly parallel — we use rayon over rows the same way
// `coarse_axis_candidate` parallelizes over offsets.
fn gaussian_downsample_2x(
    gray: &[f32],
    width: u32,
    height: u32,
    scratch: &mut [f32],
) -> PyramidLevel {
    let out_w = width.div_ceil(2).max(1);
    let out_h = height.div_ceil(2).max(1);
    let in_len = (width * height) as usize;
    assert!(scratch.len() >= in_len, "pyramid scratch buffer too small");

    {
        let blurred_h = &mut scratch[..in_len];
        blurred_h
            .par_chunks_mut(width as usize)
            .enumerate()
            .for_each(|(y, row)| {
                let y = y as u32;
                for x in 0..width {
                    let mut sum = 0.0f32;
                    for (k, weight) in PYRAMID_KERNEL.iter().enumerate() {
                        let xx = clamp_coord(x as i32 + k as i32 - 2, width);
                        sum += gray[(y * width + xx) as usize] * *weight;
                    }
                    row[x as usize] = sum / PYRAMID_KERNEL_SUM;
                }
            });
    }

    let blurred_h: &[f32] = &scratch[..in_len];
    let mut out = vec![0.0f32; (out_w * out_h) as usize];
    out.par_chunks_mut(out_w as usize)
        .enumerate()
        .for_each(|(oy, out_row)| {
            let src_y = ((oy as u32) * 2).min(height - 1);
            for ox in 0..out_w {
                let src_x = (ox * 2).min(width - 1);
                let mut sum = 0.0f32;
                for (k, weight) in PYRAMID_KERNEL.iter().enumerate() {
                    let yy = clamp_coord(src_y as i32 + k as i32 - 2, height);
                    sum += blurred_h[(yy * width + src_x) as usize] * *weight;
                }
                out_row[ox as usize] = sum / PYRAMID_KERNEL_SUM;
            }
        });

    PyramidLevel {
        scale_log2: 0,
        width: out_w,
        height: out_h,
        gray: out,
    }
}

// `FramePyramid` stores ONLY downsampled levels (1..N). Level 0 is the
// `PreparedFrame`'s grayscale buffer, referenced by callers when they need
// full-resolution access. This avoids the ~33 MB clone per 4K frame that a
// cached level 0 would incur.
fn build_frame_pyramid(gray: &[f32], width: u32, height: u32) -> FramePyramid {
    let mut levels: Vec<PyramidLevel> = Vec::new();
    // One scratch buffer, sized for level-0 dimensions, reused for every
    // subsequent level (each smaller than the last).
    let mut scratch = vec![0.0f32; (width * height) as usize];
    let mut cur_gray: &[f32] = gray;
    let mut cur_w = width;
    let mut cur_h = height;
    let mut scale_log2: u8 = 0;

    while levels.len() < (PYRAMID_MAX_LEVELS as usize - 1) {
        if cur_w <= PYRAMID_MIN_LEVEL_SIDE || cur_h <= PYRAMID_MIN_LEVEL_SIDE {
            break;
        }
        scale_log2 += 1;
        let next = gaussian_downsample_2x(cur_gray, cur_w, cur_h, &mut scratch);
        levels.push(PyramidLevel {
            scale_log2,
            width: next.width,
            height: next.height,
            gray: next.gray,
        });
        // Borrow the just-pushed level for the next iteration. Safe because we
        // never mutate `levels` until the next push, and we don't read
        // previous levels after writing them.
        let last = levels.last().expect("just pushed");
        cur_gray = &last.gray;
        cur_w = last.width;
        cur_h = last.height;
    }

    FramePyramid { levels }
}
```

Note: `par_chunks_mut` requires `use rayon::prelude::*;` which the matcher
already imports for `into_par_iter` (matcher.rs:8 area). Confirm before edit
that the import is in scope; if not, add it.

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

- [ ] **Step 7: Verify clippy passes before the commit**

At this point `PreparedFrame::pyramid`, `build_frame_pyramid`, and `gaussian_downsample_2x` exist but nothing in `estimate_motion` calls them yet (Task 4 wires them in). The project's verification flow runs `cargo clippy ... -- -D warnings`, which treats `dead_code` as a hard error. Run:

```bash
rtk cargo clippy -p rollshot-core --all-targets -- -D warnings
```

Expected: PASS.

If clippy fires `dead_code` on any of `PreparedFrame::pyramid`, `build_frame_pyramid`, `gaussian_downsample_2x`, `FramePyramid`, or `PyramidLevel`, add a scoped attribute with a TODO pointing at Task 4:

```rust
#[allow(dead_code)] // wired in by Task 4 (pyramid candidate search)
```

Remove the attribute in Task 4 Step 6 when the symbols are used.

- [ ] **Step 8: Commit pyramid construction**

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
coarsest_full_range_search, pyramid_axis_candidate, pyramid_candidates_for_axes,
pyramid_mad, pyramid_max_axis_offset, refine_at_level, PYRAMID_REFINE_RADIUS,
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
fn pyramid_large_jump_finds_correct_horizontal_candidate() {
    // Mirror of the vertical large-jump test for the horizontal axis. Catches
    // any axis-swap bug in `max_offset` (width vs height) and in the
    // offset-to-(dx, dy) mapping inside `pyramid_axis_candidate`.
    let canvas = make_wide_canvas(1200, 360);
    let prev = crop_xy(&canvas, 0, 0, 320, 320);
    let curr = crop_xy(&canvas, 210, 0, 320, 320);
    let prev_prep = PreparedFrame::new(prev);
    let curr_prep = PreparedFrame::new(curr);
    let config = StitchConfig::default();
    let mut metrics = StitchMetrics::default();

    let candidates = pyramid_candidates_for_axes(
        &prev_prep,
        &curr_prep,
        None,
        &[SearchAxis::Horizontal],
        &config,
        &mut metrics,
    );

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].dy, 0);
    assert!(
        (candidates[0].dx - 210).abs() <= PYRAMID_REFINE_RADIUS + 2,
        "dx = {} (expected around 210)",
        candidates[0].dx
    );
    assert_eq!(candidates[0].method, crate::types::MatchMethod::Pyramid);
}

#[test]
fn pyramid_recovers_retina_pair() {
    // Spec calls out 4K/retina pairs as required coverage. Use a synthetic
    // retina-scale canvas (1920x2400, the practical worst case for screen
    // capture) with a 600 px vertical jump — far beyond the default
    // `max_search_ratio`-bounded coarse window.
    let canvas = make_aperiodic_canvas(1920, 2400);
    let prev = crop(&canvas, 0, 1200);
    let curr = crop(&canvas, 600, 1200);
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

    assert_eq!(candidates.len(), 1, "retina pair must produce a pyramid candidate");
    assert_eq!(candidates[0].dx, 0);
    assert!(
        (candidates[0].dy - 600).abs() <= PYRAMID_REFINE_RADIUS + 4,
        "dy = {} (expected around 600 at retina scale)",
        candidates[0].dy
    );
    assert_eq!(candidates[0].method, crate::types::MatchMethod::Pyramid);
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
        // Second-best must be NO BETTER than best (lower score = better in
        // rollshot's confidence contract). It originates at the coarsest
        // pyramid level, where repeated-pattern ambiguity manifests — so a
        // unimodal scene should leave a clear gap here.
        assert!(second >= candidate.score);
    }
}
```

The repeated-grid alias-rejection test moves to Task 5 (Step 1) because it
asserts an integration property of `estimate_motion`, not of
`pyramid_candidates_for_axes` directly.

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-core pyramid_large_jump_finds_correct_candidate pyramid_large_jump_finds_correct_horizontal_candidate pyramid_recovers_retina_pair pyramid_score_contract_matches_ranker
```

Expected: FAIL with missing pyramid search helpers.

- [ ] **Step 3: Add pyramid search constants and the axis-offset helper**

Near the pyramid constants, add:

```rust
const PYRAMID_REFINE_RADIUS: i32 = 4;
```

Then add the axis-offset helper that EVERY pyramid level uses to compute its
search range. Introducing the helper FIRST avoids the buggy intermediate state
where `min_overlap >> scale_log2` could underflow to 0 and let `max_offset`
silently include zero-overlap shifts:

```rust
fn pyramid_max_axis_offset(
    level_width: u32,
    level_height: u32,
    axis: SearchAxis,
    min_overlap: u32,
    scale_log2: u8,
) -> i32 {
    // At deep levels, `min_overlap >> scale_log2` can become 0; clamp to at
    // least 1 px so we never search offsets that produce zero overlap.
    let scaled_min_overlap = (min_overlap >> scale_log2).max(1);
    match axis {
        SearchAxis::Vertical => level_height.saturating_sub(scaled_min_overlap) as i32,
        SearchAxis::Horizontal => level_width.saturating_sub(scaled_min_overlap) as i32,
    }
}
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

Two helpers, with single responsibilities. `coarsest_full_range_search` does
the unbounded sweep at the smallest pyramid level (where second-best
ambiguity actually manifests) and reports BOTH the best score and the
second-best. `refine_at_level` does a tight ±radius refinement at a finer
level given a seed. Splitting them avoids an overloaded `radius_or_max`
parameter that meant different things based on `seed == 0`.

Add below `coarse_axis_candidate`:

```rust
/// Full-range axis scan at the coarsest pyramid level. Returns
/// `(best_offset, best_raw_mad, second_best_raw_mad)`. Parallel over
/// candidate offsets — matches the pattern in `coarse_axis_candidate`.
fn coarsest_full_range_search(
    prev: &PyramidLevel,
    curr: &PyramidLevel,
    axis: SearchAxis,
    max_offset: i32,
) -> Option<(i32, f32, Option<f32>)> {
    let offsets: Vec<i32> = (-max_offset..=max_offset).filter(|o| *o != 0).collect();
    let mut scored: Vec<(f32, i32)> = offsets
        .into_par_iter()
        .filter_map(|offset| {
            let (dx, dy) = match axis {
                SearchAxis::Vertical => (0, offset),
                SearchAxis::Horizontal => (offset, 0),
            };
            let score = pyramid_mad(&prev.gray, &curr.gray, prev.width, prev.height, dx, dy);
            score.is_finite().then_some((score, offset))
        })
        .collect();
    scored.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let (best_score, best_offset) = *scored.first()?;
    let second = scored.get(1).map(|(score, _)| *score);
    Some((best_offset, best_score, second))
}

/// Refine `seed` within ±`radius` against the given gray buffer (any
/// pyramid level OR the caller's full-resolution level-0 gray). Sequential
/// because refinement has only ~9 offsets — thread overhead would dominate.
fn refine_at_level(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    axis: SearchAxis,
    seed: i32,
    max_abs: i32,
    radius: i32,
) -> Option<i32> {
    refinement_offsets(seed, max_abs, radius)
        .into_iter()
        .filter(|o| *o != 0)
        .filter_map(|offset| {
            let (dx, dy) = match axis {
                SearchAxis::Vertical => (0, offset),
                SearchAxis::Horizontal => (offset, 0),
            };
            let score = pyramid_mad(prev_gray, curr_gray, width, height, dx, dy);
            score.is_finite().then_some((score, offset))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(_, offset)| offset)
}

/// Drives the coarse-to-fine search. `prev`/`curr` are the cached
/// downsampled pyramids (levels 1..N). `prev_level0_gray`/`curr_level0_gray`
/// are the caller's existing grayscale buffers — passed by reference so the
/// pyramid never has to clone level 0 (saves ~33 MB per 4K frame).
fn pyramid_axis_candidate(
    prev: &FramePyramid,
    curr: &FramePyramid,
    prev_level0_gray: &[f32],
    curr_level0_gray: &[f32],
    level0_width: u32,
    level0_height: u32,
    axis: SearchAxis,
    config: &StitchConfig,
) -> Option<MotionCandidate> {
    if prev.levels.is_empty() || curr.levels.is_empty() {
        // Tiny frame produced no downsampled levels; coarse/template handle it.
        return None;
    }
    let top_idx = prev.levels.len().min(curr.levels.len()) - 1;
    let top = &prev.levels[top_idx];
    let curr_top = &curr.levels[top_idx];
    if top.width != curr_top.width || top.height != curr_top.height {
        return None;
    }

    let top_max = pyramid_max_axis_offset(
        top.width,
        top.height,
        axis,
        config.min_overlap,
        top.scale_log2,
    );
    if top_max <= 0 {
        return None;
    }

    let (top_offset, _top_best_score, top_second_score) =
        coarsest_full_range_search(top, curr_top, axis, top_max)?;

    // Propagate offset down through intermediate pyramid levels.
    let mut offset = top_offset;
    for level_idx in (0..top_idx).rev() {
        let level = &prev.levels[level_idx];
        let curr_level = &curr.levels[level_idx];
        let level_max = pyramid_max_axis_offset(
            level.width,
            level.height,
            axis,
            config.min_overlap,
            level.scale_log2,
        );
        if level_max <= 0 {
            return None;
        }
        offset = (offset * 2).clamp(-level_max, level_max);
        offset = refine_at_level(
            &level.gray,
            &curr_level.gray,
            level.width,
            level.height,
            axis,
            offset,
            level_max,
            PYRAMID_REFINE_RADIUS,
        )?;
    }

    // Final refinement at full resolution (level 0, by reference).
    let level0_max = pyramid_max_axis_offset(
        level0_width,
        level0_height,
        axis,
        config.min_overlap,
        0,
    );
    if level0_max <= 0 {
        return None;
    }
    offset = (offset * 2).clamp(-level0_max, level0_max);
    let best_offset = refine_at_level(
        prev_level0_gray,
        curr_level0_gray,
        level0_width,
        level0_height,
        axis,
        offset,
        level0_max,
        PYRAMID_REFINE_RADIUS,
    )?;

    let (dx, dy) = match axis {
        SearchAxis::Vertical => (0, best_offset),
        SearchAxis::Horizontal => (best_offset, 0),
    };
    let best_score = pyramid_mad(
        prev_level0_gray,
        curr_level0_gray,
        level0_width,
        level0_height,
        dx,
        dy,
    );
    if !best_score.is_finite() {
        return None;
    }

    // Second-best is propagated from the coarsest pyramid level. That's
    // where repeated-pattern ambiguity actually manifests — the ±4 full-res
    // refinement window would always show near-identical neighbors and
    // defeat the existing `second_best_margin` ambiguity rejection.
    Some(candidate(
        dx,
        dy,
        MatchMethod::Pyramid,
        pyramid_confidence(best_score),
        top_second_score.map(pyramid_confidence),
    ))
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
    let (level0_w, level0_h) = prev.dimensions();
    let out: Vec<_> = axes
        .iter()
        .filter_map(|axis| {
            pyramid_axis_candidate(
                prev_pyramid,
                curr_pyramid,
                prev.gray(),
                curr.gray(),
                level0_w,
                level0_h,
                *axis,
                config,
            )
        })
        .filter(|candidate| candidate_matches_axis(candidate.dx, candidate.dy, locked_axis, config))
        .collect();
    // Pyramid is called exactly once per frame (axis fast path doesn't run
    // it), so a direct assignment matches the `coarse_candidates` semantics
    // at matcher.rs:220 — no `.max()` needed.
    metrics.pyramid_candidates = out.len();
    out
}
```

If clippy `dead_code` attributes were added in Task 3 Step 7, remove them now
that these symbols are called.

- [ ] **Step 7: Run focused search tests**

Run:

```bash
rtk cargo test -p rollshot-core pyramid_large_jump_finds_correct_candidate pyramid_large_jump_finds_correct_horizontal_candidate pyramid_recovers_retina_pair pyramid_score_contract_matches_ranker
```

Expected: PASS. Each test exercises `pyramid_candidates_for_axes` directly
and is self-contained within Task 4 — no cross-task dependency.

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

Extend the test module's `use super::{ ... }` import list to include
`candidate` and `template_seed` — both are needed by the `template_seed_*`
tests below:

```rust
candidate, template_seed,
```

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

#[test]
fn pyramid_does_not_accept_repeated_grid_alias() {
    // Relocated from Task 4: this asserts a property of the full
    // `estimate_motion` pipeline (pyramid candidates filtered out by the
    // ranker via second_best_margin), so it belongs with the other Task 5
    // integration tests.
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

#[test]
fn template_seed_keeps_last_motion_when_nonzero() {
    // Direct unit test of the seed-priority contract: `last_motion` is the
    // most accurate full-resolution seed for steady scroll (P3/P4
    // behavior). Pyramid and coarse must NOT override it.
    let coarse = vec![candidate(0, 80, MatchMethod::Coarse, 0.1, None)];
    let pyramid = vec![candidate(0, 200, MatchMethod::Pyramid, 0.05, None)];
    let seed = template_seed(SearchAxis::Vertical, (0, 16), &coarse, &pyramid);
    assert_eq!(seed, 16, "nonzero last_motion must dominate pyramid + coarse");
}

#[test]
fn template_seed_prefers_pyramid_over_coarse_when_history_zero() {
    // Fallback ordering: pyramid is a stronger seed than coarse because it
    // produces a full-resolution offset (after refinement), while coarse is
    // 32-px-quantized. This contract guarantees pyramid recovery on the
    // first frame after a duplicate / cross-axis probe.
    let coarse = vec![candidate(0, 80, MatchMethod::Coarse, 0.1, None)];
    let pyramid = vec![candidate(0, 200, MatchMethod::Pyramid, 0.05, None)];
    let seed = template_seed(SearchAxis::Vertical, (0, 0), &coarse, &pyramid);
    assert_eq!(seed, 200, "with no last_motion, pyramid must win over coarse");
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
rtk cargo test -p rollshot-core pyramid_candidate_passes_existing_verifier pyramid_does_not_accept_repeated_grid_alias template_seed_keeps_last_motion_when_nonzero template_seed_prefers_pyramid_over_coarse_when_history_zero fast_scroll_beyond_default_search_ratio_recovers_via_pyramid
```

Expected: FAIL because `estimate_motion` does not use pyramid candidates yet
and `template_seed`'s signature still ignores pyramid.

- [ ] **Step 3: Insert pyramid candidates into `estimate_motion` regular path**

This is a targeted insertion, NOT a wholesale rewrite of the regular path.
Edit `estimate_motion` (matcher.rs) by adding exactly the two lines below.
Insertion point: immediately after the existing
`metrics.coarse_candidates = coarse.len();` line, and BEFORE the existing
`let template_start = std::time::Instant::now();` line.

Insert:

```rust
let pyramid = pyramid_candidates(prev, curr, locked_axis, config, metrics);
candidates.extend(pyramid.iter().copied());
```

Then update the existing `template_candidates(...)` call by adding the new
`&pyramid` argument between `&coarse` and `config` — the only change to that
call is the extra argument.

Do not rewrite or re-order any of the surrounding code (timing variables,
edge candidates, verifier ranker). The total diff for this step should be
two added lines plus one argument added to the `template_candidates` call.

- [ ] **Step 4: Update `template_seed` and propagate to all callers**

Change the signature of `template_seed`:

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

Update `template_candidates` and `template_candidates_for_axes` to accept
`pyramid: &[MotionCandidate]` and pass it through to `template_seed`.

There are TWO callsites that invoke these functions. Update both:

1. The regular path in `estimate_motion` (already updated in Step 3) —
   passes the actual `&pyramid` vector built that frame.
2. `axis_fast_path_candidate` (matcher.rs around line 321), which calls
   `template_candidates_for_axes`. The axis fast path does NOT run pyramid
   (Step 5 below), so pass an empty slice:

   ```rust
   candidates.extend(template_candidates_for_axes(
       prev,
       curr,
       last_motion,
       &coarse,
       &[], // no pyramid in fast path
       &axes,
       config,
       metrics,
   ));
   ```

Without this second-callsite update, the matcher will not compile.

- [ ] **Step 5: Keep axis fast path unchanged**

Do not add pyramid to `axis_fast_path_candidate`. The axis fast path is for steady locked-axis frames and should stay cheaper than the full regular path. Suspicious fast-path frames still fall back to the regular path, where pyramid runs.

- [ ] **Step 6: Run focused integration tests**

Run:

```bash
rtk cargo test -p rollshot-core pyramid_candidate_passes_existing_verifier pyramid_does_not_accept_repeated_grid_alias template_seed_keeps_last_motion_when_nonzero template_seed_prefers_pyramid_over_coarse_when_history_zero fast_scroll_beyond_default_search_ratio_recovers_via_pyramid repeated_grid_is_rejected_by_second_best_margin repeated_rows_do_not_append_without_clear_match
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

Add a pyramid pixel-visit counter to `SearchBudget`:

```rust
pub pyramid_pixel_visits: u64,
```

(immediately after `full_res_ncc_pixel_visits`). Then instrument
`pyramid_mad` to count its pixel visits via the existing test-only
`with_active_search_budget` helper:

```rust
fn pyramid_mad(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    dx: i32,
    dy: i32,
) -> f32 {
    let score = coarse_mad(prev_gray, curr_gray, width, height, dx, dy, 1);
    #[cfg(test)]
    if let Some(overlap) = compute_overlap(width, height, width, height, dx, dy) {
        with_active_search_budget(|budget| {
            budget.pyramid_pixel_visits = budget
                .pyramid_pixel_visits
                .saturating_add(overlap.area());
        });
    }
    score
}
```

Add a corresponding bound to the budget test:

```rust
assert!(
    budget.pyramid_pixel_visits <= 50_000_000,
    "pyramid_pixel_visits = {}",
    budget.pyramid_pixel_visits
);
```

Pyramid top-level full-range search runs on downsampled levels, so a 50M cap
leaves comfortable headroom even at 4K. The counter guards against future
regressions that drop `PYRAMID_MIN_LEVEL_SIDE` or widen
`PYRAMID_REFINE_RADIUS` without thinking about cost.

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
