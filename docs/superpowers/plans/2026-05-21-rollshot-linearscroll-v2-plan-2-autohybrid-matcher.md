# Rollshot LinearScroll v2 Plan 2: AutoHybrid Matcher Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Evolve `rollshot-core` from the Plan 1 vertical-only template wrapper into the default non-AKAZE `AutoHybrid` matcher that finds verified vertical and horizontal linear scroll motion.

**Architecture:** Keep all Plan 2 matcher work inside `rollshot-core` and avoid new dependencies. `matcher.rs` becomes a small internal pipeline: generate coarse 2D, axis-aware template, and edge-projection candidates; verify candidates with `PixelOverlapVerifier`; rank the verified candidates by normalized confidence and second-best margin; return one `MotionCandidate` to `Stitcher`. `Stitcher` passes the locked axis and last accepted motion into the matcher, then keeps using Plan 1 axis validation, verifier, and `LinearCanvas` append behavior.

```text
estimate_motion(prev, curr, locked_axis, last_motion, config)
   |
   |-- prev_gray, curr_gray = to_grayscale(prev/curr)   // computed ONCE
   |
   |    parallel fan-out (not a fallback chain):
   |-- coarse_candidates(prev_gray, curr_gray, ...)     // best (dx, dy) on stepped grid
   |-- template_candidates(prev_gray, curr_gray, ...)   // axis-aware NCC search
   |-- edge_projection_candidates(prev_gray, curr_gray, ...) // 1-D MAD on edges
   |
   v
rank_verified_candidates(prev, curr, candidates, ...)
   |
   |-- filter: accept_confidence, second_best_margin, axis lock
   |-- verify: PixelOverlapVerifier per surviving candidate
   |-- combine score + verifier MAD
   |
   v
Option<MotionCandidate> -> Stitcher
```

Design note: the v0.2 spec lists Coarse / Template / Edge / AKAZE as a numbered pipeline, but explicitly allows the matcher to "contain multiple internal candidate generators". Plan 2 picks parallel fan-out because all three non-AKAZE generators are cheap relative to the verifier and the ranker needs cross-method scoring anyway. AKAZE in Plan 3 will keep its "only run on weak top-candidate" budget by feeding the same ranker.

**Tech Stack:** Rust 2021, `image` 0.25 (`RgbaImage`), existing `rollshot-core` modules (`axis`, `overlap`, `verifier`, `canvas`), deterministic synthetic tests. No AKAZE dependency and no new CLI flags in this plan.

---

## Assumptions

- Plan 1 has landed: `MotionCandidate`, `MotionEstimate`, `ScrollAxis`, `AppendDirection`, `MatchMethod`, `LinearCanvas`, `compute_overlap`, and `PixelOverlapVerifier` already exist.
- `MotionCandidate.score` is treated as normalized confidence where lower is better, in roughly `[0.0, 1.0]`. Plan 2 makes every candidate generator follow that convention (Template uses `1.0 - NCC.clamp(0, 1)`; Coarse and Edge use normalized MAD).
- `MotionCandidate.second_best_score` is also normalized lower-is-better confidence. The second-best margin check is `second_best_score - score >= config.second_best_margin` (i.e. the runner-up must be measurably *worse*, meaning higher score).
- `match_width` (and its derived ROI) was sized for vertical scroll. Plan 2 reuses it unchanged for horizontal search; the synthetic fixtures have `match_width >= roi.w` so the ROI collapses to "full content rectangle" and the asymmetry is inert. TODO (future tuning, not in this plan): introduce an axis-aware band ROI for horizontal scroll on wide content.
- AKAZE, golden fixtures, debug match reports, and CI feature work stay in Plan 3. That includes targeted fixtures where the Coarse and Edge candidate paths *win* over Template; in Plan 2 they are tested as fallbacks-of-last-resort that produce verified candidates, but Template is expected to win on the synthetic textures used here.

## File Structure

- Modify: `crates/rollshot-core/src/matcher.rs`
  Replace the vertical-only wrapper with the non-AKAZE `AutoHybrid` pipeline: shared grayscale/ROI helpers, coarse 2D candidate generation, axis-aware template generation, edge-projection generation, verifier-backed ranking, and unit tests.
- Modify: `crates/rollshot-core/src/stitcher.rs`
  Track the last accepted `dx` and `dy`, pass `locked_axis` into `matcher::estimate_motion`, and update the last motion after successful append.
- Modify: `crates/rollshot-core/tests/stitcher.rs`
  Add integration coverage for vertical up, horizontal right, horizontal left, axis changes, repeated-content rejection, and bad-frame anchor preservation through the new matcher.
- Modify: `crates/rollshot-core/tests/common/mod.rs`
  Add deterministic repeated-grid and repeated-row fixtures used by matcher and stitcher tests.
- No changes: `crates/rollshot-cli/*`
  The CLI still consumes `StitchOutcome`; no Plan 2 user-facing controls are added.

---

