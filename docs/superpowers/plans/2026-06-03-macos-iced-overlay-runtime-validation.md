# macOS Iced Overlay Runtime Validation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the macOS iced overlay scaffold error with a runnable explicit opt-in path that starts ScreenCaptureKit capture, displays the shared iced crop overlay, begins stitched live preview after crop confirmation, and returns a `CaptureResult` to `rollshot-app`.

**Architecture:** Keep `rollshot-tauri-app` as the default macOS reference path. Move the Linux iced overlay's runner-agnostic UI state, update logic, and view code into shared `rollshot-iced-overlay::app`; keep Linux-specific layer-shell effects in `linux_runner`; add a normal iced/winit macOS runner that applies AppKit window patches through iced's raw window handle callback and reuses the shared capture driver.

**Tech Stack:** Rust workspace, `rollshot-app`, `rollshot-iced-overlay`, `rollshot-capture` with `macos-sck`, iced 0.14, iced_layershell 0.18 on Linux, ScreenCaptureKit through `scap`, `objc2`/`objc2-app-kit` for macOS window patching, `rollshot-overlay-core` for shared viewport/crop tokens.

---

## Scope

This plan implements the first product-useful macOS iced overlay runtime path behind the existing explicit selector:

```bash
rtk cargo run -p rollshot-app --features macos-sck -- --capture '{"backend":"macos-sck","fps":5,"show_cursor":false,"overlay_mode":"iced"}'
```

Success means this command no longer fails with:

```text
overlay error: macOS iced overlay runner is scaffolded but not wired to capture yet
```

and instead opens a transparent always-on-top iced overlay, lets the user select a crop, starts live scrolling stitch preview, and returns a `CaptureResult` when finalized.

## Non-Goals

- Do not make macOS iced the default for `overlay_mode:"auto"`.
- Do not delete or weaken the Tauri/webview macOS fallback.
- Do not implement image editor, settings, tray, or global hotkeys.
- Do not change Linux overlay behavior except through shared-code extraction that preserves the same observable flow.
- Do not change `rollshot-core` stitching algorithms.

## Current State

- `rollshot-app` already parses `overlay_mode` and routes explicit iced requests to `rollshot_iced_overlay::run_overlay(config)`.
- `rollshot-iced-overlay::macos_runner::run` currently returns a scaffold error.
- `rollshot-iced-overlay::driver` starts capture and stitching but is Linux-gated at the module level.
- Most iced overlay UI/update logic lives in `linux_runner.rs`, even though only the layer-shell input-region effect is Linux-specific.
- iced 0.14 provides:

```rust
iced::window::run(window_id, |window| { ... })
iced::window::enable_mouse_passthrough(window_id)
iced::window::disable_mouse_passthrough(window_id)
```

where the `window` callback exposes raw window/display handles.

## File Structure

Files to modify:

- `crates/rollshot-iced-overlay/Cargo.toml`
- `crates/rollshot-iced-overlay/src/lib.rs`
- `crates/rollshot-iced-overlay/src/app.rs`
- `crates/rollshot-iced-overlay/src/driver.rs`
- `crates/rollshot-iced-overlay/src/linux_runner.rs`
- `crates/rollshot-iced-overlay/src/macos_runner.rs`
- `crates/rollshot-iced-overlay/src/macos_window.rs`
- `crates/rollshot-app/Cargo.toml`
- `crates/rollshot-app/src/main.rs`

Files to add only if the extracted app module becomes too large:

- `crates/rollshot-iced-overlay/src/app/view.rs`
- `crates/rollshot-iced-overlay/src/app/update.rs`

Keep the first pass in `src/app.rs`. Split only if the file becomes hard to review after moving the existing Linux code.

---

## Task 1: Expose The Capture Driver To macOS

**Files:**
- `crates/rollshot-iced-overlay/Cargo.toml`
- `crates/rollshot-iced-overlay/src/lib.rs`
- `crates/rollshot-iced-overlay/src/driver.rs`

