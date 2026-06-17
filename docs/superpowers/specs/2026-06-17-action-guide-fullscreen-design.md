# Action Guide Fullscreen Recording Design

## Summary

Add fullscreen recording support to the Linux Action Guide workflow so users can
capture an entire display without selecting a region. Recording starts
immediately, is controlled through a temporary system-tray icon, and finishes
into the existing Action Guide Timeline Workspace.

```text
rollshot-app --action-guide --fullscreen
    -> start fullscreen stream
    -> show temporary system-tray recording icon
    -> show OS notification: "Click the tray icon to finish recording"
    -> user clicks tray icon
    -> stop recording, run detection
    -> open Action Guide Timeline Workspace
```

This is a Linux-only feature. macOS and Windows are explicitly out of scope for
this iteration.

## Goals

- Record the full display for an Action Guide session without a region-selection
  overlay.
- Provide a single, reliable stop mechanism: a temporary system-tray icon.
- Reuse the existing Action Guide detection and Timeline Workspace.
- Keep the tray icon's lifecycle bound to the recording session; no daemon or
  persistent tray item.
- Follow Spectacle's recording-start UX by showing an OS notification that
  explains how to stop.

## Non-Goals

- macOS or Windows support.
- Global hotkeys.
- A persistent daemon, system tray, menu bar, or DBus/IPC control surface.
- Pause/resume, right-click cancel, or other tray interactions in P0.
- Reusing the existing `fullscreen::capture` one-shot path (Action Guide needs a
  stream, not a one-shot image).
- Changing the existing region-based Action Guide flow.
- Customizable shortcuts or tray icon.

## Product Behavior

### Launch

The feature is launched through the app binary:

```text
rollshot-app --action-guide --fullscreen
```

The CLI wrapper forwards the flag:

```text
rollshot action-guide --fullscreen
```

`rollshot-app`'s `LaunchMode::ActionGuide` gains a `fullscreen: bool` field:

```rust
#[cfg(feature = "action-guide")]
LaunchMode::ActionGuide { fullscreen: bool },
```

- `--action-guide` alone keeps the existing region-selection behavior
  (`fullscreen = false`).
- `--action-guide --fullscreen` enables fullscreen recording
  (`fullscreen = true`).

### Recording Flow

1. `rollshot-app` creates an `OverlayConfig` with
   `CaptureRequest::action_guide_fullscreen()` and calls
   `rollshot_iced_overlay::run_action_guide_fullscreen(config, input_source)`.
2. The runner checks whether a system-tray icon can be created. If not, it
   returns an explicit error before any capture resource is acquired.
3. The runner creates a temporary tray icon with a red recording indicator.
4. The runner sends a transient OS notification:
   "Rollshot is recording. Click the tray icon to finish recording."
5. The runner acquires a fullscreen streaming resource (the same backend path
   used by scrolling capture, but with `RegionMode::FullSource`).
6. The runner starts Action Guide recording over the full display region.
7. The runner polls tray-icon events until the icon is clicked.
8. On click, the runner finalizes Action Guide recording and destroys the tray
   icon.
9. The runner returns `(Recording, InputCapability, CaptureRegion)`.
10. `rollshot-app` opens the Action Guide Timeline Workspace as it does for
    region recordings.

### Tray Icon

- The icon is created when recording starts and destroyed when the session ends
  (success, error, or cancellation).
- Left-click is the only P0 interaction and means "finish recording".
- The tooltip shows "Rollshot is recording — click to finish" and the elapsed
  recording time.

### OS Notification

- Sent once when recording starts.
- Uses `notify-rust` via the standard Freedesktop Desktop Notifications D-Bus
  service (`org.freedesktop.Notifications`).
- If the notification cannot be sent, the runner logs a warning and continues;
  the tray icon remains the authoritative stop path.

### Environment Without Tray Support

If the runner cannot create a tray icon, it returns an error such as:

```text
Fullscreen Action Guide requires a system tray. This environment does not support tray icons.
```

There is no fallback to a floating button, global hotkey, or fixed-duration
recording.

## Architecture

### Crate Changes

