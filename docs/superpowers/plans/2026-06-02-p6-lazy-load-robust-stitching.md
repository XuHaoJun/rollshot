# P6 Lazy-load Robust Stitching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make scrolling-capture stitching robust to load-once lazy-load images in the overlap region (first-frame and mid-capture), without weakening the misfire defense, by adding a confidence-gated tile-vote acceptance path to the verifier, making the feature matcher a routine candidate source with cached anchor descriptors, and generalizing the mid-capture re-anchor safety net.

**Architecture:** Acceptance stays `PixelOverlapVerifier`-gated (spec §3 invariant (a)). The verifier becomes `legacy-strict-mean OR confidence-gated tile-vote` — a monotonic superset, so non-dynamic output stays byte-identical. The feature path becomes a routine candidate source feeding both a candidate and an inlier-ratio confidence signal; the anchor's descriptors are cached on `PreparedFrame` via `OnceLock` (same pattern as `coarse`/`proj_v`). Mid-capture re-anchor is a last-resort floor beneath ①+②, preserving committed canvas.

**Tech Stack:** Rust, `rollshot-core` crate. `image`, `rayon`, `wide`. Tests via `cargo test -p rollshot-core`; benches via `cargo bench -p rollshot-core --bench stitch_sequences`.

**Spec:** `docs/superpowers/specs/2026-06-02-p6-lazy-load-robust-stitching-design.md`

**Conventions for every command below:** run shell via `rtk proxy <cmd>` so test stdout (`println!`, failure messages) is not filtered. Commit messages end with the `Co-Authored-By` trailer the repo uses. Branch first if on `main`.

> **Execution amendment (2026-06-02, decision (a) reconciliation).** During execution the original Phase-A integration tests were found to contradict decision (a) (verifier is the final gate + majority floor):
> - `mid_capture_lazy_load_keeps_stitching` was **vacuous** — a single transient placeholder frame is already tolerated by preserve-anchor-on-NoMatch, so it passed on current code without testing anything. **Removed.**
> - `large_lazy_region_recovered_by_feature_consensus` asserted **stitch-through** of a change covering >40% of the overlap, which the majority floor (≥0.6 tile agreement) **cannot** accept under (a). A >40%-changed overlap is **not stitchable** by design; it is only **escapable via ③ re-anchor** (logged content gap). **Reclassified to Phase D (③).**
>
> Net effect on this plan:
> - **Phase A (A2)** now commits only the two stable **guard** tests (misfire floor `repeated_rows_still_not_falsely_appended` + monotonicity `clean_scroll_unchanged`), both passing every phase. (Done — commit `12b765b`.)
> - **① localized acceptance** RED→green lives in the **verifier unit tests** (B2/B3) and the existing `reanchor_stale_first_frame.rs` integration test.
> - **② offset recovery** RED→green is a **unit test** on `feature_candidate_from_features` (C3 below), not an integration stitch-through test.
> - **③ large/unrecoverable escape** RED→green is the integration test in D1 (white frames mid-capture → stall on current code → re-anchor after Phase D).
> Where a task's step text below still references the removed tests, the corrected text is used in the actual subagent dispatch.

---

## File Structure

| File | Responsibility | Phase |
|---|---|---|
| `crates/rollshot-core/tests/common/mod.rs` | add shared `lazy_load_page(image_loaded, …)` synthetic generator | A |
| `crates/rollshot-core/tests/lazy_load_robust.rs` | NEW — in-memory lazy-load + misfire integration tests (RED first) | A |
| `crates/rollshot-core/src/types.rs` | extend `VerifierConfig` with robust-tile fields + defaults | B |
| `crates/rollshot-core/src/verifier.rs` | add `tile_agreement()`, confidence-gated tile-vote path in `verify()`, unit tests | B |
| `crates/rollshot-core/src/matcher.rs` | `PreparedFrame` feature-descriptor `OnceLock` cache; run feature as routine candidate source | C |
| `crates/rollshot-core/src/feature_matcher.rs` | `NearestDescriptors` backend trait + `BruteForceIndex`; accept cached descriptors; routine entry point | C |
| `crates/rollshot-core/src/stitcher.rs` | generalize re-anchor to mid-capture, preserving canvas | D |
| `crates/rollshot-core/benches/synthetic.rs` | add lazy-load mutation to `SyntheticSpec` + a bench spec | A |

The existing on-disk golden suite (`tests/fixtures/linearscroll_v2/*` driven by `tests/golden_fixtures.rs`) is the **monotonicity + misfire regression gate**: `repeated_grid`, `low_feature_text`, `sticky_header`, `image_cards`, `linear_*` must stay green and byte-identical after every phase. New *dynamic* scenarios are in-memory Rust tests (the `tests/reanchor_stale_first_frame.rs` pattern), because their expected output is asserted structurally rather than baked to PNG.

---

## Phase A — Test scaffolding (RED) & bench fixtures

Establishes the failing lazy-load tests and the misfire floor before any production change. PR A changes no production code.

### Task A1: Shared lazy-load page generator

**Files:**
- Modify: `crates/rollshot-core/tests/common/mod.rs` (append)

- [ ] **Step 1: Add the generator** to the end of `tests/common/mod.rs`:

```rust
/// A tall page with richly-textured text-like rows everywhere plus one large
/// product-image block spanning rows `[img_y0, img_y1)`. `image_loaded`
/// toggles whether that block is the real textured photo or a flat lazy-load
/// placeholder. Used to reproduce load-once lazy-load mutation between frames.
pub fn lazy_load_page(
    width: u32,
    height: u32,
    img_y0: u32,
    img_y1: u32,
    image_loaded: bool,
) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([250, 250, 250, 255]));
    for y in 0..height {
        if y >= img_y0 && y < img_y1 {
            continue;
        }
        let line = (y / 22) % 4;
        if line == 0 {
            for x in 30..width.saturating_sub(30) {
                if (x / 6 + y / 3) % 2 == 0 {
                    img.put_pixel(x, y, Rgba([40, 40, 40, 255]));
                }
            }
        } else if line == 1 && y % 22 < 3 {
            for x in 40..width.saturating_sub(120) {
                img.put_pixel(x, y, Rgba([70, 90, 160, 255]));
            }
        }
    }
    for y in img_y0..img_y1.min(height) {
        for x in 24..width.saturating_sub(24) {
            let px = if image_loaded {
                let r = (60 + ((x * 2 + y) % 160)) as u8;
                let g = (40 + ((x + y * 3) % 180)) as u8;
                let b = (90 + ((x * 3 + y * 2) % 150)) as u8;
                Rgba([r, g, b, 255])
            } else {
                Rgba([225, 225, 225, 255])
            };
            img.put_pixel(x, y, px);
        }
    }
    img
}
```

- [ ] **Step 2: Verify it compiles** (it is `#![allow(dead_code)]` already so unused is fine):

Run: `rtk proxy cargo build -p rollshot-core --tests`
Expected: builds with no errors.

- [ ] **Step 3: Commit**

```bash
git add crates/rollshot-core/tests/common/mod.rs
git commit -m "test(core): add shared lazy_load_page generator

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task A2: Lazy-load + misfire integration tests (RED)

**Files:**
- Create: `crates/rollshot-core/tests/lazy_load_robust.rs`

- [ ] **Step 1: Write the failing tests.** These assert target behavior that current code fails. Note the `large_search` config mirrors `golden_fixtures.rs` (small synthetic frames need a wider search ratio).

```rust
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
```

- [ ] **Step 2: Run to verify the dynamic tests FAIL and the misfire/clean tests PASS**

Run: `rtk proxy cargo test -p rollshot-core --test lazy_load_robust`
Expected: `mid_capture_lazy_load_keeps_stitching` and `large_lazy_region_recovered_by_feature_consensus` **FAIL** (stalled / not recovered); `repeated_rows_still_not_falsely_appended` and `clean_scroll_unchanged` **PASS**. Record the failure messages — they confirm the bug exists on current code.

- [ ] **Step 3: Commit (RED tests included; they will go green in Phases B & C)**

```bash
git add crates/rollshot-core/tests/lazy_load_robust.rs
git commit -m "test(core): RED lazy-load robustness + misfire-floor tests

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task A3: Lazy-load mutation in the bench harness

**Files:**
- Modify: `crates/rollshot-core/benches/synthetic.rs`

- [ ] **Step 1: Add a mutation field to `SyntheticSpec`** (add after `sticky_top_band_height`):

```rust
    /// When set to `(y0, y1)`, frames whose index is in `lazy_load_frames`
    /// paint a flat placeholder over rows [y0, y1) of the cropped frame,
    /// simulating a not-yet-loaded image. Other frames show textured content.
    pub lazy_block: Option<(u32, u32)>,
    pub lazy_load_frames: &'static [usize],
```

- [ ] **Step 2: Apply the mutation inside `frames()`** — after the `sticky_top_band_height` block, before `frame` is returned:

```rust
            if let Some((y0, y1)) = spec.lazy_block {
                if spec.lazy_load_frames.contains(&idx) {
                    let h = frame.height();
                    let (y0, y1) = (y0.min(h), y1.min(h));
                    for y in y0..y1 {
                        for x in 0..frame.width() {
                            frame.put_pixel(x, y, Rgba([225, 225, 225, 255]));
                        }
                    }
                }
            }
```

- [ ] **Step 3: Set the new fields on every existing spec in `default_specs()`** to the inert default (`lazy_block: None, lazy_load_frames: &[]`), and add one lazy spec:

```rust
        SyntheticSpec {
            name: "long_lazy_load".to_string(),
            canvas_width: 900,
            canvas_height: 9000,
            frame_width: 900,
            frame_height: 700,
            step_px: 40,
            step_jitter_px: 0,
            frame_count: 200,
            sticky_top_band_height: None,
            lazy_block: Some((560, 700)),
            lazy_load_frames: &[5, 20, 60, 120],
        },
```

(Add `lazy_block: None, lazy_load_frames: &[]` to the three pre-existing specs too.)

- [ ] **Step 4: Build benches**

Run: `rtk proxy cargo build -p rollshot-core --benches`
Expected: builds clean.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/benches/synthetic.rs
git commit -m "bench(core): add lazy-load mutation to synthetic specs

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Phase B — ① Robust / confidence-gated verifier

Makes the verifier accept a correct offset whose overlap has a *localized* change, as a monotonic superset of the legacy strict-mean check. This alone turns `mid_capture_lazy_load_keeps_stitching` green (template/coarse find the offset; the new path verifies it).

### Task B1: Extend `VerifierConfig`

**Files:**
- Modify: `crates/rollshot-core/src/types.rs` (the `VerifierConfig` struct and its `Default`)

- [ ] **Step 1: Add fields** to `VerifierConfig` (after `sample_band`):

