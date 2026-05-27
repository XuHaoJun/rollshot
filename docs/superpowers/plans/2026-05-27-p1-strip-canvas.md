# P1 StripCanvas Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace eager growing `LinearCanvas` with primary `StripCanvas`, preserving byte-identical stitch output while reducing append copy cost for long screenshots.

**Architecture:** `StripCanvas` stores first frame and appended slices as paste-ordered strips, composes a cached full `RgbaImage` only when `full_image()`/`image()` is requested, and invalidates that cache on append. Because each strip redundantly retains its overlap region (a slow-scroll `Bottom` strip stores `frame_h/2` rows but nets only `slice_px`), strips are compacted into a single base strip once total strip bytes exceed `COMPACT_FACTOR * logical_bytes`; this bounds resident memory at `~COMPACT_FACTOR * logical` while keeping append `O(frame_h)` amortized. `Stitcher` owns `Option<StripCanvas>` and changes `full_image()` to `&mut self` because lazy composition mutates cache state.

**Tech Stack:** Rust, `image::RgbaImage`, `image::imageops::crop_imm`, existing `rollshot-core` test fixtures, existing stitch sequence benchmark harness.

---

## File Structure

- Modify `crates/rollshot-core/src/canvas.rs`
  - Replace production `LinearCanvas` with `StripCanvas`.
  - Add `CanvasStrip`, `overlay_copy`, cache invalidation, metrics accessors.
  - Keep a test-only `LegacyLinearCanvas` helper inside `#[cfg(test)] mod tests`.
- Modify `crates/rollshot-core/src/lib.rs`
  - Export `StripCanvas` instead of `LinearCanvas`.
- Modify `crates/rollshot-core/src/stitcher.rs`
  - Store `Option<StripCanvas>`.
  - Change `full_image(&mut self) -> Option<&RgbaImage>`.
- Modify core tests:
  - `crates/rollshot-core/tests/canvas.rs`
  - `crates/rollshot-core/tests/stitcher.rs`
  - `crates/rollshot-core/tests/overlap_topology.rs`
  - `crates/rollshot-core/tests/golden_fixtures.rs`
  - `crates/rollshot-core/tests/metrics_population.rs`
- Modify bench runner:
  - `crates/rollshot-core/benches/stitch_sequences.rs`
- Modify Tauri session call sites:
  - `crates/rollshot-app/src-tauri/src/session.rs`

## Task 1: Add StripCanvas Equivalence Tests

**Files:**
- Modify: `crates/rollshot-core/src/canvas.rs`

- [ ] **Step 1: Add failing tests for strip-vs-legacy behavior**

Inside `#[cfg(test)] mod tests`, keep the existing tests and add these helpers/tests near the bottom. These tests intentionally reference `StripCanvas` before it exists.

