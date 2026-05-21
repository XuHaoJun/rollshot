# Rollshot LinearScroll v2 Plan 3: AKAZE + Fixtures + CI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the v0.2 AKAZE fallback path, deterministic golden fixtures, debug match artifacts, and CI coverage for the AKAZE-enabled build.

**Architecture:** Keep `AutoHybrid` as the only production strategy: cheap deterministic candidate generators run first, and AKAZE runs only when the verified cheap candidates fail. AKAZE lives behind the `akaze` Cargo feature, reports `MatchMethod::Akaze` through the existing motion model, and feeds the same pixel overlap verifier and ranker as every other candidate. Golden fixtures exercise the public `Stitcher` API and write failure artifacts under `target/test-artifacts`.

**Tech Stack:** Rust 2021, `image` 0.25, direct upstream rust-cv git `akaze` crate pinned to `d271a9ac6a9d7b39c6f22573d26d63b5ce94f3cb`, `bitarray` 0.9 with `space`, `space` 0.17, existing `serde`/`serde_json` for CLI reports and fixture motion JSON.

---

## Assumptions

- Plan 1 and Plan 2 have landed. The current tree already has `MotionCandidate`, `MotionEstimate`, `MatchMethod::Akaze`, `StitchConfig`, `PixelOverlapVerifier`, four-direction `LinearCanvas`, and non-AKAZE `AutoHybrid` generators in `crates/rollshot-core/src/matcher.rs`.
- The AKAZE dependency stays feature-gated as `akaze` for default-build cost control. This is the conservative path from the spec; making AKAZE a default dependency can be revisited after compile-time and runtime measurements.
- Use `https://github.com/rust-cv/cv.git` at exact rev `d271a9ac6a9d7b39c6f22573d26d63b5ce94f3cb`. The local `learn-projects/rust-cv` checkout is reference material only; do not add any Cargo local-path dependency pointing at it.
- Use direct `akaze`, `bitarray`, and `space` dependencies. Do not depend on the umbrella `cv` crate unless direct integration fails for a concrete compile-time reason.
- Debug controls are added to `rollshot stitch-folder`, not to the regular `capture` path. This preserves the spec's "no normal user-facing algorithm picker" constraint.

## File Structure

- Modify: `Cargo.toml`  
  Add workspace dependencies for pinned git `akaze`, `bitarray` with `space`, and `space`.
- Modify: `crates/rollshot-core/Cargo.toml`  
  Add optional AKAZE dependencies, the `akaze` feature, and test-only JSON dependencies for golden motion files.
- Modify: `crates/rollshot-cli/Cargo.toml`  
  Forward the `akaze` feature to `rollshot-core/akaze`.
- Modify: `crates/rollshot-core/src/types.rs`  
  Add `AkazeConfig` and `StitchConfig::akaze`.
- Modify: `crates/rollshot-core/src/lib.rs`  
  Add the AKAZE matcher module and export `AkazeConfig`.
- Create: `crates/rollshot-core/src/akaze_matcher.rs`  
  Own feature-gated AKAZE extraction, symmetric descriptor matching, translation voting, inlier filtering, and conversion into `MotionCandidate`.
- Modify: `crates/rollshot-core/src/matcher.rs`  
  Integrate AKAZE as the fallback after cheap verified candidates fail. Preserve ranker/verifier behavior.
- Modify: `crates/rollshot-core/src/stitcher.rs`  
  Preserve existing behavior while allowing matcher failure reasons such as `NotEnoughFeatures`.
- Modify: `crates/rollshot-core/tests/common/mod.rs`  
  Add deterministic fixture helpers for low-feature, image-card, repeated-grid, duplicate, and bad-frame cases.
- Create: `crates/rollshot-core/tests/golden_fixtures.rs`  
  Exercise golden fixture families, compare stitched output and motions, and write failure artifacts.
- Create: `crates/rollshot-core/tests/fixtures/linearscroll_v2/...`  
  Generated PNG and JSON fixture tree.
- Modify: `crates/rollshot-cli/src/args.rs`  
  Add debug-only `stitch-folder` switches: `--debug-match-report`, `--dump-overlap-debug`, `--disable-akaze`.
- Modify: `crates/rollshot-cli/src/cmd_stitch_folder.rs`  
  Emit JSON reports and overlap/diff artifacts while keeping normal output unchanged.
- Modify: `crates/rollshot-cli/tests/cli_smoke.rs`  
  Cover debug report generation.
- Modify: `.github/workflows/ci.yml`  
  Add AKAZE-enabled test coverage.
- Modify: `README.md`  
  Document local verification commands including the AKAZE feature test.

## Design Notes

- Score semantics stay unchanged: lower `MotionCandidate.score` is better, and `config.accept_confidence` remains the upper bound accepted by the ranker.
- AKAZE estimates translation only. For a matched keypoint pair, compute `dx = prev_x - curr_x` and `dy = prev_y - curr_y`.
- Descriptor matching uses the rust-cv tutorial shape, but with direct crates:

```rust
use bitarray::{BitArray, Hamming};
use space::{Knn, LinearKnn};
```

- `space::LinearKnn` is acceptable for v0.2 because `AkazeConfig::max_features` caps extraction at 1200 and AKAZE only runs after cheap matchers fail.
- The golden fixture generator writes binary PNG files. The plan provides the generator code and exact command; the generated files are then committed.