```rust
    /// Side length (px) of tiles for the robust tile-vote acceptance path.
    pub robust_tile_px: u32,
    /// Per-tile mean-MAD threshold (normalized 0..1); a tile "agrees" below it.
    pub robust_tile_tol: f32,
    /// Agreeing-tile fraction required for a weakly-supported offset.
    pub robust_accept_ratio: f32,
    /// Hard floor on the agreeing-tile fraction for ANY offset (misfire
    /// defense): a globally-wrong match (most tiles disagree) always fails.
    pub robust_accept_ratio_floor: f32,
```

- [ ] **Step 2: Add defaults** in `impl Default for VerifierConfig` (after `sample_band: 160,`):

```rust
            robust_tile_px: 48,
            robust_tile_tol: 24.0 / 255.0,
            robust_accept_ratio: 0.85,
            robust_accept_ratio_floor: 0.6,
```

- [ ] **Step 3: Update the config unit test** in `types.rs` (the `default_config_picks_auto_hybrid` test asserts verifier fields) — add:

```rust
        assert_eq!(cfg.verifier.robust_tile_px, 48);
        assert_eq!(cfg.verifier.robust_accept_ratio_floor, 0.6);
```

- [ ] **Step 4: Run** `rtk proxy cargo test -p rollshot-core --lib types`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/types.rs
git commit -m "feat(core): add robust tile-vote fields to VerifierConfig

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task B2: `tile_agreement()` + unit tests

**Files:**
- Modify: `crates/rollshot-core/src/verifier.rs`

- [ ] **Step 1: Write the failing unit test** (append to the `tests` module in `verifier.rs`):

```rust
    #[test]
    fn tile_agreement_full_on_identical_overlap() {
        let canvas = textured(160, 320);
        let prev = crop(&canvas, 0, 0, 160, 160);
        let curr = crop(&canvas, 0, 40, 160, 160);
        let r = compute_overlap(160, 160, 160, 160, 0, 40).unwrap();
        let ratio = tile_agreement(&prev, &curr, r, 32, 24.0 / 255.0);
        assert!(ratio > 0.99, "identical overlap should fully agree, got {ratio}");
    }

    #[test]
    fn tile_agreement_localized_change_is_majority_agree() {
        // Identical overlap except a small painted block on curr → minority of
        // tiles disagree, majority still agree.
        let canvas = textured(160, 320);
        let prev = crop(&canvas, 0, 0, 160, 160);
        let mut curr = crop(&canvas, 0, 40, 160, 160);
        for y in 100..160 {
            for x in 0..40 {
                curr.put_pixel(x, y, Rgba([255, 0, 255, 255]));
            }
        }
        let r = compute_overlap(160, 160, 160, 160, 0, 40).unwrap();
        let ratio = tile_agreement(&prev, &curr, r, 32, 24.0 / 255.0);
        assert!((0.6..0.99).contains(&ratio), "expected majority agree, got {ratio}");
    }

    #[test]
    fn tile_agreement_low_on_global_mismatch() {
        let prev = textured(160, 160);
        let curr = RgbaImage::from_pixel(160, 160, Rgba([255, 255, 255, 255]));
        let r = compute_overlap(160, 160, 160, 160, 0, 20).unwrap();
        let ratio = tile_agreement(&prev, &curr, r, 32, 24.0 / 255.0);
        assert!(ratio < 0.4, "global mismatch should mostly disagree, got {ratio}");
    }
```

- [ ] **Step 2: Run to verify FAIL** (function undefined)

Run: `rtk proxy cargo test -p rollshot-core --lib verifier::tests::tile_agreement`
Expected: FAIL — `cannot find function tile_agreement`.

- [ ] **Step 3: Implement `tile_agreement`** (add as a free function in `verifier.rs`, near `downsampled_mad`):

```rust
/// Fraction of `tile_px`×`tile_px` tiles over the overlap whose mean absolute
/// difference is below `tile_tol`. Partial edge tiles count by their pixels.
fn tile_agreement(
    prev: &RgbaImage,
    curr: &RgbaImage,
    r: OverlapRegion,
    tile_px: u32,
    tile_tol: f32,
) -> f32 {
    let tile = tile_px.max(1);
    let mut total_tiles = 0u32;
    let mut agree_tiles = 0u32;
    let mut ty = 0u32;
    while ty < r.height {
        let th = tile.min(r.height - ty);
        let mut tx = 0u32;
        while tx < r.width {
            let tw = tile.min(r.width - tx);
            let mut sum = 0.0f32;
            let mut count = 0u32;
            for row in 0..th {
                for col in 0..tw {
                    let p = pixel_gray(prev, r.prev_x + tx + col, r.prev_y + ty + row);
                    let c = pixel_gray(curr, r.curr_x + tx + col, r.curr_y + ty + row);
                    sum += (p - c).abs();
                    count += 1;
                }
            }
            let mad = if count == 0 { f32::INFINITY } else { sum / (count as f32 * 255.0) };
            total_tiles += 1;
            if mad <= tile_tol {
                agree_tiles += 1;
            }
            tx += tile;
        }
        ty += tile;
    }
    if total_tiles == 0 {
        return 0.0;
    }
    agree_tiles as f32 / total_tiles as f32
}
```

- [ ] **Step 4: Run to verify PASS**

Run: `rtk proxy cargo test -p rollshot-core --lib verifier::tests::tile_agreement`
Expected: 3 PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/verifier.rs
git commit -m "feat(core): add tile_agreement robust overlap statistic

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task B3: Confidence-gated tile-vote acceptance in `verify()`

**Files:**
- Modify: `crates/rollshot-core/src/verifier.rs` (the `verify` method)

- [ ] **Step 1: Write the failing test** (append to `verifier.rs` tests). It asserts a localized-change overlap that FAILS the strict mean is ACCEPTED via tile-vote when the candidate is strongly supported (low `score`):

