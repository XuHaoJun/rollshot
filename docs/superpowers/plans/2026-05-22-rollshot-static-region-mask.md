# Rollshot v0.2.1 Static Region Mask Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the visual duplication of sticky / fixed UI (header, footer, sidebars) in stitched output by detecting static edge bands and filling them with a sampled background color during canvas append.

**Architecture:** New `static_region.rs` module owns detection and the public `StaticMask` / `StickyBand` / `StaticRegionConfig` types. `LinearCanvas::append` gains an `Option<&StaticMask>` parameter and applies the mask to its slice before pasting. `Stitcher` holds a `pub(crate) StaticRegionDetector`, calls `observe` only on verified motion, and threads `detector.mask()` into canvas append. Detector locks after `min_observations` frames using channel-wise median over per-edge band measurements.

**Tech Stack:** Rust 2021, `image` crate (RgbaImage), existing rollshot-core matcher / verifier / canvas modules. No new external dependencies.

**Reference spec:** `docs/superpowers/specs/2026-05-22-rollshot-static-region-mask-design.md`

---

## File Map

```text
Create:
  crates/rollshot-core/src/static_region.rs

Modify:
  crates/rollshot-core/src/lib.rs               (mod + pub use)
  crates/rollshot-core/src/types.rs             (StitchConfig field)
  crates/rollshot-core/src/canvas.rs            (append signature + apply_static_mask)
  crates/rollshot-core/src/stitcher.rs          (detector field + observe + mask)
  crates/rollshot-core/tests/common/mod.rs      (paint_sticky_* helpers; only if it exists today, otherwise creates module)
  crates/rollshot-core/tests/stitcher.rs        (only call-site updates if signature breaks; see Task 3)

Create:
  crates/rollshot-core/tests/static_region.rs   (integration tests)
```

---

## Task 1: Public types — `StickyBand`, `StaticMask`, `StaticRegionConfig`

**Files:**
- Create: `crates/rollshot-core/src/static_region.rs`
- Modify: `crates/rollshot-core/src/lib.rs`

- [ ] **Step 1: Write the failing test** — append to a new `static_region.rs` file along with the module skeleton.

```rust
// crates/rollshot-core/src/static_region.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StickyBand {
    pub thickness: u32,
    pub bg_color: [u8; 4],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StaticMask {
    pub top: Option<StickyBand>,
    pub bottom: Option<StickyBand>,
    pub left: Option<StickyBand>,
    pub right: Option<StickyBand>,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct StaticRegionConfig {
    pub enabled: bool,
    pub min_observations: usize,
    pub static_mad_threshold: f32,
    pub motion_margin: f32,
    pub max_band_ratio: f32,
}

impl Default for StaticRegionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_observations: 3,
            static_mad_threshold: 4.0 / 255.0,
            motion_margin: 4.0 / 255.0,
            max_band_ratio: 0.30,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_mask_default_is_all_none() {
        let mask = StaticMask::default();
        assert!(mask.top.is_none());
        assert!(mask.bottom.is_none());
        assert!(mask.left.is_none());
        assert!(mask.right.is_none());
    }

    #[test]
    fn static_region_config_default_values() {
        let cfg = StaticRegionConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.min_observations, 3);
        assert!((cfg.static_mad_threshold - 4.0 / 255.0).abs() < 1e-9);
        assert!((cfg.motion_margin - 4.0 / 255.0).abs() < 1e-9);
        assert!((cfg.max_band_ratio - 0.30).abs() < 1e-9);
    }
}
```

- [ ] **Step 2: Wire the module into `lib.rs`**

Modify `crates/rollshot-core/src/lib.rs`. Add `mod static_region;` alongside the other module declarations, and append `StaticMask, StaticRegionConfig, StickyBand` to the existing `pub use` block:

```rust
mod akaze_matcher;
mod axis;
mod canvas;
mod duplicate;
mod matcher;
mod overlap;
mod static_region;        // NEW
mod stitcher;
mod types;
mod verifier;

pub use canvas::{CanvasAppendError, LinearCanvas};
pub use static_region::{StaticMask, StaticRegionConfig, StickyBand};   // NEW
pub use stitcher::Stitcher;
pub use types::{
    AkazeConfig, AppendDirection, MatchMethod, MatchStrategy, MotionCandidate, MotionEstimate,
    NoMatchReason, OverlapRegion, ScrollAxis, StitchConfig, StitchOutcome, StitchStats,
    VerifierConfig,
};
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-core --lib static_region::tests`
Expected: PASS (2 tests).

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-core/src/static_region.rs crates/rollshot-core/src/lib.rs
rtk git commit -m "$(cat <<'EOF'
feat(core): add static_region module with public types

Introduce StickyBand, StaticMask, and StaticRegionConfig with
defaults. Detector struct and detection logic will follow in
subsequent commits.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Wire `StaticRegionConfig` into `StitchConfig`

**Files:**
- Modify: `crates/rollshot-core/src/types.rs`

- [ ] **Step 1: Write the failing test** — append to the `tests` mod inside `types.rs`.

```rust
#[test]
fn default_stitch_config_enables_static_region() {
    let cfg = StitchConfig::default();
    assert!(cfg.static_region.enabled);
    assert_eq!(cfg.static_region.min_observations, 3);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-core --lib types::tests::default_stitch_config_enables_static_region -- --exact`
Expected: FAIL — `cfg.static_region` is unknown field.

- [ ] **Step 3: Add the field to `StitchConfig` and its default**

Modify `crates/rollshot-core/src/types.rs`. Add `use crate::static_region::StaticRegionConfig;` at the top imports (or `use crate::StaticRegionConfig` — match the existing import style; if `types.rs` has no other `use crate::` imports, add it just below the public `use`s).

Then edit the `StitchConfig` struct and its `Default`:

```rust
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct StitchConfig {
    pub strategy: MatchStrategy,
    pub min_overlap: u32,
    pub min_append: u32,
    pub duplicate_threshold: f32,
    pub accept_confidence: f32,
    pub axis_ratio_threshold: f32,
    pub max_cross_axis_px: i32,
    pub second_best_margin: f32,
    pub max_search_ratio: f32,
    pub match_width: u32,
    pub akaze: AkazeConfig,
    pub verifier: VerifierConfig,
    pub static_region: StaticRegionConfig,   // NEW
}

impl Default for StitchConfig {
    fn default() -> Self {
        Self {
            strategy: MatchStrategy::AutoHybrid,
            min_overlap: 64,
            min_append: 8,
            duplicate_threshold: 0.01,
            accept_confidence: 0.15,
            axis_ratio_threshold: 1.5,
            max_cross_axis_px: 6,
            second_best_margin: 0.001,
            max_search_ratio: 0.4,
            match_width: 512,
            akaze: AkazeConfig::default(),
            verifier: VerifierConfig::default(),
            static_region: StaticRegionConfig::default(),   // NEW
        }
    }
}
```

If `types.rs` does not yet import from `crate::static_region`, add this line near the other module imports at the top:

```rust
use crate::static_region::StaticRegionConfig;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `rtk cargo test -p rollshot-core --lib types::tests::default_stitch_config_enables_static_region -- --exact`
Expected: PASS.

- [ ] **Step 5: Run the whole library test suite to verify no regressions**

Run: `rtk cargo test -p rollshot-core --lib`
Expected: all existing tests still PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-core/src/types.rs
rtk git commit -m "$(cat <<'EOF'
feat(core): wire StaticRegionConfig into StitchConfig

Default has enabled=true so the mask becomes active as soon as
the rest of v0.2.1 lands; callers can opt out by setting
static_region.enabled = false.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: `LinearCanvas::append` gains `Option<&StaticMask>` (plumbing only)

This task changes the signature without changing behavior. All existing tests must still pass after we mechanically pass `None` at every call site.

**Files:**
- Modify: `crates/rollshot-core/src/canvas.rs`
- Modify: `crates/rollshot-core/src/stitcher.rs`

- [ ] **Step 1: Update `LinearCanvas::append` and its four helpers**

In `crates/rollshot-core/src/canvas.rs`, at the top imports add:

```rust
use crate::static_region::StaticMask;
```

Then change the public `append` signature:

```rust
pub fn append(
    &mut self,
    direction: AppendDirection,
    frame: &RgbaImage,
    slice_px: u32,
    mask: Option<&StaticMask>,
) -> Result<u32, CanvasAppendError> {
    let target_axis = direction.axis();
    if let Some(locked) = self.axis {
        if locked != target_axis {
            return Err(CanvasAppendError::AxisMismatch {
                locked,
                attempted: target_axis,
            });
        }
    }

    match target_axis {
        ScrollAxis::Vertical => {
            if frame.width() != self.image.width() {
                return Err(CanvasAppendError::DimensionMismatch {
                    canvas: self.image.width(),
                    frame: frame.width(),
                });
            }
        }
        ScrollAxis::Horizontal => {
            if frame.height() != self.image.height() {
                return Err(CanvasAppendError::DimensionMismatch {
                    canvas: self.image.height(),
                    frame: frame.height(),
                });
            }
        }
    }

    if slice_px == 0 {
        return Err(CanvasAppendError::EmptyAppend);
    }

    let added = match direction {
        AppendDirection::Bottom => self.append_bottom(frame, slice_px, mask),
        AppendDirection::Top => self.prepend_top(frame, slice_px, mask),
        AppendDirection::Right => self.append_right(frame, slice_px, mask),
        AppendDirection::Left => self.prepend_left(frame, slice_px, mask),
    };

    self.axis = Some(target_axis);
    Ok(added)
}
```

Update each of the four private helpers to take `mask: Option<&StaticMask>` (unused for now — name it `_mask` to silence warnings or just accept it):

```rust
fn append_bottom(&mut self, frame: &RgbaImage, slice_px: u32, _mask: Option<&StaticMask>) -> u32 {
    let slice_px = slice_px.min(frame.height());
    let overlap = frame.height() - slice_px;
    let slice = frame.view(0, overlap, frame.width(), slice_px).to_image();
    let mut combined = RgbaImage::new(self.image.width(), self.image.height() + slice_px);
    combined.copy_from(&self.image, 0, 0).expect("copy base");
    combined
        .copy_from(&slice, 0, self.image.height())
        .expect("copy slice");
    self.image = combined;
    slice_px
}