## Task 1: Wire the AKAZE Feature and Config

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/rollshot-core/Cargo.toml`
- Modify: `crates/rollshot-cli/Cargo.toml`
- Modify: `crates/rollshot-core/src/types.rs`
- Modify: `crates/rollshot-core/src/lib.rs`

- [ ] **Step 1: Add the failing config tests**

In `crates/rollshot-core/src/types.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
#[test]
fn akaze_defaults_follow_compile_feature() {
    let cfg = StitchConfig::default();

    #[cfg(feature = "akaze")]
    assert!(cfg.akaze.enabled);

    #[cfg(not(feature = "akaze"))]
    assert!(!cfg.akaze.enabled);

    assert_eq!(cfg.akaze.max_features, 1200);
    assert_eq!(cfg.akaze.detector_threshold, 0.001);
    assert_eq!(cfg.akaze.min_raw_matches, 24);
    assert_eq!(cfg.akaze.min_inliers, 16);
    assert_eq!(cfg.akaze.min_inlier_ratio, 0.35);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run:

```bash
rtk cargo test -p rollshot-core --lib types::tests::akaze_defaults_follow_compile_feature
```

Expected: FAIL with a compile error because `StitchConfig` has no `akaze` field yet.

- [ ] **Step 3: Add pinned workspace dependencies**

In the root `Cargo.toml`, append these entries to `[workspace.dependencies]`:

```toml
# Direct AKAZE extractor from upstream rust-cv. Keep this pinned; do not use a
# local path dependency from learn-projects/rust-cv.
akaze = { git = "https://github.com/rust-cv/cv.git", rev = "d271a9ac6a9d7b39c6f22573d26d63b5ce94f3cb" }
bitarray = { version = "0.9.0", features = ["space"] }
space = "0.17.0"
```

- [ ] **Step 4: Add the core feature and dev dependencies**

Replace `crates/rollshot-core/Cargo.toml` with:

```toml
[package]
name = "rollshot-core"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
image = { workspace = true }
akaze = { workspace = true, optional = true }
bitarray = { workspace = true, optional = true }
space = { workspace = true, optional = true }

[dev-dependencies]
serde = { workspace = true }
serde_json = { workspace = true }

[features]
default = []
akaze = ["dep:akaze", "dep:bitarray", "dep:space"]

[lints]
workspace = true
```

- [ ] **Step 5: Forward the CLI feature**

In `crates/rollshot-cli/Cargo.toml`, replace the `[features]` section with:

```toml
[features]
default = []
akaze = ["rollshot-core/akaze"]
macos-sck = ["rollshot-capture/macos-sck"]
```

- [ ] **Step 6: Add `AkazeConfig`, new `NoMatchReason` variants, and `#[non_exhaustive]` on public config structs**

In `crates/rollshot-core/src/types.rs`, add this struct before `StitchConfig`:

```rust
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct AkazeConfig {
    pub enabled: bool,
    pub max_features: usize,
    pub detector_threshold: f64,
    pub min_raw_matches: usize,
    pub min_inliers: usize,
    pub min_inlier_ratio: f32,
}

impl Default for AkazeConfig {
    fn default() -> Self {
        Self {
            enabled: cfg!(feature = "akaze"),
            max_features: 1200,
            detector_threshold: 0.001,
            min_raw_matches: 24,
            min_inliers: 16,
            min_inlier_ratio: 0.35,
        }
    }
}
```

Mark the existing `StitchConfig` and `VerifierConfig` declarations `#[non_exhaustive]` (add the attribute on the line immediately above each `pub struct`). All in-workspace constructions already use `StitchConfig::default()` + field mutation or `..StitchConfig::default()` spread, so this is non-breaking internally. External downstream crates must now spread `..Default::default()` or use the builder pattern; integration tests in `crates/rollshot-core/tests/` need to use the `..AkazeConfig::default()` spread when overriding only a subset of AkazeConfig fields (Task 4 and Task 5 fixtures already follow this convention below).

In the `StitchConfig` struct, add:

```rust
pub akaze: AkazeConfig,
```

In `impl Default for StitchConfig`, add:

```rust
akaze: AkazeConfig::default(),
```

Extend the existing `NoMatchReason` enum with two variants that surface AKAZE-specific failure modes. Append them at the end of the enum so existing match arms stay ordered:

```rust
    /// AKAZE fallback was attempted but disabled: either the `akaze` Cargo
    /// feature is not compiled, `AkazeConfig::enabled` is `false`, or
    /// `--disable-akaze` was passed.
    AkazeDisabled,
    /// AKAZE produced descriptors and matches but the inlier count fell
    /// below `min_inliers`, the inlier ratio fell below `min_inlier_ratio`,
    /// or every produced candidate was rejected by the pixel-overlap
    /// verifier. When verifier rejection is the cause, `best_estimate` on
    /// the resulting `StitchOutcome::NoMatch` carries the best AKAZE
    /// candidate for diagnostics.
    AkazeLowInliers,
```

The enum already derives `Debug, Clone, Copy, PartialEq, Eq`; no other change is required. Downstream `match` statements that exhaustively cover `NoMatchReason` will need to add arms for the two new variants — the CLI debug-report mapping in Task 6 already handles this via `format!("{reason:?}")`, which is exhaustive for free.

- [ ] **Step 7: Export `AkazeConfig`**

In `crates/rollshot-core/src/lib.rs`, update the public type export:

```rust
pub use types::{
    AkazeConfig, AppendDirection, MatchMethod, MatchStrategy, MotionCandidate, MotionEstimate,
    NoMatchReason, OverlapRegion, ScrollAxis, StitchConfig, StitchOutcome, StitchStats,
    VerifierConfig,
};
```

- [ ] **Step 8: Run baseline and AKAZE feature config tests**

Run:

```bash
rtk cargo test -p rollshot-core --lib types::tests::akaze_defaults_follow_compile_feature
rtk cargo test -p rollshot-core --features akaze --lib types::tests::akaze_defaults_follow_compile_feature
```

Expected: PASS. The first run asserts `enabled == false`; the feature run asserts `enabled == true`.

- [ ] **Step 9: Commit**

```bash
rtk git add Cargo.toml crates/rollshot-core/Cargo.toml crates/rollshot-cli/Cargo.toml crates/rollshot-core/src/types.rs crates/rollshot-core/src/lib.rs
rtk git commit -m "feat(core): add akaze feature config"
```

## Task 2: Add the Feature-Gated AKAZE Matcher Module

**Files:**
- Create: `crates/rollshot-core/src/akaze_matcher.rs`
- Modify: `crates/rollshot-core/src/lib.rs`

- [ ] **Step 1: Create failing AKAZE matcher tests**

Create `crates/rollshot-core/src/akaze_matcher.rs` with only the test module first:

```rust
#[cfg(all(test, feature = "akaze"))]
mod tests {
    use image::{imageops, Rgba, RgbaImage};

    use crate::akaze_matcher::{akaze_candidates, AkazeCandidateOutcome};
    use crate::types::{AkazeConfig, MatchMethod};

    fn feature_canvas(width: u32, height: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(width, height, Rgba([238, 238, 238, 255]));
        for i in 0..48u32 {
            let x = 24 + ((i * 37) % width.saturating_sub(48).max(1));
            let y = 24 + ((i * 53) % height.saturating_sub(48).max(1));
            let c = [
                (40 + (i * 17) % 180) as u8,
                (70 + (i * 29) % 170) as u8,
                (90 + (i * 31) % 150) as u8,
                255,
            ];
            for yy in y..(y + 9).min(height) {
                for xx in x..(x + 9).min(width) {
                    if xx == x || yy == y || xx + 1 == x + 9 || yy + 1 == y + 9 || xx == x + yy - y {
                        img.put_pixel(xx, yy, Rgba(c));
                    }
                }
            }
        }
        img
    }

    fn crop_xy(canvas: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
        imageops::crop_imm(canvas, x, y, w, h).to_image()
    }

    fn test_config() -> AkazeConfig {
        AkazeConfig {
            enabled: true,
            max_features: 800,
            detector_threshold: 0.0005,
            min_raw_matches: 8,
            min_inliers: 6,
            min_inlier_ratio: 0.25,
        }
    }

    #[test]
    fn akaze_candidates_estimate_translation() {
        let canvas = feature_canvas(420, 420);
        let prev = crop_xy(&canvas, 20, 30, 220, 220);
        let curr = crop_xy(&canvas, 58, 92, 220, 220);

        let outcome = akaze_candidates(&prev, &curr, &test_config());
        let candidates = match outcome {
            AkazeCandidateOutcome::Candidates(candidates) => candidates,
            other => panic!("expected AKAZE candidates, got {other:?}"),
        };

        let candidate = candidates.first().expect("one candidate");
        assert_eq!(candidate.method, MatchMethod::Akaze);
        assert!((candidate.dx - 38).abs() <= 3, "dx = {}", candidate.dx);
        assert!((candidate.dy - 62).abs() <= 3, "dy = {}", candidate.dy);
        assert!(candidate.raw_matches.unwrap_or(0) >= 8);
        assert!(candidate.inliers.unwrap_or(0) >= 6);
    }

    #[test]
    fn solid_frames_report_not_enough_features() {
        let prev = RgbaImage::from_pixel(220, 220, Rgba([250, 250, 250, 255]));
        let curr = RgbaImage::from_pixel(220, 220, Rgba([250, 250, 250, 255]));

        let outcome = akaze_candidates(&prev, &curr, &test_config());

        assert!(matches!(outcome, AkazeCandidateOutcome::NotEnoughFeatures { .. }));
    }
}
```

In `crates/rollshot-core/src/lib.rs`, add:

```rust
mod akaze_matcher;
```

- [ ] **Step 2: Run the AKAZE tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-core --features akaze --lib akaze_matcher::tests -- --nocapture
```

Expected: FAIL because `akaze_candidates` and `AkazeCandidateOutcome` are not defined.

- [ ] **Step 3: Prepend the feature-gated module implementation above the existing `mod tests` block**

In `crates/rollshot-core/src/akaze_matcher.rs` (created in Step 1 with the test module only), PREPEND the production code below so it sits above the `#[cfg(all(test, feature = "akaze"))] mod tests { ... }` block. Do NOT duplicate the test module — leave the existing one untouched.

The production code is:

```rust
use image::RgbaImage;

use crate::types::{AkazeConfig, MatchMethod, MotionCandidate};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AkazeCandidateOutcome {
    Disabled,
    NotEnoughFeatures { prev: usize, curr: usize },
    NotEnoughMatches { raw_matches: usize },
    Candidates(Vec<MotionCandidate>),
}

#[cfg(not(feature = "akaze"))]
pub(crate) fn akaze_candidates(
    _prev: &RgbaImage,
    _curr: &RgbaImage,
    _config: &AkazeConfig,
) -> AkazeCandidateOutcome {
    AkazeCandidateOutcome::Disabled
}

/// Score AKAZE-derived candidates so realistic outputs pass the default
/// `StitchConfig::accept_confidence` gate (0.15).
///
/// Acceptance budget at `min_inlier_ratio = 0.35` and worst-case residual of
/// `TRANSLATION_BUCKET_PX` pixels: `0.65 * 0.08 + 1.0 * 0.04 = 0.092`,
/// comfortably below the 0.15 gate. Median quality (`ratio = 0.6`,
/// `residual ≈ 1px`) scores around 0.042. AKAZE candidates never share a
/// ranker with cheap matchers (AKAZE is fallback-only), so this scale is
/// independent of the cheap-matcher NCC regime.
#[cfg(feature = "akaze")]
fn akaze_score(inlier_ratio: f32, residual_px: f32) -> f32 {
    let ratio_term = 1.0 - inlier_ratio.clamp(0.0, 1.0);
    let residual_term = (residual_px / 4.0).clamp(0.0, 1.0);
    (ratio_term * 0.08 + residual_term * 0.04).clamp(0.0, 1.0)
}

#[cfg(feature = "akaze")]
pub(crate) fn akaze_candidates(
    prev: &RgbaImage,
    curr: &RgbaImage,
    config: &AkazeConfig,
) -> AkazeCandidateOutcome {
    use akaze::{Akaze, KeyPoint};
    use bitarray::{BitArray, Hamming};
    use space::{Knn, LinearKnn};
    use std::collections::BTreeMap;

    const TRANSLATION_BUCKET_PX: f32 = 4.0;

    fn matching(a: &[BitArray<64>], b: &[BitArray<64>]) -> Vec<Option<usize>> {
        if b.len() < 2 {
            return vec![None; a.len()];
        }

        let knn_b = LinearKnn {
            metric: Hamming,
            iter: b.iter(),
        };

        (0..a.len())
            .map(|a_idx| {
                let neighbors = knn_b.knn(&a[a_idx], 2);
                if neighbors.len() < 2 {
                    return None;
                }
                if neighbors[0].distance + 24 < neighbors[1].distance {
                    Some(neighbors[0].index)
                } else {
                    None
                }
            })
            .collect()
    }

    fn symmetric_matching(a: &[BitArray<64>], b: &[BitArray<64>]) -> Vec<[usize; 2]> {
        let forward = matching(a, b);
        let reverse = matching(b, a);

        forward
            .into_iter()
            .enumerate()
            .filter_map(|(a_idx, b_idx)| {
                b_idx
                    .map(|b_idx| [a_idx, b_idx])
                    .filter(|&[a_idx, b_idx]| reverse[b_idx] == Some(a_idx))
            })
            .collect()
    }

    fn median(mut values: Vec<f32>) -> f32 {
        values.sort_by(f32::total_cmp);
        let mid = values.len() / 2;
        if values.len() % 2 == 0 {
            (values[mid - 1] + values[mid]) * 0.5
        } else {
            values[mid]
        }
    }

    fn median_residual(translations: &[(f32, f32)], dx: f32, dy: f32) -> f32 {
        let residuals = translations
            .iter()
            .map(|(tx, ty)| ((tx - dx).powi(2) + (ty - dy).powi(2)).sqrt())
            .collect();
        median(residuals)
    }

    fn dominant_translation(
        prev_keypoints: &[KeyPoint],
        curr_keypoints: &[KeyPoint],
        matches: &[[usize; 2]],
    ) -> Option<(i32, i32, usize, f32)> {
        let translations: Vec<(f32, f32)> = matches
            .iter()
            .map(|&[prev_idx, curr_idx]| {
                let (px, py) = prev_keypoints[prev_idx].point;
                let (cx, cy) = curr_keypoints[curr_idx].point;
                (px - cx, py - cy)
            })
            .collect();

        let mut buckets: BTreeMap<(i32, i32), Vec<(f32, f32)>> = BTreeMap::new();
        for &(dx, dy) in &translations {
            let key = (
                (dx / TRANSLATION_BUCKET_PX).round() as i32,
                (dy / TRANSLATION_BUCKET_PX).round() as i32,
            );
            buckets.entry(key).or_default().push((dx, dy));
        }

        let bucket = buckets.into_values().max_by_key(Vec::len)?;
        let dx = median(bucket.iter().map(|(dx, _)| *dx).collect());
        let dy = median(bucket.iter().map(|(_, dy)| *dy).collect());
        let inliers: Vec<(f32, f32)> = translations
            .into_iter()
            .filter(|(tx, ty)| {
                let residual = ((tx - dx).powi(2) + (ty - dy).powi(2)).sqrt();
                residual <= TRANSLATION_BUCKET_PX
            })
            .collect();
        let residual = median_residual(&inliers, dx, dy);

        Some((dx.round() as i32, dy.round() as i32, inliers.len(), residual))
    }

    if !config.enabled {
        return AkazeCandidateOutcome::Disabled;
    }

    let mut extractor = Akaze::new(config.detector_threshold);
    extractor.maximum_features = config.max_features;

    // Pre-convert to single-channel Luma8 so AKAZE's internal grayscale pass
    // is a no-op. Halves transient memory vs cloning an `RgbaImage` (upstream
    // verified at rust-cv rev d271a9ac: see akaze/src/image.rs `from_dynamic`
    // ImageLuma8 arm). Mathematically equivalent up to one rounding step.
    let prev_image = image::DynamicImage::ImageLuma8(image::imageops::grayscale(prev));
    let curr_image = image::DynamicImage::ImageLuma8(image::imageops::grayscale(curr));
    let (prev_keypoints, prev_descriptors) = extractor.extract(&prev_image);
    let (curr_keypoints, curr_descriptors) = extractor.extract(&curr_image);

    if prev_descriptors.len() < 2 || curr_descriptors.len() < 2 {
        return AkazeCandidateOutcome::NotEnoughFeatures {
            prev: prev_descriptors.len(),
            curr: curr_descriptors.len(),
        };
    }

    let matches = symmetric_matching(&prev_descriptors, &curr_descriptors);
    let raw_matches = matches.len();
    if raw_matches < config.min_raw_matches {
        return AkazeCandidateOutcome::NotEnoughMatches { raw_matches };
    }

    let Some((dx, dy, inliers, residual_px)) =
        dominant_translation(&prev_keypoints, &curr_keypoints, &matches)
    else {
        return AkazeCandidateOutcome::NotEnoughMatches { raw_matches };
    };

    let inlier_ratio = inliers as f32 / raw_matches as f32;
    if inliers < config.min_inliers || inlier_ratio < config.min_inlier_ratio {
        return AkazeCandidateOutcome::NotEnoughMatches { raw_matches };
    }

    AkazeCandidateOutcome::Candidates(vec![MotionCandidate {
        dx,
        dy,
        method: MatchMethod::Akaze,
        score: akaze_score(inlier_ratio, residual_px),
        second_best_score: None,
        inliers: Some(inliers),
        raw_matches: Some(raw_matches),
    }])
}
```

The `mod tests` block from Step 1 remains unchanged below this prepended code.

- [ ] **Step 4: Run AKAZE matcher tests**

Run:

```bash
rtk cargo test -p rollshot-core --features akaze --lib akaze_matcher::tests -- --nocapture
```

Expected: PASS. If the translation test has too few features, lower only the test's `detector_threshold` to `0.0001` and keep production default unchanged.

- [ ] **Step 5: Add an `akaze_score` unit test**

Append this feature-gated test inside the existing `#[cfg(all(test, feature = "akaze"))] mod tests` block in `crates/rollshot-core/src/akaze_matcher.rs`:

```rust
    #[test]
    fn akaze_score_passes_default_accept_confidence() {
        use crate::types::StitchConfig;

        let accept = StitchConfig::default().accept_confidence;

        // Floor case: minimum allowed inlier ratio + worst-case residual.
        // Must still clear the gate, otherwise AKAZE never accepts in production.
        let floor = super::akaze_score(0.35, 4.0);
        assert!(
            floor <= accept,
            "floor {floor} exceeds accept_confidence {accept}"
        );

        // Median quality: realistic AKAZE outcome should score well below the gate.
        let mid = super::akaze_score(0.6, 1.0);
        assert!(mid < floor, "mid {mid} >= floor {floor}");

        // Top quality: near-perfect AKAZE result scores near zero.
        let top = super::akaze_score(0.9, 0.0);
        assert!(top < mid, "top {top} >= mid {mid}");
        assert!(top <= 0.01, "top {top} > 0.01");
    }
```

Run:

```bash
rtk cargo test -p rollshot-core --features akaze --lib akaze_matcher::tests::akaze_score_passes_default_accept_confidence
```

Expected: PASS. The floor case asserts AKAZE's worst-acceptable inputs still clear `accept_confidence`; if this test fails after a future weight tweak, AKAZE will silently stop accepting candidates in production.

- [ ] **Step 6: Run baseline compile without the feature**

Run:

```bash
rtk cargo test -p rollshot-core --lib types::tests::default_config_picks_auto_hybrid
```

Expected: PASS and no compile attempt for the git `akaze` crate in the non-feature build.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-core/src/lib.rs crates/rollshot-core/src/akaze_matcher.rs Cargo.lock
rtk git commit -m "feat(core): add akaze translation matcher"
```

## Task 3: Integrate AKAZE into AutoHybrid Fallback

**Files:**
- Modify: `crates/rollshot-core/src/matcher.rs`
- Modify: `crates/rollshot-core/src/stitcher.rs`

- [ ] **Step 1: Add failing fallback tests**

In `crates/rollshot-core/src/matcher.rs`, inside `#[cfg(test)] mod tests`, extend the imports:

```rust
use crate::types::{AkazeConfig, MatchMethod, NoMatchReason, ScrollAxis, StitchConfig};
```

Add helpers in the same test module:

```rust
fn make_sparse_feature_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = make_repeated_grid(width, height);
    for i in 0..64u32 {
        let x = 18 + ((i * 41) % width.saturating_sub(36).max(1));
        let y = 18 + ((i * 67) % height.saturating_sub(36).max(1));
        for yy in y..(y + 7).min(height) {
            for xx in x..(x + 7).min(width) {
                if xx == x || yy == y || xx == x + yy - y {
                    img.put_pixel(xx, yy, Rgba([15, 15, 15, 255]));
                }
            }
        }
    }
    img
}

fn fallback_config() -> StitchConfig {
    StitchConfig {
        second_best_margin: 0.25,
        akaze: AkazeConfig {
            enabled: true,
            max_features: 1200,
            detector_threshold: 0.0005,
            min_raw_matches: 8,
            min_inliers: 6,
            min_inlier_ratio: 0.25,
        },
        ..StitchConfig::default()
    }
}
```

Add the feature-gated tests:

```rust
#[cfg(feature = "akaze")]
#[test]
fn akaze_fallback_recovers_repeated_grid_with_sparse_features() {
    let canvas = make_sparse_feature_canvas(360, 760);
    let prev = crop_xy(&canvas, 0, 0, 240, 240);
    let curr = crop_xy(&canvas, 0, 72, 240, 240);

    let outcome = estimate_motion(&prev, &curr, None, (0, 0), &fallback_config());
    let candidate = match outcome {
        MotionSearchOutcome::Candidate(candidate) => candidate,
        other => panic!("expected AKAZE candidate, got {other:?}"),
    };

    assert_eq!(candidate.method, MatchMethod::Akaze);
    assert_eq!(candidate.dx, 0);
    assert!((candidate.dy - 72).abs() <= 3, "dy = {}", candidate.dy);
    assert!(candidate.inliers.unwrap_or(0) >= 6);
}

#[cfg(feature = "akaze")]
#[test]
fn akaze_attempt_with_blank_frames_reports_not_enough_features() {
    let prev = RgbaImage::from_pixel(220, 220, Rgba([250, 250, 250, 255]));
    let curr = RgbaImage::from_pixel(220, 220, Rgba([250, 250, 250, 255]));

    let outcome = estimate_motion(&prev, &curr, None, (0, 0), &fallback_config());

    assert!(matches!(
        outcome,
        MotionSearchOutcome::NoMatch {
            reason: NoMatchReason::NotEnoughFeatures,
            best_candidate: None,
        }
    ));
}

#[cfg(feature = "akaze")]
#[test]
fn akaze_candidate_rejected_by_verifier_preserves_best_estimate() {
    // Build a frame pair where AKAZE keypoints in the un-corrupted top half
    // produce a plausible translation candidate, but the bottom half of
    // `curr` is overwritten with high-contrast noise so the pixel-overlap
    // verifier's MAD threshold rejects every AKAZE proposal. Cheap matchers
    // also fail because >40% of the overlap is noise.
    let canvas = make_sparse_feature_canvas(360, 760);
    let prev = crop_xy(&canvas, 0, 0, 240, 240);
    let mut curr = crop_xy(&canvas, 0, 72, 240, 240);
    for y in 120..240 {
        for x in 0..240 {
            let v = ((x * 41 + y * 67) % 255) as u8;
            curr.put_pixel(x, y, Rgba([v, 255 - v, v / 2, 255]));
        }
    }

    let outcome = estimate_motion(&prev, &curr, None, (0, 0), &fallback_config());

    let best = match outcome {
        MotionSearchOutcome::NoMatch {
            reason: NoMatchReason::AkazeLowInliers,
            best_candidate: Some(candidate),
        } => candidate,
        other => panic!("expected AkazeLowInliers with best_candidate, got {other:?}"),
    };
    assert_eq!(best.method, MatchMethod::Akaze);
    assert!((best.dy - 72).abs() <= 8, "best dy = {}", best.dy);
}
```

- [ ] **Step 2: Run the fallback tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-core --features akaze --lib matcher::akaze_ -- --nocapture
```

Expected: FAIL because `estimate_motion` still returns `Option<MotionCandidate>` and never calls AKAZE.

- [ ] **Step 3: Add `MotionSearchOutcome` and AKAZE fallback to `matcher.rs`**

Update the imports at the top of `crates/rollshot-core/src/matcher.rs`:

```rust
use crate::akaze_matcher::{akaze_candidates, AkazeCandidateOutcome};
use crate::types::{MatchMethod, MotionCandidate, NoMatchReason, ScrollAxis, StitchConfig};
```

Add this enum after `SearchAxis`. `NoMatch` carries an `Option<MotionCandidate>` so the stitcher can surface diagnostic info on `best_estimate` when AKAZE produced a candidate that the verifier later rejected:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MotionSearchOutcome {
    Candidate(MotionCandidate),
    NoMatch {
        reason: NoMatchReason,
        best_candidate: Option<MotionCandidate>,
    },
}
```

Change the `estimate_motion` signature and body to:

```rust
pub(crate) fn estimate_motion(
    prev: &RgbaImage,
    curr: &RgbaImage,
    locked_axis: Option<ScrollAxis>,
    last_motion: (i32, i32),
    config: &StitchConfig,
) -> MotionSearchOutcome {
    if prev.dimensions() != curr.dimensions() {
        return MotionSearchOutcome::NoMatch {
            reason: NoMatchReason::DimensionMismatch,
            best_candidate: None,
        };
    }

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

    if let Some(candidate) = rank_verified_candidates(prev, curr, locked_axis, candidates, config) {
        return MotionSearchOutcome::Candidate(candidate);
    }

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
            // Preserve the best AKAZE proposal even when the verifier rejects
            // every candidate, so debug-match reports can show the near-miss.
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

In the matcher test module, import `MotionSearchOutcome`:

```rust
use super::{
    coarse_sample_dimensions, content_roi, estimate_motion, MotionSearchOutcome,
    COARSE_DOWNSAMPLE_STEP,
};
```

Add a helper:

```rust
fn unwrap_candidate(outcome: MotionSearchOutcome) -> MotionCandidate {
    match outcome {
        MotionSearchOutcome::Candidate(candidate) => candidate,
        other => panic!("expected candidate, got {other:?}"),
    }
}
```

Migrate every existing test in `matcher::tests` that calls `estimate_motion(...)`. Two mechanical patterns cover all 10:

For tests that previously used `.expect("...")`, replace with `unwrap_candidate(...)`:

```rust
let candidate = unwrap_candidate(estimate_motion(&prev, &curr, None, (0, 0), &StitchConfig::default()));
```

For tests that previously used `.is_none()`, replace with a `matches!` check:

```rust
assert!(matches!(
    estimate_motion(&prev, &curr, None, (0, 0), &StitchConfig::default()),
    MotionSearchOutcome::NoMatch { .. }
));
```

The full list of tests to migrate (all in `crates/rollshot-core/src/matcher.rs` mod tests):

| Test | Previous shape | New shape |
| --- | --- | --- |
| `estimate_motion_respects_min_overlap` | `.expect("template candidate")` | `unwrap_candidate(...)` |
| `estimate_motion_finds_known_scroll` | `.expect("template candidate")` | `unwrap_candidate(...)` |
| `estimate_motion_returns_none_for_unrelated_frames` | `.is_none()` | `matches!(..., MotionSearchOutcome::NoMatch { .. })` |
| `estimate_motion_returns_none_for_dimension_mismatch` | `.is_none()` | `matches!(..., MotionSearchOutcome::NoMatch { .. })` |
| `estimate_motion_finds_vertical_up_scroll` | `.expect("template candidate")` | `unwrap_candidate(...)` |
| `estimate_motion_finds_horizontal_right_scroll` | `.expect("horizontal candidate")` | `unwrap_candidate(...)` |
| `estimate_motion_finds_horizontal_left_scroll` | `.expect("horizontal candidate")` | `unwrap_candidate(...)` |
| `locked_vertical_hint_rejects_unrelated_frame` | `.is_none()` | `matches!(..., MotionSearchOutcome::NoMatch { .. })` |
| `repeated_grid_is_rejected_by_second_best_margin` | `.is_none()` | `matches!(..., MotionSearchOutcome::NoMatch { .. })` |
| `locked_vertical_still_returns_reliable_axis_change_candidate` | `.expect("axis-change candidate")` | `unwrap_candidate(...)` |

Run `rtk cargo build -p rollshot-core --tests` after migrating to confirm zero compile errors before moving to Step 4. Any remaining `.expect(`/`.is_none()` calls on `estimate_motion` will show up as type errors.

Also update the two new feature-gated tests from Step 1 (`akaze_fallback_recovers_repeated_grid_with_sparse_features` and `akaze_attempt_with_blank_frames_reports_not_enough_features`): change the `NoMatchReason` assertion in the second test to use the new struct variant:

```rust
assert!(matches!(
    outcome,
    MotionSearchOutcome::NoMatch { reason: NoMatchReason::NotEnoughFeatures, .. }
));
```

- [ ] **Step 4: Update `stitcher.rs` for matcher reasons**

Change the matcher import:

```rust
use crate::matcher::{estimate_motion, MotionSearchOutcome};
```

Replace the `let candidate = match estimate_motion(...)` block with the form below. When the matcher returns `NoMatch` with a `best_candidate` (today: only AKAZE-fail-verifier path), convert it to a `MotionEstimate` via the existing `build_estimate` helper so debug-match reports can show the near-miss:

```rust
let candidate = match estimate_motion(
    anchor,
    &frame,
    self.locked_axis,
    self.last_motion,
    &self.config,
) {
    MotionSearchOutcome::Candidate(candidate) => candidate,
    MotionSearchOutcome::NoMatch {
        reason,
        best_candidate,
    } => {
        let best_estimate = best_candidate.and_then(|candidate| {
            build_estimate(anchor, &frame, &candidate, self.config.axis_ratio_threshold)
        });
        return StitchOutcome::NoMatch {
            reason,
            best_estimate,
        };
    }
};
```

- [ ] **Step 5: Run matcher and stitcher tests**

Run:

```bash
rtk cargo test -p rollshot-core --lib matcher:: -- --nocapture
rtk cargo test -p rollshot-core --test stitcher -- --nocapture
rtk cargo test -p rollshot-core --features akaze --lib matcher::akaze_ -- --nocapture
```

Expected: PASS. The feature-gated fallback test must return `MatchMethod::Akaze`.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-core/src/matcher.rs crates/rollshot-core/src/stitcher.rs
rtk git commit -m "feat(core): use akaze as autohybrid fallback"
```

## Task 4: Add Stitcher-Level AKAZE Coverage

**Files:**
- Modify: `crates/rollshot-core/tests/common/mod.rs`
- Modify: `crates/rollshot-core/tests/stitcher.rs`

- [ ] **Step 1: Add deterministic AKAZE fixture helpers**

In `crates/rollshot-core/tests/common/mod.rs`, add:

```rust
/// Builds mostly repeated content with sparse unique corners. Template and edge
/// projections see many plausible offsets, while AKAZE can vote on the sparse
/// corners.
pub fn make_akaze_fallback_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([246, 246, 246, 255]));
    for y in 0..height {
        for x in 0..width {
            let v = if (x / 18 + y / 18) % 2 == 0 { 232 } else { 214 };
            img.put_pixel(x, y, Rgba([v, v, v, 255]));
        }
    }

    for i in 0..80u32 {
        let x = 20 + ((i * 43) % width.saturating_sub(40).max(1));
        let y = 20 + ((i * 61) % height.saturating_sub(40).max(1));
        let color = Rgba([
            (20 + (i * 19) % 180) as u8,
            (30 + (i * 23) % 160) as u8,
            (40 + (i * 29) % 150) as u8,
            255,
        ]);
        for yy in y..(y + 9).min(height) {
            for xx in x..(x + 9).min(width) {
                if xx == x || yy == y || xx + 1 == x + 9 || yy + 1 == y + 9 || xx == x + yy - y {
                    img.put_pixel(xx, yy, color);
                }
            }
        }
    }

    img
}
```

- [ ] **Step 2: Add failing stitcher fallback test**

In `crates/rollshot-core/tests/stitcher.rs`, extend imports:

```rust
use common::{
    crop_frame, crop_frame_xy, make_akaze_fallback_canvas, make_repeated_rows,
    make_scroll_canvas, make_wide_canvas, paint_sticky_header,
};
use rollshot_core::{
    AkazeConfig, AppendDirection, MatchMethod, NoMatchReason, ScrollAxis, StitchConfig,
    StitchOutcome, Stitcher, VerifierConfig,
};
```

Add:

```rust
#[cfg(feature = "akaze")]
#[test]
fn akaze_fallback_appends_when_template_is_ambiguous() {
    let canvas = make_akaze_fallback_canvas(320, 900);
    let first = crop_frame(&canvas, 0, 320);
    let scrolled = crop_frame(&canvas, 96, 320);

    // AkazeConfig is `#[non_exhaustive]`, so this integration-test crate must
    // spread `..AkazeConfig::default()` instead of listing every field.
    let config = StitchConfig {
        second_best_margin: 0.25,
        akaze: AkazeConfig {
            enabled: true,
            detector_threshold: 0.0005,
            min_raw_matches: 8,
            min_inliers: 6,
            min_inlier_ratio: 0.25,
            ..AkazeConfig::default()
        },
        ..StitchConfig::default()
    };
    let mut stitcher = Stitcher::new(config);

    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);
    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended {
            direction,
            added,
            estimate,
        } => {
            assert_eq!(direction, AppendDirection::Bottom);
            assert_eq!(estimate.method, MatchMethod::Akaze);
            assert!((92..=100).contains(&added), "added = {added}");
            assert!(estimate.inliers.unwrap_or(0) >= 6);
            assert!(estimate.raw_matches.unwrap_or(0) >= 8);
        }
        other => panic!("expected AKAZE append, got {other:?}"),
    }
}
```

- [ ] **Step 3: Run the stitcher fallback test**

Run:

```bash
rtk cargo test -p rollshot-core --features akaze --test stitcher akaze_fallback_appends_when_template_is_ambiguous -- --nocapture
```

Expected: PASS and the estimate method is `Akaze`.

- [ ] **Step 4: Run baseline stitcher tests without AKAZE**

Run:

```bash
rtk cargo test -p rollshot-core --test stitcher -- --nocapture
```

Expected: PASS. The `#[cfg(feature = "akaze")]` test is not built.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-core/tests/common/mod.rs crates/rollshot-core/tests/stitcher.rs
rtk git commit -m "test(core): cover akaze stitcher fallback"
```

## Task 5: Add Golden Fixture Generator and Runner

**Files:**
- Create: `crates/rollshot-core/tests/golden_fixtures.rs`
- Create: `crates/rollshot-core/tests/fixtures/linearscroll_v2/...`

- [ ] **Step 1: Add the golden fixture runner**

Create `crates/rollshot-core/tests/golden_fixtures.rs`:

```rust
use std::fs;
use std::path::{Path, PathBuf};