```rust
    fn patterned(width: u32, height: u32, seed: u8) -> RgbaImage {
        let mut img = RgbaImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                img.put_pixel(
                    x,
                    y,
                    Rgba([
                        seed.wrapping_add((x * 3) as u8),
                        seed.wrapping_add((y * 5) as u8),
                        seed.wrapping_add(((x + y) * 7) as u8),
                        255,
                    ]),
                );
            }
        }
        img
    }

    fn assert_images_eq(left: &RgbaImage, right: &RgbaImage) {
        assert_eq!(left.dimensions(), right.dimensions());
        assert_eq!(left.as_raw(), right.as_raw());
    }

    fn assert_strip_matches_legacy(direction: AppendDirection, frames: &[RgbaImage], slices: &[u32]) {
        let mut legacy = LinearCanvas::new(frames[0].clone());
        let mut strip = StripCanvas::new(frames[0].clone());

        assert_images_eq(legacy.image(), strip.image());
        for (idx, slice_px) in slices.iter().copied().enumerate() {
            let frame = &frames[idx + 1];
            assert_eq!(
                legacy.append(direction, frame, slice_px),
                strip.append(direction, frame, slice_px)
            );
            assert_images_eq(legacy.image(), strip.image());
            assert_eq!(legacy.axis(), strip.axis());
            assert_eq!(legacy.width(), strip.width());
            assert_eq!(legacy.height(), strip.height());
        }
    }

    #[test]
    fn strip_canvas_matches_legacy_bottom_appends() {
        let frames = vec![
            patterned(9, 8, 1),
            patterned(9, 8, 11),
            patterned(9, 8, 31),
        ];
        assert_strip_matches_legacy(AppendDirection::Bottom, &frames, &[2, 5]);
    }

    #[test]
    fn strip_canvas_matches_legacy_top_prepends() {
        let frames = vec![
            patterned(9, 8, 2),
            patterned(9, 8, 12),
            patterned(9, 8, 32),
        ];
        assert_strip_matches_legacy(AppendDirection::Top, &frames, &[2, 5]);
    }

    #[test]
    fn strip_canvas_matches_legacy_right_appends() {
        let frames = vec![
            patterned(8, 9, 3),
            patterned(8, 9, 13),
            patterned(8, 9, 33),
        ];
        assert_strip_matches_legacy(AppendDirection::Right, &frames, &[2, 5]);
    }

    #[test]
    fn strip_canvas_matches_legacy_left_prepends() {
        let frames = vec![
            patterned(8, 9, 4),
            patterned(8, 9, 14),
            patterned(8, 9, 34),
        ];
        assert_strip_matches_legacy(AppendDirection::Left, &frames, &[2, 5]);
    }

    #[test]
    fn strip_canvas_full_image_cache_is_stable_and_invalidated() {
        let mut canvas = StripCanvas::new(patterned(6, 6, 5));
        let first = canvas.image().clone();
        assert_images_eq(&first, canvas.image());

        canvas
            .append(AppendDirection::Bottom, &patterned(6, 6, 25), 2)
            .unwrap();
        let after = canvas.image().clone();
        assert_ne!(first.as_raw(), after.as_raw());
        assert_images_eq(&after, canvas.image());
    }

    #[test]
    fn strip_canvas_append_copied_bytes_tracks_only_new_strip() {
        let mut canvas = StripCanvas::new(patterned(4, 8, 6));
        canvas
            .append(AppendDirection::Bottom, &patterned(4, 8, 26), 2)
            .unwrap();

        assert_eq!(canvas.width(), 4);
        assert_eq!(canvas.height(), 10);
        assert_eq!(canvas.last_append_copied_bytes(), 4 * 4 * 4);
        assert!(canvas.last_append_copied_bytes() < canvas.logical_pixels() * 4);
    }

    #[test]
    fn strip_canvas_compacts_to_keep_memory_bounded() {
        // Slow scroll: slice_px (4) << frame_h/2 (16), so each strip stores
        // ~16 rows but nets only 4. Without compaction, strip bytes grow to
        // several times the logical canvas (~3.5x here). Compaction must keep
        // resident strip+cache bytes within a small multiple of logical.
        let mut canvas = StripCanvas::new(patterned(8, 32, 7));
        for i in 0..40u8 {
            canvas
                .append(AppendDirection::Bottom, &patterned(8, 32, 50 + i), 4)
                .unwrap();
        }
        let logical_bytes = canvas.logical_pixels() * 4;
        assert!(
            canvas.allocated_bytes() <= logical_bytes * 3,
            "allocated {} should stay bounded vs logical {} (compaction not firing?)",
            canvas.allocated_bytes(),
            logical_bytes,
        );
        // Output must still be correct after compaction.
        assert_eq!(canvas.height(), 32 + 40 * 4);
    }

    #[test]
    fn strip_canvas_matches_legacy_repeated_top_prepends() {
        // Multiple prepends: each shifts all prior strips and overwrites the
        // overlap. Byte-equivalence must hold after every prepend, not just one.
        let frames = vec![
            patterned(9, 8, 2),
            patterned(9, 8, 12),
            patterned(9, 8, 22),
            patterned(9, 8, 32),
            patterned(9, 8, 42),
        ];
        assert_strip_matches_legacy(AppendDirection::Top, &frames, &[2, 3, 2, 3]);
    }

    #[test]
    fn strip_canvas_matches_legacy_mixed_directions_and_compaction() {
        // Slow bottom appends force at least one compaction (triggers at the
        // 5th append for these sizes), then a top prepend and a final bottom
        // append exercise direction changes *after* compaction. Output must
        // stay byte-identical to legacy through compaction and shifting.
        let base = patterned(8, 32, 9);
        let mut legacy = LinearCanvas::new(base.clone());
        let mut strip = StripCanvas::new(base);
        let mut ops: Vec<(AppendDirection, u8, u32)> =
            (0..30u8).map(|i| (AppendDirection::Bottom, 40 + i, 4)).collect();
        ops.push((AppendDirection::Top, 200, 3));
        ops.push((AppendDirection::Bottom, 210, 5));
        for (dir, seed, slice) in ops {
            let f = patterned(8, 32, seed);
            assert_eq!(legacy.append(dir, &f, slice), strip.append(dir, &f, slice));
            assert_images_eq(legacy.image(), strip.image());
            assert_eq!(legacy.width(), strip.width());
            assert_eq!(legacy.height(), strip.height());
        }
    }
```

