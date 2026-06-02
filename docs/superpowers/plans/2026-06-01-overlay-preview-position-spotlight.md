# Overlay Preview Position Spotlight Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make both capture overlays' live preview an always-on whole-canvas "position spotlight" (snow-shot `captuer-edge-mask` parity) that shows where the current frame maps within the whole long screenshot, and remove the now-superseded on-miss recovery mask/marker.

**Architecture:** A single shared function in `rollshot-overlay-core::preview` fits the whole stitched canvas into the fixed preview box and **bakes** the spotlight dim into the image pixels (dim everything except the current-frame window at the captured edge). Both capture paths — macOS `session.rs::stitch_preview_png` and the Linux `driver.rs` stitch thread — call it and simply display the result, so there is no per-platform mask rendering, no new status fields, and no CSS spotlight. On a miss the canvas does not grow, so the baked spotlight freezes automatically (snow-shot behavior); the throttled warning toast (already built) is kept.

**Tech Stack:** Rust workspace (`rollshot-overlay-core`, `rollshot-overlay`, `rollshot-app/src-tauri`), React 19 + Vitest (`rollshot-app`), iced layer-shell overlay on Linux.

**Spec:** `docs/superpowers/specs/2026-05-31-overlay-capture-miss-recovery-design.md`, **Amendment A1** (this plan implements A1.1–A1.3; A1 supersedes spec D5 and D4's preview part).

---

## File Structure

- Modify: `crates/rollshot-overlay-core/src/preview.rs`
  - Add `preview_with_spotlight(...)` (whole-canvas fit + baked dim). Remove the
    superseded `preview_viewport(...)` and its tests once both callers switch
    (Task 4). Owns ALL spotlight/fit math — the single source of truth.
- Modify: `crates/rollshot-app/src-tauri/src/session.rs`
  - Track the latest accepted append edge (`spotlight_edge`) and switch
    `stitch_preview_png` to `preview_with_spotlight`.
- Modify: `crates/rollshot-app/src/components/AdaptiveStitchPreview.tsx`
  - Remove the `preview-recovery-mask` overlay + its props (keep `processing`).
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.tsx`
  - Stop passing the removed recovery-mask props (keep the toast).
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.test.tsx`
  - Update the disconnected-stitching test: toast still shows, recovery mask gone.
- Modify: `crates/rollshot-app/src/App.css`
  - Remove the `.preview-recovery-mask*` rules (keep toast + processing pulse).
- Modify: `crates/rollshot-overlay/src/driver.rs`
  - Track the accepted edge in the stitch loop and bake the spotlight into the
    preview handle. Remove the orphaned `preview_viewport_handle`.
- Modify: `crates/rollshot-overlay/src/overlay.rs`
  - Remove the `recovery_marker` and the now-unused `capture_miss` /
    `capture_miss_edge` overlay state (keep the warning toast path).

**Out of scope / unchanged:** capture-miss detection + throttle (`capture_miss.rs`),
the warning toast (webview + native), the `processing` indicator, `SessionStatus`
fields and `capture.ts` types (the `capture_miss_edge` status field stays; it is
harmless and removing it is unrelated churn), and core stitching (`rollshot-core`).

---

### Task 1: Shared `preview_with_spotlight` in `rollshot-overlay-core`

**Files:**
- Modify: `crates/rollshot-overlay-core/src/preview.rs`

- [ ] **Step 1: Write the failing tests**

Add these tests to the `#[cfg(test)] mod tests` block in
`crates/rollshot-overlay-core/src/preview.rs` (alongside the existing
`preview_viewport` tests — leave those for now, they are removed in Task 4):

```rust
    use super::preview_with_spotlight;
    use crate::capture_miss::CapturedEdge;

    #[test]
    fn spotlight_fits_whole_tall_canvas_into_box() {
        // 100x400 canvas, box 280x480: height-bound fit, scale = 1.2.
        let canvas = RgbaImage::from_pixel(100, 400, Rgba([255, 255, 255, 255]));
        let view = preview_with_spotlight(&canvas, 100, 100, CapturedEdge::Bottom, 280, 480);
        assert_eq!((view.width(), view.height()), (120, 480));
    }

    #[test]
    fn spotlight_dims_outside_window_and_keeps_window_bright() {
        // region height 100 of a 400-tall canvas => 1/4 window at the bottom.
        let canvas = RgbaImage::from_pixel(100, 400, Rgba([255, 255, 255, 255]));
        let view = preview_with_spotlight(&canvas, 100, 100, CapturedEdge::Bottom, 280, 480);
        // Bottom 1/4 (rows >= 360) is the current-frame window: full brightness.
        assert_eq!(view.get_pixel(0, 470).0, [255, 255, 255, 255]);
        // Above the window is dimmed to 0.68 (255 * 0.68 -> 173), alpha intact.
        assert_eq!(view.get_pixel(0, 10).0, [173, 173, 173, 255]);
    }

    #[test]
    fn spotlight_first_frame_is_not_dimmed() {
        // region == whole canvas (fraction 1.0): nothing is dimmed.
        let canvas = RgbaImage::from_pixel(100, 100, Rgba([255, 255, 255, 255]));
        let view = preview_with_spotlight(&canvas, 100, 100, CapturedEdge::Bottom, 280, 480);
        assert_eq!(view.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(
            view.get_pixel(0, view.height() - 1).0,
            [255, 255, 255, 255]
        );
    }

    #[test]
    fn spotlight_top_edge_window_sits_at_top() {
        let canvas = RgbaImage::from_pixel(100, 400, Rgba([255, 255, 255, 255]));
        let view = preview_with_spotlight(&canvas, 100, 100, CapturedEdge::Top, 280, 480);
        // Top 1/4 bright, bottom dimmed.
        assert_eq!(view.get_pixel(0, 10).0, [255, 255, 255, 255]);
        assert_eq!(view.get_pixel(0, 470).0, [173, 173, 173, 255]);
    }

    #[test]
    fn spotlight_unknown_edge_defaults_to_bottom() {
        let canvas = RgbaImage::from_pixel(100, 400, Rgba([255, 255, 255, 255]));
        let view = preview_with_spotlight(&canvas, 100, 100, CapturedEdge::Unknown, 280, 480);
        assert_eq!(view.get_pixel(0, 470).0, [255, 255, 255, 255]);
        assert_eq!(view.get_pixel(0, 10).0, [173, 173, 173, 255]);
    }
```

- [ ] **Step 2: Run the tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-overlay-core spotlight -- --nocapture
```

Expected: FAIL (does not compile) — `preview_with_spotlight` does not exist yet.

- [ ] **Step 3: Implement `preview_with_spotlight`**

At the top of `crates/rollshot-overlay-core/src/preview.rs`, add the import
under the existing `use image::RgbaImage;`:

```rust
use crate::capture_miss::CapturedEdge;
```

Add these items above the `#[cfg(test)]` block:

```rust
/// Black-overlay alpha applied outside the current-frame window (snow-shot uses
/// `rgba(0,0,0,0.32)`). Pixels outside the window keep `1.0 - 0.32` of their RGB.
const SPOTLIGHT_DIM: f32 = 0.32;

/// Build the whole-canvas "position spotlight" preview (snow-shot
/// `captuer-edge-mask` parity). The entire stitched `image` is aspect-fit into
/// `max_width` x `max_height`, then every pixel OUTSIDE the current-frame window
/// is darkened. The window is the current screenful (`frame_width` x
/// `frame_height`, the crop region size in canvas px) anchored at `edge`; its
/// size along the scroll axis is `frame_extent / canvas_extent` of the preview.
///
/// `edge` selects the scroll axis: `Top`/`Bottom`/`Unknown` => vertical (window
/// spans full width at top/bottom; `Unknown` defaults to bottom), `Left`/`Right`
/// => horizontal (window spans full height at left/right).
///
/// On a miss the canvas does not grow, so the same image (same spotlight) is
/// produced again — the indicator freezes, which is the intended miss signal.
pub fn preview_with_spotlight(
    image: &RgbaImage,
    frame_width: u32,
    frame_height: u32,
    edge: CapturedEdge,
    max_width: u32,
    max_height: u32,
) -> RgbaImage {
    let max_width = max_width.max(1);
    let max_height = max_height.max(1);
    let w = image.width().max(1);
    let h = image.height().max(1);

    // Aspect-preserving fit of the WHOLE canvas into the box.
    let scale = (max_width as f32 / w as f32).min(max_height as f32 / h as f32);
    let out_w = ((w as f32 * scale).round() as u32).max(1);
    let out_h = ((h as f32 * scale).round() as u32).max(1);
    let mut view = if out_w == w && out_h == h {
        image.clone()
    } else {
        image::imageops::resize(image, out_w, out_h, image::imageops::FilterType::Triangle)
    };

    let vertical = !matches!(edge, CapturedEdge::Left | CapturedEdge::Right);
    let (canvas_extent, frame_extent, out_extent) = if vertical {
        (h, frame_height, out_h)
    } else {
        (w, frame_width, out_w)
    };

    // Window length along the scroll axis, in preview px. fraction >= 1 (e.g.
    // the first frame) means the window covers everything: no dimming.
    let fraction = (frame_extent as f32 / canvas_extent as f32).clamp(0.0, 1.0);
    let window_len = ((out_extent as f32 * fraction).round() as u32)
        .clamp(1, out_extent);
    if window_len >= out_extent {
        return view;
    }

    // [win_start, win_end) is the bright window; everything else is dimmed.
    let at_far_edge = matches!(edge, CapturedEdge::Bottom | CapturedEdge::Right | CapturedEdge::Unknown);
    let win_start = if at_far_edge { out_extent - window_len } else { 0 };
    let win_end = win_start + window_len;

    let keep = 1.0 - SPOTLIGHT_DIM;
    let dim = |c: u8| ((c as f32 * keep).round() as u8);
    for y in 0..out_h {
        for x in 0..out_w {
            let pos = if vertical { y } else { x };
            if pos >= win_start && pos < win_end {
                continue;
            }
            let p = view.get_pixel_mut(x, y);
            p.0[0] = dim(p.0[0]);
            p.0[1] = dim(p.0[1]);
            p.0[2] = dim(p.0[2]);
        }
    }
    view
}
```

- [ ] **Step 4: Run the tests and verify they pass**

Run:

```bash
rtk cargo test -p rollshot-overlay-core spotlight -- --nocapture
```

Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-overlay-core/src/preview.rs
git commit -m "feat(overlay-core): add whole-canvas position spotlight preview"
```

---

### Task 2: macOS — bake the spotlight into the webview preview

**Files:**
- Modify: `crates/rollshot-app/src-tauri/src/session.rs`

- [ ] **Step 1: Write the failing test**

In the `#[cfg(test)] mod tests` block of `session.rs`, add this test next to
`stitch_preview_png_uses_shared_viewport`:

```rust
    #[test]
    fn stitch_preview_png_dims_outside_current_frame_window() {
        let session = SharedSession::new();
        {
            let mut inner = session.inner.lock().expect("session lock");
            // White frames, region == full frame (80x80). Several appends grow a
            // tall canvas so the bottom window is a fraction of the whole.
            inner.store_frame_for_test(blank_frame(80, 80));
            inner
                .confirm_region(RegionDto { x: 0, y: 0, width: 80, height: 80 })
                .expect("confirm region");
            inner.start_stitching().expect("start stitching");
            inner.push_stitch_frame(scrolling_frame(0)).expect("f0");
            inner.push_stitch_frame(scrolling_frame(20)).expect("f1");
            inner.push_stitch_frame(scrolling_frame(40)).expect("f2");
        }

        let bytes = session
            .stitch_preview_png()
            .expect("encode stitch preview")
            .expect("preview exists");
        let image = image::load_from_memory(&bytes).expect("decode png").to_rgba8();

        // The canvas grew past one frame, so the top of the preview is dimmed
        // (no fully-white row at the very top) while the bottom window stays
        // brighter. Compare mean luma of the top vs bottom rows.
        let row_luma = |y: u32| -> f32 {
            (0..image.width())
                .map(|x| {
                    let p = image.get_pixel(x, y).0;
                    (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0
                })
                .sum::<f32>()
                / image.width() as f32
        };
        assert!(
            row_luma(image.height() - 1) > row_luma(0) + 10.0,
            "bottom window must be brighter than dimmed top: bottom={}, top={}",
            row_luma(image.height() - 1),
            row_luma(0),
        );
    }
```

> Note: `blank_frame` and `scrolling_frame` already exist in this test module
> (`scrolling_frame` produces a tall striped source; `blank_frame` a white one).

- [ ] **Step 2: Run the test and verify it fails**

Run:

```bash
rtk cargo test -p rollshot-app stitch_preview_png_dims_outside_current_frame_window -- --nocapture
```

Expected: FAIL — `stitch_preview_png` still uses the bottom-follow viewport (no dimming), so top and bottom luma are equal.

- [ ] **Step 3: Track the accepted edge in `AppSession`**

In `session.rs`, extend the imports from `capture_miss` (currently
`progress_signal_from_outcome, CaptureMissState, CaptureMissTracker,
CAPTURE_MISS_WARNING`) to also bring in the edge type and the signal enum:

```rust
use rollshot_overlay_core::capture_miss::{
    progress_signal_from_outcome, CaptureMissState, CaptureMissTracker, CapturedEdge,
    StitchProgressSignal, CAPTURE_MISS_WARNING,
};
```

Add a field to `struct AppSession` (after `capture_miss_state`):

```rust
    spotlight_edge: CapturedEdge,
```

Initialise it in **both** `impl Default for AppSession` and add the same reset
in `start_stitching` and `reset_capture_state`:

```rust
            spotlight_edge: CapturedEdge::Unknown,
```

(In `Default`, add the line in the struct literal next to
`capture_miss_state: CaptureMissState::default(),`. In `start_stitching` and
`reset_capture_state`, add `self.spotlight_edge = CapturedEdge::Unknown;` next
to the `self.capture_miss_state = CaptureMissState::default();` line.)

- [ ] **Step 4: Update the accepted edge in `push_stitch_frame`**

Replace the tracker-update lines in `push_stitch_frame`:

```rust
        let outcome = stitcher.push_frame(cropped.image);
        self.capture_miss_state = self.capture_miss_tracker.update(
            progress_signal_from_outcome(&outcome),
            std::time::Instant::now(),
        );
```

with (compute the signal once, reuse it for both the spotlight edge and the
tracker):

```rust
        let outcome = stitcher.push_frame(cropped.image);
        let signal = progress_signal_from_outcome(&outcome);
        if let StitchProgressSignal::Accepted { edge } = signal {
            if edge != CapturedEdge::Unknown {
                self.spotlight_edge = edge;
            }
        }
        self.capture_miss_state = self
            .capture_miss_tracker
            .update(signal, std::time::Instant::now());
```

- [ ] **Step 5: Switch `stitch_preview_png` to the spotlight**

Replace the body of the `image.as_ref().map(...)` closure in
`stitch_preview_png` (currently calling `preview_viewport`). The region is read
from `selected_region` before locking out the image, so capture it first.
Replace the whole method body with:

```rust
    pub fn stitch_preview_png(&self) -> Result<Option<Vec<u8>>, String> {
        // The live stitch preview is a whole-canvas position spotlight (snow-shot
        // captuer-edge-mask parity): the entire stitch fitted into a fixed box
        // with everything outside the current-frame window dimmed.
        let (image, region, edge) = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| "session lock poisoned".to_string())?;
            let region = inner.selected_region;
            let edge = inner.spotlight_edge;
            let image = inner
                .stitcher
                .as_mut()
                .and_then(|s| s.full_image())
                .cloned();
            (image, region, edge)
        };
        match (image, region) {
            (Some(image), Some(region)) => {
                let view = rollshot_overlay_core::preview::preview_with_spotlight(
                    &image,
                    region.width,
                    region.height,
                    edge,
                    rollshot_overlay_core::preview::PREVIEW_WIDTH,
                    rollshot_overlay_core::preview::PREVIEW_MAX_HEIGHT,
                );
                Ok(Some(encode_rgba_png(&view)?))
            }
            _ => Ok(None),
        }
    }
```

> `selected_region` is a `Region` ({ x, y, width, height }); `region.width` /
> `region.height` are `u32`. `spotlight_edge` is `Copy`.

- [ ] **Step 6: Run the new test and the existing preview test**

Run:

```bash
rtk cargo test -p rollshot-app stitch_preview_png_dims_outside_current_frame_window stitch_preview_png_uses_shared_viewport -- --nocapture
```

Expected: PASS both. (`stitch_preview_png_uses_shared_viewport` still passes:
its single full-region frame yields `fraction == 1.0` — no dimming — and the
width-bound fit of a short 960x600 canvas produces the same 280x175 image as
before.)

- [ ] **Step 7: Run the full app Rust suite**

Run:

```bash
rtk cargo test -p rollshot-app -- --nocapture
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/rollshot-app/src-tauri/src/session.rs
git commit -m "feat(session): bake position spotlight into webview stitch preview"
```

---

### Task 3: macOS frontend — remove the recovery mask

**Files:**
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.test.tsx`
- Modify: `crates/rollshot-app/src/components/AdaptiveStitchPreview.tsx`
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.tsx`
- Modify: `crates/rollshot-app/src/App.css`

- [ ] **Step 1: Update the failing test first**

In `CaptureOverlay.test.tsx`, the test
`'shows capture miss warning and preview affordance while stitching is
disconnected'` asserts the recovery mask is present. Update its title and
assertions so the toast still shows but the recovery mask is gone. Replace:

```tsx
  it('shows capture miss warning and preview affordance while stitching is disconnected', async () => {
```

with:

```tsx
  it('shows capture miss toast (no preview mask) while stitching is disconnected', async () => {
```

and replace:

```tsx
    expect(container.querySelector('.capture-miss-toast')?.textContent).toContain(
      'Scrolling too fast',
    )
    expect(container.querySelector('.preview-recovery-mask')).not.toBeNull()
```

with:

```tsx
    expect(container.querySelector('.capture-miss-toast')?.textContent).toContain(
      'Scrolling too fast',
    )
    // Snow-shot-exact: no mask is painted on the preview; the spotlight just
    // freezes. The transient toast is the only miss affordance.
    expect(container.querySelector('.preview-recovery-mask')).toBeNull()
```

- [ ] **Step 2: Run the test and verify it fails**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- CaptureOverlay.test.tsx
```

Expected: FAIL — `.preview-recovery-mask` still renders, so `toBeNull()` fails.

- [ ] **Step 3: Remove the recovery mask from `AdaptiveStitchPreview`**

Replace the entire contents of
`crates/rollshot-app/src/components/AdaptiveStitchPreview.tsx` with:

```tsx
import type { PreviewPlacement } from '../overlay/placement'

type AdaptiveStitchPreviewProps = {
  imageUrl: string | null
  status: string
  placement: PreviewPlacement
  processing?: boolean
}

export function AdaptiveStitchPreview({
  imageUrl,
  status,
  placement,
  processing,
}: AdaptiveStitchPreviewProps) {
  if (placement.mode === 'status' || !imageUrl) {
    return <div className="capture-status">{status}</div>
  }

  return (
    <div
      className={`adaptive-stitch-preview adaptive-stitch-preview-${placement.side}`}
      style={{
        left: `${placement.rect.left}px`,
        top: `${placement.rect.top}px`,
        width: `${placement.rect.width}px`,
        height: `${placement.rect.height}px`,
      }}
    >
      <img src={imageUrl} alt="Stitching preview" draggable={false} />
      {processing ? <div className="preview-processing-indicator" aria-label="Stitching" /> : null}
    </div>
  )
}
```

- [ ] **Step 4: Stop passing the removed props in `CaptureOverlay`**

In `CaptureOverlay.tsx`, replace the `<AdaptiveStitchPreview .../>` usage:

```tsx
        <AdaptiveStitchPreview
          imageUrl={stitchPreviewUrl}
          status={stats}
          placement={placement}
          captureMiss={status.state === 'stitching' ? status.capture_miss : false}
          capturedEdge={status.state === 'stitching' ? status.capture_miss_edge : 'unknown'}
          processing={status.state === 'stitching'}
        />
```

with:

```tsx
        <AdaptiveStitchPreview
          imageUrl={stitchPreviewUrl}
          status={stats}
          placement={placement}
          processing={status.state === 'stitching'}
        />
```

> Leave the `capture_miss` / `capture_miss_warning` / `capture_miss_message`
> usage that drives the toast untouched. `capture_miss_edge` is no longer read
> by the frontend; the status field stays (Rust still sends it).

- [ ] **Step 5: Remove the recovery-mask CSS**

In `App.css`, delete the five `.preview-recovery-mask*` rule blocks
(`.preview-recovery-mask`, `-bottom`, `-top`, `-left`, `-right` — the block that
starts at `.preview-recovery-mask {` and ends just before
`.preview-processing-indicator {`). Keep `.capture-miss-toast`,
`.preview-processing-indicator`, and the `@keyframes preview-processing-pulse`.

- [ ] **Step 6: Run the frontend tests + typecheck**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test
rtk pnpm --dir crates/rollshot-app run typecheck
```

Expected: PASS. (Typecheck confirms no remaining references to the removed
`captureMiss` / `capturedEdge` props.)

- [ ] **Step 7: Commit**

```bash
git add crates/rollshot-app/src/components/AdaptiveStitchPreview.tsx \
        crates/rollshot-app/src/components/CaptureOverlay.tsx \
        crates/rollshot-app/src/components/CaptureOverlay.test.tsx \
        crates/rollshot-app/src/App.css
git commit -m "feat(overlay): drop webview recovery mask in favor of frozen spotlight"
```

---

### Task 4: Linux native overlay — bake the spotlight, remove the marker

**Files:**
- Modify: `crates/rollshot-overlay/src/driver.rs`
- Modify: `crates/rollshot-overlay/src/overlay.rs`
- Modify: `crates/rollshot-overlay-core/src/preview.rs` (remove superseded fn)

- [ ] **Step 1: Track the accepted edge and bake the spotlight in `driver.rs`**

In `driver.rs`, extend the `capture_miss` import to add the edge type and signal
enum. Replace:

```rust
use rollshot_overlay_core::capture_miss::{
    progress_signal_from_outcome, CaptureMissState, CaptureMissTracker,
};
```

with:

```rust
use rollshot_overlay_core::capture_miss::{
    progress_signal_from_outcome, CaptureMissState, CaptureMissTracker, CapturedEdge,
    StitchProgressSignal,
};
```

In `begin_stitch`, add an edge accumulator next to the existing tracker setup.
Replace:

```rust
            let mut capture_miss_tracker = CaptureMissTracker::default();
            let mut last_capture_miss_active = false;
```

with:

```rust
            let mut capture_miss_tracker = CaptureMissTracker::default();
            let mut last_capture_miss_active = false;
            let mut spotlight_edge = CapturedEdge::Unknown;
```

Replace the push/update/preview block (currently from
`let outcome = stitcher.push_frame(cropped.image);` through the
`LiveOverlayEvent::Preview(handle)` send) with:

```rust
                        let outcome = stitcher.push_frame(cropped.image);
                        let signal = progress_signal_from_outcome(&outcome);
                        if let StitchProgressSignal::Accepted { edge } = signal {
                            if edge != CapturedEdge::Unknown {
                                spotlight_edge = edge;
                            }
                        }
                        let capture_miss_state =
                            capture_miss_tracker.update(signal, Instant::now());
                        if should_emit_capture_miss(&capture_miss_state, last_capture_miss_active) {
                            let _ = preview_tx
                                .unbounded_send(LiveOverlayEvent::CaptureMiss(capture_miss_state));
                        }
                        last_capture_miss_active = capture_miss_state.active;
                        if let Some(preview) = stitcher.full_image() {
                            let handle = spotlight_handle(
                                preview,
                                region,
                                spotlight_edge,
                                preview_size.width,
                                preview_size.height,
                            );
                            let _ = preview_tx.unbounded_send(LiveOverlayEvent::Preview(handle));
                        }
```

> `region` is the `rollshot_capture::Region` computed at the top of
> `begin_stitch` and is `Copy`; `region.width` / `region.height` are `u32`.

- [ ] **Step 2: Replace the preview handle helper**

In `driver.rs`, replace `preview_viewport_handle` with `spotlight_handle`:

```rust
/// Build the whole-canvas position-spotlight preview
/// (`rollshot_overlay_core::preview::preview_with_spotlight`) as an iced image
/// handle.
#[allow(dead_code)]
fn spotlight_handle(
    image: &image::RgbaImage,
    region: Region,
    edge: rollshot_overlay_core::capture_miss::CapturedEdge,
    max_width: u32,
    max_height: u32,
) -> ImageHandle {
    let view = rollshot_overlay_core::preview::preview_with_spotlight(
        image,
        region.width,
        region.height,
        edge,
        max_width,
        max_height,
    );
    ImageHandle::from_rgba(view.width(), view.height(), view.into_raw())
}
```

- [ ] **Step 3: Build the native overlay (no test change for the threaded loop)**

Run:

```bash
rtk cargo test -p rollshot-overlay -- --nocapture
```

Expected: PASS (compiles; existing `should_emit_capture_miss` and
`stitch_stream` tests still pass). The threaded `begin_stitch` loop is not unit
tested — the spotlight math is covered by Task 1's `rollshot-overlay-core` tests.

- [ ] **Step 4: Remove the `recovery_marker` and unused miss state in `overlay.rs`**

In `overlay.rs`, remove the two now-unused fields from `struct Overlay`:

```rust
    capture_miss: bool,
    capture_miss_edge: rollshot_overlay_core::capture_miss::CapturedEdge,
```

(Keep `capture_miss_warn: bool` and `capture_miss_message_expires_at`.)

In the `Message::LiveEvent(... CaptureMiss(miss))` handler, remove the two lines
that set the removed fields:

```rust
            state.capture_miss = miss.active;
            state.capture_miss_edge = miss.edge;
```

(Keep the `if miss.warn { ... }` block that arms the warning toast.)

In `view`, delete the `recovery_marker` binding and its `col.push`:

```rust
        let recovery_marker: Option<Element<'_, Message>> = state.capture_miss.then(|| {
            text("Scroll back to the captured edge")
                .size(13)
                .style(|_theme| iced::widget::text::Style {
                    color: Some(Color::from_rgb(1.0, 251.0 / 255.0, 235.0 / 255.0)),
                })
                .into()
        });
```

and in the chrome-column builder remove:

```rust
            if let Some(r) = recovery_marker {
                col = col.push(r);
            }
```

> The spotlight is now baked into the preview image (`state.preview`), so the
> existing `if let Some(handle) = &state.preview { col = col.push(image(...)) }`
> already renders it. No new overlay widget is needed.

- [ ] **Step 5: Remove the superseded `preview_viewport` from core**

Both callers (`session.rs` Task 2, `driver.rs` Step 2) now use
`preview_with_spotlight`, so `preview_viewport` is dead. In
`crates/rollshot-overlay-core/src/preview.rs`, delete the `preview_viewport`
function and its two tests (`grows_to_content_below_cap`,
`caps_and_follows_bottom_for_tall_canvas`) and the
`use super::preview_viewport;` line in the test module. Keep `PREVIEW_WIDTH`,
`PREVIEW_MAX_HEIGHT`, and `preview_with_spotlight`.

> Do NOT touch `overlay.rs::preview_viewport_size` — that is a different function
> (it sizes the chrome band) and is still used.

- [ ] **Step 6: Verify the workspace compiles and tests pass**

Run:

```bash
rtk cargo test -p rollshot-overlay -p rollshot-overlay-core -- --nocapture
```

Expected: PASS. No `unused`/dead-code warnings for `preview_viewport`,
`capture_miss`, or `capture_miss_edge`.

- [ ] **Step 7: Commit**

```bash
git add crates/rollshot-overlay/src/driver.rs \
        crates/rollshot-overlay/src/overlay.rs \
        crates/rollshot-overlay-core/src/preview.rs
git commit -m "feat(overlay): bake native spotlight preview, drop recovery marker"
```

---

### Task 5: Full verification

**Files:** none (verification only).

- [ ] **Step 1: Rust workspace tests**

Run:

```bash
rtk cargo test
```

Expected: PASS.

- [ ] **Step 2: Formatting**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS. (If it reports diffs, run `rtk cargo fmt` and amend the relevant
commit.)

- [ ] **Step 3: Clippy on the touched crates**

Run:

```bash
rtk cargo clippy -p rollshot-overlay-core -p rollshot-overlay -p rollshot-app --all-targets -- -D warnings
```

Expected: PASS (no warnings — in particular no dead-code warning for the removed
`preview_viewport` / `capture_miss` fields).

- [ ] **Step 4: Frontend tests, typecheck, build**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test
rtk pnpm --dir crates/rollshot-app run typecheck
rtk pnpm --dir crates/rollshot-app run build
```

Expected: PASS.

- [ ] **Step 5: Manual runtime acceptance (record unchecked paths)**

Per AGENTS.md platform-split rules, both paths must be checked; if a path cannot
be run locally, record it in the final notes.

Linux native overlay (KDE/Wayland):

```bash
rtk cargo run -p rollshot-overlay --bin capture_overlay
```

Expected:
- Slow scroll: the preview is the whole stitched canvas; the bright window marks
  the current screen and moves to the active edge as content grows; the rest is
  dimmed.
- Fast scroll (miss): the warning toast appears once then throttles; the
  spotlight **freezes** (stops advancing). No "Scroll back…" marker is drawn.
- Scrolling back resumes preview growth.
- The preview/spotlight stays outside the crop (never enters the stitched image).

macOS/webview:

```bash
rtk pnpm --dir crates/rollshot-app run tauri:dev
```

Expected: same spotlight behavior; warning toast on fast scroll; no
`preview-recovery-mask`; existing overlay-exclusion behavior intact.

---

## Self-Review

- **Spec coverage (Amendment A1):**
  - A1.1 (whole-canvas fit + spotlight, both platforms, ≤280×480 texture,
    shared geometry) → Task 1 (`preview_with_spotlight`, uses `PREVIEW_WIDTH`/
    `PREVIEW_MAX_HEIGHT` as the box), consumed by Task 2 (macOS) + Task 4 (Linux).
  - A1.2 (snow-shot-exact miss: keep toast, freeze spotlight, remove recovery
    mask/marker) → freeze is automatic (canvas unchanged on miss; noted in Task 1
    doc + Task 5 acceptance); toast kept (Tasks 3/4 explicitly leave it); removal
    in Task 3 (webview) + Task 4 Step 4 (native). `capture_miss`/`edge` retained
    in status to drive the baked window edge (Task 2/Task 4 edge tracking).
  - A1.3 (both platforms; thumbnail-list NOT adopted; `processing` kept; D6
    unchanged) → Tasks 2+4 cover both platforms; preview stays a single fitted
    image; `processing` prop kept in Task 3; no `rollshot-core` changes.
- **Placeholder scan:** none — every code step shows full code; every command has
  expected output.
- **Type consistency:** `preview_with_spotlight(image, frame_width, frame_height,
  edge, max_width, max_height)` is used with that exact signature in Task 2
  (session) and Task 4 (`spotlight_handle`). `spotlight_edge: CapturedEdge` is
  defined and reset consistently. `StitchProgressSignal::Accepted { edge }` is
  destructured identically in both paths. `Region.width/height` are `u32` in both
  consumers.
