# P4 Axis Fast Path Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a locked-axis fast path to the matcher so steady-state vertical or horizontal scrolling scores only the locked main axis first, guarded by a cheap cross-axis NCC sentinel and falling back to the existing dual-axis search whenever the frame looks suspicious.

**Architecture:** `estimate_motion` keeps the existing dual-axis matcher as the conservative fallback. When `locked_axis` is set and `StitchConfig::axis_fast_path.enabled` is true, it gathers coarse/template/edge candidates for only the locked main axis, verifies them with the existing `PixelOverlapVerifier`, runs a narrow cross-axis NCC probe around the accepted main-axis candidate, and returns it only when the probe is not suspicious. Suspicious frames, no fast-path candidate, first-motion frames, and disabled config all continue through the current dual-axis path unchanged.

**Tech Stack:** Rust, `rollshot-core`, existing `PreparedFrame`, `wide` NCC scorer, `StitchMetrics`, test-only `SearchBudget`, existing `stitch_sequences` benchmark harness.

**Source:** `docs/stitching-rollshot-optimizations-2.md` section `6. P4 - Axis-Locked Fast Path`. Code is the source of truth; this plan reflects the current P0-P3 implementation where `PreparedFrame`, metrics, `fast_ncc_score_shifted`, and the benchmark harness already exist.

---

## Assumptions

- P4 is a matcher-only optimization. It must not change canvas topology, append semantics, duplicate handling, or stitcher anchor updates.
- The default behavior should enable the fast path, because the acceptance path is still verifier-gated and falls back to the old dual-axis search when suspicious.
- `AxisFastPathConfig::cross_axis_probe_radius` defaults to `6`, matching the current `max_cross_axis_px` default.
- The cross-axis sentinel uses existing full-resolution NCC. It is intentionally cheap because it probes only `2 * radius + 1` offsets, not a full cross-axis search.
- The existing `SearchBudget` tests are the right structural proof that fewer offsets are scored; benchmark JSONL is the runtime proof.

## Files and Responsibilities

- Modify: `crates/rollshot-core/src/types.rs` - add `AxisFastPathConfig`, add `axis_fast_path` to `StitchConfig`, update defaults and config tests.
- Modify: `crates/rollshot-core/src/matcher.rs` - split candidate gathering into main-axis and dual-axis helpers; add cross-axis sentinel; wire the fast path into `estimate_motion`; add matcher unit tests.
- Local artifact: `bench-results/runs/p4-axis-fast-path/before.jsonl` - baseline benchmark output, not committed.
- Local artifact: `bench-results/runs/p4-axis-fast-path/after.jsonl` - post-change benchmark output, not committed.
- Modify/Create: `bench-results/compare/2026-05-28-p4-axis-fast-path-compare.md` - committed benchmark comparison report if this repository already commits compare reports for optimization PRs. If `bench-results/compare/` is gitignored or absent, leave only the local JSONL and summarize results in the final handoff.

No changes to CLI, app, capture, canvas, verifier thresholds, feature fallback behavior, or public stitcher APIs.

---

### Task 0: Capture P4 Baseline Benchmark

**Files:**
- Local artifact: `bench-results/runs/p4-axis-fast-path/before.jsonl`

- [ ] **Step 1: Confirm the worktree state before benchmarking**

Run:

```bash
rtk git status --short
```

Expected: no unrelated code changes. If there is output, inspect it. Do not revert user changes. If unrelated changes exist, record that the baseline was captured on a dirty worktree and list the files in the implementation notes.

- [ ] **Step 2: Capture the before benchmark JSONL**

Run:

```bash
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter,linear_horizontal_right \
    --repeats 3 \
    --out bench-results/runs/p4-axis-fast-path/before.jsonl
```

Expected: command exits 0 and writes `bench-results/runs/p4-axis-fast-path/before.jsonl`.

- [ ] **Step 3: Sanity-check the baseline has matcher counters**

Run:

```bash
rtk python3 scripts/bench/summarize.py bench-results/runs/p4-axis-fast-path/before.jsonl | rtk rg "ncc_offsets|template_ncc|Scenario|Fixture"
```

Expected: output includes fixture rows or summary fields for NCC offsets/timing. Keep the raw JSONL local.