| Crate | Change |
|-------|--------|
| `rollshot-capture` | Add `CaptureRequest::action_guide_fullscreen()` and allow `(Workflow::ActionGuide, CaptureScope::Fullscreen)` in `is_supported()`. |
| `rollshot-iced-overlay` | Add `run_action_guide_fullscreen(config, input_source)`; acquire fullscreen stream, manage tray icon, run action recording, return action result. |
| `rollshot-app` | Extend `LaunchMode::ActionGuide` with `fullscreen`; wire the new runner; keep Timeline Workspace handoff. |
| `rollshot-cli` | Add `--fullscreen` to `ActionGuideArgs` and forward it to `rollshot-app --action-guide --fullscreen`. |

### New Dependencies

- `tray-icon`: cross-platform tray icon support (Linux SNI in this iteration).
- `notify-rust`: Freedesktop notifications.

### Data Flow

```text
rollshot-app --action-guide --fullscreen
  │
  ▼
launch::parse_launch_args → LaunchMode::ActionGuide { fullscreen: true }
  │
  ▼
run_action_guide_record()
  │   input_source = create_input_source()
  │   config = OverlayConfig { request: action_guide_fullscreen(), ... }
  │
  ▼
rollshot_iced_overlay::run_action_guide_fullscreen(config, input_source)
  │
  ├── tray available? → No  → Err("Fullscreen Action Guide requires a system tray")
  │
  ├── create tray icon
  ├── send OS notification (best-effort)
  ├── acquire fullscreen streaming resource
  ├── begin_action_recording(full_display_region, input_source)
  ├── poll tray click
  ├── finalize_action() on click
  ├── destroy tray icon
  └── Ok(Some((recording, capability, region)))
  │
  ▼
timeline_workspace::run(recording, region, capability, source_kind)
```

### Capture Region

Fullscreen Action Guide records the entire selected display. The
`CaptureRegion` passed to the recorder is the display's logical region
(`x = 0, y = 0`, width/height from the streaming driver's source size). The
`ActionRecorder` receives uncropped full-display frames.

### Separation from Existing Fullscreen Path

The existing `rollshot_iced_overlay::fullscreen::capture` handles one-shot
screenshots. Action Guide fullscreen is a separate path because it requires a
streaming resource and action recording. Both paths bypass the overlay, but they
do not share implementation.

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Tray creation unsupported | Return `OverlayError::Capture` immediately; no resource acquired. |
| Fullscreen stream acquisition fails | Return `OverlayError::Capture`; destroy tray icon. |
| Portal picker dismissed | Return `Ok(None)`; destroy tray icon; no Timeline Workspace. |
| Recording stream ends unexpectedly | Return `OverlayError::Capture`; destroy tray icon. |
| `finalize_action` fails | Return error; destroy tray icon. |
| Notification fails | Log warning; continue recording. |

The tray icon is destroyed in all exit paths, success or failure. An RAII guard
or explicit `Drop` implementation ensures this.

## Testing

### Unit Tests (CI)

| Test | Location |
|------|----------|
| `action_guide_fullscreen()` returns the expected `CaptureRequest` | `crates/rollshot-capture/src/types.rs` |
| `is_supported()` accepts `(Workflow::ActionGuide, CaptureScope::Fullscreen)` | `crates/rollshot-capture/src/types.rs` |
| `needs_overlay()` returns `false` for fullscreen Action Guide | `crates/rollshot-capture/src/types.rs` |
| `--action-guide --fullscreen` parses correctly | `crates/rollshot-app/src/launch.rs` |
| CLI wrapper forwards `--fullscreen` | `crates/rollshot-cli/src/cmd_action_guide.rs` |

### Module Tests (Mocked)

| Test | Purpose |
|------|---------|
| Tray lifecycle: create → click → destroy | Validate that click events stop recording and that the icon is always cleaned up. |
| Tray creation failure returns error before capture | Validate the hard requirement. |
| Stream acquisition failure cleans up tray | Validate cleanup on errors. |
| Notification failure does not abort recording | Validate best-effort notification behavior. |

### Manual Tests

- Run `rollshot-app --action-guide --fullscreen` on a real KDE environment.
- Verify the tray icon appears, the notification is shown, clicking the icon
  stops recording, and the Timeline Workspace opens with detected steps.
- Verify that region Action Guide (`--action-guide` without `--fullscreen`) is
  unchanged.

## Future Work

- macOS support through the same runner shape, using `tray-icon`'s
  `NSStatusItem` backend.
- Global hotkey integration, ideally through KDE `KGlobalAccel` D-Bus for proper
  KDE Wayland support or a cross-platform crate if Wayland limitations are
  acceptable.
- Right-click tray menu with explicit Cancel.
- Pause/resume recording.
