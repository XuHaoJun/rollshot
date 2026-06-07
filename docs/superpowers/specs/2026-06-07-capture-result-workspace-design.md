# Capture Result Workspace Design

## Status

Approved design. This spec is live for the next implementation plan.

## Summary

Replace Rollshot's immediate save-dialog handoff with a Snow Shot-inspired
capture workspace that remains open after region selection and lets the user
choose how to complete the capture.

The first version provides no annotation or image-editing tools. It establishes
the interaction and layout foundation for a future editor:

- A draggable toolbar attached to the selected region.
- Normal screenshot, scrolling screenshot, save, copy, and cancel actions.
- A scrolling-capture live preview placed independently from the toolbar.
- A post-capture **Result Review** state that displays the final image before
  save or copy.
- A shared chrome-placement policy for Linux and macOS.

The interaction model follows Snow Shot, but Rollshot will implement it in iced
without copying Snow Shot's GPL-3.0 source code or assets.

## Problem

Rollshot currently has two related UX problems:

1. Normal screenshot mode finishes immediately when the user releases the
   mouse, then opens a save dialog. The user cannot inspect the result or copy
   it to the clipboard.
2. During scrolling capture, the toolbar, warning, and live preview are always
   stacked into one chrome column. Their location is selected as one unit,
   causing toolbar placement to be dictated by preview placement and wasting
   usable space around the crop.

The toolbar and live preview need consistent but independent placement. The
workflow also needs an explicit result state rather than treating the save
dialog as the only successful completion path.

## Reference Findings

### Snow Shot

Snow Shot treats scrolling capture as a tool inside one persistent full-screen
capture workspace:

- Completing a region selection keeps the workspace open.
- The main toolbar defaults below the selection, right-aligned to its edge.
- If the toolbar would exceed the bottom boundary, it moves above the
  selection.
- The toolbar is draggable and clamped to the viewport.
- The scrolling live preview is a separate thumbnail rail fixed to the
  selection's right side.
- Enabling scrolling mode disables incompatible annotation tools while keeping
  output actions available.
- Its edge auto-hide behavior partially hides a toolbar that the user drags to
  a viewport edge.

Snow Shot does **not** coordinate toolbar and live-preview placement. Its
toolbar only uses bottom-to-top fallback, and its live preview remains on the
right. It therefore does not solve Rollshot's requirement to place two chrome
components without unnecessary overlap.

### Spectacle

Spectacle allows floating toolbars to be dragged within the viewport. For
full-screen or otherwise space-constrained selections, it accepts toolbar
overlap with screenshot content rather than making controls unreachable.

This is the model for Rollshot's Result Review state: the toolbar may cover the
final image because capture has stopped, and the user can drag it away.

### Rollshot

Rollshot's stitcher automatically detects vertical or horizontal motion from
the first reliable match and locks the axis. The UI must not expose a manual
direction switch.

The current capture-phase layout:

- Selects one outside-crop band.
- Places toolbar, warning, and preview together in one vertical stack.
- Hides all capture chrome when no outside-crop band is large enough.

The target layout must replace this single-stack behavior with coordinated,
independent chrome placement.

## Goals

- Keep the full-screen capture workspace open after a normal screenshot region
  is selected.
- Let the user switch between normal and scrolling capture within the same
  selected region.
- Provide `Save`, `Copy`, and `Cancel` actions without requiring annotations.
- Display the final normal or scrolling screenshot in Result Review before
  output.
- Place the toolbar and scrolling live preview independently whenever space
  permits.
- Use the same toolbar placement priority in all capture-workspace states:
  bottom, top, left, then right.
- Keep active-capture chrome outside the crop whenever possible so it is not
  self-captured.
- Preserve a reliable way to finish or cancel when the crop leaves no usable
  outside space.
- Apply the same behavior to Linux and macOS iced overlay paths.

## Non-Goals

