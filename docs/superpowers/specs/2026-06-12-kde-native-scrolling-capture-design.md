# KDE Native Scrolling Capture Design

## Summary

On KDE Plasma Wayland, Rollshot scrolling capture currently uses the
freedesktop ScreenCast portal. The portal always presents its source picker
before it provides a PipeWire stream, even though Rollshot only wants the
currently active monitor.

Rollshot will add a KDE-native scrolling capture path that follows Spectacle's
architecture:

- use KWin `ScreenShot2` to capture the active output for the frozen selection
  background;
- use KWin's private `zkde_screencast_unstable_v1` Wayland protocol to create a
  live PipeWire stream for that output;
- reuse Rollshot's existing PipeWire frame processing, local crop, and
  stitching pipeline.

When the requested backend is `auto`, Rollshot will prefer the KDE-native path
and automatically fall back to the existing portal path if native capture
cannot start. Explicit backend selection never falls back.

## Goals

- Remove the portal source picker from the normal KDE scrolling capture flow
  when Rollshot is correctly installed.
- Start selection on the currently active KDE output, matching the existing
  macOS product flow as closely as the platform APIs allow.
- Preserve portal capture as the compatibility path for non-KDE Wayland
  desktops, development binaries, and KDE-native startup failures.
- Keep the existing PipeWire frame conversion, crop mapping, stitching, live
  preview, and result workspace behavior.
- Document the desktop entry installation required for KDE-native capture.

## Non-Goals

- Replacing the portal path on GNOME, wlroots compositors, or other non-KDE
  desktops.
- Adding native window capture.
- Using KWin's `stream_region` request in the first version.
- Falling back to the portal after a native stream has started.
- Repeated `ScreenShot2` requests as a substitute for a live stream.
- Changing normal screenshot mode behavior beyond sharing common KDE-native
  support where appropriate.
- Adding a user preference for fallback behavior. `auto` always falls back
  automatically before the overlay opens.

## Current Behavior

Linux scrolling capture resolves `auto` to the `linux-portal` backend. The
backend creates a ScreenCast portal session, requests a monitor source, waits
for the user to approve the portal picker, opens the portal PipeWire remote,
and connects Rollshot's PipeWire consumer.

The capture resource is acquired before the iced layer-shell overlay opens so
the portal picker cannot appear in captured frames. This also means any
native-to-portal fallback must complete before the overlay opens.

KDE normal screenshot mode already uses KWin `ScreenShot2` through
`CaptureActiveScreen`. It returns the active output's frozen image, output
name, logical size, physical size, and scale. This existing one-shot path is
the source of truth for selecting and placing the KDE scrolling overlay.

## Backend Selection

The streaming backend set gains an explicit KDE-native backend named
`linux-kwin`.

Backend resolution behaves as follows:

| Requested backend | Environment | Behavior |
| --- | --- | --- |
| `auto` | macOS | Existing ScreenCaptureKit backend |
| `auto` | KDE Wayland | Try `linux-kwin`; fall back to `linux-portal` if native startup fails |
| `auto` | Other Linux Wayland | Existing `linux-portal` backend |
| `linux-kwin` | KDE Wayland | Try native capture; return the native error on failure |
| `linux-portal` | Linux Wayland | Existing portal flow and source picker |

`linux-kwin` on unsupported environments returns `CaptureError::Unsupported`.
The CLI/backend parser documents `linux-kwin` as an available explicit
backend.

Fallback is a product-level `auto` behavior, not behavior internal to the KWin
backend. Keeping the native backend strict ensures explicit backend requests,
tests, and diagnostics cannot silently change capture mechanisms.

## KDE-Native Capture Flow

### 1. Acquire the Active Output

Before opening the overlay, Rollshot calls the existing KDE one-shot
`CaptureActiveScreen` path. The result provides:

- a frozen image for the selection background;
- the KWin output name used to target the layer-shell overlay;
- logical and physical dimensions used to validate coordinate mapping.

If the active output cannot be resolved reliably, native startup fails before
the overlay opens.

### 2. Open the Selection Overlay

Before opening the overlay, Rollshot binds KWin's
`zkde_screencast_unstable_v1` Wayland global, requests `stream_output` for the
same output returned by the one-shot capture, connects the native PipeWire
consumer, and waits for the first frame. Completing native stream startup at
this point allows `auto` to fall back to the portal before any Rollshot
selection UI is shown.