No commit in this task.

---

### Task 1: Add Axis Fast Path Config

**Files:**
- Modify: `crates/rollshot-core/src/types.rs`

- [ ] **Step 1: Add the config type**

In `crates/rollshot-core/src/types.rs`, insert this block immediately before `pub struct StitchConfig`:

```rust
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct AxisFastPathConfig {
    pub enabled: bool,
    pub cross_axis_probe_radius: i32,
    pub fallback_to_dual_axis_on_suspicious: bool,
}

impl Default for AxisFastPathConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cross_axis_probe_radius: 6,
            fallback_to_dual_axis_on_suspicious: true,
        }
    }
}
```

- [ ] **Step 2: Add the config field to `StitchConfig`**

In `StitchConfig`, add the field after `match_width`:

```rust
    pub axis_fast_path: AxisFastPathConfig,
```

In `impl Default for StitchConfig`, add the default after `match_width: 512,`:

```rust
            axis_fast_path: AxisFastPathConfig::default(),
```

- [ ] **Step 3: Extend the default config test**

In `crates/rollshot-core/src/types.rs`, in `default_config_picks_auto_hybrid`, add:

```rust
        assert!(cfg.axis_fast_path.enabled);
        assert_eq!(cfg.axis_fast_path.cross_axis_probe_radius, 6);
        assert!(cfg.axis_fast_path.fallback_to_dual_axis_on_suspicious);
```

- [ ] **Step 4: Verify the config change**

Run:

```bash
rtk cargo test -p rollshot-core types::tests::default_config_picks_auto_hybrid
```

Expected: test passes.

- [ ] **Step 5: Commit config**

Run:

```bash
rtk git add crates/rollshot-core/src/types.rs
rtk git commit -m "feat(core): add axis fast path config"
```

Expected: commit succeeds.

---

### Task 2: Split Candidate Gathering by Search Axis

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`

- [ ] **Step 1: Add axis helper methods**

In `crates/rollshot-core/src/matcher.rs`, immediately after `enum SearchAxis`, add:

```rust
impl SearchAxis {
    fn from_scroll_axis(axis: ScrollAxis) -> Self {
        match axis {
            ScrollAxis::Vertical => Self::Vertical,
            ScrollAxis::Horizontal => Self::Horizontal,
        }
    }

    fn cross_axis_delta(self, dx: i32, dy: i32) -> i32 {
        match self {
            Self::Vertical => dx,
            Self::Horizontal => dy,
        }
    }

    fn with_cross_axis_delta(self, main_offset: i32, cross_offset: i32) -> (i32, i32) {
        match self {
            Self::Vertical => (cross_offset, main_offset),
            Self::Horizontal => (main_offset, cross_offset),
        }
    }
}
```

- [ ] **Step 2: Replace `search_axes` with explicit dual axes**

Replace the current `fn search_axes(locked_axis: Option<ScrollAxis>) -> &'static [SearchAxis]` with:

```rust
const DUAL_SEARCH_AXES: &[SearchAxis] = &[SearchAxis::Vertical, SearchAxis::Horizontal];

fn dual_search_axes() -> &'static [SearchAxis] {
    DUAL_SEARCH_AXES
}
```

- [ ] **Step 3: Add axis-scoped wrappers for coarse candidates**

Replace the current `coarse_candidates` body with this wrapper plus helper:

```rust
fn coarse_candidates(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<ScrollAxis>,
    config: &StitchConfig,
) -> Vec<MotionCandidate> {
    coarse_candidates_for_axes(prev, curr, locked_axis, dual_search_axes(), config)
}

fn coarse_candidates_for_axes(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<ScrollAxis>,
    axes: &[SearchAxis],
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

- [ ] **Step 4: Add axis-scoped wrappers for template candidates**

Replace the current `template_candidates` body with this wrapper plus helper:

```rust
fn template_candidates(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    coarse: &[MotionCandidate],
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> Vec<MotionCandidate> {
    template_candidates_for_axes(
        prev,
        curr,
        locked_axis,
        last_motion,
        coarse,
        dual_search_axes(),
        config,
        metrics,
    )
}

fn template_candidates_for_axes(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    coarse: &[MotionCandidate],
    axes: &[SearchAxis],
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> Vec<MotionCandidate> {
    let mut out = Vec::new();
    let (width, height) = prev.dimensions();
    let roi = content_roi(width, height);
    let match_region = match_width_region(roi, config.match_width);

    for axis in axes {
        let seed = template_seed(*axis, last_motion, coarse);
        if let Some(candidate) =
            search_template_axis(prev, curr, *axis, match_region, seed, config, metrics)
        {
            out.push(candidate);
        }
    }

    out
}
```

- [ ] **Step 5: Add axis-scoped wrappers for edge candidates**

Replace the current `edge_projection_candidates` body with this wrapper plus helper:

```rust
fn edge_projection_candidates(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<ScrollAxis>,
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> Vec<MotionCandidate> {
    edge_projection_candidates_for_axes(
        prev,
        curr,
        locked_axis,
        dual_search_axes(),
        config,
        metrics,
    )
}

fn edge_projection_candidates_for_axes(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: Option<ScrollAxis>,
    axes: &[SearchAxis],
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> Vec<MotionCandidate> {
    let _ = metrics; // reserved for edge-stage counters; timing is captured by the outer ScopedTimer.
    let (width, height) = prev.dimensions();
    let mut out = Vec::new();

    for axis in axes {
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

- [ ] **Step 6: Verify the refactor preserves current behavior**

Run:

```bash
rtk cargo test -p rollshot-core --lib matcher::tests::estimate_motion
```

Expected: existing matcher estimate tests pass.

- [ ] **Step 7: Commit axis-scoped candidate helpers**

Run:

```bash
rtk git add crates/rollshot-core/src/matcher.rs
rtk git commit -m "refactor(core): split matcher searches by axis"
```

Expected: commit succeeds.

---

### Task 3: Add Cross-Axis Sentinel

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`

- [ ] **Step 1: Add sentinel data structures and threshold**

In `matcher.rs`, after `struct CandidateScore`, add:

```rust
const CROSS_AXIS_RESIDUAL_IMPROVEMENT: f32 = 0.03;

#[derive(Debug, Clone, Copy, PartialEq)]
struct CrossAxisCheck {
    estimated_cross_px: i32,
    residual_score: f32,
    suspicious: bool,
}
```

- [ ] **Step 2: Add the cross-axis check helper**

Insert this helper after `search_template_axis`:

```rust
fn cross_axis_check(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    candidate: MotionCandidate,
    main_axis: SearchAxis,
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> CrossAxisCheck {
    let radius = config.axis_fast_path.cross_axis_probe_radius.max(0);
    if radius == 0 {
        return CrossAxisCheck {
            estimated_cross_px: 0,
            residual_score: 0.0,
            suspicious: false,
        };
    }

    let (width, height) = prev.dimensions();
    let region = match_width_region(content_roi(width, height), config.match_width);
    let main_offset = predicted_offset(main_axis, (candidate.dx, candidate.dy));
    let current_cross = main_axis.cross_axis_delta(candidate.dx, candidate.dy);
    let offsets: Vec<i32> = (-radius..=radius).collect();

    metrics.ncc_offsets_scored += offsets.len();
    metrics.ncc_pixel_visits += offsets
        .len()
        .saturating_mul(region.w as usize * region.h as usize);

    let mut best_cross = current_cross;
    let mut best_score = f32::MIN;
    let mut base_score = f32::MIN;

    for cross_offset in offsets {
        let (dx, dy) = main_axis.with_cross_axis_delta(main_offset, cross_offset);
        let score = fast_ncc_score_shifted(prev, curr, region, dx, dy);
        if cross_offset == current_cross {
            base_score = score;
        }
        if score.is_finite() && (!best_score.is_finite() || score > best_score) {
            best_score = score;
            best_cross = cross_offset;
        }
    }

    let residual_score = if best_score.is_finite() && base_score.is_finite() {
        best_score - base_score
    } else {
        0.0
    };
    let suspicious = best_cross.abs() > config.max_cross_axis_px
        || residual_score > CROSS_AXIS_RESIDUAL_IMPROVEMENT;

    CrossAxisCheck {
        estimated_cross_px: best_cross,
        residual_score,
        suspicious,
    }
}
```

- [ ] **Step 3: Import sentinel items in matcher tests**

In the `#[cfg(test)] mod tests` `use super::{...}` list, add:

```rust
        cross_axis_check, CrossAxisCheck,
```

- [ ] **Step 4: Add sentinel tests**

In the matcher test module, add these tests near the existing NCC tests:

```rust
    #[test]
    fn cross_axis_check_allows_zero_cross_axis_motion() {
        let canvas = make_aperiodic_canvas(260, 360);
        let prev = crop_xy(&canvas, 0, 0, 180, 180);
        let curr = crop_xy(&canvas, 0, 40, 180, 180);
        let candidate = MotionCandidate {
            dx: 0,
            dy: 40,
            method: crate::types::MatchMethod::Template,
            score: 0.01,
            second_best_score: None,
            inliers: None,
            raw_matches: None,
        };
        let mut metrics = StitchMetrics::default();

        let check = cross_axis_check(
            &prep(&prev),
            &prep(&curr),
            candidate,
            SearchAxis::Vertical,
            &StitchConfig::default(),
            &mut metrics,
        );

        assert_eq!(check.estimated_cross_px, 0);
        assert!(
            !check.suspicious,
            "zero-cross vertical motion should not be suspicious: {check:?}"
        );
        assert!(metrics.ncc_offsets_scored > 0);
    }

    #[test]
    fn cross_axis_check_flags_drift_beyond_tolerance() {
        let canvas = make_aperiodic_canvas(280, 380);
        let prev = crop_xy(&canvas, 0, 0, 180, 180);
        let curr = crop_xy(&canvas, 10, 40, 180, 180);
        let candidate = MotionCandidate {
            dx: 0,
            dy: 40,
            method: crate::types::MatchMethod::Template,
            score: 0.01,
            second_best_score: None,
            inliers: None,
            raw_matches: None,
        };
        let config = StitchConfig {
            axis_fast_path: crate::types::AxisFastPathConfig {
                cross_axis_probe_radius: 12,
                ..crate::types::AxisFastPathConfig::default()
            },
            ..StitchConfig::default()
        };
        let mut metrics = StitchMetrics::default();

        let check = cross_axis_check(
            &prep(&prev),
            &prep(&curr),
            candidate,
            SearchAxis::Vertical,
            &config,
            &mut metrics,
        );

        assert!(
            check.suspicious,
            "dx drift should be suspicious: {check:?}"
        );
        assert!(
            check.estimated_cross_px.abs() > config.max_cross_axis_px,
            "estimated_cross_px = {}",
            check.estimated_cross_px
        );
    }
```

- [ ] **Step 5: Verify sentinel tests**

Run:

```bash
rtk cargo test -p rollshot-core --lib matcher::tests::cross_axis_check
```

Expected: both tests pass.

- [ ] **Step 6: Commit sentinel**

Run:

```bash
rtk git add crates/rollshot-core/src/matcher.rs
rtk git commit -m "feat(core): add cross-axis drift sentinel"
```

Expected: commit succeeds.

---

### Task 4: Wire Locked-Axis Fast Path into `estimate_motion`

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`

- [ ] **Step 1: Add the fast-path helper**

Insert this function before `const RELAXED_SEARCH_RATIO: f32 = 0.85;`:

```rust
fn axis_fast_path_candidate(
    prev: &PreparedFrame,
    curr: &PreparedFrame,
    locked_axis: ScrollAxis,
    last_motion: (i32, i32),
    config: &StitchConfig,
    metrics: &mut StitchMetrics,
) -> Option<MotionCandidate> {
    let main_axis = SearchAxis::from_scroll_axis(locked_axis);
    let axes = [main_axis];

    let coarse = {
        let _t = ScopedTimer::new(&mut metrics.coarse_us);
        coarse_candidates_for_axes(prev, curr, Some(locked_axis), &axes, config)
    };
    metrics.coarse_candidates = metrics.coarse_candidates.max(coarse.len());

    let mut candidates = Vec::new();
    candidates.extend(coarse.iter().copied());

    let template_start = std::time::Instant::now();
    candidates.extend(template_candidates_for_axes(
        prev,
        curr,
        Some(locked_axis),
        last_motion,
        &coarse,
        &axes,
        config,
        metrics,
    ));
    metrics.template_ncc_us += template_start.elapsed().as_micros() as u64;

    let edge_start = std::time::Instant::now();
    candidates.extend(edge_projection_candidates_for_axes(
        prev,
        curr,
        Some(locked_axis),
        &axes,
        config,
        metrics,
    ));
    metrics.edge_projection_us += edge_start.elapsed().as_micros() as u64;

    metrics.verifier_candidates += candidates.len();
    let candidate = {
        let _t = ScopedTimer::new(&mut metrics.verifier_us);
        rank_verified_candidates(prev.rgba(), curr.rgba(), Some(locked_axis), candidates, config)
    }?;

    let cross_axis = cross_axis_check(prev, curr, candidate, main_axis, config, metrics);
    if cross_axis.suspicious && config.axis_fast_path.fallback_to_dual_axis_on_suspicious {
        return None;
    }

    Some(candidate)
}
```

- [ ] **Step 2: Call the fast path before the existing dual-axis flow**

In `estimate_motion`, immediately after the dimension-mismatch check and before `let mut candidates = Vec::new();`, add:

```rust
    if let Some(axis) = locked_axis {
        if config.axis_fast_path.enabled {
            if let Some(candidate) =
                axis_fast_path_candidate(prev, curr, axis, last_motion, config, metrics)
            {
                return MotionSearchOutcome::Candidate(candidate);
            }
        }
    }
```

Leave the existing dual-axis flow below it intact. It is the fallback for disabled config, first-motion no-lock, suspicious sentinel, and fast-path miss.

- [ ] **Step 3: Add an enabled-vs-disabled structural budget test**

In the matcher test module, add:

```rust
    #[test]
    fn locked_vertical_uses_main_axis_fast_path() {
        let canvas = make_aperiodic_canvas(420, 760);
        let prev = crop_xy(&canvas, 0, 0, 320, 320);
        let curr = crop_xy(&canvas, 0, 84, 320, 320);

        let mut fast_budget = SearchBudget::default();
        let fast_candidate = unwrap_candidate(estimate_motion_with_budget(
            &prev,
            &curr,
            Some(ScrollAxis::Vertical),
            (0, 84),
            &StitchConfig::default(),
            &mut fast_budget,
        ));

        let dual_config = StitchConfig {
            axis_fast_path: crate::types::AxisFastPathConfig {
                enabled: false,
                ..crate::types::AxisFastPathConfig::default()
            },
            ..StitchConfig::default()
        };
        let mut dual_budget = SearchBudget::default();
        let dual_candidate = unwrap_candidate(estimate_motion_with_budget(
            &prev,
            &curr,
            Some(ScrollAxis::Vertical),
            (0, 84),
            &dual_config,
            &mut dual_budget,
        ));

        assert_eq!(fast_candidate.dx, 0);
        assert!(
            (fast_candidate.dy - 84).abs() <= 2,
            "dy = {} (expected ~84)",
            fast_candidate.dy
        );
        assert_eq!(fast_candidate.dx, dual_candidate.dx);
        assert_eq!(fast_candidate.dy, dual_candidate.dy);
        assert!(
            fast_budget.full_res_ncc_calls < dual_budget.full_res_ncc_calls,
            "fast budget = {fast_budget:?}, dual budget = {dual_budget:?}"
        );
    }
```

- [ ] **Step 4: Add an axis-change regression test**

In the matcher test module, add:

```rust
    #[test]
    fn locked_vertical_axis_change_still_returns_horizontal_candidate() {
        let canvas = make_wide_canvas(700, 180);
        let prev = crop_xy(&canvas, 0, 0, 180, 180);
        let curr = crop_xy(&canvas, 42, 0, 180, 180);

        let candidate = unwrap_candidate(estimate_motion(
            &prep(&prev),
            &prep(&curr),
            Some(ScrollAxis::Vertical),
            (0, 40),
            &StitchConfig::default(),
            &mut StitchMetrics::default(),
        ));

        assert_eq!(candidate.dy, 0);
        assert!(
            (candidate.dx - 42).abs() <= 2,
            "dx = {} (expected ~42)",
            candidate.dx
        );
    }
```

- [ ] **Step 5: Add a drift non-append regression test**

In the matcher test module, add:

```rust
    #[test]
    fn cross_axis_drift_does_not_accept_main_axis_fast_path() {
        let canvas = make_aperiodic_canvas(300, 420);
        let prev = crop_xy(&canvas, 0, 0, 200, 200);
        let curr = crop_xy(&canvas, 10, 48, 200, 200);
        let config = StitchConfig {
            axis_fast_path: crate::types::AxisFastPathConfig {
                cross_axis_probe_radius: 12,
                ..crate::types::AxisFastPathConfig::default()
            },
            ..StitchConfig::default()
        };

        let outcome = estimate_motion(
            &prep(&prev),
            &prep(&curr),
            Some(ScrollAxis::Vertical),
            (0, 48),
            &config,
            &mut StitchMetrics::default(),
        );

        assert!(
            matches!(outcome, MotionSearchOutcome::NoMatch { .. }),
            "cross-axis drift should not be accepted as a pure vertical append: {outcome:?}"
        );
    }
```

- [ ] **Step 6: Verify focused matcher tests**

Run:

```bash
rtk cargo test -p rollshot-core --lib matcher::tests::locked_vertical_uses_main_axis_fast_path matcher::tests::locked_vertical_axis_change_still_returns_horizontal_candidate matcher::tests::cross_axis_drift_does_not_accept_main_axis_fast_path
```

Expected: all three tests pass. If Cargo rejects multiple exact test names in one command, run them one at a time with the same `rtk cargo test -p rollshot-core --lib <test-name>` form.

- [ ] **Step 7: Verify the full matcher module**

Run:

```bash
rtk cargo test -p rollshot-core --lib matcher::tests
```

Expected: matcher tests pass.

- [ ] **Step 8: Commit fast-path wiring**

Run:

```bash
rtk git add crates/rollshot-core/src/matcher.rs
rtk git commit -m "feat(core): add locked-axis matcher fast path"
```

Expected: commit succeeds.

---

### Task 5: Run Core Regression and Benchmark Verification

**Files:**
- Local artifact: `bench-results/runs/p4-axis-fast-path/after.jsonl`
- Optional committed artifact: `bench-results/compare/2026-05-28-p4-axis-fast-path-compare.md`

- [ ] **Step 1: Run core unit and integration tests**

Run:

```bash
rtk cargo test -p rollshot-core
```

Expected: all `rollshot-core` tests pass.

- [ ] **Step 2: Run formatting check**

Run:

```bash
rtk cargo fmt --check
```

Expected: exits 0. If it fails, run `rtk cargo fmt`, inspect the diff, then rerun `rtk cargo fmt --check`.

- [ ] **Step 3: Run clippy for the touched crate**

Run:

```bash
rtk cargo clippy -p rollshot-core --all-targets -- -D warnings
```

Expected: exits 0.

- [ ] **Step 4: Capture the after benchmark JSONL**

Run:

```bash
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter,linear_horizontal_right \
    --repeats 3 \
    --out bench-results/runs/p4-axis-fast-path/after.jsonl
```

Expected: command exits 0 and writes `bench-results/runs/p4-axis-fast-path/after.jsonl`.

- [ ] **Step 5: Compare before vs after**

Run:

```bash
rtk python3 scripts/bench/compare.py \
    bench-results/runs/p4-axis-fast-path/before.jsonl \
    bench-results/runs/p4-axis-fast-path/after.jsonl
```

Expected:
- steady locked vertical fixtures show lower `ncc_offsets_scored` and lower `template_ncc_us` p95 or equivalent matcher latency metric.
- horizontal fixture does not regress materially.
- accepted/duplicate/no-match outcome counts remain unchanged unless a pre-existing fixture intentionally exercises cross-axis drift.

- [ ] **Step 6: Save a benchmark comparison report if the repo tracks compare reports**

If `bench-results/compare/` exists and is not gitignored, create `bench-results/compare/2026-05-28-p4-axis-fast-path-compare.md` with:

````markdown
---
scope: p4-axis-fast-path
before: bench-results/runs/p4-axis-fast-path/before.jsonl
after: bench-results/runs/p4-axis-fast-path/after.jsonl
---

# P4 Axis Fast Path Benchmark Compare

Command:

```bash
rtk python3 scripts/bench/compare.py \
    bench-results/runs/p4-axis-fast-path/before.jsonl \
    bench-results/runs/p4-axis-fast-path/after.jsonl
```

Summary:

- `long_vertical_text`: record p50/p95 total, template NCC timing, and NCC offsets delta from the compare output.
- `long_sticky_header`: record p50/p95 total, template NCC timing, and NCC offsets delta from the compare output.
- `long_vertical_jitter`: record p50/p95 total, template NCC timing, and NCC offsets delta from the compare output.
- `linear_horizontal_right`: record p50/p95 total and outcome-count parity from the compare output.

Outcome:

- Steady locked-axis fixtures reduce scored NCC offsets.
- Golden outcomes remain unchanged.
- Any latency regression above noise is listed here with the suspected cause.
```
````

Replace each summary bullet with the actual numbers from `compare.py` before committing. Do not commit the raw JSONL files.

- [ ] **Step 7: Commit benchmark report if created**

Run only if Step 6 created a tracked report:

```bash
rtk git add bench-results/compare/2026-05-28-p4-axis-fast-path-compare.md
rtk git commit -m "test(core): record p4 axis fast path benchmark"
```

Expected: commit succeeds.

---

### Task 6: Final Self-Review

**Files:**
- Inspect: `crates/rollshot-core/src/types.rs`
- Inspect: `crates/rollshot-core/src/matcher.rs`
- Inspect: optional benchmark report

- [ ] **Step 1: Confirm spec coverage**

Check:

```bash
rtk rg -n "axis_fast_path|cross_axis_check|axis_fast_path_candidate|fallback_to_dual_axis_on_suspicious" crates/rollshot-core/src
```

Expected:
- `AxisFastPathConfig` exists in `types.rs`.
- `axis_fast_path_candidate` exists in `matcher.rs`.
- `cross_axis_check` exists in `matcher.rs`.
- suspicious fallback reads `fallback_to_dual_axis_on_suspicious`.

- [ ] **Step 2: Confirm no search-axis regression**

Run:

```bash
rtk cargo test -p rollshot-core --lib matcher::tests::estimate_motion_finds_known_scroll matcher::tests::estimate_motion_finds_vertical_up_scroll matcher::tests::estimate_motion_finds_horizontal_right_scroll matcher::tests::estimate_motion_finds_horizontal_left_scroll
```

Expected: all tests pass. If Cargo rejects multiple exact test names, run each test separately.

- [ ] **Step 3: Confirm no incomplete markers in this plan-derived implementation**

Run:

```bash
rtk rg -n "TODO|TBD|implement later|placeholder" crates/rollshot-core/src/types.rs crates/rollshot-core/src/matcher.rs
```

Expected: no new incomplete marker text from this work. Pre-existing unrelated matches should be left alone and called out in the handoff.

- [ ] **Step 4: Confirm final workspace diff is scoped**

Run:

```bash
rtk git diff --stat HEAD
```

Expected: only intended files are modified after the last commit, or no output if all task commits were made.

---

## Plan Self-Review

- Spec coverage: P4 requires locked-axis main path, cross-axis sentinel, fallback to old dual-axis search, no regression for axis changed / cross-axis too large behavior, and lower steady-state scored offsets. Tasks 1-5 cover those requirements.
- Placeholder scan: The implementation tasks use exact file paths, commands, and code blocks. The only intentionally variable section is the benchmark report, where the executor must paste actual measured numbers from `compare.py` before committing.
- Type consistency: The plan uses `ScrollAxis`, `SearchAxis`, `MotionCandidate`, `PreparedFrame`, `StitchConfig`, and `StitchMetrics` exactly as they exist in the current code. New public config type is `AxisFastPathConfig`; new private matcher types are `CrossAxisCheck` and `axis_fast_path_candidate`.
- Scope check: This is one subsystem, `rollshot-core` matcher optimization. It does not require app, CLI, capture, canvas, or verifier changes.
