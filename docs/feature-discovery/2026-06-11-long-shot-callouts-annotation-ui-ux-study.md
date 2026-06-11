# Long-Shot Callouts: Annotation UI/UX Study

**Date:** 2026-06-11  
**Status:** UI/UX discovery; no implementation started  
**References:** Rollshot Result Workspace, Snow Shot, Mark Shot, Flameshot,
Spectacle

## Correction to the Earlier Product Boundary

There is no good reason to restrict the underlying viewer/editor to long
screenshots.

Rollshot's existing Result Workspace already behaves like an image viewer:

- It owns an immutable full-resolution `source_image`.
- It creates a separate, possibly downscaled display handle.
- It supports zoom, pan, copy, Save As, and reveal.
- Its canvas works for both ordinary screenshots and tall stitched images.

Long screenshots should influence navigation and performance requirements, not
artificially restrict which images the editor can display. The more defensible
product boundary is:

> A lightweight, non-destructive screenshot image viewer/editor whose first
> editing workflow is Long-Shot Callouts.

This does not require shipping an **Open Image** entry point in the first
release. It means the document model and editor architecture should not assume
that every image is tall or came from Rollshot's stitcher.

## Existing Rollshot UI Baseline

The current Result Workspace layout is:

1. A fixed top row toolbar:
   `Close | title | Copy | Save As | Reveal`
2. An optional inline success/error message row.
3. A central zoomable and two-axis scrollable image canvas.
4. A fixed bottom status row:
   dimensions, zoom status, fit modes, and zoom controls.

The existing layout is already a suitable foundation. Annotation should extend
it rather than replace it with a new left-hand tool palette by default.

The current implementation uses text buttons, but screenshot editors commonly
use icon buttons with tooltips and keyboard shortcuts once the action count
grows. A row toolbar remains appropriate for Rollshot's small first-release
tool set.

Relevant Rollshot code:

- `crates/rollshot-app/src/result_workspace/mod.rs:34`
- `crates/rollshot-app/src/result_workspace/mod.rs:129`
- `crates/rollshot-app/src/result_workspace/mod.rs:483`
- `crates/rollshot-app/src/result_workspace/mod.rs:548`
- `crates/rollshot-app/src/result_workspace/mod.rs:607`
- `crates/rollshot-app/src/result_workspace/viewport.rs`

## Cross-Product Findings

| Product | Primary annotation surface | Toolbar strategy | Long-image navigation | Original handling |
| --- | --- | --- | --- | --- |
| Snow Shot | Same full-screen capture overlay | Movable horizontal icon row near selection; contextual floating properties | Scroll capture has a thumbnail strip, but annotation is disabled during that flow | No explicit original action |
| Mark Shot | Same capture window or full-screen image annotation window | Dense horizontal annotation row plus separate vertical action row | Interactive detail preview and overview mini-map | Frozen source exists internally; no explicit original action |
| Flameshot | Same capture overlay | Icon buttons dynamically arranged around selection | Not designed around a completed tall-image workspace | Original retained during session; exports are flattened |
| Spectacle | Integrated result viewer with an Edit mode | Fixed top action row; annotation tools slide in at left and properties at bottom | Viewer supports pan/zoom; no dedicated callout navigator | Base image retained internally; no explicit original action |

### Strong Common Patterns

All four products use an object-based annotation layer over an unchanged base
image during the editing session. Copy and Save flatten the current annotation
state into an output bitmap.

All four distinguish tool selection from image content. Creation tools are
single-select, selected tools have a visible active state, and undo/redo are
first-class actions.

All four keep output actions readily accessible. None treats annotation as a
separate general-purpose image-editing application with filters, image
management, or a multi-document workflow.

## Product-Specific Findings

### Snow Shot

Snow Shot uses a movable horizontal icon toolbar placed near the selected
region. Tools are grouped with separators:

`navigation/selection | drawing | undo/redo | utilities | save | cancel/copy`

Related tools can share one slot through a popover that remembers the most
recent choice. Selected-object properties appear in a separate movable
contextual panel.

Its strongest long-image idea is a thumbnail strip that communicates position.
However, its scroll capture and annotation flows are mutually exclusive. This
is precisely the gap Rollshot can address.

Worth borrowing:

- Clear tool grouping in a horizontal toolbar.
- Icon buttons with active/disabled states, tooltips, and shortcuts.
- Contextual properties outside the primary toolbar.
- A compact overview for long-image navigation.

Do not borrow:

- A toolbar that mixes annotation, OCR, pinning, scroll capture, recording,
  cloud actions, and output.
