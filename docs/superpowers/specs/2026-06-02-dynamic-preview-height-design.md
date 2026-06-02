# Dynamic Preview Height Design

**Date:** 2026-06-02
**Status:** Draft
**Scope:** `rollshot-overlay` (Linux), `rollshot-app` (macOS webview)

## Problem

The stitching live preview uses a fixed height cap (`PREVIEW_MAX_HEIGHT = 480`
on Linux, `MAX_PREVIEW_SIZE.height = 260` on macOS) regardless of available
screen space. When the crop region is tall and the monitor has room, the
preview wastes vertical space. When the crop is small, the cap is irrelevant.

Reference: wayscrollshot uses `height = min(preview_image_height, region.h)`
with a fixed 280px width. The preview grows dynamically with stitched content
up to the crop region height.

## Design

### Height rule

```
preview_max_height = min(crop.height, available_band_height)
```

- `crop.height` = user-selected capture region height (fixed after selection).
- `available_band_height` = space from the crop edge to the screen edge in the
  chosen chrome band, minus toolbar and spacing.
- Remove the `PREVIEW_MAX_HEIGHT = 480` hard cap from the sizing path.

### Width rule

Both platforms use a fixed 280px width, matching wayscrollshot's
`PREVIEW_MAX_WIDTH` and the existing Linux `PREVIEW_WIDTH`.

- macOS changes from 180px to 280px.
- Linux stays at 280px.

### Content inside the preview box

The `viewport_preview()` function in `rollshot-overlay-core` crops a
frame-sized window from the stitched canvas and aspect-fits it into the
requested viewport dimensions. This logic does not change. The preview box
gets larger; the content inside is still aspect-fit with letterboxing and a
position indicator.

## Changes by file

### `rollshot-overlay-core/src/preview.rs`

- Keep `PREVIEW_WIDTH = 280` (used by both platforms).
- Keep `PREVIEW_MAX_HEIGHT = 480` — it is still used by `session.rs` for the
  `stitch_preview_png` Tauri command (final preview on save), which is a
  separate code path.
- No logic changes in `viewport_preview()`.

### `rollshot-overlay/src/overlay.rs` — `preview_viewport_size()`

Replace:

```rust
let max_height = band_height.clamp(1, PREVIEW_MAX_HEIGHT) as f32;
```

With:

```rust
let crop_h = crop.height.max(1.0);
let max_height = band_height.min(crop_h) as f32;
```

The `PREVIEW_MAX_HEIGHT` import is removed from this file.

Update the existing test `preview_viewport_uses_fixed_width_and_bottom_band_height`
to reflect the new cap (crop height instead of 480).

### `rollshot-app/src/components/CaptureOverlay.tsx`

Replace the fixed `MAX_PREVIEW_SIZE`:

```ts
const MAX_PREVIEW_SIZE = { width: 180, height: 260 }
```

With a dynamic computation using the region height:

```ts
const PREVIEW_WIDTH = 280
// In the polling loop, when region is available:
const maxPreview = { width: PREVIEW_WIDTH, height: nextStatus.region.height }
const previewSize = fitPreviewSizeToRegion({ region: nextStatus.region, maxPreview })
```

The `fitPreviewSizeToRegion` function in `placement.ts` already aspect-fits
correctly and needs no changes. The `choosePreviewPlacement` function already
tries right/left/bottom/top/inside and picks the best fit — also unchanged.

### `rollshot-app/src/overlay/placement.ts`

No changes. The placement logic already handles arbitrary preview sizes.

### `rollshot-app/src/components/NativeCaptureFlow.tsx`

Check if this file also uses a hardcoded preview size and update to 280px
width + dynamic height if so.

## Edge cases

| Scenario | Behavior |
|----------|----------|
| Crop taller than band (e.g. crop at screen center) | Height = band space. Preview smaller than crop. |
| Crop shorter than band (e.g. small crop at screen top) | Height = crop height. Preview matches crop. |
| Crop at screen edge with lots of space below | Height = crop height. Full use of space up to crop. |
| Very wide crop (e.g. 2560x200) | Aspect-fit makes preview wide and short. Height still capped at crop. |
| Very tall crop (e.g. 400x1400) | Height = min(1400, band). Width stays 280. Aspect-fit letterboxes. |

## Testing

- Update `preview_viewport_uses_fixed_width_and_bottom_band_height` test in
  `overlay.rs` to verify height = min(crop.h, band).
- Add a test where crop height < band height to verify the crop cap applies.
- Update `placement.test.ts` if the preview size fixture changes.
- Manual verification on both platforms with various crop sizes.
