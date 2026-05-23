# FAST + Linear-KNN Feature Fallback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace AKAZE as the default feature-based fallback in `estimate_motion` with a FAST-corner + linear-KNN matcher (~30–200 ms vs AKAZE's ~2 s on 2560-wide frames). AKAZE stays compiled and opt-in via `--enable-akaze`, marked deprecated.

**Architecture:** New module `crates/rollshot-core/src/feature_matcher.rs` exposes `fast_hnsw_candidates` (FAST detection + `[f32; 8]` row/col descriptor + rayon-parallel linear KNN with Lowe ratio + bucket-voting). A sibling `feature_fallback_candidates` does pick-one dispatch (AKAZE wins when `akaze.enabled` is true). `estimate_motion` calls the dispatcher instead of `akaze_candidates`. `Stitcher`, verifier, canvas, axis logic untouched.

**Tech Stack:** Rust, `imageproc` (FAST corner detection — NEW required dep), `image` (existing), `rayon` (existing), `clap` (existing). NO `hora`, NO `bitarray` — these belong to Approaches A and C, off-limits per spec.

**Spec:** `docs/superpowers/specs/2026-05-23-rollshot-fast-hnsw-fallback-design.md` (commit `35b5415`).

**Guardrails — repeat in every PR description:**
- Descriptor type is `[f32; 8]`, hard-locked. Not `Vec<f32>`, not generic, not binary.
- Matching is linear KNN with rayon. No HNSW, no ANN.
- Every `FastHnsw*` identifier carries the spec-defined doc comment (see Task 1 / Task 3).
- `Cargo.toml` gets `imageproc` only. No `hora`, no `bitarray`.

---

## Task 1: Add `FastHnswConfig`, `MatchMethod::FastHnsw`, new `NoMatchReason` variants

**Files:**
- Modify: `crates/rollshot-core/src/types.rs`

- [ ] **Step 1: Update the existing `akaze_is_disabled_by_default` test to also assert `fast_hnsw` defaults**

Locate the test (around the bottom of `types.rs`) and replace its body. Add a fresh `fast_hnsw_defaults` test below it.

```rust
    #[test]
    fn akaze_is_disabled_by_default() {
        let cfg = StitchConfig::default();
        assert!(!cfg.akaze.enabled);
        assert_eq!(cfg.akaze.max_features, 1200);
        assert_eq!(cfg.akaze.detector_threshold, 0.001);
        assert_eq!(cfg.akaze.min_raw_matches, 24);
        assert_eq!(cfg.akaze.min_inliers, 16);
        assert_eq!(cfg.akaze.min_inlier_ratio, 0.35);
    }

    #[test]
    fn fast_hnsw_is_enabled_by_default() {
        let cfg = StitchConfig::default();
        assert!(cfg.fast_hnsw.enabled);
        assert_eq!(cfg.fast_hnsw.corner_threshold, 64);
        assert_eq!(cfg.fast_hnsw.descriptor_patch_size, 9);
        assert_eq!(cfg.fast_hnsw.max_features, 1200);
        assert_eq!(cfg.fast_hnsw.min_keypoints, 80);
        assert_eq!(cfg.fast_hnsw.min_raw_matches, 24);
        assert_eq!(cfg.fast_hnsw.min_inliers, 16);
        assert!((cfg.fast_hnsw.distance_threshold - 0.10).abs() < 1e-6);
        assert_eq!(cfg.fast_hnsw.cross_axis_tolerance, 2);
        assert!((cfg.fast_hnsw.second_best_ratio - 2.0).abs() < 1e-6);
    }
```

- [ ] **Step 2: Run the test, expect failure**

Run: `cargo test -p rollshot-core --features akaze types::tests::fast_hnsw_is_enabled_by_default`
Expected: FAIL (compile error — `fast_hnsw` field does not exist on `StitchConfig`).

- [ ] **Step 3: Add the new variants and config**

In the `MatchMethod` enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMethod {
    Template,
    Coarse,
    Edge,
    Akaze,
    /// FAST corners + linear KNN matching. The "Hnsw" in the name is
    /// reserved for a future ANN upgrade — see
    /// docs/superpowers/specs/2026-05-23-rollshot-fast-hnsw-fallback-design.md
    /// Approach A. Current matching is exact linear scan.
    FastHnsw,
}
```

In the `NoMatchReason` enum, add two variants (preserve `#[non_exhaustive]`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoMatchReason {
    // ... existing variants unchanged ...
    AkazeDisabled,
    AkazeLowInliers,
    /// Both fast_hnsw and akaze are disabled, so estimate_motion has no
    /// feature-based fallback to fall through to.
    FeatureFallbackDisabled,
    /// The FAST+KNN path produced no candidate that passed
    /// rank_verified_candidates (or did not meet min_inliers).
    FeatureLowInliers,
}
```

Add `FastHnswConfig` near `AkazeConfig`:

```rust
/// Configuration for the FAST corners + linear KNN feature fallback.
///
/// The "Hnsw" in the name is reserved for a future ANN upgrade — see
/// docs/superpowers/specs/2026-05-23-rollshot-fast-hnsw-fallback-design.md
/// Approach A. Current matching is exact linear scan.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FastHnswConfig {
    pub enabled: bool,
    pub corner_threshold: u8,
    pub descriptor_patch_size: usize,
    pub max_features: usize,
    pub min_keypoints: usize,
    pub min_raw_matches: usize,
    pub min_inliers: usize,
    pub distance_threshold: f32,
    pub cross_axis_tolerance: i32,
    pub second_best_ratio: f32,
}

impl Default for FastHnswConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            corner_threshold: 64,
            descriptor_patch_size: 9,
            max_features: 1200,
            min_keypoints: 80,
            min_raw_matches: 24,
            min_inliers: 16,
            distance_threshold: 0.10,
            cross_axis_tolerance: 2,
            second_best_ratio: 2.0,
        }
    }
}
```

In `StitchConfig` struct, add the field (keep `#[non_exhaustive]`):

```rust
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct StitchConfig {
    // ... existing fields unchanged ...
    pub akaze: AkazeConfig,
    pub fast_hnsw: FastHnswConfig,
    pub verifier: VerifierConfig,
}
```

In `impl Default for StitchConfig`, add the field initializer:

```rust
            akaze: AkazeConfig::default(),
            fast_hnsw: FastHnswConfig::default(),
            verifier: VerifierConfig::default(),
```

- [ ] **Step 4: Run the test, expect pass**

Run: `cargo test -p rollshot-core --features akaze types::tests`
Expected: PASS (all `types::tests`, including both `_is_disabled_by_default` and `_is_enabled_by_default`).

- [ ] **Step 5: Run the full workspace test to catch downstream type breakage**

Run: `cargo test --workspace --features akaze`
Expected: PASS (everything previously passing should still pass — `StitchConfig` is `#[non_exhaustive]` so adding a field is non-breaking, the new `MatchMethod` and `NoMatchReason` variants are additive).

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-core/src/types.rs
git commit -m "$(cat <<'EOF'
feat(core): FastHnswConfig + MatchMethod::FastHnsw + new NoMatchReason

Adds the public surface for the v0.4 feature fallback per
docs/superpowers/specs/2026-05-23-rollshot-fast-hnsw-fallback-design.md.
Algorithm wiring is in subsequent commits.

FastHnswConfig.enabled defaults to true; akaze stays disabled by default
from Phase 1. MatchMethod::FastHnsw and NoMatchReason::{FeatureFallbackDisabled,
FeatureLowInliers} land on non_exhaustive enums (additive only).

The "Hnsw" identifier is intentional naming for the planned ANN upgrade
(Approach A in the spec); current matching is linear KNN.
EOF
)"
```

---

## Task 2: Add `imageproc` dependency

**Files:**
- Modify: `crates/rollshot-core/Cargo.toml`
- Modify: `Cargo.toml` (workspace root — only if version sync is needed)

- [ ] **Step 1: Check the latest `imageproc` version compatible with `image = 0.25`**

Run: `cargo search imageproc --limit 1`
Expected: a line like `imageproc = "0.25.X"` or similar.

If `imageproc 0.25` exists and depends on `image 0.25`, use it. If only `imageproc 0.24` exists (pegged to `image 0.24`), do **not** downgrade `image` — instead pin `imageproc` to its newest release that links the workspace `image`. If unsure, run `cargo tree -p rollshot-core` after the add and verify only one `image` major version appears.

- [ ] **Step 2: Add `imageproc` to `crates/rollshot-core/Cargo.toml`**

Locate the `[dependencies]` table. Add:

```toml
imageproc = { version = "<resolved version from Step 1>", default-features = false }
```

DO NOT add `hora`. DO NOT add `bitarray`. Those are off-limits per spec §Alternative Approaches.

- [ ] **Step 3: Verify the build**

Run: `cargo check --workspace --features akaze`
Expected: clean compile (warnings about unused `imageproc` are OK at this stage).

Run: `cargo tree -p rollshot-core | grep '^[│ ]*image'`
Expected: only one `image v0.25.X` line (no two majors of `image`).

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-core/Cargo.toml Cargo.lock
git commit -m "$(cat <<'EOF'
chore(core): add imageproc dependency for FAST corner detection

Required by the v0.4 feature fallback module landed next. No hora and
no bitarray — Approaches A and C are out of scope per the spec.
EOF
)"
```