fn prepend_top(&mut self, frame: &RgbaImage, slice_px: u32, _mask: Option<&StaticMask>) -> u32 {
    let slice_px = slice_px.min(frame.height());
    let slice = frame.view(0, 0, frame.width(), slice_px).to_image();
    let mut combined = RgbaImage::new(self.image.width(), self.image.height() + slice_px);
    combined.copy_from(&slice, 0, 0).expect("copy slice");
    combined
        .copy_from(&self.image, 0, slice_px)
        .expect("copy base");
    self.image = combined;
    slice_px
}

fn append_right(&mut self, frame: &RgbaImage, slice_px: u32, _mask: Option<&StaticMask>) -> u32 {
    let slice_px = slice_px.min(frame.width());
    let overlap = frame.width() - slice_px;
    let slice = frame.view(overlap, 0, slice_px, frame.height()).to_image();
    let mut combined = RgbaImage::new(self.image.width() + slice_px, self.image.height());
    combined.copy_from(&self.image, 0, 0).expect("copy base");
    combined
        .copy_from(&slice, self.image.width(), 0)
        .expect("copy slice");
    self.image = combined;
    slice_px
}

fn prepend_left(&mut self, frame: &RgbaImage, slice_px: u32, _mask: Option<&StaticMask>) -> u32 {
    let slice_px = slice_px.min(frame.width());
    let slice = frame.view(0, 0, slice_px, frame.height()).to_image();
    let mut combined = RgbaImage::new(self.image.width() + slice_px, self.image.height());
    combined.copy_from(&slice, 0, 0).expect("copy slice");
    combined
        .copy_from(&self.image, slice_px, 0)
        .expect("copy base");
    self.image = combined;
    slice_px
}
```

- [ ] **Step 2: Update all canvas.rs unit-test call sites to pass `None`**

In `crates/rollshot-core/src/canvas.rs` `#[cfg(test)] mod tests`, every call to `canvas.append(direction, &frame, slice_px)` becomes `canvas.append(direction, &frame, slice_px, None)`. The complete list of existing test names (do not skip any):

- `append_bottom_adds_slice_below`
- `prepend_top_adds_slice_above`
- `append_right_adds_slice_to_the_right`
- `prepend_left_adds_slice_to_the_left`
- `axis_lock_rejects_perpendicular_direction`
- `dimension_mismatch_is_reported`
- `dimension_mismatch_in_horizontal_mode_is_reported`
- `zero_slice_px_is_rejected`
- `slice_larger_than_frame_is_clamped_to_frame_size`

For each, append `, None` as the fourth argument.

- [ ] **Step 3: Update the single external caller in `stitcher.rs`**

In `crates/rollshot-core/src/stitcher.rs`, find the call to `canvas.append(direction, &frame, slice_px)` (around line 177) and change it to:

```rust
let added = match canvas.append(direction, &frame, slice_px, None) {
```

- [ ] **Step 4: Run the whole library test suite to verify nothing regressed**

Run: `rtk cargo test -p rollshot-core`
Expected: all tests PASS (canvas unit tests, stitcher integration tests, golden fixtures).

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-core/src/canvas.rs crates/rollshot-core/src/stitcher.rs
rtk git commit -m "$(cat <<'EOF'
refactor(core): thread Option<&StaticMask> through LinearCanvas::append

Signature-only change. The mask is unused for now (named _mask in
the four helpers). All existing call sites pass None and behavior
is byte-identical to before. The next commit wires in the actual
mask-application logic.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: `apply_static_mask` helper + use in all four append directions

**Files:**
- Modify: `crates/rollshot-core/src/canvas.rs`

- [ ] **Step 1: Write failing tests inside `canvas.rs` tests module**

Append these tests to the existing `#[cfg(test)] mod tests` block:

```rust
fn band(thickness: u32, color: [u8; 4]) -> StickyBand {
    StickyBand { thickness, bg_color: color }
}

fn left_only(thickness: u32, color: [u8; 4]) -> StaticMask {
    StaticMask { left: Some(band(thickness, color)), ..StaticMask::default() }
}

fn right_only(thickness: u32, color: [u8; 4]) -> StaticMask {
    StaticMask { right: Some(band(thickness, color)), ..StaticMask::default() }
}

fn top_only(thickness: u32, color: [u8; 4]) -> StaticMask {
    StaticMask { top: Some(band(thickness, color)), ..StaticMask::default() }
}

fn bottom_only(thickness: u32, color: [u8; 4]) -> StaticMask {
    StaticMask { bottom: Some(band(thickness, color)), ..StaticMask::default() }
}

#[test]
fn append_bottom_with_left_mask_fills_left_columns() {
    let base = solid(8, 4, [10, 10, 10, 255]);
    let frame = solid(8, 4, [200, 0, 0, 255]);
    let mut canvas = LinearCanvas::new(base);
    let mask = left_only(2, [50, 60, 70, 255]);
    canvas.append(AppendDirection::Bottom, &frame, 2, Some(&mask)).unwrap();
    // appended slice rows 4..6 of canvas; left two columns are bg, rest are frame red.
    assert_eq!(canvas.image().get_pixel(0, 4), &Rgba([50, 60, 70, 255]));
    assert_eq!(canvas.image().get_pixel(1, 4), &Rgba([50, 60, 70, 255]));
    assert_eq!(canvas.image().get_pixel(2, 4), &Rgba([200, 0, 0, 255]));
    assert_eq!(canvas.image().get_pixel(7, 5), &Rgba([200, 0, 0, 255]));
    // first frame untouched.
    assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([10, 10, 10, 255]));
}

#[test]
fn append_bottom_with_right_mask_fills_right_columns() {
    let base = solid(8, 4, [10, 10, 10, 255]);
    let frame = solid(8, 4, [200, 0, 0, 255]);
    let mut canvas = LinearCanvas::new(base);
    let mask = right_only(2, [50, 60, 70, 255]);
    canvas.append(AppendDirection::Bottom, &frame, 2, Some(&mask)).unwrap();
    assert_eq!(canvas.image().get_pixel(7, 4), &Rgba([50, 60, 70, 255]));
    assert_eq!(canvas.image().get_pixel(6, 4), &Rgba([50, 60, 70, 255]));
    assert_eq!(canvas.image().get_pixel(5, 4), &Rgba([200, 0, 0, 255]));
}

#[test]
fn append_bottom_with_bottom_mask_fills_bottom_rows_of_slice() {
    let base = solid(4, 4, [10, 10, 10, 255]);
    let frame = solid(4, 4, [200, 0, 0, 255]);
    let mut canvas = LinearCanvas::new(base);
    // Bottom band thickness = 2, slice_px = 3 -> last 2 rows of slice = bg.
    let mask = bottom_only(2, [50, 60, 70, 255]);
    canvas.append(AppendDirection::Bottom, &frame, 3, Some(&mask)).unwrap();
    // canvas now has 7 rows; rows 4 (frame y=1) red, rows 5..7 (frame y=2..4) bg.
    assert_eq!(canvas.image().get_pixel(0, 4), &Rgba([200, 0, 0, 255]));
    assert_eq!(canvas.image().get_pixel(0, 5), &Rgba([50, 60, 70, 255]));
    assert_eq!(canvas.image().get_pixel(0, 6), &Rgba([50, 60, 70, 255]));
}

#[test]
fn prepend_top_with_top_mask_fills_top_rows_of_slice() {
    let base = solid(4, 4, [10, 10, 10, 255]);
    let frame = solid(4, 4, [200, 0, 0, 255]);
    let mut canvas = LinearCanvas::new(base);
    let mask = top_only(2, [50, 60, 70, 255]);
    canvas.append(AppendDirection::Top, &frame, 3, Some(&mask)).unwrap();
    // canvas now has 7 rows. Slice took frame y=0..3 and was prepended.
    // First two slice rows (frame y=0..1) are bg; third (frame y=2) is red.
    assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([50, 60, 70, 255]));
    assert_eq!(canvas.image().get_pixel(0, 1), &Rgba([50, 60, 70, 255]));
    assert_eq!(canvas.image().get_pixel(0, 2), &Rgba([200, 0, 0, 255]));
}

#[test]
fn append_right_with_right_mask_fills_right_columns_of_slice() {
    let base = solid(4, 4, [10, 10, 10, 255]);
    let frame = solid(4, 4, [200, 0, 0, 255]);
    let mut canvas = LinearCanvas::new(base);
    let mask = right_only(2, [50, 60, 70, 255]);
    canvas.append(AppendDirection::Right, &frame, 3, Some(&mask)).unwrap();
    // canvas now 7 cols wide. Slice took frame x=1..4. Slice cols frame x=2..3 are bg.
    assert_eq!(canvas.image().get_pixel(4, 0), &Rgba([200, 0, 0, 255]));
    assert_eq!(canvas.image().get_pixel(5, 0), &Rgba([50, 60, 70, 255]));
    assert_eq!(canvas.image().get_pixel(6, 0), &Rgba([50, 60, 70, 255]));
}

#[test]
fn prepend_left_with_left_mask_fills_left_columns_of_slice() {
    let base = solid(4, 4, [10, 10, 10, 255]);
    let frame = solid(4, 4, [200, 0, 0, 255]);
    let mut canvas = LinearCanvas::new(base);
    let mask = left_only(2, [50, 60, 70, 255]);
    canvas.append(AppendDirection::Left, &frame, 3, Some(&mask)).unwrap();
    // Slice took frame x=0..3; leftmost two cols are bg.
    assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([50, 60, 70, 255]));
    assert_eq!(canvas.image().get_pixel(1, 0), &Rgba([50, 60, 70, 255]));
    assert_eq!(canvas.image().get_pixel(2, 0), &Rgba([200, 0, 0, 255]));
}

#[test]
fn top_band_overrides_left_band_at_corner() {
    let base = solid(4, 4, [10, 10, 10, 255]);
    let frame = solid(4, 4, [200, 0, 0, 255]);
    let mut canvas = LinearCanvas::new(base);
    // Top band thickness 1 with color A, left band thickness 1 with color B.
    let mask = StaticMask {
        top: Some(band(1, [1, 2, 3, 255])),
        left: Some(band(1, [9, 9, 9, 255])),
        ..StaticMask::default()
    };
    canvas.append(AppendDirection::Top, &frame, 2, Some(&mask)).unwrap();
    // canvas row 0 is frame y=0 -> top band -> color A everywhere.
    assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([1, 2, 3, 255]));
    assert_eq!(canvas.image().get_pixel(3, 0), &Rgba([1, 2, 3, 255]));
    // canvas row 1 is frame y=1 -> not top band; col 0 is left band -> color B.
    assert_eq!(canvas.image().get_pixel(0, 1), &Rgba([9, 9, 9, 255]));
    assert_eq!(canvas.image().get_pixel(1, 1), &Rgba([200, 0, 0, 255]));
}
```

