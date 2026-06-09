# Post-Capture Viewer Review Fixes Design

## Scope

Fix three verified gaps in `feat/post-capture-image-viewer`:

1. Place the macOS floating thumbnail at the lower-right of the active display,
   including secondary displays.
2. Downscale oversized captures before building the floating-thumbnail GPU image
   handle while retaining the full-resolution `ResultDocument`.
3. On Linux, reveal saved files through
   `org.freedesktop.FileManager1.ShowItems` when available, falling back to
   `xdg-open <parent>` when D-Bus reveal is unavailable.

No other review observations or adjacent behavior are changed.

## Design

### macOS Thumbnail Position

Keep AppKit screen discovery in `macos_native_drag.rs`, but move coordinate
conversion into a portable pure helper. The helper receives the active
`ScreenFrame`, the main display height, thumbnail size, and margin. It returns
the winit top-left position using the active screen's AppKit frame and the main
display height used by winit's macOS Y conversion.

The macOS bridge queries the active screen and main screen, then delegates to
the helper. Portable tests cover primary displays and secondary displays
positioned above, below, left, and right of the primary display.

### Thumbnail Display Handle

Reuse the Result Workspace's existing display-downscale policy and display
handle builder. Construct the thumbnail handle from a display copy constrained
to `DEFAULT_MAX_TEXTURE_DIM`; continue storing the original `RgbaImage` in
`ResultDocument`.

A product-phase test verifies that an oversized capture produces a bounded
thumbnail handle while the document retains its original dimensions.

### Linux Reveal

Add a Linux-only direct `zbus` dependency. `reveal` first calls
`org.freedesktop.FileManager1.ShowItems` on the session bus with the saved
file's `file://` URI and an empty startup ID. If connecting, creating the proxy,
or calling the method fails, it falls back to spawning `xdg-open` for the
containing directory.

Keep the public `reveal(path) -> Result<(), String>` contract. It reports an
error only when both D-Bus reveal and fallback spawning fail.

Testing isolates the reveal decision behind a small injected-operation helper:
tests verify that D-Bus success skips fallback, and D-Bus failure invokes the
fallback. The real D-Bus and process operations remain thin platform adapters.

## Verification

- Focused TDD tests for each pure/helper behavior.
- `rtk cargo test -p rollshot-app`
- `rtk cargo test --workspace`
- `rtk cargo fmt --check`
- `rtk cargo clippy --workspace --all-targets -- -D warnings`
- `rtk git diff --check`

macOS runtime verification remains required for actual multi-display placement,
native thumbnail rendering, and Finder reveal behavior.
