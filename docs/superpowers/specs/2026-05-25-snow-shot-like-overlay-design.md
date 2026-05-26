# Snow-Shot-Like Overlay UI/UX Design

Date: 2026-05-25

## Goal

Replace the current app-workbench capture experience with a dedicated overlay
capture flow that feels closer to Snow Shot and wayscrollshot:

- Launch capture directly into an overlay.
- Let the user select the capture region over the real desktop/window.
- Show live stitching preview adjacent to the selected region when safe.
- Avoid knowingly stitching rollshot's own overlay UI into the result.

This design targets a cross-platform Tauri transparent overlay first. It does
not introduce Linux-specific layer-shell in the first version.

## Context

Current rollshot shows the capture stream inside a normal Tauri window. The
user selects a region inside that preview and sees stitching output in a right
panel. This works functionally, but it feels like using a desktop app that
contains a capture surface rather than a screenshot overlay.

Snow Shot uses transparent always-on-top Tauri webview windows for its draw and
scroll screenshot UI. It does not use wlr-layer-shell. Its initial selection UI
is not a central "drag here" instruction bubble; it relies on a fullscreen
selection canvas, darkened capture surface, crosshair cursor, auxiliary lines,
and toolbar state. Its scroll preview is positioned next to the selected
region. Snow Shot also hides some UI before capture and, on macOS, passes the
current window as an excluded capture target.

wayscrollshot uses `slurp` for region selection and wlr-layer-shell for the
region border and preview overlay. Because rollshot is choosing a Tauri-first
cross-platform path, wayscrollshot is treated as UX inspiration rather than the
implementation model for this version.

OBS provides the strongest platform reference for capture exclusion:
`WDA_EXCLUDEFROMCAPTURE` on Windows and ScreenCaptureKit application/window
exclusion on macOS. OBS does not show an equivalent self-exclusion mechanism for
Linux Wayland PipeWire portal capture.

## Architecture

Implement a dedicated capture overlay flow inside `crates/rollshot-app`.

The Tauri window used for capture should be configured as an overlay window:

- transparent
- undecorated
- always on top
- skipped from taskbar where supported
- sized to cover the target monitor or virtual desktop, depending on launch
  options and existing capture geometry support

The frontend should render capture state directly instead of showing the current
app shell/start-panel workflow.

State model:

```text
Selecting -> Stitching -> Done
          -> Failed
          -> Cancelled
```

The old app-workbench layout can be removed or bypassed for the capture launch
path. The first implementation should avoid building a full screenshot editor.

Frontend units:

- `CaptureOverlay`: top-level state renderer for the capture flow.
- `SelectionLayer`: fullscreen canvas/DOM layer for selection mask, crosshair,
  auxiliary lines, and source-coordinate conversion.
- `OverlayToolbar`: minimal controls after a region is selected or while
  stitching.
- `AdaptiveStitchPreview`: live stitched-image preview positioned relative to
  the selected region.
- `CaptureStatus`: status-only fallback for platforms or placements where image
  preview could contaminate the capture.

## UI Behavior

### Selecting

On launch, show the overlay immediately.

Use a Snow-Shot-like initial affordance:

- darken the captured desktop/window behind the overlay
- set cursor to crosshair
- draw auxiliary lines through the cursor
- draw the selection rectangle while dragging
- do not show a central instruction bubble

`Esc` cancels capture.

### Region Selection

The user drags to select a crop region. On mouseup:

1. Convert overlay CSS coordinates to source pixels.
2. Clamp and validate the source region.
3. Call `confirm_region`.
4. Start stitching automatically.

Auto-start keeps the scrollshot workflow direct and avoids an unnecessary
confirmation step.

### Stitching

While stitching:

- keep the selected region visible
- allow the user to scroll inside the selected region
- show a compact stop control and status
- show live stitching preview when placement and capture-exclusion rules allow
  it

Preview placement should be adaptive rather than fixed-right.

Candidate placements:

1. right of selection
2. left of selection
3. below selection
4. above selection

The placement algorithm should score candidates by:

- whether the preview fully fits within the active monitor/overlay bounds
- available area on that side
- distance from the selected region
- whether it avoids covering selected content

If no outside placement fits:

- if overlay exclusion is verified, use compact preview inside the crop in the
  least intrusive corner