- [ ] **Step 2: Run the focused test to verify RED**

Run:

```bash
rtk cargo test -p rollshot-core strip_canvas_matches_legacy_bottom_appends -- --exact
```

Expected: FAIL to compile with an error like `use of undeclared type StripCanvas`.

## Task 2: Implement StripCanvas In Canvas Module

**Files:**
- Modify: `crates/rollshot-core/src/canvas.rs`
- Modify: `crates/rollshot-core/src/lib.rs`

- [ ] **Step 1: Replace the production canvas struct**

Before replacing the production type, copy the current `LinearCanvas` struct and
impl into `#[cfg(test)] mod tests` as `LegacyLinearCanvas`. Then update **every**
`LinearCanvas` reference in the Task 1 tests to `LegacyLinearCanvas` — both the
`assert_strip_matches_legacy` helper and the inline mixed-direction test:

```rust
let mut legacy = LegacyLinearCanvas::new(frames[0].clone());
```

`LegacyLinearCanvas` must keep the old eager append methods exactly so
equivalence tests compare against the pre-P1 behavior.

In `crates/rollshot-core/src/canvas.rs`, change imports:

```rust
use std::collections::VecDeque;

use image::{imageops, RgbaImage};
```

Rename the production struct from `LinearCanvas` to `StripCanvas` and use this shape:

```rust
pub struct StripCanvas {
    axis: Option<ScrollAxis>,
    logical_width: u32,
    logical_height: u32,
    strips: VecDeque<CanvasStrip>,
    composed_cache: Option<RgbaImage>,
    last_append_copied_bytes: u64,
}

#[derive(Debug, Clone)]
struct CanvasStrip {
    image: RgbaImage,
    x: i64,
    y: i64,
}
```

`CanvasStrip` carries only what composition reads: the crop and its paste
position. `slice_px`/`overlap_px` are intentionally **not** stored — nothing
reads them (compose uses `x`/`y`; metrics use `last_append_copied_bytes`), so
keeping them would trip `clippy -D warnings` with "field is never read". The
overlap/slice quantities still exist as locals inside the append methods.

Implement the constructor/accessors:

```rust
impl StripCanvas {
    pub fn new(first_frame: RgbaImage) -> Self {
        let logical_width = first_frame.width();
        let logical_height = first_frame.height();
        let mut strips = VecDeque::new();
        strips.push_back(CanvasStrip {
            image: first_frame,
            x: 0,
            y: 0,
        });
        Self {
            axis: None,
            logical_width,
            logical_height,
            strips,
            composed_cache: None,
            last_append_copied_bytes: 0,
        }
    }

    pub fn image(&mut self) -> &RgbaImage {
        self.compose_if_needed();
        self.composed_cache.as_ref().expect("composed image")
    }

    pub fn into_image(mut self) -> RgbaImage {
        self.compose_if_needed();
        self.composed_cache.take().expect("composed image")
    }

    pub fn axis(&self) -> Option<ScrollAxis> {
        self.axis
    }

    pub fn width(&self) -> u32 {
        self.logical_width
    }

    pub fn height(&self) -> u32 {
        self.logical_height
    }

    pub fn allocated_bytes(&self) -> u64 {
        let strip_bytes: u64 = self
            .strips
            .iter()
            .map(|strip| strip.image.as_raw().len() as u64)
            .sum();
        let cache_bytes = self
            .composed_cache
            .as_ref()
            .map(|img| img.as_raw().len() as u64)
            .unwrap_or(0);
        strip_bytes + cache_bytes
    }

    pub fn logical_pixels(&self) -> u64 {
        self.logical_width as u64 * self.logical_height as u64
    }

    pub fn last_append_copied_bytes(&self) -> u64 {
        self.last_append_copied_bytes
    }
}
```

