# Live Preview Viewport Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Rollshot's shrinking full-canvas live preview with a cross-platform viewport preview that stays readable during long captures.

**Architecture:** `rollshot-core` exposes a bounded canvas viewport primitive without depending on overlay UI types. `rollshot-overlay-core` owns the shared viewport preview renderer and position indicator. The Tauri/webview and iced/native overlay paths request the same shared preview and differ only in transport: PNG blob for Tauri, RGBA image handle for iced.

**Tech Stack:** Rust (`image`, `rollshot-core`, `rollshot-overlay-core`, iced image handles, Tauri commands), React/TypeScript for preview request dimensions, existing `rtk cargo` and `rtk pnpm` verification.

---

## File Structure

- Modify `crates/rollshot-core/src/canvas.rs`
  - Add `CanvasViewport`.
  - Add `StripCanvas::viewport(x, y, width, height)`.
  - Add tests proving the viewport uses only the requested rectangle and clamps to canvas bounds.
- Modify `crates/rollshot-core/src/stitcher.rs`
  - Add `Stitcher::canvas_viewport(x, y, width, height)`.
- Modify `crates/rollshot-core/src/lib.rs`
  - Re-export `CanvasViewport`.
- Modify `crates/rollshot-overlay-core/src/preview.rs`
  - Add `ViewportPreviewRequest`, `ViewportPreview`, viewport rectangle selection, preview scaling, and indicator drawing.
  - Keep existing `PREVIEW_WIDTH` and `PREVIEW_MAX_HEIGHT` constants.
- Modify `crates/rollshot-app/src-tauri/src/session.rs`
  - Replace `stitch_preview_png()` internals with shared viewport preview.
  - Change it to accept requested preview width and height.
- Modify `crates/rollshot-app/src-tauri/src/commands.rs`
  - Add `preview_width` and `preview_height` args to `get_stitch_preview`.
- Modify `crates/rollshot-app/src/api/capture.ts`
  - Pass dimensions to `get_stitch_preview`.
- Modify `crates/rollshot-app/src/components/CaptureOverlay.tsx`
  - Request preview dimensions matching `PREVIEW_SIZE`.
- Modify `crates/rollshot-overlay/src/driver.rs`
  - Replace `spotlight_handle` with viewport preview handle generation.
  - Emit preview updates only on accepted stitch progress.
- Modify `crates/rollshot-overlay/src/overlay.rs`
  - Keep preview placement, but ensure viewport size remains the native preview request.
- Modify tests:
  - `crates/rollshot-overlay-core/src/preview.rs`
  - `crates/rollshot-app/src-tauri/src/session.rs`
  - `crates/rollshot-app/src/components/CaptureOverlay.test.tsx`
  - `crates/rollshot-app/src/api/capture.test.ts`
  - `crates/rollshot-overlay/src/driver.rs`
  - `crates/rollshot-overlay/src/overlay.rs`

---

### Task 1: Core Canvas Viewport Primitive

**Files:**
- Modify: `crates/rollshot-core/src/canvas.rs`
- Modify: `crates/rollshot-core/src/stitcher.rs`
- Modify: `crates/rollshot-core/src/lib.rs`

- [ ] **Step 1: Write failing core canvas viewport tests**

Add these tests inside the existing `#[cfg(test)] mod tests` in `crates/rollshot-core/src/canvas.rs`:

```rust
#[test]
fn strip_canvas_viewport_crops_requested_rect() {
    let mut canvas = StripCanvas::new(patterned(8, 8, 5));
    let view = canvas.viewport(2, 3, 3, 2).expect("viewport");

    assert_eq!((view.image.width(), view.image.height()), (3, 2));
    assert_eq!((view.total_width, view.total_height), (8, 8));
    assert_eq!((view.x, view.y), (2, 3));
    for y in 0..2 {
        for x in 0..3 {
            assert_eq!(
                view.image.get_pixel(x, y),
                canvas.image().get_pixel(x + 2, y + 3)
            );
        }
    }
}

#[test]
fn strip_canvas_viewport_clamps_to_canvas_bounds() {
    let mut canvas = StripCanvas::new(patterned(8, 8, 9));
    let view = canvas.viewport(6, 7, 8, 8).expect("viewport");

    assert_eq!((view.image.width(), view.image.height()), (2, 1));
    assert_eq!((view.total_width, view.total_height), (8, 8));
    assert_eq!((view.x, view.y), (6, 7));
}

#[test]
fn strip_canvas_viewport_returns_none_for_empty_rect() {
    let mut canvas = StripCanvas::new(patterned(8, 8, 11));

    assert!(canvas.viewport(0, 0, 0, 4).is_none());
    assert!(canvas.viewport(0, 0, 4, 0).is_none());
    assert!(canvas.viewport(8, 0, 1, 1).is_none());
    assert!(canvas.viewport(0, 8, 1, 1).is_none());
}
```