use image::{imageops, Rgba, RgbaImage};
use rollshot_core::{
    AkazeConfig, AppendDirection, MatchMethod, StitchConfig, StitchOutcome, Stitcher,
};
use serde::Deserialize;

const FIXTURE_ROOT: &str = "tests/fixtures/linearscroll_v2";

// Golden fixtures compare stitched PNGs across architectures (Linux x86 +
// macOS ARM in CI). f32 NCC summation order can shift the chosen offset by
// 1 px on some fixtures, which propagates as a full-image shift. We allow a
// small per-pixel channel diff and a budget of mismatched pixels so the
// goldens survive cross-arch f32 drift but still catch real regressions.
// `motions.json` is still compared exactly.
const MAX_PIXEL_CHANNEL_DIFF: u8 = 4;
const MAX_MISMATCHED_PIXEL_RATIO: f32 = 0.005;

#[derive(Debug, Deserialize)]
struct ExpectedMotion {
    frame: usize,
    dx: i32,
    dy: i32,
    direction: String,
}

#[derive(Debug)]
struct ObservedMotion {
    frame: usize,
    dx: i32,
    dy: i32,
    direction: AppendDirection,
    method: MatchMethod,
}

#[test]
fn golden_fixtures_match_expected_outputs() {
    for family in [
        "linear_vertical_down",
        "linear_vertical_up",
        "linear_horizontal_right",
        "linear_horizontal_left",
        "sticky_header",
        "repeated_rows",
        "repeated_grid",
        "low_feature_text",
        "image_cards",
        "bad_frame",
        "duplicate_frames",
    ] {
        run_fixture(family, StitchConfig::default());
    }
}

