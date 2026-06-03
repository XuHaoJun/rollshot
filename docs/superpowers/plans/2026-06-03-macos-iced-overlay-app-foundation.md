# macOS Iced Overlay App Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename the existing Tauri app into a retained fallback app, create the new iced `rollshot-app`, rename the native overlay crate to `rollshot-iced-overlay`, and stage the macOS iced overlay path behind an explicit selector.

**Architecture:** This is a foundation-first migration. The existing Tauri/React path remains available as `rollshot-tauri-app`; the new iced-only product app owns product UI and delegates capture overlays to `rollshot-iced-overlay`. `rollshot-overlay-core`, `rollshot-core`, and `rollshot-capture` remain framework-neutral foundations.

**Tech Stack:** Rust workspace, Cargo package renames, iced 0.14, iced_layershell 0.18 on Linux, Tauri v2 retained in `rollshot-tauri-app`, React/Vite/Vitest retained for the fallback app, ScreenCaptureKit through `rollshot-capture` on macOS.

---

## File Structure

After this plan:

- `crates/rollshot-tauri-app/`
  - Renamed from current `crates/rollshot-app/`.
  - Owns current Tauri/React overlay, `src-tauri`, Vite frontend, Tauri tests, and Tauri save/fallback behavior.
- `crates/rollshot-app/`
  - New Rust-only iced product app crate.
  - Owns capture launch parsing, overlay selection, product app shell, final preview/save handoff stubs, and future editor/settings modules.
- `crates/rollshot-iced-overlay/`
  - Renamed from current `crates/rollshot-overlay/`.
  - Owns iced overlay renderer, Linux layer-shell runner, future macOS runner, and future Windows runner.
- `crates/rollshot-overlay-core/`
  - Unchanged framework-neutral overlay logic.
- `crates/rollshot-capture/src/types.rs`
  - Adds shared overlay selection to `InteractiveLaunchOptions` so old launch JSON remains valid and macOS iced remains explicit opt-in.

Do not create `rollshot-iced-ui` in this plan. Reserve the name in docs only.

---

### Task 1: Rename Current Tauri App To `rollshot-tauri-app`

**Files:**
- Move: `crates/rollshot-app/` -> `crates/rollshot-tauri-app/`
- Modify: `Cargo.toml`
- Modify: `crates/rollshot-tauri-app/src-tauri/Cargo.toml`
- Modify: `crates/rollshot-tauri-app/package.json`
- Modify: `.gitignore`
- Modify references found by `rtk rg -n "crates/rollshot-app|rollshot-app|target/release/rollshot-app" README.md Cargo.toml crates scripts docs --glob '!docs/superpowers/**'`

- [ ] **Step 1: Move the directory**

Run:

```bash
rtk git mv crates/rollshot-app crates/rollshot-tauri-app
```

Expected: no output and `git status --short` shows a rename from `crates/rollshot-app` to `crates/rollshot-tauri-app`.

- [ ] **Step 2: Update workspace membership**

In root `Cargo.toml`, replace:

```toml
"crates/rollshot-app/src-tauri",
```

with:

```toml
"crates/rollshot-tauri-app/src-tauri",
```

- [ ] **Step 3: Rename the Tauri Rust package and binary**

In `crates/rollshot-tauri-app/src-tauri/Cargo.toml`, replace the package and binary names with:

```toml
[package]
name = "rollshot-tauri-app"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[[bin]]
name = "rollshot-tauri-app"
path = "src/main.rs"
```

Also update its overlay dependency after Task 2. For this task, leave:

```toml
rollshot-overlay = { path = "../../rollshot-overlay" }
```

unchanged until the overlay crate rename happens.

- [ ] **Step 4: Rename the frontend package**

In `crates/rollshot-tauri-app/package.json`, replace:

```json
"name": "rollshot-app",
```

with:

```json
"name": "rollshot-tauri-app",
```

Replace the release run script:

```json
"tauri:release:run": "tauri build --no-bundle && ../../target/release/rollshot-app"
```

with:

```json
"tauri:release:run": "tauri build --no-bundle && ../../target/release/rollshot-tauri-app"
```

- [ ] **Step 5: Update ignored frontend build paths**

In `.gitignore`, replace:

```gitignore
crates/rollshot-app/node_modules/
crates/rollshot-app/dist/
```

with:

```gitignore
crates/rollshot-tauri-app/node_modules/
crates/rollshot-tauri-app/dist/
```