```rust
    #[test]
    fn localized_change_accepted_via_tile_vote_when_confident() {
        let canvas = textured(160, 320);
        let prev = crop(&canvas, 0, 0, 160, 160);
        let mut curr = crop(&canvas, 0, 40, 160, 160);
        // Paint a localized block big enough to break the strict mean band.
        for y in 80..160 {
            for x in 0..70 {
                curr.put_pixel(x, y, Rgba([255, 0, 255, 255]));
            }
        }
        let cfg = VerifierConfig::default();
        let verifier = PixelOverlapVerifier::new(&cfg, 64);
        // strong support: low score (≈ high NCC confidence)
        let mut cand = candidate(0, 40);
        cand.score = 0.02;
        match verifier.verify(&prev, &curr, &cand) {
            VerifierOutcome::Pass { .. } => {}
            other => panic!("expected tile-vote Pass, got {other:?}"),
        }
    }

    #[test]
    fn localized_change_rejected_when_weakly_supported() {
        let canvas = textured(160, 320);
        let prev = crop(&canvas, 0, 0, 160, 160);
        let mut curr = crop(&canvas, 0, 40, 160, 160);
        for y in 80..160 {
            for x in 0..70 {
                curr.put_pixel(x, y, Rgba([255, 0, 255, 255]));
            }
        }
        let cfg = VerifierConfig::default();
        let verifier = PixelOverlapVerifier::new(&cfg, 64);
        let mut cand = candidate(0, 40);
        cand.score = 0.5; // weak support → strict accept_ratio applies
        assert!(matches!(
            verifier.verify(&prev, &curr, &cand),
            VerifierOutcome::OverlapDisagreement { .. }
        ));
    }
```

(These rely on the `candidate(dx, dy)` helper already in the test module returning `score: 0.0`; we mutate `.score`. Confirm that helper sets the other fields — it does.)

- [ ] **Step 2: Run to verify FAIL**

Run: `rtk proxy cargo test -p rollshot-core --lib verifier::tests::localized_change`
Expected: `localized_change_accepted_via_tile_vote_when_confident` FAILS (currently returns `OverlapDisagreement`).

- [ ] **Step 3: Add the confidence helper + tile-vote path.** Add this free function:

```rust
/// Required agreeing-tile fraction for `candidate`. A strongly-supported offset
/// (high NCC confidence — low score — or high feature inlier ratio) may drop to
/// the misfire floor; a weakly-supported offset must meet the strict ratio.
fn required_agreement(candidate: &MotionCandidate, config: &VerifierConfig) -> f32 {
    const STRONG_SCORE: f32 = 0.06;
    const STRONG_INLIER_RATIO: f32 = 0.5;
    let strong_ncc = candidate.score <= STRONG_SCORE;
    let strong_feature = matches!(
        (candidate.inliers, candidate.raw_matches),
        (Some(i), Some(r)) if r > 0 && (i as f32 / r as f32) >= STRONG_INLIER_RATIO
    );
    if strong_ncc || strong_feature {
        config.robust_accept_ratio_floor
    } else {
        config.robust_accept_ratio
    }
}
```

Then, in `verify()`, replace the two early-return `OverlapDisagreement` arms with a fall-through to the tile-vote path. Concretely, change the body so that after computing `downsample_mad` and (when reached) `full_mad`, the legacy strict result is computed first; on strict failure, try tile-vote before returning disagreement:

```rust
    pub fn verify(
        &self,
        prev: &RgbaImage,
        curr: &RgbaImage,
        candidate: &MotionCandidate,
    ) -> VerifierOutcome {
        let region = match compute_overlap(
            prev.width(), prev.height(), curr.width(), curr.height(),
            candidate.dx, candidate.dy,
        ) {
            Some(r) => r,
            None => return VerifierOutcome::InsufficientOverlap,
        };
        if region.area() < self.min_overlap_area {
            return VerifierOutcome::InsufficientOverlap;
        }

        let downsample_mad = downsampled_mad(prev, curr, region, self.config.downsample_step);
        let full_mad = sample_band_mad(prev, curr, region, self.config.sample_band);

        // Legacy strict-mean acceptance (preserved exactly → monotonic superset).
        let legacy_pass = downsample_mad.is_finite()
            && downsample_mad <= self.config.downsample_max_mad
            && full_mad.is_finite()
            && full_mad <= self.config.full_res_max_mad;
        if legacy_pass {
            return VerifierOutcome::Pass { overlap: region, score: full_mad.clamp(0.0, 1.0) };
        }

        // Robust tile-vote acceptance: tolerate a localized minority of
        // disagreeing tiles, gated by how strongly the offset is supported.
        let agree = tile_agreement(
            prev, curr, region,
            self.config.robust_tile_px, self.config.robust_tile_tol,
        );
        if agree >= required_agreement(candidate, self.config) {
            // Score from the (worse) full_mad if finite, else from disagreement.
            let score = if full_mad.is_finite() { full_mad.clamp(0.0, 1.0) } else { 1.0 - agree };
            return VerifierOutcome::Pass { overlap: region, score };
        }

        VerifierOutcome::OverlapDisagreement { downsample_mad, full_mad }
    }
```

Add `use crate::types::MotionCandidate;` is already present; ensure `VerifierConfig` import covers the new fields (same struct).

- [ ] **Step 4: Run the verifier unit suite**

Run: `rtk proxy cargo test -p rollshot-core --lib verifier`
Expected: all PASS, including the two new `localized_change_*` tests and the pre-existing `unrelated_frames_fail_verification` / `matching_frames_with_known_motion_pass`.