#[cfg(feature = "akaze")]
#[test]
fn akaze_golden_fixture_uses_akaze_fallback() {
    // AkazeConfig is `#[non_exhaustive]`; spread the default to fill any
    // future fields without breaking this integration-test crate.
    let observed = run_fixture(
        "akaze_fallback",
        StitchConfig {
            second_best_margin: 0.25,
            akaze: AkazeConfig {
                enabled: true,
                detector_threshold: 0.0005,
                min_raw_matches: 8,
                min_inliers: 6,
                min_inlier_ratio: 0.25,
                ..AkazeConfig::default()
            },
            ..StitchConfig::default()
        },
    );
    assert!(
        observed.iter().any(|motion| motion.method == MatchMethod::Akaze),
        "akaze_fallback should contain at least one AKAZE motion, got {observed:?}"
    );
}

fn run_fixture(family: &str, config: StitchConfig) -> Vec<ObservedMotion> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_ROOT).join(family);
    let frames = load_frames(&root.join("frames"));
    let expected_output = image::open(root.join("expected/output.png"))
        .expect("decode expected output")
        .to_rgba8();
    let expected_motions = load_expected_motions(&root.join("expected/motions.json"));

    let mut stitcher = Stitcher::new(config);
    let mut observed = Vec::new();

    for (idx, frame) in frames.into_iter().enumerate() {
        match stitcher.push_frame(frame) {
            StitchOutcome::FirstFrame => {}
            StitchOutcome::Appended {
                direction,
                estimate,
                ..
            } => {
                observed.push(ObservedMotion {
                    frame: idx,
                    dx: estimate.dx,
                    dy: estimate.dy,
                    direction,
                    method: estimate.method,
                });
            }
            StitchOutcome::Duplicate | StitchOutcome::NoProgress { .. } => {}
            StitchOutcome::NoMatch { .. } | StitchOutcome::AxisChanged { .. } => {}
        }
    }

    let actual = stitcher.full_image().expect("stitched output");
    let image_ok = images_within_tolerance(actual, &expected_output);
    let motions_ok = motions_match(&observed, &expected_motions);
    if !image_ok || !motions_ok {
        write_failure_artifacts(family, actual, &expected_output, &observed, &expected_motions);
    }

    assert!(image_ok, "{family} output mismatch beyond tolerance");
    assert!(
        motions_ok,
        "{family} motions mismatch: observed={observed:?}, expected={expected_motions:?}"
    );

    observed
}