- [ ] **Step 6: Update non-historical path references**

Run:

```bash
rtk rg -n "crates/rollshot-app|target/release/rollshot-app|rollshot-app" README.md Cargo.toml crates scripts docs --glob '!docs/superpowers/**'
```

For references to the current Tauri app, update text to `crates/rollshot-tauri-app` and `rollshot-tauri-app`. Do not edit historical files under `docs/superpowers/`.

If a reference describes the future iced product app rather than the current Tauri app, leave it for Task 3 where the new `rollshot-app` is created.

- [ ] **Step 7: Verify the rename compiles**

Run:

```bash
rtk cargo test -p rollshot-tauri-app
```

Expected: all `rollshot-tauri-app` Rust tests pass.

Run:

```bash
rtk pnpm --dir crates/rollshot-tauri-app run typecheck
```

Expected: TypeScript typecheck exits 0.

Run:

```bash
rtk pnpm --dir crates/rollshot-tauri-app test
```

Expected: Vitest exits 0.

- [ ] **Step 8: Commit**

Run:

```bash
rtk git add Cargo.toml .gitignore README.md crates/rollshot-tauri-app
rtk git status --short
rtk git commit -m "chore(app): rename tauri app crate"
```

Expected: commit includes only the Tauri app rename and reference updates.

---

### Task 2: Rename Native Overlay Crate To `rollshot-iced-overlay`

**Files:**
- Move: `crates/rollshot-overlay/` -> `crates/rollshot-iced-overlay/`
- Modify: `Cargo.toml`
- Modify: `crates/rollshot-iced-overlay/Cargo.toml`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Modify: `crates/rollshot-tauri-app/src-tauri/Cargo.toml`
- Modify: `crates/rollshot-tauri-app/src-tauri/src/native_capture.rs`
- Modify references found by `rtk rg -n "rollshot-overlay|rollshot_overlay|crates/rollshot-overlay" Cargo.toml crates README.md docs --glob '!docs/superpowers/**'`

- [ ] **Step 1: Move the directory**

Run:

```bash
rtk git mv crates/rollshot-overlay crates/rollshot-iced-overlay
```

Expected: no output and `git status --short` shows the directory rename.

- [ ] **Step 2: Update workspace membership**

In root `Cargo.toml`, replace:

```toml
"crates/rollshot-overlay",
```

with:

```toml
"crates/rollshot-iced-overlay",
```

- [ ] **Step 3: Rename the overlay package**

In `crates/rollshot-iced-overlay/Cargo.toml`, replace:

```toml
name = "rollshot-overlay"
```

with:

```toml
name = "rollshot-iced-overlay"
```

- [ ] **Step 4: Update Tauri dependency**

In `crates/rollshot-tauri-app/src-tauri/Cargo.toml`, replace:

```toml
rollshot-overlay = { path = "../../rollshot-overlay" }
```

with:

```toml
rollshot-iced-overlay = { path = "../../rollshot-iced-overlay" }
```

- [ ] **Step 5: Update Rust imports**

In `crates/rollshot-tauri-app/src-tauri/src/native_capture.rs`, replace:

```rust
use rollshot_overlay::{run_overlay, CaptureResult, OverlayConfig, OverlayError};
```

with:

```rust
use rollshot_iced_overlay::{run_overlay, CaptureResult, OverlayConfig, OverlayError};
```

- [ ] **Step 6: Update overlay crate documentation**

In `crates/rollshot-iced-overlay/src/lib.rs`, replace the module doc comment with:

```rust
//! Iced capture overlay renderer.
//!
//! Linux currently uses the iced/layer-shell runner. macOS and Windows compile
//! to an unsupported result until their normal-window runners land. The crate is
//! named for the renderer framework so it can coexist with the retained Tauri
//! overlay during validation.
```

Keep the non-Linux `OverlayError::Unsupported` behavior for this task.

- [ ] **Step 7: Update non-historical references**

Run:

```bash
rtk rg -n "rollshot-overlay|rollshot_overlay|crates/rollshot-overlay" Cargo.toml crates README.md docs --glob '!docs/superpowers/**'
```

Update current implementation references to `rollshot-iced-overlay`, `rollshot_iced_overlay`, and `crates/rollshot-iced-overlay`.

Do not edit historical files under `docs/superpowers/`.