Rollshot opens the iced layer-shell overlay on the output named by the
one-shot capture. Selection uses the frozen active-output image as its
background, as normal screenshot mode already does.

The live stream runs while the user selects a crop, but those frames are not
sent to the stitcher. The frozen one-shot remains the selection background.

### 3. Begin Stitching

After crop confirmation, Rollshot begins consuming current live frames from
the already-started native stream. KWin's stream uses the configured cursor
mode. Rollshot connects to the user's regular PipeWire instance rather than a
portal-provided remote file descriptor, then negotiates and processes frames
through the existing Linux PipeWire implementation.

The first version captures the complete output and uses Rollshot's existing
local crop mapping before stitching. `stream_region` is deferred because
`stream_output` avoids introducing workspace-global region coordinates and
mixed-scale multi-output behavior into the first implementation.

### 4. Capture and Finish

After the native stream starts, existing scrolling behavior remains unchanged:

- input passes through inside the selected region;
- Rollshot crops frames to the selected region;
- the stitcher consumes accepted frames and updates the live preview;
- Finish, Cancel, capture-miss handling, and result handoff follow the current
  Linux overlay flow.

## Components and Boundaries

### KWin Screencast Client

A Linux-only KWin screencast client owns the Wayland connection, binds
`zkde_screencast_unstable_v1`, resolves the requested output, issues
`stream_output`, and waits for the created or failed event.

Its public responsibility is limited to producing a native stream session
containing the PipeWire identifier and resources required to keep the KWin
stream alive. It does not process frames or perform fallback.

The protocol XML is vendored or generated at build time from the KDE protocol
definition under its LGPL-2.1-or-later license. Runtime availability is
detected from the Wayland registry; the build must not require a system
installation of `plasma-wayland-protocols`.

### Native PipeWire Connection

The existing Linux PipeWire connection code gains a native connection mode
that connects to the user's regular PipeWire instance and targets the KWin
stream identifier. Portal mode continues to connect through the
portal-provided remote file descriptor.

Frame negotiation, metadata handling, pixel conversion, queueing, and
`FrameStream` behavior remain shared.

### KDE Streaming Backend

`LinuxKwinBackend` implements `CaptureBackend`. It validates KDE Wayland
availability, starts the KWin output stream, connects the native PipeWire
consumer, and returns a `FrameStream` that keeps both resources alive.

The backend is strict: it never creates or invokes a portal backend.

### Auto Fallback Coordinator

The capture startup layer coordinates `auto` fallback:

1. identify KDE Wayland;
2. acquire the active-output one-shot and complete native stream startup;
3. if startup returns a fallback-eligible error, emit a structured warning and
   start the existing portal backend;
4. return the resulting capture resource before opening the overlay.

The coordinator must not expose a partially initialized native resource to the
overlay.

## Fallback Rules

Automatic fallback is allowed only when all of the following are true:

- the requested backend is `auto`;
- no selection overlay has opened;
- no native stream has been exposed as started;
- the failure indicates that KDE-native capture is unavailable or failed
  during startup.

Fallback-eligible failures include:

- the KWin screencast Wayland global is unavailable;
- the installed desktop entry does not authorize the interface;
- KWin rejects or fails the stream request;
- native stream creation times out;
- the active output cannot be mapped to a KWin output;
- the native PipeWire connection cannot start.

The following failures do not trigger fallback:

- `CaptureError::UserCancelled`;
- explicit `linux-kwin` selection;
- errors after a native stream has started;
- invalid Rollshot configuration;
- crop or coordinate mapping errors after selection.

Every automatic fallback emits a privacy-safe structured warning with:

- stable target `rollshot::capture::linux::kwin`;
- a stable fallback reason category;
- `fallback = "linux-portal"`;
- the original error as a display-safe diagnostic field.

The portal picker itself is the user-visible indication that fallback
occurred. The first version does not add an additional prompt or persistent
fallback preference.

## Desktop Entry and Installation

KWin restricts both the `ScreenShot2` D-Bus interface and the private
screencast Wayland interface. Rollshot's desktop entry must declare both:

```ini
X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2
X-KDE-Wayland-Interfaces=zkde_screencast_unstable_v1
```