/// Per-pixel tolerance compare. Returns true when:
/// - the two images have identical dimensions, AND
/// - every pixel's max channel diff is `<= MAX_PIXEL_CHANNEL_DIFF`, AND
/// - the fraction of pixels with any non-zero channel diff is
///   `<= MAX_MISMATCHED_PIXEL_RATIO`.
fn images_within_tolerance(actual: &RgbaImage, expected: &RgbaImage) -> bool {
    if actual.dimensions() != expected.dimensions() {
        return false;
    }
    let total = (actual.width() as u64) * (actual.height() as u64);
    let mut mismatched = 0u64;
    for (a, e) in actual.pixels().zip(expected.pixels()) {
        let dr = a[0].abs_diff(e[0]);
        let dg = a[1].abs_diff(e[1]);
        let db = a[2].abs_diff(e[2]);
        let da = a[3].abs_diff(e[3]);
        let max_chan = dr.max(dg).max(db).max(da);
        if max_chan > MAX_PIXEL_CHANNEL_DIFF {
            return false;
        }
        if max_chan > 0 {
            mismatched += 1;
        }
    }
    let ratio = mismatched as f32 / total.max(1) as f32;
    ratio <= MAX_MISMATCHED_PIXEL_RATIO
}

fn load_frames(dir: &Path) -> Vec<RgbaImage> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .expect("read frames dir")
        .map(|entry| entry.expect("read frame entry").path())
        .filter(|path| path.extension().and_then(|s| s.to_str()) == Some("png"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| image::open(path).expect("decode frame").to_rgba8())
        .collect()
}

fn load_expected_motions(path: &Path) -> Vec<ExpectedMotion> {
    let text = fs::read_to_string(path).expect("read motions json");
    serde_json::from_str(&text).expect("parse motions json")
}

fn motions_match(observed: &[ObservedMotion], expected: &[ExpectedMotion]) -> bool {
    observed.len() == expected.len()
        && observed.iter().zip(expected).all(|(observed, expected)| {
            observed.frame == expected.frame
                && observed.dx == expected.dx
                && observed.dy == expected.dy
                && format!("{:?}", observed.direction) == expected.direction
        })
}