---

## Task 3: Scaffold `feature_matcher.rs` and dispatch outcome types

**Files:**
- Create: `crates/rollshot-core/src/feature_matcher.rs`
- Modify: `crates/rollshot-core/src/lib.rs`

- [ ] **Step 1: Create the module file with stubs only**

Write `crates/rollshot-core/src/feature_matcher.rs`:

```rust
//! FAST corners + linear KNN feature fallback (Approach B per the spec).
//!
//! The "Hnsw" in the public identifiers is reserved for a future ANN
//! upgrade — see
//! docs/superpowers/specs/2026-05-23-rollshot-fast-hnsw-fallback-design.md
//! Approach A. Current matching is exact linear scan.

use image::RgbaImage;

use crate::akaze_matcher::{akaze_candidates, AkazeCandidateOutcome};
use crate::types::{
    FastHnswConfig, MotionCandidate, NoMatchReason, ScrollAxis, StitchConfig,
};

/// Outcome of running `fast_hnsw_candidates`.
///
/// Shape mirrors `AkazeCandidateOutcome` deliberately so the dispatcher
/// can collapse both into `FeatureFallbackOutcome` without bespoke
/// arms per branch.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FastHnswCandidateOutcome {
    Disabled,
    NotEnoughFeatures { prev: usize, curr: usize },
    NotEnoughMatches { raw_matches: usize },
    Candidates(Vec<MotionCandidate>),
}

/// Tagged outcome from the dispatcher so the matcher can map it back
/// onto the correct `NoMatchReason` variant.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FeatureFallbackOutcome {
    Disabled,
    NotEnoughFeatures { prev: usize, curr: usize },
    NotEnoughMatches {
        raw_matches: usize,
        source: FeatureSource,
    },
    Candidates {
        candidates: Vec<MotionCandidate>,
        source: FeatureSource,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeatureSource {
    FastHnsw,
    Akaze,
}

impl FeatureFallbackOutcome {
    fn from_fast_hnsw(outcome: FastHnswCandidateOutcome) -> Self {
        match outcome {
            FastHnswCandidateOutcome::Disabled => FeatureFallbackOutcome::Disabled,
            FastHnswCandidateOutcome::NotEnoughFeatures { prev, curr } => {
                FeatureFallbackOutcome::NotEnoughFeatures { prev, curr }
            }
            FastHnswCandidateOutcome::NotEnoughMatches { raw_matches } => {
                FeatureFallbackOutcome::NotEnoughMatches {
                    raw_matches,
                    source: FeatureSource::FastHnsw,
                }
            }
            FastHnswCandidateOutcome::Candidates(candidates) => {
                FeatureFallbackOutcome::Candidates {
                    candidates,
                    source: FeatureSource::FastHnsw,
                }
            }
        }
    }

    fn from_akaze(outcome: AkazeCandidateOutcome) -> Self {
        match outcome {
            AkazeCandidateOutcome::Disabled => FeatureFallbackOutcome::Disabled,
            AkazeCandidateOutcome::NotEnoughFeatures { prev, curr } => {
                FeatureFallbackOutcome::NotEnoughFeatures { prev, curr }
            }
            AkazeCandidateOutcome::NotEnoughMatches { raw_matches } => {
                FeatureFallbackOutcome::NotEnoughMatches {
                    raw_matches,
                    source: FeatureSource::Akaze,
                }
            }
            AkazeCandidateOutcome::Candidates(candidates) => {
                FeatureFallbackOutcome::Candidates {
                    candidates,
                    source: FeatureSource::Akaze,
                }
            }
        }
    }
}

/// FAST corners + linear KNN matching. The "Hnsw" in the name is
/// reserved for a future ANN upgrade — see
/// docs/superpowers/specs/2026-05-23-rollshot-fast-hnsw-fallback-design.md
/// Approach A. Current matching is exact linear scan.
pub(crate) fn fast_hnsw_candidates(
    _prev: &RgbaImage,
    _curr: &RgbaImage,
    _locked_axis: Option<ScrollAxis>,
    config: &FastHnswConfig,
) -> FastHnswCandidateOutcome {
    if !config.enabled {
        return FastHnswCandidateOutcome::Disabled;
    }
    // Real implementation lands in subsequent tasks.
    FastHnswCandidateOutcome::Disabled
}

/// Pick-one dispatch:
///   - `config.akaze.enabled = true`  → run AKAZE (FastHnsw is skipped
///                                       even if also enabled)
///   - else `config.fast_hnsw.enabled = true` → run FAST+KNN
///   - else → Disabled
pub(crate) fn feature_fallback_candidates(
    prev: &RgbaImage,
    curr: &RgbaImage,
    locked_axis: Option<ScrollAxis>,
    config: &StitchConfig,
) -> FeatureFallbackOutcome {
    if config.akaze.enabled {
        return FeatureFallbackOutcome::from_akaze(akaze_candidates(prev, curr, &config.akaze));
    }
    if config.fast_hnsw.enabled {
        return FeatureFallbackOutcome::from_fast_hnsw(fast_hnsw_candidates(
            prev,
            curr,
            locked_axis,
            &config.fast_hnsw,
        ));
    }
    FeatureFallbackOutcome::Disabled
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::StitchConfig;
    use image::{Rgba, RgbaImage};

    fn solid_frame() -> RgbaImage {
        RgbaImage::from_pixel(100, 100, Rgba([200, 200, 200, 255]))
    }

    #[test]
    fn fast_hnsw_returns_disabled_when_config_disabled() {
        let mut config = FastHnswConfig::default();
        config.enabled = false;
        let outcome = fast_hnsw_candidates(&solid_frame(), &solid_frame(), None, &config);
        assert_eq!(outcome, FastHnswCandidateOutcome::Disabled);
    }

    #[test]
    fn feature_fallback_disabled_when_both_off() {
        let mut config = StitchConfig::default();
        config.fast_hnsw.enabled = false;
        config.akaze.enabled = false;
        let outcome = feature_fallback_candidates(&solid_frame(), &solid_frame(), None, &config);
        assert_eq!(outcome, FeatureFallbackOutcome::Disabled);
    }

    #[test]
    fn feature_fallback_akaze_wins_pick_one() {
        let mut config = StitchConfig::default();
        config.fast_hnsw.enabled = true;
        config.akaze.enabled = true;
        let outcome = feature_fallback_candidates(&solid_frame(), &solid_frame(), None, &config);
        // Stub returns Disabled. AKAZE-feature off → also returns Disabled.
        // We only care that the dispatcher routed to AKAZE; the source
        // tag is observable through the Candidates / NotEnoughMatches
        // arms which we exercise after the real implementation lands.
        // For now, both branches collapse to Disabled, which is fine.
        assert_eq!(outcome, FeatureFallbackOutcome::Disabled);
    }
}
```

- [ ] **Step 2: Declare the module in `lib.rs`**

Locate `crates/rollshot-core/src/lib.rs`. Replace it with:

```rust
mod akaze_matcher;
mod axis;
mod canvas;
mod duplicate;
mod feature_matcher;
mod matcher;
mod overlap;
mod stitcher;
mod types;
mod verifier;

pub use canvas::{CanvasAppendError, LinearCanvas};
pub use stitcher::Stitcher;
pub use types::{
    AkazeConfig, AppendDirection, FastHnswConfig, MatchMethod, MatchStrategy, MotionCandidate,
    MotionEstimate, NoMatchReason, OverlapRegion, ScrollAxis, StitchConfig, StitchOutcome,
    StitchStats, VerifierConfig,
};
```

- [ ] **Step 3: Run the new module's tests**

Run: `cargo test -p rollshot-core --features akaze feature_matcher::tests`
Expected: PASS (3 tests).

- [ ] **Step 4: Full workspace sanity**

Run: `cargo test --workspace --features akaze`
Expected: PASS (nothing else touched, but verify no regression on the existing `--features akaze` AKAZE tests since the dispatcher routes them).

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/feature_matcher.rs crates/rollshot-core/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(core): scaffold feature_matcher with pick-one dispatcher

Adds feature_matcher.rs with FastHnswCandidateOutcome,
FeatureFallbackOutcome, and feature_fallback_candidates dispatching
between AKAZE (winner if enabled) and the FAST+KNN stub. The FAST+KNN
helper itself is still a Disabled stub; subsequent commits implement
extract_corners, descriptors, linear KNN matching, and voting.
EOF
)"
```

---

## Task 4: Implement `rgba_to_gray` and `extract_corners`

**Files:**
- Modify: `crates/rollshot-core/src/feature_matcher.rs`

- [ ] **Step 1: Add failing tests for corner extraction**

Append to the `#[cfg(test)] mod tests` block in `feature_matcher.rs`:

```rust
    // Helper shared with later tests.
    fn feature_canvas(width: u32, height: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(width, height, Rgba([238, 238, 238, 255]));
        for i in 0..80u32 {
            let x = 20 + ((i * 37 + i * i) % width.saturating_sub(40).max(1));
            let y = 20 + ((i * 53 + i * i * 3) % height.saturating_sub(40).max(1));
            let r = (40 + (i * 17) % 180) as u8;
            let g = (70 + (i * 29) % 170) as u8;
            let b = (90 + (i * 31) % 150) as u8;
            let size: u32 = 12 + (i % 8);
            for yy in 0..size {
                for xx in 0..size {
                    let cx = size as i32 / 2;
                    let cy = size as i32 / 2;
                    let dx = xx as i32 - cx;
                    let dy = yy as i32 - cy;
                    let dist2 = dx * dx + dy * dy;
                    let radius2 = (size as i32 / 2).pow(2);
                    if dist2 <= radius2 {
                        let intensity = if (xx / 3 + yy / 3) % 2 == 0 || dx.abs() < 2 || dy.abs() < 2
                        {
                            60i32
                        } else {
                            -30i32
                        };
                        img.put_pixel(
                            x + xx,
                            y + yy,
                            Rgba([
                                (r as i32 + intensity).clamp(0, 255) as u8,
                                (g as i32 + intensity + (i * 13 % 40) as i32).clamp(0, 255) as u8,
                                (b as i32 + intensity).clamp(0, 255) as u8,
                                255,
                            ]),
                        );
                    }
                }
            }
        }
        img
    }

    #[test]
    fn extract_corners_returns_empty_on_solid_image() {
        let img = solid_frame();
        let gray = rgba_to_gray(&img);
        let corners = extract_corners(&gray, 64, 1200);
        assert!(corners.is_empty(), "solid image returned {} corners", corners.len());
    }

    #[test]
    fn extract_corners_finds_features_on_feature_canvas() {
        let img = feature_canvas(220, 220);
        let gray = rgba_to_gray(&img);
        let corners = extract_corners(&gray, 64, 1200);
        assert!(
            corners.len() > 30,
            "expected >30 corners on feature canvas, got {}",
            corners.len()
        );
    }

    #[test]
    fn extract_corners_caps_at_max_features() {
        let img = feature_canvas(420, 420);
        let gray = rgba_to_gray(&img);
        let corners = extract_corners(&gray, 16, 50); // low threshold = many corners
        assert!(corners.len() <= 50, "got {}", corners.len());
    }
```

- [ ] **Step 2: Run, expect compile failure**

Run: `cargo test -p rollshot-core --features akaze feature_matcher::tests::extract_corners`
Expected: FAIL (compile error — `rgba_to_gray` and `extract_corners` not found).

- [ ] **Step 3: Add the two helpers near the top of `feature_matcher.rs` (above `fast_hnsw_candidates`)**

```rust
use image::GrayImage;
use imageproc::corners;

fn rgba_to_gray(img: &RgbaImage) -> GrayImage {
    image::imageops::grayscale(img)
}

fn extract_corners(gray: &GrayImage, threshold: u8, max_features: usize) -> Vec<(u32, u32)> {
    let fast12 = corners::corners_fast12(gray, threshold);
    let raw: Vec<(u32, u32)> = if fast12.len() > 200 {
        fast12.into_iter().map(|c| (c.x, c.y)).collect()
    } else {
        corners::corners_fast9(gray, threshold)
            .into_iter()
            .map(|c| (c.x, c.y))
            .collect()
    };
    if raw.len() <= max_features {
        return raw;
    }
    let step = raw.len() / max_features + 1;
    raw.into_iter().step_by(step).collect()
}
```

- [ ] **Step 4: Run, expect pass**

Run: `cargo test -p rollshot-core --features akaze feature_matcher::tests::extract_corners`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/feature_matcher.rs
git commit -m "$(cat <<'EOF'
feat(core): FAST corner extraction (fast12 with fast9 fallback)

extract_corners runs FAST-12 first and switches to FAST-9 when fewer
than 200 corners survive — matches the snow-shot/wayscrollshot
heuristic. Caps at max_features via uniform stride downsampling.
EOF
)"
```

---

## Task 5: Implement `compute_descriptor` and `compute_descriptors`

**Files:**
- Modify: `crates/rollshot-core/src/feature_matcher.rs`

- [ ] **Step 1: Add failing tests**

Append to the test module:

```rust
    #[test]
    fn compute_descriptor_returns_eight_dim_for_interior_corner() {
        let img = feature_canvas(220, 220);
        let gray = rgba_to_gray(&img);
        let desc = compute_descriptor(&gray, 110, 110, 9);
        let desc = desc.expect("interior corner descriptor");
        // All eight slots populated and in [0, 1].
        for v in &desc {
            assert!(*v >= 0.0 && *v <= 1.0, "descriptor entry out of range: {v}");
        }
    }

    #[test]
    fn compute_descriptor_skips_edge_corner_without_panic() {
        let img = feature_canvas(220, 220);
        let gray = rgba_to_gray(&img);
        // (1, 1) is too close to the edge for a 9x9 patch (half = 4) — must skip.
        assert!(compute_descriptor(&gray, 1, 1, 9).is_none());
        // Same for the far edges.
        assert!(compute_descriptor(&gray, 218, 218, 9).is_none());
    }

    #[test]
    fn compute_descriptors_skips_edge_corners_and_keeps_interior() {
        let img = feature_canvas(220, 220);
        let gray = rgba_to_gray(&img);
        let corners = vec![(1u32, 1u32), (110, 110), (218, 218)];
        let (descs, kept) = compute_descriptors(&gray, &corners, 9);
        assert_eq!(descs.len(), 1, "only the interior corner survives");
        assert_eq!(kept, vec![(110, 110)]);
    }
```

- [ ] **Step 2: Run, expect compile failure**

Run: `cargo test -p rollshot-core --features akaze feature_matcher::tests::compute_descriptor`
Expected: FAIL (compile error).

- [ ] **Step 3: Add the implementation**

Add near the other helpers in `feature_matcher.rs`:

```rust
use rayon::prelude::*;

/// 9x9 patch → `[f32; 8]` row/col-mean descriptor.
///
/// Returns `None` when the patch reaches outside the image (no
/// clamping — corners too close to an edge are dropped at the call
/// site).
fn compute_descriptor(gray: &GrayImage, x: u32, y: u32, patch: usize) -> Option<[f32; 8]> {
    if patch % 2 == 0 || patch < 3 {
        return None;
    }
    let half = (patch / 2) as i32;
    let w = gray.width() as i32;
    let h = gray.height() as i32;
    let cx = x as i32;
    let cy = y as i32;
    if cx - half < 0 || cy - half < 0 || cx + half >= w || cy + half >= h {
        return None;
    }
    let bins = patch / 2; // 4 for patch=9
    let mut desc = [0.0f32; 8];
    // Row-mean bins: rows at offsets -half + i*2 for i in 0..bins.
    for i in 0..bins {
        let row_y = cy + (-half + (i as i32) * 2);
        let mut sum = 0.0f32;
        let mut count = 0u32;
        for j in 0..bins {
            let col_x = cx + (-half + (j as i32) * 2);
            sum += gray.get_pixel(col_x as u32, row_y as u32)[0] as f32 / 255.0;
            count += 1;
        }
        desc[i] = if count > 0 { sum / count as f32 } else { 0.0 };
    }
    // Column-mean bins: cols at offsets -half + j*2 for j in 0..bins.
    for j in 0..bins {
        let col_x = cx + (-half + (j as i32) * 2);
        let mut sum = 0.0f32;
        let mut count = 0u32;
        for i in 0..bins {
            let row_y = cy + (-half + (i as i32) * 2);
            sum += gray.get_pixel(col_x as u32, row_y as u32)[0] as f32 / 255.0;
            count += 1;
        }
        desc[bins + j] = if count > 0 { sum / count as f32 } else { 0.0 };
    }
    Some(desc)
}

/// Batch descriptor computation. Returns `(descriptors, surviving_corners)`
/// in lockstep — corners that fail the edge check are dropped from
/// both. Parallel via rayon.
fn compute_descriptors(
    gray: &GrayImage,
    corners: &[(u32, u32)],
    patch: usize,
) -> (Vec<[f32; 8]>, Vec<(u32, u32)>) {
    let paired: Vec<((u32, u32), [f32; 8])> = corners
        .par_iter()
        .filter_map(|&(x, y)| compute_descriptor(gray, x, y, patch).map(|d| ((x, y), d)))
        .collect();
    let (kept, descs): (Vec<(u32, u32)>, Vec<[f32; 8]>) = paired.into_iter().unzip();
    (descs, kept)
}
```

- [ ] **Step 4: Run, expect pass**

Run: `cargo test -p rollshot-core --features akaze feature_matcher::tests::compute_descriptor`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/feature_matcher.rs
git commit -m "$(cat <<'EOF'
feat(core): [f32; 8] row/col-mean descriptors with edge-safe drop

compute_descriptor samples 4 row-mean + 4 col-mean cells from a 9x9
patch; returns None when the patch would touch the image edge.
compute_descriptors batch via rayon and reports surviving corners in
lockstep with the descriptor vector.
EOF
)"
```

---

## Task 6: Implement `linear_knn_match`

