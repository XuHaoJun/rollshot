# rollshot-image-document

A headless, framework-neutral, non-destructive image document and editing
engine. The first consumer is Rollshot's Result Workspace (Long-Shot
Callouts), but the document is valid for any raster image.

## Owns

- The immutable source image (never modified by document edits).
- The annotation graph (Number Callouts, Text Notes, Opaque Redactions)
  with stable annotation IDs.
- Number Callout sequence state, including compact renumbering on delete.
- Image-space geometry, hit-testing, and Navigator ordering.
- Undo/redo history (snapshot-based, max 100 entries).
- Flattening the document into an annotated full-resolution image.
- The shared `RenderShape` geometry model used by both the flattened output
  and any live overlay renderer, so the two cannot diverge.

## Must NOT depend on or contain

- iced or any UI framework.
- Active tools, hover state, pointer state, or drag gestures.
- Zoom, scroll offset, viewport layout, or editor focus.
- Clipboard, file dialogs, file revealing, or platform APIs.
- Capture, stitching, or OCR execution.

The crate receives **completed** edits from an editor. A drag gesture in a UI
produces exactly one document edit on release; pointer movement never enters
this crate or its history.

## Fonts

`assets/fonts/` vendors DejaVu Sans (regular + bold) as the deterministic
baseline for flattened text; cosmic-text falls back to system fonts for
glyphs DejaVu lacks (e.g. CJK). See `assets/fonts/LICENSE-DejaVu`.