fn write_failure_artifacts(
    family: &str,
    actual: &RgbaImage,
    expected: &RgbaImage,
    observed: &[ObservedMotion],
    expected_motions: &[ExpectedMotion],
) {
    let out = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-artifacts")
        .join(family);
    fs::create_dir_all(&out).expect("create artifact dir");

    actual.save(out.join("actual.png")).expect("save actual");
    expected.save(out.join("expected.png")).expect("save expected");
    diff_image(expected, actual)
        .save(out.join("diff.png"))
        .expect("save diff");
    side_by_side(expected, actual)
        .save(out.join("matches.png"))
        .expect("save side-by-side");

    let observed_json = observed
        .iter()
        .map(|motion| {
            format!(
                "    {{ \"frame\": {}, \"dx\": {}, \"dy\": {}, \"direction\": \"{:?}\", \"method\": \"{:?}\" }}",
                motion.frame, motion.dx, motion.dy, motion.direction, motion.method
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    let expected_json = expected_motions
        .iter()
        .map(|motion| {
            format!(
                "    {{ \"frame\": {}, \"dx\": {}, \"dy\": {}, \"direction\": \"{}\" }}",
                motion.frame, motion.dx, motion.dy, motion.direction
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    let report = format!(
        "{{\n  \"family\": \"{}\",\n  \"observed\": [\n{}\n  ],\n  \"expected\": [\n{}\n  ]\n}}\n",
        family, observed_json, expected_json
    );
    fs::write(out.join("report.json"), report).expect("write report");
}

fn diff_image(expected: &RgbaImage, actual: &RgbaImage) -> RgbaImage {
    let width = expected.width().max(actual.width());
    let height = expected.height().max(actual.height());
    let mut out = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255]));
    for y in 0..height {
        for x in 0..width {
            let e = if x < expected.width() && y < expected.height() {
                *expected.get_pixel(x, y)
            } else {
                Rgba([0, 0, 0, 255])
            };
            let a = if x < actual.width() && y < actual.height() {
                *actual.get_pixel(x, y)
            } else {
                Rgba([0, 0, 0, 255])
            };
            out.put_pixel(
                x,
                y,
                Rgba([
                    e[0].abs_diff(a[0]),
                    e[1].abs_diff(a[1]),
                    e[2].abs_diff(a[2]),
                    255,
                ]),
            );
        }
    }
    out
}

fn side_by_side(left: &RgbaImage, right: &RgbaImage) -> RgbaImage {
    let width = left.width() + right.width();
    let height = left.height().max(right.height());
    let mut out = RgbaImage::from_pixel(width, height, Rgba([20, 20, 20, 255]));
    imageops::replace(&mut out, left, 0, 0);
    imageops::replace(&mut out, right, left.width() as i64, 0);
    out
}
```

- [ ] **Step 2: Run the runner to verify fixtures are missing**

Run:

```bash
rtk cargo test -p rollshot-core --test golden_fixtures golden_fixtures_match_expected_outputs
```

Expected: FAIL because `tests/fixtures/linearscroll_v2/...` does not exist yet.

- [ ] **Step 3: Add the fixture generator to the same test file**

Append this ignored generator to `crates/rollshot-core/tests/golden_fixtures.rs`:

```rust
#[ignore]
#[test]
fn refresh_linearscroll_v2_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_ROOT);
    fs::create_dir_all(&root).expect("create fixture root");

    // Vertical / horizontal / sticky_header use the rich procedural texture
    // (`make_fixture_canvas`) so cheap matchers reliably succeed.
    write_vertical_fixture(
        &root,
        "linear_vertical_down",
        FixtureCanvas::Generic,
        [0, 180, 356],
        0,
        320,
        false,
    );
    write_vertical_fixture(
        &root,
        "linear_vertical_up",
        FixtureCanvas::Generic,
        [356, 176, 0],
        0,
        320,
        false,
    );
    write_horizontal_fixture(&root, "linear_horizontal_right", [0, 180, 356], 0, 320);
    write_horizontal_fixture(&root, "linear_horizontal_left", [356, 176, 0], 0, 320);
    write_vertical_fixture(
        &root,
        "sticky_header",
        FixtureCanvas::Generic,
        [0, 160, 318],
        0,
        320,
        true,
    );

    // Distinct canvases per fixture family so the test name matches behavior.
    write_vertical_fixture(
        &root,
        "low_feature_text",
        FixtureCanvas::LowFeatureText,
        [0, 150, 300],
        0,
        320,
        false,
    );
    write_vertical_fixture(
        &root,
        "image_cards",
        FixtureCanvas::ImageCards,
        [0, 170, 340],
        0,
        320,
        false,
    );

    // Genuine ambiguity, not blank canvases: matcher should refuse to append.
    write_rejected_fixture(&root, "repeated_rows", FixtureCanvas::RepeatedRows);
    write_rejected_fixture(&root, "repeated_grid", FixtureCanvas::RepeatedGrid);

    write_bad_frame_fixture(&root);
    write_duplicate_fixture(&root);
    write_akaze_fixture(&root);
}

/// Selects the canvas builder used by the fixture writers. Different fixture
/// families need genuinely different content so each test name matches a
/// distinct failure-mode behavior, not the same texture under many names.
#[derive(Debug, Clone, Copy)]
enum FixtureCanvas {
    /// Rich procedural texture; cheap matchers succeed easily.
    Generic,
    /// Mostly white with sparse word-shaped dark blocks; tests the
    /// "low-feature-text page" case.
    LowFeatureText,
    /// Large flat color cards with thin borders; tests the
    /// "image-grid / image-card" case.
    ImageCards,
    /// Tightly repeated horizontal stripe pattern; ambiguity test for
    /// row-major repetition.
    RepeatedRows,
    /// Tightly repeated 2D grid pattern; ambiguity test for grid repetition.
    RepeatedGrid,
}

fn build_canvas(kind: FixtureCanvas, width: u32, height: u32) -> RgbaImage {
    match kind {
        FixtureCanvas::Generic => make_fixture_canvas(width, height),
        FixtureCanvas::LowFeatureText => make_low_feature_text_canvas(width, height),
        FixtureCanvas::ImageCards => make_image_card_canvas(width, height),
        FixtureCanvas::RepeatedRows => make_repeated_rows_canvas(width, height),
        FixtureCanvas::RepeatedGrid => make_repeated_grid_canvas(width, height),
    }
}

fn write_vertical_fixture(
    root: &Path,
    name: &str,
    canvas_kind: FixtureCanvas,
    offsets: [u32; 3],
    x: u32,
    viewport: u32,
    sticky: bool,
) {
    let canvas = build_canvas(canvas_kind, 480, 1100);
    let dir = root.join(name);
    let frames_dir = dir.join("frames");
    let expected_dir = dir.join("expected");
    recreate_dir(&frames_dir);
    recreate_dir(&expected_dir);

    for (idx, y) in offsets.iter().enumerate() {
        let mut frame = imageops::crop_imm(&canvas, x, *y, viewport, viewport).to_image();
        if sticky {
            paint_fixture_header(&mut frame, 42);
        }
        frame
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }

    let min_y = *offsets.iter().min().expect("min y");
    let max_y = offsets.iter().max().expect("max y") + viewport;
    let mut expected = imageops::crop_imm(&canvas, x, min_y, viewport, max_y - min_y).to_image();
    if sticky {
        paint_fixture_header(&mut expected, 42);
    }
    expected
        .save(expected_dir.join("output.png"))
        .expect("save expected output");

    write_motions(
        &expected_dir.join("motions.json"),
        &offsets
            .windows(2)
            .enumerate()
            .map(|(idx, pair)| {
                let dy = pair[1] as i32 - pair[0] as i32;
                (idx + 1, 0, dy, if dy >= 0 { "Bottom" } else { "Top" })
            })
            .collect::<Vec<_>>(),
    );
}

fn write_horizontal_fixture(root: &Path, name: &str, offsets: [u32; 3], y: u32, viewport: u32) {
    let canvas = make_fixture_canvas(1100, 480);
    let dir = root.join(name);
    let frames_dir = dir.join("frames");
    let expected_dir = dir.join("expected");
    recreate_dir(&frames_dir);
    recreate_dir(&expected_dir);

    for (idx, x) in offsets.iter().enumerate() {
        imageops::crop_imm(&canvas, *x, y, viewport, viewport)
            .to_image()
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }

    let min_x = *offsets.iter().min().expect("min x");
    let max_x = offsets.iter().max().expect("max x") + viewport;
    imageops::crop_imm(&canvas, min_x, y, max_x - min_x, viewport)
        .to_image()
        .save(expected_dir.join("output.png"))
        .expect("save expected output");

    write_motions(
        &expected_dir.join("motions.json"),
        &offsets
            .windows(2)
            .enumerate()
            .map(|(idx, pair)| {
                let dx = pair[1] as i32 - pair[0] as i32;
                (idx + 1, dx, 0, if dx >= 0 { "Right" } else { "Left" })
            })
            .collect::<Vec<_>>(),
    );
}

fn write_akaze_fixture(root: &Path) {
    let canvas = make_akaze_fixture_canvas(320, 820);
    let dir = root.join("akaze_fallback");
    let frames_dir = dir.join("frames");
    let expected_dir = dir.join("expected");
    recreate_dir(&frames_dir);
    recreate_dir(&expected_dir);

    for (idx, y) in [0u32, 96, 192].iter().enumerate() {
        imageops::crop_imm(&canvas, 0, *y, 320, 320)
            .to_image()
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }

    imageops::crop_imm(&canvas, 0, 0, 320, 512)
        .to_image()
        .save(expected_dir.join("output.png"))
        .expect("save expected");
    write_motions(
        &expected_dir.join("motions.json"),
        &[(1, 0, 96, "Bottom"), (2, 0, 96, "Bottom")],
    );
}

/// Builds a fixture where every frame uses the same ambiguous canvas with a
/// small offset. The matcher must reject the append for either
/// `AmbiguousAxis`/second-best-margin reasons or `MotionTooSmall`; the
/// stitched output equals the first frame, and `motions.json` is empty.
fn write_rejected_fixture(root: &Path, name: &str, canvas_kind: FixtureCanvas) {
    let canvas = build_canvas(canvas_kind, 320, 700);
    let dir = root.join(name);
    let frames_dir = dir.join("frames");
    let expected_dir = dir.join("expected");
    recreate_dir(&frames_dir);
    recreate_dir(&expected_dir);

    for (idx, y) in [0u32, 32, 64].iter().enumerate() {
        imageops::crop_imm(&canvas, 0, *y, 320, 320)
            .to_image()
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }
    imageops::crop_imm(&canvas, 0, 0, 320, 320)
        .to_image()
        .save(expected_dir.join("output.png"))
        .expect("save expected");
    write_motions(&expected_dir.join("motions.json"), &[]);
}

fn write_bad_frame_fixture(root: &Path) {
    let canvas = make_fixture_canvas(320, 760);
    let dir = root.join("bad_frame");
    let frames_dir = dir.join("frames");
    let expected_dir = dir.join("expected");
    recreate_dir(&frames_dir);
    recreate_dir(&expected_dir);

    imageops::crop_imm(&canvas, 0, 0, 320, 320)
        .to_image()
        .save(frames_dir.join("frame_000.png"))
        .expect("save frame");
    RgbaImage::from_pixel(320, 320, Rgba([255, 255, 255, 255]))
        .save(frames_dir.join("frame_001.png"))
        .expect("save bad frame");
    imageops::crop_imm(&canvas, 0, 120, 320, 320)
        .to_image()
        .save(frames_dir.join("frame_002.png"))
        .expect("save recovery frame");
    imageops::crop_imm(&canvas, 0, 0, 320, 440)
        .to_image()
        .save(expected_dir.join("output.png"))
        .expect("save expected");
    write_motions(&expected_dir.join("motions.json"), &[(2, 0, 120, "Bottom")]);
}

fn write_duplicate_fixture(root: &Path) {
    let canvas = make_fixture_canvas(320, 760);
    let dir = root.join("duplicate_frames");
    let frames_dir = dir.join("frames");
    let expected_dir = dir.join("expected");
    recreate_dir(&frames_dir);
    recreate_dir(&expected_dir);

    let first = imageops::crop_imm(&canvas, 0, 0, 320, 320).to_image();
    first.save(frames_dir.join("frame_000.png")).expect("save first");
    first.save(frames_dir.join("frame_001.png")).expect("save duplicate");
    imageops::crop_imm(&canvas, 0, 100, 320, 320)
        .to_image()
        .save(frames_dir.join("frame_002.png"))
        .expect("save scrolled");
    imageops::crop_imm(&canvas, 0, 0, 320, 420)
        .to_image()
        .save(expected_dir.join("output.png"))
        .expect("save expected");
    write_motions(&expected_dir.join("motions.json"), &[(2, 0, 100, "Bottom")]);
}

fn recreate_dir(path: &Path) {
    let _ = fs::remove_dir_all(path);
    fs::create_dir_all(path).expect("create dir");
}

fn write_motions(path: &Path, motions: &[(usize, i32, i32, &str)]) {
    let mut out = String::from("[\n");
    for (idx, (frame, dx, dy, direction)) in motions.iter().enumerate() {
        let comma = if idx + 1 == motions.len() { "" } else { "," };
        out.push_str(&format!(
            "  {{ \"frame\": {frame}, \"dx\": {dx}, \"dy\": {dy}, \"direction\": \"{direction}\" }}{comma}\n"
        ));
    }
    out.push_str("]\n");
    fs::write(path, out).expect("write motions");
}

fn make_fixture_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([242, 242, 242, 255]));
    for y in (0..height).step_by(31) {
        for x in 16..width.saturating_sub(16) {
            let v = ((x / 5 + y / 7) % 180) as u8;
            img.put_pixel(x, y, Rgba([40 + v / 2, 80 + v / 3, 130 + v / 4, 255]));
        }
    }
    for i in 0..80u32 {
        let x = 18 + ((i * 47) % width.saturating_sub(36).max(1));
        let y = 18 + ((i * 59) % height.saturating_sub(36).max(1));
        let color = Rgba([
            (30 + (i * 13) % 190) as u8,
            (50 + (i * 17) % 170) as u8,
            (70 + (i * 23) % 150) as u8,
            255,
        ]);
        for yy in y..(y + 24).min(height) {
            for xx in x..(x + 42).min(width) {
                if xx % 7 == 0 || yy % 11 == 0 {
                    img.put_pixel(xx, yy, color);
                }
            }
        }
    }
    img
}

/// Mostly-white canvas with sparse word-shaped dark blocks. Simulates a
/// text-heavy page that has few strong corner features per viewport — the
/// matcher should still succeed because line ends and letter clusters are
/// locally distinctive across vertical scroll, but cheap matchers operate
/// with a thinner confidence margin.
fn make_low_feature_text_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([252, 252, 252, 255]));
    // Word-shaped blobs: 6-12 px tall, 20-60 px wide, deterministic positions.
    for line in 0..(height / 28) {
        let y = 16 + line * 28;
        if y + 12 >= height {
            break;
        }
        let mut x = 20u32;
        let mut word_idx = 0u32;
        while x + 24 < width.saturating_sub(20) {
            let word_w = 24 + ((line * 7 + word_idx * 11) % 38);
            let gray = (28 + (word_idx * 23) % 60) as u8;
            for yy in y..(y + 10).min(height) {
                for xx in x..(x + word_w).min(width) {
                    img.put_pixel(xx, yy, Rgba([gray, gray, gray, 255]));
                }
            }
            x += word_w + 12 + ((word_idx * 5) % 14);
            word_idx += 1;
        }
    }
    img
}

