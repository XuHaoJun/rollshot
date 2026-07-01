# OCR Selectable Text Design

## Summary

Rollshot will add a product-level OCR text selection feature to the result
workspace. The first version must feel like real text selection: users enter a
dedicated OCR Text tool, drag across recognized text with visible selection
highlighting, and press Ctrl/Cmd+C to copy the selected text. "Copy all OCR
text" is a secondary fallback, not the primary interaction.

The OCR data stays ephemeral in the result workspace. It is not written into
`rollshot-image-document`, saved files, history, or diagnostics.

OCR must respect redaction annotations: text covered by a redaction is never
rendered, selectable, or copyable, recomputed against the current redactions.

## Goals

- Add a dedicated OCR Text mode in the result workspace.
- Run product OCR on the captured image and cache results in memory.
- Render recognized text over the image with image-space alignment.
- Support high-quality selection: partial text, cross-line ranges, reverse
  drag direction, visible selection highlights, Ctrl/Cmd+A, and Ctrl/Cmd+C.
- Keep annotation tools and OCR selection mutually exclusive so pointer
  behavior remains predictable.
- Reuse the existing OCR backend architecture without tying the product UI to
  Smart Redaction workbench internals.

## Non-Goals

- Persisting OCR text in `ImageDocument` or saved result metadata.
- Making OCR text editable.
- Translating OCR text.
- Using a DOM, webview, or iframe inside Rollshot.
- Replacing Smart Redaction's existing agent OCR capability path.
- Building a generic OCR platform settings UI in this first feature.

## Product Review Decisions (2026-07-01)

Locked during `/plan-ceo-review`:

1. Redaction safety is a hard invariant, not a byproduct of mode exclusivity.
   OCR items covered by a redaction annotation are excluded from render,
   selection, and copy, recomputed as redactions change. Mode exclusivity
   governs the pointer; annotation-masking governs the data.
2. The feature ships opt-in behind the `ocr` build feature. When the app is
   built without `ocr`, the OCR Text tool is omitted from the toolbar entirely
   (compile-time gated), not shown permanently disabled.
3. v1 selection fidelity targets axis-aligned line/block and cross-line
   selection. Rotated-text character hit-testing and the low-confidence
   interaction tier are deferred to v2.
4. No top-level "Copy Text" toolbar button. Ctrl/Cmd+C is the primary copy
   path; copy-all is a separate explicit, labeled in-mode action.
5. Clean selection across tile seams on long captures is a v1 acceptance bar,
   not a footnote.

## Context

Rollshot already has OCR infrastructure:

- `crates/rollshot-ocr` wraps RapidOCR/ONNX Runtime in an unsafe-isolation
  crate and returns input-native OCR detections.
- `crates/rollshot-vision` exposes OCR through `RealAutomationHost` and maps
  OCR regions back into full-image coordinates.
- `crates/rollshot-app` enables OCR through the `ocr` Cargo feature and uses it
  in Smart Redaction.

The product result workspace already renders the image as an iced image widget
stacked with an annotation `Canvas`. The zoom/scroll math is centralized in
`result_workspace::viewport`, and annotation pointer events already translate
canvas-local positions into image coordinates.

Snow Shot's OCR UX is a useful reference. It has dedicated OCR draw states,
disables the annotation canvas while OCR selection is active, turns OCR boxes
into absolutely positioned DOM text, and lets the browser handle text
selection in an iframe. Rollshot cannot reuse that DOM selection machinery in
iced, so it must implement its own text-selection model.

## UX Model

The result workspace toolbar gains an OCR Text tool.

When the user activates OCR Text:

1. Rollshot prepares OCR if the current image has no OCR cache.
2. The canvas enters OCR Text mode.
3. Annotation selection, annotation dragging, redaction creation, text-note
   editing, and pan-drag behavior are disabled for left-button OCR selection.
4. Recognized text is shown as a selectable overlay. Text covered by a
   redaction annotation is excluded from the overlay entirely.
5. Dragging across text creates a selection with a visible highlight.
6. Ctrl/Cmd+C copies the selected OCR text. With no selection it shows "No OCR
   text selected" rather than silently copying everything.