You also need to add the necessary imports at the top of the `tests` module:

```rust
use crate::static_region::{StaticMask, StickyBand};
```

- [ ] **Step 2: Run the new tests and confirm they fail**

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::append_bottom_with_left_mask_fills_left_columns -- --exact`
Expected: FAIL (mask currently ignored; left columns remain frame red).

- [ ] **Step 3: Implement `apply_static_mask` and call it from each direction**

In `crates/rollshot-core/src/canvas.rs`, near the imports, ensure these are in scope:

```rust
use image::{GenericImage, GenericImageView, Rgba, RgbaImage};
```

Add a free helper function (place it above the `LinearCanvas` impl block):

```rust
fn apply_static_mask(
    slice: &mut RgbaImage,
    frame_w: u32,
    frame_h: u32,
    slice_origin_in_frame: (u32, u32),
    mask: &StaticMask,
) {
    let (off_x, off_y) = slice_origin_in_frame;
    for sy in 0..slice.height() {
        for sx in 0..slice.width() {
            let fx = sx + off_x;
            let fy = sy + off_y;

            // top > bottom > left > right precedence.
            let fill = if let Some(b) = mask.top.filter(|b| fy < b.thickness) {
                Some(b.bg_color)
            } else if let Some(b) = mask
                .bottom
                .filter(|b| fy + b.thickness >= frame_h && b.thickness <= frame_h)
            {
                Some(b.bg_color)
            } else if let Some(b) = mask.left.filter(|b| fx < b.thickness) {
                Some(b.bg_color)
            } else if let Some(b) = mask
                .right
                .filter(|b| fx + b.thickness >= frame_w && b.thickness <= frame_w)
            {
                Some(b.bg_color)
            } else {
                None
            };

            if let Some(color) = fill {
                slice.put_pixel(sx, sy, Rgba(color));
            }
        }
    }
}
```

Update each direction to call it. Replace each of the four helpers with:

```rust
fn append_bottom(&mut self, frame: &RgbaImage, slice_px: u32, mask: Option<&StaticMask>) -> u32 {
    let slice_px = slice_px.min(frame.height());
    let overlap = frame.height() - slice_px;
    let mut slice = frame.view(0, overlap, frame.width(), slice_px).to_image();
    if let Some(mask) = mask {
        apply_static_mask(&mut slice, frame.width(), frame.height(), (0, overlap), mask);
    }
    let mut combined = RgbaImage::new(self.image.width(), self.image.height() + slice_px);
    combined.copy_from(&self.image, 0, 0).expect("copy base");
    combined
        .copy_from(&slice, 0, self.image.height())
        .expect("copy slice");
    self.image = combined;
    slice_px
}

fn prepend_top(&mut self, frame: &RgbaImage, slice_px: u32, mask: Option<&StaticMask>) -> u32 {
    let slice_px = slice_px.min(frame.height());
    let mut slice = frame.view(0, 0, frame.width(), slice_px).to_image();
    if let Some(mask) = mask {
        apply_static_mask(&mut slice, frame.width(), frame.height(), (0, 0), mask);
    }
    let mut combined = RgbaImage::new(self.image.width(), self.image.height() + slice_px);
    combined.copy_from(&slice, 0, 0).expect("copy slice");
    combined
        .copy_from(&self.image, 0, slice_px)
        .expect("copy base");
    self.image = combined;
    slice_px
}

fn append_right(&mut self, frame: &RgbaImage, slice_px: u32, mask: Option<&StaticMask>) -> u32 {
    let slice_px = slice_px.min(frame.width());
    let overlap = frame.width() - slice_px;
    let mut slice = frame.view(overlap, 0, slice_px, frame.height()).to_image();
    if let Some(mask) = mask {
        apply_static_mask(&mut slice, frame.width(), frame.height(), (overlap, 0), mask);
    }
    let mut combined = RgbaImage::new(self.image.width() + slice_px, self.image.height());
    combined.copy_from(&self.image, 0, 0).expect("copy base");
    combined
        .copy_from(&slice, self.image.width(), 0)
        .expect("copy slice");
    self.image = combined;
    slice_px
}

fn prepend_left(&mut self, frame: &RgbaImage, slice_px: u32, mask: Option<&StaticMask>) -> u32 {
    let slice_px = slice_px.min(frame.width());
    let mut slice = frame.view(0, 0, slice_px, frame.height()).to_image();
    if let Some(mask) = mask {
        apply_static_mask(&mut slice, frame.width(), frame.height(), (0, 0), mask);
    }
    let mut combined = RgbaImage::new(self.image.width() + slice_px, self.image.height());
    combined.copy_from(&slice, 0, 0).expect("copy slice");
    combined
        .copy_from(&self.image, slice_px, 0)
        .expect("copy base");
    self.image = combined;
    slice_px
}
```

- [ ] **Step 4: Run all canvas tests to verify they pass**

Run: `rtk cargo test -p rollshot-core --lib canvas::tests`
Expected: all original tests still PASS, plus the seven new mask tests PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-core/src/canvas.rs
rtk git commit -m "$(cat <<'EOF'
feat(core): apply StaticMask in LinearCanvas append

Each append direction now rewrites slice pixels per the mask:
top/bottom precedence over left/right at corners. With mask=None
behavior is unchanged.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: `StaticRegionDetector` skeleton

State: stores raw per-observation extents and colors, returns no mask until `min_observations` reached. No actual detection yet (Task 6–8 fill the algorithm; Task 9 wires it together).

**Files:**
- Modify: `crates/rollshot-core/src/static_region.rs`

- [ ] **Step 1: Write the failing tests**

Append to `static_region.rs`:

```rust
use image::RgbaImage;

#[derive(Debug, Clone, Copy)]
struct BandObs {
    thickness: u32,
    color: [u8; 4],
}

#[derive(Debug, Clone, Copy)]
struct EdgeObservation {
    top: BandObs,
    bottom: BandObs,
    left: BandObs,
    right: BandObs,
}

pub(crate) struct StaticRegionDetector {
    config: StaticRegionConfig,
    observations: Vec<EdgeObservation>,
    locked: Option<StaticMask>,
}

impl StaticRegionDetector {
    pub(crate) fn new(config: StaticRegionConfig) -> Self {
        Self {
            config,
            observations: Vec::new(),
            locked: None,
        }
    }

    pub(crate) fn observe(
        &mut self,
        _prev: &RgbaImage,
        _curr: &RgbaImage,
        _dx: i32,
        _dy: i32,
    ) {
        // Real implementation lands in Task 9. For now: bump observation
        // count with a zero observation so other tests can sequence calls.
        if self.locked.is_some() {
            return;
        }
        self.observations.push(EdgeObservation {
            top:    BandObs { thickness: 0, color: [0, 0, 0, 0] },
            bottom: BandObs { thickness: 0, color: [0, 0, 0, 0] },
            left:   BandObs { thickness: 0, color: [0, 0, 0, 0] },
            right:  BandObs { thickness: 0, color: [0, 0, 0, 0] },
        });
    }

    pub(crate) fn mask(&self) -> Option<&StaticMask> {
        self.locked.as_ref()
    }
}

#[cfg(test)]
mod detector_tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn black(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 255]))
    }

    #[test]
    fn detector_returns_none_before_any_observation() {
        let d = StaticRegionDetector::new(StaticRegionConfig::default());
        assert!(d.mask().is_none());
    }

    #[test]
    fn detector_returns_none_below_min_observations() {
        let cfg = StaticRegionConfig { min_observations: 3, ..StaticRegionConfig::default() };
        let mut d = StaticRegionDetector::new(cfg);
        let prev = black(4, 4);
        let curr = black(4, 4);
        d.observe(&prev, &curr, 0, 1);
        d.observe(&prev, &curr, 0, 1);
        assert!(d.mask().is_none(), "must not lock with fewer than min_observations");
    }
}
```

- [ ] **Step 2: Run new tests to verify they pass**

Run: `rtk cargo test -p rollshot-core --lib static_region::detector_tests`
Expected: both PASS.

- [ ] **Step 3: Commit**

```bash
rtk git add crates/rollshot-core/src/static_region.rs
rtk git commit -m "$(cat <<'EOF'
feat(core): add StaticRegionDetector skeleton

Detection state plumbing only. Returns None until the algorithm
in Task 6-9 fills in observe() and locks the mask.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Per-row and per-col MAD computations

Add four internal helpers that compute the four arrays from the algorithm: `row_static`, `row_motion`, `col_static`, `col_motion`. Use grayscale to stay consistent with `verifier.rs`.

**Files:**
- Modify: `crates/rollshot-core/src/static_region.rs`

- [ ] **Step 1: Write the failing tests**

Append into the `detector_tests` mod in `static_region.rs`:

```rust
use crate::static_region::{compute_col_motion, compute_col_static, compute_row_motion, compute_row_static};
use image::{imageops, Rgba};

fn textured_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([240, 240, 240, 255]));
    for y in 0..height {
        for x in 0..width {
            if (x / 4 + y / 6) % 2 == 0 {
                img.put_pixel(x, y, Rgba([30, ((x * 7) % 200) as u8, ((y * 11) % 200) as u8, 255]));
            }
        }
    }
    img
}

fn crop(canvas: &RgbaImage, y: u32, h: u32) -> RgbaImage {
    imageops::crop_imm(canvas, 0, y, canvas.width(), h).to_image()
}

#[test]
fn row_static_zero_for_identical_frames() {
    let prev = textured_canvas(20, 30);
    let curr = prev.clone();
    let row_static = compute_row_static(&prev, &curr);
    assert_eq!(row_static.len(), 30);
    for v in row_static { assert!(v < 1e-6); }
}

#[test]
fn row_motion_zero_for_aligned_vertical_scroll() {
    let canvas = textured_canvas(40, 200);
    let prev = crop(&canvas, 0, 80);
    let curr = crop(&canvas, 20, 80);  // 20 px scroll down
    let row_motion = compute_row_motion(&prev, &curr, 0, 20);
    // Only rows that fall inside the overlap have a defined value (non-NaN); pick a middle row.
    let middle = row_motion[40];
    assert!(middle.is_finite(), "middle row should have a defined motion-aligned MAD");
    assert!(middle < 1e-3, "aligned content should produce near-zero MAD, got {middle}");
}

#[test]
fn col_static_zero_for_identical_frames() {
    let prev = textured_canvas(30, 20);
    let curr = prev.clone();
    let col_static = compute_col_static(&prev, &curr);
    assert_eq!(col_static.len(), 30);
    for v in col_static { assert!(v < 1e-6); }
}

#[test]
fn col_motion_zero_for_aligned_horizontal_scroll() {
    let canvas = textured_canvas(200, 40);
    let prev = imageops::crop_imm(&canvas, 0, 0, 80, 40).to_image();
    let curr = imageops::crop_imm(&canvas, 20, 0, 80, 40).to_image();
    let col_motion = compute_col_motion(&prev, &curr, 20, 0);
    let middle = col_motion[40];
    assert!(middle.is_finite());
    assert!(middle < 1e-3, "aligned content should produce near-zero MAD, got {middle}");
}
```

- [ ] **Step 2: Run the new tests and confirm they fail**

Run: `rtk cargo test -p rollshot-core --lib static_region::detector_tests::row_static_zero_for_identical_frames -- --exact`
Expected: FAIL — `compute_row_static` does not exist yet.

- [ ] **Step 3: Implement the four helpers**

Add to `static_region.rs` (above `StaticRegionDetector`):

```rust
fn pixel_gray(img: &RgbaImage, x: u32, y: u32) -> f32 {
    let image::Rgba([r, g, b, _]) = *img.get_pixel(x, y);
    0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
}

pub(super) fn compute_row_static(prev: &RgbaImage, curr: &RgbaImage) -> Vec<f32> {
    debug_assert_eq!(prev.dimensions(), curr.dimensions());
    let w = prev.width();
    let h = prev.height();
    let mut out = vec![0.0; h as usize];
    for y in 0..h {
        let mut sum = 0.0;
        for x in 0..w {
            sum += (pixel_gray(prev, x, y) - pixel_gray(curr, x, y)).abs();
        }
        out[y as usize] = sum / (w as f32 * 255.0);
    }
    out
}

pub(super) fn compute_col_static(prev: &RgbaImage, curr: &RgbaImage) -> Vec<f32> {
    debug_assert_eq!(prev.dimensions(), curr.dimensions());
    let w = prev.width();
    let h = prev.height();
    let mut out = vec![0.0; w as usize];
    for x in 0..w {
        let mut sum = 0.0;
        for y in 0..h {
            sum += (pixel_gray(prev, x, y) - pixel_gray(curr, x, y)).abs();
        }
        out[x as usize] = sum / (h as f32 * 255.0);
    }
    out
}

pub(super) fn compute_row_motion(prev: &RgbaImage, curr: &RgbaImage, dx: i32, dy: i32) -> Vec<f32> {
    debug_assert_eq!(prev.dimensions(), curr.dimensions());
    let w = prev.width() as i32;
    let h = prev.height() as i32;
    let mut out = vec![f32::NAN; h as usize];
    for y in 0..h {
        let py = y + dy;
        if py < 0 || py >= h { continue; }
        let mut sum = 0.0;
        let mut count = 0u32;
        for x in 0..w {
            let px = x + dx;
            if px < 0 || px >= w { continue; }
            sum += (pixel_gray(prev, px as u32, py as u32) - pixel_gray(curr, x as u32, y as u32)).abs();
            count += 1;
        }
        out[y as usize] = if count == 0 { f32::NAN } else { sum / (count as f32 * 255.0) };
    }
    out
}

pub(super) fn compute_col_motion(prev: &RgbaImage, curr: &RgbaImage, dx: i32, dy: i32) -> Vec<f32> {
    debug_assert_eq!(prev.dimensions(), curr.dimensions());
    let w = prev.width() as i32;
    let h = prev.height() as i32;
    let mut out = vec![f32::NAN; w as usize];
    for x in 0..w {
        let px = x + dx;
        if px < 0 || px >= w { continue; }
        let mut sum = 0.0;
        let mut count = 0u32;
        for y in 0..h {
            let py = y + dy;
            if py < 0 || py >= h { continue; }
            sum += (pixel_gray(prev, px as u32, py as u32) - pixel_gray(curr, x as u32, y as u32)).abs();
            count += 1;
        }
        out[x as usize] = if count == 0 { f32::NAN } else { sum / (count as f32 * 255.0) };
    }
    out
}
```

- [ ] **Step 4: Run the new tests and confirm they pass**

Run: `rtk cargo test -p rollshot-core --lib static_region::detector_tests`
Expected: all four MAD tests PASS, prior detector tests still PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-core/src/static_region.rs
rtk git commit -m "$(cat <<'EOF'
feat(core): row/col MAD primitives for static region detection

Grayscale-based per-line aggregates for both static (zero offset)
and motion-aligned comparisons, used as building blocks for the
edge scan in the next commit.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Edge scan + `max_band_ratio` guard

Convert the four MAD arrays into per-edge band extents.

**Files:**
- Modify: `crates/rollshot-core/src/static_region.rs`

- [ ] **Step 1: Write the failing tests**

Append to the `detector_tests` mod:

```rust
use crate::static_region::{scan_edges, EdgeExtents};

#[test]
fn scan_returns_zero_when_no_static_lines() {
    let h = 10usize;
    let w = 8usize;
    let row_static  = vec![0.5; h];
    let row_motion  = vec![0.5; h];
    let col_static  = vec![0.5; w];
    let col_motion  = vec![0.5; w];
    let cfg = StaticRegionConfig::default();
    let e = scan_edges(&row_static, &row_motion, &col_static, &col_motion, &cfg);
    assert_eq!(e, EdgeExtents { top: 0, bottom: 0, left: 0, right: 0 });
}

#[test]
fn scan_finds_top_band_up_to_first_non_static_row() {
    // First 3 rows are very static (low static MAD, much higher motion MAD);
    // remaining rows are scrollable (high static MAD).
    let row_static  = vec![0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];
    let row_motion  = vec![0.4, 0.4, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let col_static  = vec![0.5; 8];
    let col_motion  = vec![0.0; 8];
    let cfg = StaticRegionConfig::default();
    let e = scan_edges(&row_static, &row_motion, &col_static, &col_motion, &cfg);
    assert_eq!(e.top, 3);
    assert_eq!(e.bottom, 0);
}

#[test]
fn scan_finds_bottom_band() {
    let row_static = vec![0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0];
    let row_motion = vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.4, 0.4, 0.4];
    let col_static = vec![0.5; 8];
    let col_motion = vec![0.0; 8];
    let cfg = StaticRegionConfig::default();
    let e = scan_edges(&row_static, &row_motion, &col_static, &col_motion, &cfg);
    assert_eq!(e.bottom, 3);
}

#[test]
fn scan_finds_left_and_right_columns() {
    let row_static = vec![0.5; 10];
    let row_motion = vec![0.0; 10];
    let col_static = vec![0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0];
    let col_motion = vec![0.4, 0.4, 0.0, 0.0, 0.0, 0.0, 0.4, 0.4];
    let cfg = StaticRegionConfig::default();
    let e = scan_edges(&row_static, &row_motion, &col_static, &col_motion, &cfg);
    assert_eq!(e.left, 2);
    assert_eq!(e.right, 2);
}

#[test]
fn scan_clamps_extent_above_max_band_ratio() {
    // 10 rows entirely static; with max_band_ratio = 0.3 the cap is 3.
    let row_static = vec![0.0; 10];
    let row_motion = vec![0.4; 10];
    let col_static = vec![0.5; 8];
    let col_motion = vec![0.0; 8];
    let cfg = StaticRegionConfig { max_band_ratio: 0.3, ..StaticRegionConfig::default() };
    let e = scan_edges(&row_static, &row_motion, &col_static, &col_motion, &cfg);
    // top extent would be 10 unclamped -> exceeds 0.3 * 10 = 3 -> zeroed (Step 5 of algorithm).
    assert_eq!(e.top, 0, "extent above max_band_ratio must be zeroed, got {}", e.top);
}

#[test]
fn scan_treats_nan_motion_as_static_only_when_static_score_is_very_low() {
    // Edge rows where motion alignment falls off the frame (NaN motion).
    // First 3 rows have static ≈ 0 (well below threshold/4) -> accepted as static.
    let row_static = vec![0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];
    let row_motion = vec![f32::NAN, f32::NAN, f32::NAN, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let col_static = vec![0.5; 8];
    let col_motion = vec![0.0; 8];
    let cfg = StaticRegionConfig::default();
    let e = scan_edges(&row_static, &row_motion, &col_static, &col_motion, &cfg);
    assert_eq!(e.top, 3, "NaN motion + very low static should classify as static");
}

#[test]
fn scan_rejects_nan_motion_when_static_score_not_negligible() {
    // Default threshold = 4/255 ≈ 0.01568; threshold/4 ≈ 0.00392.
    // Static = 0.01 is below the main threshold but above threshold/4,
    // so NaN motion rows should NOT be classified as static.
    let row_static = vec![0.01, 0.01, 0.01, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];
    let row_motion = vec![f32::NAN, f32::NAN, f32::NAN, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let col_static = vec![0.5; 8];
    let col_motion = vec![0.0; 8];
    let cfg = StaticRegionConfig::default();
    let e = scan_edges(&row_static, &row_motion, &col_static, &col_motion, &cfg);
    assert_eq!(e.top, 0, "NaN motion + moderate static should NOT count as static");
}
```

- [ ] **Step 2: Run the new tests and confirm they fail**

Run: `rtk cargo test -p rollshot-core --lib static_region::detector_tests::scan_returns_zero_when_no_static_lines -- --exact`
Expected: FAIL — `scan_edges` not defined.

