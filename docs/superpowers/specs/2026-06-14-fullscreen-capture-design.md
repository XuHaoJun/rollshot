# Fullscreen Capture Design

## Summary

Add a fullscreen capture workflow that immediately captures the display
containing the pointer without opening the selection overlay. The completed
image continues through Rollshot's existing platform presentation policy:
Linux opens the Result Workspace, while macOS shows the saved-capture
thumbnail.

The launch contract gains `initial_mode: "fullscreen"`. The existing
`screenshot` mode is renamed to `region` because it represents rectangular
region selection rather than every kind of screenshot. Existing payloads using
`initial_mode: "screenshot"` remain accepted as a compatibility alias.

Fullscreen capture supports macOS and KDE/KWin. Other Linux environments return
an explicit unsupported error because the Screenshot portal cannot guarantee a
capture of the display containing the pointer.

## Goals

- Capture the complete display containing the pointer with no selection step.
- Support fullscreen capture through the existing `--capture <JSON>` launch
  contract.
- Preserve each platform's existing post-capture save and presentation policy.
- Rename the region-selection workflow from `Screenshot` to `Region`.
- Preserve compatibility with old JSON payloads using `"screenshot"`.
- Keep capture acquisition shared between region and fullscreen workflows.

## Non-Goals

- Capturing all displays as one image.
- Capturing the primary display regardless of pointer location.
- Adding a tray menu, global shortcut system, desktop action, or new CLI flag.
- Adding fullscreen as an in-overlay toolbar mode.
- Changing auto-save behavior, the Result Workspace, or the macOS thumbnail
  flow.
- Supporting fullscreen capture through the non-KDE Linux Screenshot portal.
- Changing headless capture behavior.

## Product Behavior

### Region

`initial_mode: "region"` preserves the current normal screenshot behavior:

1. Capture a frozen one-shot image of the display containing the pointer.
2. Open the selection overlay on that display.
3. Let the user select a rectangular region.
4. Complete with the cropped image.

The overlay toolbar may switch between `Region` and `Scrolling` because both
are interactive overlay workflows.

### Fullscreen

`initial_mode: "fullscreen"` performs a direct capture:

1. Capture a frozen one-shot image of the display containing the pointer.
2. Do not create selection overlay or controls windows.
3. Complete with the entire one-shot image.
4. Apply the existing platform presentation policy.

Fullscreen is a launch-time capture intent, not an overlay workflow. It is not
available from the overlay toolbar and cannot be switched to or from during an
overlay session.

### Post-Capture Presentation

Fullscreen does not introduce a special result flow:

- Linux auto-saves and opens the Result Workspace.
- macOS auto-saves and shows the existing eight-second thumbnail. Clicking the
  thumbnail opens the Result Workspace.
- Existing unsaved-workspace behavior remains unchanged when auto-save fails.

## Launch Contract

`InteractiveLaunchOptions.initial_mode` accepts:

```json
{"backend":"auto","fps":5,"show_cursor":false,"initial_mode":"scrolling"}
{"backend":"auto","fps":5,"show_cursor":false,"initial_mode":"region"}
{"backend":"auto","fps":5,"show_cursor":false,"initial_mode":"fullscreen"}
```

Missing `initial_mode` continues to default to `scrolling`.

The Rust enum becomes:

```rust
enum CaptureMode {
    Scrolling,
    Region,
    Fullscreen,
}
```

`CaptureMode::Region` serializes as `"region"` and accepts `"screenshot"` only
as a deserialization alias. Rollshot's serialization, tests, and current
documentation use `"region"` exclusively.

The existing `initial_mode` field remains the single launch selector. A new
parallel intent field is unnecessary because all three values select the
capture workflow that starts when the app launches.

## Architecture

```text
rollshot-app --capture <JSON>
            |
            v
InteractiveLaunchOptions.initial_mode
            |
            v
      Scrolling | Region | Fullscreen
            |        |         |
            |        |         +--> one-shot --> complete
            |        +------------> one-shot --> selection overlay --> crop
            +---------------------> stream --> selection overlay --> stitch
                                                     |
                                                     v
                                      existing post-capture presentation
```

### `rollshot-capture`

`rollshot-capture` owns the mode contract and one-shot acquisition:

- Rename `CaptureMode::Screenshot` to `CaptureMode::Region`.
- Add `CaptureMode::Fullscreen`.
- Preserve the existing one-shot backend abstraction and result types.
- Ensure the selected one-shot backend captures the display containing the
  pointer.
- Return an explicit unsupported error when fullscreen cannot guarantee that
  semantic.

Region and fullscreen use the same one-shot acquisition. Their difference is
what happens after acquisition: region opens the overlay, while fullscreen
returns the complete image immediately.

### `rollshot-iced-overlay`

`rollshot-iced-overlay` remains the shared capture-session boundary used by
both platform product paths:

- `Scrolling` acquires streaming resources and opens the interactive overlay.
- `Region` acquires a one-shot resource and opens the interactive overlay.
- `Fullscreen` acquires a one-shot resource and completes without creating
  overlay or controls windows.

The crate should expose one shared fullscreen completion path so
`rollshot-app` does not duplicate platform backend selection, capture error
mapping, or `CaptureResult` construction.

Overlay state, toolbar state, and mode-switch effects support only `Scrolling`
and `Region`. Exhaustive matches must treat `Fullscreen` as unreachable in an
active overlay session or route it before constructing overlay state.