- Annotation, cropping, resizing, blur, text, arrows, or other image editing.
- OCR, pin-to-screen, cloud upload, history, or configurable toolbar contents.
- A manual vertical/horizontal scrolling-direction control.
- Persisting a user-dragged toolbar position between capture sessions.
- Reproducing Snow Shot's visual assets or implementation.
- Redesigning capture backends or stitching algorithms.
- Adding a system tray or global hotkeys.

## Terminology And States

The capture workspace has four user-visible states:

### Selecting

The user is choosing or adjusting a region. The frozen screenshot is visible in
normal mode; the live target is visible in scrolling mode.

### Selected

A valid region exists and the workspace remains open. The toolbar is visible
and contains:

- Drag handle
- Normal screenshot mode
- Scrolling screenshot mode
- Save
- Copy
- Cancel

Selecting normal screenshot mode keeps or refreshes the frozen crop result.
Selecting scrolling screenshot mode starts the scrolling workflow for the same
region.

### Scrolling Capture

The scrolling driver and stitcher are active. The selected region passes input
through to the target where required. The toolbar remains available, and the
live preview displays the growing stitched result.

The toolbar adds a `Finish` action while scrolling is active. `Finish`
finalizes stitching and enters Result Review without writing output.

`Save` or `Copy` finalizes stitching before performing the requested output,
without requiring an intermediate Result Review step.
Selecting normal screenshot mode stops and discards the active scrolling
result, then returns to the normal Selected state for the same region.

### Result Review

Capture and stitching have stopped. The final image replaces the live target
inside the selected-region viewport.

The toolbar contains:

- Drag handle
- Save
- Copy
- Close

The toolbar may overlap the final image and remains draggable. The final image
is scrollable within the selected-region viewport when it is larger than the
available area. No editing is provided.

## Toolbar Behavior

### Attachment And Dragging

Before the user drags it, the toolbar is automatically attached near the
selected region. Automatic placement is recalculated when:

- The selection changes size or position.
- The active workspace state changes.
- The toolbar's visible actions change its dimensions.
- The overlay viewport changes.

After the user drags the toolbar, its clamped user position takes priority.
Changing the selection clears the manual position and restores automatic
placement.

The toolbar must remain fully inside the overlay viewport.

### Actions

The first version uses icon buttons with tooltips and clear active-state
styling. It does not display disabled placeholders for future editing tools.

`Finish` is visible only during Scrolling Capture. It finalizes the current
stitched result and enters Result Review.

`Save` opens the native Save As dialog. A successful save ends the session and
closes the workspace. Cancelling the dialog returns to Result Review.

`Copy` writes the full-resolution image to the system clipboard. A successful
copy ends the session and closes the workspace.

`Cancel` exits without producing a result. In Result Review the equivalent
action is labelled `Close`.

`Esc` remains a best-effort focused-overlay shortcut for cancellation. It is
not the only exit path.

## Coordinated Chrome Placement

### Components

The placement engine treats the following as separate components:

- `Toolbar`
- `LivePreview`

Transient warning and status messages are floating overlays and do not reserve
a placement band.

### Available Bands

For a selected crop and overlay viewport, calculate the four rectangles outside
the crop:

- Bottom
- Top
- Left
- Right

A component fits a band only when its complete bounds, required spacing, and
viewport margins fit inside that band.

### Toolbar Priority

The toolbar always evaluates bands in this fixed order:

1. Bottom
2. Top
3. Left
4. Right

It uses the first band that fits. This ordering applies consistently in
Selected and Scrolling Capture states.

For bottom and top placement, the toolbar aligns to the crop's right edge when
possible and is clamped to the viewport. For left and right placement, it
aligns to the crop's bottom edge when possible and is clamped to the viewport.

### Live Preview Placement

After reserving the toolbar's rectangle, place the live preview in the
remaining outside-crop space:

1. Prefer a different band from the toolbar.
2. Among fitting different bands, select the band with the largest usable area.
3. If no different band fits, place it in the toolbar's band only when both
   components fit without overlap.
4. If only one band can host chrome, use a combined layout in that band.

Combined layout orientation:

- Bottom or top band: toolbar and preview are arranged horizontally when the
  width permits; otherwise the toolbar sits above the preview.
