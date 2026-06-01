# Rollshot Live Preview Viewport Design

Date: 2026-06-01

## Context

Rollshot currently has two live capture UI paths:

- Linux uses the native iced/layer-shell overlay in `crates/rollshot-overlay`.
- macOS and other webview paths use the Tauri/React overlay in `crates/rollshot-app`.

Both paths ultimately depend on `rollshot-core` for stitching and
`rollshot-overlay-core` for shared overlay behavior. The current live stitching
preview uses `preview_with_spotlight`, which aspect-fits the entire stitched
canvas into a fixed box and dims everything outside the current screenful. For
long captures this becomes unreadable: the full stitched canvas keeps shrinking
to fit the box, and the current-frame highlight becomes a thin strip.

The current webview path also polls `get_stitch_preview` every 160ms. That call
can clone, compose, resize, and PNG-encode the full stitched image, which
undercuts the incremental `StripCanvas` append model.

## Goal

Replace the full-canvas minimap preview with a shared viewport preview model
that stays readable for long scrolling captures and can be used by both Tauri
and iced without duplicating UI semantics.

Success criteria:

- Long captures show a readable current-end viewport instead of a shrinking
  full-canvas minimap.
- The preview semantics are shared in Rust, not reimplemented separately in
  React and iced.
- The preview renderer avoids composing or cloning the full stitched canvas on
  every live-preview update.
- Linux native overlay and webview overlay expose the same preview behavior,
  with platform-specific transport only.

## Non-Goals

- Do not implement a snow-shot-style DOM/image-handle filmstrip as the primary
  preview model.
- Do not add interactive preview scrolling in the first version.
- Do not redesign final result preview or save/export behavior.
- Do not change stitch matching behavior except where a read-only preview API is
  needed.

## Chosen Approach

Use a shared viewport renderer in Rust:

```text
rollshot-core stitched canvas
  -> rollshot-overlay-core viewport renderer
    -> Tauri/webview: PNG blob shown in React
    -> iced/native: RGBA ImageHandle shown in iced
```

The live preview is a fixed-size image showing the latest accepted edge of the
stitched canvas. It does not scale the entire stitched canvas into the preview
box. Instead, it renders a crop of the stitched canvas around the current edge,
scaled to the preview box while preserving aspect ratio. A small position
indicator shows where that viewport sits within the total stitched extent.

For vertical captures, the viewport follows the top or bottom edge depending on
the most recent accepted append direction. `Unknown` defaults to bottom, matching
today's behavior. For horizontal captures, it follows the left or right edge.

## Why Not Incremental Filmstrip First

A filmstrip maps naturally to the webview path: React can append `<img>` nodes
inside a scroll container. It does not map cleanly to iced:

- iced would need to manage an unbounded list of image handles.
- overlap offsets, top/bottom prepends, and rollback semantics would move into
  two separate UI layers.
- long captures would grow GPU/image-widget state linearly.
- parity between Tauri and iced would depend on duplicated layout logic.

The viewport model keeps UI state bounded and puts preview semantics in a shared
Rust layer. A filmstrip can still be considered later as an optional advanced or
debug preview, but it should not be the first cross-platform live preview
replacement.

## Data Model

Add shared preview request/result types in `rollshot-overlay-core::preview`.
`rollshot-overlay-core` already depends on `rollshot-core`; `rollshot-core`
must not depend on `rollshot-overlay-core`.

```rust
pub struct ViewportPreviewRequest {
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub frame_width: u32,
    pub frame_height: u32,
    pub edge: CapturedEdge,
}

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

The pixel buffer is RGBA. Transport layers can encode it as PNG for Tauri or
turn it directly into an iced `ImageHandle`.

## Core Preview Access

The renderer needs a bounded crop of the stitched canvas, not always the full
canvas. `rollshot-core` should expose a read-only crop/snapshot primitive on
`Stitcher` or `StripCanvas` that returns image data and canvas dimensions using
types owned by `rollshot-core`, such as:

```rust
pub struct CanvasViewport {
    pub image: RgbaImage,
    pub total_width: u32,
    pub total_height: u32,
    pub x: u32,
    pub y: u32,
}

