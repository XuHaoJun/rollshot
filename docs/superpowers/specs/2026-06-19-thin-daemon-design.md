# Thin Daemon Design

Date: 2026-06-19

## Summary

`rollshot-app` will support a long-running `daemon` mode in the same binary as
the existing one-shot capture modes. The daemon owns a system tray item and one
configurable global shortcut. Triggering either starts a separate
`rollshot-app capture` child process for a region screenshot. The capture
overlay already lets the user switch the selected region from Screenshot to
Scrolling mode, so the daemon does not need separate screenshot and scrolling
shortcuts.

The complete design covers Linux and macOS. The first implementation plan will
deliver and validate Linux on KDE Plasma 6 only. The Linux platform boundary
must permit later desktop-specific adapters without claiming support for them.

## Goals

- Add `rollshot-app daemon` without changing existing one-shot invocation
  behavior.
- Provide a persistent tray menu with:
  - `Capture Region`
  - `Quit Rollshot`
- Register one configurable global shortcut for region capture.
- Default to `Alt+Shift+6` on Linux and `Command+Shift+6` on macOS.
- Start capture in a separate process of the same executable.
- Permit at most one daemon and one active capture child.
- Keep the daemon usable through the tray if global shortcut registration
  fails.
- Define Linux and macOS platform adapters while implementing Linux KDE 6
  first.

## Non-goals

- Autostart or launch-at-login management.
- A settings window or tray-based hotkey editor.
- Live configuration reload.
- Multiple shortcut actions or arbitrary command execution.
- Multiple simultaneous capture sessions.
- X11, GNOME, or other Linux desktop support in the first implementation.
- Moving the existing capture UI into the daemon process.
- Changing the existing capture toolbar or workflow-switching behavior.

## User-facing behavior

### CLI

The same binary provides both modes:

```text
rollshot-app daemon
rollshot-app capture [existing capture options]
```

Running `rollshot-app` without a subcommand retains its current meaning:
one-shot scrolling region capture with the existing defaults. It does not start
the daemon.

The daemon always launches this logical capture request:

```text
rollshot-app capture --workflow screenshot --scope region
```

It resolves the executable with `std::env::current_exe()` rather than searching
`PATH`, ensuring the child uses the same installed Rollshot binary.

### Region capture workflow

The shortcut opens region selection in Screenshot mode. After selecting a crop,
the existing overlay toolbar allows switching to Scrolling mode. Shortcut
configuration therefore selects the entry point, not the final capture
workflow.

### Tray

The tray menu exposes exactly two product actions:

- `Capture Region`: behaves like the global shortcut.
- `Quit Rollshot`: terminates an active capture child, tears down platform
  integrations, and exits the daemon.

Tray icon primary-click behavior is not part of the cross-platform contract;
the menu actions are the supported interface.

## Configuration

The daemon reads `rollshot/config.toml` under the platform configuration
directory returned by the existing `dirs` dependency:

- Linux default: `$XDG_CONFIG_HOME/rollshot/config.toml`, falling back to
  `~/.config/rollshot/config.toml` through the platform API.
- macOS default: the Rollshot configuration file under the user's Application
  Support configuration directory.

The first version supports one setting:

```toml
[daemon]
capture_region_hotkey = "Alt+Shift+6"
```

Platform defaults are:

- Linux: `Alt+Shift+6`, translated for the portal preferred trigger to
  `ALT+SHIFT+6`.
- macOS: `Command+Shift+6`.

If the file is absent, the daemon uses the platform default without warning. If
the file cannot be read, TOML is invalid, or the hotkey is invalid, the daemon
logs a warning and falls back to the platform default. Configuration is loaded
once at startup; changes require restarting the daemon.

The configuration model owns a platform-neutral shortcut representation. Each
platform adapter is responsible for parsing or translating it into its native
registration format.

## Architecture

### Launch dispatch

`LaunchCommand` and `LaunchMode` gain a `Daemon` variant. The existing
`main -> resolve_launch_mode -> run` dispatch remains the top-level boundary.
Capture and daemon initialization remain mutually exclusive within a process.