- [ ] **Step 2: Run failing core test**

Run:

```bash
rtk cargo test -p rollshot-core strip_canvas_viewport_crops_requested_rect
```

Expected: FAIL with an error that `StripCanvas` has no method named `viewport`.

- [ ] **Step 3: Implement `CanvasViewport` and `StripCanvas::viewport`**

In `crates/rollshot-core/src/canvas.rs`, add this public struct near `StripCanvas`:

```rust
pub struct CanvasViewport {
    pub image: RgbaImage,
    pub total_width: u32,
    pub total_height: u32,
    pub x: u32,
    pub y: u32,
}
```

Add this method inside `impl StripCanvas`:

```rust
pub fn viewport(&mut self, x: u32, y: u32, width: u32, height: u32) -> Option<CanvasViewport> {
    if width == 0 || height == 0 || x >= self.logical_width || y >= self.logical_height {
        return None;
    }

    let crop_width = width.min(self.logical_width - x);
    let crop_height = height.min(self.logical_height - y);
    if crop_width == 0 || crop_height == 0 {
        return None;
    }

    self.compose_if_needed();
    let full = self.composed_cache.as_ref().expect("composed image");
    let image = imageops::crop_imm(full, x, y, crop_width, crop_height).to_image();

    Some(CanvasViewport {
        image,
        total_width: self.logical_width,
        total_height: self.logical_height,
        x,
        y,
    })
}
```

This first implementation still composes internally for correctness, but it does not clone the full image and keeps the public API viewport-oriented.

- [ ] **Step 4: Export the viewport type**

In `crates/rollshot-core/src/lib.rs`, change the canvas export to:

```rust
pub use canvas::{CanvasAppendError, CanvasViewport, StripCanvas};
```

- [ ] **Step 5: Add `Stitcher::canvas_viewport`**

In `crates/rollshot-core/src/stitcher.rs`, add this method near `full_image()`:

```rust
pub fn canvas_viewport(
    &mut self,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Option<crate::canvas::CanvasViewport> {
    self.canvas
        .as_mut()
        .and_then(|canvas| canvas.viewport(x, y, width, height))
}
```

- [ ] **Step 6: Run core viewport tests**

Run:

```bash
rtk cargo test -p rollshot-core strip_canvas_viewport
```

Expected: PASS for the three viewport tests.

- [ ] **Step 7: Commit core viewport primitive**

Run:

```bash
rtk git add crates/rollshot-core/src/canvas.rs crates/rollshot-core/src/stitcher.rs crates/rollshot-core/src/lib.rs
rtk git commit -m "feat(core): add canvas viewport snapshots"
```

---

### Task 2: Shared Viewport Preview Renderer

**Files:**
- Modify: `crates/rollshot-overlay-core/src/preview.rs`

- [ ] **Step 1: Write failing overlay-core viewport preview tests**

Replace the old full-canvas spotlight expectations with viewport expectations by adding these tests in `crates/rollshot-overlay-core/src/preview.rs`:

```rust
#[test]
fn viewport_preview_bottom_edge_shows_bottom_rows() {
    let mut stitcher = rollshot_core::Stitcher::new(rollshot_core::StitchConfig::default());
    assert_eq!(stitcher.push_frame(numbered_rows(20, 120)), rollshot_core::StitchOutcome::FirstFrame);

    let preview = viewport_preview(
        &mut stitcher,
        ViewportPreviewRequest {
            viewport_width: 100,
            viewport_height: 80,
            frame_width: 20,
            frame_height: 40,
            edge: CapturedEdge::Bottom,
        },
    )
    .expect("preview");

    assert_eq!((preview.width, preview.height), (100, 80));
    assert_eq!((preview.viewport_x, preview.viewport_y), (0, 80));
    assert_eq!((preview.viewport_width_in_canvas, preview.viewport_height_in_canvas), (20, 40));
}

#[test]
fn viewport_preview_top_edge_shows_top_rows() {
    let mut stitcher = rollshot_core::Stitcher::new(rollshot_core::StitchConfig::default());
    assert_eq!(stitcher.push_frame(numbered_rows(20, 120)), rollshot_core::StitchOutcome::FirstFrame);

    let preview = viewport_preview(
        &mut stitcher,
        ViewportPreviewRequest {
            viewport_width: 100,
            viewport_height: 80,
            frame_width: 20,
            frame_height: 40,
            edge: CapturedEdge::Top,
        },
    )
    .expect("preview");

    assert_eq!((preview.viewport_x, preview.viewport_y), (0, 0));
    assert_eq!((preview.viewport_width_in_canvas, preview.viewport_height_in_canvas), (20, 40));
}

#[test]
fn viewport_preview_right_edge_shows_right_columns() {
    let mut stitcher = rollshot_core::Stitcher::new(rollshot_core::StitchConfig::default());
    assert_eq!(stitcher.push_frame(numbered_cols(120, 20)), rollshot_core::StitchOutcome::FirstFrame);

    let preview = viewport_preview(
        &mut stitcher,
        ViewportPreviewRequest {
            viewport_width: 100,
            viewport_height: 80,
            frame_width: 40,
            frame_height: 20,
            edge: CapturedEdge::Right,
        },
    )
    .expect("preview");

    assert_eq!((preview.viewport_x, preview.viewport_y), (80, 0));
    assert_eq!((preview.viewport_width_in_canvas, preview.viewport_height_in_canvas), (40, 20));
}
```

Add these test helpers in the same test module:

```rust
fn numbered_rows(width: u32, height: u32) -> image::RgbaImage {
    let mut image = image::RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            image.put_pixel(x, y, image::Rgba([(y % 251) as u8, 80, 120, 255]));
        }
    }
    image
}

fn numbered_cols(width: u32, height: u32) -> image::RgbaImage {
    let mut image = image::RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            image.put_pixel(x, y, image::Rgba([(x % 251) as u8, 80, 120, 255]));
        }
    }
    image
}
```

- [ ] **Step 2: Run failing overlay-core tests**

Run:

```bash
rtk cargo test -p rollshot-overlay-core viewport_preview_bottom_edge_shows_bottom_rows
```

Expected: FAIL with unresolved `viewport_preview` or `ViewportPreviewRequest`.

- [ ] **Step 3: Implement viewport request/result types and renderer**

