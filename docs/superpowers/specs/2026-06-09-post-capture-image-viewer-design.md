# Post-Capture Image Viewer Design

**Date:** 2026-06-09  
**Status:** Approved design, revised by 2026-06-09 product review, pending implementation plan  
**Scope:** Active iced product path in `rollshot-app` and `rollshot-iced-overlay`

## Revision — 2026-06-09 product review

Updated after a product/UX review (`plan-ceo-review`). Changes from the
original approved design:

- Linux now auto-saves every successful capture (previously a Non-Goal). Both
  platforms auto-save; macOS additionally shows a floating thumbnail, while
  Linux opens a saved Result Workspace directly.
- Auto-save default location is the desktop directory on both platforms
  (Linux: `XDG_DESKTOP_DIR`, fallback `~/Pictures`; the directory is not
  created if missing).
- `Fit Height` gains a dedicated status-bar button (previously an unexposed
  mode).
- Window-manager / OS-level window close is intercepted and routed through the
  unsaved-capture discard confirmation, so an unsaved capture cannot be lost by
  closing the window directly.
- A saved Result Workspace shows its saved path in the inline message area on
  open.

The macOS floating thumbnail + AppKit native file drag remain in scope
(required). Implementation sequencing is left to the engineering plan.

## 1. Summary

Rollshot will move result review out of the capture overlay and into an
independent Result Workspace owned by `rollshot-app`.

The Result Workspace will be a complete image viewer for inspecting long
screenshots, not a lightweight editor. It will provide a fixed action toolbar,
a centered image canvas, explicit zoom modes, and prominent scrollbars.
Annotation, editable document formats, layers, and undo/redo are out of scope.

Post-capture presentation differs by platform:

- macOS auto-saves to Desktop and shows a floating thumbnail. The thumbnail can
  be dragged as a native file into another application or clicked to open the
  Result Workspace.
- Linux auto-saves to the desktop directory and opens the Result Workspace
  immediately with the saved result (no floating thumbnail).

Each capture remains an independent `rollshot-app` process. There is no
persistent app, shared thumbnail host, cross-capture queue, or shared Result
Workspace.

This design supersedes the product direction captured in
`2026-06-07-capture-result-workspace-design.md`; that earlier file remains an
unchanged historical snapshot.

## 2. Goals

- Separate capture UI from post-capture result handling.
- Make long screenshots practical to inspect with zoom and scrolling.
- Auto-save every successful capture on both platforms so a durable file always
  exists without requiring a save dialog.
- Keep the default macOS capture flow fast and non-interrupting for the user's
  current desktop work.
- Make native file drag from the macOS floating thumbnail a required feature.
- Preserve the captured image if auto-save fails.
- Leave a clean path for future annotation without implementing editor
  functionality now.

## 3. Non-Goals

- Annotation tools, layers, selections, or editable document formats.
- Undo/redo or dirty-state tracking for edits.
- A persistent Rollshot process, system tray, or macOS menu bar app.
- Cross-capture thumbnail coordination, stacking, queue limits, or a shared
  Result Workspace.
- Floating thumbnails on Linux.
- Refactoring the deprecated Tauri result path.

## 4. Product Model

### 4.1 Capture Overlay

`rollshot-iced-overlay` owns capture only:

- Selecting a region.
- Choosing screenshot or scrolling mode.
- Running capture and stitching.
- Finishing or cancelling capture.
- Returning a `CaptureResult`.

It does not own result review, Save, Copy, or post-capture output UI.

The overlay workspace phases become:

- `Selecting`
- `Selected`
- `ScrollingCapture`

`WorkspacePhase::ResultReview` and its overlay-specific state and actions are
removed.

### 4.2 Result Workspace

`rollshot-app` owns the independent Result Workspace:

- File actions.
- Result status and errors.
- Image viewport, zoom, and scrolling.
- Saved versus unsaved result semantics.

The Result Workspace is an ordinary application window, visually distinct from
the frameless capture overlay.

### 4.3 Process Lifetime

Each interactive capture launches its own `rollshot-app` process.

```text
rollshot CLI
    |
    v
rollshot-app process
    |
    +-- capture overlay
    +-- platform post-capture flow
    +-- optional floating thumbnail
    +-- optional Result Workspace
    |
    +-- exits after all post-capture UI closes
```

The CLI continues using its current blocking launch behavior. It may remain
waiting while the floating thumbnail or Result Workspace is open.

## 5. Platform Flows

### 5.1 macOS Success

1. The capture overlay returns `CaptureResult` and closes.
2. `rollshot-app` generates a unique Desktop path.
3. The PNG is written to Desktop.
4. A floating thumbnail is displayed for eight seconds.
5. The user may:
   - Drag the saved PNG into another application.
   - Click the thumbnail to open the Result Workspace.
   - Ignore the thumbnail and find the file on Desktop later.
6. If the thumbnail closes and no Result Workspace is open, the process exits.

### 5.2 macOS Auto-Save Failure

