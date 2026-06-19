# macOS Action Guide Completion

## Goal

Make every advertised macOS Action Guide entry point usable: CLI and app
subcommands launch region recording, fullscreen recording has an in-app stop
control, and visual-only mode offers a direct Input Monitoring recovery action.

## Scope

- Fix `rollshot action-guide` to invoke the current Clap subcommand syntax.
- Route `rollshot-app action-guide` into the existing `MacosProduct` region
  recording flow.
- Extend the existing macOS capture component for fullscreen Action Guide:
  acquire a streaming capture, select the complete source, begin recording at
  boot, and make the fullscreen overlay mouse-transparent.
- Show a temporary macOS menu-bar item with Finish Recording and Cancel
  Recording actions for the fullscreen session.
- Add an `Open System Settings` action when Input Monitoring is unavailable.
- Keep all behavior behind the existing non-default `action-guide` feature.

## Architecture

```text
rollshot action-guide [--fullscreen]
                │
                ▼
rollshot-app action-guide [--fullscreen]
                │
        CaptureRequest::ActionGuide
                │
       ┌────────┴─────────┐
       │                  │
    Region            Fullscreen
       │                  │
 existing picker     preselect full source
       │                  │
       └──── macos_capture::Component ────┐
                                          ▼
                             Driver + MacosInputSource
                                          │
                                  Finish / Cancel
                                          ▼
                              TimelineWorkspace → Export
```

`MacosProduct` remains the only iced daemon. The temporary tray uses
`tray-icon`'s process-global menu callback and feeds events into the existing
iced subscription; it does not create another event loop or application.

## Behavior

### Region

The direct Action Guide subcommand opens the existing region overlay with
Action Guide selected. Selection, Start, Finish, Timeline, and export behave
the same as selecting the toolbar's Action Guide button from normal capture.

### Fullscreen

The selected display is the capture backend's full source. Recording starts
without a crop gesture. The fullscreen overlay becomes mouse-transparent and
renders no controls so the user can operate the application below it. A
temporary menu-bar item marked `● Rollshot` exposes Finish Recording and Cancel
Recording.

Finish stops input observation and capture, then opens Timeline. Cancel tears
down both resources and exits without a Timeline.

### Permission recovery

When capability is visual-only on macOS, recording and Timeline remain usable.
The advisory includes an `Open System Settings` button that opens Privacy &
Security → Input Monitoring. It does not request Accessibility.

## Failure handling

- Capture permission denial remains fatal to the capture session.
- Input Monitoring denial degrades to visual-only.
- Failure to open System Settings is logged and does not close Timeline.
- Fullscreen setup failure exits with the existing typed capture error.
- All capture/input resources stop on Finish, Cancel, fatal error, and Drop.

## Verification

- Unit tests for CLI argument forwarding and launch-mode routing.
- Component tests for fullscreen request acceptance, full-source region setup,
  immediate recording state, passthrough, Finish, and Cancel.
- Timeline update/view tests for the permission recovery action.
- macOS `cargo test`, `cargo clippy -D warnings`, and `cargo fmt --check` with
  `action-guide`.
- Real region and fullscreen smoke tests through the CLI.