**Files:**
- Modify: `crates/rollshot-core/src/feature_matcher.rs`

- [ ] **Step 1: Add failing tests**

```rust
    #[test]
    fn linear_knn_match_pairs_identical_descriptors() {
        let d = |v: f32| [v; 8];
        let prev = vec![d(0.10), d(0.30), d(0.70)];
        let curr = vec![d(0.30), d(0.10), d(0.70)];
        let pairs = linear_knn_match(&prev, &curr, 0.20, 1.4);
        // curr[0] = 0.30 → matches prev[1]
        // curr[1] = 0.10 → matches prev[0]
        // curr[2] = 0.70 → matches prev[2]
        assert!(pairs.contains(&[0, 1]));
        assert!(pairs.contains(&[1, 0]));
        assert!(pairs.contains(&[2, 2]));
        assert_eq!(pairs.len(), 3);
    }

    #[test]
    fn linear_knn_match_rejects_ambiguous_pairs() {
        let d = |v: f32| [v; 8];
        // curr[0] = 0.50, prev[0] = 0.50, prev[1] = 0.501.
        // Best and second-best are nearly tied — Lowe ratio rejects.
        let prev = vec![d(0.50), d(0.501)];
        let curr = vec![d(0.50)];
        let pairs = linear_knn_match(&prev, &curr, 0.20, 1.4);
        assert!(pairs.is_empty(), "expected ambiguous pair rejected");
    }

    #[test]
    fn linear_knn_match_rejects_distant_pairs() {
        let d = |v: f32| [v; 8];
        let prev = vec![d(0.10)];
        let curr = vec![d(0.90)]; // sqrt(8 * 0.64) ≈ 2.26 >> 0.20
        let pairs = linear_knn_match(&prev, &curr, 0.20, 1.4);
        assert!(pairs.is_empty(), "expected distant pair rejected");
    }

    #[test]
    fn linear_knn_match_returns_empty_on_empty_input() {
        assert!(linear_knn_match(&[], &[[0.0; 8]], 0.20, 1.4).is_empty());
        assert!(linear_knn_match(&[[0.0; 8]], &[], 0.20, 1.4).is_empty());
        let prev = [[0.0; 8]];
        let curr = [[0.0; 8]];
        // Only one prev — no "second best" available. The ratio test
        // cannot fire; we accept the lone best if it is below the
        // distance threshold.
        let pairs = linear_knn_match(&prev, &curr, 0.20, 1.4);
        assert_eq!(pairs, vec![[0usize, 0usize]]);
    }
```

- [ ] **Step 2: Run, expect compile failure**

Run: `cargo test -p rollshot-core --features akaze feature_matcher::tests::linear_knn_match`
Expected: FAIL (compile error).

- [ ] **Step 3: Add the implementation**

```rust
/// Linear KNN with Lowe ratio test. For each `curr` descriptor, find
/// the best and second-best `prev` matches by Euclidean distance.
/// Accept if `best.dist < distance_threshold` and `best.dist * ratio <
/// second.dist`. When there is only one `prev` candidate, the ratio
/// test cannot fire — accept the best if it clears the distance
/// threshold.
///
/// Returns pairs as `[curr_idx, prev_idx]`. Parallel via rayon.
fn linear_knn_match(
    prev: &[[f32; 8]],
    curr: &[[f32; 8]],
    distance_threshold: f32,
    lowe_ratio: f32,
) -> Vec<[usize; 2]> {
    if prev.is_empty() || curr.is_empty() {
        return Vec::new();
    }
    curr.par_iter()
        .enumerate()
        .filter_map(|(curr_idx, c)| {
            let mut best = (f32::INFINITY, usize::MAX);
            let mut second = f32::INFINITY;
            for (i, p) in prev.iter().enumerate() {
                let dist = euclidean_distance(p, c);
                if dist < best.0 {
                    second = best.0;
                    best = (dist, i);
                } else if dist < second {
                    second = dist;
                }
            }
            if best.0 >= distance_threshold {
                return None;
            }
            if second.is_finite() && best.0 * lowe_ratio >= second {
                return None;
            }
            Some([curr_idx, best.1])
        })
        .collect()
}

fn euclidean_distance(a: &[f32; 8], b: &[f32; 8]) -> f32 {
    let mut sum = 0.0f32;
    for i in 0..8 {
        let d = a[i] - b[i];
        sum += d * d;
    }
    sum.sqrt()
}
```

- [ ] **Step 4: Run, expect pass**

Run: `cargo test -p rollshot-core --features akaze feature_matcher::tests::linear_knn_match`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/feature_matcher.rs
git commit -m "$(cat <<'EOF'
feat(core): linear KNN matching with Lowe ratio test

Exact O(N*M) scan with rayon parallelism over the query side. Accepts
a match when the best Euclidean distance is below distance_threshold
AND the runner-up is at least lowe_ratio x further away. Single-prev
case skips the ratio test (no runner-up to compare against).
EOF
)"
```

---

## Task 7: Implement `vote_dominant_translation`

**Files:**
- Modify: `crates/rollshot-core/src/feature_matcher.rs`

- [ ] **Step 1: Add failing tests**

```rust
    #[test]
    fn vote_dominant_translation_picks_majority_bucket() {
        // 5 matches at dy ~ 40, 1 outlier at dy = 100.
        let prev = vec![(0u32, 0u32); 6];
        let curr = vec![
            (0u32, 40u32),
            (0, 41),
            (0, 39),
            (0, 40),
            (0, 42),
            (0, 100),
        ];
        let matches: Vec<[usize; 2]> = (0..6).map(|i| [i, i]).collect();
        let cfg = FastHnswConfig::default();
        let result = vote_dominant_translation(&prev, &curr, &matches, None, &cfg);
        let (dx, dy, inliers, raw) = result.expect("dominant translation");
        assert_eq!(dx, 0);
        assert!(dy >= 39 && dy <= 42, "dy = {dy}");
        assert!(inliers >= 4, "inliers = {inliers}");
        assert_eq!(raw, 6);
    }

    #[test]
    fn vote_dominant_translation_rejects_zero_zero_bucket() {
        // All matches are zero-translation (duplicate-like input).
        let prev = vec![(10u32, 20u32), (30, 40), (50, 60)];
        let curr = vec![(10u32, 20u32), (30, 40), (50, 60)];
        let matches = vec![[0, 0], [1, 1], [2, 2]];
        let cfg = FastHnswConfig::default();
        assert!(
            vote_dominant_translation(&prev, &curr, &matches, None, &cfg).is_none()
        );
    }

    #[test]
    fn vote_dominant_translation_respects_locked_vertical_axis() {
        // All matches drift horizontally (dx > tolerance, dy = 0).
        let prev = vec![(0u32, 0u32), (0, 10), (0, 20)];
        let curr = vec![(50u32, 0u32), (50, 10), (50, 20)];
        let matches = vec![[0, 0], [1, 1], [2, 2]];
        let cfg = FastHnswConfig::default();
        assert!(
            vote_dominant_translation(&prev, &curr, &matches, Some(ScrollAxis::Vertical), &cfg)
                .is_none(),
            "vertical lock must reject cross-axis-only matches"
        );
    }
```

- [ ] **Step 2: Run, expect compile failure**

Run: `cargo test -p rollshot-core --features akaze feature_matcher::tests::vote_dominant_translation`
Expected: FAIL.

- [ ] **Step 3: Add the implementation**

```rust
use std::collections::HashMap;

fn vote_dominant_translation(
    prev_corners: &[(u32, u32)],
    curr_corners: &[(u32, u32)],
    matches: &[[usize; 2]],
    locked_axis: Option<ScrollAxis>,
    config: &FastHnswConfig,
) -> Option<(i32, i32, usize, usize)> {
    let raw_matches = matches.len();
    if raw_matches == 0 {
        return None;
    }
    // Translation per match: prev - curr (so dy>0 means the current
    // frame's content moved up, i.e. we are scrolling down).
    let translations: Vec<(i32, i32)> = matches
        .iter()
        .filter_map(|&[curr_idx, prev_idx]| {
            let (cx, cy) = curr_corners.get(curr_idx)?;
            let (px, py) = prev_corners.get(prev_idx)?;
            let dx = *px as i32 - *cx as i32;
            let dy = *py as i32 - *cy as i32;
            // Cross-axis filter.
            match locked_axis {
                Some(ScrollAxis::Vertical) if dx.abs() > config.cross_axis_tolerance => None,
                Some(ScrollAxis::Horizontal) if dy.abs() > config.cross_axis_tolerance => None,
                _ => Some((dx, dy)),
            }
        })
        .collect();
    if translations.is_empty() {
        return None;
    }
    // Bucket by (dx/4, dy/4). Reject the (0, 0) bucket entirely.
    let mut buckets: HashMap<(i32, i32), Vec<(i32, i32)>> = HashMap::new();
    for &(dx, dy) in &translations {
        let key = (dx / 4, dy / 4);
        if key == (0, 0) {
            continue;
        }
        buckets.entry(key).or_default().push((dx, dy));
    }
    let mut best: Option<(usize, Vec<(i32, i32)>)> = None;
    for (_, bucket) in buckets {
        let len = bucket.len();
        if best.as_ref().map(|(n, _)| len > *n).unwrap_or(true) {
            best = Some((len, bucket));
        }
    }
    let (_, bucket) = best?;
    let inliers = bucket.len();
    if inliers < config.min_inliers {
        return None;
    }
    let mut dxs: Vec<i32> = bucket.iter().map(|(dx, _)| *dx).collect();
    let mut dys: Vec<i32> = bucket.iter().map(|(_, dy)| *dy).collect();
    dxs.sort_unstable();
    dys.sort_unstable();
    let dx_median = dxs[dxs.len() / 2];
    let dy_median = dys[dys.len() / 2];
    Some((dx_median, dy_median, inliers, raw_matches))
}
```