- [ ] **Step 2: Implement append validation**

Use the current validation semantics, replacing `self.image.width()/height()` with logical dimensions:

```rust
    pub fn append(
        &mut self,
        direction: AppendDirection,
        frame: &RgbaImage,
        slice_px: u32,
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
                if frame.width() != self.logical_width {
                    return Err(CanvasAppendError::DimensionMismatch {
                        canvas: self.logical_width,
                        frame: frame.width(),
                    });
                }
                if frame.height() > self.logical_height {
                    return Err(CanvasAppendError::DimensionMismatch {
                        canvas: self.logical_height,
                        frame: frame.height(),
                    });
                }
            }
            ScrollAxis::Horizontal => {
                if frame.height() != self.logical_height {
                    return Err(CanvasAppendError::DimensionMismatch {
                        canvas: self.logical_height,
                        frame: frame.height(),
                    });
                }
                if frame.width() > self.logical_width {
                    return Err(CanvasAppendError::DimensionMismatch {
                        canvas: self.logical_width,
                        frame: frame.width(),
                    });
                }
            }
        }

        if slice_px == 0 {
            return Err(CanvasAppendError::EmptyAppend);
        }

        let added = match direction {
            AppendDirection::Bottom => self.append_bottom(frame, slice_px),
            AppendDirection::Top => self.prepend_top(frame, slice_px),
            AppendDirection::Right => self.append_right(frame, slice_px),
            AppendDirection::Left => self.prepend_left(frame, slice_px),
        };

        self.axis = Some(target_axis);
        self.composed_cache = None;
        self.compact_if_needed();
        Ok(added)
    }
```

- [ ] **Step 3: Implement strip append/prepend methods**

Add the methods with paste-order semantics. New strips always `push_back`; top/left shift older strips first.

```rust
    fn append_bottom(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let frame_h = frame.height();
        let slice_px = slice_px.min(frame_h);
        let overlap_px = (frame_h / 2).saturating_sub(slice_px);
        let total_slice = (slice_px + overlap_px).min(frame_h);
        let crop = imageops::crop_imm(frame, 0, frame_h - total_slice, frame.width(), total_slice)
            .to_image();
        let paste_y = self.logical_height as i64 - overlap_px as i64;
        self.last_append_copied_bytes = crop.as_raw().len() as u64;
        self.strips.push_back(CanvasStrip {
            image: crop,
            x: 0,
            y: paste_y,
        });
        self.logical_height += slice_px;
        slice_px
    }

    fn prepend_top(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let frame_h = frame.height();
        let slice_px = slice_px.min(frame_h);
        let overlap_px = (frame_h / 2).saturating_sub(slice_px);
        let total_slice = (slice_px + overlap_px).min(frame_h);
        for strip in &mut self.strips {
            strip.y += slice_px as i64;
        }
        let crop = imageops::crop_imm(frame, 0, 0, frame.width(), total_slice).to_image();
        self.last_append_copied_bytes = crop.as_raw().len() as u64;
        self.strips.push_back(CanvasStrip {
            image: crop,
            x: 0,
            y: 0,
        });
        self.logical_height += slice_px;
        slice_px
    }

    fn append_right(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let frame_w = frame.width();
        let slice_px = slice_px.min(frame_w);
        let overlap_px = (frame_w / 2).saturating_sub(slice_px);
        let total_slice = (slice_px + overlap_px).min(frame_w);
        let crop = imageops::crop_imm(frame, frame_w - total_slice, 0, total_slice, frame.height())
            .to_image();
        let paste_x = self.logical_width as i64 - overlap_px as i64;
        self.last_append_copied_bytes = crop.as_raw().len() as u64;
        self.strips.push_back(CanvasStrip {
            image: crop,
            x: paste_x,
            y: 0,
        });
        self.logical_width += slice_px;
        slice_px
    }

    fn prepend_left(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let frame_w = frame.width();
        let slice_px = slice_px.min(frame_w);
        let overlap_px = (frame_w / 2).saturating_sub(slice_px);
        let total_slice = (slice_px + overlap_px).min(frame_w);
        for strip in &mut self.strips {
            strip.x += slice_px as i64;
        }
        let crop = imageops::crop_imm(frame, 0, 0, total_slice, frame.height()).to_image();
        self.last_append_copied_bytes = crop.as_raw().len() as u64;
        self.strips.push_back(CanvasStrip {
            image: crop,
            x: 0,
            y: 0,
        });
        self.logical_width += slice_px;
        slice_px
    }
```

