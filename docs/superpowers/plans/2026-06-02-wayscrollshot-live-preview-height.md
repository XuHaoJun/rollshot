# Wayscrollshot-Style Live Preview Height Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make vertical stitching live previews use fixed 280px width and grow in height with scaled stitched content until capped by crop height and placement band space.

**Architecture:** Add a shared `growing_preview()` helper in `rollshot-overlay-core` that scales the full stitched image to a fixed width and crops the top/bottom slice when capped. Route vertical live preview calls on both macOS/webview and Linux native overlay through that helper, while preserving existing `viewport_preview()` behavior for horizontal Left/Right previews and final/save flows. Update the webview placement helper so placement sizing uses stitched content aspect, not crop aspect.

**Tech Stack:** Rust (`rollshot-overlay-core`, `rollshot-overlay`, Tauri session), TypeScript/React (`rollshot-app`), Vitest, Cargo tests

**Spec:** `docs/superpowers/specs/2026-06-02-wayscrollshot-live-preview-height-design.md`

---

## File Map

| File | Action | Responsibility |
| --- | --- | --- |
| `crates/rollshot-overlay-core/src/preview.rs` | Modify | Add `GrowingPreviewRequest`, `GrowingPreview`, and `growing_preview()`; keep `viewport_preview()` unchanged |
| `crates/rollshot-app/src-tauri/src/session.rs` | Modify | Use `growing_preview()` for vertical live preview PNGs; keep `viewport_preview()` for Left/Right |
| `crates/rollshot-overlay/src/overlay.rs` | Modify | Replace crop-aspect preview sizing with band-aware preview constraints |
| `crates/rollshot-overlay/src/driver.rs` | Modify | Pass constraints to the stitch thread and use growing preview for vertical accepted updates |
| `crates/rollshot-app/src/overlay/placement.ts` | Modify | Size dynamic preview from stitched content aspect and candidate band space |
| `crates/rollshot-app/src/overlay/placement.test.ts` | Modify | Cover fixed-width/growing-height placement and caps |
| `crates/rollshot-app/src/components/CaptureOverlay.tsx` | Modify | Pass stitched content size into placement for both polling and display |
| `crates/rollshot-app/src/components/CaptureOverlay.test.tsx` | Modify | Cover request height growth and caps in macOS/webview path |

---

### Task 1: Shared Core Growing Preview

**Files:**
- Modify: `crates/rollshot-overlay-core/src/preview.rs`

- [ ] **Step 1: Add failing core tests for growing preview**

In `crates/rollshot-overlay-core/src/preview.rs`, update the test import:

```rust
use super::{growing_preview, GrowingPreviewRequest, viewport_preview, ViewportPreviewRequest};
```

Add these tests before `viewport_preview_bottom_edge_shows_bottom_rows`:

```rust
#[test]
fn growing_preview_scales_full_image_to_fixed_width() {
    let mut stitcher = rollshot_core::Stitcher::new(rollshot_core::StitchConfig::default());
    assert_eq!(
        stitcher.push_frame(numbered_rows(20, 40)),
        rollshot_core::StitchOutcome::FirstFrame
    );

    let preview = growing_preview(
        &mut stitcher,
        GrowingPreviewRequest {
            fixed_width: 10,
            max_height: 100,
            edge: CapturedEdge::Bottom,
        },
    )
    .expect("growing preview");

    assert_eq!((preview.width, preview.height), (10, 20));
    assert_eq!(preview.scaled_full_height, 20);
    assert_eq!((preview.total_width, preview.total_height), (20, 40));
}

#[test]
fn growing_preview_caps_height_and_returns_bottom_slice() {
    let mut stitcher = rollshot_core::Stitcher::new(rollshot_core::StitchConfig::default());
    assert_eq!(
        stitcher.push_frame(numbered_rows(10, 60)),
        rollshot_core::StitchOutcome::FirstFrame
    );

    let preview = growing_preview(
        &mut stitcher,
        GrowingPreviewRequest {
            fixed_width: 10,
            max_height: 20,
            edge: CapturedEdge::Bottom,
        },
    )
    .expect("growing preview");

    assert_eq!((preview.width, preview.height), (10, 20));
    assert_eq!(preview.scaled_full_height, 60);
    assert_eq!(preview.pixels[0], 40, "bottom slice starts at scaled row 40");
}

#[test]
fn growing_preview_top_edge_returns_top_slice() {
    let mut stitcher = rollshot_core::Stitcher::new(rollshot_core::StitchConfig::default());
    assert_eq!(
        stitcher.push_frame(numbered_rows(10, 60)),
        rollshot_core::StitchOutcome::FirstFrame
    );

    let preview = growing_preview(
        &mut stitcher,
        GrowingPreviewRequest {
            fixed_width: 10,
            max_height: 20,
            edge: CapturedEdge::Top,
        },
    )
    .expect("growing preview");

    assert_eq!((preview.width, preview.height), (10, 20));
    assert_eq!(preview.pixels[0], 0, "top slice starts at scaled row 0");
}

#[test]
fn growing_preview_clamps_zero_requested_size_to_one_pixel() {
    let mut stitcher = rollshot_core::Stitcher::new(rollshot_core::StitchConfig::default());
    assert_eq!(
        stitcher.push_frame(numbered_rows(20, 20)),
        rollshot_core::StitchOutcome::FirstFrame
    );

    let preview = growing_preview(
        &mut stitcher,
        GrowingPreviewRequest {
            fixed_width: 0,
            max_height: 0,
            edge: CapturedEdge::Bottom,
        },
    )
    .expect("growing preview");

    assert_eq!((preview.width, preview.height), (1, 1));
}
```

- [ ] **Step 2: Run core preview tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-overlay-core growing_preview
```

Expected: FAIL because `growing_preview` and its request/response types do not exist.

- [ ] **Step 3: Implement `growing_preview()`**

In `crates/rollshot-overlay-core/src/preview.rs`, change the image import:

```rust
use image::{imageops, Rgba, RgbaImage};
```

Add these types after `ViewportPreview`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrowingPreviewRequest {
    pub fixed_width: u32,
    pub max_height: u32,
    pub edge: CapturedEdge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrowingPreview {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub scaled_full_height: u32,
    pub total_width: u32,
    pub total_height: u32,
}
```

Add this function before `viewport_preview()`:

```rust
/// Build a wayscrollshot-style live preview by scaling the full stitched image
/// to a fixed width, then cropping vertically to the requested height cap.
pub fn growing_preview(
    stitcher: &mut rollshot_core::Stitcher,
    request: GrowingPreviewRequest,
) -> Option<GrowingPreview> {
    let full = stitcher.full_image()?;
    let total_width = full.width().max(1);
    let total_height = full.height().max(1);
    let target_width = request.fixed_width.max(1);
    let max_height = request.max_height.max(1);
    let scale = target_width as f32 / total_width as f32;
    let scaled_height = ((total_height as f32 * scale).round() as u32).max(1);

    let resized = imageops::resize(full, target_width, scaled_height, imageops::FilterType::Triangle);
    let out_height = scaled_height.min(max_height);
    let y = if scaled_height <= out_height {
        0
    } else if matches!(request.edge, CapturedEdge::Top) {
        0
    } else {
        scaled_height - out_height
    };

    let cropped = imageops::crop_imm(&resized, 0, y, target_width, out_height).to_image();

    Some(GrowingPreview {
        width: cropped.width(),
        height: cropped.height(),
        pixels: cropped.into_raw(),
        scaled_full_height: scaled_height,
        total_width,
        total_height,
    })
}
```

- [ ] **Step 4: Run core preview tests to verify they pass**

Run:

```bash
rtk cargo test -p rollshot-overlay-core growing_preview
```

Expected: PASS.

- [ ] **Step 5: Run all overlay-core tests**

Run:

```bash
rtk cargo test -p rollshot-overlay-core
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```bash
rtk git add crates/rollshot-overlay-core/src/preview.rs
rtk git commit -m "feat(preview): add growing stitched preview helper"
```

---

### Task 2: Tauri Session Uses Growing Preview for Vertical Live Preview

**Files:**
- Modify: `crates/rollshot-app/src-tauri/src/session.rs`

- [ ] **Step 1: Add failing session tests**

In `crates/rollshot-app/src-tauri/src/session.rs`, replace the existing `stitch_preview_png_uses_shared_viewport` test with:

```rust
#[test]
fn stitch_preview_png_uses_growing_vertical_preview_height() {
    let session = SharedSession::new();
    {
        let mut inner = session.inner.lock().expect("session lock");
        inner.store_frame_for_test(make_test_frame(960, 600));
        inner
            .confirm_region(RegionDto {
                x: 0,
                y: 0,
                width: 960,
                height: 600,
            })
            .expect("confirm region");
        inner.start_stitching().expect("start stitching");
        inner
            .push_stitch_frame(make_test_frame(960, 600))
            .expect("push frame");
    }

    let bytes = session
        .stitch_preview_png(PREVIEW_WIDTH, PREVIEW_MAX_HEIGHT)
        .expect("encode stitch preview")
        .expect("preview exists");
    let image = image::load_from_memory(&bytes).expect("decode png");

    assert_eq!(image.width(), PREVIEW_WIDTH);
    assert_eq!(image.height(), 175);
}
```

Add this test after `stitch_preview_png_returns_requested_viewport_size`:

```rust
#[test]
fn horizontal_stitch_preview_keeps_requested_viewport_size() {
    let session = SharedSession::new();
    {
        let mut inner = session.inner.lock().expect("session lock");
        inner.store_frame_for_test(blank_frame(80, 80));
        inner
            .confirm_region(RegionDto {
                x: 0,
                y: 0,
                width: 80,
                height: 80,
            })
            .expect("confirm region");
        inner.start_stitching().expect("start stitching");
        inner.push_stitch_frame(scrolling_frame(0)).expect("f0");
        inner.spotlight_edge = CapturedEdge::Right;
    }

    let bytes = session
        .stitch_preview_png(180, 260)
        .expect("encode stitch preview")
        .expect("preview exists");
    let image = image::load_from_memory(&bytes).expect("decode png");

    assert_eq!((image.width(), image.height()), (180, 260));
}
```

- [ ] **Step 2: Run session preview tests to verify vertical test fails**

Run:

```bash
rtk cargo test -p rollshot-app --lib stitch_preview_png
```

Expected: FAIL for `stitch_preview_png_uses_growing_vertical_preview_height`; old code returns height `480`.

- [ ] **Step 3: Route vertical session previews through `growing_preview()`**

In `SharedSession::stitch_preview_png()`, replace the `preview` match block with:

```rust
let preview = match inner.stitcher.as_mut() {
    Some(stitcher) => region.and_then(|region| {
        if matches!(edge, CapturedEdge::Left | CapturedEdge::Right) {
            rollshot_overlay_core::preview::viewport_preview(
                stitcher,
                rollshot_overlay_core::preview::ViewportPreviewRequest {
                    viewport_width: preview_width,
                    viewport_height: preview_height,
                    frame_width: region.width,
                    frame_height: region.height,
                    edge,
                },
            )
            .map(|preview| (preview.width, preview.height, preview.pixels))
        } else {
            rollshot_overlay_core::preview::growing_preview(
                stitcher,
                rollshot_overlay_core::preview::GrowingPreviewRequest {
                    fixed_width: preview_width,
                    max_height: preview_height,
                    edge,
                },
            )
            .map(|preview| (preview.width, preview.height, preview.pixels))
        }
    }),
    None => None,
};
```

Then update the encoding match to consume the tuple:

```rust
match preview {
    Some((width, height, pixels)) => {
        let image = RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| "invalid stitch preview buffer".to_string())?;
        Ok(Some(encode_rgba_png(&image)?))
    }
    None => Ok(None),
}
```

- [ ] **Step 4: Run session preview tests**

Run:

```bash
rtk cargo test -p rollshot-app --lib stitch_preview_png
```

Expected: PASS.

- [ ] **Step 5: Commit**

Run:

```bash
rtk git add crates/rollshot-app/src-tauri/src/session.rs
rtk git commit -m "feat(app): use growing preview for vertical stitch PNGs"
```

---

### Task 3: Linux Native Overlay Uses Growing Preview Constraints

**Files:**
- Modify: `crates/rollshot-overlay/src/overlay.rs`
- Modify: `crates/rollshot-overlay/src/driver.rs`

- [ ] **Step 1: Rename Linux preview sizing tests to constraints tests**

In `crates/rollshot-overlay/src/overlay.rs`, rename:

```rust
fn preview_viewport_uses_fixed_width_and_bottom_band_height()
fn preview_viewport_clamps_width_to_side_band_and_preserves_aspect()
fn preview_viewport_caps_height_at_crop_height()
```

to:

```rust
fn preview_constraints_use_fixed_width_and_bottom_band_height()
fn preview_constraints_clamp_width_to_side_band()
fn preview_constraints_cap_height_at_crop_height()
```

Keep the existing assertions for now.

- [ ] **Step 2: Add a failing Linux driver test for growing vertical preview**

In `crates/rollshot-overlay/src/driver.rs`, update the test import:

```rust
use super::{
    overlay_stitch_config, preview_handle, should_emit_capture_miss, should_emit_preview,
    stitch_stream, PreviewConstraints,
};
```

Replace `viewport_handle_uses_requested_size` with:

```rust
#[test]
fn preview_handle_uses_growing_height_for_vertical_preview() {
    let mut stitcher = Stitcher::new(overlay_stitch_config());
    assert_eq!(
        stitcher.push_frame(scrolling_frame(0).image),
        StitchOutcome::FirstFrame
    );

    let handle = preview_handle(
        &mut stitcher,
        Region {
            x: 0,
            y: 0,
            width: 80,
            height: 80,
        },
        rollshot_overlay_core::capture_miss::CapturedEdge::Bottom,
        PreviewConstraints {
            fixed_width: 120,
            max_height: 180,
        },
    )
    .expect("growing preview for first frame");

    match handle {
        ImageHandle::Rgba { width, height, .. } => {
            assert_eq!(width, 120);
            assert_eq!(height, 120);
        }
        other => panic!("expected Rgba handle, got {other:?}"),
    }
}