In `crates/rollshot-overlay-core/src/preview.rs`, add these types below the constants:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewportPreviewRequest {
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub frame_width: u32,
    pub frame_height: u32,
    pub edge: CapturedEdge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewportPreview {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub total_width: u32,
    pub total_height: u32,
    pub viewport_x: u32,
    pub viewport_y: u32,
    pub viewport_width_in_canvas: u32,
    pub viewport_height_in_canvas: u32,
}
```

Add this renderer:

```rust
pub fn viewport_preview(
    stitcher: &mut rollshot_core::Stitcher,
    request: ViewportPreviewRequest,
) -> Option<ViewportPreview> {
    let stats = stitcher.stats();
    let total_width = stats.total_width.max(1);
    let total_height = stats.total_height.max(1);
    let vertical = !matches!(request.edge, CapturedEdge::Left | CapturedEdge::Right);

    let crop_width = if vertical {
        total_width
    } else {
        request.frame_width.min(total_width).max(1)
    };
    let crop_height = if vertical {
        request.frame_height.min(total_height).max(1)
    } else {
        total_height
    };

    let x = match request.edge {
        CapturedEdge::Right => total_width.saturating_sub(crop_width),
        CapturedEdge::Left => 0,
        _ => 0,
    };
    let y = match request.edge {
        CapturedEdge::Top => 0,
        CapturedEdge::Bottom | CapturedEdge::Unknown => total_height.saturating_sub(crop_height),
        CapturedEdge::Left | CapturedEdge::Right => 0,
    };

    let canvas = stitcher.canvas_viewport(x, y, crop_width, crop_height)?;
    let target_width = request.viewport_width.max(1);
    let target_height = request.viewport_height.max(1);
    let scale = (target_width as f32 / canvas.image.width().max(1) as f32)
        .min(target_height as f32 / canvas.image.height().max(1) as f32);
    let out_w = ((canvas.image.width() as f32 * scale).round() as u32).clamp(1, target_width);
    let out_h = ((canvas.image.height() as f32 * scale).round() as u32).clamp(1, target_height);
    let resized = image::imageops::resize(
        &canvas.image,
        out_w,
        out_h,
        image::imageops::FilterType::Triangle,
    );

    let mut boxed = RgbaImage::from_pixel(target_width, target_height, Rgba([255, 255, 255, 255]));
    let offset_x = (target_width - out_w) / 2;
    let offset_y = (target_height - out_h) / 2;
    for py in 0..out_h {
        for px in 0..out_w {
            boxed.put_pixel(offset_x + px, offset_y + py, *resized.get_pixel(px, py));
        }
    }

    draw_position_indicator(
        &mut boxed,
        vertical,
        canvas.x,
        canvas.y,
        canvas.image.width(),
        canvas.image.height(),
        canvas.total_width,
        canvas.total_height,
    );

    Some(ViewportPreview {
        width: boxed.width(),
        height: boxed.height(),
        pixels: boxed.into_raw(),
        total_width: canvas.total_width,
        total_height: canvas.total_height,
        viewport_x: canvas.x,
        viewport_y: canvas.y,
        viewport_width_in_canvas: canvas.image.width(),
        viewport_height_in_canvas: canvas.image.height(),
    })
}
```

Add this indicator helper:

```rust
fn draw_position_indicator(
    image: &mut RgbaImage,
    vertical: bool,
    viewport_x: u32,
    viewport_y: u32,
    viewport_w: u32,
    viewport_h: u32,
    total_w: u32,
    total_h: u32,
) {
    const TRACK: Rgba<u8> = Rgba([15, 23, 42, 128]);
    const THUMB: Rgba<u8> = Rgba([56, 189, 248, 230]);
    const MIN_THUMB: u32 = 8;
    let w = image.width();
    let h = image.height();

    if vertical {
        let x0 = w.saturating_sub(4);
        for y in 0..h {
            for x in x0..w {
                image.put_pixel(x, y, TRACK);
            }
        }
        let ratio = viewport_h as f32 / total_h.max(1) as f32;
        let thumb_len = ((h as f32 * ratio).round() as u32).clamp(MIN_THUMB.min(h), h);
        let max_start = h.saturating_sub(thumb_len);
        let start_ratio = viewport_y as f32 / total_h.saturating_sub(viewport_h).max(1) as f32;
        let start = ((max_start as f32 * start_ratio).round() as u32).min(max_start);
        for y in start..start + thumb_len {
            for x in x0..w {
                image.put_pixel(x, y, THUMB);
            }
        }
    } else {
        let y0 = h.saturating_sub(4);
        for y in y0..h {
            for x in 0..w {
                image.put_pixel(x, y, TRACK);
            }
        }
        let ratio = viewport_w as f32 / total_w.max(1) as f32;
        let thumb_len = ((w as f32 * ratio).round() as u32).clamp(MIN_THUMB.min(w), w);
        let max_start = w.saturating_sub(thumb_len);
        let start_ratio = viewport_x as f32 / total_w.saturating_sub(viewport_w).max(1) as f32;
        let start = ((max_start as f32 * start_ratio).round() as u32).min(max_start);
        for y in y0..h {
            for x in start..start + thumb_len {
                image.put_pixel(x, y, THUMB);
            }
        }
    }
}
```

- [ ] **Step 4: Run overlay-core tests**

Run:

```bash
rtk cargo test -p rollshot-overlay-core viewport_preview
```

Expected: PASS for viewport preview tests.

- [ ] **Step 5: Commit shared renderer**

Run:

```bash
rtk git add crates/rollshot-overlay-core/src/preview.rs
rtk git commit -m "feat(overlay-core): add viewport live preview"
```

---

### Task 3: Migrate Tauri/Webview Preview

**Files:**
- Modify: `crates/rollshot-app/src-tauri/src/session.rs`
- Modify: `crates/rollshot-app/src-tauri/src/commands.rs`
- Modify: `crates/rollshot-app/src/api/capture.ts`
- Modify: `crates/rollshot-app/src/api/capture.test.ts`
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.tsx`
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.test.tsx`

- [ ] **Step 1: Write failing command/API tests**

In `crates/rollshot-app/src/api/capture.test.ts`, update the `getStitchPreview` expectation to require explicit dimensions:

```ts
it('requests stitch preview at the displayed dimensions', async () => {
  const { getStitchPreview } = await import('./capture')
  invokeMock.mockResolvedValue(new ArrayBuffer(4))

  const blob = await getStitchPreview(180, 260)

  expect(blob).toBeInstanceOf(Blob)
  expect(invokeMock).toHaveBeenCalledWith('get_stitch_preview', {
    previewWidth: 180,
    previewHeight: 260,
  })
})
```

In `crates/rollshot-app/src/components/CaptureOverlay.test.tsx`, update the stitching poll assertion:

```ts
expect(api.getStitchPreview).toHaveBeenCalledWith(180, 260)
```

- [ ] **Step 2: Run failing frontend tests**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- CaptureOverlay capture
```

Expected: FAIL because `getStitchPreview` currently takes no dimensions.

- [ ] **Step 3: Update Tauri command signature**

In `crates/rollshot-app/src-tauri/src/commands.rs`, replace `get_stitch_preview` with:

```rust
#[tauri::command]
pub fn get_stitch_preview(
    session: tauri::State<'_, Arc<SharedSession>>,
    preview_width: u32,
    preview_height: u32,
) -> Result<Response, String> {
    let bytes = session
        .stitch_preview_png(preview_width, preview_height)?
        .unwrap_or_default();
    Ok(Response::new(bytes))
}
```

- [ ] **Step 4: Update session preview encoding**

In `crates/rollshot-app/src-tauri/src/session.rs`, change the signature to:

```rust
pub fn stitch_preview_png(
    &self,
    preview_width: u32,
    preview_height: u32,
) -> Result<Option<Vec<u8>>, String> {
```

Replace the body with:

```rust
let preview = {
    let mut inner = self
        .inner
        .lock()
        .map_err(|_| "session lock poisoned".to_string())?;
    let region = inner.selected_region;
    let edge = inner.spotlight_edge;
    let preview = match inner.stitcher.as_mut() {
        Some(stitcher) => region.and_then(|region| {
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
        }),
        None => None,
    };
    preview
};

match preview {
    Some(preview) => {
        let image = RgbaImage::from_raw(preview.width, preview.height, preview.pixels)
            .ok_or_else(|| "invalid viewport preview buffer".to_string())?;
        Ok(Some(encode_rgba_png(&image)?))
    }
    None => Ok(None),
}
```

- [ ] **Step 5: Update TypeScript API**

In `crates/rollshot-app/src/api/capture.ts`, replace `getStitchPreview` with:

```ts
export async function getStitchPreview(previewWidth: number, previewHeight: number): Promise<Blob | null> {
  const bytes = await invoke<ArrayBuffer>('get_stitch_preview', { previewWidth, previewHeight })
  if (bytes.byteLength === 0) {
    return null
  }
  return new Blob([bytes], { type: 'image/png' })
}
```

- [ ] **Step 6: Update React call site**

In `crates/rollshot-app/src/components/CaptureOverlay.tsx`, replace:

```ts
const blob = await getStitchPreview()
```

with:

```ts
const blob = await getStitchPreview(PREVIEW_SIZE.width, PREVIEW_SIZE.height)
```

- [ ] **Step 7: Add Rust session viewport test**

In `crates/rollshot-app/src-tauri/src/session.rs`, replace the old `stitch_preview_png_dims_outside_current_frame_window` test with:

```rust
#[test]
fn stitch_preview_png_returns_requested_viewport_size() {
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
        inner.push_stitch_frame(scrolling_frame(20)).expect("f1");
        inner.push_stitch_frame(scrolling_frame(40)).expect("f2");
    }

    let bytes = session
        .stitch_preview_png(180, 260)
        .expect("encode stitch preview")
        .expect("preview exists");
    let image = image::load_from_memory(&bytes).expect("decode png");

    assert_eq!((image.width(), image.height()), (180, 260));
}
```

- [ ] **Step 8: Run webview verification**

Run:

```bash
rtk cargo test -p rollshot-app stitch_preview_png_returns_requested_viewport_size
rtk pnpm --dir crates/rollshot-app test -- CaptureOverlay capture
rtk pnpm --dir crates/rollshot-app run typecheck
```

Expected: all commands PASS.

- [ ] **Step 9: Commit webview migration**

Run:

```bash
rtk git add crates/rollshot-app/src-tauri/src/session.rs crates/rollshot-app/src-tauri/src/commands.rs crates/rollshot-app/src/api/capture.ts crates/rollshot-app/src/api/capture.test.ts crates/rollshot-app/src/components/CaptureOverlay.tsx crates/rollshot-app/src/components/CaptureOverlay.test.tsx
rtk git commit -m "feat(app): use viewport stitch preview"
```

---

### Task 4: Migrate Native iced Overlay Preview

**Files:**
- Modify: `crates/rollshot-overlay/src/driver.rs`
- Modify: `crates/rollshot-overlay/src/overlay.rs`

- [ ] **Step 1: Write failing native preview handle test**

In `crates/rollshot-overlay/src/driver.rs`, add a unit test near existing driver tests:

Update the test module imports to include `viewport_handle`, `Stitcher`, and
`StitchOutcome`:

```rust
use super::{overlay_stitch_config, should_emit_capture_miss, stitch_stream, viewport_handle};
use rollshot_core::{StitchOutcome, Stitcher};
```

```rust
#[test]
fn viewport_handle_uses_requested_size() {
    let mut stitcher = Stitcher::new(overlay_stitch_config());
    assert_eq!(stitcher.push_frame(scrolling_frame(0).image), StitchOutcome::FirstFrame);

    let handle = viewport_handle(
        &mut stitcher,
        Region {
            x: 0,
            y: 0,
            width: 80,
            height: 80,
        },
        rollshot_overlay_core::capture_miss::CapturedEdge::Bottom,
        120,
        180,
    )
    .expect("handle");

    let _ = handle;
}
```

This test should compile only after `viewport_handle` exists and returns `Option<ImageHandle>`.

- [ ] **Step 2: Run failing native test**

Run:

```bash
rtk cargo test -p rollshot-overlay viewport_handle_uses_requested_size
```

Expected: FAIL because `viewport_handle` does not exist.

- [ ] **Step 3: Replace `spotlight_handle` with `viewport_handle`**

In `crates/rollshot-overlay/src/driver.rs`, replace the old helper with:

```rust
fn viewport_handle(
    stitcher: &mut Stitcher,
    region: Region,
    edge: rollshot_overlay_core::capture_miss::CapturedEdge,
    max_width: u32,
    max_height: u32,
) -> Option<ImageHandle> {
    let view = rollshot_overlay_core::preview::viewport_preview(
        stitcher,
        rollshot_overlay_core::preview::ViewportPreviewRequest {
            viewport_width: max_width,
            viewport_height: max_height,
            frame_width: region.width,
            frame_height: region.height,
            edge,
        },
    )?;
    Some(ImageHandle::from_rgba(view.width, view.height, view.pixels))
}
```

- [ ] **Step 4: Emit native preview only on accepted progress**

In `Driver::begin_stitch`, replace the preview generation block with:

```rust
let should_emit_preview = matches!(signal, StitchProgressSignal::Accepted { .. });
if should_emit_preview {
    if let Some(handle) = viewport_handle(
        &mut stitcher,
        region,
        spotlight_edge,
        preview_size.width,
        preview_size.height,
    ) {
        let _ = preview_tx.unbounded_send(LiveOverlayEvent::Preview(handle));
    }
}
```

Keep the existing capture-miss warning emission before this block.

- [ ] **Step 5: Keep native preview sizing tests aligned**

In `crates/rollshot-overlay/src/overlay.rs`, keep `preview_viewport_size` tests but update comments that mention full-canvas or texture flicker to say viewport request size. The expected values stay:

```rust
assert_eq!(viewport.width, PREVIEW_WIDTH);
assert_eq!(viewport.height, (440.0 - TOOLBAR_H - CHROME_SPACING) as u32);
```

and:

```rust
assert_eq!(viewport.width, 200);
assert_eq!(viewport.height, PREVIEW_MAX_HEIGHT);
```

- [ ] **Step 6: Run native overlay tests**

Run:

```bash
rtk cargo test -p rollshot-overlay viewport_handle_uses_requested_size
rtk cargo test -p rollshot-overlay preview_viewport
```

Expected: both commands PASS.

- [ ] **Step 7: Commit native migration**

Run:

```bash
rtk git add crates/rollshot-overlay/src/driver.rs crates/rollshot-overlay/src/overlay.rs
rtk git commit -m "feat(overlay): use viewport stitch preview"
```

---

### Task 5: Remove Full-Canvas Spotlight Preview Usage

**Files:**
- Modify: `crates/rollshot-overlay-core/src/preview.rs`
- Modify: tests that still mention `preview_with_spotlight`

- [ ] **Step 1: Find old spotlight references**

Run:

```bash
rtk rg -n "preview_with_spotlight|spotlight" crates/rollshot-overlay-core crates/rollshot-overlay crates/rollshot-app
```

Expected: references remain only in the old function/tests before this task changes them.

- [ ] **Step 2: Remove old function and tests**

In `crates/rollshot-overlay-core/src/preview.rs`, remove the public
`preview_with_spotlight` function that currently starts with this signature:

```rust
pub fn preview_with_spotlight(
    image: &RgbaImage,
    frame_width: u32,
    frame_height: u32,
    edge: CapturedEdge,
    max_width: u32,
    max_height: u32,
) -> RgbaImage
```

Remove these old tests:

```rust
spotlight_keeps_fixed_width_for_tall_canvas
spotlight_dims_outside_window_and_keeps_window_bright
spotlight_first_frame_is_not_dimmed
spotlight_top_edge_window_sits_at_top
spotlight_unknown_edge_defaults_to_bottom
```

Keep the `PREVIEW_WIDTH` and `PREVIEW_MAX_HEIGHT` constants because both UI paths still use them.

- [ ] **Step 3: Run reference scan**

Run:

```bash
rtk rg -n "preview_with_spotlight|spotlight" crates/rollshot-overlay-core crates/rollshot-overlay crates/rollshot-app
```

Expected: no references to `preview_with_spotlight`; references to `spotlight_edge` may remain in session/driver as the current-edge state name.

- [ ] **Step 4: Run overlay-core tests**

Run:

```bash
rtk cargo test -p rollshot-overlay-core
```

Expected: PASS.

- [ ] **Step 5: Commit cleanup**

Run:

```bash
rtk git add crates/rollshot-overlay-core/src/preview.rs
rtk git commit -m "refactor(overlay-core): remove full-canvas spotlight preview"
```

---

### Task 6: Full Verification

**Files:**
- No source edits unless a verification failure reveals a defect in the previous tasks.

- [ ] **Step 1: Run focused Rust tests**

Run:

```bash
rtk cargo test -p rollshot-core strip_canvas_viewport
rtk cargo test -p rollshot-overlay-core viewport_preview
rtk cargo test -p rollshot-app stitch_preview_png_returns_requested_viewport_size
rtk cargo test -p rollshot-overlay viewport_handle_uses_requested_size
```

Expected: all commands PASS.

- [ ] **Step 2: Run frontend tests and typecheck**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- CaptureOverlay capture
rtk pnpm --dir crates/rollshot-app run typecheck
```

Expected: both commands PASS.

- [ ] **Step 3: Run formatting check**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 4: Run clippy because this touches shared Rust preview paths**

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Inspect final diff**

Run:

```bash
rtk git status --short
rtk git log --oneline -6
```

Expected: working tree clean after the task commits; last commits correspond to Tasks 1-5.

---

## Notes For Executor

- Keep all shell commands prefixed with `rtk`.
- Do not introduce a snow-shot-style thumbnail list in this implementation.
- Do not change final preview/save behavior.
- Do not make `rollshot-core` depend on `rollshot-overlay-core`.
- If `cargo clippy` reports that `spotlight_edge` naming is misleading after the migration, rename it to `preview_edge` in `session.rs` and `driver.rs` in the same cleanup task, and update tests in that task.