/// Canvas of large flat color cards with thin borders, like a feed of image
/// cards. Each card spans full width with consistent vertical rhythm so
/// matchers must distinguish between adjacent cards rather than picking any
/// repeated card. Cards have distinct interior colors to disambiguate.
fn make_image_card_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([248, 248, 248, 255]));
    let card_h = 140u32;
    let gap = 24u32;
    let mut idx = 0u32;
    let mut y = 16u32;
    while y + card_h < height {
        let interior = Rgba([
            (40 + (idx * 47) % 180) as u8,
            (60 + (idx * 53) % 170) as u8,
            (80 + (idx * 59) % 160) as u8,
            255,
        ]);
        let border = Rgba([30, 30, 30, 255]);
        for yy in y..(y + card_h).min(height) {
            for xx in 16..width.saturating_sub(16) {
                let on_border = yy == y
                    || yy + 1 == y + card_h
                    || xx == 16
                    || xx + 1 == width.saturating_sub(16);
                img.put_pixel(xx, yy, if on_border { border } else { interior });
            }
        }
        y += card_h + gap;
        idx += 1;
    }
    img
}

/// Tightly repeating horizontal stripes. Any vertical offset that's a
/// multiple of the stripe period looks equally plausible to a template
/// matcher, so this fixture confirms the second-best-margin rejection.
fn make_repeated_rows_canvas(width: u32, height: u32) -> RgbaImage {
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

/// Tightly repeating 2D grid. Both axes look ambiguous, so any offset that
/// aligns the grid is plausible. Stitcher must reject via second-best
/// margin and overlap verification.
fn make_repeated_grid_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([245, 245, 245, 255]));
    for y in 0..height {
        for x in 0..width {
            let v = if (x / 16 + y / 16) % 2 == 0 { 48 } else { 208 };
            img.put_pixel(x, y, Rgba([v, v, v, 255]));
        }
    }
    img
}

fn make_akaze_fixture_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([246, 246, 246, 255]));
    for y in 0..height {
        for x in 0..width {
            let v = if (x / 18 + y / 18) % 2 == 0 { 232 } else { 214 };
            img.put_pixel(x, y, Rgba([v, v, v, 255]));
        }
    }

    for i in 0..90u32 {
        let x = 20 + ((i * 43) % width.saturating_sub(40).max(1));
        let y = 20 + ((i * 61) % height.saturating_sub(40).max(1));
        let color = Rgba([
            (20 + (i * 19) % 180) as u8,
            (30 + (i * 23) % 160) as u8,
            (40 + (i * 29) % 150) as u8,
            255,
        ]);
        for yy in y..(y + 9).min(height) {
            for xx in x..(x + 9).min(width) {
                if xx == x || yy == y || xx + 1 == x + 9 || yy + 1 == y + 9 || xx == x + yy - y {
                    img.put_pixel(xx, yy, color);
                }
            }
        }
    }

    img
}