KWin identifies the caller from the running executable and installed desktop
entry. The desktop entry's absolute `Exec` path must match the running
`rollshot-app` binary. Launching from the application menu is not required,
but launching a different binary path, such as `cargo run` or
`target/debug/rollshot-app`, normally prevents native authorization and causes
`auto` to fall back to the portal.

README's existing KDE normal screenshot permission section becomes a KDE
native capture installation section. It retains the current system and local
installation commands and adds:

- the new `X-KDE-Wayland-Interfaces` requirement;
- expected `auto` behavior with and without a matching desktop entry;
- a native verification command that should not show the portal picker;
- an explicit `linux-portal` command for testing the fallback path;
- troubleshooting for mismatched `Exec` paths and missing Wayland interface
  authorization.

## Error Handling and Diagnostics

Native startup stages use bounded waits and map failures into existing
`CaptureError` categories where possible:

- unavailable protocol or unsupported environment:
  `CaptureError::Unsupported`;
- authorization rejection: `CaptureError::PermissionDenied`;
- bounded stage expiration: `CaptureError::Timeout`;
- protocol, PipeWire, or lifecycle failure: `CaptureError::Backend`;
- unreliable output/coordinate mapping: `CaptureError::Mapping`;
- explicit compositor cancellation: `CaptureError::UserCancelled`.

KWin-native runtime diagnostics use stable explicit targets under
`rollshot::capture::linux::kwin` and structured fields. Per-frame details, if
needed, use `trace`.

Fallback warnings record the native failure without hiding it. If the portal
fallback also fails, the returned error reports both the native and portal
failure contexts.

## Testing

### Unit Tests

- backend selection matrix for `auto`, `linux-kwin`, and `linux-portal`;
- KDE detection and strict explicit-backend behavior;
- fallback eligibility classification;
- `UserCancelled`, invalid configuration, and post-start failures never
  fallback;
- native success skips portal construction;
- eligible native startup failure invokes the portal exactly once;
- both native and portal failure contexts are preserved;
- KWin screencast protocol event and timeout mapping;
- output-name mapping and mismatch rejection;
- native PipeWire connection selection without a portal file descriptor;
- desktop entry contains both required KDE declarations.

### Existing Regression Coverage

- portal capture tests continue to pass unchanged;
- KDE one-shot screenshot tests continue to pass;
- Linux overlay selection, crop mapping, stitching, preview, Finish, and Cancel
  tests continue to pass;
- macOS capture and overlay behavior remains unchanged.

### KDE Runtime Verification

Runtime verification requires an installed release binary whose path matches
the desktop entry:

1. `auto` scrolling capture opens directly on the active output without a
   portal picker.
2. Selection background and live stream target the same output.
3. Scrolling capture works with cursor disabled and enabled.
4. Single-monitor, multi-monitor, integer-scale, and fractional-scale setups
   map the crop correctly.
5. Removing or invalidating native authorization causes `auto` to show the
   portal picker and continue through portal capture.
6. Explicit `linux-kwin` returns the native error without a portal picker.
7. Explicit `linux-portal` always shows the portal picker.
8. A native stream failure after capture starts reports an error without
   opening the portal picker.

## Risks

`zkde_screencast_unstable_v1` explicitly describes itself as a KDE desktop
implementation detail and permits backward-incompatible changes. The client
must detect supported protocol versions at runtime, keep the implementation
isolated, and fail into the portal compatibility path when `auto` is used.

KWin authorization and output identity depend on desktop integration details.
The product documentation and diagnostics must make a mismatched executable
path distinguishable from a general capture failure.

Native PipeWire connection lifecycle differs from the portal remote lifecycle.
The native stream session must remain alive until the PipeWire consumer stops,
and teardown order must be covered by tests.

## Success Criteria

- Correctly installed Rollshot on KDE Wayland starts scrolling selection on the
  active output without displaying the portal picker.
- `auto` falls back to the existing portal picker when KDE-native startup
  cannot complete before the overlay opens.
- Explicit `linux-kwin` never falls back, and explicit `linux-portal` never
  attempts native capture.
- Native capture reuses the existing Linux frame processing, local crop, and
  stitching behavior.
- README provides complete system-install, local-install, verification, and
  fallback-testing instructions.