- [ ] Add a `macos-sck` feature to `rollshot-iced-overlay` so the crate can be checked directly on macOS without relying only on workspace feature unification through `rollshot-app`.

In `crates/rollshot-iced-overlay/Cargo.toml`:

```toml
[features]
default = []
macos-sck = ["rollshot-capture/macos-sck"]
```

- [ ] Gate `coords` and `driver` for both Linux and macOS.

In `src/lib.rs`, change:

```rust
#[cfg(target_os = "linux")]
mod coords;
#[cfg(target_os = "linux")]
mod driver;
```

to:

```rust
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod coords;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod driver;
```

- [ ] Update the `SendStream` safety comment in `driver.rs` so it describes the actual invariant across Linux and macOS: the frame stream is moved exactly once into one reader thread, never accessed from the creating thread afterward, and all cancellation happens through `AtomicBool` plus thread join boundaries.

- [ ] Add these accessors to `Driver` so `macos_runner` does not need to duplicate capture startup state:

```rust
impl Driver {
    pub(crate) fn source_size(&self) -> Size<u32> {
        self.source_size
    }

    pub(crate) fn overlay_size(&self) -> Size<f32> {
        self.overlay_logical
    }
}
```

- [ ] Run platform-local checks:

```bash
rtk cargo test -p rollshot-iced-overlay
rtk cargo check -p rollshot-app --features macos-sck
```

On a macOS host, also run:

```bash
rtk cargo check -p rollshot-iced-overlay --features macos-sck
```

- [ ] Commit:

```bash
rtk git add crates/rollshot-iced-overlay crates/rollshot-app
rtk git commit -m "feat(iced-overlay): expose driver for macos"
```

---

## Task 2: Extract Shared Overlay App State

**Files:**
- `crates/rollshot-iced-overlay/src/app.rs`
- `crates/rollshot-iced-overlay/src/linux_runner.rs`

- [ ] Move runner-agnostic types and helpers out of `linux_runner.rs` into `app.rs`.

Move these items unchanged first:

- overlay state struct
- overlay message enum entries that are not layer-shell actions
- `preview_stream`
- `subscription`
- `token_color`
- `crop_mask_bands`
- `CropCanvas`
- `Band`
- `choose_chrome_band`
- `place_outside_crop`
- `toolbar_input_rect`
- `preview_constraints`
- `magenta_toolbar`
- `view`
- `style`
- existing unit tests for placement/crop helpers

- [ ] Define a runner-neutral effect enum in `app.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum OverlayEffect {
    None,
    BeginStitch,
    Finish,
    Cancel,
    EnablePassthrough,
    DisablePassthrough,
}
```

- [ ] Define the shared message enum in `app.rs`:

```rust
#[derive(Debug, Clone)]
pub(crate) enum OverlayMessage {
    IcedEvent(iced::Event),
    Finish,
    Cancel,
    LiveEvent(crate::driver::LiveOverlayEvent),
    Tick,
}
```

- [ ] Make shared update logic return effects instead of directly calling Linux layer-shell actions:

```rust
pub(crate) fn update(
    state: &mut OverlayState,
    message: OverlayMessage,
) -> OverlayEffect {
    // Existing event handling remains here.
    // On first crop confirmation, return OverlayEffect::BeginStitch.
    // On final Enter/Escape finish, return OverlayEffect::Finish.
    // On cancel, return OverlayEffect::Cancel.
    // When a confirmed crop should allow underlying scroll input, return
    // OverlayEffect::EnablePassthrough after BeginStitch has been requested.
}
```

Use exact existing behavior from `linux_runner.rs` for crop selection, warnings, live preview, and keyboard handling.

- [ ] Keep `Driver` calls out of `app.rs`. The app module may store crop rectangles, preview state, and warnings, but runner modules own driver lifecycle because Linux and macOS translate effects differently.

- [ ] Verify the extraction before changing behavior:

```bash
rtk cargo test -p rollshot-iced-overlay
```