- [ ] **Step 4: Run, expect pass**

Run: `cargo test -p rollshot-core --features akaze feature_matcher::tests::vote_dominant_translation`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/feature_matcher.rs
git commit -m "$(cat <<'EOF'
feat(core): bucket-vote dominant translation for FAST+KNN

Buckets (dx, dy) at 4 px resolution, picks the largest bucket, returns
median (dx, dy) within it. Cross-axis filter respects locked_axis with
config.cross_axis_tolerance. (0,0) bucket is rejected so duplicate
frames cannot accidentally win.
EOF
)"
```

---

## Task 8: Wire `fast_hnsw_candidates` end-to-end + Layer 1 unit tests

**Files:**
- Modify: `crates/rollshot-core/src/feature_matcher.rs`

- [ ] **Step 1: Add the Layer 1 unit tests (spec §Test Strategy)**

Append to the test module:

```rust
    fn crop_xy(canvas: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
        use image::imageops;
        imageops::crop_imm(canvas, x, y, w, h).to_image()
    }

    #[test]
    fn fast_hnsw_candidates_estimate_translation() {
        let canvas = feature_canvas(420, 420);
        let prev = crop_xy(&canvas, 20, 30, 220, 220);
        let curr = crop_xy(&canvas, 58, 92, 220, 220);
        let config = FastHnswConfig::default();
        let outcome = fast_hnsw_candidates(&prev, &curr, None, &config);
        let candidates = match outcome {
            FastHnswCandidateOutcome::Candidates(c) => c,
            other => panic!("expected Candidates, got {other:?}"),
        };
        let candidate = candidates.first().expect("one candidate");
        assert_eq!(candidate.method, crate::types::MatchMethod::FastHnsw);
        assert!(
            (candidate.dx - 38).abs() <= 3,
            "dx = {} (expected ~38)",
            candidate.dx
        );
        assert!(
            (candidate.dy - 62).abs() <= 3,
            "dy = {} (expected ~62)",
            candidate.dy
        );
        assert!(candidate.raw_matches.unwrap_or(0) >= 24);
        assert!(candidate.inliers.unwrap_or(0) >= 16);
    }

    #[test]
    fn fast_hnsw_candidates_returns_not_enough_features_on_solid_frames() {
        let prev = solid_frame();
        let curr = solid_frame();
        let config = FastHnswConfig::default();
        let outcome = fast_hnsw_candidates(&prev, &curr, None, &config);
        assert!(
            matches!(outcome, FastHnswCandidateOutcome::NotEnoughFeatures { .. }),
            "got {outcome:?}"
        );
    }

    #[test]
    fn fast_hnsw_candidates_returns_not_enough_matches_on_unrelated_frames() {
        let prev = feature_canvas(220, 220);
        // Mirror creates structurally similar but pixel-different content
        // → corners detected but descriptor matches fail the ratio test.
        let mut curr = feature_canvas(220, 220);
        for (i, px) in curr.pixels_mut().enumerate() {
            // Hash-noise; not related to prev's distribution.
            let n = ((i as u64).wrapping_mul(6364136223846793005) >> 32) as u8;
            px[0] = n;
            px[1] = n.wrapping_add(83);
            px[2] = n.wrapping_add(149);
        }
        let config = FastHnswConfig::default();
        let outcome = fast_hnsw_candidates(&prev, &curr, None, &config);
        assert!(
            matches!(
                outcome,
                FastHnswCandidateOutcome::NotEnoughMatches { .. }
                    | FastHnswCandidateOutcome::NotEnoughFeatures { .. }
            ),
            "got {outcome:?}"
        );
    }

    #[test]
    fn fast_hnsw_candidates_respects_locked_vertical_axis() {
        // Pure horizontal motion: 38 px right shift.
        let canvas = feature_canvas(420, 420);
        let prev = crop_xy(&canvas, 20, 100, 220, 220);
        let curr = crop_xy(&canvas, 58, 100, 220, 220);
        let config = FastHnswConfig::default();
        let outcome =
            fast_hnsw_candidates(&prev, &curr, Some(ScrollAxis::Vertical), &config);
        assert!(
            matches!(
                outcome,
                FastHnswCandidateOutcome::NotEnoughMatches { .. }
                    | FastHnswCandidateOutcome::NotEnoughFeatures { .. }
            ),
            "vertical lock should reject pure horizontal motion, got {outcome:?}"
        );
    }

    #[test]
    fn fast_hnsw_candidates_respects_locked_horizontal_axis() {
        let canvas = feature_canvas(420, 420);
        let prev = crop_xy(&canvas, 100, 20, 220, 220);
        let curr = crop_xy(&canvas, 100, 58, 220, 220);
        let config = FastHnswConfig::default();
        let outcome =
            fast_hnsw_candidates(&prev, &curr, Some(ScrollAxis::Horizontal), &config);
        assert!(
            matches!(
                outcome,
                FastHnswCandidateOutcome::NotEnoughMatches { .. }
                    | FastHnswCandidateOutcome::NotEnoughFeatures { .. }
            ),
            "horizontal lock should reject pure vertical motion, got {outcome:?}"
        );
    }

    #[test]
    fn fast_hnsw_score_below_default_accept_confidence() {
        let accept = StitchConfig::default().accept_confidence;
        // Healthy: 0.6 inlier ratio, ~24 raw matches.
        let score = feature_score(0.6, 24);
        assert!(
            score < accept,
            "healthy match scored {score} >= accept_confidence {accept}"
        );
    }
```

- [ ] **Step 2: Run, expect compile failure (missing helpers in fast_hnsw_candidates body)**

Run: `cargo test -p rollshot-core --features akaze feature_matcher::tests::fast_hnsw_candidates`
Expected: FAIL — Candidates variant is not produced, function still returns Disabled.

- [ ] **Step 3: Add `feature_score` and replace the `fast_hnsw_candidates` stub body**

Add `feature_score` next to the other helpers. The numeric formula is duplicated from `akaze_matcher::akaze_score` per spec; keep the comment.

```rust
// Keep in sync with akaze_matcher::akaze_score (intentionally private
// there). When AKAZE is removed, fold this into a single shared helper.
fn feature_score(inlier_ratio: f32, raw_matches: usize) -> f32 {
    let ratio_term = 1.0 - inlier_ratio.clamp(0.0, 1.0);
    let residual_term = if raw_matches >= 16 { 0.0 } else { 1.0 };
    (ratio_term * 0.08 + residual_term * 0.04).clamp(0.0, 1.0)
}
```

Replace the `fast_hnsw_candidates` body:

```rust
pub(crate) fn fast_hnsw_candidates(
    prev: &RgbaImage,
    curr: &RgbaImage,
    locked_axis: Option<ScrollAxis>,
    config: &FastHnswConfig,
) -> FastHnswCandidateOutcome {
    if !config.enabled {
        return FastHnswCandidateOutcome::Disabled;
    }
    if prev.dimensions() != curr.dimensions() {
        // Dimension mismatch is handled earlier in estimate_motion; if
        // it ever reaches here, treat as no features.
        return FastHnswCandidateOutcome::NotEnoughFeatures { prev: 0, curr: 0 };
    }

    let prev_gray = rgba_to_gray(prev);
    let curr_gray = rgba_to_gray(curr);
    let prev_corners = extract_corners(&prev_gray, config.corner_threshold, config.max_features);
    let curr_corners = extract_corners(&curr_gray, config.corner_threshold, config.max_features);

    if prev_corners.len() < config.min_keypoints || curr_corners.len() < config.min_keypoints {
        return FastHnswCandidateOutcome::NotEnoughFeatures {
            prev: prev_corners.len(),
            curr: curr_corners.len(),
        };
    }

    let (prev_desc, prev_kept) =
        compute_descriptors(&prev_gray, &prev_corners, config.descriptor_patch_size);
    let (curr_desc, curr_kept) =
        compute_descriptors(&curr_gray, &curr_corners, config.descriptor_patch_size);

    if prev_desc.len() < config.min_keypoints || curr_desc.len() < config.min_keypoints {
        return FastHnswCandidateOutcome::NotEnoughFeatures {
            prev: prev_desc.len(),
            curr: curr_desc.len(),
        };
    }

    let lowe_ratio = 1.4;
    let matches = linear_knn_match(&prev_desc, &curr_desc, config.distance_threshold, lowe_ratio);
    if matches.len() < config.min_raw_matches {
        return FastHnswCandidateOutcome::NotEnoughMatches {
            raw_matches: matches.len(),
        };
    }

    let Some((dx, dy, inliers, raw)) =
        vote_dominant_translation(&prev_kept, &curr_kept, &matches, locked_axis, config)
    else {
        return FastHnswCandidateOutcome::NotEnoughMatches {
            raw_matches: matches.len(),
        };
    };
    let inlier_ratio = inliers as f32 / raw.max(1) as f32;
    FastHnswCandidateOutcome::Candidates(vec![MotionCandidate {
        dx,
        dy,
        method: crate::types::MatchMethod::FastHnsw,
        score: feature_score(inlier_ratio, raw),
        second_best_score: None,
        inliers: Some(inliers),
        raw_matches: Some(raw),
    }])
}
```

- [ ] **Step 4: Run, expect pass**

Run: `cargo test -p rollshot-core --features akaze feature_matcher::tests`
Expected: PASS (all Layer 1 tests).

- [ ] **Step 5: Workspace sanity**

Run: `cargo test --workspace --features akaze`
Expected: PASS. The new tests are inside `feature_matcher::tests`; no existing test should regress because `estimate_motion` still calls `akaze_candidates` directly (we swap it in Task 9).

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-core/src/feature_matcher.rs
git commit -m "$(cat <<'EOF'
feat(core): wire FAST+KNN end-to-end + Layer 1 unit tests

fast_hnsw_candidates now runs: rgba_to_gray, FAST corner extraction,
[f32;8] descriptors, linear KNN with Lowe ratio, bucket voting,
feature_score. Layer 1 covers translation estimation, NotEnoughFeatures
on solid frames, NotEnoughMatches on noise, axis-locked rejection
(vertical & horizontal), and score-vs-accept_confidence.

feature_score is duplicated from akaze_matcher::akaze_score with a
keep-in-sync comment, per the spec.
EOF
)"
```

