# Rollshot App Tauri Deprecation Design

## Context

`rollshot-app` is becoming the iced product app for interactive capture. The
retained Tauri app is still useful as a short-term reference and fallback, but
it should no longer shape the active product launch path.

Today `rollshot-app` still carries Tauri-oriented overlay selection:

- macOS `overlay_mode = auto` resolves to the retained Tauri overlay.
- CLI interactive capture looks for `rollshot-tauri-app` by default.
- macOS ScreenCaptureKit is hidden behind the `macos-sck` feature even though
  macOS has no alternate real backend.

This creates unnecessary launch complexity while iced/macOS development is now
the active path.

## Goals

- Remove Tauri fallback and Tauri overlay-selection behavior from
  `crates/rollshot-app`.
- Make `rollshot-cli capture` launch `rollshot-app` by default for interactive
  capture.
- Keep `crates/rollshot-tauri-app` in the workspace, but mark it deprecated and
  no longer the product path.
- Make the macOS ScreenCaptureKit backend platform-default instead of requiring
  a `macos-sck` feature.
- Keep changes surgical; do not delete the Tauri crate yet.

## Non-Goals

- Do not remove `crates/rollshot-tauri-app` from the workspace in this change.
- Do not rewrite the Tauri app internals.
- Do not change stitching behavior, crop behavior, save behavior, or overlay
  visuals.
- Do not remove the `ROLLSHOT_APP` override.

## Design

### `rollshot-app` launch path

`rollshot-app` should run the iced overlay directly for capture launch. The
`OverlayRunner::Tauri` abstraction and platform fallback logic are removed from
the app.

`InteractiveLaunchOptions.overlay_mode` can remain in shared launch JSON for
compatibility during this migration, but `rollshot-app` should ignore it. That
keeps this change focused and avoids forcing unrelated CLI/Tauri JSON churn.

The save-dialog helper mode stays. It is not Tauri code; it is a helper process
used so the native save dialog does not share state with the completed iced/winit
event loop.

### CLI interactive app resolution

`rollshot-cli` keeps `ROLLSHOT_APP` as an explicit binary override. Without the
override, it should look for `rollshot-app` next to `rollshot`.

Development error hints should point to `cargo build -p rollshot-app`, not the
old pnpm/Tauri build flow. Tests that assert default binary names and hints
should be updated with the new product app name.

### Tauri crate status

`crates/rollshot-tauri-app` remains buildable and in the workspace for now. It
should be documented as deprecated legacy/reference code, not as the active
interactive app.

The README should describe `rollshot-app` as the interactive capture app and
`rollshot-tauri-app` as deprecated. Existing historical docs under
`docs/superpowers/` should not be edited.

### macOS ScreenCaptureKit default

On macOS, `rollshot-capture` should always include the ScreenCaptureKit backend.
The `scap` dependency should be target-specific for macOS, not optional behind a
Cargo feature.

Code currently gated by `all(target_os = "macos", feature = "macos-sck")`
should become `target_os = "macos"`. The default backend for macOS should be
`BackendKind::MacosScreenCaptureKit`.

Feature forwarding from `rollshot-cli`, `rollshot-app`, and
`rollshot-iced-overlay` should be removed when it only exists to enable
`rollshot-capture/macos-sck`.

## Testing

Use TDD for behavior changes:

- Update CLI launcher tests first so they expect `rollshot-app` and the new
  build hint.
- Update `rollshot-app` overlay-selection tests by removing the Tauri fallback
  expectation or deleting the now-obsolete module tests.
- Update capture backend tests so macOS default backend no longer depends on a
  feature flag.

Verification commands:

- `rtk cargo test -p rollshot-app`
- `rtk cargo test -p rollshot-cli`
- `rtk cargo test -p rollshot-capture`
- `rtk cargo fmt --check`

If the change expands beyond the expected files, run the broader workspace test
or clippy command before completion.

## Runtime Risk

This change can verify compile-time wiring and unit behavior locally. Real macOS
capture still needs manual runtime smoke testing because Screen Recording
permission, ScreenCaptureKit session behavior, iced window focus, and mouse
passthrough require an interactive macOS desktop.