#[test]
fn preview_handle_keeps_viewport_size_for_horizontal_preview() {
    let mut stitcher = Stitcher::new(overlay_stitch_config());
    assert_eq!(
        stitcher.push_frame(scrolling_frame(0).image),
        StitchOutcome::FirstFrame
    );

    let handle = preview_handle(
        &mut stitcher,
        Region {
            x: 0,
            y: 0,
            width: 80,
            height: 80,
        },
        rollshot_overlay_core::capture_miss::CapturedEdge::Right,
        PreviewConstraints {
            fixed_width: 120,
            max_height: 180,
        },
    )
    .expect("viewport preview for first frame");

    match handle {
        ImageHandle::Rgba { width, height, .. } => {
            assert_eq!(width, 120);
            assert_eq!(height, 180);
        }
        other => panic!("expected Rgba handle, got {other:?}"),
    }
}
```

- [ ] **Step 3: Run Linux overlay driver test to verify it fails**

Run:

```bash
rtk cargo test -p rollshot-overlay preview_handle
```

Expected: FAIL because `PreviewConstraints` and `preview_handle` do not exist yet.

- [ ] **Step 4: Replace viewport size with preview constraints in `overlay.rs`**

In `crates/rollshot-overlay/src/overlay.rs`, add this struct near the other constants/types:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreviewConstraints {
    fixed_width: u32,
    max_height: u32,
}
```

Rename `preview_viewport_size()` to `preview_constraints()` and replace the final fit call:

```rust
PreviewConstraints {
    fixed_width: max_width.floor().max(1.0) as u32,
    max_height: max_height.floor().max(1.0) as u32,
}
```

Remove `fit_preview_size_to_crop()` if it becomes unused by this file.

In `update()`, change:

```rust
let preview_size = preview_viewport_size(crop, ws);
driver.begin_stitch(crop_logical, overlay_logical, preview_size);
```

to:

```rust
let preview_constraints = preview_constraints(crop, ws);
driver.begin_stitch(crop_logical, overlay_logical, preview_constraints);
```

Update tests to assert constraints:

```rust
let constraints = preview_constraints(crop, window);
assert_eq!(constraints.fixed_width, PREVIEW_WIDTH);
assert_eq!(constraints.max_height, 382);
```

For `preview_constraints_clamp_width_to_side_band`, expect:

```rust
assert_eq!(constraints.fixed_width, 200);
assert_eq!(constraints.max_height, 1372);
```

For `preview_constraints_cap_height_at_crop_height`, expect:

```rust
assert_eq!(constraints.fixed_width, PREVIEW_WIDTH);
assert_eq!(constraints.max_height, 600);
```

- [ ] **Step 5: Update `driver.rs` to use constraints and growing preview**

In `crates/rollshot-overlay/src/driver.rs`, import:

```rust
use crate::overlay::PreviewConstraints;
```

Change `Driver::begin_stitch()` signature:

```rust
pub fn begin_stitch(
    &mut self,
    crop_logical: LogicalRect,
    overlay_logical: Size,
    preview_constraints: PreviewConstraints,
)
```

Pass `preview_constraints` into the thread closure and replace calls to `viewport_handle()` with:

```rust
if let Some(handle) = preview_handle(
    &mut stitcher,
    region,
    spotlight_edge,
    preview_constraints,
) {
    let _ = preview_tx.unbounded_send(LiveOverlayEvent::Preview(handle));
}
```

Rename `viewport_handle()` to `preview_handle()` and implement:

```rust
fn preview_handle(
    stitcher: &mut Stitcher,
    region: Region,
    edge: rollshot_overlay_core::capture_miss::CapturedEdge,
    constraints: PreviewConstraints,
) -> Option<ImageHandle> {
    if matches!(
        edge,
        rollshot_overlay_core::capture_miss::CapturedEdge::Left
            | rollshot_overlay_core::capture_miss::CapturedEdge::Right
    ) {
        let view = rollshot_overlay_core::preview::viewport_preview(
            stitcher,
            rollshot_overlay_core::preview::ViewportPreviewRequest {
                viewport_width: constraints.fixed_width,
                viewport_height: constraints.max_height,
                frame_width: region.width,
                frame_height: region.height,
                edge,
            },
        )?;
        return Some(ImageHandle::from_rgba(view.width, view.height, view.pixels));
    }

    let view = rollshot_overlay_core::preview::growing_preview(
        stitcher,
        rollshot_overlay_core::preview::GrowingPreviewRequest {
            fixed_width: constraints.fixed_width,
            max_height: constraints.max_height,
            edge,
        },
    )?;
    Some(ImageHandle::from_rgba(view.width, view.height, view.pixels))
}
```

- [ ] **Step 6: Run Linux overlay tests**

Run:

```bash
rtk cargo test -p rollshot-overlay preview_constraints
rtk cargo test -p rollshot-overlay preview_handle
rtk cargo test -p rollshot-overlay
```

Expected: PASS.

- [ ] **Step 7: Commit**

Run:

```bash
rtk git add crates/rollshot-overlay/src/overlay.rs crates/rollshot-overlay/src/driver.rs
rtk git commit -m "feat(overlay): grow native stitch preview vertically"
```

---

### Task 4: Webview Placement Uses Stitched Content Height

**Files:**
- Modify: `crates/rollshot-app/src/overlay/placement.ts`
- Modify: `crates/rollshot-app/src/overlay/placement.test.ts`