- [ ] Commit:

```bash
rtk git add crates/rollshot-iced-overlay/src/app.rs crates/rollshot-iced-overlay/src/linux_runner.rs
rtk git commit -m "refactor(iced-overlay): share overlay app state"
```

---

## Task 3: Rewire Linux Runner Through Shared Effects

**Files:**
- `crates/rollshot-iced-overlay/src/linux_runner.rs`
- `crates/rollshot-iced-overlay/src/app.rs`

- [ ] Keep the `iced_layershell` wrapper message type Linux-only:

```rust
#[to_layer_message]
#[derive(Debug, Clone)]
pub(crate) enum Message {
    Overlay(app::OverlayMessage),
    SetInputRegion(ActionCallback),
}
```

If `to_layer_message` does not accept the tuple variant, use named variants that wrap the shared messages one-for-one:

```rust
#[to_layer_message]
#[derive(Debug, Clone)]
pub(crate) enum Message {
    IcedEvent(Event),
    Finish,
    Cancel,
    LiveEvent(LiveOverlayEvent),
    Tick,
    SetInputRegion(ActionCallback),
}
```

and convert into `app::OverlayMessage` at the start of `update`.

- [ ] Keep all Linux-specific input-region behavior in `linux_runner.rs`.

When `app::update` returns `OverlayEffect::BeginStitch`:

1. Read crop/window state from `OverlayState`.
2. Call `driver.begin_stitch(...)`.
3. Return the existing layer-shell `SetInputRegion` action using `toolbar_input_rect`.

When it returns `OverlayEffect::Finish`:

1. Call `driver.finalize()`.
2. Store the result in `RESULT_SLOT`.
3. Return `get_layer_shell().destroy()`.

When it returns `OverlayEffect::Cancel`:

1. Call `driver.cancel()`.
2. Return `get_layer_shell().destroy()`.

- [ ] Preserve existing Linux tests and add one pure app-state test:

```rust
#[test]
fn finish_without_crop_requests_warning_not_effect() {
    let mut state = OverlayState::default();
    let effect = app::update(&mut state, OverlayMessage::Finish);
    assert_eq!(effect, OverlayEffect::None);
    assert!(state.warning().is_some());
}
```

Expose a read-only `warning(&self) -> Option<&str>` accessor if needed.

- [ ] Verify Linux behavior still compiles and tests pass:

```bash
rtk cargo test -p rollshot-iced-overlay
rtk cargo check -p rollshot-app --features macos-sck
```

- [ ] Commit:

```bash
rtk git add crates/rollshot-iced-overlay/src/app.rs crates/rollshot-iced-overlay/src/linux_runner.rs
rtk git commit -m "refactor(iced-overlay): route linux runner effects"
```

---

## Task 4: Implement macOS AppKit Window Patch

**Files:**
- `crates/rollshot-iced-overlay/Cargo.toml`
- `crates/rollshot-iced-overlay/src/macos_window.rs`

- [ ] Ensure macOS dependencies are target-gated:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.6"
objc2-app-kit = { version = "0.3", features = [
  "NSView",
  "NSWindow",
  "NSWindowCollectionBehavior",
] }
objc2-foundation = "0.3"
```

If the workspace already contains compatible `objc2` versions through transitive dependencies, use the same major versions Cargo resolves for `rfd`/`winit` to avoid duplicate Objective-C bindings.

- [ ] Replace the no-op helper with a raw-window-handle based patch:

```rust
use iced::window;

pub(crate) fn apply_overlay_window_patch(
    handle: &dyn window::Window,
) -> Result<(), String> {
    apply_overlay_window_patch_impl(handle)
}

