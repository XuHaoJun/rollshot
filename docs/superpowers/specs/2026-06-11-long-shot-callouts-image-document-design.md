# Long-Shot Callouts and Image Document Design

**Date:** 2026-06-11
**Status:** Approved design, pending implementation plan
**Revision:** 2026-06-11 product review — compact renumbering on Number
Callout delete (D1), labeled output cluster in the toolbar (D2), visual
defaults required as a plan deliverable.
**Scope:** New headless image-document crate and active iced Result Workspace in
`rollshot-app`

## 1. Summary

Rollshot will evolve its existing Result Workspace into a lightweight,
non-destructive image viewer/editor. Its first editing workflow is Long-Shot
Callouts:

- Number Callout
- Text Note
- Opaque Redaction
- Callout Navigator
- Undo and Redo
- Copy Annotated and Copy Original
- Save As annotated output
- Permanent preservation of the original image

The editor is not restricted to long screenshots at the architecture level.
Long screenshots drive navigation and performance requirements, while the
underlying image document remains valid for ordinary screenshots and future
arbitrary-image workflows.

The first release does not expose an Open Image action and does not persist an
editable project format.

## 2. Product Position

The product boundary is:

> A lightweight, non-destructive screenshot image viewer/editor whose first
> editing workflow is Long-Shot Callouts.

Long-Shot Callouts is not a separate editor window and is not limited to images
created by the stitcher. It extends the existing Result Workspace in place.

The design follows the research recorded in:

- `docs/feature-discovery/2026-06-11-next-feature-post-capture-annotation.md`
- `docs/feature-discovery/2026-06-11-long-shot-callouts-annotation-ui-ux-study.md`

## 3. Goals

- Make completed screenshots directly explainable and safely shareable.
- Make annotations practical to create and navigate on very tall images.
- Preserve the original image and original auto-saved file.
- Keep editing non-destructive during the Result Workspace session.
- Keep document behavior independent of iced, windowing, and platform APIs.
- Reuse the same document rendering behavior for live overlays and flattened
  exports.
- Leave a clean path for future Open Image, crop, OCR metadata, and other
  editing capabilities without implementing them now.

## 4. Non-Goals

- Open Image UI or arbitrary-file loading in the first release.
- Editable project or sidecar persistence.
- OCR execution or OCR UI.
- Generic image filters.
- Crop, rotate, freehand, arrows, rectangles, blur, or mosaic.
- Multi-selection, rotation handles, layer ordering, or a layers panel.
- Annotation during capture or stitching.
- Refactoring the deprecated Tauri result path.
- Extracting the iced viewer/editor UI into a reusable UI crate.
- A spatial index for annotations.

## 5. Crate Boundary

### 5.1 `rollshot-image-document`

Create a new workspace crate named `rollshot-image-document`.

It is a headless, framework-neutral, non-destructive image document and editing
engine:

```text
rollshot-app
  toolbar / gestures / viewport / navigator / platform actions
                            |
                            v
rollshot-image-document
  source / annotations / history / geometry / rendering
                            |
                            v
                         image
```

The crate owns:

- The immutable source image.
- The annotation graph.
- Stable annotation IDs.
- Number Callout sequence state.
- Image-space geometry and hit-testing.
- Annotation creation, modification, and deletion.
- Undo and redo history.
- Navigator ordering.
- Flattening the document into an annotated full-resolution image.

The crate does not own or depend on:

- iced or any UI framework.
- Active tools, hover state, pointer state, or drag gestures.
- Zoom, scroll offset, or viewport layout.
- Inline editor focus.
- Navigator visibility.
- Clipboard, file dialogs, file revealing, or platform APIs.
- Capture and stitching.
- OCR execution.

The crate receives completed edits from an editor. It does not model how a UI
gesture produces those edits.

The new crate must include its own `README.md` explaining this positioning,
responsibility boundary, and prohibited UI/platform dependencies.

### 5.2 Why UI Tool State Stays Outside

Active tools and gesture state describe how the user is currently interacting
with one editor, not what the image document contains.

During a drag:

1. `rollshot-app` stores pointer and draft state.
2. The canvas renders the draft as a transient overlay.
3. Pointer release submits one completed document edit.
4. The document records one undo entry.

This prevents pointer movement from polluting document history and keeps the
document usable from future CLI, batch, or alternative UI frontends.

## 6. Document Model

The conceptual first-release model is:

```rust
ImageDocument {
    source: RgbaImage,
    annotations: Vec<Annotation>,
    number_sequence: u32,
    history: History,
}

enum Annotation {
    NumberCallout {
        id: AnnotationId,
        number: u32,
        tip: ImagePoint,
        bubble: ImagePoint,
    },
    TextNote {
        id: AnnotationId,
        position: ImagePoint,
        text: String,
    },
    OpaqueRedaction {
        id: AnnotationId,
        bounds: ImageRect,
    },
}
```

Exact Rust representation is left to the implementation plan. The following
invariants are required:

- `source` cannot be modified through document editing operations.
- Every annotation has a stable ID for selection and Navigator synchronization.
- Annotation geometry is stored in full-resolution image coordinates.
- Number allocation is automatic and deterministic.
- Deleting a Number Callout compactly renumbers the remaining Number Callouts,
  preserving their relative order; the next allocation is one greater than the
  highest remaining number.
- Number sequence and numbering state follow undo and redo, including
  renumbering caused by deletion.
- Empty Text Notes and zero-area Redactions are not committed.
- Flattening never renders selection handles, hover effects, or drafts.

First-release styles are fixed product defaults. The document model may retain
style values needed to render consistently, but the UI does not expose generic
color, font, opacity, or stroke controls.

The implementation plan must define the concrete visual defaults as a reviewed
deliverable — Number bubble shape and contrast treatment, Text Note backing
for legibility over busy content, Opaque Redaction fill, and selection handle
visuals — rather than leaving them to implementation byproduct.

## 7. Result Workspace State

`rollshot-app` continues to own the active Result Workspace.

In addition to the current viewer state, it owns:

- Active tool: Select, Number, Text, or Redact.
- Selected annotation ID.
- Current hover and handle target.
- In-progress gesture draft.
- Inline Text Note draft and focus state.
- Navigator open/closed state.
- Annotation dirty state.
- Original source path and latest annotated export path.

The source and output paths are distinct:

- `source_path`: the original auto-saved capture, when available. It never
  changes because of annotation export.
- `last_export_path`: the most recent successful annotated Save As, when
  available.

`Reveal` opens `last_export_path` when one exists; otherwise it opens
`source_path`. This preserves original identity while making the most recent
durable output easy to find.

## 8. Workspace Layout

Keep the existing Result Workspace structure:

```text
Top row toolbar
Inline success/error message
Scrollable image canvas + optional Navigator drawer
Bottom zoom/status row
```

### 8.1 Top Toolbar

The fixed top row becomes grouped buttons with tooltips and shortcuts:

```text
Close | title | Select, Number, Text, Redact | Undo, Redo |
Navigator | Copy ▼ | Save As | Reveal
```

Creation tools (Select, Number, Text, Redact), Undo, Redo, and Navigator are
icon buttons. The output cluster (Copy ▼, Save As, Reveal) remains
text-labeled (or icon plus label): these are the trust-bearing actions, and
distinctions such as Copy Annotated versus Copy Original must be readable
without hover.

Requirements:

- Tool buttons visibly show active state.
- Undo and Redo visibly show disabled state when unavailable.
- Copy, Save As, and Reveal keep visible text labels; trust-critical output
  distinctions must not rely on hover tooltips.
- Toolbars remain fixed relative to the workspace, not image content.
- Existing zoom and fit controls remain in the bottom status row.
- Primary creation tools remain directly discoverable and do not hide behind
  hover-only menus.

### 8.2 Navigator Drawer

The Navigator is a right-side drawer toggled from the top toolbar.

- It lists all annotations.
- Number Callouts display their number.
- Text Notes display a short text summary.
- Opaque Redactions display `Redaction`.
- Items are ordered by image-space vertical position from top to bottom.
- Ties use horizontal position, then stable annotation ID.
- Clicking an item selects the annotation and scrolls its center into view.
- Selecting on canvas highlights the matching Navigator item.
- Ordinary short images default to Navigator closed.
- Long images default to Navigator open.

The long-image threshold should reuse an existing viewport concept when
possible; the implementation plan must define a deterministic threshold and
tests rather than leaving this to visual judgment.

## 9. Tool Interaction

### 9.1 Select

Select is the default tool.

- Clicking an annotation selects it.
- Dragging an annotation or its handle edits it.
- Dragging empty canvas pans the image.
- `Delete` or `Backspace` deletes the selected annotation.
- Clicking empty canvas clears selection without modifying the document.