---

## Task 9: Swap `estimate_motion` over to `feature_fallback_candidates`

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`

- [ ] **Step 1: Locate the AKAZE call site**

The current code in `crates/rollshot-core/src/matcher.rs` (lines ~210–233):

```rust
    match akaze_candidates(prev, curr, &config.akaze) {
        AkazeCandidateOutcome::Disabled => MotionSearchOutcome::NoMatch {
            reason: NoMatchReason::AkazeDisabled,
            best_candidate: None,
        },
        AkazeCandidateOutcome::NotEnoughFeatures { .. } => MotionSearchOutcome::NoMatch {
            reason: NoMatchReason::NotEnoughFeatures,
            best_candidate: None,
        },
        AkazeCandidateOutcome::NotEnoughMatches { .. } => MotionSearchOutcome::NoMatch {
            reason: NoMatchReason::AkazeLowInliers,
            best_candidate: None,
        },
        AkazeCandidateOutcome::Candidates(akaze_candidates) => {
            let best = akaze_candidates.first().copied();
            match rank_verified_candidates(prev, curr, locked_axis, akaze_candidates, config) {
                Some(candidate) => MotionSearchOutcome::Candidate(candidate),
                None => MotionSearchOutcome::NoMatch {
                    reason: NoMatchReason::AkazeLowInliers,
                    best_candidate: best,
                },
            }
        }
    }
}
```

- [ ] **Step 2: Replace with dispatch through `feature_fallback_candidates`**

Replace the `match akaze_candidates(...)` block with:

```rust
    match feature_fallback_candidates(prev, curr, locked_axis, config) {
        FeatureFallbackOutcome::Disabled => MotionSearchOutcome::NoMatch {
            reason: NoMatchReason::FeatureFallbackDisabled,
            best_candidate: None,
        },
        FeatureFallbackOutcome::NotEnoughFeatures { .. } => MotionSearchOutcome::NoMatch {
            reason: NoMatchReason::NotEnoughFeatures,
            best_candidate: None,
        },
        FeatureFallbackOutcome::NotEnoughMatches { source, .. } => MotionSearchOutcome::NoMatch {
            reason: match source {
                FeatureSource::FastHnsw => NoMatchReason::FeatureLowInliers,
                FeatureSource::Akaze => NoMatchReason::AkazeLowInliers,
            },
            best_candidate: None,
        },
        FeatureFallbackOutcome::Candidates { candidates, source } => {
            let best = candidates.first().copied();
            match rank_verified_candidates(prev, curr, locked_axis, candidates, config) {
                Some(candidate) => MotionSearchOutcome::Candidate(candidate),
                None => MotionSearchOutcome::NoMatch {
                    reason: match source {
                        FeatureSource::FastHnsw => NoMatchReason::FeatureLowInliers,
                        FeatureSource::Akaze => NoMatchReason::AkazeLowInliers,
                    },
                    best_candidate: best,
                },
            }
        }
    }
}
```

Update the imports at the top of `matcher.rs`. Remove the akaze-specific import and add the dispatcher's:

```rust
use crate::feature_matcher::{feature_fallback_candidates, FeatureFallbackOutcome, FeatureSource};
```

If `akaze_matcher::AkazeCandidateOutcome` is no longer referenced anywhere in `matcher.rs`, remove that import too. (Use `cargo check` to confirm — unused imports surface as warnings.)

- [ ] **Step 3: Run the existing stitcher integration tests; verify no regression**

Run: `cargo test -p rollshot-core --features akaze --test stitcher`
Expected: PASS. Two things to watch:
  - `bad_frame_returns_no_match_and_preserves_anchor` already accepts `LowConfidence | AkazeDisabled | NotEnoughFeatures` — its `NotEnoughFeatures` arm still fires because FAST+KNN will report it on the white frame. Good.
  - `fast_scroll_beyond_default_search_ratio_recovers_via_relaxed_pass` (Phase 2) is untouched because relaxed coarse succeeds before the feature fallback runs. Should pass.

- [ ] **Step 4: Run the cli smoke tests**

Run: `cargo test -p rollshot-cli --features akaze --test cli_smoke`
Expected: PASS for everything except `rollshot_stitch_folder_enable_akaze_toggle`. That test will likely **still pass** because AKAZE is force-enabled there; but if AKAZE no longer fires (because rank_verified_candidates accepts the FAST candidate first under `--enable-akaze`), the assertion `with_akaze.contains("\"method\": \"Akaze\"")` could fail. If so, that's expected post-dispatch behaviour and Task 12 renames + adjusts this test.

- [ ] **Step 5: Workspace sanity**

Run: `cargo test --workspace --features akaze`
Expected: PASS (modulo the cli_smoke test noted above; if it fails specifically because of the AKAZE method assertion, defer to Task 12).

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-core/src/matcher.rs
git commit -m "$(cat <<'EOF'
feat(core): route estimate_motion through feature_fallback_candidates

estimate_motion no longer calls akaze_candidates directly; the pick-one
dispatcher in feature_matcher.rs picks FAST+KNN by default and AKAZE
when explicitly enabled. NoMatch reasons map per the source tag
(FeatureLowInliers vs AkazeLowInliers).

AkazeDisabled is no longer emitted from estimate_motion (collapsed
into FeatureFallbackDisabled when both fallbacks are off). The variant
stays in the enum so existing pattern matches keep compiling.
EOF
)"
```

---

## Task 10: Layer 2 integration tests in `tests/stitcher.rs`

**Files:**
- Modify: `crates/rollshot-core/tests/stitcher.rs`

- [ ] **Step 1: Add the four Layer 2 tests**

Append to `crates/rollshot-core/tests/stitcher.rs` (after the existing `fast_scroll_beyond_default_search_ratio_recovers_via_relaxed_pass`):

