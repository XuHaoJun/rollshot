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
   Because `needs_overlay()` is already `false` for `CaptureScope::Fullscreen`,
   no layer-shell overlay window is created — this runner is **headless** (see
   "Runner Architecture").
2. The runner probes for an SNI host (a registered `StatusNotifierWatcher` with
   `IsStatusNotifierHostRegistered == true`). If absent, it returns an explicit
   error before any capture resource is acquired.
3. The runner registers a temporary SNI item (via `ksni`) with a red recording
   indicator and an on-activate handler that sends `()` over a `finish` channel.
4. The runner sends a transient OS notification:
   "Rollshot is recording. Click the tray icon to finish recording."
5. The runner acquires a fullscreen streaming `Driver` (the same backend path
   used by scrolling capture — always `RegionMode::FullSource`).
6. The runner derives the full-display `CaptureRegion` from the driver's source
   size (`x = 0, y = 0, width/height` from the stream) and calls
   `driver.begin_action_recording(full_region, input_source)`, owning the
   `Driver` as a local value.
7. The runner blocks on the `finish` channel (the tray activate event). No
   second event loop is spawned; `ksni` drives its own DBus task on the tokio
   runtime.
8. On the channel signal, the runner calls `driver.finalize_action()`; the
   `ksni` item is torn down by its RAII guard (see "Error Handling").
9. The runner returns `(Recording, InputCapability, CaptureRegion)`.
10. `rollshot-app` opens the Action Guide Timeline Workspace as it does for
    region recordings.

### Tray Icon

- The icon is created when recording starts and destroyed when the session ends
  (success, error, or cancellation).
- Activate (left-click / default activate, surfaced by `ksni` as the
  `activate` callback) is the only P0 interaction and means "finish recording".
- The tooltip shows "Rollshot is recording — click to finish". The elapsed
  recording time is updated via a tokio interval that calls `Handle::update`;
  if that adds complexity, the elapsed-time portion of the tooltip may be
  dropped for P0 (the static "click to finish" text is the requirement).

### OS Notification

- Sent once when recording starts.
- Uses `notify-rust` via the standard Freedesktop Desktop Notifications D-Bus
  service (`org.freedesktop.Notifications`).
- Set as **transient** (so it does not persist in notification history) with a
  short timeout, matching Spectacle's recording notification. Verify the exact
  `notify_rust::Hint` variant against the pinned crate version, as the hint API
  has shifted across releases.
- If the notification cannot be sent, the runner logs a warning and continues;
  the tray icon remains the authoritative stop path.

### Environment Without Tray Support

If no SNI host is registered (no `StatusNotifierWatcher` /
`IsStatusNotifierHostRegistered == false`), the runner returns an error such as:

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
| `rollshot-iced-overlay` | Add `run_action_guide_fullscreen(config, input_source)`; acquire fullscreen stream, manage the SNI tray item (behind a `RecordingTray` trait seam for testability), run action recording, return action result. Add `acquire_fullscreen_driver()` shared with the scrolling path. |
| `rollshot-app` | Extend `LaunchMode::ActionGuide` with `fullscreen`; wire the new runner; keep Timeline Workspace handoff. |
| `rollshot-cli` | Add `--fullscreen` to `ActionGuideArgs` and forward it to `rollshot-app --action-guide --fullscreen`. |

### New Dependencies

- `ksni`: pure-Rust StatusNotifierItem (SNI) implementation. This is KDE
  Plasma's native tray protocol (`org.kde.StatusNotifierItem`) and the direct
  analog of `KStatusNotifierItem`, which Spectacle uses for the same
  recording-stop UX. Runs as a background async task on the existing tokio
  runtime; the click event arrives over a channel.
- `notify-rust`: Freedesktop notifications (`org.freedesktop.Notifications`).