Number Callouts expose separate tip and bubble handles. Text Notes expose their
position or bounding handle. Opaque Redactions expose rectangle resize handles.

### 9.2 Number Callout

- Clicking creates a numbered stamp with coincident or default-offset tip and
  bubble positions.
- Dragging creates a callout with separated tip and bubble.
- Committing increments the number sequence.
- The Number tool stays active after commit to support consecutive callouts.
- Selecting a Number Callout permits independent movement of tip and bubble.
- Deleting a Number Callout compactly renumbers the remaining callouts,
  preserving relative order; the deletion and its renumbering form one
  history entry. The next created callout receives the highest remaining
  number plus one.

### 9.3 Text Note

- Clicking the image opens an inline text editor at that image position.
- `Ctrl+Enter` or clicking outside commits the complete text as one edit.
- `Esc` cancels the inline draft without editing the document.
- Double-clicking an existing Text Note opens inline re-editing.
- Committing a text change creates one undo entry, not one entry per keystroke.

### 9.4 Opaque Redaction

- Dragging creates an axis-aligned opaque rectangle.
- Selection permits movement and resizing.
- The live overlay and flattened output use an opaque solid fill.
- The output operation replaces covered output pixels; it does not use blur,
  mosaic, or reversible visual effects.

### 9.5 Escape Behavior

`Esc` handles the most local transient state first:

1. Cancel inline text editing or the current creation/drag draft.
2. Clear current annotation selection.
3. If no transient edit or selection remains, request workspace close using
   the normal dirty-state rules.

## 10. Undo and Redo

The document records one history entry per completed semantic edit:

- Create annotation.
- Delete annotation.
- Move annotation.
- Move Number tip or bubble.
- Resize Opaque Redaction.
- Commit changed Text Note content.

Pointer movement and individual text keystrokes do not create entries.

History requirements:

- Maximum 100 undo entries.
- A new edit after undo clears redo history.
- Undo and redo restore annotation graph and Number sequence state, including
  compact renumbering caused by deletion — undoing a Number Callout deletion
  restores the exact prior numbering.
- Selection and active tool are editor session state and do not enter document
  history.
- Undoing removal or creation retains stable annotation identity where
  practical so Navigator synchronization remains predictable.

## 11. Rendering

### 11.1 Live Workspace Rendering

- Continue using the existing image display handle for the base image.
- Render committed annotations as a separate overlay using the same image-space
  transform as the base image.
- Render the current editor draft separately from committed annotations.
- Cull committed annotations whose geometry bounds do not intersect the visible
  viewport.
- Do not rebuild or upload a complete composited `RgbaImage` on pointer
  movement.

The implementation plan must choose the iced rendering mechanism after a small
technical validation, but the document crate must remain independent of it.

### 11.2 Flattened Output

`rollshot-image-document` renders annotated output from the full-resolution
source image:

1. Clone or allocate an output image from the source.
2. Render every committed annotation in full-resolution image coordinates.
3. Return the flattened `RgbaImage`.

Selection, hover, Navigator state, viewport scale, and transient drafts are
excluded.

Flattening occurs only for explicit Copy Annotated or Save As actions. It does
not occur after every document edit.

The renderer used for flattening and the geometry used by the live overlay must
share enough rules that the visible result and output do not diverge.

## 12. Copy, Save, Reveal, and Dirty State

### 12.1 Copy

- The primary Copy action copies the current annotated flattened image.
- A directly adjacent menu exposes Copy Original.
- Copy Original copies the immutable source image.
- Successful Copy does not clear annotation dirty state because clipboard
  contents are not a durable saved output.
- Copy does not close the workspace.

When no annotations exist, Copy Annotated and Copy Original are pixel-identical;
the same UI remains available for consistency.

### 12.2 Save As

- With annotations, Save As writes the flattened annotated image.
- Without annotations, Save As writes the original image.
- Save As never overwrites or changes `source_path`.
- A successful Save As updates `last_export_path` and clears annotation dirty
  state.
- Editing after a successful Save As makes annotations dirty again.
- Save As does not close the workspace.

### 12.3 Close Confirmation

Closing requires confirmation when:

- The original capture has no durable `source_path`, following existing
  unsaved-capture behavior; or
- Annotation edits are dirty relative to the last successful Save As.

The confirmation must clearly distinguish discarding annotation edits from
discarding an unsaved original capture.

Because the first release has no editable project format, closing always loses
the editable annotation graph even after Save As. A successful Save As permits
closing without an additional warning because the flattened visible result is
durable and the original remains preserved.

