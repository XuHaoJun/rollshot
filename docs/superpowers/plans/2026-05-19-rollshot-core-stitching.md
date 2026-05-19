# Rollshot Core Stitching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first real `rollshot-core` Stitcher (duplicate detection + template-based vertical offset matching with content ROI + safe anchor preservation) and wire it into a working `rollshot stitch-folder <frames-dir> --output <png>` CLI command.

**Architecture:** `rollshot-core` exposes a single `Stitcher` type that consumes `image::RgbaImage` frames one at a time, runs duplicate detection against the last good anchor, estimates a vertical offset with normalized cross-correlation on a content ROI, and either appends a bottom slice or rejects the frame without poisoning the anchor. The CLI sorts a folder of frames, decodes them with the `image` crate, feeds them to a `Stitcher`, and saves the stitched PNG.

**Tech Stack:** Rust 2021, `image` 0.25 (PNG/JPEG features only), workspace `[workspace.dependencies]`, standard library elsewhere, deterministic synthetic fixtures for tests.

---

## File Structure

- Modify: `Cargo.toml`
  Add `[workspace.dependencies]` with `image`.
- Modify: `crates/rollshot-core/Cargo.toml`
  Depend on workspace `image`.
- Replace: `crates/rollshot-core/src/lib.rs`
  Module declarations and public re-exports only.
- Create: `crates/rollshot-core/src/types.rs`
  `MatchAlgorithm`, `StitchConfig` (+ `Default`), `StitchOutcome`, `StitchStats`, `OffsetEstimate`.
- Create: `crates/rollshot-core/src/image_ext.rs`
  `append_below` helper for appending the bottom N rows of a frame to a stitched image.
- Create: `crates/rollshot-core/src/duplicate.rs`
  Frame signature builder and mean-absolute-difference duplicate check.
- Create: `crates/rollshot-core/src/matcher.rs`
  Grayscale conversion, content ROI, NCC scoring, predict-aware search, second-best margin check, overlap MAD verification, `estimate_offset`.
- Create: `crates/rollshot-core/src/stitcher.rs`
  `Stitcher` state machine and `push_frame` flow.
- Create: `crates/rollshot-core/tests/common/mod.rs`
  Deterministic synthetic canvas + crop + sticky-header helpers shared by integration tests.
- Create: `crates/rollshot-core/tests/stitcher.rs`
  Spec-required Stitcher integration tests.
- Modify: `crates/rollshot-cli/Cargo.toml`
  Depend on workspace `image`.
- Modify: `crates/rollshot-cli/src/lib.rs`
  Replace the bootstrap stub `stitch_folder` with the real implementation; update help text.
- Modify: `crates/rollshot-cli/tests/cli_smoke.rs`
  Add a folder-stitching smoke test against the compiled binary.

---

## Task 1: Add the `image` Workspace Dependency

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/rollshot-core/Cargo.toml`

- [ ] **Step 1: Declare `image` in the workspace manifest**

Replace `Cargo.toml` with:

```toml
[workspace]
members = [
    "crates/rollshot-core",
    "crates/rollshot-capture",
    "crates/rollshot-cli",
    "crates/rollshot-app",
]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
repository = "https://github.com/xuhaojun/rollshot"
rust-version = "1.80"

[workspace.dependencies]
image = { version = "0.25", default-features = false, features = ["png", "jpeg"] }

[workspace.lints.rust]
unsafe_code = "forbid"
```

- [ ] **Step 2: Wire `image` into `rollshot-core`**

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

[lints]
workspace = true
```

- [ ] **Step 3: Verify the workspace still builds**

Run: `rtk cargo build -p rollshot-core`