#[allow(unsafe_code)]
fn apply_overlay_window_patch_impl(
    handle: &dyn window::Window,
) -> Result<(), String> {
    use iced::window::raw_window_handle::RawWindowHandle;
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSView, NSWindowCollectionBehavior};

    let raw = handle
        .window_handle()
        .map_err(|err| format!("failed to read macOS window handle: {err}"))?
        .as_raw();

    let RawWindowHandle::AppKit(appkit) = raw else {
        return Err("expected AppKit window handle for macOS iced overlay".to_string());
    };

    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "macOS window patch must run on the main thread".to_string())?;

    let view = appkit.ns_view.as_ptr() as *mut NSView;
    let view = unsafe {
        Retained::retain(view)
            .ok_or_else(|| "failed to retain iced NSView".to_string())?
    };

    let ns_window = view
        .window()
        .ok_or_else(|| "iced NSView is not attached to an NSWindow".to_string())?;

    unsafe {
        ns_window.setHasShadow(false);
        ns_window.setOpaque(false);
        ns_window.setIgnoresMouseEvents(false);
        ns_window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary,
        );
    }

    let _ = mtm;
    Ok(())
}
```

Adjust method names only as required by the selected `objc2-app-kit` version. Keep the same behavior: transparent, no shadow, all spaces, fullscreen auxiliary, stationary, initially accepting mouse events.

- [ ] Add a unit-testable pure helper for error formatting if needed. Do not try to unit-test real AppKit window patching in CI.

- [ ] Verify on macOS:

```bash
rtk cargo check -p rollshot-iced-overlay --features macos-sck
```

- [ ] Commit:

```bash
rtk git add crates/rollshot-iced-overlay/Cargo.toml crates/rollshot-iced-overlay/src/macos_window.rs
rtk git commit -m "feat(iced-overlay): patch macos overlay window"
```

---

## Task 5: Implement The macOS iced Runner

**Files:**
- `crates/rollshot-iced-overlay/src/macos_runner.rs`
- `crates/rollshot-iced-overlay/src/app.rs`
- `crates/rollshot-iced-overlay/src/driver.rs`

- [ ] Replace the scaffold error with a normal iced application runner.

Target structure:

```rust
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::Duration;

use iced::{Task, window};

use crate::app::{self, OverlayEffect, OverlayMessage, OverlayState};
use crate::driver::{Driver, LiveOverlayEvent};
use crate::{CaptureResult, OverlayConfig, OverlayError};

static PREVIEW_RX: OnceLock<Mutex<Option<mpsc::Receiver<LiveOverlayEvent>>>> =
    OnceLock::new();
static RESULT_SLOT: OnceLock<Mutex<Option<CaptureResult>>> = OnceLock::new();
static DRIVER_SLOT: OnceLock<Mutex<Option<Driver>>> = OnceLock::new();
```

Do not share these statics with Linux in the first implementation. The runners are mutually exclusive per process, and keeping per-runner slots avoids coupling the layer-shell runner to the winit runner.

- [ ] Add the macOS runner `Message` type:

```rust
#[derive(Debug, Clone)]
enum Message {
    Overlay(OverlayMessage),
    WindowPatched(Result<(), String>),
    EnablePassthrough,
    DisablePassthrough,
    Exit,
}
```

- [ ] Initialize capture exactly like Linux:

```rust
let (preview_tx, preview_rx) = mpsc::channel();
let driver = Driver::start_capture(
    config.backend.as_str(),
    config.fps,
    config.show_cursor,
    preview_tx,
)?;
let source_size = driver.source_size();
```

Store `preview_rx`, `driver`, and an empty result slot before starting iced.

- [ ] Create one borderless transparent always-on-top iced window:

```rust
let settings = window::Settings {
    size: iced::Size::new(source_size.width as f32, source_size.height as f32),
    position: window::Position::Specific(iced::Point::ORIGIN),
    decorations: false,
    transparent: true,
    level: window::Level::AlwaysOnTop,
    resizable: false,
    ..window::Settings::default()
};
```

Use `iced::application("Rollshot", update, app::view)
    .window(settings)
    .subscription(subscription)
    .theme(|_| iced::Theme::Dark)
    .style(app::style)
    .run_with(init)`.

- [ ] In `init`, return `OverlayState::new(source_size)` or the existing state constructor from Task 2. Do not start capture in `init`; capture has already started so first-frame failures can return `OverlayError`.

- [ ] Implement `subscription` by combining:

```rust
iced::event::listen().map(|event| Message::Overlay(OverlayMessage::IcedEvent(event)))
```

with `app::preview_stream(preview_rx).map(|event| Message::Overlay(OverlayMessage::LiveEvent(event)))` and the existing tick subscription from Linux.

- [ ] On `Window::Opened`, patch the AppKit window:

The shared `app::update` should record the opened `window::Id` in `OverlayState`. The macOS runner then returns:

```rust
window::run(id, crate::macos_window::apply_overlay_window_patch)
    .map(Message::WindowPatched)