- [ ] **Step 5: Run the full core lib + golden suite to prove monotonicity**

Run: `rtk proxy cargo test -p rollshot-core`
Expected: ALL PASS, **especially** `golden_fixtures::golden_fixtures_match_expected_outputs` (byte-identical output preserved — clean content still takes the legacy path) and the Phase-A `clean_scroll_unchanged` + `repeated_rows_still_not_falsely_appended`.

- [ ] **Step 6: Verify the mid-capture lazy-load test now passes**

Run: `rtk proxy cargo test -p rollshot-core --test lazy_load_robust mid_capture_lazy_load_keeps_stitching`
Expected: PASS (template/coarse find the offset; tile-vote accepts despite the local change). `large_lazy_region_*` may still FAIL — that needs Phase C.

- [ ] **Step 7: fmt + clippy**

Run: `rtk proxy cargo fmt --check` then `rtk proxy cargo clippy -p rollshot-core --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/rollshot-core/src/verifier.rs
git commit -m "feat(core): confidence-gated tile-vote acceptance in PixelOverlapVerifier

Accept a geometrically-correct offset whose overlap has a localized change
(lazy-load image) as a monotonic superset of the legacy strict-mean check:
strict mean OR tile-vote. Tolerance widens only for strongly-supported
offsets and never below a majority floor, preserving the misfire defense.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Phase C — ② Routine feature path + cached anchor descriptors

Makes the feature matcher a routine candidate source (so a large changed region that defeats template is still recovered) and caches the anchor's descriptors on `PreparedFrame` via `OnceLock` so routine matching is affordable. No Cargo feature gate (spec §4.3); brute-force/SIMD KNN now, ANN later behind the same shape.

### Task C1: Cache anchor descriptors on `PreparedFrame`

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs` (the `PreparedFrame` struct + impl)
- Modify: `crates/rollshot-core/src/feature_matcher.rs` (expose a descriptor-extraction entry that takes gray + returns `(corners, descriptors)`)

- [ ] **Step 1: Expose a reusable extractor in `feature_matcher.rs`.** Add a `pub(crate)` struct + function that does corner+descriptor extraction from an RGBA frame, reusing the existing private `rgba_to_gray`/`extract_corners`/`compute_descriptors`. Then have `fast_hnsw_candidates` call it (refactor its extraction to go through this) to keep one extraction path:

```rust
/// Cached FAST corners + 8-D descriptors for one frame (kept corners are
/// aligned 1:1 with descriptors).
pub(crate) struct FrameFeatures {
    pub corners: Vec<(u32, u32)>,
    pub descriptors: Vec<[f32; 8]>,
}

pub(crate) fn extract_frame_features(
    rgba: &RgbaImage,
    config: &FastHnswConfig,
) -> FrameFeatures {
    let gray = rgba_to_gray(rgba);
    let corners = extract_corners(&gray, config.corner_threshold, config.max_features);
    let (descriptors, kept) = compute_descriptors(&gray, &corners, config.descriptor_patch_size);
    FrameFeatures { corners: kept, descriptors }
}
```

- [ ] **Step 2: Add a `OnceLock<FrameFeatures>` to `PreparedFrame`** in `matcher.rs` (next to `coarse`, `proj_v`, `proj_h`):

```rust
    features: OnceLock<crate::feature_matcher::FrameFeatures>,
```

Initialize `features: OnceLock::new()` in the `PreparedFrame::from_parts` constructor (the only place that builds the struct literal; `new()` delegates to it). Add an accessor that extracts from the owned RGBA on first use and caches it:

```rust
    pub(crate) fn features(&self, config: &crate::types::FastHnswConfig)
        -> &crate::feature_matcher::FrameFeatures
    {
        self.features
            .get_or_init(|| crate::feature_matcher::extract_frame_features(&self.rgba, config))
    }
```

- [ ] **Step 3: Add a unit test** (in `matcher.rs` tests) that the cache is built once and stable. Build a textured RGBA inline so the test needs no cross-module helper:

```rust
    #[test]
    fn prepared_frame_caches_features() {
        let mut img = RgbaImage::from_pixel(240, 240, Rgba([240, 240, 240, 255]));
        for y in 0..240u32 {
            for x in 0..240u32 {
                if (x / 5 + y / 7) % 2 == 0 {
                    img.put_pixel(x, y, Rgba([20, ((x * 7) % 200) as u8, ((y * 11) % 200) as u8, 255]));
                }
            }
        }
        let pf = PreparedFrame::new(img);
        let cfg = crate::types::FastHnswConfig::default();
        let a = pf.features(&cfg).descriptors.len();
        let b = pf.features(&cfg).descriptors.len();
        assert_eq!(a, b);
        assert!(a > 0);
    }
```

(Ensure `image::Rgba` is imported in the `matcher.rs` test module; the file already imports `image::{Rgba, RgbaImage}` at top.)

- [ ] **Step 4: Run** `rtk proxy cargo test -p rollshot-core --lib prepared_frame_caches_features`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/feature_matcher.rs crates/rollshot-core/src/matcher.rs
git commit -m "feat(core): cache anchor FAST descriptors on PreparedFrame (edge-index reuse)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task C2: Backend trait for nearest-descriptor search

**Files:**
- Modify: `crates/rollshot-core/src/feature_matcher.rs`

- [ ] **Step 1: Write a failing test** that the brute-force backend reproduces `linear_knn_match` on a small set:

```rust
    #[test]
    fn brute_force_backend_matches_linear_knn() {
        let a = vec![[0.0f32; 8], [1.0; 8], [5.0; 8]];
        let b = vec![[5.01f32; 8], [0.02; 8], [1.03; 8]];
        let direct = linear_knn_match(&a, &b, 0.5, 1.4);
        let backend = BruteForceIndex::build(&a);
        let viaindex = backend.match_against(&a, &b, 0.5, 1.4);
        assert_eq!(direct, viaindex);
    }
```

- [ ] **Step 2: Run to verify FAIL** (type undefined).

Run: `rtk proxy cargo test -p rollshot-core --lib brute_force_backend_matches_linear_knn`
Expected: FAIL.

- [ ] **Step 3: Implement the trait + brute-force impl.** The trait is the seam HNSW will later drop into; `match_against` returns the same `Vec<[usize;2]>` mutual-NN pairs `linear_knn_match` produces:

```rust
/// Nearest-descriptor backend over 8-D descriptors. Brute-force today;
/// an ANN (HNSW) backend can replace it behind this trait without touching
/// callers. Permanent (no Cargo feature gate) — routine feature matching
/// depends on it being always present (spec §4.3).
pub(crate) trait NearestDescriptors {
    /// Mutual nearest-neighbour matches between `prev` and `curr` descriptors,
    /// `[curr_idx, prev_idx]`, with the same distance + Lowe-ratio gates.
    fn match_against(
        &self,
        prev: &[[f32; 8]],
        curr: &[[f32; 8]],
        distance_threshold: f32,
        lowe_ratio: f32,
    ) -> Vec<[usize; 2]>;
}

pub(crate) struct BruteForceIndex;

impl BruteForceIndex {
    pub(crate) fn build(_prev: &[[f32; 8]]) -> Self {
        BruteForceIndex
    }
}

impl NearestDescriptors for BruteForceIndex {
    fn match_against(
        &self,
        prev: &[[f32; 8]],
        curr: &[[f32; 8]],
        distance_threshold: f32,
        lowe_ratio: f32,
    ) -> Vec<[usize; 2]> {
        linear_knn_match(prev, curr, distance_threshold, lowe_ratio)
    }
}
```

- [ ] **Step 4: Run to verify PASS**

Run: `rtk proxy cargo test -p rollshot-core --lib brute_force_backend_matches_linear_knn`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/feature_matcher.rs
git commit -m "feat(core): NearestDescriptors backend trait + brute-force impl

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task C3: Routine feature candidate from cached descriptors

**Files:**
- Modify: `crates/rollshot-core/src/feature_matcher.rs` (add a PreparedFrame-based entry)
- Modify: `crates/rollshot-core/src/matcher.rs` (`estimate_motion`: add feature as a routine candidate source)

- [ ] **Step 1: Add a routine entry in `feature_matcher.rs`** that takes already-extracted features (so the anchor’s come from cache) and produces a `MotionCandidate` using the backend trait. This reuses `vote_dominant_translation` + `feature_score` unchanged:

```rust
/// Routine feature candidate from pre-extracted features (anchor features are
/// cached on PreparedFrame; curr features extracted this frame). Returns None
/// when gates (min_keypoints / min_raw_matches / min_inliers / second_best)
/// are not met — feature matching is a candidate *source*, not the gate; the
/// PixelOverlapVerifier remains the final gate (spec §3 invariant a).
pub(crate) fn feature_candidate_from_features(
    prev: &FrameFeatures,
    curr: &FrameFeatures,
    locked_axis: Option<ScrollAxis>,
    config: &FastHnswConfig,
) -> Option<MotionCandidate> {
    if !config.enabled
        || prev.descriptors.len() < config.min_keypoints
        || curr.descriptors.len() < config.min_keypoints
    {
        return None;
    }
    let lowe_ratio = 1.4;
    let backend = BruteForceIndex::build(&prev.descriptors);
    let matches = backend.match_against(&prev.descriptors, &curr.descriptors, config.distance_threshold, lowe_ratio);
    if matches.len() < config.min_raw_matches {
        return None;
    }
    let (dx, dy, inliers, raw, residual_px) =
        vote_dominant_translation(&prev.corners, &curr.corners, &matches, locked_axis, config)?;
    let inlier_ratio = inliers as f32 / raw.max(1) as f32;
    Some(MotionCandidate {
        dx,
        dy,
        method: crate::types::MatchMethod::FastHnsw,
        score: feature_score(inlier_ratio, residual_px),
        second_best_score: None,
        inliers: Some(inliers),
        raw_matches: Some(raw),
    })
}
```

- [ ] **Step 2: Wire it as a routine candidate source in `matcher.rs::estimate_motion`.** After the `edge_result` is pushed into `candidates` and BEFORE the first `rank_verified_candidates` call (currently ~line 234-242), add the feature candidate to the pool:

```rust
    if config.fast_hnsw.enabled {
        let feat_start = std::time::Instant::now();
        let prev_feats = prev.features(&config.fast_hnsw);
        let curr_feats = curr.features(&config.fast_hnsw);
        if let Some(c) = crate::feature_matcher::feature_candidate_from_features(
            prev_feats, curr_feats, locked_axis, &config.fast_hnsw,
        ) {
            candidates.push(c);
        }
        metrics.fallback_us += feat_start.elapsed().as_micros() as u64;
    }
```

(The anchor `prev.features(...)` hits the `OnceLock` cache after the first frame; only `curr.features(...)` does work each frame. Time is *accumulated* into `metrics.fallback_us` with `+=` — not a `ScopedTimer` that would overwrite — so it composes with the existing last-resort fallback timer below.) Keep the existing last-resort `feature_fallback_candidates` block as-is — it remains the deeper fallback when the whole candidate pool is rejected, and routine inclusion does not change its behavior.