pub fn canvas_viewport(
    &mut self,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Option<CanvasViewport>
```

`rollshot-overlay-core` computes the desired viewport rectangle from
`ViewportPreviewRequest`, calls this core primitive, then scales and annotates
the returned crop.

The implementation must avoid calling `full_image().clone()` for every preview.
If a first pass composes internally to preserve correctness, that limitation
must be explicit in tests and comments, and the public API must still be
viewport-oriented so it can later be optimized without UI changes.

The target end state is for `StripCanvas` to render only the requested viewport
from its strips into a small output buffer. That keeps live preview work bounded
by preview size plus the visible canvas crop, not by full stitched length.

## Rendering Behavior

For vertical captures:

- Bottom edge: choose a canvas crop ending at `total_height`.
- Top edge: choose a crop starting at `0`.
- Crop width spans the full stitched canvas width.
- Crop height is approximately the selected frame height, clamped to the canvas
  height.

For horizontal captures:

- Right edge: choose a crop ending at `total_width`.
- Left edge: choose a crop starting at `0`.
- Crop height spans the full stitched canvas height.
- Crop width is approximately the selected frame width, clamped to the canvas
  width.

The crop is resized into the fixed preview box with aspect ratio preserved. The
renderer should draw a compact position indicator into the preview image:

- vertical: a narrow track on the right edge;
- horizontal: a narrow track on the bottom edge;
- thumb size is `viewport_extent / total_extent`, with a minimum visible size;
- thumb position is based on `viewport_x/y` and total extent.

This indicator replaces the current dimmed full-canvas spotlight. The primary
image remains readable because it is a viewport, not a full minimap.

## Tauri/Webview Path

`SharedSession::stitch_preview_png` should request a `ViewportPreview` and encode
only that bounded RGBA buffer as PNG. The React component remains simple:

- poll `getStitchPreview` as today;
- create/revoke object URLs as today;
- display the returned image in `AdaptiveStitchPreview`.

The React preview size should match the shared viewport dimensions or request
the dimensions it actually displays. Avoid producing a `280x480` Rust preview
and then forcing it into a `180x260` React box. The first implementation should
make the webview request explicit dimensions from the frontend or align the
frontend CSS box to the Rust constants.

## iced Native Overlay Path

The native overlay should request the same `ViewportPreview` after accepted
stitch progress and convert its RGBA buffer into an iced image handle. The iced
path should not maintain a list of historical preview handles.

To reduce redundant work, the driver should avoid emitting a new preview on
duplicate/no-progress/no-match outcomes unless the capture-miss state changed
and the UI needs to reflect that state. This keeps preview generation tied to
actual canvas changes.

## Error Handling

If no stitched image exists yet, return no preview and show the existing status
text. If the selected region dimensions are invalid, return a command error
rather than silently falling back to a full-canvas minimap.

If preview encoding fails in the Tauri path, surface the existing command error
message. The preview failure should not discard the stitched session or final
image.

## Tests

Add unit tests in `rollshot-overlay-core` for:

- bottom-edge viewport shows the bottom of a tall canvas;
- top-edge viewport shows the top of a tall canvas;
- right/left edge behavior for horizontal canvases;
- position indicator thumb size and placement;
- very long canvas keeps preview dimensions fixed and content readable.

Add app/session tests for:

- `stitch_preview_png` returns viewport-sized output, not a full-canvas minimap;
- repeated duplicate/no-progress outcomes do not require preview regeneration;
- final preview/save behavior remains unchanged.

Add native overlay tests where practical for:

- preview viewport size selection;
- preview emission only on accepted progress or relevant warning-state changes.

## Migration Plan

1. Introduce the viewport preview renderer beside the existing
   `preview_with_spotlight`.
2. Switch the webview path to use the viewport preview while keeping existing UI
   placement.
3. Switch the native iced overlay to the same renderer.
4. Remove or deprecate `preview_with_spotlight` after both paths are migrated and
   tests no longer rely on the old full-canvas spotlight behavior.

## Open Decisions Locked For This Iteration

- The first implementation uses viewport preview as the primary live preview.
- Filmstrip preview is out of scope for this iteration.
- The preview is not user-scrollable in this iteration.
- Shared Rust preview semantics take priority over platform-specific UI
  affordances.