1. The captured image remains in memory.
2. No floating thumbnail is displayed.
3. An unsaved Result Workspace opens immediately.
4. The auto-save error is shown in the inline message area.
5. The user may Copy, Save As, or discard the result.

### 5.3 Linux Success

1. The capture overlay returns `CaptureResult` and closes.
2. `rollshot-app` generates a unique path in the desktop directory.
3. The PNG is written to that path.
4. A saved Result Workspace opens immediately (no floating thumbnail).
5. The user may Copy, Save As to another location, or Reveal the result.

If Linux auto-save fails, the flow follows the same path as macOS (§5.2): the
image stays in memory and an unsaved Result Workspace opens with the auto-save
error shown inline.

### 5.4 Cancellation

Cancelling capture closes the overlay and exits the process without opening
post-capture UI.

## 6. Desktop Auto-Save

Both platforms auto-save every successful capture and use the desktop directory
as the default location (both for auto-save and as the Save As dialog default):

- macOS: `~/Desktop`.
- Linux: `XDG_DESKTOP_DIR`, falling back to `~/Pictures` when it does not
  resolve. The directory is not created if it does not already exist; the
  fallback is used instead.

The auto-save filename format is identical on both platforms:

```text
Rollshot YYYY-MM-DD at HH.MM.SS.png
```

If that path exists, append `-2`, `-3`, and so on before `.png`.

The macOS floating thumbnail is shown only after the PNG has been written
successfully. It represents a durable saved file, not a temporary file.

## 7. macOS Floating Thumbnail

### 7.1 Appearance

The floating thumbnail is a compact image card:

- Screenshot preview.
- `Saved` status.
- `Drag or click` interaction hint.
- Positioned near the lower-right corner of the active display.
- Visually lightweight and distinct from the Result Workspace.

### 7.2 Lifetime

- Automatically closes after eight seconds.
- Hover pauses the countdown.
- Native dragging pauses the countdown.
- A cancelled or failed drag restarts the countdown.
- A successful drag closes the thumbnail.

There is no cross-capture coordination. Multiple capture processes may show
independent thumbnails, including overlapping thumbnails.

### 7.3 Click and Drag

- Mouse down and release without crossing the system drag threshold opens the
  Result Workspace.
- Opening the Result Workspace closes the floating thumbnail.
- Crossing the system drag threshold starts an AppKit native file drag for the
  saved PNG.
- Native drag is required for the floating-thumbnail MVP.
- Dragging the window itself is not an acceptable substitute.

## 8. Result Workspace Layout

```text
+------------------------------------------------------+
| Close   filename           Copy  Save As  Reveal     |
+------------------------------------------------------+
| inline saved / copied / error message                |
+------------------------------------------------------+
|                                                      |
|               centered image canvas                  |
|                                      thick scrollbar |
|                                                      |
+------------------------------------------------------+
| 1440x18240 Fit Width Fit Window Fit Height 100% - +  |
+------------------------------------------------------+
```

### 8.1 Top Action Toolbar

- `Close`
- Filename or `Unsaved capture`
- `Copy`
- `Save As`
- `Reveal`

`Reveal` is disabled when no saved path exists.

There is no generic `Save` action. The MVP has no editable state to overwrite.

### 8.2 Inline Message Area

The area below the toolbar displays:

- Initial auto-save success, showing the saved path, when a saved Result
  Workspace opens.
- Auto-save failure.
- Save As success or failure.
- Copy success or failure.
- Reveal failure.

Messages do not cover the image canvas. Success messages expire; errors remain
until dismissed or replaced.

### 8.3 Bottom Status Bar

The status bar displays:

- Original image dimensions.
- Active fit mode or custom zoom percentage.
- `Fit Width`
- `Fit Window`
- `Fit Height`
- `100%`
- Zoom Out
- Zoom In

## 9. Viewport and Zoom

### 9.1 Default Mode

- Normal-aspect image: `Fit Window`, centered.
- Vertical long image: `Fit Width`, initially scrolled to the top.
- Horizontal long image: `Fit Height`, initially scrolled to the left.
- An image is long when its long edge exceeds its short edge by more than `2x`.

`Fit Height` is a supported zoom mode with a dedicated status-bar button. It is
the horizontal-long-image default and can be reselected at any time.

### 9.2 Zoom Modes

Supported modes:

- `Fit Window`
- `Fit Width`
- `Fit Height`
- `100%`
- Custom percentage

Fixed custom zoom steps:

```text
25%, 33%, 50%, 67%, 100%, 125%, 150%, 200%, 300%, 400%
```

Zoom is clamped to `25%` through `400%`.

### 9.3 Input

- Mouse wheel scrolls vertically.
- Shift + mouse wheel scrolls horizontally.
- Cmd + mouse wheel on macOS zooms.
- Ctrl + mouse wheel on Linux zooms.
- Pointer-driven zoom keeps the image point under the pointer stationary when
  possible.

### 9.4 Resize

- Fit modes recompute their scale when the window or viewport changes.
- Custom percentage remains unchanged on resize.
- Images smaller than the viewport are centered on both axes.