## Task 1: Add Failing Matcher Tests For Plan 2 Behavior

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`

- [ ] **Step 1: Add the missing matcher test helpers**

In `crates/rollshot-core/src/matcher.rs`, inside the existing `#[cfg(test)] mod tests`, extend the `use` list and add horizontal/repeated fixture helpers:

```rust
use crate::types::{MatchMethod, ScrollAxis, StitchConfig};
use image::{imageops, Rgba, RgbaImage};

fn make_wide_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([240, 240, 240, 255]));
    for x in (0..width).step_by(11) {
        let accent = ((x / 3) % 180) as u8;
        for y in 8..height.saturating_sub(8) {
            let stripe = if (x / 5 + y / 7) % 2 == 0 { 220 } else { 180 };
            img.put_pixel(x, y, Rgba([stripe, accent, 80, 255]));
            if x + 1 < width {
                img.put_pixel(x + 1, y, Rgba([30, 30, 30, 255]));
            }
        }
    }
    for row in [21u32, 47, 73, 99, 125] {
        if row >= height {
            continue;
        }
        for x in 12..width.saturating_sub(12) {
            if (x / 13) % 3 != 0 {
                img.put_pixel(x, row, Rgba([20, 20, 20, 255]));
            }
        }
    }
    img
}

fn make_repeated_grid(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([245, 245, 245, 255]));
    for y in 0..height {
        for x in 0..width {
            let v = if (x / 16 + y / 16) % 2 == 0 { 48 } else { 208 };
            img.put_pixel(x, y, Rgba([v, v, v, 255]));
        }
    }
    img
}

fn crop_xy(canvas: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
    imageops::crop_imm(canvas, x, y, w, h).to_image()
}
```

- [ ] **Step 2: Add tests for all four non-AKAZE motion directions**

Add these tests to the same test module:

```rust
#[test]
fn estimate_motion_finds_vertical_up_scroll() {
    let canvas = make_textured_canvas(160, 700);
    let prev = crop(&canvas, 220, 160);
    let curr = crop(&canvas, 180, 160);

    let candidate = estimate_motion(
        &prev,
        &curr,
        None,
        (0, 0),
        &StitchConfig::default(),
    )
    .expect("template candidate");

    assert_eq!(candidate.method, MatchMethod::Template);
    assert_eq!(candidate.dx, 0);
    assert!(
        (candidate.dy + 40).abs() <= 2,
        "dy = {} (expected ~-40)",
        candidate.dy
    );
}

#[test]
fn estimate_motion_finds_horizontal_right_scroll() {
    let canvas = make_wide_canvas(700, 160);
    let prev = crop_xy(&canvas, 0, 0, 160, 160);
    let curr = crop_xy(&canvas, 40, 0, 160, 160);

    let candidate = estimate_motion(
        &prev,
        &curr,
        None,
        (0, 0),
        &StitchConfig::default(),
    )
    .expect("horizontal candidate");

    assert_eq!(candidate.dy, 0);
    assert!(
        (candidate.dx - 40).abs() <= 2,
        "dx = {} (expected ~40)",
        candidate.dx
    );
}

#[test]
fn estimate_motion_finds_horizontal_left_scroll() {
    let canvas = make_wide_canvas(700, 160);
    let prev = crop_xy(&canvas, 220, 0, 160, 160);
    let curr = crop_xy(&canvas, 180, 0, 160, 160);

    let candidate = estimate_motion(
        &prev,
        &curr,
        Some(ScrollAxis::Horizontal),
        (40, 0),
        &StitchConfig::default(),
    )
    .expect("horizontal candidate");

    assert_eq!(candidate.dy, 0);
    assert!(
        (candidate.dx + 40).abs() <= 2,
        "dx = {} (expected ~-40)",
        candidate.dx
    );
}
```

- [ ] **Step 3: Add tests for axis hinting and repeated-content rejection**

Add:

```rust
#[test]
fn locked_vertical_hint_rejects_horizontal_candidate() {
    let canvas = make_wide_canvas(700, 160);
    let prev = crop_xy(&canvas, 0, 0, 160, 160);
    let curr = crop_xy(&canvas, 40, 0, 160, 160);

    let candidate = estimate_motion(
        &prev,
        &curr,
        Some(ScrollAxis::Vertical),
        (0, 40),
        &StitchConfig::default(),
    );

    assert!(candidate.is_none());
}

#[test]
fn repeated_grid_is_rejected_by_second_best_margin() {
    let canvas = make_repeated_grid(240, 560);
    let prev = crop_xy(&canvas, 0, 0, 160, 160);
    let curr = crop_xy(&canvas, 0, 32, 160, 160);

    let candidate = estimate_motion(
        &prev,
        &curr,
        None,
        (0, 0),
        &StitchConfig::default(),
    );

    assert!(candidate.is_none());
}
```

- [ ] **Step 4: Run matcher tests and confirm they fail for the expected reason**

Run:

```bash
rtk cargo test -p rollshot-core matcher:: -- --nocapture
```

Expected: tests do not pass yet. Initially the compile fails because `estimate_motion` still takes `(prev, curr, last_offset, config)` instead of `(prev, curr, locked_axis, last_motion, config)`. After Task 2 updates the signature, the new directional / locked-axis / repeated-grid tests should still fail because the multi-generator pipeline (template, coarse, edge) is not implemented yet. Either failure mode is acceptable here — the gate is "these tests are red".

- [ ] **Step 5: Commit the failing tests**

```bash
git add crates/rollshot-core/src/matcher.rs
git commit -m "test(core): cover autohybrid matcher directions"
```

---

## Task 2: Add Matcher Pipeline And Verified Ranking

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`

- [ ] **Step 1: Update matcher imports**

Replace the current import block with:

```rust
use image::{Rgba, RgbaImage};

use crate::axis::{classify_axis, validate_with_lock, AxisClassification, AxisValidation};
use crate::overlap::compute_overlap;
use crate::types::{
    MatchMethod, MotionCandidate, ScrollAxis, StitchConfig,
};
use crate::verifier::{PixelOverlapVerifier, VerifierOutcome};
```

- [ ] **Step 2: Add shared matcher structs and constants**

Keep the existing ROI constants, then replace `VerticalTemplateEstimate` with:

```rust
const COARSE_DOWNSAMPLE_STEP: u32 = 4;
const EDGE_PROJECTION_STEP: u32 = 2;

