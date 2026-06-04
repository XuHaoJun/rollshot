# macOS Iced Overlay App Foundation Design

Date: 2026-06-03

## Context

Rollshot currently has two capture UI paths:

- Linux uses the native iced layer-shell overlay in `crates/rollshot-overlay`.
- macOS and other non-Linux paths use the Tauri/React webview overlay in
  `crates/rollshot-app`.

The project has already evaluated unifying the GUI on iced in
`docs/iced-migration-evaluation.md`. That document is the starting point for this
design, but this design adds two important constraints:

- The macOS iced overlay is real product implementation work, not a throwaway
  spike.
- The current Tauri overlay must remain available for a validation period, even
  after the macOS iced overlay exists, because iced window behavior still needs
  runtime confidence on macOS.

The goal is to set the crate ownership and migration path before adding more UI
code, so the future iced app, macOS overlay, image preview/editor, and settings
pages do not land under temporary or misleading names.

## Goals

- Rename the current Tauri app path so it is clearly the retained Tauri
  reference/fallback app.
- Reserve `rollshot-app` for the new iced-only product app.
- Rename the current native overlay crate so it is clearly the iced overlay
  renderer, not all overlay concepts.
- Add a macOS iced overlay path that can reach parity with the existing Tauri
  overlay while Tauri remains available.
- Keep future app surfaces such as image preview, editor, settings, tray, and
  hotkeys owned by the new iced `rollshot-app`.
- Avoid creating empty speculative crates.

## Non-Goals

- Do not delete the Tauri/React overlay in this design.
- Do not make the macOS iced overlay the default immediately.
- Do not implement the image editor, settings pages, tray, or global hotkeys as
  part of the overlay foundation.
- Do not create an empty shared iced UI crate before real shared components
  exist.
- Do not change stitching or capture backend behavior except where needed to
  connect the iced overlay flow.

## Chosen Approach

Use a foundation-first migration:

1. Rename the existing Tauri app package/binary/path to `rollshot-tauri-app`.
2. Create the new iced-only product app as `rollshot-app`.
3. Rename `rollshot-overlay` to `rollshot-iced-overlay`.
4. Refactor the iced overlay so shared state, messages, update logic, and view
   are runner-agnostic.
5. Keep Linux on its existing `iced_layershell` runner.
6. Add a macOS runner using a normal iced/winit transparent always-on-top window
   plus a small AppKit patch.
7. Keep the current Tauri overlay available as the behavior reference and
   fallback until the iced macOS path has enough runtime validation.

This costs more early Cargo/package churn than building under temporary names,
but it aligns names with the intended end state before more code depends on the
old structure.

## Target Crate Ownership

### `rollshot-core` and `rollshot-capture`

These remain the framework-independent foundations. `rollshot-core` owns
stitching. `rollshot-capture` owns platform capture backends, including
ScreenCaptureKit on macOS and portal/PipeWire on Linux.

### `rollshot-overlay-core`

This remains framework-neutral overlay logic:

- live preview viewport generation
- capture-miss recovery state
- crop visual tokens

It must not depend on iced, Tauri, React, or platform window APIs.

### `rollshot-iced-overlay`

This is the iced overlay renderer crate. It owns:

- shared iced overlay state
- shared overlay `Message`
- shared update logic
- shared view/widgets/canvas rendering
- live preview rendering using iced image handles
- runner-specific entry points

Platform runners:

- Linux uses `iced_layershell` with layer-shell input regions.
- macOS uses a normal iced/winit transparent, borderless, always-on-top window,
  with AppKit patching for macOS-only window behavior.
- Windows is a future topmost-window runner and is not implemented by this
  design.

The crate name intentionally includes `iced` because the Tauri overlay remains
present during validation. This avoids ambiguity between "the old overlay" and
"the iced overlay".

### `rollshot-app`

This is the new iced-only product app. It owns product-level desktop UI:

- capture host flow
- final image preview
- save handoff
- future image editor
- future settings pages
- future tray and hotkey integration

The image preview/editor and settings pages should start as modules inside
`rollshot-app`. They are app surfaces, not overlay renderer code.

### `rollshot-tauri-app`

This is the renamed current Tauri/React app. It remains available as:

- the reference implementation for current macOS overlay behavior
- a fallback while the macOS iced overlay is validated
- a source of parity tests and expected behavior

The name is factual instead of `legacy` because the app remains intentionally
supported during the validation period. It can be called legacy in docs later
when it becomes delete-only.

### Reserved: `rollshot-iced-ui`

Reserve the name `rollshot-iced-ui`, but do not create the crate in the
foundation phase.

Create it only when there are real, non-trivial iced components shared by at
least two surfaces, for example between `rollshot-app` and
`rollshot-iced-overlay`. Good candidates would be toolbar primitives, crop
handles, reusable image canvas widgets, or token-to-iced style adapters.

Until that pressure exists, keeping editor/settings/preview code in
`rollshot-app` is simpler and clearer.

## Capture And Overlay Flow

### New iced path

The new iced product path is:

```text
rollshot-app
  -> rollshot-iced-overlay macOS runner
    -> rollshot-capture ScreenCaptureKit stream
      -> shared live driver / stitcher flow
        -> iced live preview and capture-miss events
  -> CaptureResult returned to rollshot-app
  -> final preview / save flow in iced
```

The macOS runner should reuse the existing Linux iced driver pattern where
practical: frame stream in, crop/stitch in Rust, live preview and capture-miss
events pushed into the iced update loop. This replaces the Tauri webview path's
polling, DTOs, and PNG-over-IPC transport for the iced path.

The final `CaptureResult` returns to `rollshot-app`, which owns final preview
and save behavior. Future editor entry points should open from this result in
`rollshot-app`, not from `rollshot-iced-overlay`.

### Retained Tauri path

`rollshot-tauri-app` keeps the current React/webview overlay flow:

- Tauri commands
- `SharedSession`
- polling `session_status`
- PNG preview IPC
- current final preview and save behavior

This path remains a parity reference and fallback. It is not deleted by this
design.

## macOS Window Behavior

The macOS iced overlay runner needs to cover the same behavior currently handled
by the Tauri overlay and described in the iced migration evaluation:

- transparent window
- no decorations
- always-on-top level
- primary-display coverage
- no shadow
- collection behavior suitable for Spaces/fullscreen interaction
- mouse passthrough while scrolling the target
- usable overlay controls while passthrough is active

Native iced/winit APIs cover the basic window shape and topmost behavior. macOS
specific behavior such as collection behavior and disabling shadow should live
behind a small macOS-only AppKit helper. That helper belongs in
`rollshot-iced-overlay`, not in `rollshot-overlay-core`.

Whole-window mouse passthrough is a design constraint on macOS. If the overlay
needs controls to stay clickable while the rest of the screen passes input
through, the design should use a second always-interactive toolbar window or an
equivalent macOS-specific runner strategy. This is runner behavior, not shared
overlay-core logic.

## Coexistence And Defaults

During the validation period:

- `rollshot-tauri-app` remains available.
- macOS iced overlay is opt-in.
- Tauri remains the default macOS overlay until a later decision flips the
  default.
- Deleting Tauri is a separate future phase after parity and runtime confidence.

The selector should be explicit rather than implied by platform alone. The
implementation plan can choose the exact surface, such as a launch option field
or CLI flag, but the behavior must be clear:

```text
macOS default: tauri overlay
macOS opt-in: iced overlay
Linux default: iced layer-shell overlay
```

## Parity Checklist

The macOS iced overlay must be checked against the current Tauri overlay for:

- capture launch options: backend, fps, show cursor
- primary display coverage
- logical overlay crop to capture frame coordinate mapping
- crop selection and confirmation
- clearing overlay chrome before stitching starts
- input passthrough during scrolling
- keeping required overlay controls usable
- live stitch preview rendering
- preview placement that avoids the selected region
- capture-miss warning and recovery state
- Escape, stop, and cancel behavior
- final preview
- PNG save flow
- permission, capture, stitch, and save error reporting

This checklist is why `rollshot-tauri-app` remains present. It is the behavior
reference while the iced path matures.

## Verification

The implementation plan should include:

- Rust tests for pure crop mapping, preview constraints, driver state
  transitions, and final result handoff.
- Existing Tauri/React tests retained under `rollshot-tauri-app` while that path
  remains.
- Workspace build checks after crate/package renames.
- Linux native overlay regression checks after the shared iced overlay refactor.
- Manual macOS runtime checks for transparent/topmost window behavior, AppKit
  patching, input passthrough, multi-monitor behavior, ScreenCaptureKit
  permissions, live preview, final preview, and save.

Rust changes should use the normal project verification commands, with
`rtk cargo test` and `rtk cargo fmt --check` as the baseline. Frontend/Tauri
checks continue to apply to `rollshot-tauri-app` while it exists.

## Risks

### Early rename churn

Renaming the Tauri app and overlay crate first touches workspace metadata,
package names, paths, imports, scripts, and docs. This should be a contained
foundation phase with tests passing before behavior changes begin.

### iced/macOS window behavior

The macOS iced overlay depends on transparent topmost window behavior,
passthrough, and AppKit patching. Tauri remains available until these are proven
in runtime use.

### Shared overlay refactor can regress Linux

The Linux overlay already works. Moving its state/update/view into a
runner-agnostic structure must preserve Linux layer-shell behavior, including
input regions and live preview.

### Premature shared UI crate

Creating `rollshot-iced-ui` before real shared widgets exist would add an
unclear abstraction. The name is reserved, but extraction waits for real reuse.

## Open Decisions For The Implementation Plan

- Exact filesystem layout for `rollshot-tauri-app` and the new `rollshot-app`.
- Exact launch selector shape for macOS iced opt-in.
- Whether macOS overlay controls require a second toolbar window for
  passthrough.
- Which tests move with the Tauri rename and which new Rust tests are added in
  the foundation phase.
- The staged point at which `rollshot-app` invokes `rollshot-iced-overlay`
  directly versus first using a standalone harness.