- [ ] **Step 1: Add failing placement tests**

In `crates/rollshot-app/src/overlay/placement.test.ts`, replace the `chooseDynamicPreviewPlacement` tests with:

```ts
describe('chooseDynamicPreviewPlacement', () => {
  it('uses fixed width and a short height when scaled content is short', () => {
    expect(
      chooseDynamicPreviewPlacement({
        bounds: { left: 0, top: 0, width: 1000, height: 900 },
        region: { left: 100, top: 100, width: 400, height: 300 },
        previewWidth: 280,
        content: { width: 400, height: 200 },
        overlayExclusion: 'unsupported',
        gap: 12,
      }),
    ).toEqual({
      mode: 'image',
      side: 'right',
      rect: { left: 512, top: 100, width: 280, height: 140 },
      preview: { width: 280, height: 140 },
    })
  })

  it('grows height with scaled content and caps at crop height', () => {
    expect(
      chooseDynamicPreviewPlacement({
        bounds: { left: 0, top: 0, width: 1000, height: 900 },
        region: { left: 100, top: 100, width: 400, height: 300 },
        previewWidth: 280,
        content: { width: 400, height: 600 },
        overlayExclusion: 'unsupported',
        gap: 12,
      }),
    ).toEqual({
      mode: 'image',
      side: 'right',
      rect: { left: 512, top: 100, width: 280, height: 300 },
      preview: { width: 280, height: 300 },
    })
  })

  it('caps side preview height to the available band before choosing placement', () => {
    expect(
      chooseDynamicPreviewPlacement({
        bounds: { left: 0, top: 0, width: 1000, height: 700 },
        region: { left: 100, top: 450, width: 400, height: 300 },
        previewWidth: 280,
        content: { width: 400, height: 600 },
        overlayExclusion: 'unsupported',
        gap: 12,
      }),
    ).toEqual({
      mode: 'image',
      side: 'right',
      rect: { left: 512, top: 450, width: 280, height: 250 },
      preview: { width: 280, height: 250 },
    })
  })

  it('uses verified inside placement with growing preview height', () => {
    expect(
      chooseDynamicPreviewPlacement({
        bounds: { left: 0, top: 0, width: 1000, height: 700 },
        region: { left: 0, top: 0, width: 1000, height: 700 },
        previewWidth: 280,
        content: { width: 1000, height: 1400 },
        overlayExclusion: 'verified',
        gap: 12,
      }),
    ).toEqual({
      mode: 'image',
      side: 'inside',
      rect: { left: 708, top: 12, width: 280, height: 392 },
      preview: { width: 280, height: 392 },
    })
  })
})
```