- Hover-only discovery for primary tools.
- Disabling annotation for long captures.

References:

- `learn-projects/snow-shot/src/pages/draw/components/drawToolbar/index.tsx:756`
- `learn-projects/snow-shot/src/pages/draw/components/drawToolbar/index.tsx:886`
- `learn-projects/snow-shot/src/pages/draw/components/drawToolbar/components/toolButton/index.tsx:36`
- `learn-projects/snow-shot/src/components/drawCore/excalidrawRenders/layoutMenuRender.tsx:47`
- `learn-projects/snow-shot/src/pages/draw/components/drawToolbar/components/tools/scrollScreenshotTool/index.tsx:714`

### Mark Shot

Mark Shot has the closest implementation of the proposed Number Callout:

- Click creates a numbered stamp.
- Drag separates the pointer tip from the number bubble.
- Tip and bubble have independent edit handles.
- The sequence increments automatically and can be reset.

Text is edited inline on the image, including re-editing through double-click.
Undo/redo snapshots the annotation graph and counters, not the frozen image.

Mark Shot also has the strongest long-image navigation reference: its scroll
preview combines a local detail view with an overview mini-map. Users can drag
the overview viewport or use the wheel to navigate.

Worth borrowing:

- Number Callout creation and two-handle editing.
- Inline Text Note editing.
- Immutable base image plus vector annotation history.
- Interactive overview navigation.

Do not borrow:

- Its very dense annotation toolbar.
- Mosaic as a security redaction feature.
- Multi-select, rotation, magnifier, laser, and broad styling in the first
  release.
- Copy or Save automatically closing the editor.

References:

- `learn-projects/mark-shot/src/shot_window_setup.cpp:339`
- `learn-projects/mark-shot/src/shot_window_canvas.cpp:540`
- `learn-projects/mark-shot/src/shot_window_input.cpp:675`
- `learn-projects/mark-shot/src/shot_window_annotation_editing.cpp:34`
- `learn-projects/mark-shot/src/shot_window_annotation_painting.cpp:643`
- `learn-projects/mark-shot/src/scroll/scroll_session_window_preview.cpp:222`

### Flameshot

Flameshot minimizes the capture-to-annotation transition by keeping selection,
annotation, and output on one surface. Its icon-only tools dynamically surround
the capture region and use tooltips for names and shortcuts.

The editor keeps an original screenshot plus editable objects. Each update
rebuilds the composited preview from the original and object list. Existing
objects can be selected, moved, recolored, resized by stroke width, and deleted.

Worth borrowing:

- Direct object selection and movement.
- Keyboard-first delete, undo, redo, copy, and save.
- Clear active tool state.
- Hiding selection handles from flattened output.

Do not borrow:

- Dynamically placing tools around a tall image.
- Mixing capture-region selection with annotation-object selection.
- Repainting the complete full-resolution image after every object change.
- Treating flattened output as the only useful result.

References:

- `learn-projects/flameshot/src/widgets/capture/buttonhandler.cpp:66`
- `learn-projects/flameshot/src/widgets/capture/capturetoolbutton.cpp:133`
- `learn-projects/flameshot/src/widgets/capture/capturetoolobjects.cpp:64`
- `learn-projects/flameshot/src/widgets/capture/modificationcommand.cpp:7`
- `learn-projects/flameshot/src/widgets/capture/capturewidget.cpp:1839`

### Spectacle

Spectacle most closely matches Rollshot's current product surface. It presents
the completed screenshot in a result viewer, then toggles annotation through an
Edit action without opening another application or replacing the image view.

Its top row retains global actions such as Save, Copy, and Export. Annotation
tools slide in at the left, while contextual properties and zoom controls appear
at the bottom. This is effective for Spectacle's broad tool set, but Rollshot's
first release is small enough to keep its creation tools in the existing top
row.

An important detail is history coalescing: continuous property changes are
merged before becoming undo entries, preventing a slider drag from filling the
undo stack.

Worth borrowing:

- Viewer and editor as modes of the same workspace.
- Global output actions remaining stable across modes.
- Separate Select and creation tools.
- Contextual properties driven by the active tool or selected object.
- Coalescing continuous edits into one undo operation.

Do not borrow:

- Switching to 100% zoom when annotation begins.
- A broad annotation suite in the first release.
- Relying on undo as the only way to recover or export the original.

References:

- `learn-projects/spectacle/src/Gui/ViewerPage.qml:37`
- `learn-projects/spectacle/src/Gui/ViewerPage.qml:102`
- `learn-projects/spectacle/src/Gui/ViewerPage.qml:151`
- `learn-projects/spectacle/src/Gui/EditAction.qml:8`
- `learn-projects/spectacle/src/Gui/AnnotationOptionsToolBarContents.qml:15`
- `learn-projects/spectacle/src/Gui/AnnotationOptionsToolBarContents.qml:28`

## Recommended Rollshot UI Direction

### Workspace, Not a Separate App

Evolve the current Result Workspace into a lightweight image viewer/editor.
Annotation should be an editing mode in the same window and on the same canvas.

This keeps:

- The user's current zoom and scroll position.
- Existing Copy, Save As, Reveal, status, and close behavior.
- A single mental model for ordinary and stitched screenshots.

It also leaves room for a future Open Image entry point without requiring that
feature in the first release.

### Keep the Row Toolbar

Use the current fixed top row as the primary control surface. As actions become
icon buttons, group them with separators:

`Close | title | Select, Number, Text, Redact | Undo, Redo | Navigator | Copy | Save As | Reveal`

Recommendations:

- Use icon buttons with tooltips and shortcuts.
- Keep selected tools visibly active.
- Disable Undo/Redo when unavailable.
- Do not place toolbars relative to image content or selected annotations.
- Keep zoom and fit controls in the existing bottom row.

The exact Copy design remains unresolved. Two explicit icon actions, **Copy
Annotated** and **Copy Original**, are trustworthy but consume toolbar space. A
primary Copy Annotated action with a small adjacent menu for Copy Original is
more compact but makes the original less discoverable.

### Contextual Editing

Do not expose a permanent properties panel in the first release.

- Number Callout needs direct tip and bubble handles.
- Text Note should open an inline text editor.
- Opaque Redaction needs direct rectangle resize handles.
- Selected annotations need Delete.

If color or size customization is included later, add a compact contextual row
or popover driven by the active tool or selected object. Do not add a generic
inspector before properties exist that justify it.

### Callout Navigator

The Callout Navigator is the part that makes this editor specifically good for
long images. It is not a substitute for ordinary canvas selection.

Recommended behavior:

- A toolbar button toggles a right-side navigator drawer.
- The drawer lists annotations in image-space top-to-bottom order.
- Clicking an item scrolls the existing viewport to the annotation and selects
  it.
- Selecting an annotation on the canvas highlights its navigator item.
- The drawer is optional and closed by default for ordinary screenshots.

A visual mini-map may become useful later, but it solves a different problem:
spatial navigation. The first-release Navigator should prioritize semantic
navigation between annotations.

## Architecture Implications from UI/UX

The research supports a refactor, but not a separate generic editor
application.

Recommended boundary:

- Keep the iced Result Workspace and platform actions in `rollshot-app`.
- Extract framework-neutral annotation document, geometry, hit-testing,
  history, ordering, and flattening logic into a focused crate.
- Do not name or design that crate around long screenshots.
- Do not put file dialogs, clipboard APIs, toolbar widgets, or arbitrary-image
  opening into the annotation core.

An independent viewer/editor **UI crate** is premature. The current viewer is
tightly integrated with Rollshot's post-capture lifecycle, save/reveal
behavior, and iced application shell. Extracting the UI now would create an API
boundary before the interaction model is proven.

A focused annotation core crate is justified because:

- The current `result_workspace/mod.rs` is already large.
- Annotation geometry and history are easier to test without iced.
- Copy Annotated and live preview must share rendering behavior.
- Image-space data must remain independent of viewport zoom and pan.

## Open Product Decisions

These decisions should be resolved during the design phase:

1. Should the first release expose an **Open Image** action, or merely avoid
   architecture that prevents it later?
2. Does **Save As** save the annotated result when annotations exist, or must it
   offer explicit Original/Annotated choices?
3. Should **Copy Original** be a visible peer action or live in a Copy menu?
4. Does closing discard editable annotations after confirmation, or should the
   first release introduce an editable sidecar/project format?
5. Should Navigator list all annotations or only Number Callouts and Text Notes?

## Final Recommendation

Build Long-Shot Callouts as the first editing workflow of Rollshot's existing
image viewer/editor.

Preserve the existing fixed row-toolbar structure, progressively replace text
actions with grouped icon buttons, keep global output actions stable, and add
Callout Navigator as an optional right-side drawer. Use an immutable source
image and a framework-neutral annotation graph, while leaving arbitrary-image
opening as a separate product decision rather than an architectural
restriction.