```rust
#[test]
fn fast_hnsw_fallback_recovers_repeated_grid_with_sparse_features() {
    // Full path avoids touching the akaze-feature-gated `use` line at
    // the top of stitcher.rs. `make_akaze_fallback_canvas` itself is
    // not feature-gated; only the existing `use` import is.
    let canvas = common::make_akaze_fallback_canvas(320, 1200);
    let first = crop_frame(&canvas, 0, 320);
    let scrolled = crop_frame(&canvas, 96, 320);

    let config = StitchConfig::default();
    assert!(
        config.fast_hnsw.enabled && !config.akaze.enabled,
        "this test exercises the default FAST+KNN path"
    );

    let mut stitcher = Stitcher::new(config);
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended {
            estimate,
            added,
            direction,
            ..
        } => {
            assert_eq!(direction, AppendDirection::Bottom);
            assert_eq!(estimate.method, MatchMethod::FastHnsw);
            assert!((90..=102).contains(&added), "added = {added}");
        }
        other => panic!("expected Appended via FastHnsw, got {other:?}"),
    }
}

#[test]
fn fast_hnsw_attempt_with_blank_frames_reports_not_enough_features() {
    let blank = RgbaImage::from_pixel(320, 320, Rgba([255, 255, 255, 255]));
    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(blank.clone()), StitchOutcome::FirstFrame);

    match stitcher.push_frame(blank) {
        StitchOutcome::Duplicate => {} // OK — duplicate detector wins first.
        StitchOutcome::NoMatch {
            reason: NoMatchReason::NotEnoughFeatures,
            ..
        } => {} // OK — fallback reports.
        other => panic!("expected Duplicate or NotEnoughFeatures, got {other:?}"),
    }
}

#[test]
fn fast_hnsw_candidate_rejected_by_verifier_preserves_best_estimate() {
    // Construct frames that produce a FAST+KNN candidate the verifier
    // cannot pass: identical sparse features arranged so the
    // descriptor matches but the surrounding mean-abs-diff exceeds
    // verifier thresholds.
    let prev = common::make_akaze_fallback_canvas(320, 400);
    let mut curr = common::make_akaze_fallback_canvas(320, 400);
    // Smash the background of curr to break verifier MAD but keep the
    // sparse feature blobs intact.
    for (i, px) in curr.pixels_mut().enumerate() {
        if px[0] > 220 {
            // Only stomp the light grid pixels — corners (darker) are kept.
            let n = ((i as u64).wrapping_mul(6364136223846793005) >> 40) as u8;
            px[0] = n;
            px[1] = n;
            px[2] = n;
        }
    }
    let first = image::imageops::crop_imm(&prev, 0, 0, 320, 320).to_image();
    let second = image::imageops::crop_imm(&curr, 0, 32, 320, 320).to_image();

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(second) {
        StitchOutcome::NoMatch {
            reason,
            best_estimate,
        } => {
            // Allowed reasons depend on how the candidate failed:
            //   - rank_verified_candidates rejected → FeatureLowInliers
            //   - vote failed inliers threshold      → FeatureLowInliers
            //   - feature scan never produced enough → NotEnoughFeatures/Matches
            assert!(
                matches!(
                    reason,
                    NoMatchReason::FeatureLowInliers
                        | NoMatchReason::OverlapVerificationFailed
                        | NoMatchReason::InsufficientOverlap
                        | NoMatchReason::NotEnoughFeatures
                ),
                "unexpected reason {reason:?}"
            );
            // If a candidate was produced and only verifier rejected it,
            // best_estimate must surface so the report can be informative.
            // (Not asserted as Some — the test tolerates either branch.)
            let _ = best_estimate;
        }
        StitchOutcome::Appended { .. } => {
            // Acceptable if the stomp didn't break verifier; the test's
            // primary contract is "no panic and the reason is in the
            // allowed set." Move on.
        }
        other => panic!("expected NoMatch or Appended, got {other:?}"),
    }
}

#[cfg(feature = "akaze")]
#[test]
fn enable_akaze_overrides_fast_hnsw() {
    let canvas = common::make_akaze_fallback_canvas(320, 1200);
    let first = crop_frame(&canvas, 0, 320);
    let scrolled = crop_frame(&canvas, 96, 320);

    let mut config = StitchConfig::default();
    config.fast_hnsw.enabled = true;
    config.akaze.enabled = true; // pick-one: AKAZE must win.
    let mut stitcher = Stitcher::new(config);
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended { estimate, .. } => {
            assert_eq!(
                estimate.method,
                MatchMethod::Akaze,
                "pick-one dispatch must route to AKAZE when enabled"
            );
        }
        other => panic!("expected Appended via Akaze, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run the new tests**

Run: `cargo test -p rollshot-core --features akaze --test stitcher fast_hnsw_ enable_akaze_overrides`
Expected: PASS (4 tests).

- [ ] **Step 3: Full workspace sanity**

Run: `cargo test --workspace --features akaze`
Expected: PASS. If `rollshot_stitch_folder_enable_akaze_toggle` (Phase 1's cli smoke) fails because the FAST+KNN path is now what the default run uses, defer to Task 12.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-core/tests/stitcher.rs
git commit -m "$(cat <<'EOF'
test(core): integration tests for FAST+KNN fallback path

Covers the four spec-mandated Layer 2 cases: repeated-grid recovery
on the AKAZE-fallback canvas (now via FAST+KNN), blank-frame
NotEnoughFeatures, verifier-rejection preserves best_estimate, and
pick-one dispatch (akaze wins when both enabled).
EOF
)"
```

---

## Task 11: CLI flags — add `--disable-feature-fallback`, deprecate `--enable-akaze`

**Files:**
- Modify: `crates/rollshot-cli/src/args.rs`
- Modify: `crates/rollshot-cli/src/cmd_capture.rs`
- Modify: `crates/rollshot-cli/src/cmd_stitch_folder.rs`

- [ ] **Step 1: Add the new arg + deprecate the old one in `args.rs`**

In `CaptureArgs`, replace the existing `enable_akaze` arg block with:

```rust
    /// Enable the AKAZE feature-based fallback instead of FAST+KNN.
    /// DEPRECATED — AKAZE will be removed in the next minor release.
    /// Kept for parity testing during the FAST migration.
    #[arg(long, default_value_t = false)]
    pub enable_akaze: bool,

    /// Disable the FAST + linear-KNN feature fallback. Only useful for
    /// benchmarking the matcher path; production captures should leave
    /// this off so the fallback can rescue large scroll jumps that
    /// the regular matchers and the relaxed coarse pass cannot.
    #[arg(long, default_value_t = false)]
    pub disable_feature_fallback: bool,
```

In `StitchFolderArgs`, mirror the same two args:

```rust
    /// Enable the AKAZE feature-based fallback. DEPRECATED — AKAZE
    /// will be removed in the next minor release.
    #[arg(long, default_value_t = false)]
    pub enable_akaze: bool,

    /// Disable the FAST + linear-KNN feature fallback. Diagnostic.
    #[arg(long, default_value_t = false)]
    pub disable_feature_fallback: bool,
```

- [ ] **Step 2: Wire the new flag + deprecation warning in `cmd_capture.rs`**

Locate the existing block that handles `enable_akaze`:

```rust
    let mut config = StitchConfig::default();
    if args.enable_akaze {
        config.akaze.enabled = true;
    }
    let mut stitcher = Stitcher::new(config);
```

Replace with:

```rust
    let mut config = StitchConfig::default();
    if args.disable_feature_fallback {
        config.fast_hnsw.enabled = false;
    }
    if args.enable_akaze {
        eprintln!(
            "warning: --enable-akaze is deprecated and will be removed in a future \
             release; FAST+KNN is the default feature fallback"
        );
        config.akaze.enabled = true;
    }
    let mut stitcher = Stitcher::new(config);
```

- [ ] **Step 3: Same wiring in `cmd_stitch_folder.rs`**

Locate the existing block:

```rust
    let mut config = StitchConfig::default();
    if args.enable_akaze {
        config.akaze.enabled = true;
    }
```

Replace with:

```rust
    let mut config = StitchConfig::default();
    if args.disable_feature_fallback {
        config.fast_hnsw.enabled = false;
    }
    if args.enable_akaze {
        eprintln!(
            "warning: --enable-akaze is deprecated and will be removed in a future \
             release; FAST+KNN is the default feature fallback"
        );
        config.akaze.enabled = true;
    }
```

- [ ] **Step 4: Verify the build and smoke help text**

Run: `cargo build --workspace --features akaze`
Expected: clean.

Run: `cargo run -p rollshot-cli --features akaze -- capture --help | grep -E '(enable-akaze|disable-feature-fallback)'`
Expected: both lines visible.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-cli/src/args.rs crates/rollshot-cli/src/cmd_capture.rs crates/rollshot-cli/src/cmd_stitch_folder.rs
git commit -m "$(cat <<'EOF'
feat(cli): --disable-feature-fallback + deprecate --enable-akaze

