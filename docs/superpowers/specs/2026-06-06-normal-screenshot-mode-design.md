# Normal Screenshot Mode Design

## Summary

Add a basic normal screenshot workflow to `rollshot-app` alongside the existing
scrolling screenshot workflow. The launch payload selects the initial workflow,
with scrolling screenshot remaining the default for backward compatibility.

Normal screenshot mode captures one frozen image of the target display without
starting a screen stream. The user drags a rectangular selection over that
frozen image, and releasing the left mouse button immediately crops the image
and opens the existing PNG save flow.

This MVP supports Linux Wayland and macOS. KDE/KWin receives a precise native
one-shot path. Other Wayland compositors may use the Screenshot portal only
when the returned desktop image can be reliably mapped to the compositor's
active output.

## Goals

- Add `scrolling` and `screenshot` capture modes, with `scrolling` as default.
- Ensure screenshot mode never starts ScreenCast, PipeWire, `SCStream`, or the
  stitching pipeline.
- Show a frozen image while the user selects the screenshot region.
- Capture the display containing the mouse on KDE/KWin and macOS.
- Keep the overlay session architecture capable of switching modes from a
  future toolbar without restructuring platform runners or capture backends.

## Non-Goals

- Implementing a toolbar or in-session mode switch UI.
- Selection adjustment or reselection after mouse release.
- Annotations, clipboard output, window detection, UI element detection, or
  delayed screenshots.
- Cross-display region selection.
- Using the portal's system region picker.
- Falling back to the first frame of a stream.

## Launch Contract

`InteractiveLaunchOptions` gains an `initial_mode` field:

```json
{
  "initial_mode": "screenshot",
  "backend": "auto",
  "fps": 5,
  "show_cursor": false,
  "overlay_mode": "iced"
}
```

`initial_mode` accepts `scrolling` and `screenshot`. Missing `initial_mode`
defaults to `scrolling`, preserving old payload behavior. The `fps` field is
ignored by screenshot mode.

The field is named `initial_mode`, rather than `mode`, because it selects the
workflow active when the overlay starts. The overlay session and platform
runners must not assume that the workflow remains fixed for the session's
entire lifetime.

## Capture Architecture

### Separate One-Shot Capability

One-shot capture is separate from the existing streaming `CaptureBackend`:

```rust
trait OneShotCaptureBackend {
    fn capture_once(
        &mut self,
        options: OneShotCaptureOptions,
    ) -> Result<OneShotCapture, CaptureError>;
}

struct OneShotCapture {
    image: RgbaImage,
    target_display: DisplayTarget,
}
```

`DisplayTarget` contains the platform output identity and geometry needed to
open the overlay on the same display and map logical selection coordinates to
image pixels.

There is no default implementation that reads the first frame from a stream.
If a platform has no valid one-shot backend, screenshot mode returns an
unsupported or capture error.

### Platform Backends

#### KDE/KWin

KDE/KWin uses the restricted native DBus interface:

```text
org.kde.KWin.ScreenShot2.CaptureActiveScreen
```

This captures the display containing the mouse and returns the KWin screen
identity with the image metadata. The iced layer-shell overlay opens on that
same output using `StartMode::TargetScreen`.

The installed desktop entry declares:

```ini
X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2
```

If `ScreenShot2` is unavailable, permission is denied, capture fails, or the
returned screen cannot be matched to an output, Rollshot returns an error.
KDE/KWin must not fall back to the Screenshot portal.

#### Other Wayland Compositors

Other Wayland compositors use `org.freedesktop.portal.Screenshot` for a
one-shot desktop image. Rollshot may proceed only when it can reliably identify
the compositor's active output and map that output to the returned desktop
image. Otherwise it returns unsupported or a mapping error.

The portal path is best-effort with respect to the user's mouse display because
the Wayland and Screenshot portal standards do not guarantee global pointer
position access.

Portal cancellation is treated as user cancellation, not an error.

#### macOS

macOS identifies the display containing the mouse and captures it through
`SCScreenshotManager`. The overlay opens on the same display.

Screenshot mode still requires macOS Screen Recording permission. Missing
permission produces an explicit permission error. Rollshot must not fall back
to `SCStream` or scap streaming capture.

## Overlay Session Architecture

The overlay owns a session whose active workflow can change:

```rust
enum CaptureWorkflow {
    Scrolling(ScrollingWorkflow),
    Screenshot(ScreenshotWorkflow),
}

struct OverlaySession {
    active_mode: CaptureMode,
    workflow: CaptureWorkflow,
}
```

Mode-specific fields live inside the corresponding workflow. Shared fields are
limited to truly shared UI state. In particular, scrolling state such as
stitching progress and screenshot state such as the frozen image must not share
ambiguous flags such as a session-wide `crop_confirmed`.