7. Ctrl/Cmd+A selects all OCR text.
8. Escape clears OCR selection first. If no OCR selection exists, Escape leaves
   OCR Text mode and returns to Select. If already in Select with no draft, the
   existing close behavior applies.

The existing image Copy button remains image-copy by default and is not
duplicated with an OCR variant in the toolbar. Keyboard copy (Ctrl/Cmd+C) is the
primary path for selected OCR text. "Copy all OCR text" is a separate, explicitly
labeled in-mode action (surfaced contextually while OCR Text mode is active, e.g.
the status bar), never a silent fallback of an empty copy.

## Architecture

### Product OCR State

Add a new result-workspace OCR module:
`crates/rollshot-app/src/result_workspace/ocr_text.rs`.

It owns:

- OCR lifecycle state: idle, preparing, ready, unavailable, failed.
- In-memory OCR items, including text, confidence, full-image bounds, and
  full-image quadrilateral points. The OCR backend contract must preserve
  quadrilateral geometry instead of reducing detections to only axis-aligned
  bounds.
- Reading-order normalization.
- Line grouping.
- Selection state.
- Pure helpers for hit-testing, range construction, selected-text formatting,
  select-all, and copy-all.
- Redaction-aware filtering: OCR items intersecting redaction annotation bounds
  are excluded from render, selection, and copy, recomputed against the current
  redactions rather than only at OCR time.

`ResultWorkspace` gets an `ocr_text` field. The field is UI/session state, like
`EditorState`; it does not belong in `ResultDocument` or
`rollshot-image-document`.

### OCR Execution

Product OCR should not call Smart Redaction's `workbench::run` helpers. Those
helpers prepare agent-oriented canonical regions, not a user-visible full text
surface.

The implementation adds a small product OCR preparation helper in
`rollshot-app` that reuses lower-level OCR capability code:

- Use `rollshot-vision` / `rollshot-ocr` when built with the `ocr` feature.
- Return a typed product OCR result in full-image coordinates.
- Report `ocr_disabled`, `ocr_region_too_large`, `ocr_session_init`, and
  `ocr_detect` as stable user-facing states.

For screenshots under the existing OCR area limit, product OCR runs on the full
image. For tall captures or images over the OCR area limit, product OCR uses
deterministic vertical tiles with overlap and merges duplicate detections by
IoU/text similarity. The UX must not silently show only top and edge strips as
selectable text.

### OCR Text Tool

Extend `result_workspace::canvas::Tool` with `OcrText`.

Tool behavior is mutually exclusive:

- `Select`: annotation selection, annotation movement, text-note editing, and
  pan behavior.
- `Number`: number callout creation.
- `Text`: text-note creation.
- `Redact`: redaction creation/editing.
- `OcrText`: OCR text selection and OCR text copy.

This follows Snow Shot's design: OCR is a dedicated mode, and annotation canvas
pointer interactions are disabled while OCR selection owns the pointer.

### Text Selection Layer

The first version must support real text-range selection, not only whole OCR
block selection.

The rendering surface is a custom iced `advanced::Widget` named
`OcrTextLayer`. It lives in the existing scrollable image stack, not in an iced
overlay, because it must scroll and zoom with the image.

The layer is stacked above the image and annotation canvas:

```text
stack![
    image,
    annotation_canvas,
    OcrTextLayer,
    inline_text_editor_when_active
]
```

The text layer responsibilities:

- Convert OCR item geometry from image coordinates to canvas-local logical
  pixels using the same scale as annotations.
- Build text layouts for each OCR line/block.
- Hit-test mouse points into text cursor positions.
- Track drag anchor and focus cursor.
- Compute selection ranges across OCR lines.
- Draw selected text highlights.
- Draw OCR text in a way that aligns with OCR bounds.
- Return text cursor interaction while hovering selectable OCR text.
- Capture mouse events during OCR selection so scroll/pan/annotation logic
  cannot also consume them.

Iced 0.14 does not expose reusable generic text selection outside
`text_input` / `text_editor`. `OcrTextLayer` uses lower-level text layout
concepts such as `advanced::text::Paragraph` for measurement and grapheme
hit-testing, while the selection model remains Rollshot-owned.