### `rollshot-app`

`rollshot-app` continues to:

- Parse `InteractiveLaunchOptions`.
- Forward the selected initial mode into the capture session.
- Route completed captures through existing platform-specific product flows.

Fullscreen changes acquisition only. It does not add a separate app launch
mode, save policy, or result presentation policy.

## Platform Behavior

### macOS

macOS uses the existing `SCScreenshotManager` one-shot path. It identifies and
captures the display containing the pointer, returning native-resolution pixels
and logical display geometry.

Fullscreen does not create the iced capture overlay. A successful capture
enters the existing macOS completion path, which auto-saves and displays the
saved-capture thumbnail.

Missing Screen Recording permission or one-shot capture failure returns the
existing explicit capture error. There is no streaming fallback.

### KDE/KWin

KDE/KWin uses the existing restricted ScreenShot2 request:

```text
org.kde.KWin.ScreenShot2.CaptureActiveScreen
```

This captures the screen containing the pointer and returns its output name.
Fullscreen completes with the full returned image and does not create the
layer-shell selection overlay.

If ScreenShot2 is unavailable, denied, cancelled, or returns invalid metadata,
fullscreen returns the corresponding existing capture error. It does not fall
back to the Screenshot portal.

### Other Linux Environments

Fullscreen is unsupported outside KDE/KWin. Rollshot must return an explicit
unsupported error before opening an overlay or Result Workspace.

The non-KDE Screenshot portal is not used as a fallback. It cannot reliably
guarantee that a non-interactive screenshot is a single image of the display
containing the pointer.

Region mode retains its existing non-KDE behavior; this restriction applies
only to fullscreen.

## Error Handling and Diagnostics

- User cancellation remains cancellation and produces no result presentation.
- Permission denial, unsupported environment, and acquisition failure produce
  existing capture errors.
- Fullscreen failure must not open overlay, controls, thumbnail, or Result
  Workspace windows.
- Fullscreen must not start ScreenCast, PipeWire, `SCStream`, a streaming
  driver, or stitching threads.
- Runtime diagnostics use stable existing `rollshot::*` tracing targets with
  structured fields.
- Diagnostics may record mode, backend category, platform, and outcome, but
  must not record image contents or sensitive output paths.

## Compatibility

- No `initial_mode` field: defaults to `scrolling`.
- `"initial_mode":"screenshot"`: deserializes as `CaptureMode::Region`.
- `"initial_mode":"region"`: deserializes and serializes as
  `CaptureMode::Region`.
- `"initial_mode":"fullscreen"`: selects direct fullscreen capture.
- Existing code and documentation migrate from `Screenshot`/`"screenshot"` to
  `Region`/`"region"` except for explicit compatibility tests.
- Existing `rollshot capture` behavior remains scrolling because its launcher
  continues to emit `initial_mode: "scrolling"`.

## Testing

### Mode Contract

- Missing `initial_mode` defaults to `Scrolling`.
- `Scrolling`, `Region`, and `Fullscreen` round-trip through JSON using their
  current names.
- Legacy `"screenshot"` JSON deserializes as `Region`.
- Serializing `Region` never emits `"screenshot"`.
- App launch parsing and CLI launcher forwarding preserve the selected mode.

### Shared Capture Flow

- Region and fullscreen select the same one-shot acquisition capability.
- Region opens selection UI and completes with a cropped image.
- Fullscreen completes with the unchanged full one-shot image.
- Fullscreen creates no overlay or controls windows.
- Fullscreen creates no stream, preview subscription, driver, stitcher, or
  stitching thread.
- Overlay toolbar mode switching remains limited to Region and Scrolling.

### Platform Behavior

- KDE/KWin fullscreen uses `CaptureActiveScreen`, returns the complete image,
  and never falls back to portal.
- Non-KDE Linux fullscreen returns unsupported without invoking portal.
- macOS fullscreen uses the native one-shot path and does not start `SCStream`.
- Existing region-mode platform tests continue to pass after the rename.

### Post-Capture Behavior

- Linux fullscreen success follows the existing saved/unsaved Result Workspace
  policy.
- macOS fullscreen success follows the existing saved-thumbnail policy.
- macOS auto-save failure follows the existing unsaved-workspace policy.
- Capture failure or cancellation produces no post-capture presentation.

### Manual Verification

- On macOS with multiple displays, place the pointer on each display and verify
  fullscreen captures exactly that complete display, then shows the saved
  thumbnail.
- On KDE/KWin with multiple displays, repeat the pointer-display check and
  verify the Result Workspace opens with the complete captured display.
- On non-KDE Linux, verify fullscreen fails explicitly without a portal picker
  or selection overlay.
- Verify region selection and scrolling capture remain unchanged on both active
  platform paths.

## Acceptance Criteria

- `initial_mode: "fullscreen"` immediately captures the display containing the
  pointer on macOS and KDE/KWin.
- Fullscreen never opens the selection overlay and never crops the one-shot
  image.
- Fullscreen preserves Linux and macOS post-capture presentation policies.
- Non-KDE Linux fullscreen fails explicitly without portal fallback.
- `CaptureMode::Screenshot` is replaced by `CaptureMode::Region`.
- New payloads and documentation use `"region"`, while legacy
  `"screenshot"` payloads remain accepted.
- Existing scrolling and region-selection behavior remains unchanged.