Expected: PASS. The crate builds with the new `image` dependency and the existing `lib.rs` unchanged.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml crates/rollshot-core/Cargo.toml Cargo.lock
git commit -m "build(core): add image crate workspace dependency"
```

---

## Task 2: Extract Public Types into `types.rs`

**Files:**
- Create: `crates/rollshot-core/src/types.rs`
- Modify: `crates/rollshot-core/src/lib.rs`

- [ ] **Step 1: Move existing types and add the new public types**

Create `crates/rollshot-core/src/types.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchAlgorithm {
    Template,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StitchConfig {
    pub algorithm: MatchAlgorithm,
    pub min_overlap: u32,
    pub min_append: u32,
    pub accept_diff: f32,
    pub match_width: u32,
    pub duplicate_threshold: f32,
}

impl Default for StitchConfig {
    fn default() -> Self {
        Self {
            algorithm: MatchAlgorithm::Template,
            min_overlap: 64,
            min_append: 8,
            accept_diff: 0.15,
            match_width: 512,
            duplicate_threshold: 0.01,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StitchStats {
    pub frame_count: u32,
    pub total_height: u32,
    pub last_append: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OffsetEstimate {
    pub dy: i32,
    pub confidence: f32,
    pub method: MatchAlgorithm,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StitchOutcome {
    FirstFrame,
    Appended { added: u32 },
    NoProgress,
    NoMatch { confidence: f32 },
    Duplicate,
}

#[cfg(test)]
mod tests {
    use super::{MatchAlgorithm, StitchConfig, StitchOutcome};

    #[test]
    fn default_config_uses_template_matching() {
        let config = StitchConfig::default();

        assert_eq!(config.algorithm, MatchAlgorithm::Template);
        assert_eq!(config.min_overlap, 64);
        assert_eq!(config.min_append, 8);
        assert_eq!(config.match_width, 512);
        assert_eq!(config.duplicate_threshold, 0.01);
    }

    #[test]
    fn stitch_outcome_distinguishes_variants() {
        let appended = StitchOutcome::Appended { added: 12 };
        let no_match = StitchOutcome::NoMatch { confidence: 0.42 };

        assert_ne!(appended, StitchOutcome::FirstFrame);
        assert_ne!(no_match, StitchOutcome::Duplicate);
        assert_ne!(no_match, StitchOutcome::NoProgress);
    }
}
```

- [ ] **Step 2: Replace `lib.rs` with module declarations + re-exports**

Replace `crates/rollshot-core/src/lib.rs` with:

```rust
mod types;

pub use types::{MatchAlgorithm, OffsetEstimate, StitchConfig, StitchOutcome, StitchStats};
```

- [ ] **Step 3: Run the type tests**

Run: `rtk cargo test -p rollshot-core --lib`

Expected: PASS. Two tests run — `default_config_uses_template_matching` and `stitch_outcome_distinguishes_variants` — and both pass.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-core/src/lib.rs crates/rollshot-core/src/types.rs
git commit -m "refactor(core): extract types module and add stitch outcome enum"
```

---

## Task 3: `image_ext::append_below` Helper

**Files:**
- Create: `crates/rollshot-core/src/image_ext.rs`
- Modify: `crates/rollshot-core/src/lib.rs`

- [ ] **Step 1: Write the failing append test**

Create `crates/rollshot-core/src/image_ext.rs`:

```rust
use image::{GenericImage, GenericImageView, RgbaImage};

/// Returns a new image with the bottom `dy` rows of `frame` stacked under `base`.
///
/// `base` and `frame` must share the same width. When `dy` is 0 the function
/// returns a clone of `base`. When `dy` is larger than `frame.height()` the full
/// frame is appended.
pub fn append_below(base: &RgbaImage, frame: &RgbaImage, dy: u32) -> RgbaImage {
    assert_eq!(
        base.width(),
        frame.width(),
        "append_below requires equal widths"
    );

    if dy == 0 {
        return base.clone();
    }

    let dy = dy.min(frame.height());
    let mut combined = RgbaImage::new(base.width(), base.height() + dy);
    combined
        .copy_from(base, 0, 0)
        .expect("copy base into combined");

    let overlap = frame.height() - dy;
    let slice = frame.view(0, overlap, frame.width(), dy).to_image();
    combined
        .copy_from(&slice, 0, base.height())
        .expect("copy slice into combined");

    combined
}

#[cfg(test)]
mod tests {
    use super::append_below;
    use image::{Rgba, RgbaImage};

    fn solid(width: u32, height: u32, color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba(color))
    }

    #[test]
    fn dy_zero_returns_clone_of_base() {
        let base = solid(4, 3, [10, 20, 30, 255]);
        let frame = solid(4, 3, [40, 50, 60, 255]);

        let combined = append_below(&base, &frame, 0);

        assert_eq!(combined.dimensions(), (4, 3));
        assert_eq!(combined.get_pixel(0, 0), &Rgba([10, 20, 30, 255]));
    }

    #[test]
    fn appends_bottom_rows_of_frame() {
        let base = solid(2, 2, [10, 10, 10, 255]);
        let mut frame = solid(2, 4, [0, 0, 0, 255]);
        frame.put_pixel(0, 2, Rgba([1, 1, 1, 255]));
        frame.put_pixel(1, 2, Rgba([2, 2, 2, 255]));
        frame.put_pixel(0, 3, Rgba([3, 3, 3, 255]));
        frame.put_pixel(1, 3, Rgba([4, 4, 4, 255]));

        let combined = append_below(&base, &frame, 2);

        assert_eq!(combined.dimensions(), (2, 4));
        assert_eq!(combined.get_pixel(0, 0), &Rgba([10, 10, 10, 255]));
        assert_eq!(combined.get_pixel(0, 2), &Rgba([1, 1, 1, 255]));
        assert_eq!(combined.get_pixel(1, 3), &Rgba([4, 4, 4, 255]));
    }

    #[test]
    fn dy_larger_than_frame_height_appends_full_frame() {
        let base = solid(2, 1, [10, 10, 10, 255]);
        let frame = solid(2, 2, [7, 7, 7, 255]);

        let combined = append_below(&base, &frame, 999);

        assert_eq!(combined.dimensions(), (2, 3));
        assert_eq!(combined.get_pixel(0, 1), &Rgba([7, 7, 7, 255]));
        assert_eq!(combined.get_pixel(1, 2), &Rgba([7, 7, 7, 255]));
    }
}
```

- [ ] **Step 2: Register the module in `lib.rs`**

Replace `crates/rollshot-core/src/lib.rs` with:

```rust
mod image_ext;
mod types;

pub use types::{MatchAlgorithm, OffsetEstimate, StitchConfig, StitchOutcome, StitchStats};
```

- [ ] **Step 3: Run the new tests and verify they pass**

Run: `rtk cargo test -p rollshot-core --lib image_ext::`

Expected: PASS. Three tests pass: `dy_zero_returns_clone_of_base`, `appends_bottom_rows_of_frame`, `dy_larger_than_frame_height_appends_full_frame`.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-core/src/image_ext.rs crates/rollshot-core/src/lib.rs
git commit -m "feat(core): add append_below image helper"
```

---

## Task 4: `duplicate::is_duplicate` Detection

**Files:**
- Create: `crates/rollshot-core/src/duplicate.rs`
- Modify: `crates/rollshot-core/src/lib.rs`

- [ ] **Step 1: Write the failing duplicate-detection tests**

Create `crates/rollshot-core/src/duplicate.rs`:

```rust
use image::RgbaImage;

/// Number of horizontal samples taken per row when building a duplicate signature.
const SIGNATURE_COLS: u32 = 18;
/// Number of rows sampled when building a duplicate signature.
const SIGNATURE_ROWS: u32 = 24;

/// Builds a tiny grayscale signature of the frame for cheap duplicate detection.
///
/// The frame is sampled on a fixed `rows x cols` grid (no smoothing). The result
/// is a stable fingerprint that catches frames the user has not scrolled.
pub fn signature(frame: &RgbaImage) -> Vec<u8> {
    sample(frame, SIGNATURE_COLS, SIGNATURE_ROWS)
}

/// Returns `true` when the mean absolute difference between two signatures,
/// normalized into the `[0.0, 1.0]` range, is at or below `threshold`.
pub fn is_duplicate(prev: &[u8], curr: &[u8], threshold: f32) -> bool {
    if prev.len() != curr.len() || prev.is_empty() {
        return false;
    }

    let mut sum = 0.0f32;
    for (&a, &b) in prev.iter().zip(curr.iter()) {
        sum += a.abs_diff(b) as f32;
    }

    let mad = sum / (prev.len() as f32 * 255.0);
    mad <= threshold
}

fn sample(frame: &RgbaImage, cols: u32, rows: u32) -> Vec<u8> {
    let width = frame.width().max(1);
    let height = frame.height().max(1);
    let cols = cols.max(1);
    let rows = rows.max(1);
    let mut out = Vec::with_capacity((cols * rows) as usize);

    for row in 0..rows {
        let y = ((row * height) / rows).min(height - 1);
        for col in 0..cols {
            let x = ((col * width) / cols).min(width - 1);
            let p = frame.get_pixel(x, y);
            let gray = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
            out.push(gray as u8);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{is_duplicate, signature};
    use image::{Rgba, RgbaImage};

    fn checkerboard(width: u32, height: u32, shift: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            let on = ((x + shift) ^ y) & 1 == 0;
            if on {
                Rgba([220, 220, 220, 255])
            } else {
                Rgba([20, 20, 20, 255])
            }
        })
    }

    #[test]
    fn identical_frames_are_duplicates() {
        let frame = checkerboard(64, 64, 0);
        let sig_a = signature(&frame);
        let sig_b = signature(&frame);

        assert!(is_duplicate(&sig_a, &sig_b, 0.01));
    }

    #[test]
    fn very_different_frames_are_not_duplicates() {
        let a = RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 255]));
        let b = RgbaImage::from_pixel(64, 64, Rgba([255, 255, 255, 255]));

        let sig_a = signature(&a);
        let sig_b = signature(&b);

        assert!(!is_duplicate(&sig_a, &sig_b, 0.01));
    }

    #[test]
    fn mismatched_signature_lengths_are_not_duplicates() {
        let short = vec![10u8; 4];
        let long = vec![10u8; 8];

        assert!(!is_duplicate(&short, &long, 0.5));
    }

    #[test]
    fn signature_length_matches_grid_size() {
        let frame = checkerboard(64, 64, 0);
        let sig = signature(&frame);

        assert_eq!(sig.len(), 18 * 24);
    }
}
```

- [ ] **Step 2: Register the module in `lib.rs`**

Replace `crates/rollshot-core/src/lib.rs` with:

```rust
mod duplicate;
mod image_ext;
mod types;

pub use types::{MatchAlgorithm, OffsetEstimate, StitchConfig, StitchOutcome, StitchStats};
```

- [ ] **Step 3: Run the duplicate tests**

Run: `rtk cargo test -p rollshot-core --lib duplicate::`

Expected: PASS. Four tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-core/src/duplicate.rs crates/rollshot-core/src/lib.rs
git commit -m "feat(core): add signature-based duplicate detection"
```

---

## Task 5: `matcher::estimate_offset` Template Matching

**Files:**
- Create: `crates/rollshot-core/src/matcher.rs`
- Modify: `crates/rollshot-core/src/lib.rs`

- [ ] **Step 1: Write the failing matcher tests**

Create `crates/rollshot-core/src/matcher.rs`:

```rust
use image::{Rgba, RgbaImage};

use crate::types::{MatchAlgorithm, OffsetEstimate, StitchConfig};

const TOP_IGNORE_RATIO: f32 = 0.12;
const BOTTOM_IGNORE_RATIO: f32 = 0.08;
const SIDE_IGNORE_RATIO: f32 = 0.04;
const MIN_IGNORE_PX: u32 = 24;
const TEMPLATE_MIN_HEIGHT: u32 = 48;
const SECOND_BEST_MIN_MARGIN: f32 = 0.015;
const VERIFY_MAX_NORMALIZED_DIFF: f32 = 18.0 / 255.0;

/// Estimates the vertical offset that takes `prev` onto `curr`.
///
/// Confidence follows the rollshot convention: lower is better, and an
/// estimate is acceptable when `confidence <= StitchConfig::accept_diff`.
///
/// Returns an estimate with `confidence = f32::INFINITY` when the frames
/// are too small to match, the content ROI is empty, the best score is
/// indistinguishable from the second-best, or the verification step
/// reports too much pixel disagreement.
pub fn estimate_offset(
    prev: &RgbaImage,
    curr: &RgbaImage,
    last_offset: i32,
    config: &StitchConfig,
) -> OffsetEstimate {
    let no_match = OffsetEstimate {
        dy: 0,
        confidence: f32::INFINITY,
        method: config.algorithm,
    };

    if prev.dimensions() != curr.dimensions() {
        return no_match;
    }

    let width = prev.width();
    let height = prev.height();
    if height < 100 || width < 50 {
        return no_match;
    }

    let (roi_x, roi_y, roi_w, roi_h) = content_roi(width, height);
    if roi_h < TEMPLATE_MIN_HEIGHT * 2 || roi_w < 40 {
        return no_match;
    }

    let template_h = (roi_h / 3).max(TEMPLATE_MIN_HEIGHT).min(roi_h - 1);
    let search_start = roi_y as i32;
    let search_end = (roi_y + roi_h - template_h) as i32;
    if search_end <= search_start {
        return no_match;
    }

    let prev_gray = to_grayscale(prev);
    let curr_gray = to_grayscale(curr);

    let max_offset =
        (height as i32 - config.min_overlap as i32).max(0).min(search_end - search_start);
    let predict = last_offset.clamp(0, max_offset);

    let mut best_offset = 0i32;
    let mut best_score = f32::MIN;
    let mut second_score = f32::MIN;

    for offset in predict_iter(search_end - search_start, predict) {
        let search_y = search_start + offset;
        if search_y < 0 || search_y + template_h as i32 > height as i32 {
            continue;
        }

        let score = ncc_score_region(
            &prev_gray,
            &curr_gray,
            width,
            roi_x,
            roi_w,
            search_y as u32,
            roi_y,
            template_h,
        );

        if score > best_score {
            second_score = best_score;
            best_score = score;
            best_offset = offset;
        } else if score > second_score {
            second_score = score;
        }
    }

    if !best_score.is_finite() || best_score <= 0.0 {
        return no_match;
    }

    if second_score.is_finite() && best_score - second_score < SECOND_BEST_MIN_MARGIN {
        return no_match;
    }

    let overlap_h = height.saturating_sub(best_offset as u32);
    let verify = overlap_mean_abs_diff(
        &prev_gray,
        &curr_gray,
        width,
        roi_x,
        roi_w,
        best_offset as u32,
        overlap_h,
    );

    if !verify.is_finite() || verify > VERIFY_MAX_NORMALIZED_DIFF {
        return no_match;
    }

    let confidence = (1.0 - best_score.clamp(0.0, 1.0)) + verify * 0.5;
    OffsetEstimate {
        dy: best_offset,
        confidence,
        method: MatchAlgorithm::Template,
    }
}

fn content_roi(width: u32, height: u32) -> (u32, u32, u32, u32) {
    let side = ((width as f32 * SIDE_IGNORE_RATIO) as u32).max(MIN_IGNORE_PX);
    let top = ((height as f32 * TOP_IGNORE_RATIO) as u32).max(MIN_IGNORE_PX);
    let bottom = ((height as f32 * BOTTOM_IGNORE_RATIO) as u32).max(MIN_IGNORE_PX);
    let x = side.min(width.saturating_sub(1));
    let y = top.min(height.saturating_sub(1));
    let roi_w = width.saturating_sub(x.saturating_mul(2)).max(1);
    let roi_h = height.saturating_sub(y).saturating_sub(bottom).max(1);
    (x, y, roi_w, roi_h)
}

fn to_grayscale(img: &RgbaImage) -> Vec<f32> {
    img.pixels()
        .map(|Rgba([r, g, b, _])| 0.299 * *r as f32 + 0.587 * *g as f32 + 0.114 * *b as f32)
        .collect()
}

fn predict_iter(max: i32, predict: i32) -> Vec<i32> {
    let p = predict.clamp(0, max);
    let mut out = Vec::with_capacity((max as usize).saturating_mul(2) + 1);
    out.push(p);
    for delta in 1..=max {
        if p + delta <= max {
            out.push(p + delta);
        }
        if p - delta >= 0 {
            out.push(p - delta);
        }
    }
    out
}

fn ncc_score_region(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    roi_x: u32,
    roi_w: u32,
    prev_y: u32,
    curr_y: u32,
    template_h: u32,
) -> f32 {
    if roi_w == 0 || template_h == 0 || width == 0 {
        return f32::MIN;
    }

    let mut prev_sum = 0.0f32;
    let mut curr_sum = 0.0f32;
    let mut count = 0usize;

    for row in 0..template_h {
        let prev_base = ((prev_y + row) * width + roi_x) as usize;
        let curr_base = ((curr_y + row) * width + roi_x) as usize;
        for col in 0..roi_w as usize {
            prev_sum += prev_gray[prev_base + col];
            curr_sum += curr_gray[curr_base + col];
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

    for row in 0..template_h {
        let prev_base = ((prev_y + row) * width + roi_x) as usize;
        let curr_base = ((curr_y + row) * width + roi_x) as usize;
        for col in 0..roi_w as usize {
            let p = prev_gray[prev_base + col] - prev_mean;
            let c = curr_gray[curr_base + col] - curr_mean;
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

fn overlap_mean_abs_diff(
    prev_gray: &[f32],
    curr_gray: &[f32],
    width: u32,
    roi_x: u32,
    roi_w: u32,
    offset: u32,
    overlap_h: u32,
) -> f32 {
    if roi_w == 0 || overlap_h == 0 {
        return f32::INFINITY;
    }

    let sample_h = overlap_h.min(160);
    let prev_start_y = offset + overlap_h.saturating_sub(sample_h);
    let curr_start_y = overlap_h.saturating_sub(sample_h);

    let mut sum = 0.0f32;
    let mut count = 0usize;
    for row in 0..sample_h {
        let prev_base = ((prev_start_y + row) * width + roi_x) as usize;
        let curr_base = ((curr_start_y + row) * width + roi_x) as usize;
        for col in 0..roi_w as usize {
            sum += (prev_gray[prev_base + col] - curr_gray[curr_base + col]).abs();
            count += 1;
        }
    }

    if count == 0 {
        return f32::INFINITY;
    }

    sum / (count as f32 * 255.0)
}

#[cfg(test)]
mod tests {
    use super::{content_roi, estimate_offset};
    use crate::types::{MatchAlgorithm, StitchConfig};
    use image::{imageops, Rgba, RgbaImage};

    fn make_textured_canvas(width: u32, height: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(width, height, Rgba([245, 245, 245, 255]));
        for y in (0..height).step_by(11) {
            let accent = ((y / 3) % 180) as u8;
            for x in 8..width.saturating_sub(8) {
                let stripe = if (x / 5 + y / 7) % 2 == 0 { 220 } else { 180 };
                img.put_pixel(x, y, Rgba([accent, stripe, 80, 255]));
                if y + 1 < height {
                    img.put_pixel(x, y + 1, Rgba([30, 30, 30, 255]));
                }
            }
        }
        for col in [21, 47, 73, 99, 125] {
            if col >= width {
                continue;
            }
            for y in 12..height.saturating_sub(12) {
                if (y / 13) % 3 != 0 {
                    img.put_pixel(col, y, Rgba([20, 20, 20, 255]));
                }
            }
        }
        img
    }

    fn crop(canvas: &RgbaImage, y: u32, h: u32) -> RgbaImage {
        imageops::crop_imm(canvas, 0, y, canvas.width(), h).to_image()
    }

    #[test]
    fn content_roi_skips_borders() {
        let (x, y, w, h) = content_roi(320, 320);

        assert!(x >= 24);
        assert!(y >= 24);
        assert!(w < 320);
        assert!(h < 320);
    }

    #[test]
    fn estimate_offset_finds_known_scroll() {
        let canvas = make_textured_canvas(160, 600);
        let prev = crop(&canvas, 0, 160);
        let curr = crop(&canvas, 40, 160);

        let estimate = estimate_offset(&prev, &curr, 0, &StitchConfig::default());

        assert_eq!(estimate.method, MatchAlgorithm::Template);
        assert!(
            (estimate.dy - 40).abs() <= 2,
            "dy = {} (expected ~40)",
            estimate.dy
        );
        assert!(
            estimate.confidence < StitchConfig::default().accept_diff,
            "confidence = {} (expected < {})",
            estimate.confidence,
            StitchConfig::default().accept_diff
        );
    }

    #[test]
    fn estimate_offset_rejects_unrelated_frames() {
        let prev = make_textured_canvas(160, 160);
        let curr = RgbaImage::from_pixel(160, 160, Rgba([255, 255, 255, 255]));

        let estimate = estimate_offset(&prev, &curr, 0, &StitchConfig::default());

        assert!(
            estimate.confidence > StitchConfig::default().accept_diff,
            "confidence = {} (expected > accept_diff)",
            estimate.confidence
        );
    }

    #[test]
    fn estimate_offset_rejects_dimension_mismatch() {
        let prev = make_textured_canvas(160, 160);
        let curr = make_textured_canvas(160, 200);

        let estimate = estimate_offset(&prev, &curr, 0, &StitchConfig::default());

        assert!(!estimate.confidence.is_finite());
    }
}
```

- [ ] **Step 2: Register the module in `lib.rs`**

Replace `crates/rollshot-core/src/lib.rs` with:

```rust
mod duplicate;
mod image_ext;
mod matcher;
mod types;

pub use types::{MatchAlgorithm, OffsetEstimate, StitchConfig, StitchOutcome, StitchStats};
```

- [ ] **Step 3: Run the matcher tests**

Run: `rtk cargo test -p rollshot-core --lib matcher::`

Expected: PASS. Four tests pass: `content_roi_skips_borders`, `estimate_offset_finds_known_scroll`, `estimate_offset_rejects_unrelated_frames`, `estimate_offset_rejects_dimension_mismatch`.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-core/src/matcher.rs crates/rollshot-core/src/lib.rs
git commit -m "feat(core): add template-based vertical offset matcher"
```

---

## Task 6: `Stitcher` Skeleton + First-Frame Behavior

**Files:**
- Create: `crates/rollshot-core/src/stitcher.rs`
- Create: `crates/rollshot-core/tests/common/mod.rs`
- Create: `crates/rollshot-core/tests/stitcher.rs`
- Modify: `crates/rollshot-core/src/lib.rs`

- [ ] **Step 1: Add the shared synthetic fixture helper**

Create `crates/rollshot-core/tests/common/mod.rs`:

```rust
#![allow(dead_code)]

use image::{imageops, Rgba, RgbaImage};

/// Builds a tall, deterministic canvas with stripes, color blocks and column
/// patterns. The texture is rich enough that NCC template matching picks a
/// confident offset on any viewport-sized crop.
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

    for block in 0..10u32 {
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

    for col in [42u32, 96, 154, 211, 268] {
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

/// Crops a viewport-sized frame from the canvas.
pub fn crop_frame(canvas: &RgbaImage, y: u32, height: u32) -> RgbaImage {
    imageops::crop_imm(canvas, 0, y, canvas.width(), height).to_image()
}

/// Overlays a constant header band on a frame, simulating a sticky UI header.
pub fn paint_sticky_header(frame: &mut RgbaImage, header_h: u32) {
    let header_h = header_h.min(frame.height());
    for y in 0..header_h {
        for x in 0..frame.width() {
            let on = ((x / 4) + (y / 3)) % 2 == 0;
            let color = if on {
                Rgba([200, 60, 60, 255])
            } else {
                Rgba([30, 30, 90, 255])
            };
            frame.put_pixel(x, y, color);
        }
    }
}
```

- [ ] **Step 2: Write the failing first-frame integration test**

Create `crates/rollshot-core/tests/stitcher.rs`:

```rust
mod common;

use common::{crop_frame, make_scroll_canvas};
use rollshot_core::{StitchConfig, StitchOutcome, Stitcher};

#[test]
fn first_frame_initializes_stitched_image() {
    let canvas = make_scroll_canvas(320, 1000);
    let first = crop_frame(&canvas, 0, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());

    assert_eq!(stitcher.push_frame(first.clone()), StitchOutcome::FirstFrame);

    let full = stitcher.full_image().expect("first frame stored");
    assert_eq!(full.dimensions(), (320, 320));

    let stats = stitcher.stats();
    assert_eq!(stats.frame_count, 1);
    assert_eq!(stats.total_height, 320);
    assert_eq!(stats.last_append, 320);
}
```

- [ ] **Step 3: Run the test and verify it fails**

Run: `rtk cargo test -p rollshot-core --test stitcher first_frame_initializes_stitched_image`

Expected: FAIL with a compile error such as `unresolved import rollshot_core::Stitcher` — the type does not exist yet.

- [ ] **Step 4: Implement the minimal `Stitcher` for the first-frame case**

Create `crates/rollshot-core/src/stitcher.rs`:

```rust
use image::RgbaImage;

use crate::duplicate;
use crate::types::{StitchConfig, StitchOutcome, StitchStats};

pub struct Stitcher {
    config: StitchConfig,
    full_image: Option<RgbaImage>,
    last_good_frame: Option<RgbaImage>,
    last_good_signature: Option<Vec<u8>>,
    last_offset: i32,
    stats: StitchStats,
}

impl Stitcher {
    pub fn new(config: StitchConfig) -> Self {
        Self {
            config,
            full_image: None,
            last_good_frame: None,
            last_good_signature: None,
            last_offset: 0,
            stats: StitchStats::default(),
        }
    }

    pub fn push_frame(&mut self, frame: RgbaImage) -> StitchOutcome {
        if self.full_image.is_none() {
            return self.accept_first_frame(frame);
        }

        let _ = &self.config;
        let _ = &self.last_offset;
        let _ = frame;
        StitchOutcome::NoMatch {
            confidence: f32::INFINITY,
        }
    }

    pub fn full_image(&self) -> Option<&RgbaImage> {
        self.full_image.as_ref()
    }

    pub fn stats(&self) -> StitchStats {
        self.stats
    }

    fn accept_first_frame(&mut self, frame: RgbaImage) -> StitchOutcome {
        let height = frame.height();
        self.stats = StitchStats {
            frame_count: 1,
            total_height: height,
            last_append: height,
        };
        self.last_good_signature = Some(duplicate::signature(&frame));
        self.last_good_frame = Some(frame.clone());
        self.full_image = Some(frame);
        StitchOutcome::FirstFrame
    }
}
```

The `let _ = ...` lines silence "field never read" / unused-variable lint noise for items that later tasks will start using. They are removed in Task 9 when the real `push_frame` body lands.

- [ ] **Step 5: Re-export `Stitcher` from `lib.rs`**

Replace `crates/rollshot-core/src/lib.rs` with:

```rust
mod duplicate;
mod image_ext;
mod matcher;
mod stitcher;
mod types;

pub use stitcher::Stitcher;
pub use types::{MatchAlgorithm, OffsetEstimate, StitchConfig, StitchOutcome, StitchStats};
```

- [ ] **Step 6: Run the test and verify it passes**

Run: `rtk cargo test -p rollshot-core --test stitcher first_frame_initializes_stitched_image`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/rollshot-core/src/stitcher.rs crates/rollshot-core/src/lib.rs crates/rollshot-core/tests/common/mod.rs crates/rollshot-core/tests/stitcher.rs
git commit -m "feat(core): introduce Stitcher with first-frame handling"
```

---

## Task 7: Stitcher Rejects Dimension Mismatches

**Files:**
- Modify: `crates/rollshot-core/src/stitcher.rs`
- Modify: `crates/rollshot-core/tests/stitcher.rs`

- [ ] **Step 1: Append the failing dimension-mismatch test**

Append to `crates/rollshot-core/tests/stitcher.rs`:

```rust
use image::{Rgba, RgbaImage};

#[test]
fn dimension_mismatch_returns_no_match() {
    let canvas = make_scroll_canvas(320, 1000);
    let first = crop_frame(&canvas, 0, 320);
    let wrong_size = RgbaImage::from_pixel(200, 320, Rgba([255, 255, 255, 255]));

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(wrong_size) {
        StitchOutcome::NoMatch { confidence } => {
            assert!(!confidence.is_finite(), "confidence = {confidence}");
        }
        other => panic!("expected NoMatch, got {other:?}"),
    }

    let stats = stitcher.stats();
    assert_eq!(stats.frame_count, 1);
    assert_eq!(stats.total_height, 320);
}
```

- [ ] **Step 2: Run the test and verify it fails**

Run: `rtk cargo test -p rollshot-core --test stitcher dimension_mismatch_returns_no_match`

Expected: FAIL. The placeholder `push_frame` returns `NoMatch` with infinite confidence, which actually satisfies this test, but the test will still fail to compile until the `Debug` derive on `StitchOutcome` is in scope. Confirm the failure mode is a compile error or a behavior gap before proceeding.

If it already passes by accident, that is acceptable — the next step still needs to be done.

- [ ] **Step 3: Replace the placeholder `push_frame` with explicit dimension handling**

Replace the `push_frame` method in `crates/rollshot-core/src/stitcher.rs` with:

```rust
    pub fn push_frame(&mut self, frame: RgbaImage) -> StitchOutcome {
        if self.full_image.is_none() {
            return self.accept_first_frame(frame);
        }

        let anchor = self
            .last_good_frame
            .as_ref()
            .expect("anchor present after first frame");

        if anchor.dimensions() != frame.dimensions() {
            return StitchOutcome::NoMatch {
                confidence: f32::INFINITY,
            };
        }

        let _ = &self.config;
        let _ = &self.last_offset;
        let _ = frame;
        StitchOutcome::NoMatch {
            confidence: f32::INFINITY,
        }
    }
```

- [ ] **Step 4: Run both tests and verify they pass**

Run: `rtk cargo test -p rollshot-core --test stitcher`

Expected: PASS. Two tests pass: `first_frame_initializes_stitched_image` and `dimension_mismatch_returns_no_match`.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/stitcher.rs crates/rollshot-core/tests/stitcher.rs
git commit -m "feat(core): reject dimension-mismatched frames as NoMatch"
```

---

## Task 8: Stitcher Detects Duplicate Frames

**Files:**
- Modify: `crates/rollshot-core/src/stitcher.rs`
- Modify: `crates/rollshot-core/tests/stitcher.rs`

- [ ] **Step 1: Append the failing duplicate test**

Append to `crates/rollshot-core/tests/stitcher.rs`:

```rust
#[test]
fn duplicate_frame_returns_duplicate_without_growing() {
    let canvas = make_scroll_canvas(320, 1000);
    let first = crop_frame(&canvas, 0, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first.clone()), StitchOutcome::FirstFrame);
    assert_eq!(stitcher.push_frame(first.clone()), StitchOutcome::Duplicate);

    let full = stitcher.full_image().expect("image stored");
    assert_eq!(full.dimensions(), (320, 320));
    assert_eq!(stitcher.stats().frame_count, 1);
}
```

- [ ] **Step 2: Run the test and verify it fails**

Run: `rtk cargo test -p rollshot-core --test stitcher duplicate_frame_returns_duplicate_without_growing`

Expected: FAIL — `assert_eq!` on `StitchOutcome::Duplicate` fails because the current placeholder returns `NoMatch`.

- [ ] **Step 3: Add duplicate detection in `push_frame`**

Replace the `push_frame` method in `crates/rollshot-core/src/stitcher.rs` with:

```rust
    pub fn push_frame(&mut self, frame: RgbaImage) -> StitchOutcome {
        if self.full_image.is_none() {
            return self.accept_first_frame(frame);
        }

        let anchor = self
            .last_good_frame
            .as_ref()
            .expect("anchor present after first frame");

        if anchor.dimensions() != frame.dimensions() {
            return StitchOutcome::NoMatch {
                confidence: f32::INFINITY,
            };
        }

        let signature = duplicate::signature(&frame);
        if let Some(prev_sig) = self.last_good_signature.as_ref() {
            if duplicate::is_duplicate(prev_sig, &signature, self.config.duplicate_threshold) {
                return StitchOutcome::Duplicate;
            }
        }

        let _ = signature;
        let _ = &self.last_offset;
        let _ = frame;
        StitchOutcome::NoMatch {
            confidence: f32::INFINITY,
        }
    }
```

- [ ] **Step 4: Run the duplicate test and verify it passes**

Run: `rtk cargo test -p rollshot-core --test stitcher duplicate_frame_returns_duplicate_without_growing`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/stitcher.rs crates/rollshot-core/tests/stitcher.rs
git commit -m "feat(core): detect duplicate frames before offset matching"
```

---

## Task 9: Stitcher Appends Normal Scroll and Reports NoProgress

**Files:**
- Modify: `crates/rollshot-core/src/stitcher.rs`
- Modify: `crates/rollshot-core/tests/stitcher.rs`

- [ ] **Step 1: Append the failing scroll + no-progress tests**

Append to `crates/rollshot-core/tests/stitcher.rs`:

```rust
#[test]
fn normal_scroll_appends_expected_pixels() {
    let canvas = make_scroll_canvas(320, 1200);
    let first = crop_frame(&canvas, 0, 320);
    let scrolled = crop_frame(&canvas, 80, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended { added } => {
            assert!((76..=84).contains(&added), "added = {added}");
        }
        other => panic!("expected Appended, got {other:?}"),
    }

    let full = stitcher.full_image().expect("stitched image");
    assert!(full.height() > 320);
    let stats = stitcher.stats();
    assert_eq!(stats.frame_count, 2);
    assert_eq!(stats.total_height, full.height());
}

#[test]
fn small_scroll_below_min_append_reports_no_progress() {
    let canvas = make_scroll_canvas(320, 1000);
    let first = crop_frame(&canvas, 0, 320);
    // 16 px is large enough to be clearly non-duplicate and clearly
    // matchable, but small enough to sit under the custom min_append below.
    let nudged = crop_frame(&canvas, 16, 320);

    let mut stitcher = Stitcher::new(StitchConfig {
        min_append: 64,
        ..StitchConfig::default()
    });
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);
    assert_eq!(stitcher.push_frame(nudged), StitchOutcome::NoProgress);

    let full = stitcher.full_image().expect("stitched image");
    assert_eq!(full.height(), 320);
}
```

- [ ] **Step 2: Run the tests and verify they fail**

Run: `rtk cargo test -p rollshot-core --test stitcher normal_scroll_appends_expected_pixels small_scroll_below_min_append_reports_no_progress`

Expected: FAIL — both tests panic with `expected Appended, got NoMatch { .. }` / `assert_eq` mismatch.

- [ ] **Step 3: Implement offset estimation, append, and progress checks**

First update the imports at the top of `crates/rollshot-core/src/stitcher.rs` to:

```rust
use image::RgbaImage;

use crate::duplicate;
use crate::image_ext::append_below;
use crate::matcher::estimate_offset;
use crate::types::{StitchConfig, StitchOutcome, StitchStats};
```

Then replace the `push_frame` method with:

```rust
    pub fn push_frame(&mut self, frame: RgbaImage) -> StitchOutcome {
        if self.full_image.is_none() {
            return self.accept_first_frame(frame);
        }

        let anchor = self
            .last_good_frame
            .as_ref()
            .expect("anchor present after first frame");

        if anchor.dimensions() != frame.dimensions() {
            return StitchOutcome::NoMatch {
                confidence: f32::INFINITY,
            };
        }

        let signature = duplicate::signature(&frame);
        if let Some(prev_sig) = self.last_good_signature.as_ref() {
            if duplicate::is_duplicate(prev_sig, &signature, self.config.duplicate_threshold) {
                return StitchOutcome::Duplicate;
            }
        }

        let estimate = estimate_offset(anchor, &frame, self.last_offset, &self.config);

        if estimate.confidence > self.config.accept_diff {
            return StitchOutcome::NoMatch {
                confidence: estimate.confidence,
            };
        }

        let dy = estimate.dy.max(0) as u32;
        if dy < self.config.min_append {
            return StitchOutcome::NoProgress;
        }

        let base = self
            .full_image
            .as_ref()
            .expect("full image present after first frame");
        let combined = append_below(base, &frame, dy);
        let total_height = combined.height();
        self.full_image = Some(combined);
        self.last_good_frame = Some(frame);
        self.last_good_signature = Some(signature);
        self.last_offset = estimate.dy;
        self.stats.frame_count += 1;
        self.stats.total_height = total_height;
        self.stats.last_append = dy;

        StitchOutcome::Appended { added: dy }
    }
```

- [ ] **Step 4: Run the full stitcher test file and verify success**

Run: `rtk cargo test -p rollshot-core --test stitcher`

Expected: PASS. Five tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-core/src/stitcher.rs crates/rollshot-core/tests/stitcher.rs
git commit -m "feat(core): append matched scroll content with min-append gate"
```

---

## Task 10: Bad Frames Do Not Poison the Anchor

**Files:**
- Modify: `crates/rollshot-core/tests/stitcher.rs`

- [ ] **Step 1: Append the failing anchor-preservation tests**

Append to `crates/rollshot-core/tests/stitcher.rs`:

```rust
#[test]
fn bad_frame_returns_no_match_and_preserves_anchor() {
    let canvas = make_scroll_canvas(320, 1200);
    let first = crop_frame(&canvas, 0, 320);
    let bad = RgbaImage::from_pixel(320, 320, Rgba([255, 255, 255, 255]));
    let recovered = crop_frame(&canvas, 96, 320);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(bad) {
        StitchOutcome::NoMatch { confidence } => {
            assert!(confidence > StitchConfig::default().accept_diff);
        }
        other => panic!("expected NoMatch on white frame, got {other:?}"),
    }

    let stats_after_bad = stitcher.stats();
    assert_eq!(stats_after_bad.frame_count, 1);
    assert_eq!(stats_after_bad.total_height, 320);

    match stitcher.push_frame(recovered) {
        StitchOutcome::Appended { added } => {
            assert!((92..=100).contains(&added), "added = {added}");
        }
        other => panic!("expected Appended after recovery, got {other:?}"),
    }

    let stats_after_recover = stitcher.stats();
    assert_eq!(stats_after_recover.frame_count, 2);
}
```

- [ ] **Step 2: Run the test and verify behavior**

Run: `rtk cargo test -p rollshot-core --test stitcher bad_frame_returns_no_match_and_preserves_anchor`

Expected: PASS. The current `push_frame` already preserves the anchor because it only updates `last_good_frame` / `last_good_signature` on a successful append. This test guards that invariant against future regressions; if it fails, the regression is in `push_frame`'s state updates.

If the test fails: ensure no code path overwrites `self.last_good_frame` or `self.last_good_signature` on the `NoMatch`, `NoProgress`, or `Duplicate` paths.

- [ ] **Step 3: Commit**

```bash
git add crates/rollshot-core/tests/stitcher.rs
git commit -m "test(core): pin Stitcher anchor preservation across bad frames"
```

---

## Task 11: Sticky Header Synthetic Frames Still Append

**Files:**
- Modify: `crates/rollshot-core/tests/stitcher.rs`

- [ ] **Step 1: Append the failing sticky-header test**

Append to `crates/rollshot-core/tests/stitcher.rs`:

```rust
use common::paint_sticky_header;

#[test]
fn sticky_header_frames_still_append_expected_amount() {
    let canvas = make_scroll_canvas(320, 1400);
    let mut first = crop_frame(&canvas, 0, 320);
    let mut scrolled = crop_frame(&canvas, 70, 320);

    paint_sticky_header(&mut first, 36);
    paint_sticky_header(&mut scrolled, 36);

    let mut stitcher = Stitcher::new(StitchConfig::default());
    assert_eq!(stitcher.push_frame(first), StitchOutcome::FirstFrame);

    match stitcher.push_frame(scrolled) {
        StitchOutcome::Appended { added } => {
            assert!((66..=74).contains(&added), "added = {added}");
        }
        other => panic!("expected Appended with sticky header, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run the test and verify it passes**

Run: `rtk cargo test -p rollshot-core --test stitcher sticky_header_frames_still_append_expected_amount`

Expected: PASS. The matcher's content ROI ignores the top 12% (~38px) plus a 24px minimum band, so the painted sticky header in rows 0..36 falls outside the template region and does not bias the offset estimate.

If the test fails: re-check the `TOP_IGNORE_RATIO` / `MIN_IGNORE_PX` constants in `matcher.rs` — they must guarantee the top band is excluded from the template.

- [ ] **Step 3: Run the full core test suite**

Run: `rtk cargo test -p rollshot-core`

Expected: PASS. All `lib.rs` module unit tests plus seven integration tests.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-core/tests/stitcher.rs
git commit -m "test(core): cover sticky-header frames in stitcher"
```

---

## Task 12: Wire `rollshot stitch-folder` to the Stitcher

**Files:**
- Modify: `crates/rollshot-cli/Cargo.toml`
- Modify: `crates/rollshot-cli/src/lib.rs`

- [ ] **Step 1: Depend on `image` from the CLI crate**

Replace `crates/rollshot-cli/Cargo.toml` with:

```toml
[package]
name = "rollshot-cli"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[[bin]]
name = "rollshot"
path = "src/main.rs"

[dependencies]
image = { workspace = true }
rollshot-capture = { path = "../rollshot-capture" }
rollshot-core = { path = "../rollshot-core" }

[lints]
workspace = true
```

- [ ] **Step 2: Replace the bootstrap-stub `stitch_folder` with the real command**

Replace the contents of `crates/rollshot-cli/src/lib.rs` with:

```rust
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use image::DynamicImage;
use rollshot_core::{StitchConfig, StitchOutcome, Stitcher};

pub fn run<I, S>(args: I) -> Result<String, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect::<Vec<_>>();

    match args.get(1).map(String::as_str) {
        None | Some("--help" | "-h") => Ok(help()),
        Some("--version" | "-V") => Ok(format!("rollshot {}\n", env!("CARGO_PKG_VERSION"))),
        Some("probe") => Ok(probe()),
        Some("stitch-folder") => stitch_folder(&args[2..]),
        Some(command) => Err(format!("unknown command: {command}\n\n{}", help())),
    }
}

fn help() -> String {
    String::from(
        "rollshot\n\
         \n\
         Usage:\n\
           rollshot probe\n\
           rollshot stitch-folder <frames-dir> --output <png>\n\
           rollshot --version\n",
    )
}

fn probe() -> String {
    format!(
        "rollshot {}\n\
         os: {}\n\
         real capture: unavailable in bootstrap phase\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
    )
}

fn stitch_folder(args: &[String]) -> Result<String, String> {
    let mut frames_dir: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--output" | "-o" => {
                let value = iter
                    .next()
                    .ok_or_else(|| String::from("--output requires a path argument"))?;
                output = Some(PathBuf::from(value));
            }
            other if frames_dir.is_none() => {
                frames_dir = Some(PathBuf::from(other));
            }
            other => {
                return Err(format!("unexpected argument: {other}"));
            }
        }
    }

    let frames_dir = frames_dir.ok_or_else(|| {
        String::from("usage: rollshot stitch-folder <frames-dir> --output <png>")
    })?;
    let output = output.ok_or_else(|| {
        String::from("usage: rollshot stitch-folder <frames-dir> --output <png>")
    })?;

    if !frames_dir.is_dir() {
        return Err(format!(
            "frames directory not found: {}",
            frames_dir.display()
        ));
    }

    let frame_paths = collect_frame_paths(&frames_dir)?;
    if frame_paths.is_empty() {
        return Err(format!(
            "no supported images in {} (expected .png/.jpg/.jpeg)",
            frames_dir.display()
        ));
    }

    let mut stitcher = Stitcher::new(StitchConfig::default());
    let mut appended = 0u32;
    let mut duplicates = 0u32;
    let mut no_match = 0u32;
    let mut no_progress = 0u32;

    for path in &frame_paths {
        let img = image::open(path)
            .map_err(|err| format!("failed to decode {}: {err}", path.display()))?;
        let frame = into_rgba(img);

        match stitcher.push_frame(frame) {
            StitchOutcome::FirstFrame => {}
            StitchOutcome::Appended { .. } => appended += 1,
            StitchOutcome::Duplicate => duplicates += 1,
            StitchOutcome::NoMatch { .. } => no_match += 1,
            StitchOutcome::NoProgress => no_progress += 1,
        }
    }

    let stitched = stitcher
        .full_image()
        .ok_or_else(|| String::from("no stitched output available"))?;
    stitched
        .save(&output)
        .map_err(|err| format!("failed to save {}: {err}", output.display()))?;

    Ok(format!(
        "stitch-folder: {dir}\n\
         input frames: {input}\n\
         appended: {appended}\n\
         duplicates: {duplicates}\n\
         no-progress: {no_progress}\n\
         no-match: {no_match}\n\
         output: {out} ({w}x{h})\n",
        dir = frames_dir.display(),
        input = frame_paths.len(),
        appended = appended,
        duplicates = duplicates,
        no_progress = no_progress,
        no_match = no_match,
        out = output.display(),
        w = stitched.width(),
        h = stitched.height(),
    ))
}

fn collect_frame_paths(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(dir)
        .map_err(|err| format!("failed to read {}: {err}", dir.display()))?;

    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|entry| entry.path())
        .filter(|path| {
            matches!(
                path.extension().and_then(OsStr::to_str).map(str::to_ascii_lowercase).as_deref(),
                Some("png" | "jpg" | "jpeg")
            )
        })
        .collect();

    paths.sort();
    Ok(paths)
}

fn into_rgba(image: DynamicImage) -> image::RgbaImage {
    match image {
        DynamicImage::ImageRgba8(rgba) => rgba,
        other => other.to_rgba8(),
    }
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn probe_reports_bootstrap_status() {
        let output = run(["rollshot", "probe"]).expect("probe should succeed");

        assert!(output.contains("rollshot"));
        assert!(output.contains("real capture: unavailable"));
    }

    #[test]
    fn stitch_folder_requires_arguments() {
        let err = run(["rollshot", "stitch-folder"]).expect_err("missing args should fail");

        assert!(err.contains("usage"), "err = {err}");
    }

    #[test]
    fn stitch_folder_rejects_missing_directory() {
        let err = run([
            "rollshot",
            "stitch-folder",
            "/tmp/this/path/does/not/exist-rollshot",
            "--output",
            "/tmp/should-never-write.png",
        ])
        .expect_err("missing dir should fail");

        assert!(err.contains("not found"), "err = {err}");
    }
}
```

- [ ] **Step 3: Run the lib-level CLI tests**

Run: `rtk cargo test -p rollshot-cli --lib`

Expected: PASS. Three tests pass: `probe_reports_bootstrap_status`, `stitch_folder_requires_arguments`, `stitch_folder_rejects_missing_directory`.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-cli/Cargo.toml crates/rollshot-cli/src/lib.rs Cargo.lock
git commit -m "feat(cli): implement stitch-folder backed by rollshot-core"
```

---

## Task 13: End-to-End CLI Smoke Test

**Files:**
- Modify: `crates/rollshot-cli/tests/cli_smoke.rs`

- [ ] **Step 1: Add `image` as a CLI dev-dependency**

Append to `crates/rollshot-cli/Cargo.toml`:

```toml

[dev-dependencies]
image = { workspace = true }
```

- [ ] **Step 2: Write the failing end-to-end smoke test**

Replace the contents of `crates/rollshot-cli/tests/cli_smoke.rs` with:

```rust
use std::path::PathBuf;
use std::process::Command;

use image::{imageops, Rgba, RgbaImage};

#[test]
fn rollshot_probe_binary_runs() {
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("probe")
        .output()
        .expect("run rollshot probe");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("rollshot"));
    assert!(stdout.contains("real capture: unavailable"));
}

#[test]
fn rollshot_stitch_folder_writes_png() {
    let tempdir = tempdir_for_test("rollshot-stitch-folder");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");

    let canvas = make_scroll_canvas(160, 600);
    for (idx, y) in [0u32, 40, 80, 120].iter().enumerate() {
        let frame = imageops::crop_imm(&canvas, 0, *y, canvas.width(), 160).to_image();
        let path = frames_dir.join(format!("frame_{:03}.png", idx));
        frame.save(&path).expect("save frame");
    }

    let output_png = tempdir.join("stitched.png");

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("stitch-folder")
        .arg(&frames_dir)
        .arg("--output")
        .arg(&output_png)
        .output()
        .expect("run rollshot stitch-folder");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("stitch-folder"), "stdout = {stdout}");
    assert!(stdout.contains("input frames: 4"), "stdout = {stdout}");
    assert!(stdout.contains("appended:"), "stdout = {stdout}");
    assert!(stdout.contains(output_png.to_string_lossy().as_ref()));

    assert!(output_png.exists(), "{} should exist", output_png.display());
    let stitched = image::open(&output_png).expect("decode stitched png").to_rgba8();
    assert_eq!(stitched.width(), 160);
    assert!(stitched.height() > 160, "height = {}", stitched.height());

    let _ = std::fs::remove_dir_all(&tempdir);
}

fn tempdir_for_test(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("{label}-{nanos}-{}", std::process::id()));
    std::fs::create_dir_all(&path).expect("create temp dir");
    path
}

fn make_scroll_canvas(width: u32, height: u32) -> RgbaImage {
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

    for col in [21u32, 47, 73, 99, 125] {
        if col >= width {
            continue;
        }
        for y in 12..height.saturating_sub(12) {
            if (y / 13) % 3 != 0 {
                img.put_pixel(col, y, Rgba([20, 20, 20, 255]));
            }
        }
    }

    img
}
```

- [ ] **Step 3: Run the smoke test**

Run: `rtk cargo test -p rollshot-cli --test cli_smoke`

Expected: PASS. Two tests pass: `rollshot_probe_binary_runs` and `rollshot_stitch_folder_writes_png`. The second test writes 4 PNG frames to a temp directory, invokes the binary, and asserts that the stitched PNG exists with a height greater than the source frame height.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-cli/Cargo.toml crates/rollshot-cli/tests/cli_smoke.rs Cargo.lock
git commit -m "test(cli): exercise stitch-folder end-to-end on synthetic frames"
```

---

## Task 14: Workspace Verification

**Files:** (no source changes — verification only)

- [ ] **Step 1: Format check**

Run: `rtk cargo fmt --all -- --check`

Expected: PASS with no diff.

If it fails: run `rtk cargo fmt --all`, re-run the check, and amend the most recent commit only if it was your own and not yet pushed. Otherwise create a fixup commit `style: cargo fmt`.

- [ ] **Step 2: Clippy with warnings as errors**

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS with no warnings. Common issues to watch for:
- Unused imports introduced by partial implementations earlier in the plan.
- `clippy::redundant_clone` on the synthetic fixtures (acceptable if necessary for test correctness — silence with a local `#[allow]` rather than removing the clone if removing changes behavior).

- [ ] **Step 3: Full workspace test run**

Run: `rtk cargo test --workspace`

Expected: PASS. Every test added or carried forward in this plan succeeds. Expected counts:
- `rollshot-core` lib unit tests: 13 (types: 2, image_ext: 3, duplicate: 4, matcher: 4).
- `rollshot-core` integration test (`stitcher`): 7.
- `rollshot-cli` lib tests: 3.
- `rollshot-cli` integration test (`cli_smoke`): 2.
- `rollshot-capture` lib tests: 1 (pre-existing fake-stream test).

Treat the exact totals as informational rather than load-bearing — what matters is that no test fails.

- [ ] **Step 4: Commit any verification-only fixes (if any)**

```bash
git status
```

If `git status` is clean: skip the commit. Otherwise:

```bash
git add -A
git commit -m "style: workspace fmt/clippy cleanup"
```

---

## Spec Coverage Check

| Spec requirement | Covered by |
|------------------|------------|
| `rollshot-core` operates on `image::RgbaImage` | Tasks 3, 5, 6 |
| `Stitcher` API reusable by future capture backends | Task 6 |
| Duplicate detection | Task 4 (unit), Task 8 (integration) |
| Template-based vertical offset matching with content ROI | Task 5 |
| Preserve last good anchor after a bad frame | Tasks 9, 10 |
| Synthetic fixtures for normal scroll, duplicates, small scrolls, sticky headers, bad frames | Tasks 6, 8, 9, 10, 11 |
| `rollshot stitch-folder <frames-dir> --output <png>` writes a real PNG | Task 12, validated in Task 13 |
| Re-exported public API (`Stitcher`, `StitchConfig`, `StitchStats`, `MatchAlgorithm`, `StitchOutcome`, `OffsetEstimate`) | Tasks 2, 6 |
| First-frame initialization | Task 6 |
| Dimension mismatch → `NoMatch` | Task 7 |
| `confidence > accept_diff` → `NoMatch` | Task 9 |
| `dy < min_append` → `NoProgress` | Task 9 |
| Append bottom `dy` pixels | Tasks 3, 9 |
| Update last-good frame only after successful append | Task 9 (state updates inside the `Appended` branch) |
| Frame signatures + MAD duplicate detection | Task 4 |
| Equal-dim, top/bottom/side ROI, NCC, second-best margin, overlap MAD | Task 5 |
| Errors: missing dir, no supported images, decode failure, no stitched output, save failure | Task 12 (`stitch_folder` error branches) |
| Remove "not available in bootstrap phase" stitch-folder response | Task 12 |
| Workspace verification: fmt, clippy, test | Task 14 |
| No OpenCV / hora / imageproc / rayon / PipeWire / DBus / scap / OBS | Tasks 1, 12 (only `image` added) |