- [ ] **Step 8: Verify the rename compiles**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay
```

Expected: all overlay crate tests pass or the package reports zero tests with exit 0.

Run:

```bash
rtk cargo test -p rollshot-tauri-app
```

Expected: all Tauri Rust tests pass with the renamed overlay dependency.

- [ ] **Step 9: Commit**

Run:

```bash
rtk git add Cargo.toml README.md crates/rollshot-iced-overlay crates/rollshot-tauri-app
rtk git status --short
rtk git commit -m "chore(overlay): rename iced overlay crate"
```

Expected: commit contains only the overlay crate rename and reference updates.

---

### Task 3: Add Shared Overlay Mode To Launch Options

**Files:**
- Modify: `crates/rollshot-capture/src/types.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`
- Modify: `crates/rollshot-tauri-app/src-tauri/src/launch.rs`
- Modify: `crates/rollshot-tauri-app/src-tauri/src/native_capture.rs`
- Modify: `crates/rollshot-tauri-app/src/api/capture.ts`
- Test: existing Rust tests in `crates/rollshot-capture/src/types.rs`, `crates/rollshot-tauri-app/src-tauri/src/launch.rs`, and `crates/rollshot-tauri-app/src-tauri/src/native_capture.rs`

- [ ] **Step 1: Add `OverlayMode`**

In `crates/rollshot-capture/src/types.rs`, add this enum above `InteractiveLaunchOptions`:

```rust
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayMode {
    #[default]
    Auto,
    Tauri,
    Iced,
}
```

Replace `InteractiveLaunchOptions` with:

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InteractiveLaunchOptions {
    pub backend: String,
    pub fps: u32,
    pub show_cursor: bool,
    #[serde(default)]
    pub overlay_mode: OverlayMode,
}

impl InteractiveLaunchOptions {
    pub fn default_capture() -> Self {
        Self {
            backend: "auto".to_string(),
            fps: 5,
            show_cursor: false,
            overlay_mode: OverlayMode::Auto,
        }
    }
}
```

- [ ] **Step 2: Re-export the selector**

In `crates/rollshot-capture/src/lib.rs`, replace:

```rust
    CaptureOptions, CaptureProbe, CapturedFrame, FrameMetadata, InteractiveLaunchOptions,
    PixelFormat, Region, RegionMode, Size,
```

with:

```rust
    CaptureOptions, CaptureProbe, CapturedFrame, FrameMetadata, InteractiveLaunchOptions,
    OverlayMode, PixelFormat, Region, RegionMode, Size,
```

- [ ] **Step 3: Update launch option tests**

In `crates/rollshot-capture/src/types.rs`, update the test module to import `OverlayMode`:

```rust
use super::{InteractiveLaunchOptions, OverlayMode};
```

Update `interactive_launch_options_round_trip_json` to construct:

```rust
let options = InteractiveLaunchOptions {
    backend: "linux-portal".to_string(),
    fps: 7,
    show_cursor: true,
    overlay_mode: OverlayMode::Iced,
};
```

Add this test:

```rust
#[test]
fn interactive_launch_options_default_overlay_mode_for_old_json() {
    let decoded: InteractiveLaunchOptions =
        serde_json::from_str(r#"{"backend":"auto","fps":5,"show_cursor":false}"#)
            .expect("deserialize old launch options");

    assert_eq!(decoded.overlay_mode, OverlayMode::Auto);
}
```

- [ ] **Step 4: Use the constructor for no-arg launch**

In `crates/rollshot-tauri-app/src-tauri/src/launch.rs`, replace the no-argument default construction:

```rust
return Ok(LaunchMode::Capture(InteractiveLaunchOptions {
    backend: "auto".to_string(),
    fps: 5,
    show_cursor: false,
}));
```

with:

```rust
return Ok(LaunchMode::Capture(InteractiveLaunchOptions::default_capture()));
```

In the same file, update `parses_capture_launch_options` to assert:

```rust
assert_eq!(options.overlay_mode, rollshot_capture::OverlayMode::Auto);
```

Add a test:

```rust
#[test]
fn parses_overlay_mode() {
    let mode = parse_launch_args([
        "rollshot-tauri-app",
        "--capture",
        r#"{"backend":"macos-sck","fps":30,"show_cursor":false,"overlay_mode":"iced"}"#,
    ])
    .expect("parse launch args");

    match mode {
        LaunchMode::Capture(options) => {
            assert_eq!(options.overlay_mode, rollshot_capture::OverlayMode::Iced);
        }
    }
}
```

- [ ] **Step 5: Update explicit Rust constructions**