### 9.5 Scrollbars

- Vertical and horizontal scrollbars are visibly thicker than platform-default
  subtle overlay scrollbars.
- A scrollbar is shown only when content exceeds the viewport on that axis.
- Scrollbars remain visible while overflow exists.

## 10. Result Document Model

The MVP uses a small concrete document model:

```rust
struct ResultDocument {
    source_image: image::RgbaImage,
    saved_path: Option<std::path::PathBuf>,
}

struct ViewportState {
    zoom: ZoomMode,
    scroll_offset: iced::Vector,
}
```

The document does not contain annotations, layers, dirty state, or undo
history.

Future editing may extend the document with annotation objects and an export
render pipeline. The viewer canvas and viewport state should remain reusable,
but the MVP must not introduce speculative editor abstractions.

## 11. Result Workspace Actions

### 11.1 Copy

Copy writes the original captured result image to the clipboard.

Success displays a transient inline message. Failure leaves the workspace open
and displays an error.

### 11.2 Save As

Save As opens a native file dialog. On success:

- The PNG is written.
- `saved_path` is updated to the selected path.
- `Reveal` becomes enabled.
- A transient success message is displayed.

Cancelling Save As leaves the workspace open without an error.

### 11.3 Reveal

Reveal opens the containing folder in the platform file manager and selects the
saved file where supported. Failure leaves the workspace open and displays an
error.

### 11.4 Close

- A saved Result Workspace closes immediately.
- An unsaved Result Workspace asks for confirmation:
  `Discard unsaved capture?`
- Rejecting discard returns to the workspace.
- Window-manager / OS-level window close (macOS red close button or Cmd-W,
  Linux title-bar close) is intercepted and routed through the same close
  decision as the in-app `Close` action, so an unsaved capture cannot be lost
  by closing the window directly.

## 12. Refactoring Boundaries

### 12.1 Remove From `rollshot-iced-overlay`

- `WorkspacePhase::ResultReview`
- Overlay result image handle and size
- Overlay result-review renderer
- ResultReview toolbar action set
- Overlay Save and Copy actions
- Overlay output phase transitions
- `PostOverlayRequest`, once no remaining caller requires it

The capture overlay returns completed results instead of transitioning into a
result-review phase.

### 12.2 Add To `rollshot-app`

- Platform post-capture policy.
- Desktop-directory path generation and auto-save on both platforms.
- Independent Result Workspace application/window.
- Result document and viewport state.
- File actions and inline messages.
- macOS floating thumbnail.
- macOS AppKit native file drag adapter.

### 12.3 Deprecated Tauri Path

No behavior or UI changes are required under `crates/rollshot-tauri-app`.

## 13. Error Handling

- Capture errors retain existing overlay error behavior until no result can be
  produced.
- Auto-save errors on either platform open an unsaved Result Workspace.
- Result Workspace action errors remain inline and non-destructive.
- Native drag cancellation or failure keeps the thumbnail available.
- Failure to create the floating thumbnail after successful auto-save prints an
  error and exits; the durable Desktop file remains available.

## 14. Testing and Verification

### 14.1 Unit Tests

- Unique auto-save filename generation on both platforms.
- Linux desktop-directory resolution with `XDG_DESKTOP_DIR` and `~/Pictures`
  fallback.
- Saved and unsaved close decisions, including window-manager / OS-level close
  routing.
- Default zoom mode for normal, vertical-long, and horizontal-long images.
- Fit mode scale calculations.
- Fixed zoom-step transitions and clamping.
- Pointer-anchor preservation during zoom.
- Scrollbar overflow decisions.
- Platform post-capture policy.

### 14.2 Integration Tests

- Capture completion returns a result without entering overlay ResultReview.
- Capture cancellation does not open post-capture UI.
- macOS successful auto-save selects floating-thumbnail presentation.
- macOS auto-save failure selects unsaved Result Workspace presentation.
- Linux completion auto-saves and selects saved Result Workspace presentation.
- Linux auto-save failure selects unsaved Result Workspace presentation.
- Save As updates the document saved path.

### 14.3 Runtime Verification

macOS:

- Auto-save to Desktop.
- Unique names for rapid repeated captures.
- Thumbnail position and eight-second timeout.
- Hover and drag pause.
- Native drag into Finder and Notes.
- Click opens Result Workspace.
- Auto-save failure opens unsaved Result Workspace.

Linux:

- Capture completion auto-saves to the desktop directory and opens a saved
  Result Workspace.
- Copy and Save As.
- Auto-save failure opens an unsaved Result Workspace.
- Unsaved close confirmation, including window-manager / OS-level close.
- Vertical and horizontal long-image navigation.

Both:

- Normal, vertical-long, and horizontal-long images.
- Fit modes, `100%`, zoom steps, pointer-anchored zoom.
- Thick persistent scrollbars when overflowing.
- Window resize behavior.

### 14.4 Repository Checks

```bash
rtk cargo test
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

This work does not modify core stitching paths, so stitching benchmarks are not
required.
