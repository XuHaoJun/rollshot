# Dynamic Preview Height Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the fixed preview height cap with `min(crop_height, available_band_space)` on both Linux overlay and macOS webview, and unify preview width to 280px.

**Architecture:** The `viewport_preview()` function in `rollshot-overlay-core` is unchanged — it already aspect-fits content into whatever viewport dimensions are requested. Linux changes `preview_viewport_size()` to request `min(crop_height, available_band_space)`. macOS/webview changes `placement.ts` so preview sizing happens after each candidate band is measured, then `CaptureOverlay.tsx` uses that placement-aware preview size for both display placement and stitch-preview requests.

**Tech Stack:** Rust (rollshot-overlay, rollshot-overlay-core), TypeScript/React (rollshot-app)

**Spec:** `docs/superpowers/specs/2026-06-02-dynamic-preview-height-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `crates/rollshot-overlay/src/overlay.rs` | Modify | Remove `PREVIEW_MAX_HEIGHT` cap from `preview_viewport_size()`, update tests |
| `crates/rollshot-app/src/overlay/placement.ts` | Modify | Add placement-aware preview sizing helper that caps height by candidate band space |
| `crates/rollshot-app/src/overlay/placement.test.ts` | Modify | Cover 280px preview sizing and candidate band-height capping |
| `crates/rollshot-app/src/components/CaptureOverlay.tsx` | Modify | Replace fixed `MAX_PREVIEW_SIZE` with placement-aware 280px preview sizing |
| `crates/rollshot-app/src/components/CaptureOverlay.test.tsx` | Modify | Update existing stitch-preview request assertion for new preview dimensions |

**Unchanged (verified):**
- `crates/rollshot-overlay-core/src/preview.rs` — `PREVIEW_WIDTH`, `PREVIEW_MAX_HEIGHT` constants stay; `viewport_preview()` logic untouched
- `crates/rollshot-overlay/src/driver.rs` — passes through whatever size `preview_viewport_size()` returns
- `crates/rollshot-app/src-tauri/src/session.rs` — `stitch_preview_png()` passes through caller's dimensions; existing test uses `PREVIEW_MAX_HEIGHT` directly (separate code path, unchanged)
- `crates/rollshot-app/src/components/NativeCaptureFlow.tsx` — no preview size references
- `crates/rollshot-app/src/api/capture.test.ts` — tests the API wrapper with arbitrary values, not sizing logic

**Existing behavior preserved:**
- `crates/rollshot-app/src/overlay/placement.ts` — keep `fitPreviewSizeToRegion()` and fixed-size `choosePreviewPlacement()` behavior intact; add a new helper for dynamic sizing

---

### Task 1: Linux overlay — remove `PREVIEW_MAX_HEIGHT` cap

**Files:**
- Modify: `crates/rollshot-overlay/src/overlay.rs:20,493-516,700-739`

- [ ] **Step 1: Add a failing test for the crop-height cap**

In `crates/rollshot-overlay/src/overlay.rs`, add a new test that exercises the crop-height cap (tall narrow crop with ample band space):

```rust
    #[test]
    fn preview_viewport_caps_height_at_crop_height() {
        // Tall narrow crop (200x600) with lots of space on the right band.
        // Band::Right wins (right=2260, area=3,254,400).
        // available_height = 1440-100 = 1340, band_height = 1340-50-8 = 1282.
        // New: max_height = min(1282, 600) = 600.
        // aspect = 200/600 = 0.333, max_aspect = 280/600 = 0.467.
        // aspect < max_aspect → width = round(600*0.333) = 200, height = 600.
        let crop = Rectangle {
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 600.0,
        };
        let window = Size::new(2560.0, 1440.0);

        let viewport = preview_viewport_size(crop, window);

        assert_eq!(viewport.width, 200);
        assert_eq!(viewport.height, 600);
    }
```

Add this test after the existing `preview_viewport_clamps_width_to_side_band_and_preserves_aspect` test (after line 739).

- [ ] **Step 2: Run the new test to verify it fails**

Run: `rtk cargo test -p rollshot-overlay preview_viewport_caps_height_at_crop_height`
Expected: FAIL — old code clamps to `PREVIEW_MAX_HEIGHT=480`, producing `160x480` instead of `200x600`.

- [ ] **Step 3: Remove `PREVIEW_MAX_HEIGHT` import and apply crop-height cap**

In `crates/rollshot-overlay/src/overlay.rs`, change the import on line 20:

```rust
// Before:
use rollshot_overlay_core::preview::{PREVIEW_MAX_HEIGHT, PREVIEW_WIDTH};

// After:
use rollshot_overlay_core::preview::PREVIEW_WIDTH;
```

In `preview_viewport_size()` (line 513), replace the `PREVIEW_MAX_HEIGHT` clamp:

```rust
// Before:
    let max_height = band_height.clamp(1, PREVIEW_MAX_HEIGHT) as f32;