### Selection Semantics

Selection is ordered by a normalized text stream:

- OCR blocks are grouped into lines using y-overlap and baseline proximity.
- Lines are sorted top-to-bottom.
- Text within a line is sorted left-to-right for horizontal text.
- Selection ranges are stored as text cursor positions in that normalized
  stream, not as unordered rectangles.
- Dragging backward produces the same copied text as dragging forward over the
  same range.

Line breaks are inserted between normalized OCR lines. Text fragments on the
same line are separated by spaces when their geometry indicates a visible gap.

v1 targets axis-aligned text. Rotations under three degrees are snapped to
horizontal for display and hit-testing. Larger rotations are deferred to v2: in
v1 they are not character-selectable (at most coarse quad-level block
selection). The selection result must remain deterministic.

### Clipboard

Keyboard routing lives at the result workspace level because Canvas is not a
focusable text widget by default.

Add messages for:

- OCR preparation request/finish/failure.
- OCR text selection start/update/end.
- Select all OCR text.
- Copy selected OCR text.
- Copy all OCR text.

Clipboard writing uses the existing iced task/message pattern through a
`copy_text` helper beside the existing `copy_image` action. Tests cover
message/state behavior without relying on a live platform clipboard.

### Privacy And Diagnostics

OCR text may contain private information. The implementation must not log raw
OCR text, selected text, or image contents. Tracing may include counts, timing,
image dimensions, region dimensions, and stable error codes.

Debug implementations for OCR cache state must avoid printing recognized text.

## Error Handling

When the app is built without the `ocr` feature, the OCR Text tool is omitted
from the toolbar entirely (compile-time gated). A build-time-absent capability
is not shown as a permanently disabled control, which would imply it could be
enabled in-app.

If OCR fails, the workspace remains usable and returns to Select mode. The
error message should identify the stable category without exposing OCR text.

If OCR returns no text, OCR Text mode remains active and shows an empty state.
Ctrl/Cmd+C shows "No OCR text selected" instead of copying an empty string
silently.

## Testing Strategy

Add pure tests for:

- OCR reading-order normalization.
- Line grouping.
- Selection range construction in forward and reverse drag directions.
- Selected-text formatting across lines.
- Select-all text formatting.
- Hit-testing cursor positions within a text item.
- OCR cache state transitions.

Add result workspace update tests for:

- Entering OCR Text mode requests OCR preparation.
- Annotation drag/edit behavior is not triggered in OCR Text mode.
- Escape clears OCR selection before leaving OCR Text mode.
- Ctrl/Cmd+A selects all OCR text in OCR Text mode.
- Ctrl/Cmd+C copies selected OCR text in OCR Text mode.
- OCR unavailable and OCR failed states produce inline messages.

Add rendering-adjacent tests where possible for:

- Image-coordinate to canvas-local transforms.
- Selection highlight rectangles under zoom.
- Visible-region culling for long screenshots.
- Selection continuity across tile seams on long captures (a range spanning a
  tile boundary copies as one clean run).
- Redaction-covered OCR items are excluded from selection and copy.

OCR backend integration tests should remain in the OCR feature lane.

## Risks

- Iced has no DOM selection engine; text selection must be implemented by
  Rollshot.
- Character-level hit-testing and font metrics may differ across platforms.
- Long screenshots can produce many OCR items; layout and hit-testing need
  culling or indexing.
- OCR model output ordering may not match reading order, so Rollshot must
  normalize it.
- Product OCR full-image behavior for captures over the OCR area limit needs a
  tiling policy, not Smart Redaction's strip catalog.

## Product Decisions

- OCR Text mode starts OCR immediately on activation when no cache is ready.
- Copied text uses Rollshot normalized reading order, not backend output order.
- Builds without the `ocr` feature omit the OCR Text toolbar item entirely
  (compile-time gated), rather than showing it disabled.
- The low-confidence interaction tier (faint overlay for text below 0.30 with
  drag-only inclusion) is deferred to v2.