### Shared daemon core

The shared daemon core owns:

- loaded daemon configuration;
- the single-instance guard;
- daemon state;
- the active capture child, if any;
- event handling for `CaptureRegion`, `CaptureExited`, and `Quit`;
- platform-independent cleanup ordering.

The core does not depend on SNI, D-Bus, AppKit, iced, or a concrete global
shortcut library. Platform adapters send semantic events into the core.

The core-facing platform contract is conceptually:

```rust
enum DaemonEvent {
    CaptureRegion,
    CaptureExited(CaptureExit),
    Quit,
}

trait DaemonPlatform {
    fn start(event_sender: Sender<DaemonEvent>, config: &DaemonConfig)
        -> Result<Self, PlatformError>;
}
```

The exact Rust shape may differ to match event-loop ownership, but the
dependency direction must remain: adapters produce semantic events; the core
owns product behavior.

### Child process boundary

The daemon process never starts an iced capture runner itself. It spawns a new
instance of the same binary. This preserves the existing Linux layer-shell and
macOS iced event-loop ownership and isolates capture failures from the tray
daemon.

The child is placed in a daemon-owned process group or equivalent platform
termination boundary. On daemon quit:

1. request termination of the active capture process group;
2. wait up to two seconds;
3. force termination if it is still running;
4. tear down shortcut and tray resources;
5. release the single-instance guard and exit.

Normal cancellation or non-zero capture exit never terminates the daemon.

### Single instance

Daemon startup acquires an OS-backed exclusive file lock at
`<platform-config-dir>/rollshot/daemon.lock`, beside `config.toml`. The lock
handle remains owned for the daemon lifetime. The operating system releases the
lock after a crash, so correctness does not depend on deleting the persistent
lock file.

If another daemon already owns the lock, the new invocation logs an information
event and exits successfully without creating a tray or registering a shortcut.
One-shot capture processes do not acquire this lock.

## State machine

```text
Starting ──ready──> Idle ──capture──> Capturing
    │                 │                  │
    │                 │                  └──child exited──> Idle
    │                 │
    └──fatal────────> Exiting <──────────┴──quit
```

Rules:

- Startup creates the tray before declaring the daemon ready.
- Shortcut setup follows tray setup. Shortcut failure degrades to tray-only.
- `CaptureRegion` in `Idle` starts one child and enters `Capturing`.
- `CaptureRegion` in `Capturing` is ignored and logged at debug level.
- Child spawn failure logs an error and returns immediately to `Idle`.
- Child success, cancellation, or non-zero exit returns to `Idle`.
- `Quit` from any running state enters `Exiting`.
- Platform resources are owned by guards and are torn down on every exit path.

## Linux KDE Plasma 6 adapter

Linux support in the first implementation is explicitly KDE Plasma 6 on a
Wayland session.

### Tray

The adapter uses `ksni` and the StatusNotifierItem protocol, matching the
existing Action Guide tray implementation. It checks for a registered KDE or
freedesktop StatusNotifierHost before completing startup. Unlike the temporary
Action Guide tray, this tray exposes the daemon's two persistent menu actions.

If no SNI host is available or tray registration fails, daemon startup fails.
The first implementation does not provide a hidden trayless daemon.

The reusable SNI host detection and lifecycle behavior should be moved to or
shared through an app-appropriate boundary rather than making the daemon depend
on the Action Guide overlay's private temporary tray type.

### Global shortcut

The adapter uses the XDG Desktop Portal GlobalShortcuts interface through the
workspace `ashpd` dependency, whose current 0.9 API exposes session creation,
shortcut binding, and activation signals:

1. create a shortcut session;
2. bind one logical shortcut with ID `capture-region`;
3. use `ALT+SHIFT+6` as the default `preferred_trigger`;
4. accept the shortcut selected or approved by KDE;
5. convert the portal `Activated` signal into `CaptureRegion`.