// After:
    let crop_h = crop.height.max(1.0);
    let max_height = (band_height as f32).min(crop_h);
```

- [ ] **Step 4: Run all overlay tests to verify**

Run: `rtk cargo test -p rollshot-overlay`
Expected: All tests pass. The existing tests (`preview_viewport_uses_fixed_width_and_bottom_band_height` and `preview_viewport_clamps_width_to_side_band_and_preserves_aspect`) still pass because the band height is already less than the crop height in those scenarios.

- [ ] **Step 5: Run clippy**

Run: `rtk cargo clippy -p rollshot-overlay --all-targets -- -D warnings`
Expected: No warnings.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-overlay/src/overlay.rs
rtk git commit -m "feat(overlay): cap preview height at crop height instead of fixed 480px"
```

---

### Task 2: macOS webview — add placement-aware preview sizing

**Files:**
- Modify: `crates/rollshot-app/src/overlay/placement.ts`
- Modify: `crates/rollshot-app/src/overlay/placement.test.ts`

- [ ] **Step 1: Add failing tests for band-capped dynamic placement**

In `crates/rollshot-app/src/overlay/placement.test.ts`, import the new helper
that will be implemented in Step 3:

```ts
import {
  chooseDynamicPreviewPlacement,
  choosePreviewPlacement,
  fitPreviewSizeToRegion,
  type OverlayExclusion,
} from './placement'
```

Add these tests after the existing `choosePreviewPlacement` tests:

```ts
describe('chooseDynamicPreviewPlacement', () => {
  it('caps side preview height to the available band before choosing placement', () => {
    expect(
      chooseDynamicPreviewPlacement({
        bounds: { left: 0, top: 0, width: 1000, height: 700 },
        region: { left: 100, top: 450, width: 200, height: 300 },
        previewWidth: 280,
        overlayExclusion: 'unsupported',
        gap: 12,
      }),
    ).toEqual({
      mode: 'image',
      side: 'right',
      rect: { left: 312, top: 450, width: 167, height: 250 },
      preview: { width: 167, height: 250 },
    })
  })

  it('caps preview height at crop height when the band has extra room', () => {
    expect(
      chooseDynamicPreviewPlacement({
        bounds: { left: 0, top: 0, width: 1000, height: 900 },
        region: { left: 100, top: 100, width: 200, height: 300 },
        previewWidth: 280,
        overlayExclusion: 'unsupported',
        gap: 12,
      }),
    ).toEqual({
      mode: 'image',
      side: 'right',
      rect: { left: 312, top: 100, width: 200, height: 300 },
      preview: { width: 200, height: 300 },
    })
  })
})
```

- [ ] **Step 2: Run the new tests to verify they fail**

Run: `rtk pnpm --dir crates/rollshot-app test -- placement.test`
Expected: FAIL — `chooseDynamicPreviewPlacement` does not exist yet.

- [ ] **Step 3: Implement `chooseDynamicPreviewPlacement()`**

In `crates/rollshot-app/src/overlay/placement.ts`, add a dynamic helper that
keeps the existing fixed-size `choosePreviewPlacement()` intact for current
callers and tests:

```ts
export type DynamicPreviewPlacement =
  | {
      mode: 'image'
      side: 'right' | 'left' | 'bottom' | 'top' | 'inside'
      rect: OverlayRect
      preview: PreviewSize
    }
  | { mode: 'status' }

type DynamicPlacementInput = {
  bounds: OverlayRect
  region: OverlayRect
  previewWidth: number
  overlayExclusion: OverlayExclusion
  gap?: number
}
```

Implementation rules:
- Evaluate candidates in the existing order: right, left, bottom, top, then inside only when `overlayExclusion === 'verified'`.
- For each candidate, compute the available width/height for that side before calling `fitPreviewSizeToRegion()`.
- Use `maxPreview.width = min(previewWidth, availableWidth)` and `maxPreview.height = min(region.height, availableHeight)`.
- Return the first candidate whose dynamically sized rect fits.
- Keep dimensions floored to at least `1`, matching `fitPreviewSizeToRegion()`.

Side-space definitions:

```ts
const boundsRight = bounds.left + bounds.width
const boundsBottom = bounds.top + bounds.height
const regionRight = region.left + region.width
const regionBottom = region.top + region.height

right:  width = boundsRight - regionRight - gap, height = boundsBottom - region.top
left:   width = region.left - bounds.left - gap, height = boundsBottom - region.top
bottom: width = boundsRight - region.left,       height = boundsBottom - regionBottom - gap
top:    width = boundsRight - region.left,       height = region.top - bounds.top - gap
inside: width = region.width - gap * 2,                height = region.height - gap * 2
```

- [ ] **Step 4: Update `fitPreviewSizeToRegion` fixture tests to use 280px dynamic caps**

In the existing `fitPreviewSizeToRegion` tests, update the fixtures:

```ts
// Wide crop (2400x900): aspect=2.667, max_aspect=280/900=0.311
// aspect >= max_aspect -> w=280, h=round(280/2.667)=105
expect(
  fitPreviewSizeToRegion({
    region: { width: 2400, height: 900 },
    maxPreview: { width: 280, height: 900 },
  }),
).toEqual({ width: 280, height: 105 })

// Tall crop (400x1200): aspect=0.333, max_aspect=280/1200=0.233
// aspect >= max_aspect -> w=280, h=round(280/0.333)=840
expect(
  fitPreviewSizeToRegion({
    region: { width: 400, height: 1200 },
    maxPreview: { width: 280, height: 1200 },
  }),
).toEqual({ width: 280, height: 840 })
```

- [ ] **Step 5: Run placement tests to verify**

Run: `rtk pnpm --dir crates/rollshot-app test -- placement.test`
Expected: All placement tests pass.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/overlay/placement.ts crates/rollshot-app/src/overlay/placement.test.ts
rtk git commit -m "feat(app): size stitch preview from available placement band"
```

---

### Task 3: macOS webview — wire dynamic placement into `CaptureOverlay`

**Files:**
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.tsx`
- Modify: `crates/rollshot-app/src/components/CaptureOverlay.test.tsx`

- [ ] **Step 1: Update the existing component test to expect 280px dynamic preview requests**

In `crates/rollshot-app/src/components/CaptureOverlay.test.tsx`, update the
`shows capture miss toast (no preview mask) while stitching is disconnected`
fixture so the source region is large enough to exercise the 280px width:

```ts
region: { x: 100, y: 50, width: 800, height: 400 },
```

Update the preview request assertion:

```ts
expect(api.getStitchPreview).toHaveBeenCalledWith(280, 140)
```

Expected math: source and CSS crop aspect is `2.0`; max preview is `280x200`
in display units, so the requested preview is `280x140`.

- [ ] **Step 2: Run the component test to verify it fails**

Run: `rtk pnpm --dir crates/rollshot-app test -- CaptureOverlay.test`
Expected: FAIL — old code still requests `180x90`.

- [ ] **Step 3: Replace fixed preview sizing in `CaptureOverlay.tsx`**

In `crates/rollshot-app/src/components/CaptureOverlay.tsx`, replace:

```ts
const MAX_PREVIEW_SIZE = { width: 180, height: 260 }
```

With:

```ts
const PREVIEW_WIDTH = 280
```

Import the new placement helper:

```ts
import { chooseDynamicPreviewPlacement } from '../overlay/placement'
```

Update the placement memo so it calls `chooseDynamicPreviewPlacement()` with
`bounds`, `activeRegionRect`, `PREVIEW_WIDTH`, and `overlayMode`; pass the
returned placement to `AdaptiveStitchPreview`.

Update the polling loop so stitch preview requests use the same dynamic sizing
logic:
- If `nextStatus.state !== 'stitching'`, do nothing.
- If the current `scale` is not available yet, skip the preview request for this poll.
- Convert `nextStatus.region` to a CSS rect with `sourceRegionToCssRect(nextStatus.region, scale)`.
- Call `chooseDynamicPreviewPlacement()` with the same bounds and `PREVIEW_WIDTH`.
- If the result is `mode: 'image'`, call `getStitchPreview(preview.width, preview.height)`.
- If the result is `mode: 'status'`, skip the preview request for this poll.

This keeps the image dimensions and displayed placement in sync instead of
pre-sizing the image before the candidate band is known.

- [ ] **Step 4: Run the component test to verify**

Run: `rtk pnpm --dir crates/rollshot-app test -- CaptureOverlay.test`
Expected: All component tests pass.

- [ ] **Step 5: Run frontend typecheck**

Run: `rtk pnpm --dir crates/rollshot-app run typecheck`
Expected: No errors.

- [ ] **Step 6: Run frontend tests**

Run: `rtk pnpm --dir crates/rollshot-app test`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/components/CaptureOverlay.tsx crates/rollshot-app/src/components/CaptureOverlay.test.tsx
rtk git commit -m "feat(app): use dynamic 280px stitch preview placement"
```

---

### Task 4: Final verification

- [ ] **Step 1: Run full Rust test suite**

Run: `rtk cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 2: Run Rust formatting check**

Run: `rtk cargo fmt --check`
Expected: No formatting issues.

- [ ] **Step 3: Run Rust clippy**

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`
Expected: No warnings.

- [ ] **Step 4: Run frontend typecheck**

Run: `rtk pnpm --dir crates/rollshot-app run typecheck`
Expected: No errors.

- [ ] **Step 5: Run frontend tests**

Run: `rtk pnpm --dir crates/rollshot-app test`
Expected: All tests pass.

- [ ] **Step 6: Run frontend build**

Run: `rtk pnpm --dir crates/rollshot-app run build`
Expected: Build succeeds.