## 13. Long-Image Behavior and Performance

- The existing viewport remains the authority for zoom and pan.
- Entering an annotation tool does not reset zoom or scroll position.
- Navigator jumps use the existing scrollable and viewport geometry.
- Hit-testing uses annotation geometry bounds and shape-specific checks.
- First release performs a linear annotation scan; no spatial index is added.
- Navigator order is recomputed only when annotation geometry or membership
  changes.
- Live overlay rendering culls off-viewport annotations.
- Full-resolution flattening occurs only for explicit output.

This design assumes a moderate annotation count. Performance tests should cover
a long image with at least 100 annotations, matching the history limit scale.

## 14. Result Workspace Refactor

The current `result_workspace/mod.rs` already combines document state, update
logic, viewport event routing, view construction, and tests. Annotation should
not be added entirely to that file.

Target module responsibilities:

```text
result_workspace/
  mod.rs          orchestration and run entry point
  document.rs     app-facing image-document and path integration
  update.rs       messages and state transitions
  view.rs         workspace chrome and toolbar
  canvas.rs       iced overlay and gesture state
  navigator.rs    annotation list and jump behavior
  actions.rs      clipboard, Save As, and Reveal
  viewport.rs     existing zoom and pan math
```

The implementation plan may adjust exact file boundaries when existing iced
lifetimes or test placement make a different split simpler. The required
architectural result is:

- Headless document behavior in `rollshot-image-document`.
- UI/session behavior in `rollshot-app`.
- No annotation implementation added to capture overlays or deprecated Tauri
  code.

## 15. Error Handling

- Document edits that violate invariants return explicit errors or are rejected
  without modifying history.
- A failed Copy or Save As leaves the document and dirty state unchanged and
  displays an inline error.
- A failed flatten leaves the document unchanged.
- Navigator selection of an annotation removed by a preceding edit is ignored
  and selection is cleared.
- Losing inline text focus commits only when the current draft is valid;
  otherwise it cancels without creating an empty annotation.

## 16. Testing and Verification

### 16.1 `rollshot-image-document`

Unit tests must cover:

- Source image remains unchanged across every edit and flatten operation.
- Number allocation and sequence restoration through undo/redo.
- Compact renumbering after Number Callout deletion, including allocation
  after delete (highest remaining number plus one).
- Undo of a Number Callout deletion restoring the exact prior numbering.
- Create, move, resize, text edit, delete, undo, and redo.
- Redo clearing after a new edit.
- 100-entry history limit.
- Stable IDs across relevant undo/redo operations.
- Hit-testing for each annotation and Number handles.
- Deterministic Navigator ordering.
- Flattened Number, Text, and Opaque Redaction output.
- Opaque Redaction replaces covered output pixels.
- Empty Text and zero-area Redaction rejection.

### 16.2 `rollshot-app`

Tests must cover:

- Existing viewer zoom, pan, copy, Save As, Reveal, and close behavior remains
  valid.
- Active tool and selection transitions.
- Drag drafts commit exactly one document edit.
- Text typing commits exactly one document edit.
- Navigator and canvas selection stay synchronized.
- Navigator jump computes the correct viewport target.
- Copy Annotated versus Copy Original routing.
- Copy does not clear dirty state.
- Successful Save As updates `last_export_path` without changing `source_path`
  and clears dirty state.
- Failed Save As leaves paths and dirty state unchanged.
- Dirty annotation edits require close confirmation.
- `Esc` priority behavior.
- Long-image Navigator default versus short-image default.

### 16.3 Commands

- `rtk cargo test -p rollshot-image-document`
- `rtk cargo test -p rollshot-app`
- `rtk cargo test --workspace`
- `rtk cargo fmt --check`
- `rtk cargo clippy --workspace --all-targets -- -D warnings`
- `rtk git diff --check`

Runtime verification is required on Linux and macOS for pointer gestures,
inline text focus, clipboard behavior, Save As, Reveal, zoom/pan interaction,
Navigator jumps, icon sizing, and long-image responsiveness.

## 17. Implementation Constraints

- Preserve existing user changes and unrelated worktree changes.
- Make no changes to the deprecated Tauri result path.
- Keep platform behavior aligned because Result Workspace is shared active iced
  code.
- Add no speculative OCR, Open Image, project persistence, style system, or
  generic editor APIs.
- Do not commit the design or implementation unless the user explicitly asks.