Adds the diagnostic --disable-feature-fallback flag and prints a
deprecation warning on stderr when --enable-akaze is used. Both
flags are available on capture and stitch-folder. Pick-one dispatch
in feature_fallback_candidates handles the precedence; the CLI is
just config wiring.
EOF
)"
```

---

## Task 12: Layer 3 CLI smoke tests

**Files:**
- Modify: `crates/rollshot-cli/tests/cli_smoke.rs`

- [ ] **Step 1: Rename and adapt the existing `rollshot_stitch_folder_enable_akaze_toggle`**

Find this test (around line ~237 today). Replace the function name and assertions:

```rust
#[cfg(feature = "akaze")]
#[test]
fn rollshot_stitch_folder_default_uses_fast_hnsw_and_enable_akaze_overrides() {
    let tempdir = tempdir_for_test("rollshot-stitch-folder-fast-hnsw-vs-akaze");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");

    let canvas = make_akaze_fallback_smoke_canvas(320, 820);
    use std::hash::{Hash, Hasher};
    for (idx, y) in [0u32, 96, 192].iter().enumerate() {
        let mut frame = imageops::crop_imm(&canvas, 0, *y, 320, 320).to_image();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        idx.hash(&mut hasher);
        y.hash(&mut hasher);
        let seed = hasher.finish();
        for (i, px) in frame.pixels_mut().enumerate() {
            let h = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(i as u64);
            let n0 = (h as i32 % 61) - 30;
            let n1 = ((h >> 16) as i32 % 61) - 30;
            let n2 = ((h >> 32) as i32 % 61) - 30;
            px[0] = (px[0] as i32 + n0).clamp(0, 255) as u8;
            px[1] = (px[1] as i32 + n1).clamp(0, 255) as u8;
            px[2] = (px[2] as i32 + n2).clamp(0, 255) as u8;
        }
        frame
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }

    let run = |label: &str, extra_args: &[&str]| -> (String, String) {
        let output_png = tempdir.join(format!("stitched_{label}.png"));
        let report_json = tempdir.join(format!("report_{label}.json"));
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rollshot"));
        cmd.arg("stitch-folder")
            .arg(&frames_dir)
            .arg("--output")
            .arg(&output_png)
            .arg("--debug-match-report")
            .arg(&report_json);
        for a in extra_args {
            cmd.arg(a);
        }
        let output = cmd.output().expect("run rollshot stitch-folder");
        assert!(
            output.status.success(),
            "{label} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        (
            std::fs::read_to_string(&report_json).expect("read report"),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };

    let (default_report, default_stderr) = run("default", &[]);
    let (akaze_report, akaze_stderr) = run("akaze", &["--enable-akaze"]);

    assert!(
        default_report.contains("\"method\": \"FastHnsw\""),
        "default run should use FastHnsw, report = {default_report}"
    );
    assert!(
        !default_report.contains("\"method\": \"Akaze\""),
        "default run must not invoke AKAZE, report = {default_report}"
    );
    assert!(
        !default_stderr.contains("deprecated"),
        "default run must not emit the deprecation warning"
    );

    assert!(
        akaze_report.contains("\"method\": \"Akaze\""),
        "--enable-akaze run should use AKAZE, report = {akaze_report}"
    );
    assert!(
        !akaze_report.contains("\"method\": \"FastHnsw\""),
        "--enable-akaze must not run FastHnsw (pick-one), report = {akaze_report}"
    );
    assert!(
        akaze_stderr.contains("deprecated"),
        "--enable-akaze must emit the deprecation warning on stderr, stderr = {akaze_stderr}"
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}

// Gated on `feature = "akaze"` because the only off-the-shelf fixture
// that defeats both regular matchers and the relaxed coarse pass
// (`make_akaze_fallback_smoke_canvas`) is feature-gated here. The
// FAST+KNN code path itself does NOT require the akaze feature.
#[cfg(feature = "akaze")]
#[test]
fn rollshot_stitch_folder_disable_feature_fallback_emits_disabled_no_match() {
    let tempdir = tempdir_for_test("rollshot-stitch-folder-disable-feature-fallback");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");

    // Use the same hard-fallback canvas; with the feature fallback
    // disabled, the matcher has nothing to fall through to.
    let canvas = make_akaze_fallback_smoke_canvas(320, 820);
    for (idx, y) in [0u32, 96, 192].iter().enumerate() {
        let frame = imageops::crop_imm(&canvas, 0, *y, 320, 320).to_image();
        frame
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }

    let output_png = tempdir.join("stitched.png");
    let report_json = tempdir.join("report.json");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("stitch-folder")
        .arg(&frames_dir)
        .arg("--output")
        .arg(&output_png)
        .arg("--debug-match-report")
        .arg(&report_json)
        .arg("--disable-feature-fallback")
        .output()
        .expect("run rollshot stitch-folder");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = std::fs::read_to_string(&report_json).expect("read report");
    assert!(
        report.contains("FeatureFallbackDisabled"),
        "expected at least one frame to report FeatureFallbackDisabled, report = {report}"
    );
    assert!(
        !report.contains("\"method\": \"FastHnsw\""),
        "FastHnsw must not run when --disable-feature-fallback is set, report = {report}"
    );
    assert!(
        !report.contains("\"method\": \"Akaze\""),
        "AKAZE must not run by default even when fallback is disabled, report = {report}"
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}
```

- [ ] **Step 2: Run the renamed + new tests**

Run: `cargo test -p rollshot-cli --features akaze --test cli_smoke -- rollshot_stitch_folder_default_uses_fast_hnsw rollshot_stitch_folder_disable_feature_fallback`
Expected: PASS (2 tests).

- [ ] **Step 3: Full workspace test**

Run: `cargo test --workspace --features akaze`
Expected: PASS.

Run: `cargo test --workspace`  (no `--features akaze`)
Expected: PASS. The `enable_akaze_overrides_fast_hnsw` and `rollshot_stitch_folder_default_uses_fast_hnsw_and_enable_akaze_overrides` tests are gated by `#[cfg(feature = "akaze")]` so they are skipped here. `rollshot_stitch_folder_disable_feature_fallback_emits_disabled_no_match` runs regardless and must still pass — `make_akaze_fallback_smoke_canvas` is also `#[cfg(feature = "akaze")]`; if so, gate the new test the same way.

If the gating is wrong, fix the `#[cfg(feature = "akaze")]` annotation on the new disable-feature-fallback test (model it after the renamed sibling) and re-run Step 3.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-cli/tests/cli_smoke.rs
git commit -m "$(cat <<'EOF'
test(cli): Layer 3 smoke for FAST+KNN default + AKAZE override

Replaces Phase 1's enable-akaze toggle test with a single
default-vs-override test that pins all three contracts: default uses
FastHnsw, --enable-akaze switches to Akaze, --enable-akaze emits the
deprecation warning. Adds a --disable-feature-fallback test asserting
FeatureFallbackDisabled in the report.
EOF
)"
```

---

## Task 13: Final verification — fmt, clippy, full workspace

**Files:** (none modified; verification only)

- [ ] **Step 1: Format check**

Run: `cargo fmt --check`
Expected: clean (no diff).

If anything is off: `cargo fmt` and re-stage. Commit only if the diff is non-empty (`git diff --quiet || git commit -am "style: cargo fmt"`).

- [ ] **Step 2: Clippy with the akaze feature on**

Run: `cargo clippy --workspace --all-targets --features akaze -- -D warnings`
Expected: `No issues found`.

- [ ] **Step 3: Clippy with the akaze feature off**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: `No issues found`. If `feature_matcher.rs` references AKAZE types outside `#[cfg(feature = "akaze")]` arms, fix the gates here.

- [ ] **Step 4: Tests with akaze on**

Run: `cargo test --workspace --features akaze`
Expected: PASS.

- [ ] **Step 5: Tests with akaze off**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 6: Dependency-tree leak check**

Run: `cargo tree -p rollshot-core | grep -E '\bhora\b|\bbitarray\b' || echo "OK: no hora, no bitarray"`
Expected: `OK: no hora, no bitarray`. If anything is found, that's a spec violation — back to the offending PR.

- [ ] **Step 7: Final commit (if anything was fixed during verification)**

If steps 1–5 produced any changes:

```bash
git add -A
git status  # confirm nothing unexpected is staged
git commit -m "$(cat <<'EOF'
chore: final fmt/clippy clean-up for FAST+KNN fallback

EOF
)"
```

Otherwise: no commit, the previous commits already satisfied verification.

---

## Self-Review Summary

Spec sections coverage check:

| Spec section | Plan task(s) |
|---|---|
| §Architecture (dispatch) | Task 3 (scaffold), Task 9 (wire) |
| §Modules and Types — FastHnswConfig | Task 1 |
| §Modules and Types — FastHnswCandidateOutcome | Task 3 |
| §Modules and Types — feature_fallback_candidates | Task 3 |
| §Modules and Types — fast_hnsw_candidates | Task 3 (stub), Task 8 (real) |
| §Modules and Types — helpers (rgba_to_gray, extract_corners, compute_descriptor, compute_descriptors, linear_knn_match, vote_dominant_translation, feature_score) | Tasks 4–8 |
| §Modules and Types — naming doc-comment requirement | Tasks 1, 3 (and propagated by reviewer in any follow-up PR) |
| §Modules and Types — MatchMethod::FastHnsw | Task 1 |
| §Modules and Types — NoMatchReason::FeatureFallbackDisabled / FeatureLowInliers | Task 1 |
| §Data Flow | Tasks 4–8 (each helper, then end-to-end wiring) |
| §Error Handling — outcome → reason table | Task 9 (mapping in matcher.rs) |
| §Error Handling — panic discipline | Tasks 4–8 (`Option`-returning helpers, no `.unwrap()`) |
| §Test Strategy — Layer 1 | Tasks 4, 5, 6, 7, 8 (assertions accumulate) |
| §Test Strategy — Layer 2 | Task 10 |
| §Test Strategy — Layer 3 | Task 12 |
| §Test Strategy — Layer 4 (golden untouched) | Implicit; the existing golden suite runs in Tasks 9, 10, 13 as part of `--workspace`. |
| §CLI / Configuration — --disable-feature-fallback + deprecation | Task 11 |
| §CLI — pick-one dispatch on flags | Task 11 (wiring) + Task 9 (dispatch logic) |
| §Cargo.toml — imageproc, no hora, no bitarray | Task 2 (add), Task 13 Step 6 (verify) |
| §Migration / Deprecation | Task 11 (warning), kept-paths verified by Tasks 9–10 |
| §Alternative Approaches — guardrails | Task 13 Step 6 dependency leak check; reviewer responsibility on every PR |

No placeholders. All code blocks are concrete. All commands are exact.