> **Decision: `ksni`, not `tray-icon`.** An earlier draft proposed the
> `tray-icon` crate. `tray-icon`'s Linux backend is GTK-based
> (`libappindicator` + a GTK main loop that must be pumped on the thread that
> creates the icon). That introduces a **second native event loop** competing
> with iced / winit / `iced_layershell` for the main thread, plus GTK/libxdo
> runtime dependencies this app does not otherwise carry. `ksni` speaks the SNI
> DBus protocol directly with no GTK, runs on the tokio runtime we already have,
> and delivers the click over a channel — which is exactly what the headless
> runner below needs (see "Runner Architecture"). KDE's Spectacle implements
> this same feature via `KStatusNotifierItem` (the C++ SNI), confirming SNI as
> the correct platform path.
>
> **Tradeoff (accepted):** `ksni` is Linux/freedesktop-only; `tray-icon` is
> cross-platform. macOS support is out of scope for this iteration and the
> overlay runner is already platform-split (`linux_runner.rs` vs
> `macos_capture.rs`), so a future macOS tray would be a separate backend
> (`NSStatusItem`) regardless of crate choice. Choosing `ksni` therefore costs
> nothing for the macOS path while removing the event-loop risk on Linux.

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
  │   (headless — no layer-shell overlay, no iced Application)
  │
  ├── SNI host registered? → No → Err("…requires a system tray")
  │
  ├── ksni: register item, on-activate → finish_tx.send(())   [TrayGuard owns handle]
  ├── send OS notification (best-effort, transient)
  ├── driver = acquire fullscreen Driver (RegionMode::FullSource)
  ├── region  = CaptureRegion { x:0, y:0, w/h from driver source size }
  ├── capability = driver.begin_action_recording(region, input_source)
  ├── finish_rx.recv()                      ← blocks on tray activate
  ├── (recording, capability) = driver.finalize_action()?
  ├── TrayGuard drops → item torn down
  └── Ok(Some((recording, capability, region)))
  │
  ▼
timeline_workspace::run(recording, region, capability, source_kind)
```

### Runner Architecture

```text
                 ┌──────────────────────────────────────────┐
                 │ run_action_guide_fullscreen  (main thread)│
                 │  - owns Driver (local var)                │
                 │  - blocks on finish_rx.recv()             │
                 └───────┬───────────────────────┬───────────┘
            begin_action_recording          finish_tx.send(())
                         │                       ▲
            ┌────────────▼─────────┐   ┌──────────┴──────────┐
            │ action consumer      │   │ ksni DBus task      │
            │ thread (std::thread) │   │ (tokio runtime)     │
            │  crop + poll_input   │   │  StatusNotifierItem │
            └────────────┬─────────┘   └─────────────────────┘
                         │ reads Shared.latest
            ┌────────────▼─────────┐
            │ capture reader thread│
            │ (PipeWire/scap)      │
            └──────────────────────┘