In `crates/rollshot-tauri-app/src-tauri/src/native_capture.rs`, add `overlay_mode: rollshot_capture::OverlayMode::Auto` to each `InteractiveLaunchOptions` literal in tests.

Example:

```rust
let config = overlay_config(&InteractiveLaunchOptions {
    backend: "linux-portal".to_string(),
    fps: 5,
    show_cursor: true,
    overlay_mode: rollshot_capture::OverlayMode::Auto,
});
```

- [ ] **Step 6: Update TypeScript launch type**

In `crates/rollshot-tauri-app/src/api/capture.ts`, replace:

```ts
export type InteractiveLaunchOptions = {
  backend: string
  fps: number
  show_cursor: boolean
}
```

with:

```ts
export type OverlayMode = 'auto' | 'tauri' | 'iced'

export type InteractiveLaunchOptions = {
  backend: string
  fps: number
  show_cursor: boolean
  overlay_mode: OverlayMode
}
```

- [ ] **Step 7: Verify shared selector compatibility**

Run:

```bash
rtk cargo test -p rollshot-capture interactive_launch_options
```

Expected: launch option serialization tests pass.

Run:

```bash
rtk cargo test -p rollshot-tauri-app launch
```

Expected: launch parser tests pass.

Run:

```bash
rtk pnpm --dir crates/rollshot-tauri-app run typecheck
```

Expected: TypeScript typecheck exits 0.

- [ ] **Step 8: Commit**

Run:

```bash
rtk git add crates/rollshot-capture crates/rollshot-tauri-app
rtk git status --short
rtk git commit -m "feat(capture): add overlay mode launch option"
```

Expected: commit contains only launch selector changes.

---

### Task 4: Create New Iced `rollshot-app` Shell

**Files:**
- Create: `crates/rollshot-app/Cargo.toml`
- Create: `crates/rollshot-app/src/main.rs`
- Create: `crates/rollshot-app/src/launch.rs`
- Create: `crates/rollshot-app/src/overlay_selection.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Add the new app to the workspace**

In root `Cargo.toml`, add:

```toml
"crates/rollshot-app",
```

to `workspace.members` near `crates/rollshot-tauri-app/src-tauri`.

- [ ] **Step 2: Create the new app manifest**

Create `crates/rollshot-app/Cargo.toml`:

```toml
[package]
name = "rollshot-app"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[[bin]]
name = "rollshot-app"
path = "src/main.rs"

[dependencies]
iced = { version = "0.14", features = ["tokio"] }
rollshot-capture = { path = "../rollshot-capture" }
rollshot-iced-overlay = { path = "../rollshot-iced-overlay" }
serde_json = { workspace = true }

[features]
default = []
macos-sck = ["rollshot-capture/macos-sck"]

[lints]
workspace = true
```

- [ ] **Step 3: Add launch parsing**

Create `crates/rollshot-app/src/launch.rs`:

```rust
use rollshot_capture::InteractiveLaunchOptions;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchMode {
    Capture(InteractiveLaunchOptions),
}

pub fn parse_launch_args<I, S>(args: I) -> Result<LaunchMode, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into);
    let _program = args.next();

    let Some(flag) = args.next() else {
        return Ok(LaunchMode::Capture(InteractiveLaunchOptions::default_capture()));
    };

    if flag != "--capture" {
        return Err(format!("unknown rollshot-app argument '{flag}'"));
    }

    let Some(payload) = args.next() else {
        return Err("--capture requires a JSON payload".to_string());
    };

    if let Some(extra) = args.next() {
        return Err(format!(
            "unexpected argument after capture payload: '{extra}'"
        ));
    }

    let options: InteractiveLaunchOptions = serde_json::from_str(&payload)
        .map_err(|err| format!("invalid --capture JSON payload: {err}"))?;
    Ok(LaunchMode::Capture(options))
}

#[cfg(test)]
mod tests {
    use super::{parse_launch_args, LaunchMode};
    use rollshot_capture::OverlayMode;

    #[test]
    fn no_args_uses_defaults() {
        let mode = parse_launch_args(["rollshot-app"]).expect("no args should succeed");
        match mode {
            LaunchMode::Capture(options) => {
                assert_eq!(options.backend, "auto");
                assert_eq!(options.fps, 5);
                assert!(!options.show_cursor);
                assert_eq!(options.overlay_mode, OverlayMode::Auto);
            }
        }
    }