- [ ] **Step 3: Implement `scan_edges`**

Add to `static_region.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EdgeExtents {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

fn is_static_line(static_score: f32, motion_score: f32, cfg: &StaticRegionConfig) -> bool {
    if !static_score.is_finite() { return false; }
    if static_score >= cfg.static_mad_threshold { return false; }
    if !motion_score.is_finite() {
        // Edge row / column: the motion-aligned comparison fell off the frame
        // (shifted (x + dx, y + dy) coordinate is out of bounds for every x or y
        // we tried). We only have row_static / col_static evidence.
        //
        // To detect sticky footer / sticky right-sidebar (which sit exactly on
        // the appended-edge of each slice and therefore have no in-bounds
        // motion alignment), we must accept some rows / cols here. To avoid
        // false-positives on benign uniform-color scrollable content (e.g. a
        // paragraph break or all-white area), require static_score well below
        // threshold — concretely threshold / 4. This is the minimum guard that
        // lets edge-anchored sticky bands be detected. Unit tests
        // `scan_treats_nan_motion_as_static_only_when_static_score_is_very_low`
        // and `scan_rejects_nan_motion_when_static_score_not_negligible` pin
        // both sides of this rule.
        return static_score < cfg.static_mad_threshold / 4.0;
    }
    (motion_score - static_score) > cfg.motion_margin
}

fn scan_from_start(static_scores: &[f32], motion_scores: &[f32], cfg: &StaticRegionConfig) -> u32 {
    let mut extent = 0u32;
    for i in 0..static_scores.len() {
        if is_static_line(static_scores[i], motion_scores[i], cfg) {
            extent += 1;
        } else {
            break;
        }
    }
    extent
}

fn scan_from_end(static_scores: &[f32], motion_scores: &[f32], cfg: &StaticRegionConfig) -> u32 {
    let mut extent = 0u32;
    for i in (0..static_scores.len()).rev() {
        if is_static_line(static_scores[i], motion_scores[i], cfg) {
            extent += 1;
        } else {
            break;
        }
    }
    extent
}

pub(super) fn scan_edges(
    row_static: &[f32],
    row_motion: &[f32],
    col_static: &[f32],
    col_motion: &[f32],
    cfg: &StaticRegionConfig,
) -> EdgeExtents {
    let h = row_static.len() as u32;
    let w = col_static.len() as u32;
    let mut top = scan_from_start(row_static, row_motion, cfg);
    let mut bottom = scan_from_end(row_static, row_motion, cfg);
    let mut left = scan_from_start(col_static, col_motion, cfg);
    let mut right = scan_from_end(col_static, col_motion, cfg);

    let max_row = (h as f32 * cfg.max_band_ratio) as u32;
    let max_col = (w as f32 * cfg.max_band_ratio) as u32;
    if top > max_row { top = 0; }
    if bottom > max_row { bottom = 0; }
    if left > max_col { left = 0; }
    if right > max_col { right = 0; }

    EdgeExtents { top, bottom, left, right }
}
```

- [ ] **Step 4: Run the new tests and confirm they pass**

Run: `rtk cargo test -p rollshot-core --lib static_region::detector_tests`
Expected: all scan tests PASS, prior tests still PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-core/src/static_region.rs
rtk git commit -m "$(cat <<'EOF'
feat(core): contiguous-from-edge band scan with max_band_ratio guard

scan_edges turns the four MAD arrays into per-edge extents,
rejecting any band that exceeds max_band_ratio of its dimension.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Background color sampling

Sample a channel-wise median over a thin inner strip on `prev` for each non-zero band.

**Files:**
- Modify: `crates/rollshot-core/src/static_region.rs`

- [ ] **Step 1: Write the failing tests**

Append to `detector_tests`:

```rust
use crate::static_region::sample_band_bg_color;

#[test]
fn bg_color_returns_uniform_band_color() {
    let mut img = RgbaImage::from_pixel(20, 20, Rgba([255, 255, 255, 255]));
    // 4-px sticky top band painted gray.
    for y in 0..4 {
        for x in 0..20 {
            img.put_pixel(x, y, Rgba([100, 110, 120, 255]));
        }
    }
    let bg = sample_band_bg_color(&img, Edge::Top, 4).expect("non-zero band");
    assert_eq!(bg, [100, 110, 120, 255]);
}

#[test]
fn bg_color_is_channel_wise_median_for_noisy_band() {
    let mut img = RgbaImage::from_pixel(20, 20, Rgba([0, 0, 0, 255]));
    // Top band of mostly gray, with a couple of outlier pixels.
    for y in 0..4 {
        for x in 0..20 {
            img.put_pixel(x, y, Rgba([100, 110, 120, 255]));
        }
    }
    img.put_pixel(0, 0, Rgba([255, 0, 255, 255]));
    img.put_pixel(19, 3, Rgba([0, 255, 0, 255]));
    let bg = sample_band_bg_color(&img, Edge::Top, 4).expect("non-zero band");
    // Median across the strip should still be the dominant gray color.
    assert_eq!(bg, [100, 110, 120, 255]);
}

#[test]
fn bg_color_zero_thickness_returns_none() {
    let img = RgbaImage::from_pixel(10, 10, Rgba([0, 0, 0, 255]));
    assert!(sample_band_bg_color(&img, Edge::Top, 0).is_none());
}
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `rtk cargo test -p rollshot-core --lib static_region::detector_tests::bg_color_returns_uniform_band_color -- --exact`
Expected: FAIL — `sample_band_bg_color` and `Edge` not defined.

- [ ] **Step 3: Implement `sample_band_bg_color`**

Add to `static_region.rs`:

```rust
#[derive(Debug, Clone, Copy)]
pub(super) enum Edge { Top, Bottom, Left, Right }

/// Sorted-vector median, picking the upper of the two middles when len is even.
/// Shared between this task's edge sampling and Task 9's per-band aggregation.
pub(super) fn median_u8(mut v: Vec<u8>) -> u8 {
    v.sort_unstable();
    v[v.len() / 2]
}

pub(super) fn sample_band_bg_color(img: &RgbaImage, edge: Edge, thickness: u32) -> Option<[u8; 4]> {
    if thickness == 0 { return None; }
    let w = img.width();
    let h = img.height();
    // 1-px strip on the inner border of the band.
    let (x0, y0, sw, sh) = match edge {
        Edge::Top    => (0, thickness.saturating_sub(1), w, 1.min(thickness)),
        Edge::Bottom => (0, h.saturating_sub(thickness), w, 1.min(thickness)),
        Edge::Left   => (thickness.saturating_sub(1), 0, 1.min(thickness), h),
        Edge::Right  => (w.saturating_sub(thickness), 0, 1.min(thickness), h),
    };
    let mut rs = Vec::new();
    let mut gs = Vec::new();
    let mut bs = Vec::new();
    let mut as_ = Vec::new();
    for y in y0..(y0 + sh).min(h) {
        for x in x0..(x0 + sw).min(w) {
            let Rgba([r, g, b, a]) = *img.get_pixel(x, y);
            rs.push(r); gs.push(g); bs.push(b); as_.push(a);
        }
    }
    if rs.is_empty() { return None; }
    Some([median_u8(rs), median_u8(gs), median_u8(bs), median_u8(as_)])
}
```

- [ ] **Step 4: Run new tests to verify they pass**

Run: `rtk cargo test -p rollshot-core --lib static_region::detector_tests`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-core/src/static_region.rs
rtk git commit -m "$(cat <<'EOF'
feat(core): sample band background color via channel-wise median

Thin inner-edge strip per band, RGBA median per channel. Returns
None for zero-thickness bands so callers can skip them naturally.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Detector lock + median aggregation, full `observe` pipeline

Wire MAD + scan + bg_color into `observe`. After `min_observations` calls, lock the final `StaticMask` using the channel-wise median of observed extents and colors.

**Files:**
- Modify: `crates/rollshot-core/src/static_region.rs`

- [ ] **Step 1: Write the failing tests**

Append to `detector_tests` (replace any earlier `detector_returns_none_below_min_observations` content if it conflicts; the test below supersedes it):

```rust
fn paint_left_sidebar(frame: &mut RgbaImage, width: u32, color: [u8; 4]) {
    for y in 0..frame.height() {
        for x in 0..width.min(frame.width()) {
            frame.put_pixel(x, y, Rgba(color));
        }
    }
}

fn paint_top_band(frame: &mut RgbaImage, height: u32, color: [u8; 4]) {
    for y in 0..height.min(frame.height()) {
        for x in 0..frame.width() {
            frame.put_pixel(x, y, Rgba(color));
        }
    }
}

#[test]
fn pure_scroll_input_locks_with_all_none_bands() {
    let canvas = textured_canvas(40, 300);
    let mut d = StaticRegionDetector::new(StaticRegionConfig::default());
    for i in 0..4 {
        let prev = crop(&canvas, (i * 20) as u32, 80);
        let curr = crop(&canvas, ((i + 1) * 20) as u32, 80);
        d.observe(&prev, &curr, 0, 20);
    }
    let mask = d.mask().expect("detector must lock after min_observations");
    assert!(mask.top.is_none());
    assert!(mask.bottom.is_none());
    assert!(mask.left.is_none());
    assert!(mask.right.is_none());
}

#[test]
fn detector_locks_left_sidebar_with_median_thickness() {
    let bg = [100, 110, 120, 255];
    let canvas = textured_canvas(40, 300);
    let mut d = StaticRegionDetector::new(StaticRegionConfig::default());
    for i in 0..4 {
        let mut prev = crop(&canvas, (i * 20) as u32, 80);
        let mut curr = crop(&canvas, ((i + 1) * 20) as u32, 80);
        paint_left_sidebar(&mut prev, 6, bg);
        paint_left_sidebar(&mut curr, 6, bg);
        d.observe(&prev, &curr, 0, 20);
    }
    let mask = d.mask().expect("must lock");
    let left = mask.left.expect("left band detected");
    assert_eq!(left.thickness, 6);
    assert_eq!(left.bg_color, bg);
    assert!(mask.top.is_none());
    assert!(mask.right.is_none());
    assert!(mask.bottom.is_none());
}

