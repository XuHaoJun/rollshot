# Wayscrollshot-Style Live Preview Height Design

**Date:** 2026-06-02
**Status:** Draft
**Scope:** Linux native overlay and macOS/webview vertical stitching live preview

## Goal

Make the stitching live preview behave like the wayscrollshot reference:

- Preview width is fixed at 280 px.
- Preview height grows with the scaled stitched image content.
- Preview height is capped by the selected crop height and the available
  placement band.
- Once the scaled stitched image is taller than the cap, the preview keeps the
  capped height and shows the current/bottom slice of the stitched image.

This replaces the current Rollshot behavior where the macOS/webview live preview
height is derived from the crop aspect ratio, so wide crops produce a short
preview even after the stitched content grows.

This spec targets vertical scrolling captures. Rollshot's existing horizontal
Left/Right live preview behavior is preserved for this iteration because
wayscrollshot's reference model is vertical and fixed-width scaling does not map
cleanly to horizontal growth.

## Reference Behavior

`learn-projects/wayscrollshot` uses two separate steps:

1. `stitch.rs::build_preview()` scales the stitched full image to a fixed width.
   The resulting preview image height is proportional to the stitched image
   height.
2. `overlay.rs::desired_size_from_preview()` sizes the overlay to
   `min(preview.height, region.h - control_bar_height)`.

That means the width stays fixed and only height changes. The panel does not
start at crop height unless the scaled stitched preview is already that tall.

## Current Rollshot Behavior

The shared `rollshot-overlay-core::preview::viewport_preview()` takes a requested
viewport width and height, crops a frame-sized window from the stitched canvas,
then aspect-fits that window into the requested viewport. It always returns an
image exactly matching the requested dimensions.

That behavior is useful for a viewport thumbnail, but it is not the wayscrollshot
model. If Rollshot only increases the requested viewport height, the current
frame window becomes letterboxed inside a taller white image instead of showing
a taller stitched preview.

## Proposed Architecture

Add a second shared preview builder in `rollshot-overlay-core`, separate from
`viewport_preview()`:

```rust
pub struct GrowingPreviewRequest {
    pub fixed_width: u32,
    pub max_height: u32,
    pub edge: CapturedEdge,
}

pub struct GrowingPreview {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub scaled_full_height: u32,
    pub total_width: u32,
    pub total_height: u32,
}
```

`growing_preview()` scales `Stitcher::full_image()` to `fixed_width`, then returns
either the full scaled preview if it fits `max_height`, or a cropped slice if it
is taller than `max_height`.

For vertical captures:

- `CapturedEdge::Bottom` and `CapturedEdge::Unknown` return the bottom slice.
- `CapturedEdge::Top` returns the top slice.

For horizontal captures (`CapturedEdge::Left` or `CapturedEdge::Right`), keep the
existing viewport-thumbnail behavior. Horizontal growing-preview semantics are a
non-goal for this iteration.

Keep `viewport_preview()` for existing tests and any code paths that need the
old viewport-thumbnail semantics. Move live stitching preview paths to the new
growing preview helper.

## macOS/Webview Data Flow

`CaptureOverlay.tsx` already receives stitching stats:

```ts
status.stats.total_width
status.stats.total_height
```

The frontend should compute the intended displayed preview height before
requesting the PNG:

```ts
scaledHeight = round(status.stats.total_height * 280 / status.stats.total_width)
previewHeight = min(scaledHeight, cropCssHeight, availableBandHeight)
```

The placement helper should take the scaled content height as input and return:

- `rect.width = 280` or less if the band width is narrower.
- `rect.height = min(scaledContentHeight, crop.height, availableHeight)`.
- `preview.width` and `preview.height` matching the displayed rect.

`CaptureOverlay.tsx` must use the same placement result for:

- `getStitchPreview(preview.width, preview.height)`
- `AdaptiveStitchPreview` display placement

This keeps the requested PNG dimensions and displayed panel dimensions in sync.

## Linux Native Overlay Data Flow

The Linux native overlay currently computes a preview size once when stitching
starts and passes that fixed size to `Driver::begin_stitch()`.

To match the growing behavior, pass preview constraints instead:

```rust
pub struct PreviewConstraints {
    pub fixed_width: u32,
    pub max_height: u32,
}
```

`overlay.rs` still computes band-aware constraints from the crop and window:

```rust
fixed_width = min(280, available_band_width)
max_height = min(crop.height, available_band_height_after_toolbar)
```

`driver.rs` then calls the new shared `growing_preview()` after each accepted
vertical stitch update. The new preview image handle naturally grows in height
as the stitched full image grows, capped by `max_height`. Left/Right stitch
updates continue to use the existing viewport-thumbnail preview path.

## Placement Rules

Candidate order stays unchanged:

1. right
2. left
3. bottom
4. top
5. inside only when overlay exclusion is verified

For each candidate, compute available width and height before sizing:

```ts
availableWidth = side-specific free width
availableHeight = side-specific free height
previewWidth = min(280, availableWidth)
previewHeight = min(scaledContentHeight, crop.height, availableHeight)
```

Return the first candidate whose rect fits. If no outside candidate fits and
overlay exclusion is not verified, return status-only.

## Edge Cases

| Scenario | Behavior |
| --- | --- |
| First stitched frame, wide crop | Preview is short because scaled stitched content is short. |
| More vertical content accepted | Preview height grows as `total_height` grows. |
| Scaled content exceeds crop height | Preview height stays capped at crop height. |
| Crop is near screen edge | Preview height is capped by available band height. |
| Full-screen crop on macOS with verified overlay exclusion | Preview may use inside placement and still grows up to crop height. |
| Full-screen crop without verified overlay exclusion | Status-only, because drawing inside the crop could self-capture. |
| No accepted stitch progress | Preview height does not grow. |
| Horizontal Left/Right stitch | Preserve the existing viewport-thumbnail behavior. |

## Tests

### Core Rust Tests

Add tests for `growing_preview()`:

- Scales a stitched full image to fixed width and proportional height.
- Caps height to `max_height` and returns the bottom slice for bottom/unknown
  vertical captures.
- Returns the top slice for top captures.
- Clamps zero requested width/height to one pixel.
- Leaves horizontal Left/Right captures to the existing viewport-thumbnail path.

### Linux Overlay Tests

Update `rollshot-overlay` tests:

- `preview_viewport_size()` should become a constraint helper test.
- Add a driver/core test showing accepted vertical stitch growth increases the
  generated preview image height until the max height.
- Add a driver/core test showing horizontal stitch preview behavior is unchanged.

### Webview Tests

Update `placement.test.ts`:

- A crop with small `scaledContentHeight` returns a short fixed-width preview.
- Larger `scaledContentHeight` returns a taller preview.
- Height caps at crop height.
- Height caps at candidate band height.
- Verified inside placement uses the same dynamic sizing.

Update `CaptureOverlay.test.tsx`:

- A stitching status with smaller `total_height` requests a short preview.
- A later status with larger `total_height` requests a taller preview.
- The requested preview height caps at crop height.
- The verified inside-placement regression remains covered.
- Horizontal stitch statuses keep existing preview sizing behavior.

## Non-Goals

- Do not change final preview or save behavior.
- Do not add user-facing settings for preview size.
- Do not change crop selection, input passthrough, or overlay exclusion logic.
- Do not replace the existing `viewport_preview()` helper globally; only move
  vertical live stitching preview paths to the new growing-preview behavior.
- Do not redesign horizontal Left/Right live preview semantics.