    #[test]
    fn parses_overlay_mode() {
        let mode = parse_launch_args([
            "rollshot-app",
            "--capture",
            r#"{"backend":"macos-sck","fps":30,"show_cursor":false,"overlay_mode":"iced"}"#,
        ])
        .expect("parse launch args");

        match mode {
            LaunchMode::Capture(options) => {
                assert_eq!(options.overlay_mode, OverlayMode::Iced);
            }
        }
    }
}
```

- [ ] **Step 4: Add overlay selection**

Create `crates/rollshot-app/src/overlay_selection.rs`:

```rust
use rollshot_capture::OverlayMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayRunner {
    Iced,
    Tauri,
}

pub fn resolve_overlay_runner(os: &str, mode: OverlayMode) -> OverlayRunner {
    match (os, mode) {
        (_, OverlayMode::Iced) => OverlayRunner::Iced,
        (_, OverlayMode::Tauri) => OverlayRunner::Tauri,
        ("linux", OverlayMode::Auto) => OverlayRunner::Iced,
        ("macos", OverlayMode::Auto) => OverlayRunner::Tauri,
        (_, OverlayMode::Auto) => OverlayRunner::Tauri,
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_overlay_runner, OverlayRunner};
    use rollshot_capture::OverlayMode;

    #[test]
    fn linux_auto_uses_iced_overlay() {
        assert_eq!(
            resolve_overlay_runner("linux", OverlayMode::Auto),
            OverlayRunner::Iced
        );
    }

    #[test]
    fn macos_auto_keeps_tauri_fallback() {
        assert_eq!(
            resolve_overlay_runner("macos", OverlayMode::Auto),
            OverlayRunner::Tauri
        );
    }

    #[test]
    fn macos_iced_is_explicit_opt_in() {
        assert_eq!(
            resolve_overlay_runner("macos", OverlayMode::Iced),
            OverlayRunner::Iced
        );
    }
}
```

- [ ] **Step 5: Add the new app entry point**

Create `crates/rollshot-app/src/main.rs`:

```rust
mod launch;
mod overlay_selection;

use launch::LaunchMode;
use overlay_selection::{resolve_overlay_runner, OverlayRunner};

fn main() {
    let launch_mode = match launch::parse_launch_args(std::env::args()) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

    let LaunchMode::Capture(options) = launch_mode;
    match resolve_overlay_runner(std::env::consts::OS, options.overlay_mode) {
        OverlayRunner::Iced => run_iced_capture(options),
        OverlayRunner::Tauri => {
            eprintln!(
                "selected overlay mode requires rollshot-tauri-app during the validation period"
            );
            std::process::exit(2);
        }
    }
}