- [ ] **Step 3: Run the large-region recovery test**

Run: `rtk proxy cargo test -p rollshot-core --test lazy_load_robust large_lazy_region_recovered_by_feature_consensus`
Expected: PASS (feature consensus supplies the offset; ① verifies it via tile-vote, gated by the now-available inlier ratio).

- [ ] **Step 4: Full suite + monotonicity + misfire + bench-smoke**

Run: `rtk proxy cargo test -p rollshot-core`
Expected: ALL PASS — golden suite byte-identical, `repeated_rows_still_not_falsely_appended` green (routine feature does not bypass the second-best/verifier gates).

- [ ] **Step 5: fmt + clippy**

Run: `rtk proxy cargo fmt --check` then `rtk proxy cargo clippy -p rollshot-core --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Benchmark — confirm clean-frame cost stays bounded**

Run: `rtk proxy cargo bench -p rollshot-core --bench stitch_sequences -- --out bench-results/runs/p6-routine-feature/after.jsonl`
Then capture a baseline from `main` the same way into `before.jsonl` (or reuse an existing baseline) and:
Run: `rtk proxy python3 scripts/bench/compare.py bench-results/runs/p6-routine-feature/before.jsonl bench-results/runs/p6-routine-feature/after.jsonl`
Expected/Acceptance: clean-sequence (`long_vertical_text`, `long_sticky_header`) total p50 regression stays within a small budget (target ≤ ~10%); the cached anchor descriptors keep per-frame feature cost off the hot path. Record numbers in the commit body. If p50 regresses badly, fall back to gating feature on borderline frames (spec §7) — note it, do not silently ship.

- [ ] **Step 7: Commit**

```bash
git add crates/rollshot-core/src/feature_matcher.rs crates/rollshot-core/src/matcher.rs
git commit -m "feat(core): routine feature candidate source w/ cached anchor descriptors

Feature matching now contributes a candidate every frame (not just last-resort
fallback), recovering offsets when a large changed region defeats template, and
supplying the inlier-ratio that gates the robust verifier. Anchor descriptors
are reused via the PreparedFrame OnceLock cache. Backend behind NearestDescriptors
trait (brute-force now, ANN drop-in later). [bench numbers in body]

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Phase D — ③ Mid-capture re-anchor safety net

Generalizes the shipped first-frame re-anchor to a bounded last-resort floor anywhere in the capture, **preserving committed canvas** (the critical distinction from the first-frame path, spec §4.4).

### Task D1: Mid-capture re-anchor preserving canvas

**Files:**
- Modify: `crates/rollshot-core/src/stitcher.rs`
- Test: `crates/rollshot-core/tests/lazy_load_robust.rs`