- [ ] **Step 2: Run placement tests to verify they fail**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- placement.test
```

Expected: FAIL because `chooseDynamicPreviewPlacement()` does not accept `content` and still uses crop aspect.

- [ ] **Step 3: Update placement helper types**

In `crates/rollshot-app/src/overlay/placement.ts`, add:

```ts
type ContentSize = {
  width: number
  height: number
}
```

Change `DynamicPlacementInput`:

```ts
type DynamicPlacementInput = {
  bounds: OverlayRect
  region: OverlayRect
  previewWidth: number
  content: ContentSize
  overlayExclusion: OverlayExclusion
  gap?: number
}
```

Add helper functions before `chooseDynamicPreviewPlacement()`:

```ts
function dynamicPreviewSize(input: {
  content: ContentSize
  previewWidth: number
  maxWidth: number
  maxHeight: number
  cropHeight: number
}): PreviewSize {
  const width = Math.max(1, Math.floor(Math.min(input.previewWidth, input.maxWidth)))
  const contentWidth = Math.max(1, input.content.width)
  const contentHeight = Math.max(1, input.content.height)
  const scaledHeight = Math.max(1, Math.round((contentHeight * width) / contentWidth))
  const height = Math.max(
    1,
    Math.floor(Math.min(scaledHeight, input.cropHeight, input.maxHeight)),
  )
  return { width, height }
}
```

- [ ] **Step 4: Use content-aware sizing for all dynamic candidates**

In each side candidate loop, replace the old `fitPreviewSizeToRegion()` call with:

```ts
const preview = dynamicPreviewSize({
  content,
  previewWidth,
  maxWidth: availWidth,
  maxHeight: availHeight,
  cropHeight: region.height,
})
```

In the verified inside block, replace the old `fitPreviewSizeToRegion()` call with:

```ts
const preview = dynamicPreviewSize({
  content,
  previewWidth,
  maxWidth: insideAvailWidth,
  maxHeight: insideAvailHeight,
  cropHeight: region.height,
})
```

Keep `fitPreviewSizeToRegion()` and `choosePreviewPlacement()` unchanged for existing fixed-size tests.

- [ ] **Step 5: Run placement tests**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- placement.test
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```bash
rtk git add crates/rollshot-app/src/overlay/placement.ts crates/rollshot-app/src/overlay/placement.test.ts
rtk git commit -m "feat(app): size preview placement from stitched content"
```

---

### Task 5: CaptureOverlay Requests Growing Preview Dimensions

**Files:**
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.tsx`
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.test.tsx`

- [ ] **Step 1: Add failing CaptureOverlay test for growing request height**

In `crates/rollshot-app/src/components/CaptureOverlay.test.tsx`, add this test after `shows capture miss toast...`:

```tsx
it('requests taller stitch previews as vertical stitched content grows', async () => {
  api.sessionStatus
    .mockResolvedValueOnce({
      state: 'stitching',
      frame_width: 1000,
      frame_height: 500,
      region: { x: 100, y: 50, width: 800, height: 400 },
      stats: { frame_count: 1, total_width: 800, total_height: 400, last_append: 0 },
      last_outcome: 'first frame',
      capture_miss: false,
      capture_miss_warning: false,
      capture_miss_edge: 'unknown',
      capture_miss_message: '',
    } satisfies SessionStatus)
    .mockResolvedValue({
      state: 'stitching',
      frame_width: 1000,
      frame_height: 500,
      region: { x: 100, y: 50, width: 800, height: 400 },
      stats: { frame_count: 3, total_width: 800, total_height: 900, last_append: 200 },
      last_outcome: 'appended 200px Bottom',
      capture_miss: false,
      capture_miss_warning: false,
      capture_miss_edge: 'unknown',
      capture_miss_message: '',
    } satisfies SessionStatus)
  api.getStitchPreview.mockResolvedValue(new Blob(['png'], { type: 'image/png' }))

  act(() => root.render(<CaptureOverlay />))
  await flush()
  await act(async () => {
    await vi.advanceTimersByTimeAsync(160)
  })
  await act(async () => {
    await vi.advanceTimersByTimeAsync(160)
  })

  expect(api.getStitchPreview).toHaveBeenNthCalledWith(1, 280, 140)
  expect(api.getStitchPreview).toHaveBeenNthCalledWith(2, 280, 200)
})
```

- [ ] **Step 2: Update existing request expectations**

In `shows capture miss toast...`, change the expected preview request:

```ts
expect(api.getStitchPreview).toHaveBeenCalledWith(280, 200)
```

In `requests inside stitch preview dimensions after verified overlay exclusion resolves`, change:

```ts
expect(api.getStitchPreview).toHaveBeenCalledWith(280, 420)
```

- [ ] **Step 3: Run CaptureOverlay tests to verify failures**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- CaptureOverlay.test
```

Expected: FAIL because `CaptureOverlay` still calls placement with crop-only sizing.

- [ ] **Step 4: Add content-size derivation in `CaptureOverlay.tsx`**

In `CaptureOverlay.tsx`, add this helper near `waitForOverlayClear()`:

```ts
function stitchPreviewContentSize(status: Extract<SessionStatus, { state: 'stitching' }>) {
  const horizontalGrowth =
    status.stats.total_width > status.region.width &&
    status.stats.total_height <= status.region.height

  if (horizontalGrowth) {
    return { width: status.region.width, height: status.region.height }
  }

  return {
    width: Math.max(1, status.stats.total_width),
    height: Math.max(1, status.stats.total_height),
  }
}
```

- [ ] **Step 5: Pass content size in the polling placement call**

In the polling loop, change:

```ts
const dynamicPlacement = chooseDynamicPreviewPlacement({
  bounds,
  region: cssRegion,
  previewWidth: PREVIEW_WIDTH,
  overlayExclusion: overlayModeRef.current,
})
```

to:

```ts
const dynamicPlacement = chooseDynamicPreviewPlacement({
  bounds,
  region: cssRegion,
  previewWidth: PREVIEW_WIDTH,
  content: stitchPreviewContentSize(nextStatus),
  overlayExclusion: overlayModeRef.current,
})
```

- [ ] **Step 6: Pass content size in the display placement memo**

Before `const placement = useMemo(...)`, add:

```ts
const stitchPreviewContent = useMemo(() => {
  if (status.state !== 'stitching') return null
  return stitchPreviewContentSize(status)
}, [status])
```

Then change the placement memo guard:

```ts
if (!activeRegionRect || !stitchPreviewContent) {
  return { mode: 'status' } as const
}
```

and pass:

```ts
content: stitchPreviewContent,
```

Update the dependency list:

```ts
}, [activeRegionRect, overlayMode, stitchPreviewContent])
```

- [ ] **Step 7: Run CaptureOverlay tests**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- CaptureOverlay.test
```

Expected: PASS.

- [ ] **Step 8: Run full app tests**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test
```

Expected: PASS.

- [ ] **Step 9: Commit**

Run:

```bash
rtk git add crates/rollshot-app/src/components/CaptureOverlay.tsx crates/rollshot-app/src/components/CaptureOverlay.test.tsx
rtk git commit -m "feat(app): grow webview stitch preview height"
```

---

### Task 6: Final Verification

**Files:**
- Verify only

- [ ] **Step 1: Run Rust tests**

Run:

```bash
rtk cargo test
```

Expected: PASS.

- [ ] **Step 2: Run Rust formatting check**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 3: Run focused Rust clippy**

Run:

```bash
rtk cargo clippy -p rollshot-overlay-core -p rollshot-overlay -p rollshot-app --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Run frontend checks**

Run:

```bash
rtk pnpm --dir crates/rollshot-app run typecheck
rtk pnpm --dir crates/rollshot-app test
rtk pnpm --dir crates/rollshot-app run build
```

Expected: PASS.

- [ ] **Step 5: Manual runtime verification**

On macOS webview path:

1. Start an interactive capture.
2. Select a wide vertical crop.
3. Start stitching and scroll downward.
4. Confirm the live preview width stays fixed at 280px.
5. Confirm the live preview starts short and grows as accepted stitched content grows.
6. Confirm height stops growing once it reaches crop height or available band height.
7. Confirm full-screen crop with verified overlay exclusion still requests and displays an inside preview.

On Linux native overlay path:

1. Start native capture.
2. Select a wide vertical crop.
3. Scroll downward.
4. Confirm the preview image grows vertically in the outside chrome band.
5. Confirm horizontal/Left/Right captures still use the old viewport-thumbnail behavior.

- [ ] **Step 6: Commit any verification-only cleanup**

If formatting changed files, run:

```bash
rtk git status --short
rtk git add crates/rollshot-overlay-core/src/preview.rs crates/rollshot-overlay/src/overlay.rs crates/rollshot-overlay/src/driver.rs crates/rollshot-app/src-tauri/src/session.rs crates/rollshot-app/src/overlay/placement.ts crates/rollshot-app/src/overlay/placement.test.ts crates/rollshot-app/src/components/CaptureOverlay.tsx crates/rollshot-app/src/components/CaptureOverlay.test.tsx
rtk git commit -m "style: format growing preview changes"
```

If no files changed, do not create a commit.