- Left or right band: toolbar sits above the preview.

The toolbar is never rendered as a duplicate inside the live-preview
component. Combined placement is one coordinated layout containing one toolbar
and one preview.

The preview keeps its existing bounded, low-resolution behavior. Its dimensions
shrink to the assigned rectangle without changing the full-resolution stitched
result.

### No-Space Active Capture

When toolbar and live preview cannot fit outside the crop, active capture enters
**activity auto-hide**:

- While accepted stitch frames continue arriving, toolbar and live preview are
  hidden so they are not self-captured.
- After `500ms` without an accepted stitch frame, toolbar and live preview
  appear over the crop.
- A capture-miss event does not count as accepted activity and therefore does
  not indefinitely hide the controls.
- Hovering, clicking, or dragging the toolbar keeps the controls visible.
- On the next accepted stitch frame, the controls hide again unless the user is
  interacting with the toolbar.
- The over-crop fallback uses a viewport-clamped draggable toolbar and a
  non-interactive live preview.
- `Esc` remains available when the overlay receives keyboard input.

Activity auto-hide is included in this scope because otherwise full-screen and
near-full-screen scrolling selections can leave no reliable finish or cancel
control.

### Transient Messages

Capture-miss warnings and short status text appear as temporary floating
messages over the dimmed area or crop. They must not force toolbar/live-preview
relayout.

## Result Review Layout

Result Review begins after the user explicitly finishes scrolling capture or
after a normal screenshot result is ready for output.

- Stop capture before displaying Result Review.
- Display the final image inside the crop viewport.
- Fit a normal screenshot result inside the viewport initially.
- Fit a scrolling result to the cross-axis initially:
  - Vertical result: fit width and allow vertical scrolling.
  - Horizontal result: fit height and allow horizontal scrolling.
- Keep the full-resolution image as the source for Save and Copy.
- Place the toolbar using the same automatic bottom, top, left, right priority.
- If no outside band fits, place the toolbar over the image.
- Allow the user to drag the toolbar anywhere inside the overlay viewport.
- Do not auto-hide Result Review chrome because capture has stopped.

The live preview does not appear in Result Review; it is replaced by the final
image viewport.

## State And Data Flow

The overlay session must separate workflow state from shared workspace state.
A representative shape is:

```rust
enum WorkspacePhase {
    Selecting,
    Selected,
    ScrollingCapture,
    ResultReview,
}

enum CaptureWorkflow {
    Screenshot(ScreenshotWorkflow),
    Scrolling(ScrollingWorkflow),
}

struct WorkspaceState {
    phase: WorkspacePhase,
    workflow: CaptureWorkflow,
    crop: Option<Rectangle>,
    toolbar_position: ToolbarPosition,
    chrome_placement: ChromePlacement,
    final_image: Option<RgbaImage>,
}
```

`ChromePlacement` is calculated from component requirements and geometry. It
must be testable without rendering iced widgets:

```rust
struct ChromeRequirements {
    toolbar_size: Size,
    preview_size: Option<Size>,
    margin: f32,
    spacing: f32,
}

enum ChromePlacement {
    Separate {
        toolbar: Rectangle,
        preview: Option<Rectangle>,
    },
    Combined {
        band: Band,
        toolbar: Rectangle,
        preview: Rectangle,
    },
    ActivityAutoHide {
        overlay_toolbar: Rectangle,
        overlay_preview: Option<Rectangle>,
    },
}
```

The exact type names may follow existing crate style, but the separation of
requirements, placement result, and rendering is required.

The scrolling driver emits accepted-frame activity separately from preview
image updates so the UI can drive activity auto-hide without relying on wheel
events or platform-specific input observation.

## Platform Behavior

### Linux

The layer-shell input region must include the visible toolbar rectangle. During
outside-crop placement, the crop remains pointer-pass-through. During
activity-auto-hide over-crop display, the implementation must expose a
clickable toolbar area without making the entire crop consume scroll input.

### macOS

The macOS runner must keep the toolbar clickable while the target receives
scroll input. Activity-auto-hide must update the native input region or
passthrough behavior so only the visible toolbar receives clicks.