```

This runner is **headless** and does **not** reuse the global
`ACTION_INPUT_SLOT` / `DRIVER_SLOT` / `ACTION_RESULT_SLOT` statics. Those
statics exist only to bridge the iced overlay's borrow model (recording is
started/finished from inside iced `update()` messages, so the driver must be
parked in a static). The fullscreen runner has no iced loop, so it owns the
`Driver` as a plain local value and threads the result back by normal return —
simpler and with no shared mutable state.

Because `ksni` runs on the tokio runtime and delivers the click over a channel,
there is **no second native event loop** (the reason `tray-icon`/GTK was
rejected). The main thread simply blocks on `finish_rx.recv()` while the capture
reader thread, action consumer thread, and ksni DBus task run concurrently.

**DRY note:** the existing scrolling path acquires its streaming `Driver` inside
`acquire_scrolling_resource()` (one-shot probe to establish display bounds, then
`Driver::new(..)` with `RegionMode::FullSource`). The fullscreen action-guide
runner needs the same acquisition. Factor the shared part into a helper (e.g.
`acquire_fullscreen_driver(&OverlayConfig)`) used by both, rather than copying
the probe/construct sequence.

**Runtime-verification risk (resolve with a short spike before/at Task start):**
today the `Driver` lifecycle is only ever driven from within the iced event
loop (the overlay's `run()`), even though its reader/consumer threads are
independent. Confirm that `Driver::new` + `begin_action_recording` +
`finalize_action` work correctly when invoked from a plain thread with **no**
iced/winit/GTK loop running — in particular that PipeWire/portal stream
negotiation (ashpd) does not assume an ambient glib/main loop. If it does, the
acquisition step must `block_on` its own executor. This is the one feasibility
unknown in the design; everything else is mechanical.

### Capture Region

Fullscreen Action Guide records the entire selected display. The
`CaptureRegion` passed to the recorder is the display's logical region
(`x = 0, y = 0`, width/height from the streaming driver's source size). The
`ActionRecorder` receives uncropped full-display frames.

**"Fullscreen" is not "no interaction" on portal backends.** Display selection
follows whatever the acquired backend does for scrolling capture: a
KWin/direct backend can use the configured/primary output, but a Portal
(`xdg-desktop-portal` / PipeWire) backend will still present the compositor's
screen-picker so the user chooses which display to share. That picker is the
source of the "Portal picker dismissed → `Ok(None)`" row in the error table.
This is unavoidable on Wayland portals and matches the existing scrolling flow;
the spec does not attempt to bypass it.

### Separation from Existing Fullscreen Path

The existing `rollshot_iced_overlay::fullscreen::capture` handles one-shot
screenshots. Action Guide fullscreen is a separate path because it requires a
streaming resource and action recording. Both paths bypass the overlay, but they
do not share implementation.

## Error Handling

| Scenario | Behavior |
|----------|----------|
| No SNI host registered | Return `OverlayError::Capture` immediately; no resource acquired. |
| Fullscreen stream acquisition fails | Return `OverlayError::Capture`; destroy tray icon. |
| Portal picker dismissed | Return `Ok(None)`; destroy tray icon; no Timeline Workspace. |
| Recording stream ends unexpectedly | Return `OverlayError::Capture`; destroy tray icon. |
| `finalize_action` fails | Return error; destroy tray icon. |
| Notification fails | Log warning; continue recording. |

The tray item is destroyed in all exit paths, success or failure. A `TrayGuard`
RAII wrapper owns the `ksni` handle and, on `Drop`, shuts the item down (and
aborts/joins the spawned DBus task). Because `ksni` returns a handle rather than
requiring a live event loop, the guard is the only cleanup mechanism needed — no
explicit `destroy()` call has to be threaded through every `?` early-return.

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

CI has no SNI host and no live DBus session, so the tray and notification must
sit behind a seam. Define a `RecordingTray` trait (e.g. `fn show() -> Result<Self>`,
the activate `Receiver`, and `Drop` for teardown) with a real `ksni` impl and a
`FakeTray` that lets tests drive the activate signal and assert teardown. The
runner is written against the trait so these tests run on hosted CI.

| Test | Purpose |
|------|---------|
| Tray lifecycle: show → activate → drop | Validate that the activate signal stops recording and that the item is always torn down. |
| Tray-host-absent returns error before capture | Validate the hard requirement: no `Driver` acquired when `show()` fails. |
| Stream acquisition failure drops tray | Validate the `TrayGuard` cleans up on the error path. |
| Notification failure does not abort recording | Validate best-effort notification behavior. |

### Manual Tests

- Run `rollshot-app --action-guide --fullscreen` on a real KDE environment.
- Verify the tray icon appears, the notification is shown, clicking the icon
  stops recording, and the Timeline Workspace opens with detected steps.
- Verify that region Action Guide (`--action-guide` without `--fullscreen`) is
  unchanged.

## Future Work

- macOS support through the same runner shape. Since `ksni` is
  Linux/freedesktop-only, macOS would use a separate tray backend
  (`NSStatusItem`, e.g. via `objc2` in the existing `macos-*` unsafe-isolation
  pattern, or `tray-icon` if cross-platform reuse is wanted there). The runner
  is already platform-split, so this is an additive backend, not a rework.
- Global hotkey integration, ideally through KDE `KGlobalAccel` D-Bus for proper
  KDE Wayland support or a cross-platform crate if Wayland limitations are
  acceptable.
- Right-click tray menu with explicit Cancel.
- Pause/resume recording.