#[test]
fn detector_locks_top_band() {
    let bg = [80, 80, 80, 255];
    let canvas = textured_canvas(40, 300);
    let mut d = StaticRegionDetector::new(StaticRegionConfig::default());
    for i in 0..4 {
        let mut prev = crop(&canvas, (i * 20) as u32, 80);
        let mut curr = crop(&canvas, ((i + 1) * 20) as u32, 80);
        paint_top_band(&mut prev, 5, bg);
        paint_top_band(&mut curr, 5, bg);
        d.observe(&prev, &curr, 0, 20);
    }
    let mask = d.mask().expect("must lock");
    let top = mask.top.expect("top band detected");
    assert_eq!(top.thickness, 5);
    assert_eq!(top.bg_color, bg);
}

#[test]
fn detector_single_outlier_does_not_shift_locked_thickness() {
    // Three frames with 6-px sidebar, one with 10-px (outlier).
    // Median of (6, 6, 6, 10) is 6.
    let bg = [100, 110, 120, 255];
    let canvas = textured_canvas(40, 300);
    let cfg = StaticRegionConfig { min_observations: 4, ..StaticRegionConfig::default() };
    let mut d = StaticRegionDetector::new(cfg);
    let widths = [6, 6, 10, 6];
    for (i, w) in widths.iter().enumerate() {
        let mut prev = crop(&canvas, (i * 20) as u32, 80);
        let mut curr = crop(&canvas, ((i + 1) * 20) as u32, 80);
        paint_left_sidebar(&mut prev, *w, bg);
        paint_left_sidebar(&mut curr, *w, bg);
        d.observe(&prev, &curr, 0, 20);
    }
    let mask = d.mask().expect("must lock");
    let left = mask.left.expect("left band");
    assert_eq!(left.thickness, 6, "median should suppress single outlier");
}

#[test]
fn subsequent_observations_after_lock_are_noops() {
    let bg = [100, 110, 120, 255];
    let canvas = textured_canvas(40, 300);
    let mut d = StaticRegionDetector::new(StaticRegionConfig::default());
    for i in 0..3 {
        let mut prev = crop(&canvas, (i * 20) as u32, 80);
        let mut curr = crop(&canvas, ((i + 1) * 20) as u32, 80);
        paint_left_sidebar(&mut prev, 6, bg);
        paint_left_sidebar(&mut curr, 6, bg);
        d.observe(&prev, &curr, 0, 20);
    }
    let locked = *d.mask().unwrap();
    // Now feed an outlier observation; mask must not change.
    let mut prev = crop(&canvas, 60, 80);
    let mut curr = crop(&canvas, 80, 80);
    paint_left_sidebar(&mut prev, 18, [1, 1, 1, 255]);
    paint_left_sidebar(&mut curr, 18, [1, 1, 1, 255]);
    d.observe(&prev, &curr, 0, 20);
    assert_eq!(d.mask().copied().unwrap(), locked);
}
```

- [ ] **Step 2: Run the new tests and confirm they fail**

Run: `rtk cargo test -p rollshot-core --lib static_region::detector_tests::pure_scroll_input_locks_with_all_none_bands -- --exact`
Expected: FAIL — detector still returns None.

- [ ] **Step 3: Replace the stub `observe` and `mask` with the full pipeline**

In `static_region.rs`, replace the existing `impl StaticRegionDetector { ... }` block with:

```rust
impl StaticRegionDetector {
    pub(crate) fn new(config: StaticRegionConfig) -> Self {
        Self { config, observations: Vec::new(), locked: None }
    }

    pub(crate) fn observe(
        &mut self,
        prev: &RgbaImage,
        curr: &RgbaImage,
        dx: i32,
        dy: i32,
    ) {
        if self.locked.is_some() { return; }
        if prev.dimensions() != curr.dimensions() { return; }

        let row_static = compute_row_static(prev, curr);
        let row_motion = compute_row_motion(prev, curr, dx, dy);
        let col_static = compute_col_static(prev, curr);
        let col_motion = compute_col_motion(prev, curr, dx, dy);
        let extents = scan_edges(&row_static, &row_motion, &col_static, &col_motion, &self.config);

        let top    = sample_band_bg_color(prev, Edge::Top,    extents.top)
                     .unwrap_or([0, 0, 0, 0]);
        let bottom = sample_band_bg_color(prev, Edge::Bottom, extents.bottom)
                     .unwrap_or([0, 0, 0, 0]);
        let left   = sample_band_bg_color(prev, Edge::Left,   extents.left)
                     .unwrap_or([0, 0, 0, 0]);
        let right  = sample_band_bg_color(prev, Edge::Right,  extents.right)
                     .unwrap_or([0, 0, 0, 0]);

        self.observations.push(EdgeObservation {
            top:    BandObs { thickness: extents.top,    color: top },
            bottom: BandObs { thickness: extents.bottom, color: bottom },
            left:   BandObs { thickness: extents.left,   color: left },
            right:  BandObs { thickness: extents.right,  color: right },
        });

        if self.observations.len() >= self.config.min_observations {
            self.locked = Some(self.aggregate_mask());
        }
    }

    pub(crate) fn mask(&self) -> Option<&StaticMask> {
        self.locked.as_ref()
    }

    fn aggregate_mask(&self) -> StaticMask {
        StaticMask {
            top:    self.aggregate_band(|o| o.top),
            bottom: self.aggregate_band(|o| o.bottom),
            left:   self.aggregate_band(|o| o.left),
            right:  self.aggregate_band(|o| o.right),
        }
    }

    fn aggregate_band(&self, pick: impl Fn(&EdgeObservation) -> BandObs) -> Option<StickyBand> {
        let mut thicknesses: Vec<u32> = self.observations.iter().map(|o| pick(o).thickness).collect();
        thicknesses.sort_unstable();
        let median_thickness = thicknesses[thicknesses.len() / 2];
        if median_thickness == 0 { return None; }

        // Median color only over frames where the edge actually fired.
        let mut rs = Vec::new();
        let mut gs = Vec::new();
        let mut bs = Vec::new();
        let mut as_ = Vec::new();
        for obs in &self.observations {
            let b = pick(obs);
            if b.thickness == 0 { continue; }
            rs.push(b.color[0]); gs.push(b.color[1]);
            bs.push(b.color[2]); as_.push(b.color[3]);
        }
        let color = [median_u8(rs), median_u8(gs), median_u8(bs), median_u8(as_)];
        Some(StickyBand { thickness: median_thickness, bg_color: color })
    }
}
```

Also delete the stub `observe` body and the placeholder zero-observation push that Task 5 added — the block above replaces them.

- [ ] **Step 4: Run all detector tests**

Run: `rtk cargo test -p rollshot-core --lib static_region::detector_tests`
Expected: all detector tests PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-core/src/static_region.rs
rtk git commit -m "$(cat <<'EOF'
feat(core): wire StaticRegionDetector pipeline with median lock

observe now runs the full row/col MAD + edge scan + bg color
sample, and aggregates with channel-wise median after
min_observations frames. Lock is immutable afterwards.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Integrate detector into `Stitcher`

`Stitcher` holds the detector, calls `observe` after verification, and passes `mask()` to `canvas.append`.

**Files:**
- Modify: `crates/rollshot-core/src/stitcher.rs`

- [ ] **Step 1: Add the field and constructor wiring**

In `crates/rollshot-core/src/stitcher.rs`, add the import:

```rust
use crate::static_region::StaticRegionDetector;
```

Add the field to the struct:

```rust
pub struct Stitcher {
    config: StitchConfig,
    canvas: Option<LinearCanvas>,
    last_good_frame: Option<RgbaImage>,
    last_good_signature: Option<Vec<u8>>,
    last_motion: (i32, i32),
    locked_axis: Option<ScrollAxis>,
    stats: StitchStats,
    static_detector: StaticRegionDetector,   // NEW
}
```

Update `Stitcher::new`:

```rust
pub fn new(config: StitchConfig) -> Self {
    let static_detector = StaticRegionDetector::new(config.static_region.clone());
    Self {
        config,
        canvas: None,
        last_good_frame: None,
        last_good_signature: None,
        last_motion: (0, 0),
        locked_axis: None,
        stats: StitchStats::default(),
        static_detector,
    }
}
```

- [ ] **Step 2: Hook `observe` and pass the mask to `append`**

In `push_frame`, find the block that just succeeded `PixelOverlapVerifier::Pass` and is about to call `canvas.append`. Currently (paraphrased):

```rust
let (overlap_region, _verifier_score) = match verifier.verify(anchor, &frame, &candidate) { ... };