### Shared Behavior

Placement policy, activity timing, toolbar actions, workspace phases, and
Result Review behavior live in shared iced overlay code. Platform runners only
apply window/input-region effects.

## Error Handling

- Save-dialog cancellation returns to Result Review with the final image
  intact.
- Save failure remains in Result Review and displays a transient error.
- Clipboard failure remains in Result Review and displays a transient error.
- Switching away from scrolling mode stops the driver before discarding its
  result.
- Finishing scrolling capture stops the driver before entering Result Review.
- Finishing scrolling capture with no usable stitched result remains in
  Scrolling Capture and displays an error.
- Empty selections remain in Selecting.
- Placement failure must resolve to activity auto-hide or Result Review
  over-image placement; it must never silently remove all exit controls.

## Expected Code Boundaries

### `rollshot-overlay-core`

- Add pure coordinated chrome-placement geometry and tests.
- Keep framework-neutral placement inputs and outputs.

### `rollshot-iced-overlay`

- Introduce explicit workspace phases.
- Render the selection-attached draggable toolbar.
- Render live preview independently from toolbar placement.
- Add activity-auto-hide state driven by accepted stitch activity.
- Add Result Review with a scrollable final-image viewport.
- Keep Linux and macOS user-facing behavior aligned.

### `rollshot-app`

- Replace immediate save-dialog handoff with the workspace-owned Save action.
- Add full-resolution clipboard image output.
- Keep native Save As behavior for the toolbar's Save action.

### `rollshot-capture` And `rollshot-core`

- No stitching or capture algorithm changes are expected.
- Expose accepted stitch activity through existing driver/UI event boundaries
  without changing matching behavior.

## Testing

### Placement Unit Tests

- Toolbar selects bottom when all four bands fit.
- Toolbar selects top when bottom does not fit.
- Toolbar selects left when bottom and top do not fit.
- Toolbar selects right when only right fits.
- Preview selects a different fitting band from the toolbar.
- Preview selects the largest usable different band.
- Toolbar and preview share one band without overlap when no different band
  fits.
- No-space placement returns activity auto-hide.
- Manual toolbar positions are clamped to the viewport.
- Selection changes clear the manual toolbar position.

### Workspace State Tests

- Normal screenshot selection enters Selected instead of immediately finishing.
- Selecting scrolling mode starts the scrolling workflow for the existing crop.
- Accepted stitch activity hides no-space chrome.
- `500ms` without accepted stitch activity reveals no-space chrome.
- Toolbar interaction prevents activity auto-hide while interacting.
- Finishing scrolling capture enters Result Review.
- Save-dialog cancellation remains in Result Review.
- Successful Save closes the session.
- Successful Copy writes the full-resolution image and closes the session.
- Clipboard or save failure remains in Result Review.
- Switching from scrolling to normal stops and discards scrolling state.
- Result Review never displays the live preview.

### Manual Runtime Verification

- Linux and macOS: normal screenshot selection keeps the workspace open.
- Linux and macOS: toolbar follows bottom, top, left, right fallback order.
- Linux and macOS: toolbar can be dragged and remains inside the viewport.
- Linux and macOS: scrolling preview avoids the toolbar when another band fits.
- Linux and macOS: one-band combined layout shows one toolbar and one preview.
- Linux and macOS: full-screen scrolling selection reveals controls after
  scrolling stops and hides them when accepted stitching resumes.
- Linux and macOS: Result Review displays normal and long screenshots and
  supports Save, Copy, and Close.

## Deferred Follow-Ups

These are explicitly outside this implementation but must remain compatible
with the design:

- Annotation and basic image editing tools in Selected and Result Review.
- Configurable toolbar contents and ordering.
- Persisted toolbar position preferences.
- Edge-peek auto-hide for a toolbar intentionally dragged to a viewport edge,
  similar to Snow Shot.
- Zoom controls and minimap navigation for very large Result Review images.
- Capture history, pin-to-screen, OCR, and other output actions.