- [ ] **Step 4: Implement lazy composition and raw row copy**

Add:

```rust
    fn compose_if_needed(&mut self) {
        if self.composed_cache.is_some() {
            return;
        }
        let mut out = RgbaImage::new(self.logical_width, self.logical_height);
        for strip in &self.strips {
            overlay_copy(&mut out, &strip.image, strip.x, strip.y);
        }
        self.composed_cache = Some(out);
    }

    /// Collapse strips into a single base strip once their redundant overlap
    /// retention pushes total strip bytes past `COMPACT_FACTOR * logical`. This
    /// bounds resident memory while keeping append `O(frame_h)` amortized.
    fn compact_if_needed(&mut self) {
        let logical_bytes = self.logical_pixels() * 4;
        let strip_bytes: u64 = self
            .strips
            .iter()
            .map(|strip| strip.image.as_raw().len() as u64)
            .sum();
        if strip_bytes <= logical_bytes.saturating_mul(COMPACT_FACTOR) {
            return;
        }
        self.compose_if_needed();
        let base = self.composed_cache.take().expect("composed image");
        self.strips.clear();
        self.strips.push_back(CanvasStrip {
            image: base,
            x: 0,
            y: 0,
        });
    }
```

Add the compaction threshold as a module-level const near the top of
`canvas.rs` (next to the imports):

```rust
/// Compact strips into a single base strip once their combined byte size
/// exceeds this multiple of the logical canvas. `2` bounds resident memory at
/// roughly the same level as the old eager `LinearCanvas` while preserving the
/// `O(frame_h)` amortized append cost.
const COMPACT_FACTOR: u64 = 2;
```

Add this free function below the impl:

```rust
fn overlay_copy(dst: &mut RgbaImage, src: &RgbaImage, x: i64, y: i64) {
    let dst_w = dst.width() as i64;
    let dst_h = dst.height() as i64;
    let src_w = src.width() as i64;
    let src_h = src.height() as i64;

    let copy_x0 = x.max(0);
    let copy_x1 = (x + src_w).min(dst_w);
    if copy_x1 <= copy_x0 {
        return;
    }
    let sx0 = (copy_x0 - x) as usize;
    let len_px = (copy_x1 - copy_x0) as usize;
    let len = len_px * 4;

    for sy in 0..src_h {
        let dy = y + sy;
        if dy < 0 || dy >= dst_h {
            continue;
        }
        let src_start = ((sy as usize * src.width() as usize) + sx0) * 4;
        let dst_start = ((dy as usize * dst.width() as usize) + copy_x0 as usize) * 4;
        dst.as_mut()[dst_start..dst_start + len]
            .copy_from_slice(&src.as_raw()[src_start..src_start + len]);
    }
}
```

- [ ] **Step 5: Update public export**

In `crates/rollshot-core/src/lib.rs`, change:

```rust
pub use canvas::{CanvasAppendError, LinearCanvas};
```

to:

```rust
pub use canvas::{CanvasAppendError, StripCanvas};
```

- [ ] **Step 6: Run focused canvas tests**

Run:

```bash
rtk cargo test -p rollshot-core --lib strip_canvas
```