let canvas = self.canvas.as_mut().expect("canvas present after first frame");
let added = match canvas.append(direction, &frame, slice_px, None) {
```

Insert detector observation and mask query right between the verifier check and the canvas mutable borrow:

```rust
if self.config.static_region.enabled {
    self.static_detector.observe(anchor, &frame, candidate.dx, candidate.dy);
}
let mask = if self.config.static_region.enabled {
    self.static_detector.mask()
} else {
    None
};

let canvas = self.canvas.as_mut().expect("canvas present after first frame");
let added = match canvas.append(direction, &frame, slice_px, mask) {
```

Make sure the `anchor` and `frame` borrows are still valid here. `anchor` is `&self.last_good_frame` (borrowed at the top of `push_frame`); the new observe call comes before `self.canvas.as_mut()` which borrows `self` mutably, so this ordering compiles.

- [ ] **Step 3: Run the full library + integration test suite**

Run: `rtk cargo test -p rollshot-core`
Expected: all existing tests PASS. The detector activates by default but on the synthetic fixtures that already exist (pure-scroll), it should lock with all-None bands and therefore have no visible effect.

If any pre-existing fixture trips on a false-positive sticky detection, log it and adjust by setting `config.static_region.enabled = false` only inside the failing test, then file a follow-up task to investigate — but do NOT loosen the detector defaults in this commit.

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-core/src/stitcher.rs
rtk git commit -m "$(cat <<'EOF'
feat(core): plug StaticRegionDetector into Stitcher

push_frame now feeds verified motion to the detector and threads
detector.mask() into canvas.append. Detector activates by default
(opt-out via config.static_region.enabled = false).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: Test helpers — `paint_sticky_sidebar`, `paint_sticky_footer`, `paint_sticky_horizontal_band`

**Files:**
- Modify: `crates/rollshot-core/tests/common/mod.rs` (or create if missing; check first with `rtk ls crates/rollshot-core/tests/common/`)

- [ ] **Step 1: Check whether the file exists and what it currently exports**

Run: `rtk ls crates/rollshot-core/tests/common/ 2>&1`
Run: `rtk grep -n 'pub fn paint_' crates/rollshot-core/tests/common/mod.rs`

You should see `paint_sticky_header` already exported. Match its existing style and place new helpers in the same file.

- [ ] **Step 2: Add the three new helpers**

Append to `crates/rollshot-core/tests/common/mod.rs`:

```rust
#[derive(Debug, Clone, Copy)]
pub enum Side { Left, Right }

/// Paints a sticky vertical sidebar of the given pixel `width` on the chosen
/// side, full frame height. Uses a simple icon pattern so the sidebar is not
/// uniform-color (forcing the detector to actually do work).
pub fn paint_sticky_sidebar(frame: &mut image::RgbaImage, side: Side, width: u32) {
    let h = frame.height();
    let w = frame.width();
    let x_start = match side {
        Side::Left => 0,
        Side::Right => w.saturating_sub(width),
    };
    for y in 0..h {
        for x in x_start..(x_start + width).min(w) {
            // Slight pattern so the sidebar contains internal variation.
            let v = if (y / 7) % 2 == 0 { 100 } else { 140 };
            frame.put_pixel(x, y, image::Rgba([v, v, v, 255]));
        }
    }
}

/// Paints a sticky footer band of `height` pixels along the bottom edge.
pub fn paint_sticky_footer(frame: &mut image::RgbaImage, height: u32) {
    let h = frame.height();
    let w = frame.width();
    let y_start = h.saturating_sub(height);
    for y in y_start..h {
        for x in 0..w {
            let v = if (x / 9) % 2 == 0 { 110 } else { 150 };
            frame.put_pixel(x, y, image::Rgba([v, v, v, 255]));
        }
    }
}

/// Paints sticky horizontal bands of `top_h` pixels at the top and `bottom_h`
/// at the bottom (either may be zero). Used for horizontal-scroll fixtures.
pub fn paint_sticky_horizontal_band(frame: &mut image::RgbaImage, top_h: u32, bottom_h: u32) {
    let h = frame.height();
    let w = frame.width();
    for y in 0..top_h.min(h) {
        for x in 0..w {
            let v = if (x / 5) % 2 == 0 { 90 } else { 130 };
            frame.put_pixel(x, y, image::Rgba([v, v, v, 255]));
        }
    }
    let bottom_start = h.saturating_sub(bottom_h);
    for y in bottom_start..h {
        for x in 0..w {
            let v = if (x / 5) % 2 == 0 { 95 } else { 135 };
            frame.put_pixel(x, y, image::Rgba([v, v, v, 255]));
        }
    }
}
```

- [ ] **Step 3: Verify it compiles (no tests exercise the helpers directly yet)**

Run: `rtk cargo test -p rollshot-core --tests --no-run`
Expected: clean compile, no warnings about the new functions (they will be used in the next task — if `#[allow(dead_code)]` is needed temporarily because the integration test file hasn't landed yet, add it on the `mod common` declaration in any failing test file or on each new helper).

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-core/tests/common/mod.rs
rtk git commit -m "$(cat <<'EOF'
test(core): add paint_sticky_sidebar / footer / horizontal_band helpers

Companion to the existing paint_sticky_header. Each helper draws
a band with internal vertical / horizontal variation so the
detector cannot trivially pass on flat-color edges.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 12: Vertical-scroll integration tests

End-to-end tests that drive the `Stitcher` with painted fixtures and verify the stitched output.

**Files:**
- Create: `crates/rollshot-core/tests/static_region.rs`

- [ ] **Step 1: Write the failing integration tests**

Create `crates/rollshot-core/tests/static_region.rs`:

```rust
mod common;

use common::{paint_sticky_footer, paint_sticky_header, paint_sticky_sidebar, Side};
use image::{GenericImageView, Rgba, RgbaImage};
use rollshot_core::{StitchConfig, StitchOutcome, Stitcher};

fn make_scroll_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([240, 240, 240, 255]));
    for y in 0..height {
        for x in 0..width {
            if (x / 4 + y / 6) % 2 == 0 {
                img.put_pixel(x, y, Rgba([30, ((x * 7) % 200) as u8, ((y * 11) % 200) as u8, 255]));
            }
        }
    }
    img
}

fn crop(canvas: &RgbaImage, y: u32, h: u32) -> RgbaImage {
    image::imageops::crop_imm(canvas, 0, y, canvas.width(), h).to_image()
}

fn drive_vertical(stitcher: &mut Stitcher, canvas: &RgbaImage, frame_h: u32, step: u32, paint: impl Fn(&mut RgbaImage)) {
    let mut y = 0;
    while y + frame_h <= canvas.height() {
        let mut f = crop(canvas, y, frame_h);
        paint(&mut f);
        stitcher.push_frame(f);
        y += step;
    }
}

#[test]
fn sticky_left_sidebar_does_not_duplicate_in_output() {
    let canvas = make_scroll_canvas(120, 600);
    let mut stitcher = Stitcher::new(StitchConfig::default());
    drive_vertical(&mut stitcher, &canvas, 160, 40, |f| paint_sticky_sidebar(f, Side::Left, 12));

    let stitched = stitcher.full_image().expect("stitched output exists");
    // The detector should have flagged the left 12 columns as static.
    // First frame keeps the painted sidebar verbatim, so column 0 of canvas row 0 is painted.
    let first_frame_pixel = stitched.get_pixel(0, 0);
    assert_ne!(first_frame_pixel, &Rgba([240, 240, 240, 255]), "first frame's sidebar must be preserved");
    // After the first frame, sidebar columns should be flat bg color (the median painted gray ≈ 100 or 140).
    let later_pixel = stitched.get_pixel(0, stitched.height() - 1);
    // Pixel should be one of the two flat values used in paint_sticky_sidebar (100 or 140 gray).
    let gray = later_pixel[0];
    assert!(gray == 100 || gray == 140, "left-edge pixel at canvas bottom = {gray:?}");
    assert_eq!(later_pixel[0], later_pixel[1], "bg should be gray (R==G)");
    assert_eq!(later_pixel[1], later_pixel[2], "bg should be gray (G==B)");
}

#[test]
fn sticky_right_sidebar_does_not_duplicate_in_output() {
    let canvas = make_scroll_canvas(120, 600);
    let mut stitcher = Stitcher::new(StitchConfig::default());
    drive_vertical(&mut stitcher, &canvas, 160, 40, |f| paint_sticky_sidebar(f, Side::Right, 12));

    let stitched = stitcher.full_image().expect("stitched output exists");
    let w = stitched.width();
    let later_pixel = stitched.get_pixel(w - 1, stitched.height() - 1);
    let gray = later_pixel[0];
    assert!(gray == 100 || gray == 140, "right-edge bg gray = {gray:?}");
}

#[test]
fn sticky_footer_does_not_duplicate_in_output() {
    let canvas = make_scroll_canvas(120, 600);
    let mut stitcher = Stitcher::new(StitchConfig::default());
    drive_vertical(&mut stitcher, &canvas, 160, 40, |f| paint_sticky_footer(f, 12));

    let stitched = stitcher.full_image().expect("stitched output exists");
    // Bottom rows of the canvas (last appended slice's bottom 12 rows) should be bg.
    let later_pixel = stitched.get_pixel(stitched.width() / 2, stitched.height() - 1);
    let gray = later_pixel[0];
    assert!(gray == 110 || gray == 150, "footer-edge bg gray = {gray:?}");
}

#[test]
fn sticky_header_output_is_clean_after_first_frame() {
    let canvas = make_scroll_canvas(120, 600);
    let mut stitcher = Stitcher::new(StitchConfig::default());
    drive_vertical(&mut stitcher, &canvas, 160, 40, |f| paint_sticky_header(f, 12));

    let stitched = stitcher.full_image().expect("stitched output exists");
    // Beyond the first frame, header rows should never re-appear: pick a y just after
    // the first frame ends (>= 160). The earliest re-appearance of a sticky header
    // would be in the first append's slice (rows [160 - slice_px, 160) of canvas,
    // mapping to bottom slice_px rows of frame 1), so check that those are bg-painted.
    // We pick a row at canvas y = 200 which is comfortably inside the appended region.
    let mid_pixel = stitched.get_pixel(stitched.width() / 2, 200);
    // Header colors are gray values produced by paint_sticky_header (existing helper);
    // the surrounding scroll content is colorful. Header bg is gray (R==G==B).
    // If the header had duplicated we'd see gray=R=G=B at every (slice_px * k) interval.
    // Without duplication this row is content (R/G/B differ).
    assert!(
        mid_pixel[0] != mid_pixel[1] || mid_pixel[1] != mid_pixel[2],
        "row 200 should be scrollable content, got {mid_pixel:?}"
    );
}

#[test]
fn first_frame_keeps_sticky_pixels_verbatim() {
    let canvas = make_scroll_canvas(120, 600);
    let mut stitcher = Stitcher::new(StitchConfig::default());

    // Push the first frame manually with a known sidebar pattern.
    let mut first = crop(&canvas, 0, 160);
    paint_sticky_sidebar(&mut first, Side::Left, 8);
    let expected_first = first.clone();
    let outcome = stitcher.push_frame(first);
    assert!(matches!(outcome, StitchOutcome::FirstFrame));

    // Drive a few more frames so the canvas grows.
    drive_vertical(&mut stitcher, &canvas, 160, 40, |f| paint_sticky_sidebar(f, Side::Left, 8));

    let stitched = stitcher.full_image().expect("stitched output");
    // First 160 rows of stitched must equal the first painted frame exactly.
    for y in 0..160 {
        for x in 0..120 {
            assert_eq!(
                stitched.get_pixel(x, y),
                expected_first.get_pixel(x, y),
                "first-frame pixel mismatch at ({x}, {y})"
            );
        }
    }
}
```

- [ ] **Step 2: Run integration tests and confirm initial state**

Run: `rtk cargo test -p rollshot-core --test static_region`
Expected: depending on detector tuning some may already PASS once the prior tasks land. If any FAIL because the detector under-/over-detects, do NOT loosen detector defaults — re-examine `paint_sticky_sidebar`'s contrast and `make_scroll_canvas`'s motion-aligned MAD. Sidebar paints should produce a sidebar where `static_score << motion_score`. If the sidebar pattern's `motion_score` is also near zero, increase the internal variation in `paint_sticky_sidebar` (smaller stripe period).

- [ ] **Step 3: Commit**

```bash
rtk git add crates/rollshot-core/tests/static_region.rs
rtk git commit -m "$(cat <<'EOF'
test(core): vertical-scroll sticky region integration tests

Cover left/right sidebar, footer, header, and first-frame
preservation. Each test drives the full Stitcher pipeline and
asserts on the final stitched RgbaImage.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 13: Horizontal-scroll integration tests

**Files:**
- Modify: `crates/rollshot-core/tests/static_region.rs`

- [ ] **Step 1: Add horizontal-scroll fixtures and helpers**

Append to `crates/rollshot-core/tests/static_region.rs`:

```rust
use common::paint_sticky_horizontal_band;

fn make_wide_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([240, 240, 240, 255]));
    for x in 0..width {
        for y in 0..height {
            if (x / 4 + y / 6) % 2 == 0 {
                img.put_pixel(x, y, Rgba([((x * 7) % 200) as u8, 30, ((y * 11) % 200) as u8, 255]));
            }
        }
    }
    img
}