- if overlay exclusion is unsupported or unknown, use status-only mode

Linux/Wayland portal starts with the safe behavior: no inside image preview when
outside placement is unavailable.

### Done

Stopping stitching produces a final image. The overlay then shows:

- final preview
- save action
- close/cancel action

Copy/export actions can be added later if they fit existing app APIs cheaply,
but they are not required for the first version.

## Data Flow

1. CLI/launcher passes capture options to Tauri.
2. Overlay window opens and calls `start_capture`.
3. Capture reader updates latest frame/status as it does today.
4. `SelectionLayer` uses source dimensions and overlay geometry to map selected
   CSS pixels to source pixels.
5. On mouseup, frontend calls `confirm_region`, then `start_stitching`.
6. The stitching loop crops frames to the selected region and updates stitch
   stats/preview.
7. `AdaptiveStitchPreview` refreshes from `get_stitch_preview`.
8. User stops stitching.
9. Frontend calls `stop_stitching`, then shows final preview and save controls.

Polling can remain for the first version if it keeps the patch smaller. Evented
updates are a later optimization.

## Overlay Exclusion

Add an overlay exclusion capability visible to the frontend:

```ts
type OverlayExclusion = "verified" | "unsupported" | "unknown"
```

The frontend must only render image preview inside the selected crop when the
capability is `verified`.

Platform behavior:

- Windows: apply `WDA_EXCLUDEFROMCAPTURE` to the Tauri overlay HWND. Reapply it
  when the overlay window is shown or recreated. Mark `verified` only after the
  native call succeeds on a supported OS version.
- macOS: use the capture backend's excluded window/application target support.
  Mark `verified` only when the active backend can exclude the overlay from the
  captured stream.
- Linux Wayland portal: mark `unsupported` for self-exclusion in the first
  version. Use outside preview placement when available and status-only fallback
  otherwise.
- Unknown or unsupported backends: mark `unknown` or `unsupported`; use the
  safe fallback.

## Capture-Time UI Hiding

Even when overlay exclusion is verified, keep a Snow-Shot-style mitigation:

1. Hide or make capture-sensitive UI transparent before sampling a frame.
2. Wait one animation frame or a short backend-specific delay.
3. Capture/process the frame.
4. Restore UI.

Capture-sensitive UI includes toolbars, image preview, and any status element
that could overlap the selected region.

This is defense-in-depth for backends where exclusion is imperfect or briefly
not applied.

## First-Version Scope

In scope:

- Tauri transparent overlay capture window
- Snow-Shot-like selection state
- drag-to-select crop
- automatic stitch start on mouseup
- adaptive live preview placement
- safe no-space fallback based on overlay exclusion capability
- stop/final preview/save
- `Esc` cancel

Out of scope:

- drawing and annotation tools
- OCR
- window/element auto-select
- Wayland layer-shell implementation
- risky live preview toggle
- full screenshot editor
- rich resize handles unless they are cheap to preserve from current code

## Testing

Frontend tests:

- preview placement chooses right when it fits
- preview placement chooses left when right does not fit
- preview placement chooses top/bottom when horizontal sides do not fit
- no outside space returns inside-preview only for `verified`
- no outside space returns status-only for `unsupported` and `unknown`
- selection coordinate conversion handles HiDPI scaling
- `Esc` cancels
- mouseup transitions from selecting to stitching
- stop transitions from stitching to done

Rust/session tests:

- existing stitching tests continue to pass
- capability status is exposed to the frontend
- unsupported/unknown capability prevents unsafe inside-preview mode
- verified capability allows inside-preview mode
- region confirmation still rejects invalid and out-of-bounds regions

Manual verification:

- launching capture shows no normal app workbench
- overlay covers the intended display area
- selecting feels like a screenshot overlay, not an app preview
- live preview appears adjacent to the crop when space allows
- fullscreen crop on Linux/Wayland shows status-only rather than image preview
- stopping produces a final stitched image
- saving final image works

## Success Criteria

- The capture flow feels like an overlay tool.
- The user can launch, drag a crop, scroll, watch stitching progress when safe,
  stop, and save without interacting with an app workbench.
- rollshot does not knowingly include its own toolbar or preview in stitched
  output.
- Platform-specific exclusion limitations are explicit in behavior rather than
  hidden behind optimistic UI.