The portal may display configuration UI and may return a binding different from
the preferred trigger. The daemon treats the portal result as authoritative.
If the portal is unavailable, the user rejects binding, or no shortcut is
bound, the daemon logs a warning and continues in tray-only mode. It does not
retry in a loop or implement an X11 fallback in the first version.

The Linux adapter boundary must allow later alternatives such as another
portal-backed desktop adapter or an X11-specific adapter without changing the
daemon core.

## macOS adapter

macOS is part of the complete design but is deferred from the first
implementation plan.

The adapter will use:

- `tray-icon` for the status item and menu;
- `global-hotkey` for `Command+Shift+6`;
- the native main-thread event loop required by those crates.

It emits the same `CaptureRegion` and `Quit` semantic events as Linux and uses
the same shared daemon core and child-process contract. macOS capture remains a
separate `rollshot-app capture` process, preserving the existing iced product
event loop.

The macOS implementation must satisfy the same single-instance, busy-trigger,
configuration fallback, and quit-termination behavior before macOS daemon
support is considered complete.

## Error handling and diagnostics

- Tray initialization failure is fatal.
- Global shortcut initialization or binding failure is non-fatal and results
  in tray-only operation.
- Capture spawn failure is non-fatal.
- Capture non-zero exit is non-fatal.
- Cleanup errors are logged while cleanup continues.
- Runtime diagnostics use stable explicit targets under
  `rollshot::daemon::*`.
- Diagnostics never log raw key events. They may log the configured shortcut
  description and the portal's user-facing binding description.
- Intentional pre-subscriber startup failures may use user-facing stderr;
  normal daemon diagnostics use `tracing`.

## Testing and verification

### Automated tests

- CLI parsing:
  - `daemon` resolves to daemon mode;
  - existing `capture` options remain unchanged;
  - no-subcommand behavior remains unchanged.
- Configuration:
  - absent file uses the platform default;
  - valid custom hotkey is loaded;
  - unreadable, malformed, and invalid values fall back to the default.
- State machine:
  - idle trigger starts one capture;
  - triggers while capturing are ignored;
  - child exit returns to idle;
  - spawn failure returns to idle;
  - quit from idle cleans up;
  - quit while capturing terminates the child before cleanup.
- Single instance:
  - first guard succeeds;
  - second guard reports an existing daemon;
  - dropping the first guard permits reacquisition.
- Platform orchestration with fakes:
  - tray events route to semantic daemon events;
  - shortcut failure preserves tray operation;
  - tray failure aborts startup;
  - resources are dropped on all exit paths.
- Fake child process:
  - success, non-zero exit, spawn failure, graceful termination, and forced
    termination paths.

### Linux KDE 6 manual verification

- Start `rollshot-app daemon` on KDE Plasma 6 Wayland.
- Confirm the SNI tray item and its two menu actions.
- Approve or configure the portal shortcut.
- Trigger region capture with the actual portal binding.
- Select a crop, switch from Screenshot to Scrolling in the toolbar, and
  complete the capture.
- Trigger again after completion.
- Trigger repeatedly during capture and verify no second capture appears.
- Start a second daemon and verify it exits successfully without another tray.
- Deny or remove the shortcut and verify tray-only operation.
- Quit while idle.
- Quit while capture is active and verify the capture process group exits.

Required repository checks for the implementation are:

```text
rtk cargo test
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

### Deferred macOS verification

The later macOS implementation must manually verify status-item behavior,
shortcut registration and conflict handling, main-thread event-loop ownership,
capture child termination, and compatibility with the existing macOS capture
and result-workspace flow.

## Delivery sequence

The complete feature is delivered in platform slices:

1. Shared daemon core, CLI, configuration, single-instance guard, and child
   management.
2. Linux KDE Plasma 6 SNI tray and XDG GlobalShortcuts portal adapter.
3. Linux KDE 6 automated and manual verification.
4. Deferred follow-up: macOS `tray-icon` and `global-hotkey` adapter.
5. Deferred follow-up: other Linux desktop adapters based on demonstrated
   demand and platform-specific validation.

The first implementation plan covers steps 1–3 only.