fn crop_xy(canvas: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
    image::imageops::crop_imm(canvas, x, y, w, h).to_image()
}

fn drive_horizontal(stitcher: &mut Stitcher, canvas: &RgbaImage, frame_w: u32, step: u32, paint: impl Fn(&mut RgbaImage)) {
    let mut x = 0;
    while x + frame_w <= canvas.width() {
        let mut f = crop_xy(canvas, x, 0, frame_w, canvas.height());
        paint(&mut f);
        stitcher.push_frame(f);
        x += step;
    }
}

#[test]
fn horizontal_scroll_with_sticky_top_band() {
    let canvas = make_wide_canvas(600, 120);
    let mut stitcher = Stitcher::new(StitchConfig::default());
    drive_horizontal(&mut stitcher, &canvas, 160, 40, |f| paint_sticky_horizontal_band(f, 10, 0));

    let stitched = stitcher.full_image().expect("stitched output");
    // The top 10 rows of canvas, beyond the first frame, must be bg-filled (gray).
    // Pick a column comfortably past the first frame: x = 250 is in the second appended slice.
    let later_pixel = stitched.get_pixel(250, 0);
    let gray = later_pixel[0];
    assert!(gray == 90 || gray == 130, "top-band bg gray = {gray:?}");
    assert_eq!(later_pixel[0], later_pixel[1]);
    assert_eq!(later_pixel[1], later_pixel[2]);
}

#[test]
fn horizontal_scroll_with_sticky_bottom_band() {
    let canvas = make_wide_canvas(600, 120);
    let mut stitcher = Stitcher::new(StitchConfig::default());
    drive_horizontal(&mut stitcher, &canvas, 160, 40, |f| paint_sticky_horizontal_band(f, 0, 8));

    let stitched = stitcher.full_image().expect("stitched output");
    let h = stitched.height();
    let later_pixel = stitched.get_pixel(250, h - 1);
    let gray = later_pixel[0];
    assert!(gray == 95 || gray == 135, "bottom-band bg gray = {gray:?}");
}
```

- [ ] **Step 2: Run the new tests**

Run: `rtk cargo test -p rollshot-core --test static_region`
Expected: all four horizontal-scroll tests PASS, along with the vertical-scroll tests from Task 12.

- [ ] **Step 3: Commit**

```bash
rtk git add crates/rollshot-core/tests/static_region.rs
rtk git commit -m "$(cat <<'EOF'
test(core): horizontal-scroll sticky band integration tests

Verify the detector and canvas mask behave symmetrically across
the scroll axis: top and bottom bands during horizontal scroll
are masked after the first frame.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 14: Regression tests — disabled config + no_sticky byte-identical baseline

**Files:**
- Modify: `crates/rollshot-core/tests/static_region.rs`

- [ ] **Step 1: Add the disabled-config and byte-identical tests**

Append to `crates/rollshot-core/tests/static_region.rs`:

```rust
use rollshot_core::StaticRegionConfig;

fn disabled_config() -> StitchConfig {
    StitchConfig {
        static_region: StaticRegionConfig { enabled: false, ..StaticRegionConfig::default() },
        ..StitchConfig::default()
    }
}

#[test]
fn detector_disabled_via_config_reproduces_legacy_pixel_for_pixel() {
    let canvas = make_scroll_canvas(120, 600);

    let mut s_on  = Stitcher::new(StitchConfig::default());
    let mut s_off = Stitcher::new(disabled_config());

    // Drive both with painted sticky sidebar.
    drive_vertical(&mut s_on,  &canvas, 160, 40, |f| paint_sticky_sidebar(f, Side::Left, 12));
    drive_vertical(&mut s_off, &canvas, 160, 40, |f| paint_sticky_sidebar(f, Side::Left, 12));

    let on  = s_on.full_image().expect("on output");
    let off = s_off.full_image().expect("off output");
    assert_eq!(on.dimensions(), off.dimensions());

    // The disabled stitcher's output is what v0.2 would produce.
    // With detector ON, at least one pixel inside the sidebar columns of an
    // appended slice must differ — that is the entire point of v0.2.1.
    let mut differs = false;
    for y in 160..on.height() {
        for x in 0..12 {
            if on.get_pixel(x, y) != off.get_pixel(x, y) {
                differs = true;
                break;
            }
        }
        if differs { break; }
    }
    assert!(differs, "with detector ON some sidebar pixel in appended slices must differ from v0.2");
}

#[test]
fn no_sticky_baseline_output_byte_identical_to_disabled_config() {
    // On a pure-scroll fixture (no painted sticky region), default-config and
    // disabled-config Stitchers must produce byte-identical output. This is
    // the no-regression gate for v0.2.1.
    let canvas = make_scroll_canvas(120, 600);

    let mut s_on  = Stitcher::new(StitchConfig::default());
    let mut s_off = Stitcher::new(disabled_config());

    drive_vertical(&mut s_on,  &canvas, 160, 40, |_| {});
    drive_vertical(&mut s_off, &canvas, 160, 40, |_| {});

    let on  = s_on.full_image().expect("on output");
    let off = s_off.full_image().expect("off output");
    assert_eq!(on.dimensions(), off.dimensions());
    assert_eq!(on.as_raw(), off.as_raw(), "default config must be byte-identical to disabled on pure-scroll input");
}
```

- [ ] **Step 2: Run the regression tests**

Run: `rtk cargo test -p rollshot-core --test static_region`
Expected: both new tests PASS, all previous static_region tests still PASS.

If `no_sticky_baseline_output_byte_identical_to_disabled_config` fails, the detector locked with a non-None band on pure-scroll content (false positive). Do NOT loosen `motion_margin`; instead:
- inspect which edge fired with `dbg!(s_on.static_detector...)` (not possible publicly — temporarily add a `pub(crate) fn debug_mask` or use a `println!` inside the detector when reaching lock state, then revert);
- if the false positive comes from `make_scroll_canvas`'s top / bottom rows being too uniform, increase the row variation in `make_scroll_canvas` (e.g. add a diagonal line crossing rows 0..H so every row has non-zero motion-aligned MAD).

- [ ] **Step 3: Commit**

```bash
rtk git add crates/rollshot-core/tests/static_region.rs
rtk git commit -m "$(cat <<'EOF'
test(core): regression gates for static_region

Two assertions guard the v0.2.1 contract: (1) disabling via
config reproduces v0.2 behavior pixel-for-pixel, (2) on a
pure-scroll fixture the default-on detector must lock with
all-None bands so output stays byte-identical to v0.2.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 15: Final verification — fmt, clippy, full workspace test

- [ ] **Step 1: Run `cargo fmt --check`**

Run: `rtk cargo fmt --check`
Expected: clean (no output). If it complains, run `rtk cargo fmt` and stage the formatting changes as part of the cleanup commit at the end.

- [ ] **Step 2: Run clippy with `-D warnings`**

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings. Common things to fix:
- unused `_mask` placeholders (now used; remove the underscore — but only if the parameter is actually used in that function);
- needless `clone()` on `StaticRegionConfig` if you didn't derive `Clone` properly (the struct must `#[derive(Clone)]`);
- match the precision style of `4.0f32 / 255.0`.

If clippy fixes were needed, commit them:

```bash
rtk git add -A
rtk git commit -m "$(cat <<'EOF'
chore(core): satisfy fmt + clippy on v0.2.1 static_region work

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 3: Run the full workspace test suite**

Run: `rtk cargo test --workspace`
Expected: all crates PASS. The `rollshot-capture` and `rollshot-cli` crates have no dependency on the new code path and should be unaffected.

- [ ] **Step 4: Confirm git status is clean**

Run: `rtk git status`
Expected: `clean — nothing to commit`. If anything is uncommitted, decide whether it belongs in a follow-up commit or should be reverted.

- [ ] **Step 5: Optional — push to remote**

If the team wants the branch pushed: `rtk git push -u origin <branch-name>`. Do not push without explicit confirmation — v0.2.1 may want to land on a dedicated branch.

---

## Closing Notes

- The acceptance checklist in `docs/superpowers/specs/2026-05-22-rollshot-static-region-mask-design.md#acceptance-criteria` is the authoritative tick-list. Each box maps to one or more tasks above; trace before declaring v0.2.1 done.
- If during integration any v0.2 fixture (e.g. `sticky_header_frames_still_append_expected_amount`) starts failing due to the detector activating, the fix is to ensure the existing fixture's sticky band is detectable AND its mask application does not change the *motion estimate* (matcher path is untouched). If a fixture asserts on output pixels in the sticky region, update its assertions to expect bg-fill.
- Performance: nothing in this plan should change the matcher's structural budget. If `large_pair_stays_within_structural_search_budget` ever fails because of this work, the regression is in `Stitcher::push_frame`'s shape (e.g. an accidental `clone()` of the anchor frame). Inspect that path first.