Platform runners execute workflow-independent effects:

```rust
enum OverlayEffect {
    None,
    StartScrollingCapture,
    StopScrollingCapture,
    CaptureOneShot,
    Finish,
    Cancel,
}
```

The MVP starts the workflow selected by `initial_mode` and does not expose a
switch-mode message. A future toolbar can add
`OverlayMessage::SwitchMode(CaptureMode)`:

- Screenshot to scrolling discards the frozen image, starts the streaming
  driver, and requests a new scrolling region.
- Scrolling to screenshot stops the driver, performs a fresh one-shot capture,
  and requests a new screenshot region.

Backends are started on demand and stopped before activating the other
workflow.

## Screenshot Interaction

1. Complete one-shot capture before presenting the selection overlay.
2. Display the captured image as the overlay's frozen background.
3. Draw the existing dim mask, crosshair guides, and selection border above the
   frozen image.
4. Let the user drag a non-empty rectangular region.
5. On left mouse release, immediately map the selection to image pixels, crop
   the frozen image, close the overlay, and open the existing PNG save flow.
6. Before a valid selection exists, `Esc` cancels without producing an image.

A click or empty selection does not complete the capture and leaves the
selection overlay active.

## Result Contract

`CaptureResult.stats` becomes `Option<StitchStats>`:

- Scrolling screenshot returns `Some(stats)`.
- Normal screenshot returns `None`.

This avoids fabricating stitching metrics for a workflow that does not use the
stitcher.

## Error Handling

- KDE/KWin native API absence, permission denial, output mismatch, or capture
  failure is an error with no portal fallback.
- Other Wayland portal cancellation is cancellation.
- Other Wayland output identification or image mapping failure is an explicit
  unsupported or mapping error.
- Missing macOS Screen Recording permission is an explicit permission error.
- Invalid or empty screenshot selections remain in selection mode.
- No screenshot-mode error path starts a stream as fallback.

## Expected Code Boundaries

### `rollshot-capture`

- Add `CaptureMode`, defaulting to `Scrolling`.
- Add one-shot capture options, result, backend trait, and backend selection.
- Add KWin `ScreenShot2`, Wayland Screenshot portal, and macOS
  `SCScreenshotManager` one-shot implementations.

### `rollshot-app`

- Parse and forward `initial_mode`.
- Preserve scrolling as the no-argument and old-payload default.
- Continue using the existing PNG save helper.

### `rollshot-iced-overlay`

- Introduce the workflow-oriented overlay session.
- Keep the existing streaming driver and stitcher inside the scrolling
  workflow.
- Add frozen-image rendering and direct image cropping to the screenshot
  workflow.
- Keep Linux and macOS runners workflow-independent by executing overlay
  effects.

### Linux Packaging

- Declare the KWin restricted screenshot interface in the installed desktop
  entry.

## Testing

### Unit and Integration Tests

- Old JSON without `initial_mode` parses as `scrolling`.
- Both valid `initial_mode` values round-trip through JSON.
- Screenshot mode ignores `fps`.
- Backend selection chooses KWin `ScreenShot2` on KDE and does not portal
  fallback after a KWin failure.
- Other Wayland rejects unreliable active-output mappings.
- Screenshot mode does not construct a stream, `Driver`, or `Stitcher`.
- Frozen-image selection maps and crops correctly at 1x and HiDPI scales.
- Valid drag release immediately finishes.
- Click, empty selection, and `Esc` follow the specified behavior.
- Scrolling results contain stats and screenshot results do not.
- Existing scrolling overlay tests continue to pass.

### Platform Verification

- Linux KDE/KWin: verify exact mouse-display capture, output matching, frozen
  selection, immediate completion, and explicit failure when `ScreenShot2`
  access is unavailable.
- Linux non-KDE Wayland: verify Screenshot portal cancellation and supported
  active-output mapping; verify unsupported mapping fails explicitly.
- macOS: verify exact mouse-display capture, Screen Recording permission
  handling, frozen selection, and absence of `SCStream`.
- Run Rust tests, formatting, and workspace clippy checks.

## Acceptance Criteria

- Starting `rollshot-app` without a mode still starts scrolling screenshot.
- `initial_mode: screenshot` never creates ScreenCast, PipeWire, `SCStream`, a
  streaming driver, or stitching threads.
- KDE/KWin captures the display containing the mouse through `ScreenShot2` and
  fails explicitly if that native path cannot be used.
- Other Wayland runs only when active-output mapping is reliable.
- macOS captures the display containing the mouse through a native one-shot
  API.
- Screenshot selection displays a frozen image and completes immediately on a
  valid left-button release.
- The overlay session and platform runner boundaries permit a future toolbar to
  switch workflows without replacing capture backend interfaces or rewriting
  platform runners.
