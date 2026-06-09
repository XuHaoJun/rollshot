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

## Revision — 2026-06-09 engineering review

Updated after an engineering review (`plan-eng-review`) of this design and its
companion plan (`docs/superpowers/plans/2026-06-09-post-capture-image-viewer.md`).
These changes harden correctness and surface load-bearing assumptions; they do
not alter the product model:

- **Single event loop per process** is now stated explicitly as the reason the
  two platforms diverge (§4.4). The Linux flow depends on starting an ordinary
  (winit-backed) Result Workspace *after* the layer-shell overlay exits in the
  same process; this must be smoke-validated before the full Linux flow is built.
- **Large-image rendering ceiling** (§9.6): the viewer uploads the result as a
  single GPU texture (as today's overlay result-review already does). Long
  screenshots can exceed the device max texture dimension, so a downscaled
  *display* texture is required above a safe threshold while the full-resolution
  image is retained for Copy/Save As.
- **Atomic unique filenames** (§6): auto-save must create files with
  exclusive-create semantics so concurrent capture processes cannot clobber each
  other, replacing the check-then-write scheme.
- **Clipboard persistence** (§11.1): Copy's behavior on a one-shot process that
  exits is now specified (depends on a running clipboard manager).
- **Reveal mechanism** (§11.3) and **macOS Desktop TCC** auto-save failures
  (§5.2, §6) are specified.
- Additional negative/boundary tests and runtime checks added to §14.

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

### 4.4 Single Event Loop Per Process

The platform split below is not stylistic — it is forced by a runtime
constraint. The pinned winit backend rejects creating a second event loop in one
process. The two platforms satisfy this differently:

- **Linux** runs the capture overlay on the `iced_layershell` (sctk) backend,
  which does not touch winit. After the overlay event loop exits, the process
  starts one ordinary winit-backed iced Result Workspace — the first and only
  winit loop in the process.
- **macOS** cannot run two iced applications in sequence, so it uses a single
  long-lived iced daemon that owns capture, the floating thumbnail, and the
  Result Workspace as windows/phases within one event loop.

```text
Linux:   [layer-shell overlay loop] -> exit -> [winit Result Workspace loop]
         (two loops, two backends, sequential, one process)

macOS:   [single iced daemon: capture -> thumbnail -> workspace windows]
         (one loop, one backend, phase transitions)
```

The Linux "second event loop after the first exits" path is the load-bearing
assumption for the entire Linux flow. It must be smoke-validated (launch an
ordinary iced window in the same process after the layer-shell overlay exits,
confirming no `EventLoop can't be recreated` error) **before** the dependent
Result Workspace tasks are built. If it does not hold, Linux must adopt the same
single-daemon model as macOS, which changes the orchestration in §5.3 and §12.2.

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

- macOS: `~/Desktop`. On macOS 10.15+ the Desktop folder is TCC-protected, so
  the first write may surface a system permission prompt and a denied write
  fails. A denied or otherwise failed Desktop write is treated as an auto-save
  failure and follows §5.2 (unsaved Result Workspace) — it is never a silent
  loss of the capture.
- Linux: `XDG_DESKTOP_DIR`, falling back to `~/Pictures` when it does not
  resolve. The directory is not created if it does not already exist; the
  fallback is used instead. If `~/Pictures` is also missing, auto-save fails and
  follows §5.3 (unsaved Result Workspace).

Resolution reuses `dirs::desktop_dir()` / `dirs::picture_dir()` where possible
rather than reimplementing freedesktop `user-dirs.dirs` parsing; only the
"directory exists" check and the Desktop→Pictures fallback decision are
project-specific, and that decision is kept in a pure, unit-testable function.

The auto-save filename format is identical on both platforms:

```text
Rollshot YYYY-MM-DD at HH.MM.SS.png
```

If that path exists, append `-2`, `-3`, and so on before `.png`.

Because each capture is an independent process (§4.3), two captures finishing in
the same second would resolve the same suffix and one could overwrite the other.
The unique-path selection and write are therefore atomic: the file is created
with exclusive-create semantics (`OpenOptions::create_new(true)` / `O_EXCL`), and
on `AlreadyExists` the suffix is incremented and the create retried. A pure
suffix-selection helper remains unit-testable; the exclusive create closes the
cross-process race.

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
  The comparison is strict: a ratio of exactly `2.0` is normal-aspect
  (`Fit Window`), and a ratio greater than `2.0` is long. This boundary is
  exercised by a unit test (§14.1).

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

### 9.6 Large Image Rendering Ceiling

The canvas uploads the result as a single GPU texture (the existing overlay
result-review path already does this via `image::Handle::from_rgba`). Long
screenshots are exactly the case that can exceed the device maximum 2D texture
dimension (`wgpu` reports `8192` on many GPUs, `16384` on newer ones); a long
vertical capture can easily exceed this on its long edge. Above the ceiling the
single-texture upload fails or renders blank — the precise failure this viewer
exists to handle well.

Behavior:

- The full-resolution image is always retained in `ResultDocument::source_image`
  and is the source for Copy and Save As. Display fidelity never affects the
  saved/copied bytes.
- When either dimension exceeds a conservative safe threshold (the queried
  device max texture dimension, with a fallback constant such as `8192` when it
  cannot be queried), the *display* handle is built from a downscaled copy that
  fits the ceiling. Zoom percentages and fit math continue to report against the
  original dimensions shown in the status bar.
- The downscale-decision (threshold → display scale) is a pure, unit-tested
  function (§14.1). Whether to additionally tile very large images is out of
  scope for the MVP; downscaling is sufficient to guarantee the image renders.

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

Copy writes the original captured result image to the clipboard (full-resolution
RGBA, independent of the display downscale in §9.6).

Success displays a transient inline message. Failure leaves the workspace open
and displays an error.

Clipboard persistence: on Linux (X11 and Wayland) clipboard contents are served
by the owning process. Because each capture is an independent process that exits
when the Result Workspace closes (§4.3), copied image data survives process exit
only if a clipboard manager has taken ownership — the same constraint the
existing overlay Copy path already lives under. This is documented behavior, not
a regression: Copy depends on a running clipboard manager on Linux, and the
"Copy then Save As / Reveal" affordances exist as a durable alternative. A
runtime-verification step (§14.3) confirms paste behavior with a clipboard
manager present.

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

Mechanism:

- macOS: `open -R <path>` selects the file in Finder.
- Linux: file selection is best-effort. Prefer the freedesktop
  `org.freedesktop.FileManager1` D-Bus `ShowItems` method (which selects the
  file); fall back to `xdg-open <parent>` (opens the folder without selecting)
  when that interface is unavailable. "Selects the saved file where supported"
  refers to this fallback ladder — `xdg-open` alone cannot select a file.

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
- Atomic exclusive-create suffix retry: an existing `-N` candidate advances to
  `-(N+1)` rather than overwriting (§6).
- Linux desktop-directory resolution with `XDG_DESKTOP_DIR` and `~/Pictures`
  fallback, including the case where both Desktop and Pictures are missing
  (auto-save failure).
- Saved and unsaved close decisions, including window-manager / OS-level close
  routing.
- Default zoom mode for normal, vertical-long, and horizontal-long images,
  including the exact `2.0` ratio boundary (normal, not long — §9.1).
- Fit mode scale calculations.
- Fixed zoom-step transitions and clamping.
- Pointer-anchor preservation during zoom.
- Scrollbar overflow decisions.
- Large-image display downscale decision: dimensions at/under the threshold use
  scale `1.0`; dimensions over the threshold produce a display scale that fits
  the ceiling while status-bar dimensions stay at the original size (§9.6).
- Platform post-capture policy.

### 14.2 Integration Tests

- Capture completion returns a result without entering overlay ResultReview.
- Capture cancellation does not open post-capture UI.
- macOS successful auto-save selects floating-thumbnail presentation.
- macOS auto-save failure selects unsaved Result Workspace presentation.
- Linux completion auto-saves and selects saved Result Workspace presentation.
- Linux auto-save failure selects unsaved Result Workspace presentation.
- Save As success updates the document saved path and enables Reveal.
- Save As to an unwritable path leaves `saved_path` unchanged and shows a
  persistent inline error (negative path).
- Copy failure and Reveal failure leave the workspace open and show an inline
  error without mutating document state (negative paths).

### 14.3 Runtime Verification

macOS:

- Auto-save to Desktop.
- Unique names for rapid repeated captures.
- Thumbnail position and eight-second timeout.
- Hover and drag pause.
- Native drag into Finder and Notes.
- Click opens Result Workspace.
- Auto-save failure opens unsaved Result Workspace.
- A TCC-denied Desktop write degrades to an unsaved Result Workspace with the
  permission error shown inline (not a silent loss).

Linux:

- Pre-build gate (§4.4): an ordinary winit-backed iced window launches in the
  same process after the layer-shell overlay exits, with no
  `EventLoop can't be recreated` error.
- Capture completion auto-saves to the desktop directory and opens a saved
  Result Workspace.
- Copy and Save As, including paste into another application with a clipboard
  manager present (§11.1).
- Auto-save failure opens an unsaved Result Workspace.
- Unsaved close confirmation, including window-manager / OS-level close.
- Vertical and horizontal long-image navigation.

Both:

- Normal, vertical-long, and horizontal-long images.
- A very long screenshot whose long edge exceeds the device max texture
  dimension (e.g. > 8192 px) renders via the §9.6 display downscale rather than
  blank, while Save As still writes the full-resolution image.
- Two captures finishing within the same second produce two distinct files with
  no overwrite (§6 atomic create).
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