- [ ] **Step 1: Write the failing test** (append to `lazy_load_robust.rs`). A mid-capture *unrecoverable* run (the changed region fills the whole overlap so even ①+② can't verify) must not stall forever AND must keep the already-stitched content:

```rust
/// ③ floor: an unrecoverable mid-capture disagreement must re-anchor (not
/// stall) AND must preserve the canvas stitched before it.
#[test]
fn mid_capture_unrecoverable_reanchors_preserving_canvas() {
    let loaded = lazy_load_page(W, CANVAS_H, IMG_Y0, IMG_Y1, true);
    let mut s = Stitcher::new(cfg());
    assert_eq!(s.push_frame(crop_frame(&loaded, 0, FRAME_H)), StitchOutcome::FirstFrame);
    let _ = s.push_frame(crop_frame(&loaded, STEP, FRAME_H));
    let _ = s.push_frame(crop_frame(&loaded, 2 * STEP, FRAME_H));
    let height_before = s.stats().total_height;
    assert!(height_before > FRAME_H, "precondition: some content stitched");

    // Feed frames that share no overlap with the anchor (jump far + full white)
    // → repeated NoMatch → must re-anchor, not stall, and not wipe the canvas.
    let white = image::RgbaImage::from_pixel(W, FRAME_H, image::Rgba([255, 255, 255, 255]));
    for _ in 0..4 {
        let _ = s.push_frame(white.clone());
    }
    // After re-anchor, a fresh consistent run must append again.
    let canvas2 = make_scroll_canvas(W, CANVAS_H);
    let mut progressed = false;
    for i in 0..4u32 {
        if matches!(s.push_frame(crop_frame(&canvas2, i * STEP, FRAME_H)), StitchOutcome::Appended { .. }) {
            progressed = true;
        }
    }
    assert!(progressed, "did not recover after mid-capture re-anchor");
    assert!(
        s.stats().total_height >= height_before,
        "mid-capture re-anchor wiped committed canvas: {} < {}",
        s.stats().total_height, height_before
    );
}
```

- [ ] **Step 2: Run to verify FAIL**

Run: `rtk proxy cargo test -p rollshot-core --test lazy_load_robust mid_capture_unrecoverable_reanchors_preserving_canvas`
Expected: FAIL — current re-anchor only triggers at `frame_count == 1`, so mid-capture stalls (no recovery / no progress).

- [ ] **Step 3: Implement mid-capture re-anchor.** In `stitcher.rs`, broaden the candidate clone and add a mid-capture branch that does NOT reset the canvas. Replace the current `reanchor_candidate` block and the post-`outcome` handling in `push_frame`:

```rust
        // Keep a copy so a stale/bad anchor (lazy-load not painted yet) cannot
        // block the capture forever. First-frame and mid-capture re-anchor have
        // DIFFERENT semantics (see reanchor_to / reanchor_mid_capture).
        let reanchor_candidate = if self.canvas.is_some() {
            Some(frame.clone())
        } else {
            None
        };

        let outcome = self.push_frame_inner(frame);

        if let Some(candidate) = reanchor_candidate {
            if matches!(outcome, StitchOutcome::NoMatch { .. }) {
                self.first_frame_misses += 1;
                if self.first_frame_misses >= REANCHOR_MISS_THRESHOLD {
                    if self.stats.frame_count == 1 {
                        // Stale first frame: nothing committed, rebuild from scratch.
                        self.reanchor_to(candidate);
                    } else {
                        // Mid-capture: PRESERVE the committed canvas; only move
                        // the match anchor forward to this frame, leaving a gap.
                        self.reanchor_mid_capture(candidate);
                    }
                }
            } else {
                self.first_frame_misses = 0;
            }
        }
```

Add the new method next to `reanchor_to`:

```rust
    /// Mid-capture re-anchor: the committed canvas is real content and MUST be
    /// kept. Only reset the match anchor (`last_good`) to `frame` and clear the
    /// motion/axis lock so the next frame matches fresh content. A content gap
    /// is accepted and logged; this is a last-resort floor beneath the robust
    /// verifier + feature consensus.
    fn reanchor_mid_capture(&mut self, frame: RgbaImage) {
        eprintln!(
            "rollshot: mid-capture re-anchor after {} consecutive misses; \
             a content gap may appear at canvas height {}",
            self.first_frame_misses, self.stats.total_height
        );
        self.last_good = Some(PreparedFrame::new(frame));
        self.last_motion = (0, 0);
        self.locked_axis = None;
        self.locked_direction = None;
        self.first_frame_misses = 0;
    }
```

(`reanchor_to` and `accept_first_frame` are unchanged; `accept_first_frame` still resets `first_frame_misses` for the first-frame path.)

- [ ] **Step 4: Run the new test + the existing re-anchor test**

Run: `rtk proxy cargo test -p rollshot-core --test lazy_load_robust mid_capture_unrecoverable_reanchors_preserving_canvas` then `rtk proxy cargo test -p rollshot-core --test reanchor_stale_first_frame`
Expected: both PASS. (First-frame test still green — `frame_count == 1` branch unchanged.)

- [ ] **Step 5: Full suite + fmt + clippy**

Run: `rtk proxy cargo test -p rollshot-core` then `rtk proxy cargo fmt --check` then `rtk proxy cargo clippy -p rollshot-core --all-targets -- -D warnings`
Expected: ALL PASS, golden suite byte-identical, clean lints.

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-core/src/stitcher.rs crates/rollshot-core/tests/lazy_load_robust.rs
git commit -m "feat(core): generalize re-anchor to mid-capture, preserving canvas

Extend the bounded NoMatch re-anchor safety net beyond the first frame: after
REANCHOR_MISS_THRESHOLD consecutive misses mid-capture, move the match anchor
to the latest frame WITHOUT resetting the committed canvas (logged content
gap). Last-resort floor beneath the robust verifier + feature consensus.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Phase E — Workspace verification & PR

### Task E1: Cross-crate verification

- [ ] **Step 1: Full workspace tests** (CLI + app consume the stitcher)

Run: `rtk proxy cargo test --workspace`
Expected: ALL PASS, no failures/errors.

- [ ] **Step 2: Workspace clippy**

Run: `rtk proxy cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 3: Push branch + open PR** (only when the user asks). Each phase B/C/D may also be split into its own PR per spec §6 if preferred; B alone is independently shippable and fixes the primary lazy-load bug.

---

## Spec coverage check (self-review)

- spec §4.2 robust verifier (mean ∨ tile-vote, majority floor, confidence-gate, monotonic superset) → Tasks B1–B3.
- spec §4.3 routine feature, edge-index reuse, backend interface, no Cargo gate → Tasks C1–C3.
- spec §4.4 mid-capture re-anchor preserving canvas → Task D1.
- spec §5.1 monotonicity (byte-identical non-dynamic output) → golden suite gate in B3 step 5, C3 step 4, D1 step 5.
- spec §5.2 lazy-load goldens → A2 (`mid_capture_…`, `large_lazy_region_…`), D1 (`mid_capture_unrecoverable_…`); first-frame already covered by `reanchor_stale_first_frame.rs`.
- spec §5.3 misfire-defense → A2 (`repeated_rows_…`) + existing `repeated_grid`/`low_feature_text`/`sticky_header` goldens kept green every phase.
- spec §5.4 benchmark → A3 (lazy bench spec) + C3 step 6 (clean-frame p50 budget).
- spec §5.5 TDD order → phases ordered goldens → ① → ② → ③.

## Notes on residual judgement (not placeholders)

- Tile-vote constants (`STRONG_SCORE = 0.06`, `STRONG_INLIER_RATIO = 0.5`, defaults `robust_tile_px=48`, `robust_accept_ratio=0.85`, floor `0.6`) are concrete starting values. The gate on changing them is the misfire suite (A2 `repeated_rows_…` + goldens `repeated_grid`/`low_feature_text`): if any goes red, tighten `robust_accept_ratio` / raise the floor, do not relax the misfire tests.
- If C3 step 6 shows an unacceptable clean-frame p50 regression, switch the feature source from "every frame" to "only when the robust verifier returns `OverlapDisagreement`" (spec §7 borderline-only fallback) — same `feature_candidate_from_features` entry, called from the disagreement path instead of the routine pool. Log the decision in the PR.