Expected: library canvas tests compile and pass.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-core/src/canvas.rs crates/rollshot-core/src/lib.rs
rtk git commit -m "feat(core): add strip-backed canvas"
```

## Task 3: Update Core Call Sites For StripCanvas And Mutable Full Image

**Files:**
- Modify: `crates/rollshot-core/src/stitcher.rs`
- Modify: `crates/rollshot-core/tests/canvas.rs`
- Modify: `crates/rollshot-core/tests/stitcher.rs`
- Modify: `crates/rollshot-core/tests/overlap_topology.rs`
- Modify: `crates/rollshot-core/tests/golden_fixtures.rs`
- Modify: `crates/rollshot-core/tests/metrics_population.rs`
- Modify: `crates/rollshot-core/benches/stitch_sequences.rs`

- [ ] **Step 1: Update `Stitcher` to own `StripCanvas`**

In `crates/rollshot-core/src/stitcher.rs`, change imports and field type:

```rust
use crate::canvas::{CanvasAppendError, StripCanvas};
```

```rust
canvas: Option<StripCanvas>,
```

Change first-frame initialization:

```rust
self.canvas = Some(StripCanvas::new(frame));
```

Change `full_image`:

```rust
pub fn full_image(&mut self) -> Option<&RgbaImage> {
    self.canvas.as_mut().map(StripCanvas::image)
}
```

- [ ] **Step 2: Update external canvas tests**

In `crates/rollshot-core/tests/canvas.rs`, change:

```rust
use rollshot_core::{AppendDirection, LinearCanvas, ScrollAxis};
```

to:

```rust
use rollshot_core::{AppendDirection, ScrollAxis, StripCanvas};
```

Then replace `LinearCanvas::new(` with `StripCanvas::new(`.

Because `StripCanvas::image()` composes lazily and takes `&mut self`, all existing test bindings are already mutable.

- [ ] **Step 3: Update core full_image tests**

In these files, ensure every stitcher binding used for `full_image()` is mutable:

```text
crates/rollshot-core/tests/stitcher.rs
crates/rollshot-core/tests/overlap_topology.rs
crates/rollshot-core/tests/golden_fixtures.rs
crates/rollshot-core/tests/metrics_population.rs
```

Use this replacement pattern:

```rust
let mut stitcher = Stitcher::new(StitchConfig::default());
```

For chained calls like:

```rust
let stitched = Stitcher::new(config)
    .full_image()
    .expect("stitched");
```

replace with:

```rust
let mut stitcher = Stitcher::new(config);
let stitched = stitcher.full_image().expect("stitched");
```

- [ ] **Step 4: Update bench runner full_image call**

In `crates/rollshot-core/benches/stitch_sequences.rs`, keep the existing `let mut stitcher = Stitcher::new(...)` binding and change no logic. The existing line should compile once `full_image()` takes `&mut self`:

```rust
let stitched: Option<RgbaImage> = stitcher.full_image().cloned();
```

- [ ] **Step 5: Run core tests**

Run:

```bash
rtk cargo test -p rollshot-core
```

Expected: all `rollshot-core` tests pass.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-core/src/stitcher.rs crates/rollshot-core/tests crates/rollshot-core/benches/stitch_sequences.rs
rtk git commit -m "refactor(core): switch stitcher to strip canvas"
```

## Task 4: Update Tauri Session Preview And Finish Call Sites

**Files:**
- Modify: `crates/rollshot-app/src-tauri/src/session.rs`

- [ ] **Step 1: Update finish path to take mutable stitcher**

Change:

```rust
let stitcher = self
    .stitcher
    .take()
    .ok_or_else(|| "stitching has not started".to_string())?;
let image = stitcher
    .full_image()
    .ok_or_else(|| "stitcher produced no output".to_string())?
    .clone();
```

to:

```rust
let mut stitcher = self
    .stitcher
    .take()
    .ok_or_else(|| "stitching has not started".to_string())?;
let image = stitcher
    .full_image()
    .ok_or_else(|| "stitcher produced no output".to_string())?
    .clone();
```

- [ ] **Step 2: Update preview path to lock mutably**

Change:

```rust
let inner = self
    .inner
    .lock()
    .map_err(|_| "session lock poisoned".to_string())?;
inner
    .stitcher
    .as_ref()
    .and_then(|s| s.full_image())
    .cloned()
```

to:

```rust
let mut inner = self
    .inner
    .lock()
    .map_err(|_| "session lock poisoned".to_string())?;
inner
    .stitcher
    .as_mut()
    .and_then(|s| s.full_image())
    .cloned()
```

- [ ] **Step 3: Run Tauri/core compile checks**

Run:

```bash
rtk cargo test -p rollshot-core
rtk cargo check -p rollshot-app
```

Expected: both commands pass.

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-app/src-tauri/src/session.rs
rtk git commit -m "refactor(app): adapt previews to mutable full image"
```

## Task 5: Full Verification Before Benchmark

**Files:**
- No source changes expected.

- [ ] **Step 1: Run formatting**

```bash
rtk cargo fmt --check
```

Expected: passes.

- [ ] **Step 2: Run Rust tests**

```bash
rtk cargo test
```

Expected: all Rust tests pass.

- [ ] **Step 3: Run clippy**

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: no warnings.

- [ ] **Step 4: Run frontend checks after app session changes**

Because `session.rs` changed app-facing preview behavior, run:

```bash
rtk pnpm --dir crates/rollshot-app run typecheck
```

Expected: typecheck passes.

- [ ] **Step 5: Commit only if verification required code fixes**

If previous steps forced source edits, commit them:

```bash
rtk git status --short
rtk git add crates/rollshot-core crates/rollshot-app/src-tauri/src/session.rs
rtk git commit -m "fix: address strip canvas verification issues"
```

If no source edits were needed, do not create an empty commit.

## Task 6: P1 Performance Verification

**Files:**
- Read: `bench-results/2026-05-27-p1-strip-canvas-before-f404e61.jsonl`
- Create: `bench-results/2026-05-27-p1-strip-canvas-after.jsonl`
- Create: `bench-results/2026-05-27-p1-strip-canvas-compare.md`

- [ ] **Step 1: Check for saved baseline first**

Run:

```bash
rtk test -f bench-results/2026-05-27-p1-strip-canvas-before-f404e61.jsonl
```

Expected: exits 0.

If it exits non-zero, stop and ask:

```text
The P1 baseline JSONL is missing from bench-results/. Do you have a backup copy of bench-results/2026-05-27-p1-strip-canvas-before-f404e61.jsonl that you can restore before I run the comparison?
```

Do not run a before/after comparison until the user restores the file or explicitly says the backup is lost.

- [ ] **Step 2: Run after benchmark**

If the baseline exists, run:

```bash
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
  --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter \
  --repeats 3 \
  --out bench-results/2026-05-27-p1-strip-canvas-after.jsonl
```

Expected: `9 worker run(s), 0 failed`.

If the baseline backup is explicitly lost, run the same after benchmark anyway and record in the final report that no before/after comparison was possible.

- [ ] **Step 3: Compare against baseline**

Only when the baseline JSONL exists, run:

```bash
rtk python3 scripts/bench/compare.py \
  bench-results/2026-05-27-p1-strip-canvas-before-f404e61.jsonl \
  bench-results/2026-05-27-p1-strip-canvas-after.jsonl \
  > bench-results/2026-05-27-p1-strip-canvas-compare.md
```

Expected: compare markdown is written.

- [ ] **Step 4: Inspect benchmark outputs**

Run:

```bash
rtk sed -n '1,220p' bench-results/2026-05-27-p1-strip-canvas-compare.md
```

Expected: report includes append, total, prepare, NCC, verifier sections. Confirm P1-specific values:

- `p95_append_us` improves on `long_vertical_text` and `long_vertical_jitter`.
- `append_copied_bytes` no longer scales with final canvas size.
- `peak_rss_kb_delta` stays bounded — comparable to the baseline, not a multiple
  of it. Compaction keeps resident strips at `~2x logical`; a regression to
  several times the baseline means compaction is not firing and must be fixed,
  not explained away.
- `p99`/`max_append_us` may show occasional compaction spikes; note them rather
  than treating them as regressions.
- output hash/correctness drift is absent or explicitly explained.

- [ ] **Step 5: Commit source changes are already complete**

Do not commit `bench-results/` unless the user explicitly asks to version benchmark artifacts. Leave benchmark files untracked for backup/reporting.

## Plan Self-Review Checklist

- Spec coverage:
  - Primary `StripCanvas`: Task 2.
  - Mutable `full_image`: Tasks 3 and 4.
  - Byte-identical topology tests: Task 1 and Task 2.
  - Bounded memory via compaction: `compact_if_needed` in Task 2; bounded-memory
    test in Task 1; RSS expectation in Task 6.
  - No P2/P3 matcher work: no task touches matcher preparation or NCC.
  - Benchmark baseline lookup gate: Task 6.
- Placeholder scan:
  - No incomplete steps.
  - All commands include expected outcomes.
- Type consistency:
  - Production type is `StripCanvas`.
  - `full_image()` takes `&mut self`.
  - `image()` on `StripCanvas` takes `&mut self` for lazy cache composition.