fn paint_fixture_header(frame: &mut RgbaImage, header_h: u32) {
    for y in 0..header_h.min(frame.height()) {
        for x in 0..frame.width() {
            let color = if (x / 6 + y / 4) % 2 == 0 {
                Rgba([180, 40, 40, 255])
            } else {
                Rgba([35, 35, 90, 255])
            };
            frame.put_pixel(x, y, color);
        }
    }
}
```

- [ ] **Step 4: Generate and commit fixtures**

Run:

```bash
rtk cargo test -p rollshot-core --test golden_fixtures refresh_linearscroll_v2_fixtures -- --ignored --nocapture
```

Expected: PASS and creates `crates/rollshot-core/tests/fixtures/linearscroll_v2/<family>/frames/*.png` plus `expected/output.png` and `expected/motions.json`.

- [ ] **Step 5: Run golden tests**

Run:

```bash
rtk cargo test -p rollshot-core --test golden_fixtures golden_fixtures_match_expected_outputs -- --nocapture
rtk cargo test -p rollshot-core --features akaze --test golden_fixtures akaze_golden_fixture_uses_akaze_fallback -- --nocapture
```

Expected: PASS. If any fixture fails, inspect `target/test-artifacts/<fixture-name>/report.json`, `actual.png`, `expected.png`, `diff.png`, and `matches.png`.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-core/tests/golden_fixtures.rs crates/rollshot-core/tests/fixtures/linearscroll_v2
rtk git commit -m "test(core): add linearscroll golden fixtures"
```

## Task 6: Add Debug Report and Artifact CLI Controls

**Files:**
- Modify: `crates/rollshot-cli/src/args.rs`
- Modify: `crates/rollshot-cli/src/cmd_stitch_folder.rs`
- Modify: `crates/rollshot-cli/tests/cli_smoke.rs`

- [ ] **Step 1: Add failing CLI debug test**

In `crates/rollshot-cli/tests/cli_smoke.rs`, add:

```rust
#[test]
fn rollshot_stitch_folder_writes_debug_report() {
    let tempdir = tempdir_for_test("rollshot-stitch-folder-debug");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");

    let canvas = make_scroll_canvas(160, 600);
    for (idx, y) in [0u32, 40, 80].iter().enumerate() {
        let frame = imageops::crop_imm(&canvas, 0, *y, canvas.width(), 160).to_image();
        frame
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }

    let output_png = tempdir.join("stitched.png");
    let report_json = tempdir.join("report.json");
    let debug_dir = tempdir.join("debug");

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("stitch-folder")
        .arg(&frames_dir)
        .arg("--output")
        .arg(&output_png)
        .arg("--debug-match-report")
        .arg(&report_json)
        .arg("--dump-overlap-debug")
        .arg(&debug_dir)
        .arg("--disable-akaze")
        .output()
        .expect("run rollshot stitch-folder");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = std::fs::read_to_string(&report_json).expect("read report");
    assert!(report.contains("\"frames\""), "report = {report}");
    assert!(report.contains("\"outcome\""), "report = {report}");
    assert!(debug_dir.exists(), "{} should exist", debug_dir.display());

    // The cheap matchers handle this canvas, so at least one Appended frame
    // (frame_001) should have written overlap_prev.png. A debug dir that
    // exists but is empty would mean the artifact-writing path silently
    // never fired.
    let overlap_entries: Vec<_> = std::fs::read_dir(&debug_dir)
        .expect("read debug dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.contains("overlap_prev") && n.ends_with(".png"))
                .unwrap_or(false)
        })
        .collect();
    assert!(
        !overlap_entries.is_empty(),
        "expected at least one overlap_prev PNG in {}, found {:?}",
        debug_dir.display(),
        std::fs::read_dir(&debug_dir)
            .map(|it| it.filter_map(Result::ok).map(|e| e.file_name()).collect::<Vec<_>>())
            .unwrap_or_default(),
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}

/// Verifies that `--disable-akaze` actually skips AKAZE: feeds the binary an
/// AKAZE-fallback canvas (repeated grid + sparse corners), then runs twice —
/// once with the feature available, once with `--disable-akaze`. The report
/// from the first run must contain at least one `"method": "Akaze"` entry;
/// the second must contain none. If `--disable-akaze` is silently broken,
/// the second assertion fails.
#[cfg(feature = "akaze")]
#[test]
fn rollshot_stitch_folder_disable_akaze_skips_akaze() {
    let tempdir = tempdir_for_test("rollshot-stitch-folder-disable-akaze");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");

    let canvas = make_akaze_fallback_smoke_canvas(320, 820);
    for (idx, y) in [0u32, 96, 192].iter().enumerate() {
        let frame = imageops::crop_imm(&canvas, 0, *y, 320, 320).to_image();
        frame
            .save(frames_dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }

    let run = |label: &str, extra_args: &[&str]| -> String {
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
        std::fs::read_to_string(&report_json).expect("read report")
    };

    let with_akaze = run("with", &[]);
    let without_akaze = run("without", &["--disable-akaze"]);

    assert!(
        with_akaze.contains("\"method\": \"Akaze\""),
        "expected AKAZE to fire in baseline run, report = {with_akaze}"
    );
    assert!(
        !without_akaze.contains("\"method\": \"Akaze\""),
        "--disable-akaze should suppress AKAZE entirely, report = {without_akaze}"
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}

/// AKAZE-friendly canvas: repeated grid (cheap matchers can't disambiguate)
/// with sparse unique corners (AKAZE can vote). Mirrors the
/// `make_akaze_fixture_canvas` used by the golden fixture.
#[cfg(feature = "akaze")]
fn make_akaze_fallback_smoke_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([246, 246, 246, 255]));
    for y in 0..height {
        for x in 0..width {
            let v = if (x / 18 + y / 18) % 2 == 0 { 232 } else { 214 };
            img.put_pixel(x, y, Rgba([v, v, v, 255]));
        }
    }
    for i in 0..90u32 {
        let x = 20 + ((i * 43) % width.saturating_sub(40).max(1));
        let y = 20 + ((i * 61) % height.saturating_sub(40).max(1));
        let color = Rgba([
            (20 + (i * 19) % 180) as u8,
            (30 + (i * 23) % 160) as u8,
            (40 + (i * 29) % 150) as u8,
            255,
        ]);
        for yy in y..(y + 9).min(height) {
            for xx in x..(x + 9).min(width) {
                if xx == x || yy == y || xx + 1 == x + 9 || yy + 1 == y + 9 || xx == x + yy - y {
                    img.put_pixel(xx, yy, color);
                }
            }
        }
    }
    img
}
```

- [ ] **Step 2: Run the CLI test to verify it fails**

Run:

```bash
rtk cargo test -p rollshot-cli --test cli_smoke rollshot_stitch_folder_writes_debug_report -- --nocapture
```

Expected: FAIL because the new flags are unknown.

- [ ] **Step 3: Add debug-only args**

In `crates/rollshot-cli/src/args.rs`, extend `StitchFolderArgs`:

```rust
#[derive(Debug, clap::Args)]
pub struct StitchFolderArgs {
    /// Directory of frames to stitch.
    pub frames_dir: PathBuf,

    /// Output PNG path.
    #[arg(long, short)]
    pub output: PathBuf,

    /// Write a JSON report with one match outcome per input frame.
    #[arg(long)]
    pub debug_match_report: Option<PathBuf>,

    /// Write overlap and diff images for frames with estimates.
    #[arg(long)]
    pub dump_overlap_debug: Option<PathBuf>,

    /// Diagnostic switch that forces AutoHybrid to skip AKAZE fallback.
    #[arg(long, default_value_t = false)]
    pub disable_akaze: bool,
}
```

- [ ] **Step 4: Add report structs and artifact helpers**

In `crates/rollshot-cli/src/cmd_stitch_folder.rs`, update imports:

```rust
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use rollshot_core::{
    MotionEstimate, OverlapRegion, StitchConfig, StitchOutcome, Stitcher,
};
use serde::Serialize;
```

Add these structs below the imports:

```rust
#[derive(Debug, Serialize)]
struct MatchReport {
    frames: Vec<FrameReport>,
}

#[derive(Debug, Serialize)]
struct FrameReport {
    frame_index: usize,
    path: String,
    outcome: String,
    reason: Option<String>,
    estimate: Option<EstimateReport>,
}

#[derive(Debug, Serialize)]
struct EstimateReport {
    dx: i32,
    dy: i32,
    direction: String,
    method: String,
    confidence: f32,
    inliers: Option<usize>,
    raw_matches: Option<usize>,
    overlap: OverlapReport,
}

#[derive(Debug, Serialize)]
struct OverlapReport {
    prev_x: u32,
    prev_y: u32,
    curr_x: u32,
    curr_y: u32,
    width: u32,
    height: u32,
}
```

Add helper functions:

```rust
fn estimate_report(estimate: &MotionEstimate) -> EstimateReport {
    EstimateReport {
        dx: estimate.dx,
        dy: estimate.dy,
        direction: format!("{:?}", estimate.direction),
        method: format!("{:?}", estimate.method),
        confidence: estimate.confidence,
        inliers: estimate.inliers,
        raw_matches: estimate.raw_matches,
        overlap: overlap_report(estimate.overlap),
    }
}

fn overlap_report(overlap: OverlapRegion) -> OverlapReport {
    OverlapReport {
        prev_x: overlap.prev_x,
        prev_y: overlap.prev_y,
        curr_x: overlap.curr_x,
        curr_y: overlap.curr_y,
        width: overlap.width,
        height: overlap.height,
    }
}

fn write_report(path: &Path, report: &MatchReport) -> Result<(), CliError> {
    let json = serde_json::to_string_pretty(report)
        .map_err(|err| CliError::new(format!("failed to encode match report: {err}"), 1))?;
    std::fs::write(path, json)
        .map_err(|err| CliError::new(format!("failed to write {}: {err}", path.display()), 1))
}

fn write_overlap_artifacts(
    dir: &Path,
    frame_index: usize,
    prev: &RgbaImage,
    curr: &RgbaImage,
    estimate: &MotionEstimate,
) -> Result<(), CliError> {
    std::fs::create_dir_all(dir).map_err(|err| {
        CliError::new(
            format!("failed to create debug dir {}: {err}", dir.display()),
            1,
        )
    })?;
    let prefix = format!("frame_{frame_index:03}");
    crop_overlap(prev, estimate.overlap.prev_x, estimate.overlap.prev_y, estimate.overlap.width, estimate.overlap.height)
        .save_with_format(dir.join(format!("{prefix}_overlap_prev.png")), ImageFormat::Png)
        .map_err(|err| CliError::new(format!("failed to save overlap prev: {err}"), 1))?;
    crop_overlap(curr, estimate.overlap.curr_x, estimate.overlap.curr_y, estimate.overlap.width, estimate.overlap.height)
        .save_with_format(dir.join(format!("{prefix}_overlap_curr.png")), ImageFormat::Png)
        .map_err(|err| CliError::new(format!("failed to save overlap curr: {err}"), 1))?;
    diff_overlap(prev, curr, estimate.overlap)
        .save_with_format(dir.join(format!("{prefix}_diff.png")), ImageFormat::Png)
        .map_err(|err| CliError::new(format!("failed to save overlap diff: {err}"), 1))?;
    Ok(())
}

fn crop_overlap(img: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
    image::imageops::crop_imm(img, x, y, w, h).to_image()
}

fn diff_overlap(prev: &RgbaImage, curr: &RgbaImage, overlap: OverlapRegion) -> RgbaImage {
    let mut out = RgbaImage::from_pixel(overlap.width, overlap.height, Rgba([0, 0, 0, 255]));
    for y in 0..overlap.height {
        for x in 0..overlap.width {
            let p = prev.get_pixel(overlap.prev_x + x, overlap.prev_y + y);
            let c = curr.get_pixel(overlap.curr_x + x, overlap.curr_y + y);
            out.put_pixel(
                x,
                y,
                Rgba([p[0].abs_diff(c[0]), p[1].abs_diff(c[1]), p[2].abs_diff(c[2]), 255]),
            );
        }
    }
    out
}
```

- [ ] **Step 5: Wire debug reporting in `run`**

In `cmd_stitch_folder.rs`, initialize config and debug state:

```rust
let mut config = StitchConfig::default();
if args.disable_akaze {
    config.akaze.enabled = false;
}
let mut stitcher = Stitcher::new(config);
let mut report = MatchReport { frames: Vec::new() };
let mut last_accepted: Option<RgbaImage> = None;
```

Replace the `match stitcher.push_frame(frame)` block in the frame loop with:

```rust
let outcome = stitcher.push_frame(frame.clone());
let mut frame_report = FrameReport {
    frame_index: report.frames.len(),
    path: path.display().to_string(),
    outcome: String::new(),
    reason: None,
    estimate: None,
};

match &outcome {
    StitchOutcome::FirstFrame => {
        frame_report.outcome = "FirstFrame".to_string();
        last_accepted = Some(frame);
    }
    StitchOutcome::Appended { estimate, .. } => {
        appended += 1;
        frame_report.outcome = "Appended".to_string();
        frame_report.estimate = Some(estimate_report(estimate));
        if let (Some(dir), Some(prev)) = (args.dump_overlap_debug.as_ref(), last_accepted.as_ref()) {
            write_overlap_artifacts(dir, report.frames.len(), prev, &frame, estimate)?;
        }
        last_accepted = Some(frame);
    }
    StitchOutcome::Duplicate => {
        duplicates += 1;
        frame_report.outcome = "Duplicate".to_string();
    }
    StitchOutcome::NoMatch {
        reason,
        best_estimate,
    } => {
        no_match += 1;
        frame_report.outcome = "NoMatch".to_string();
        frame_report.reason = Some(format!("{reason:?}"));
        frame_report.estimate = best_estimate.as_ref().map(estimate_report);
        if let (Some(dir), Some(prev), Some(estimate)) = (
            args.dump_overlap_debug.as_ref(),
            last_accepted.as_ref(),
            best_estimate.as_ref(),
        ) {
            write_overlap_artifacts(dir, report.frames.len(), prev, &frame, estimate)?;
        }
    }
    StitchOutcome::NoProgress { estimate } => {
        no_progress += 1;
        frame_report.outcome = "NoProgress".to_string();
        frame_report.estimate = estimate.as_ref().map(estimate_report);
    }
    StitchOutcome::AxisChanged { estimate, .. } => {
        no_match += 1;
        frame_report.outcome = "AxisChanged".to_string();
        frame_report.estimate = Some(estimate_report(estimate));
    }
}

report.frames.push(frame_report);
```

Before returning `Ok(format!(...))`, add:

```rust
if let Some(path) = args.debug_match_report.as_ref() {
    write_report(path, &report)?;
}
```

- [ ] **Step 6: Run CLI debug tests**

Run:

```bash
rtk cargo test -p rollshot-cli --test cli_smoke rollshot_stitch_folder_writes_debug_report -- --nocapture
rtk cargo test -p rollshot-cli --features akaze --test cli_smoke rollshot_stitch_folder_disable_akaze_skips_akaze -- --nocapture
```

Expected: PASS for both. The baseline test asserts the report JSON contains frame outcomes AND that the debug directory contains at least one `*_overlap_prev.png`. The AKAZE-feature test asserts that `--disable-akaze` produces a report without any `"method": "Akaze"` entry while the same fixture WITH AKAZE enabled produces at least one such entry — proving the diagnostic switch actually fires.

- [ ] **Step 7: Run normal CLI smoke test**

Run:

```bash
rtk cargo test -p rollshot-cli --test cli_smoke -- --nocapture
```

Expected: PASS. Existing `stitch-folder` behavior remains unchanged when debug flags are absent.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-cli/src/args.rs crates/rollshot-cli/src/cmd_stitch_folder.rs crates/rollshot-cli/tests/cli_smoke.rs
rtk git commit -m "feat(cli): add stitch-folder match debug output"
```

## Task 7: Update CI and README Verification

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `README.md`

- [ ] **Step 1: Add AKAZE CI step**

In `.github/workflows/ci.yml`, add this step after the existing `Test` step:

```yaml
      - name: Test AKAZE feature
        run: cargo test --workspace --features akaze
```

- [ ] **Step 2: Update README local verification commands**

In `README.md`, in both the local development command block and GitHub Actions command block, add:

```bash
cargo test --workspace --features akaze
```

In the manual checklist after `cargo test --workspace`, add:

```markdown
- [ ] `cargo test --workspace --features akaze` passes.
```

- [ ] **Step 3: Verify workflow syntax**

Run:

```bash
rtk ruby -e 'require "yaml"; YAML.load_file(".github/workflows/ci.yml"); puts "ok"'
```

Expected: prints `ok`.

- [ ] **Step 4: Run local CI-equivalent commands**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk cargo test --workspace
rtk cargo test --workspace --features akaze
```

Expected: all commands PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add .github/workflows/ci.yml README.md
rtk git commit -m "ci: test akaze feature path"
```

## Completion Criteria

- `akaze` is pinned to `https://github.com/rust-cv/cv.git` at rev `d271a9ac6a9d7b39c6f22573d26d63b5ce94f3cb`.
- Default builds do not compile AKAZE dependencies.
- `rtk cargo test -p rollshot-core --features akaze --lib akaze_matcher::tests -- --nocapture` passes.
- `rtk cargo test -p rollshot-core --features akaze --test stitcher akaze_fallback_appends_when_template_is_ambiguous -- --nocapture` passes and observes `MatchMethod::Akaze`.
- Golden fixtures pass in the baseline build.
- `akaze_fallback` golden fixture passes in the AKAZE-enabled build and includes at least one `MatchMethod::Akaze` motion.
- `rollshot stitch-folder` exposes debug-only `--debug-match-report`, `--dump-overlap-debug`, and `--disable-akaze`; normal commands expose no algorithm picker.
- CI runs baseline tests and AKAZE-enabled tests.

## Self-Review Notes

- Spec coverage: dependency decision, keypoint extraction, descriptor matching, translation voting, inlier filtering, verifier integration, debug match reports, golden fixtures, AKAZE fallback fixture, and CI feature coverage are each mapped to a task.
- Type consistency: `AkazeConfig` is part of `StitchConfig`, exported from `rollshot-core`, and consumed by `matcher.rs`, CLI debug controls, and tests with the same field names.
- Dependency hygiene: this plan uses direct `akaze`, `bitarray`, and `space`; it does not use the umbrella `cv` crate or local path crates from `learn-projects/rust-cv`.