#[derive(Clone, Copy)]
struct Region {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

#[derive(Debug, Clone, Copy)]
struct CandidateScore {
    candidate: MotionCandidate,
    verifier_score: f32,
}

#[derive(Debug, Clone, Copy)]
enum SearchAxis {
    Vertical,
    Horizontal,
}
```

- [ ] **Step 3: Replace `estimate_motion` with the AutoHybrid entrypoint**

Replace the current public `estimate_motion` function with:

```rust
pub fn estimate_motion(
    prev: &RgbaImage,
    curr: &RgbaImage,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    config: &StitchConfig,
) -> Option<MotionCandidate> {
    if prev.dimensions() != curr.dimensions() {
        return None;
    }

    // Grayscale is the only buffer every generator needs. Compute it once and
    // thread `&[f32]` through the pipeline so we don't allocate `2 * 4 * W * H`
    // bytes three times per frame.
    let width = prev.width();
    let height = prev.height();
    let prev_gray = to_grayscale(prev);
    let curr_gray = to_grayscale(curr);

    let mut candidates = Vec::new();
    candidates.extend(coarse_candidates(
        &prev_gray,
        &curr_gray,
        width,
        height,
        locked_axis,
        config,
    ));
    candidates.extend(template_candidates(
        &prev_gray,
        &curr_gray,
        width,
        height,
        locked_axis,
        last_motion,
        config,
    ));
    candidates.extend(edge_projection_candidates(
        &prev_gray,
        &curr_gray,
        width,
        height,
        locked_axis,
        config,
    ));

    rank_verified_candidates(prev, curr, locked_axis, candidates, config)
}
```

Note: only `rank_verified_candidates` keeps the `&RgbaImage` references, because it hands the original frames to `PixelOverlapVerifier` (which samples RGBA pixels through `image::RgbaImage::get_pixel` for the full-res band).

- [ ] **Step 4: Add axis and second-best filtering helpers**

Add these helpers below `estimate_motion`:

```rust
fn rank_verified_candidates(
    prev: &RgbaImage,
    curr: &RgbaImage,
    locked_axis: Option<ScrollAxis>,
    candidates: Vec<MotionCandidate>,
    config: &StitchConfig,
) -> Option<MotionCandidate> {
    let verifier = PixelOverlapVerifier::new(&config.verifier, config.min_overlap);
    let mut scored = Vec::new();

    for mut candidate in candidates {
        if candidate.score > config.accept_confidence {
            continue;
        }
        if !passes_second_best_margin(&candidate, config.second_best_margin) {
            continue;
        }
        if !candidate_matches_axis(candidate.dx, candidate.dy, locked_axis, config) {
            continue;
        }

        let verifier_score = match verifier.verify(prev, curr, &candidate) {
            VerifierOutcome::Pass { score, .. } => score,
            VerifierOutcome::InsufficientOverlap
            | VerifierOutcome::OverlapDisagreement { .. } => continue,
        };

        candidate.score = (candidate.score + verifier_score * 0.5).clamp(0.0, 1.0);
        scored.push(CandidateScore {
            candidate,
            verifier_score,
        });
    }

    scored.sort_by(|a, b| {
        a.candidate
            .score
            .total_cmp(&b.candidate.score)
            .then(a.verifier_score.total_cmp(&b.verifier_score))
    });

    scored.first().map(|s| s.candidate)
}

fn passes_second_best_margin(candidate: &MotionCandidate, margin: f32) -> bool {
    match candidate.second_best_score {
        Some(second) => second - candidate.score >= margin,
        None => true,
    }
}

fn candidate_matches_axis(
    dx: i32,
    dy: i32,
    locked_axis: Option<ScrollAxis>,
    config: &StitchConfig,
) -> bool {
    match locked_axis {
        None => !matches!(
            classify_axis(dx, dy, config.axis_ratio_threshold),
            AxisClassification::Ambiguous
        ),
        Some(axis) => matches!(
            validate_with_lock(axis, dx, dy, config.max_cross_axis_px),
            AxisValidation::OnAxis { .. }
        ),
    }
}

fn candidate(
    dx: i32,
    dy: i32,
    method: MatchMethod,
    score: f32,
    second_best_score: Option<f32>,
) -> MotionCandidate {
    MotionCandidate {
        dx,
        dy,
        method,
        score,
        second_best_score,
        inliers: None,
        raw_matches: None,
    }
}
```

- [ ] **Step 5: Run the compiler-focused matcher test**

Run:

```bash
rtk cargo test -p rollshot-core matcher::estimate_motion_returns_none_for_dimension_mismatch
```

Expected: compile still fails because `coarse_candidates`, `template_candidates`, and `edge_projection_candidates` are not defined yet.

- [ ] **Step 6: Commit the pipeline skeleton**

```bash
git add crates/rollshot-core/src/matcher.rs
git commit -m "refactor(core): add autohybrid matcher pipeline"
```

---

## Task 3: Implement Axis-Aware Template Candidates

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`

- [ ] **Step 1: Add axis selection and prediction helpers**

Add:

```rust
fn search_axes(locked_axis: Option<ScrollAxis>) -> &'static [SearchAxis] {
    match locked_axis {
        Some(ScrollAxis::Vertical) => &[SearchAxis::Vertical],
        Some(ScrollAxis::Horizontal) => &[SearchAxis::Horizontal],
        None => &[SearchAxis::Vertical, SearchAxis::Horizontal],
    }
}

fn predicted_offset(axis: SearchAxis, last_motion: (i32, i32)) -> i32 {
    match axis {
        SearchAxis::Vertical => last_motion.1,
        SearchAxis::Horizontal => last_motion.0,
    }
}
```

- [ ] **Step 2: Add `template_candidates`**

Add:

```rust
fn template_candidates(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    config: &StitchConfig,
) -> Vec<MotionCandidate> {
    let mut out = Vec::new();
    let roi = content_roi(width, height);
    let match_region = match_width_region(roi, config.match_width);

    for axis in search_axes(locked_axis) {
        if let Some(candidate) = search_template_axis(
            prev_gray,
            curr_gray,
            width,
            height,
            *axis,
            match_region,
            predicted_offset(*axis, last_motion),
            config,
        ) {
            out.push(candidate);
        }
    }

    out
}
```

- [ ] **Step 3: Add the signed template search implementation**

Add:

```rust
fn search_template_axis(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    axis: SearchAxis,
    region: Region,
    last_offset: i32,
    config: &StitchConfig,
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

    let mut best_offset = 0i32;
    let mut best_score = f32::MIN;
    let mut second_score = f32::MIN;

    for offset in signed_predict_iter(max_offset, last_offset) {
        let score = match axis {
            SearchAxis::Vertical => ncc_score_shifted(
                prev_gray,
                curr_gray,
                width,
                height,
                region,
                0,
                offset,
            ),
            SearchAxis::Horizontal => ncc_score_shifted(
                prev_gray,
                curr_gray,
                width,
                height,
                region,
                offset,
                0,
            ),
        };

        if score > best_score {
            second_score = best_score;
            best_score = score;
            best_offset = offset;
        } else if score > second_score {
            second_score = score;
        }
    }

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

- [ ] **Step 4: Replace one-sided prediction with signed prediction**

Replace `predict_iter` with:

```rust
fn signed_predict_iter(max_abs: i32, predict: i32) -> Vec<i32> {
    let p = predict.clamp(-max_abs, max_abs);
    let mut out = Vec::with_capacity((max_abs as usize).saturating_mul(2) + 1);
    out.push(p);
    for delta in 1..=max_abs {
        if p + delta <= max_abs {
            out.push(p + delta);
        }
        if p - delta >= -max_abs {
            out.push(p - delta);
        }
    }
    out
}
```

- [ ] **Step 5: Add shifted NCC scoring**

Keep `to_grayscale`; replace `ncc_score_region` and `overlap_mean_abs_diff` with this generic shifted scorer:

```rust
fn ncc_score_shifted(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    region: Region,
    dx: i32,
    dy: i32,
) -> f32 {
    let overlap = match compute_overlap(width, height, width, height, dx, dy) {
        Some(overlap) => overlap,
        None => return f32::MIN,
    };
    let x0 = region.x.max(overlap.prev_x);
    let y0 = region.y.max(overlap.prev_y);
    let x1 = (region.x + region.w).min(overlap.prev_x + overlap.width);
    let y1 = (region.y + region.h).min(overlap.prev_y + overlap.height);
    if x1 <= x0 || y1 <= y0 {
        return f32::MIN;
    }

    let mut prev_sum = 0.0f32;
    let mut curr_sum = 0.0f32;
    let mut count = 0usize;
    for prev_y in y0..y1 {
        for prev_x in x0..x1 {
            let curr_x = (prev_x as i32 - dx) as u32;
            let curr_y = (prev_y as i32 - dy) as u32;
            let prev_idx = (prev_y * width + prev_x) as usize;
            let curr_idx = (curr_y * width + curr_x) as usize;
            prev_sum += prev_gray[prev_idx];
            curr_sum += curr_gray[curr_idx];
            count += 1;
        }
    }
    if count == 0 {
        return f32::MIN;
    }

    let prev_mean = prev_sum / count as f32;
    let curr_mean = curr_sum / count as f32;
    let mut num = 0.0f32;
    let mut prev_var = 0.0f32;
    let mut curr_var = 0.0f32;
    for prev_y in y0..y1 {
        for prev_x in x0..x1 {
            let curr_x = (prev_x as i32 - dx) as u32;
            let curr_y = (prev_y as i32 - dy) as u32;
            let prev_idx = (prev_y * width + prev_x) as usize;
            let curr_idx = (curr_y * width + curr_x) as usize;
            let p = prev_gray[prev_idx] - prev_mean;
            let c = curr_gray[curr_idx] - curr_mean;
            num += p * c;
            prev_var += p * p;
            curr_var += c * c;
        }
    }

    if prev_var <= 1.0 || curr_var <= 1.0 {
        return f32::MIN;
    }
    num / (prev_var.sqrt() * curr_var.sqrt())
}
```

- [ ] **Step 6: Run template-focused matcher tests**

Run:

```bash
rtk cargo test -p rollshot-core matcher::estimate_motion_finds_known_scroll matcher::estimate_motion_finds_vertical_up_scroll matcher::estimate_motion_finds_horizontal_right_scroll matcher::estimate_motion_finds_horizontal_left_scroll
```

Expected: direction tests pass or only repeated-content tests remain failing because coarse/edge generators are still absent.

- [ ] **Step 7: Commit template matching**

```bash
git add crates/rollshot-core/src/matcher.rs
git commit -m "feat(core): add axis-aware template matching"
```

---

## Task 4: Implement Coarse 2D And Edge Projection Candidates

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`

- [ ] **Step 1: Add coarse 2D candidate generation**

Performance note: with `max_search_ratio = 0.75` and an unlocked axis (only happens on frame 2), the dense `(dx, dy)` grid contains up to `(121 * 121)` candidates for a 320-square frame. `coarse_mad` itself steps with `COARSE_DOWNSAMPLE_STEP`, so each candidate samples ~`(overlap_w / 4) * (overlap_h / 4)` cells. Total work is bounded at ~50M ops on frame 2 (one-shot) and ~5M ops on subsequent frames (axis locked → `dx` constrained to `±max_cross_axis_px`). Acceptable for Plan 2. If profiling later shows this is the matcher's hot spot, the follow-up is to build a true ¼-resolution image once and exhaustively search there (Plan 3 or later, not in scope here).

Add:

```rust
fn coarse_candidates(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    locked_axis: Option<ScrollAxis>,
    config: &StitchConfig,
) -> Vec<MotionCandidate> {
    let max_dx = (width as f32 * config.max_search_ratio) as i32;
    let max_dy = (height as f32 * config.max_search_ratio) as i32;
    let step = COARSE_DOWNSAMPLE_STEP as i32;
    let mut scored = Vec::new();

    let dx_values: Vec<i32> = match locked_axis {
        Some(ScrollAxis::Vertical) => (-config.max_cross_axis_px..=config.max_cross_axis_px).collect(),
        _ => ((-max_dx / step)..=(max_dx / step)).map(|n| n * step).collect(),
    };
    let dy_values: Vec<i32> = match locked_axis {
        Some(ScrollAxis::Horizontal) => (-config.max_cross_axis_px..=config.max_cross_axis_px).collect(),
        _ => ((-max_dy / step)..=(max_dy / step)).map(|n| n * step).collect(),
    };

    for dy in dy_values {
        for dx in dx_values.iter().copied() {
            if dx == 0 && dy == 0 {
                continue;
            }
            if !candidate_matches_axis(dx, dy, locked_axis, config) {
                continue;
            }
            let diff = coarse_mad(
                prev_gray,
                curr_gray,
                width,
                height,
                dx,
                dy,
                COARSE_DOWNSAMPLE_STEP,
            );
            if diff.is_finite() {
                scored.push((diff, dx, dy));
            }
        }
    }

    scored.sort_by(|a, b| a.0.total_cmp(&b.0));

    // Only emit the top-1 coarse candidate. Earlier drafts emitted the top-4,
    // but `passes_second_best_margin` would always reject candidates 2..4
    // (their `second_best_score = scored[0].0` is *lower* than their own
    // score, so the margin check is always negative). Emitting them was dead
    // code that wasted verifier cycles.
    let (best_score, best_dx, best_dy) = match scored.first() {
        Some(t) => *t,
        None => return Vec::new(),
    };
    let second = scored.get(1).map(|(score, _, _)| *score);
    vec![candidate(
        best_dx,
        best_dy,
        MatchMethod::Coarse,
        best_score,
        second,
    )]
}

fn coarse_mad(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    dx: i32,
    dy: i32,
    step: u32,
) -> f32 {
    let overlap = match compute_overlap(width, height, width, height, dx, dy) {
        Some(overlap) => overlap,
        None => return f32::INFINITY,
    };

    let mut sum = 0.0f32;
    let mut count = 0u32;
    let mut y = 0;
    while y < overlap.height {
        let mut x = 0;
        while x < overlap.width {
            let prev_idx = ((overlap.prev_y + y) * width + overlap.prev_x + x) as usize;
            let curr_idx = ((overlap.curr_y + y) * width + overlap.curr_x + x) as usize;
            sum += (prev_gray[prev_idx] - curr_gray[curr_idx]).abs();
            count += 1;
            x += step.max(1);
        }
        y += step.max(1);
    }
    if count == 0 {
        return f32::INFINITY;
    }
    sum / (count as f32 * 255.0)
}
```

- [ ] **Step 2: Add edge projection candidate generation**

Add:

```rust
fn edge_projection_candidates(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    height: u32,
    locked_axis: Option<ScrollAxis>,
    config: &StitchConfig,
) -> Vec<MotionCandidate> {
    let mut out = Vec::new();

    for axis in search_axes(locked_axis) {
        if let Some(candidate) = edge_projection_axis(
            prev_gray,
            curr_gray,
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

fn edge_projection_axis(
    prev_gray: &[f32],
    curr_gray: &[f32],
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

    let prev_proj = edge_projection(prev_gray, width, height, axis);
    let curr_proj = edge_projection(curr_gray, width, height, axis);
    let mut scored = Vec::new();
    for offset in signed_predict_iter(max_offset, 0) {
        let score = projection_mad(&prev_proj, &curr_proj, offset, EDGE_PROJECTION_STEP as usize);
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

fn edge_projection(gray: &[f32], width: u32, height: u32, axis: SearchAxis) -> Vec<f32> {
    match axis {
        SearchAxis::Vertical => {
            let mut rows = vec![0.0; height as usize];
            for y in 1..height {
                let mut sum = 0.0;
                for x in 0..width {
                    let idx = (y * width + x) as usize;
                    let prev = ((y - 1) * width + x) as usize;
                    sum += (gray[idx] - gray[prev]).abs();
                }
                rows[y as usize] = sum / width.max(1) as f32 / 255.0;
            }
            rows
        }
        SearchAxis::Horizontal => {
            let mut cols = vec![0.0; width as usize];
            for x in 1..width {
                let mut sum = 0.0;
                for y in 0..height {
                    let idx = (y * width + x) as usize;
                    let prev = (y * width + x - 1) as usize;
                    sum += (gray[idx] - gray[prev]).abs();
                }
                cols[x as usize] = sum / height.max(1) as f32 / 255.0;
            }
            cols
        }
    }
}

fn projection_mad(prev: &[f32], curr: &[f32], offset: i32, step: usize) -> f32 {
    let prev_start = offset.max(0) as usize;
    let curr_start = (-offset).max(0) as usize;
    let overlap = prev
        .len()
        .min(curr.len())
        .saturating_sub(offset.unsigned_abs() as usize);
    if overlap == 0 {
        return f32::INFINITY;
    }

    let step = step.max(1);
    let mut sum = 0.0f32;
    let mut count = 0usize;
    for i in (0..overlap).step_by(step) {
        sum += (prev[prev_start + i] - curr[curr_start + i]).abs();
        count += 1;
    }
    if count == 0 {
        return f32::INFINITY;
    }
    sum / count as f32
}
```

- [ ] **Step 3: Run all matcher tests**

Run:

```bash
rtk cargo test -p rollshot-core matcher:: -- --nocapture
```

Expected: all matcher tests pass, including repeated-grid rejection. If repeated-grid is still accepted, tighten only the second-best margin logic by requiring `second_best_score` for `MatchMethod::Template` and `MatchMethod::Edge`; do not tune verifier thresholds globally.

- [ ] **Step 4: Commit coarse and edge candidates**

```bash
git add crates/rollshot-core/src/matcher.rs
git commit -m "feat(core): add coarse and edge motion candidates"
```

---

## Task 5: Wire Locked-Axis Matching Through `Stitcher`

**Files:**
- Modify: `crates/rollshot-core/src/stitcher.rs`

- [ ] **Step 1: Replace `last_offset` with 2D last motion**

In `Stitcher`, replace:

```rust
last_offset: i32,
```

with:

```rust
last_motion: (i32, i32),
```

In `Stitcher::new`, replace:

```rust
last_offset: 0,
```

with:

```rust
last_motion: (0, 0),
```

- [ ] **Step 2: Pass axis and last motion into the matcher**

Replace the matcher call in `push_frame`:

```rust
let candidate = match estimate_motion(anchor, &frame, self.last_offset, &self.config) {
```

with:

```rust
let candidate = match estimate_motion(
    anchor,
    &frame,
    self.locked_axis,
    self.last_motion,
    &self.config,
) {
```

- [ ] **Step 3: Store the accepted 2D motion**

Replace:

```rust
self.last_offset = candidate.dy;
```

with:

```rust
self.last_motion = (candidate.dx, candidate.dy);
```

- [ ] **Step 4: Run stitcher compile tests**

Run:

```bash
rtk cargo test -p rollshot-core stitcher::first_frame_initializes_stitched_image
```

Expected: pass.

- [ ] **Step 5: Commit stitcher integration**

```bash
git add crates/rollshot-core/src/stitcher.rs
git commit -m "refactor(core): pass locked axis to matcher"
```

---

## Task 6: Add Stitcher Integration Tests For AutoHybrid

**Files:**
- Modify: `crates/rollshot-core/tests/common/mod.rs`
- Modify: `crates/rollshot-core/tests/stitcher.rs`

- [ ] **Step 1: Add repeated-content fixture helpers**

Append to `crates/rollshot-core/tests/common/mod.rs`:

```rust
/// Builds deliberately ambiguous repeated rows. Multiple offsets look equally
/// plausible, so Plan 2 should reject them without AKAZE.
pub fn make_repeated_rows(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([250, 250, 250, 255]));
    for y in 0..height {
        let band = (y / 16) % 2;
        let color = if band == 0 {
            Rgba([40, 40, 40, 255])
        } else {
            Rgba([210, 210, 210, 255])
        };
        for x in 0..width {
            img.put_pixel(x, y, color);
        }
    }
    img
}
```

- [ ] **Step 2: Update stitcher test imports**

In `crates/rollshot-core/tests/stitcher.rs`, change:

```rust
use common::{crop_frame, make_scroll_canvas, paint_sticky_header};
```

to:

```rust
use common::{
    crop_frame, crop_frame_xy, make_repeated_rows, make_scroll_canvas, make_wide_canvas,
    paint_sticky_header,
};
```

- [ ] **Step 3: Add vertical-up stitcher integration test**

Add:

```rust
#[test]
fn vertical_up_scroll_prepends_top() {
    let canvas = make_scroll_canvas(320, 1400);
    let first = crop_frame(&canvas, 800, 320);
    let scrolled = crop_frame(&canvas, 720, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended {
            direction,
            added,
            estimate,
        } => {
            assert_eq!(direction, AppendDirection::Top);
            assert_eq!(estimate.axis, ScrollAxis::Vertical);
            assert!(estimate.dy < 0, "dy = {}", estimate.dy);
            assert!((76..=84).contains(&added), "added = {added}");
        }
        other => panic!("expected top append, got {other:?}"),
    }

    assert_eq!(stitcher.stats().total_width, 320);
    assert!(stitcher.stats().total_height > 320);
}
```

- [ ] **Step 4: Add horizontal stitcher integration tests**

Add:

```rust
#[test]
fn horizontal_right_scroll_appends_right() {
    let canvas = make_wide_canvas(1400, 320);
    let first = crop_frame_xy(&canvas, 0, 0, 320, 320);
    let scrolled = crop_frame_xy(&canvas, 80, 0, 320, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended {
            direction,
            added,
            estimate,
        } => {
            assert_eq!(direction, AppendDirection::Right);
            assert_eq!(estimate.axis, ScrollAxis::Horizontal);
            assert!(estimate.dx > 0, "dx = {}", estimate.dx);
            assert!((76..=84).contains(&added), "added = {added}");
        }
        other => panic!("expected right append, got {other:?}"),
    }

    assert!(stitcher.stats().total_width > 320);
    assert_eq!(stitcher.stats().total_height, 320);
}

#[test]
fn horizontal_left_scroll_prepends_left() {
    let canvas = make_wide_canvas(1400, 320);
    let first = crop_frame_xy(&canvas, 800, 0, 320, 320);
    let scrolled = crop_frame_xy(&canvas, 720, 0, 320, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended {
            direction,
            added,
            estimate,
        } => {
            assert_eq!(direction, AppendDirection::Left);
            assert_eq!(estimate.axis, ScrollAxis::Horizontal);
            assert!(estimate.dx < 0, "dx = {}", estimate.dx);
            assert!((76..=84).contains(&added), "added = {added}");
        }
        other => panic!("expected left append, got {other:?}"),
    }

    assert!(stitcher.stats().total_width > 320);
    assert_eq!(stitcher.stats().total_height, 320);
}
```

- [ ] **Step 5: Add axis-change and repeated-content integration tests**

Add:

```rust
#[test]
fn horizontal_after_vertical_lock_is_rejected_as_axis_change() {
    let vertical = make_scroll_canvas(320, 1200);
    let first = crop_frame(&vertical, 0, 320);
    let down = crop_frame(&vertical, 80, 320);
    let horizontal = make_wide_canvas(1400, 320);
    let right = crop_frame_xy(&horizontal, 160, 0, 320, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);
    assert!(matches!(
        stitcher.push_frame(down),
        StitchOutcome::Appended {
            direction: AppendDirection::Bottom,
            ..
        }
    ));

    match stitcher.push_frame(right) {
        StitchOutcome::NoMatch {
            reason: NoMatchReason::LowConfidence | NoMatchReason::CrossAxisTooLarge,
            ..
        }
        | StitchOutcome::AxisChanged {
            previous_axis: ScrollAxis::Vertical,
            new_axis: ScrollAxis::Horizontal,
            ..
        } => {}
        other => panic!("expected horizontal frame rejected after vertical lock, got {other:?}"),
    }

    assert_eq!(stitcher.stats().frame_count, 2);
}

#[test]
fn repeated_rows_do_not_append_without_clear_match() {
    let canvas = make_repeated_rows(320, 1000);
    let first = crop_frame(&canvas, 0, 320);
    let repeated = crop_frame(&canvas, 32, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(repeated) {
        StitchOutcome::NoMatch { reason, .. } => {
            assert!(matches!(
                reason,
                NoMatchReason::LowConfidence
                    | NoMatchReason::AmbiguousAxis
                    | NoMatchReason::OverlapVerificationFailed
            ));
        }
        other => panic!("expected repeated rows to be rejected, got {other:?}"),
    }

    assert_eq!(stitcher.stats().frame_count, 1);
    assert_eq!(stitcher.stats().total_height, 320);
}
```

- [ ] **Step 6: Run stitcher integration tests**

Run:

```bash
rtk cargo test -p rollshot-core --test stitcher -- --nocapture
```

Expected: all stitcher integration tests pass.

- [ ] **Step 7: Commit integration tests**

```bash
git add crates/rollshot-core/tests/common/mod.rs crates/rollshot-core/tests/stitcher.rs
git commit -m "test(core): cover autohybrid stitching directions"
```

---

## Task 7: Final Verification And Cleanup

**Files:**
- Modify only files touched by earlier tasks if verification exposes compile, clippy, or formatting issues.

- [ ] **Step 1: Run formatter**

Run:

```bash
rtk cargo fmt
```

Expected: command exits 0.

- [ ] **Step 2: Run core tests**

Run:

```bash
rtk cargo test -p rollshot-core
```

Expected: all `rollshot-core` unit and integration tests pass.

- [ ] **Step 3: Run workspace tests**

Run:

```bash
rtk cargo test --workspace
```

Expected: all workspace tests pass. If CLI tests fail because matcher behavior changed an expected stitched size by a few pixels, update only the assertion range in that CLI test and keep the user-facing behavior unchanged.

- [ ] **Step 4: Run clippy**

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: clippy exits 0.

- [ ] **Step 5: Run formatting check**

Run:

```bash
rtk cargo fmt --check
```

Expected: command exits 0.

- [ ] **Step 6: Commit final fixes**

If formatter or test fixes changed files:

```bash
git add crates/rollshot-core/src/matcher.rs crates/rollshot-core/src/stitcher.rs crates/rollshot-core/tests/common/mod.rs crates/rollshot-core/tests/stitcher.rs
git commit -m "fix(core): stabilize autohybrid matcher verification"
```

If there are no changes, skip this commit.

---

## Completion Criteria

- `estimate_motion` returns verified `MotionCandidate`s for vertical down, vertical up, horizontal right, and horizontal left synthetic scrolls.
- `Stitcher` appends/prepends through `LinearCanvas` in all four directions using matcher output, not direct test-fed estimates.
- Locked-axis matching does not silently switch from vertical to horizontal or horizontal to vertical.
- Bad frames and repeated-content frames do not advance the anchor.
- AKAZE remains unimplemented and unconfigured in this plan.
- These commands pass:

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk cargo test --workspace
```

## Self-Review Notes

- Spec coverage: Plan 2 covers template conversion to `MotionCandidate`, vertical/horizontal template search, coarse 2D matching, verifier-backed ranking, second-best margin rejection, sticky/repeated behavior without AKAZE, and anchor preservation on matcher failures.
- Deferred by design: AKAZE dependency, AKAZE fixtures, golden fixture layout, debug reports, and AKAZE-enabled CI are Plan 3 scope.
- Placeholder scan: no unfinished placeholder steps are required for implementation.