```

Log patch failures to stderr and continue. A patch failure should not panic because capture may still be usable.

- [ ] Translate shared effects:

For `OverlayEffect::BeginStitch`:

1. Call `driver.begin_stitch(...)` with crop/window state from `OverlayState`.
2. Return `window::enable_mouse_passthrough(id).map(|_| Message::EnablePassthrough)`.

For `OverlayEffect::Finish`:

1. Return `window::disable_mouse_passthrough(id).map(|_| Message::DisablePassthrough)` if passthrough is active.
2. Finalize the driver, store `RESULT_SLOT`, and return `window::close(id)`.

For `OverlayEffect::Cancel`:

1. Disable passthrough if active.
2. Cancel the driver.
3. Return `window::close(id)`.

If iced keyboard events stop arriving after whole-window passthrough is enabled, change the macOS runner within this task to require finalization from a non-passthrough toolbar window by switching from `iced::application` to `iced::daemon`. Preserve the same shared `OverlayState` and `app::view` for the overlay window, and keep toolbar-only messages in `macos_runner.rs`.

- [ ] After iced exits, cancel any leftover driver and return:

```rust
Ok(RESULT_SLOT
    .get_or_init(|| Mutex::new(None))
    .lock()
    .expect("result slot poisoned")
    .take())
```

- [ ] Verify on macOS:

```bash
rtk cargo run -p rollshot-app --features macos-sck -- --capture '{"backend":"macos-sck","fps":5,"show_cursor":false,"overlay_mode":"iced"}'
```

Expected manual behavior:

- Transparent overlay opens above the active display.
- Dragging creates the same crop visual as Linux.
- Releasing the drag confirms crop and starts live preview.
- Underlying page receives scroll input after crop confirmation.
- `Esc` cancels before crop confirmation.
- `Esc` finalizes after crop confirmation if live preview is active.
- The command exits without the scaffold error and prints the captured result summary.

- [ ] Commit:

```bash
rtk git add crates/rollshot-iced-overlay/src/macos_runner.rs crates/rollshot-iced-overlay/src/app.rs crates/rollshot-iced-overlay/src/driver.rs
rtk git commit -m "feat(iced-overlay): run macos capture overlay"
```

---

## Task 6: Preserve Explicit Selection And Default Fallback

**Files:**
- `crates/rollshot-app/src/overlay_selection.rs`
- `crates/rollshot-app/src/main.rs`
- `crates/rollshot-app/Cargo.toml`

- [ ] Confirm selection still resolves this way:

```rust
assert_eq!(
    resolve_overlay_runner("macos", OverlayMode::Auto),
    OverlayRunner::Tauri,
);
assert_eq!(
    resolve_overlay_runner("macos", OverlayMode::Iced),
    OverlayRunner::Iced,
);
```

Add or keep these tests in `overlay_selection.rs`.

- [ ] Ensure `rollshot-app` forwards the macOS capture feature to both capture and overlay crates:

```toml
[features]
default = []
macos-sck = [
  "rollshot-capture/macos-sck",
  "rollshot-iced-overlay/macos-sck",
]
```

- [ ] Keep the `OverlayRunner::Tauri` arm returning the existing "not available from this host" error in `rollshot-app`. This plan does not embed the Tauri app in the iced host.

- [ ] Verify:

```bash
rtk cargo test -p rollshot-app
rtk cargo check -p rollshot-app --features macos-sck
```

- [ ] Commit:

```bash
rtk git add crates/rollshot-app
rtk git commit -m "test(app): preserve macos overlay selection"
```

---

## Task 7: Cross-Platform Regression Checks

**Files:**
- No intended file edits.

- [ ] Run Rust formatting:

```bash
rtk cargo fmt --check
```

- [ ] Run focused Rust tests:

```bash
rtk cargo test -p rollshot-app
rtk cargo test -p rollshot-iced-overlay
rtk cargo test -p rollshot-overlay-core
```

- [ ] Run workspace compile with macOS feature enabled on macOS:

```bash
rtk cargo check -p rollshot-app --features macos-sck
rtk cargo check -p rollshot-iced-overlay --features macos-sck
```

- [ ] Run Linux compile on Linux:

```bash
rtk cargo check -p rollshot-iced-overlay
```

- [ ] Run clippy if the focused tests pass:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

If clippy reports pre-existing unrelated warnings, document them in the final response and keep this plan's changes warning-clean where touched.

---

## Task 8: Manual Runtime Validation Matrix

**Files:**
- `README.md` only if command examples need correction after verifying code behavior.
- Do not edit historical `docs/superpowers/specs` or older `docs/superpowers/plans`.

- [ ] macOS explicit iced path:

```bash
rtk cargo run -p rollshot-app --features macos-sck -- --capture '{"backend":"macos-sck","fps":5,"show_cursor":false,"overlay_mode":"iced"}'
```

Record:

- macOS version.
- Display count and scale factor.
- Whether the overlay covers the expected display.
- Whether crop coordinates align with the captured frame.
- Whether scroll input reaches the underlying app after crop confirmation.
- Whether `Esc` cancels before crop and finalizes after crop.
- Whether a `CaptureResult` is returned.

- [ ] macOS default fallback remains unchanged:

```bash
rtk cargo run -p rollshot-app --features macos-sck -- --capture '{"backend":"macos-sck","fps":5,"show_cursor":false,"overlay_mode":"auto"}'
```

Expected in `rollshot-app`: the Tauri runner branch remains selected and reports that Tauri fallback is not launched from this iced host. The separate `rollshot-tauri-app` remains the actual default product fallback.

- [ ] Linux explicit iced path still compiles and, on a Linux Wayland host, still opens the layer-shell overlay:

```bash
rtk cargo run -p rollshot-app -- --capture '{"backend":"portal","fps":5,"show_cursor":false,"overlay_mode":"iced"}'
```

Record any runtime risk if Linux cannot be manually checked on the current machine.

---

## Task 9: Final Review And Commit Hygiene

- [ ] Inspect changed files:

```bash
rtk git status --short
rtk git diff --stat
rtk git diff
```

- [ ] Confirm there are no historical spec/plan edits other than this live plan unless explicitly requested:

```bash
rtk git diff --name-only | rtk rg '^docs/superpowers/(specs|plans)/' || true
```

- [ ] Run code-review-graph change review:

```text
Use code-review-graph detect_changes with base=HEAD~1 or the feature branch base.
Use get_affected_flows for `crates/rollshot-iced-overlay` and `crates/rollshot-app`.
```

- [ ] Create a final squashed or logical commit if the executor used temporary task commits and the branch policy calls for cleanup. Keep the final message conventional:

```bash
rtk git commit -m "feat(iced-overlay): enable macos runtime path"
```

---

## Expected Final Response

Report:

- The macOS iced scaffold error is gone or the exact blocker that remains.
- Which commands passed.
- Which manual runtime checks were performed.
- Whether Linux behavior was verified or only compile-checked.
- Whether macOS `overlay_mode:"auto"` still uses the Tauri fallback path.

Use specific dates and command strings when describing runtime validation.