fn run_iced_capture(options: rollshot_capture::InteractiveLaunchOptions) {
    let config = rollshot_iced_overlay::OverlayConfig {
        backend: options.backend,
        fps: options.fps,
        show_cursor: options.show_cursor,
    };

    match rollshot_iced_overlay::run_overlay(config) {
        Ok(Some(result)) => {
            println!(
                "captured {}x{} ({} frames)",
                result.image.width(),
                result.image.height(),
                result.stats.frame_count
            );
        }
        Ok(None) => {
            println!("capture cancelled");
        }
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
}
```

This shell intentionally does not implement final preview/editor/settings yet. It gives `rollshot-app` ownership of the product app name and a working iced capture entry point where the platform supports `rollshot-iced-overlay`.

- [ ] **Step 6: Verify the new app shell**

Run:

```bash
rtk cargo test -p rollshot-app
```

Expected: launch and overlay selection tests pass.

Run:

```bash
rtk cargo check -p rollshot-app
```

Expected: new iced app compiles.

- [ ] **Step 7: Commit**

Run:

```bash
rtk git add Cargo.toml crates/rollshot-app
rtk git status --short
rtk git commit -m "feat(app): add iced product app shell"
```

Expected: commit contains only the new app shell and workspace membership.

---

### Task 5: Split Shared Iced Overlay State From Linux Runner

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Create: `crates/rollshot-iced-overlay/src/app.rs`
- Create: `crates/rollshot-iced-overlay/src/linux_runner.rs`
- Delete after move: `crates/rollshot-iced-overlay/src/overlay.rs`
- Modify: `crates/rollshot-iced-overlay/src/driver.rs`
- Test: existing preview placement tests in `crates/rollshot-iced-overlay/src/linux_runner.rs`

- [ ] **Step 1: Move the current Linux runner file**

Run:

```bash
rtk git mv crates/rollshot-iced-overlay/src/overlay.rs crates/rollshot-iced-overlay/src/linux_runner.rs
```

Expected: file rename is staged as a move.

- [ ] **Step 2: Create shared app state module**

Create `crates/rollshot-iced-overlay/src/app.rs` by moving shared state and the preview constraint type out of `linux_runner.rs`. Move these existing items from the renamed Linux runner:

- `PreviewConstraints`
- `Overlay`, renamed to `OverlayState`

Use this content:

```rust
use iced::widget::image;
use iced::{Point, Rectangle, Size};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreviewConstraints {
    pub(crate) fixed_width: u32,
    pub(crate) max_height: u32,
}

#[derive(Default)]
pub(crate) struct OverlayState {
    pub(crate) drag_start: Option<Point>,
    pub(crate) crop: Option<Rectangle>,
    pub(crate) crop_confirmed: bool,
    pub(crate) preview: Option<image::Handle>,
    pub(crate) window_size: Option<Size>,
    pub(crate) capture_miss_warn: bool,
    pub(crate) capture_miss_message_expires_at: Option<std::time::Instant>,
}
```

Keep `Band`, `choose_chrome_band`, `place_outside_crop`, `toolbar_input_rect`, `preview_constraints`, and the existing preview constraint tests in `linux_runner.rs` for this task. That preserves the current Linux placement behavior while starting the shared app boundary.

- [ ] **Step 3: Wire modules from `lib.rs`**

In `crates/rollshot-iced-overlay/src/lib.rs`, replace:

```rust
#[cfg(target_os = "linux")]
mod overlay;
```

with:

```rust
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod app;
#[cfg(target_os = "linux")]
mod linux_runner;
```

Replace the Linux `run_overlay` arm:

```rust
overlay::run(config)
```

with:

```rust
linux_runner::run(config)
```

- [ ] **Step 4: Update imports in the Linux runner and driver**

In `crates/rollshot-iced-overlay/src/driver.rs`, replace:

```rust
use crate::overlay::PreviewConstraints;
```

with:

```rust
use crate::app::PreviewConstraints;
```

In `crates/rollshot-iced-overlay/src/linux_runner.rs`, replace the local `PreviewConstraints` definition and local `Overlay` state definition with:

```rust
use crate::app::{OverlayState as Overlay, PreviewConstraints};
```

Keep the current `Message` enum in `linux_runner.rs` because it uses `#[to_layer_message]`.

- [ ] **Step 5: Preserve Linux placement tests**

In `crates/rollshot-iced-overlay/src/linux_runner.rs`, keep the existing preview placement tests unchanged:

```rust
fn preview_constraints_use_fixed_width_and_bottom_band_height()
fn preview_constraints_clamp_width_to_side_band()
fn preview_constraints_cap_height_at_crop_height()
```

These tests prove the state/type extraction did not change Linux preview placement behavior.

- [ ] **Step 6: Verify Linux overlay still compiles**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay
```

Expected: new `preview_constraints` test passes and Linux overlay code compiles on Linux.

- [ ] **Step 7: Commit**

Run:

```bash
rtk git add crates/rollshot-iced-overlay
rtk git status --short
rtk git commit -m "refactor(overlay): split iced app state from linux runner"
```

Expected: commit contains only the first runner-agnostic overlay split.

---

### Task 6: Add macOS Runner Scaffolding Behind Opt-In

**Files:**
- Modify: `crates/rollshot-iced-overlay/Cargo.toml`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Create: `crates/rollshot-iced-overlay/src/macos_runner.rs`
- Create: `crates/rollshot-iced-overlay/src/macos_window.rs`

- [ ] **Step 1: Add macOS iced dependencies**

In `crates/rollshot-iced-overlay/Cargo.toml`, add:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
iced = { version = "0.14", features = ["canvas", "image", "tokio"] }
objc2 = "0.6"
objc2-app-kit = "0.3"
objc2-foundation = "0.3"
raw-window-handle = "0.6"
```

Keep the existing Linux `iced` and `iced_layershell` dependencies unchanged.

- [ ] **Step 2: Wire the macOS module**

In `crates/rollshot-iced-overlay/src/lib.rs`, add:

```rust
#[cfg(target_os = "macos")]
mod macos_runner;
#[cfg(target_os = "macos")]
mod macos_window;
```

Replace the non-Linux unsupported arm in `run_overlay` with platform-specific arms:

```rust
pub fn run_overlay(config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    #[cfg(target_os = "linux")]
    {
        linux_runner::run(config)
    }
    #[cfg(target_os = "macos")]
    {
        macos_runner::run(config)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = config;
        Err(OverlayError::Unsupported)
    }
}
```

- [ ] **Step 3: Add macOS window patch helper**

Create `crates/rollshot-iced-overlay/src/macos_window.rs`:

```rust
use iced::window;

pub(crate) fn apply_overlay_window_patch(_id: window::Id) {
    // The concrete AppKit calls are isolated here so runner code does not
    // depend on Objective-C symbols directly. This scaffold is a no-op until
    // the macOS runner task replaces it with tested AppKit calls.
}
```

This helper is intentionally small and macOS-gated. It exists now to lock the ownership boundary; it does not claim runtime parity until the manual macOS checklist passes.

- [ ] **Step 4: Add macOS runner scaffold**

Create `crates/rollshot-iced-overlay/src/macos_runner.rs`:

```rust
use crate::{CaptureResult, OverlayConfig, OverlayError};

pub(crate) fn run(config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    let _ = config;
    Err(OverlayError::Overlay(
        "macOS iced overlay runner is scaffolded but not wired to capture yet".to_string(),
    ))
}
```

- [ ] **Step 5: Verify cross-platform scaffolding from Linux**

Run:

```bash
rtk cargo check -p rollshot-iced-overlay
```

Expected on Linux: overlay crate compiles and macOS-gated modules are not compiled.

Run:

```bash
rtk cargo check -p rollshot-app
```

Expected on Linux: new app compiles against the scaffolded overlay crate.

- [ ] **Step 6: Commit**

Run:

```bash
rtk git add crates/rollshot-iced-overlay crates/rollshot-app
rtk git status --short
rtk git commit -m "feat(overlay): scaffold macos iced runner"
```

Expected: commit contains macOS-gated runner scaffolding and dependency updates.

---

### Task 7: Implement Iced App Delegation And Tauri Fallback Message

**Files:**
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-app/src/overlay_selection.rs`
- Modify: `crates/rollshot-tauri-app/src-tauri/src/native_capture.rs`
- Modify: `crates/rollshot-tauri-app/src/App.tsx`

- [ ] **Step 1: Keep Tauri native overlay default Linux-only**

In `crates/rollshot-tauri-app/src-tauri/src/native_capture.rs`, keep:

```rust
#[tauri::command]
pub fn uses_native_overlay() -> bool {
    cfg!(target_os = "linux")
}
```

Do not make this depend on `OverlayMode`. The retained Tauri app keeps its existing behavior: Linux launches the native iced overlay handoff; macOS keeps React/webview overlay.

- [ ] **Step 2: Ensure new iced app resolves macOS default to Tauri**

In `crates/rollshot-app/src/overlay_selection.rs`, keep these tests from Task 4:

```rust
#[test]
fn macos_auto_keeps_tauri_fallback() {
    assert_eq!(
        resolve_overlay_runner("macos", OverlayMode::Auto),
        OverlayRunner::Tauri
    );
}

#[test]
fn macos_iced_is_explicit_opt_in() {
    assert_eq!(
        resolve_overlay_runner("macos", OverlayMode::Iced),
        OverlayRunner::Iced
    );
}
```

These tests are the executable statement of the coexistence rule.

- [ ] **Step 3: Make the Tauri fallback error actionable**

In `crates/rollshot-app/src/main.rs`, replace the Tauri fallback message with:

```rust
eprintln!(
    "selected overlay mode resolves to the retained Tauri overlay; run rollshot-tauri-app or pass overlay_mode=\"iced\" for the iced validation path"
);
```

- [ ] **Step 4: Verify app selector behavior**

Run:

```bash
rtk cargo test -p rollshot-app overlay_selection
```

Expected: Linux auto, macOS auto, and macOS iced tests pass.

Run:

```bash
rtk cargo test -p rollshot-tauri-app native_capture::tests::uses_native_overlay_matches_target_os
```

Expected: retained Tauri app still reports native overlay only on Linux.

- [ ] **Step 5: Commit**

Run:

```bash
rtk git add crates/rollshot-app crates/rollshot-tauri-app
rtk git status --short
rtk git commit -m "feat(app): keep macos iced overlay opt-in"
```

Expected: commit contains only selector and fallback behavior.

---

### Task 8: Add Documentation For The Temporary Coexistence Model

**Files:**
- Modify: `README.md`
- Modify: `docs/iced-migration-evaluation.md` only if it has current, non-historical command/path references that are now misleading
- Do not modify historical files under `docs/superpowers/specs/` or `docs/superpowers/plans/` except this plan

- [ ] **Step 1: Document app names**

In `README.md`, add a short section near setup or development commands:

```markdown
### Desktop app crates during iced migration

- `rollshot-app` is the iced product app.
- `rollshot-tauri-app` is the retained Tauri/React app used as the macOS overlay
  reference and fallback during validation.
- `rollshot-iced-overlay` is the iced overlay renderer used by Linux today and
  by the opt-in macOS iced overlay path as it lands.
```

- [ ] **Step 2: Update command examples**

Run:

```bash
rtk rg -n "crates/rollshot-app|rollshot-app|rollshot-overlay|rollshot_overlay" README.md docs/iced-migration-evaluation.md
```

Update examples that refer to the current Tauri app to `rollshot-tauri-app`.
Update examples that refer to the native iced overlay crate to `rollshot-iced-overlay`.
Keep text that intentionally describes the future iced product app as `rollshot-app`.

- [ ] **Step 3: Verify docs have no stale current-path references**

Run:

```bash
rtk rg -n "crates/rollshot-app/src-tauri|crates/rollshot-overlay|rollshot_overlay" README.md docs/iced-migration-evaluation.md
```

Expected: no output for current-path references. If output remains, each line must be historical discussion explicitly marked as pre-rename context.

- [ ] **Step 4: Commit**

Run:

```bash
rtk git add README.md docs/iced-migration-evaluation.md
rtk git status --short
rtk git commit -m "docs: document iced app coexistence"
```

Expected: commit contains only current docs updates.

---

### Task 9: Final Verification

**Files:**
- No new files
- Checks all changed workspace crates

- [ ] **Step 1: Run Rust tests**

Run:

```bash
rtk cargo test
```

Expected: all Rust tests pass.

- [ ] **Step 2: Run formatting check**

Run:

```bash
rtk cargo fmt --check
```

Expected: formatter exits 0.

- [ ] **Step 3: Run Tauri frontend checks**

Run:

```bash
rtk pnpm --dir crates/rollshot-tauri-app run typecheck
```

Expected: TypeScript typecheck exits 0.

Run:

```bash
rtk pnpm --dir crates/rollshot-tauri-app test
```

Expected: Vitest exits 0.

- [ ] **Step 4: Run focused app checks**

Run:

```bash
rtk cargo check -p rollshot-app
```

Expected: new iced app compiles.

Run:

```bash
rtk cargo check -p rollshot-iced-overlay
```

Expected: renamed iced overlay crate compiles.

- [ ] **Step 5: Inspect final diff**

Run:

```bash
rtk git status --short
rtk git log --oneline --decorate -8
```

Expected: working tree has only intentional changes, and the recent commits match the task commits in this plan.

---

## Manual macOS Runtime Checklist

Run this checklist on macOS after the macOS runner is wired beyond the scaffold:

- `rollshot-tauri-app` still launches the current Tauri/React overlay by default.
- `rollshot-app --capture '{"backend":"macos-sck","fps":30,"show_cursor":false,"overlay_mode":"iced"}'` enters the iced validation path.
- The iced overlay window is transparent, borderless, topmost, shadowless, and covers the primary display.
- ScreenCaptureKit permission prompts are understandable and do not leave the app stuck.
- Crop selection maps to captured frame pixels correctly on Retina scale factors.
- During scrolling, target content receives input.
- Required overlay controls remain usable under the chosen macOS passthrough strategy.
- Live preview avoids the selected region.
- Capture-miss warning appears and clears.
- Escape, stop, and cancel behave consistently with the Tauri overlay.
- Final capture result returns to `rollshot-app`.
- Save writes a PNG and reports errors without trapping the user.

---

## Spec Coverage Self-Review

- Tauri app rename: Task 1.
- New iced `rollshot-app`: Task 4.
- `rollshot-iced-overlay` rename: Task 2.
- Shared overlay selector and macOS opt-in: Tasks 3 and 7.
- `rollshot-iced-ui` reserved but not created: File Structure and Task 8 docs.
- Tauri retained as reference/fallback: Tasks 1, 7, 8, and manual checklist.
- macOS runner ownership boundary: Task 6.
- Linux regression risk after overlay split: Tasks 5 and 9.
- Verification: Tasks 1 through 9 and manual macOS checklist.
