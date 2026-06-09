# Post-Capture Image Viewer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move completed captures out of the capture overlay, auto-save every successful capture, open a full Result Workspace on Linux or auto-save failure, and show a draggable floating thumbnail before the Result Workspace on macOS.

**Architecture:** `rollshot-iced-overlay` becomes capture-only and returns a plain `CaptureResult` as soon as capture finishes. `rollshot-app` owns a post-capture pipeline that auto-saves first, selects the platform presentation, and then runs either the Linux Result Workspace or a macOS post-capture child process. The macOS child owns one iced daemon that transitions from floating thumbnail to Result Workspace without recreating winit's event loop; the Result Workspace keeps document state, system actions, and pure viewport geometry in focused reusable modules, while a narrow AppKit bridge performs native file drag.

**Tech Stack:** Rust 2021, iced 0.14, `image`, `rfd`, `arboard`, `chrono`, `dirs`, macOS `objc2`/AppKit, existing Rollshot capture and overlay crates.

---

## Implementation Assumptions

- Treat Linux `XDG_DESKTOP_DIR` as the value from `${XDG_CONFIG_HOME:-$HOME/.config}/user-dirs.dirs`, expanding only `$HOME` and `${HOME}`. Use `~/Pictures` when that value is absent, malformed, relative, or points to a missing directory. Do not create either directory.
- Use local wall-clock time for `Rollshot YYYY-MM-DD at HH.MM.SS.png`.
- The pinned winit 0.30 backend rejects a second event loop in one process with `EventLoopError::RecreationAttempt`. Linux may start its one ordinary iced Result Workspace after the layer-shell overlay exits. macOS must launch a transient `rollshot-app` post-capture child and wait for it; the child uses one iced daemon for thumbnail and Result Workspace. This is the plan's only intentional deviation from the design's single-process statement.
- For macOS auto-save success, the child losslessly reloads the saved PNG. For macOS auto-save failure, the parent encodes the captured image as PNG and writes it to the child's stdin; no temporary file is created, and the parent waits for the child.
- The viewer image handle is built once from its `RgbaImage`.
- Use `rfd::AsyncFileDialog` for Result Workspace Save As so the native dialog is not opened synchronously inside iced's event loop.
- Implement unsaved-close confirmation as an iced modal containing the exact prompt `Discard unsaved capture?`; both the toolbar Close action and `window::Event::CloseRequested` route to the same state transition.
- Iced does not expose a named AppKit drag-threshold API. Use a tested four-logical-point threshold before calling AppKit's native `beginDraggingSessionWithItems:event:source:`. The transfer after that threshold is a real native file drag.
- Keep the AppKit bridge inside `rollshot-app`. Change that package's unsafe lint from inherited `forbid` to package-local `deny`, and use one explicitly annotated `#[allow(unsafe_code)]` bridge function/module, matching the existing `rollshot-iced-overlay/src/macos_window.rs` pattern.
- Do not modify `crates/rollshot-tauri-app`.

## Engineering Constraint Requiring Approval

The approved design says one `rollshot-app` process owns capture and all
post-capture UI. The current macOS overlay already consumes the process's only
winit event loop, and pinned winit 0.30 rejects another with
`EventLoopError::RecreationAttempt`. Keeping exactly one process would require a
larger redesign that moves the macOS capture overlay and post-capture host into
one shared daemon, crossing the current `rollshot-app` /
`rollshot-iced-overlay` ownership boundary.

This plan instead uses one transient macOS post-capture child and makes the
capture process wait for it. The child is per-capture, has no shared state, and
owns one daemon for thumbnail/workspace transitions. Execution should not begin
until this explicit process-model deviation is approved.

## Post-Capture Flow

```text
run_overlay
  |
  +-- Ok(None) ------------------------------> exit success
  |
  +-- Err(error) ----------------------------> print error, exit failure
  |
  +-- Ok(Some(CaptureResult))
          |
          v
      auto_save(image)
          |
          +-- Err(error), Linux -------------> unsaved Result Workspace
          |
          +-- Ok(path), Linux ---------------> saved Result Workspace
          |
          +-- Err(error), macOS -------------> child stdin PNG -> one-daemon unsaved Result Workspace
          |
          +-- Ok(path), macOS ---------------> child loads PNG -> one-daemon floating thumbnail
                                                                          |
                                                                          +-- click -> saved Result Workspace
                                                                          +-- timeout/native drag success -> exit
```

## File Structure

### Create

- `crates/rollshot-app/src/storage.rs`
  - Desktop-directory resolution, timestamped unique path generation, and PNG writing.
- `crates/rollshot-app/src/post_capture.rs`
  - Platform presentation policy and orchestration after `run_overlay`.
- `crates/rollshot-app/src/post_capture_ipc.rs`
  - macOS post-capture child launch, saved-path arguments, unsaved PNG stdin transfer, waiting, and exit-status propagation.
- `crates/rollshot-app/src/result_workspace/mod.rs`
  - Result document, workspace state, iced messages/update/view, close routing, and runner.
- `crates/rollshot-app/src/result_workspace/actions.rs`
  - Clipboard, Save As completion, and Reveal system actions.
- `crates/rollshot-app/src/result_workspace/viewport.rs`
  - Pure zoom, centering, overflow, clamping, and pointer-anchor geometry.
- `crates/rollshot-app/src/macos_thumbnail.rs`
  - macOS floating-thumbnail state, presentation, timeout, hover, click, and drag-request handling.
- `crates/rollshot-app/src/macos_post_capture.rs`
  - macOS-only single iced daemon that owns thumbnail/workspace windows and transitions without recreating the event loop.
- `crates/rollshot-app/src/macos_native_drag.rs`
  - macOS-only AppKit window patch, file-drag source, active-screen placement, and native drag completion bridge.

### Modify

- `crates/rollshot-app/src/main.rs`
  - Replace post-overlay Save As handoff with the post-capture pipeline.
- `crates/rollshot-app/src/launch.rs`
  - Parse internal saved and unsaved macOS post-capture child launch modes.
- `crates/rollshot-app/src/save.rs`
  - Delete after storage and Result Workspace actions replace it.
- `crates/rollshot-app/Cargo.toml`
  - Add app-owned dependencies and the isolated macOS bridge dependencies/lint boundary.
- `crates/rollshot-iced-overlay/src/lib.rs`
  - Remove `PostOverlayRequest`, result-review/output modules, and the request field from `CaptureResult`.
- `crates/rollshot-iced-overlay/src/workspace.rs`
  - Remove `ResultReview`, output actions, and output-dependent transitions.
- `crates/rollshot-iced-overlay/src/toolbar.rs`
  - Remove Save/Copy/Close and expose capture-only actions.
- `crates/rollshot-iced-overlay/src/app.rs`
  - Remove result-review state/rendering/effects and emit capture-finalization effects only.
- `crates/rollshot-iced-overlay/src/linux_runner.rs`
  - Store the completed result and exit immediately after screenshot or scrolling finalization.
- `crates/rollshot-iced-overlay/src/macos_runner.rs`
  - Store the completed result, disable passthrough when needed, and exit immediately.
- `crates/rollshot-iced-overlay/src/screenshot.rs`
  - Construct the simplified `CaptureResult`.
- `crates/rollshot-iced-overlay/src/driver.rs`
  - Construct the simplified `CaptureResult`.
- `crates/rollshot-iced-overlay/src/output.rs`
  - Delete after output moves to `rollshot-app`.
- `crates/rollshot-iced-overlay/src/result_review.rs`
  - Delete after the independent Result Workspace replaces it.
- `crates/rollshot-iced-overlay/Cargo.toml`
  - Remove `arboard` and `rfd`.
- `Cargo.lock`
  - Record dependency ownership changes and new direct app dependencies.

## Task 1: Make The Capture Overlay Return Results Immediately

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Modify: `crates/rollshot-iced-overlay/src/workspace.rs`
- Modify: `crates/rollshot-iced-overlay/src/toolbar.rs`
- Modify: `crates/rollshot-iced-overlay/src/app.rs`
- Modify: `crates/rollshot-iced-overlay/src/linux_runner.rs`
- Modify: `crates/rollshot-iced-overlay/src/macos_runner.rs`
- Modify: `crates/rollshot-iced-overlay/src/screenshot.rs`
- Modify: `crates/rollshot-iced-overlay/src/driver.rs`
- Delete: `crates/rollshot-iced-overlay/src/output.rs`
- Delete: `crates/rollshot-iced-overlay/src/result_review.rs`
- Modify: `crates/rollshot-iced-overlay/Cargo.toml`
- Test: inline unit tests in the modified Rust modules

- [ ] **Step 1: Replace result-review tests with capture-only transition tests**

Update workspace, toolbar, app, screenshot, driver, and runner tests to require:

```rust
#[test]
fn workspace_has_only_capture_phases() {
    let phases = [
        WorkspacePhase::Selecting,
        WorkspacePhase::Selected,
        WorkspacePhase::ScrollingCapture,
    ];
    assert_eq!(phases.len(), 3);
}

#[test]
fn selected_toolbar_contains_finish_but_no_output_actions() {
    assert_eq!(
        actions_for(WorkspacePhase::Selected),
        vec![
            ToolbarAction::ScreenshotMode,
            ToolbarAction::ScrollingMode,
            ToolbarAction::Finish,
            ToolbarAction::Cancel,
        ]
    );
}

#[test]
fn screenshot_release_requests_immediate_finalization() {
    let mut state = OverlayState {
        mode: CaptureMode::Screenshot,
        crop: Some(Rectangle::new(Point::new(10.0, 10.0), Size::new(50.0, 50.0))),
        ..OverlayState::default()
    };

    let (effect, _) = update(
        &mut state,
        OverlayMessage::IcedEvent(Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        ))),
    );

    assert_eq!(effect, OverlayEffect::FinishScreenshot);
}

#[test]
fn capture_result_has_no_post_overlay_request() {
    let result = CaptureResult {
        image: image::RgbaImage::new(1, 1),
        stats: None,
    };
    assert_eq!(result.image.dimensions(), (1, 1));
}
```

Delete tests for `ResultReview`, `OutputAction`, `PostOverlayRequest`, overlay Save/Copy, and output-phase decisions.

- [ ] **Step 2: Run focused tests to verify the old contract fails**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay workspace
rtk cargo test -p rollshot-iced-overlay toolbar
rtk cargo test -p rollshot-iced-overlay app
```

Expected: FAIL until the result-review variants and output actions are removed and the new finalization effects exist.

- [ ] **Step 3: Simplify the public result and workspace contracts**

Use these capture-only contracts:

```rust
#[derive(Debug, Clone)]
pub struct CaptureResult {
    pub image: image::RgbaImage,
    pub stats: Option<rollshot_core::StitchStats>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspacePhase {
    Selecting,
    Selected,
    ScrollingCapture,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceEffect {
    None,
    ActivateMode(CaptureMode),
    StartScrolling,
    FinishScrolling,
    FinishScreenshot,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum OverlayEffect {
    None,
    BeginStitch,
    FinishScrolling,
    FinishScreenshot,
    Cancel,
    EnablePassthrough,
    DisablePassthrough,
    ActivateMode(CaptureMode),
}
```

Remove `PostOverlayRequest`, `OutputAction`, result handles/sizes, result-review rendering, and all overlay output effects. Keep `transient_error` for capture/finalization errors.

- [ ] **Step 4: Make both runners store the final result and exit**

For screenshot finalization:

```rust
let outcome = match ONE_SHOT_SLOT.lock().unwrap().take() {
    Some(capture) => crate::screenshot::finish_screenshot(
        &capture,
        crop_logical,
        overlay_logical,
    )
    .map(Some),
    None => Ok(None),
};
*RESULT_SLOT.lock().unwrap() = Some(outcome);
iced::exit()
```

For scrolling finalization:

```rust
let outcome = match DRIVER_SLOT.lock().unwrap().take() {
    Some(driver) => driver.finalize().map(Some),
    None => Err("No driver available".to_string()),
};
*RESULT_SLOT.lock().unwrap() = Some(outcome);
iced::exit()
```

On macOS, preserve the existing passthrough-disable chain before exiting. On finalization failure, preserve the existing inline capture error and keep the capture overlay open.

- [ ] **Step 5: Delete overlay result-review/output modules and dependencies**

Delete the two modules, remove their `mod` declarations, and remove:

```toml
arboard = "3.4"
rfd = "0.15"
```

from `crates/rollshot-iced-overlay/Cargo.toml`.

- [ ] **Step 6: Run overlay verification**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay
rtk cargo fmt --check
```

Expected: PASS, with capture completion returning `CaptureResult` without entering a result-review phase.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-iced-overlay Cargo.lock
rtk git commit -m "refactor(overlay): return completed captures directly"
```

## Task 2: Add Desktop Resolution, Unique Names, And Auto-Save

**Files:**
- Create: `crates/rollshot-app/src/storage.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `Cargo.lock`
- Test: `crates/rollshot-app/src/storage.rs`

- [ ] **Step 1: Write failing storage tests**

Add deterministic tests:

```rust
#[test]
fn linux_desktop_expands_home_from_user_dirs() {
    let home = Path::new("/home/noah");
    let configured = r#"XDG_DESKTOP_DIR="$HOME/Desktop""#;
    assert_eq!(
        linux_desktop_from(configured, home, |path| path == Path::new("/home/noah/Desktop")),
        PathBuf::from("/home/noah/Desktop")
    );
}

#[test]
fn linux_desktop_falls_back_to_pictures_when_configured_directory_is_missing() {
    let home = Path::new("/home/noah");
    let configured = r#"XDG_DESKTOP_DIR="$HOME/Desktop""#;
    assert_eq!(
        linux_desktop_from(configured, home, |path| path == Path::new("/home/noah/Pictures")),
        PathBuf::from("/home/noah/Pictures")
    );
}

#[test]
fn unique_capture_path_appends_numeric_suffix() {
    let dir = Path::new("/tmp");
    let path = unique_capture_path(dir, "2026-06-09 at 12.34.56", |candidate| {
        candidate.ends_with("Rollshot 2026-06-09 at 12.34.56.png")
            || candidate.ends_with("Rollshot 2026-06-09 at 12.34.56-2.png")
    });
    assert_eq!(
        path,
        PathBuf::from("/tmp/Rollshot 2026-06-09 at 12.34.56-3.png")
    );
}

#[test]
fn auto_save_does_not_create_missing_directory() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("Desktop");
    let err = auto_save_to(&image::RgbaImage::new(2, 2), &missing, "2026-06-09 at 12.34.56")
        .expect_err("missing directory must fail");
    assert!(err.contains("does not exist"));
    assert!(!missing.exists());
}
```

- [ ] **Step 2: Run app tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app storage
```

Expected: FAIL because `storage` and its helpers do not exist.

- [ ] **Step 3: Add direct dependencies and the storage API**

Add:

```toml
[dependencies]
arboard = "3.4"
chrono = "0.4"
dirs = "6"

[dev-dependencies]
tempfile = "3"
```

Implement:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Macos,
}

impl Platform {
    pub fn current() -> Result<Self, String>;
}

pub fn default_output_dir(platform: Platform) -> Result<PathBuf, String>;
pub fn linux_desktop_from(
    user_dirs: &str,
    home: &Path,
    is_dir: impl Fn(&Path) -> bool,
) -> PathBuf;
pub fn unique_capture_path(
    dir: &Path,
    timestamp: &str,
    exists: impl Fn(&Path) -> bool,
) -> PathBuf;
pub fn write_png(image: &RgbaImage, path: &Path) -> Result<(), String>;
pub fn auto_save(image: &RgbaImage, platform: Platform) -> Result<PathBuf, String>;
pub fn auto_save_to(image: &RgbaImage, dir: &Path, timestamp: &str) -> Result<PathBuf, String>;
```

`auto_save` formats `chrono::Local::now()` with `%Y-%m-%d at %H.%M.%S`, verifies the chosen directory already exists, selects a unique path, and calls `write_png`.

- [ ] **Step 4: Run storage tests**

Run:

```bash
rtk cargo test -p rollshot-app storage
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/storage.rs crates/rollshot-app/src/main.rs crates/rollshot-app/Cargo.toml Cargo.lock
rtk git commit -m "feat(app): add desktop auto-save storage"
```

## Task 3: Add The Result Document, Actions, Messages, And Close Decisions

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/mod.rs`
- Create: `crates/rollshot-app/src/result_workspace/actions.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Delete: `crates/rollshot-app/src/save.rs`
- Test: inline unit tests in the new modules

- [ ] **Step 1: Write failing document and close-decision tests**

Add:

```rust
#[test]
fn saved_document_closes_immediately() {
    let document = ResultDocument::saved(image(), PathBuf::from("/tmp/result.png"));
    assert_eq!(close_decision(&document), CloseDecision::Close);
}

#[test]
fn unsaved_document_requests_discard_confirmation() {
    let document = ResultDocument::unsaved(image());
    assert_eq!(close_decision(&document), CloseDecision::ConfirmDiscard);
}

#[test]
fn save_as_success_updates_saved_path_and_message() {
    let mut state = ResultWorkspace::new(ResultDocument::unsaved(image()), None);
    state.apply_save_as(Ok(Some(PathBuf::from("/tmp/result.png"))));
    assert_eq!(
        state.document.saved_path.as_deref(),
        Some(Path::new("/tmp/result.png"))
    );
    assert!(matches!(state.message, Some(InlineMessage::Success { .. })));
}

#[test]
fn saved_workspace_starts_with_saved_path_message() {
    let path = PathBuf::from("/tmp/result.png");
    let state = ResultWorkspace::new(ResultDocument::saved(image(), path.clone()), None);
    assert_eq!(state.message_text(), Some(format!("Saved to {}", path.display())));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace
```

Expected: FAIL because the Result Workspace modules do not exist.

- [ ] **Step 3: Implement the concrete document and message model**

Use:

```rust
pub struct ResultDocument {
    pub source_image: image::RgbaImage,
    pub saved_path: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineMessage {
    Success {
        text: String,
        expires_at: std::time::Instant,
    },
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    Close,
    ConfirmDiscard,
}

pub fn close_decision(document: &ResultDocument) -> CloseDecision {
    if document.saved_path.is_some() {
        CloseDecision::Close
    } else {
        CloseDecision::ConfirmDiscard
    }
}
```

`ResultWorkspace::new` builds the iced image handle once and initializes a saved-path success message or the supplied auto-save error.

- [ ] **Step 4: Implement app-owned system actions**

Implement:

```rust
pub fn copy_image(image: &RgbaImage) -> Result<(), String>;
pub async fn prompt_save_as(
    default_dir: PathBuf,
    default_name: String,
) -> Option<PathBuf>;
pub fn write_save_as(image: &RgbaImage, path: &Path) -> Result<PathBuf, String>;
pub fn reveal(path: &Path) -> Result<(), String>;
```

Use `arboard` for full-resolution RGBA clipboard data. Use `rfd::AsyncFileDialog` with `.set_directory(default_dir)`, `.set_file_name(default_name)`, and a PNG filter. Use `open -R <path>` on macOS and `xdg-open <parent>` on Linux.

- [ ] **Step 5: Route both close sources through one decision**

Use one message for both sources:

```rust
#[derive(Debug, Clone)]
pub enum Message {
    RequestClose,
    ConfirmDiscard,
    KeepUnsaved,
    DismissMessage,
    Copy,
    CopyFinished(Result<(), String>),
    SaveAs,
    SavePathChosen(Option<PathBuf>),
    SaveFinished(Result<PathBuf, String>),
    Reveal,
    RevealFinished(Result<(), String>),
    Tick(std::time::Instant),
}
```

`Message::RequestClose` closes a saved document immediately and sets `confirming_discard = true` for an unsaved document. `Message::ConfirmDiscard` exits; `Message::KeepUnsaved` returns to the workspace. Errors remain until dismissed or replaced; success messages expire on `Tick`.

- [ ] **Step 6: Run Result Workspace model/action tests**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace crates/rollshot-app/src/main.rs crates/rollshot-app/src/save.rs
rtk git commit -m "feat(app): add result document and actions"
```

## Task 4: Implement Pure Viewport And Zoom Geometry

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/viewport.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Test: `crates/rollshot-app/src/result_workspace/viewport.rs`

- [ ] **Step 1: Write failing viewport tests**

Cover every spec rule:

```rust
#[test]
fn default_modes_match_image_shape() {
    assert_eq!(default_zoom(Size::new(1200.0, 800.0)), ZoomMode::FitWindow);
    assert_eq!(default_zoom(Size::new(800.0, 2401.0)), ZoomMode::FitWidth);
    assert_eq!(default_zoom(Size::new(2401.0, 800.0)), ZoomMode::FitHeight);
}

#[test]
fn fit_scales_use_the_requested_axis() {
    let image = Size::new(1000.0, 2000.0);
    let viewport = Size::new(500.0, 600.0);
    assert_eq!(scale_for(ZoomMode::FitWidth, image, viewport), 0.5);
    assert_eq!(scale_for(ZoomMode::FitHeight, image, viewport), 0.3);
    assert_eq!(scale_for(ZoomMode::FitWindow, image, viewport), 0.3);
}

#[test]
fn fixed_steps_clamp_to_supported_range() {
    assert_eq!(step_zoom(ZoomMode::Custom(25), ZoomDirection::Out), ZoomMode::Custom(25));
    assert_eq!(step_zoom(ZoomMode::Custom(100), ZoomDirection::In), ZoomMode::Custom(125));
    assert_eq!(step_zoom(ZoomMode::Custom(400), ZoomDirection::In), ZoomMode::Custom(400));
}

#[test]
fn smaller_images_are_centered_without_overflow() {
    let geometry = geometry_for(
        ZoomMode::ActualSize,
        Size::new(300.0, 200.0),
        Size::new(800.0, 600.0),
    );
    assert_eq!(geometry.image_origin, Point::new(250.0, 200.0));
    assert_eq!(geometry.max_scroll, Vector::new(0.0, 0.0));
    assert!(!geometry.horizontal_overflow);
    assert!(!geometry.vertical_overflow);
}

#[test]
fn pointer_anchor_preserves_the_image_point_when_possible() {
    let old = geometry_for(
        ZoomMode::Custom(100),
        Size::new(1000.0, 2000.0),
        Size::new(500.0, 500.0),
    );
    let new = geometry_for(
        ZoomMode::Custom(200),
        Size::new(1000.0, 2000.0),
        Size::new(500.0, 500.0),
    );
    assert_eq!(
        anchored_scroll(
            Vector::new(100.0, 300.0),
            Point::new(250.0, 250.0),
            old,
            new,
        ),
        Vector::new(450.0, 850.0)
    );
}
```

- [ ] **Step 2: Run viewport tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app viewport
```

Expected: FAIL because the viewport module does not exist.

- [ ] **Step 3: Implement zoom modes and geometry**

Use:

```rust
pub const ZOOM_STEPS: [u16; 10] = [25, 33, 50, 67, 100, 125, 150, 200, 300, 400];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomMode {
    FitWindow,
    FitWidth,
    FitHeight,
    ActualSize,
    Custom(u16),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportGeometry {
    pub scale: f32,
    pub rendered_size: Size,
    pub content_size: Size,
    pub image_origin: Point,
    pub max_scroll: Vector,
    pub horizontal_overflow: bool,
    pub vertical_overflow: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportState {
    pub zoom: ZoomMode,
    pub scroll_offset: iced::Vector,
}
```

Implement `default_zoom`, `scale_for`, `geometry_for`, `step_zoom`, `anchored_scroll`, and `clamp_scroll`. Fit modes recompute from the current viewport; custom/actual-size modes do not change percentage on resize.

- [ ] **Step 4: Run viewport tests**

Run:

```bash
rtk cargo test -p rollshot-app viewport
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace
rtk git commit -m "feat(app): add result viewport geometry"
```

## Task 5: Build The Independent Result Workspace Window

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/actions.rs`
- Modify: `crates/rollshot-app/src/result_workspace/viewport.rs`
- Modify: `crates/rollshot-app/Cargo.toml`
- Test: inline unit tests in Result Workspace modules

- [ ] **Step 1: Write failing update-state integration tests**

Add tests for status controls, resize behavior, scroll routing, and window-close routing:

```rust
#[test]
fn fit_height_button_selects_fit_height() {
    let mut state = workspace();
    update(&mut state, Message::SetZoom(ZoomMode::FitHeight));
    assert_eq!(state.viewport.zoom, ZoomMode::FitHeight);
}

#[test]
fn resize_keeps_custom_zoom_percentage() {
    let mut state = workspace();
    state.viewport.zoom = ZoomMode::Custom(150);
    state.apply_viewport_bounds(Size::new(900.0, 700.0));
    assert_eq!(state.viewport.zoom, ZoomMode::Custom(150));
}

#[test]
fn operating_system_close_uses_unsaved_close_confirmation() {
    let mut state = unsaved_workspace();
    update(&mut state, Message::RequestClose);
    assert!(state.confirming_discard);
}

#[test]
fn reveal_is_disabled_without_a_saved_path() {
    assert!(!unsaved_workspace().can_reveal());
    assert!(saved_workspace().can_reveal());
}
```

- [ ] **Step 2: Run focused tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace
```

Expected: FAIL until the full update state and view helpers exist.

- [ ] **Step 3: Implement the ordinary decorated iced window**

Expose the reusable state/update/view APIs plus the Linux runner:

```rust
pub(crate) fn update(state: &mut ResultWorkspace, message: Message) -> iced::Task<Message>;
pub(crate) fn view(state: &ResultWorkspace) -> iced::Element<'_, Message>;
pub(crate) fn subscription(state: &ResultWorkspace) -> iced::Subscription<Message>;
#[cfg(target_os = "linux")]
pub fn run(document: ResultDocument, initial_error: Option<String>) -> Result<(), String>;
```

Run the Linux Result Workspace with:

```rust
let boot = std::sync::Arc::new(std::sync::Mutex::new(Some((
    document,
    initial_error,
))));

iced::application(
    move || {
        let (document, initial_error) = boot
            .lock()
            .unwrap()
            .take()
            .expect("Result Workspace booted more than once");
        ResultWorkspace::new(document, initial_error)
    },
    update,
    view,
)
.title(|state| state.window_title())
.subscription(subscription)
.window(iced::window::Settings {
    size: iced::Size::new(1100.0, 760.0),
    min_size: Some(iced::Size::new(640.0, 420.0)),
    decorations: true,
    resizable: true,
    exit_on_close_request: false,
    ..Default::default()
})
.run()
```

Subscribe to `iced::window::close_requests()` mapped to `Message::RequestClose`, ignored cursor/modifier events, and a timer only while a success message has an expiry.

The macOS post-capture daemon added in Task 7 reuses the same
`ResultWorkspace`, `Message`, `update`, `view`, and `subscription` APIs instead
of starting another application runner.

- [ ] **Step 4: Render the specified layout and controlled viewport**

Render:

```text
Close | filename/Unsaved capture | Copy | Save As | Reveal
inline message + Dismiss for errors
two-axis scrollable image canvas
dimensions | active zoom | Fit Width | Fit Window | Fit Height | 100% | - | +
```

Use one `scrollable::Id`, `scrollable::Direction::Both`, and embedded thick scrollbars:

```rust
let scrollbar = scrollable::Scrollbar::new()
    .width(14)
    .scroller_width(14)
    .spacing(2);

scrollable(content)
    .id(state.scrollable_id.clone())
    .direction(scrollable::Direction::Both {
        vertical: scrollbar,
        horizontal: scrollbar,
    })
    .on_scroll(Message::ViewportChanged)
```

Build content at `geometry.content_size`, place the image at `geometry.image_origin`, and only render a visible scrollbar on an overflowing axis by using `Scrollbar::hidden()` for the non-overflowing axis.

- [ ] **Step 5: Implement wheel, modifier, zoom, and resize behavior**

Track the last pointer position and current keyboard modifiers. Route `mouse_area(...).on_scroll(Message::WheelScrolled)` as follows:

```rust
if zoom_modifier(state.modifiers) {
    state.zoom_at_pointer(direction, state.pointer_position)
} else if state.modifiers.shift() {
    state.scroll_by(Vector::new(-wheel_y, -wheel_x))
} else {
    state.scroll_by(Vector::new(-wheel_x, -wheel_y))
}
```

Use `iced::widget::operation::scroll_to` after zoom and `scroll_by` after manual wheel routing. On `ViewportChanged`, store `viewport.bounds().size()` and `viewport.absolute_offset()` so fit modes and pointer anchoring use actual canvas bounds.

- [ ] **Step 6: Wire actions and messages**

- Copy uses the original `source_image`, then shows `Copied image` or a persistent error.
- Save As first awaits `prompt_save_as`, writes PNG off the UI update path with `Task::perform`, updates `saved_path`, enables Reveal, and shows `Saved to <path>`.
- Reveal leaves the workspace open and shows only failures.
- Saved Close exits immediately.
- Unsaved Close and OS close show the same discard modal.

- [ ] **Step 7: Run Result Workspace tests and package checks**

Run:

```bash
rtk cargo test -p rollshot-app
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace crates/rollshot-app/Cargo.toml Cargo.lock
rtk git commit -m "feat(app): add independent result workspace"
```

## Task 6: Add Post-Capture Policy, Linux Flow, And macOS Child Handoff

**Files:**
- Create: `crates/rollshot-app/src/post_capture.rs`
- Create: `crates/rollshot-app/src/post_capture_ipc.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-app/src/launch.rs`
- Test: `crates/rollshot-app/src/post_capture.rs`
- Test: `crates/rollshot-app/src/post_capture_ipc.rs`
- Test: `crates/rollshot-app/src/main.rs`

- [ ] **Step 1: Write failing policy, cancellation, and child-mode tests**

Add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presentation {
    LinuxSavedWorkspace(PathBuf),
    LinuxUnsavedWorkspace(String),
    MacosSavedThumbnail(PathBuf),
    MacosUnsavedWorkspace(String),
}

#[test]
fn platform_policy_selects_the_required_presentations() {
    assert_eq!(
        select_presentation(Platform::Linux, Ok(PathBuf::from("/tmp/a.png"))),
        Presentation::LinuxSavedWorkspace(PathBuf::from("/tmp/a.png"))
    );
    assert_eq!(
        select_presentation(Platform::Macos, Ok(PathBuf::from("/tmp/a.png"))),
        Presentation::MacosSavedThumbnail(PathBuf::from("/tmp/a.png"))
    );
    assert_eq!(
        select_presentation(Platform::Linux, Err("disk full".to_string())),
        Presentation::LinuxUnsavedWorkspace("disk full".to_string())
    );
    assert_eq!(
        select_presentation(Platform::Macos, Err("disk full".to_string())),
        Presentation::MacosUnsavedWorkspace("disk full".to_string())
    );
}

#[test]
fn cancelled_capture_has_no_post_capture_presentation() {
    assert!(matches!(
        capture_completion(None),
        CaptureCompletion::Cancelled
    ));
}

#[test]
fn internal_child_modes_parse() {
    assert_eq!(
        launch::parse_launch_args(["rollshot-app", "--post-capture-saved", "/tmp/a.png"])
            .unwrap(),
        LaunchMode::PostCaptureSaved(PathBuf::from("/tmp/a.png"))
    );
    assert_eq!(
        launch::parse_launch_args(["rollshot-app", "--post-capture-unsaved", "disk full"])
            .unwrap(),
        LaunchMode::PostCaptureUnsaved {
            error: "disk full".to_string(),
        }
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app post_capture
rtk cargo test -p rollshot-app launch
```

Expected: FAIL because the policy, child handoff, and internal launch modes do not exist.

- [ ] **Step 3: Implement platform policy and Linux orchestration**

Implement:

```rust
pub enum CaptureCompletion {
    Present(rollshot_iced_overlay::CaptureResult),
    Cancelled,
}

pub fn capture_completion(
    result: Option<rollshot_iced_overlay::CaptureResult>,
) -> CaptureCompletion;

pub fn select_presentation(
    platform: Platform,
    auto_save: Result<PathBuf, String>,
) -> Presentation;

pub fn handle_capture(result: rollshot_iced_overlay::CaptureResult) -> Result<(), String> {
    let platform = Platform::current()?;
    let auto_save = storage::auto_save(&result.image, platform);
    match select_presentation(platform, auto_save) {
        Presentation::LinuxSavedWorkspace(path) => {
            result_workspace::run(ResultDocument::saved(result.image, path), None)
        }
        Presentation::LinuxUnsavedWorkspace(error) => {
            result_workspace::run(ResultDocument::unsaved(result.image), Some(error))
        }
        Presentation::MacosSavedThumbnail(path) => post_capture_ipc::run_saved_child(&path),
        Presentation::MacosUnsavedWorkspace(error) => {
            post_capture_ipc::run_unsaved_child(&result.image, &error)
        }
    }
}
```

On non-macOS builds, child-handoff functions return an explicit unsupported error if called.

- [ ] **Step 4: Implement the macOS child IPC**

Expose:

```rust
pub fn run_saved_child(path: &Path) -> Result<(), String>;
pub fn run_unsaved_child(image: &RgbaImage, error: &str) -> Result<(), String>;
pub fn read_unsaved_png(reader: impl Read) -> Result<RgbaImage, String>;
```

`run_saved_child` launches `std::env::current_exe()` with
`--post-capture-saved <path>` and waits for its status.

`run_unsaved_child` launches `std::env::current_exe()` with
`--post-capture-unsaved <error>`, pipes stdin, encodes the image as PNG into the
pipe, closes stdin, and waits. Treat spawn, encode, pipe, and non-zero child
status failures as application errors. Add a round-trip test that encodes a
small RGBA image into a byte vector and proves `read_unsaved_png` restores its
dimensions and pixels.

- [ ] **Step 5: Add internal launch modes and replace the old main handoff**

Extend `LaunchMode`:

```rust
pub enum LaunchMode {
    Capture(InteractiveLaunchOptions),
    PostCaptureSaved(PathBuf),
    PostCaptureUnsaved { error: String },
}
```

Use:

```rust
fn run_iced_capture(options: InteractiveLaunchOptions) -> Result<(), String> {
    let config = rollshot_iced_overlay::OverlayConfig {
        backend: options.backend,
        fps: options.fps,
        show_cursor: options.show_cursor,
        initial_mode: options.initial_mode,
    };

    match post_capture::capture_completion(
        rollshot_iced_overlay::run_overlay(config).map_err(|e| e.to_string())?,
    ) {
        post_capture::CaptureCompletion::Present(result) => post_capture::handle_capture(result),
        post_capture::CaptureCompletion::Cancelled => Ok(()),
    }
}
```

Route `PostCaptureSaved` and `PostCaptureUnsaved` to the macOS one-daemon host
added in Task 7. Before that host lands, those internal modes return
`"macOS post-capture child requires the one-daemon host"` so this task remains
compilable and testable. Delete the old `PostOverlayAction::SaveAs`,
`post_overlay_request` fixtures, and `save.rs`.

- [ ] **Step 6: Run app and overlay integration tests**

Run:

```bash
rtk cargo test -p rollshot-app
rtk cargo test -p rollshot-iced-overlay
```

Expected: PASS, with Linux selecting a local Result Workspace and macOS
selecting a blocking child handoff that can transfer an unsaved PNG without a
temporary file.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app
rtk git commit -m "feat(app): add post-capture policy and child handoff"
```

## Task 7: Build The One-Daemon macOS Thumbnail And Workspace Host

**Files:**
- Create: `crates/rollshot-app/src/macos_thumbnail.rs`
- Create: `crates/rollshot-app/src/macos_post_capture.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Test: `crates/rollshot-app/src/macos_thumbnail.rs`
- Test: `crates/rollshot-app/src/macos_post_capture.rs`

- [ ] **Step 1: Write failing thumbnail and host-transition tests**

Keep timer, interaction, and phase decisions pure:

```rust
#[test]
fn thumbnail_expires_after_eight_unpaused_seconds() {
    let start = Instant::now();
    let mut timer = ThumbnailTimer::new(start, Duration::from_secs(8));
    assert!(!timer.tick(start + Duration::from_millis(7_999)));
    assert!(timer.tick(start + Duration::from_secs(8)));
}

#[test]
fn hover_and_drag_pause_timeout() {
    let start = Instant::now();
    let mut timer = ThumbnailTimer::new(start, Duration::from_secs(8));
    timer.set_hovering(true, start + Duration::from_secs(4));
    assert!(!timer.tick(start + Duration::from_secs(20)));
    timer.set_hovering(false, start + Duration::from_secs(20));
    assert!(timer.tick(start + Duration::from_secs(24)));
}

#[test]
fn release_below_drag_threshold_opens_workspace() {
    assert_eq!(
        release_action(Point::new(10.0, 10.0), Point::new(12.0, 12.0), false),
        ThumbnailAction::OpenWorkspace
    );
}

#[test]
fn saved_input_starts_in_thumbnail_and_click_transitions_to_workspace() {
    let mut state = MacPostCapture::from_saved(document());
    assert!(matches!(state.phase, Phase::Thumbnail(_)));
    state.open_workspace();
    assert!(matches!(state.phase, Phase::Workspace(_)));
}

#[test]
fn unsaved_input_starts_in_workspace_with_error() {
    let state = MacPostCapture::from_unsaved(image(), "disk full".to_string());
    assert!(matches!(state.phase, Phase::Workspace(_)));
    assert_eq!(state.workspace().unwrap().message_text(), Some("disk full".to_string()));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app macos_thumbnail
rtk cargo test -p rollshot-app macos_post_capture
```

Expected: FAIL because the thumbnail component and one-daemon host do not exist.

- [ ] **Step 3: Implement thumbnail state and view**

Use:

```rust
pub const DRAG_THRESHOLD_POINTS: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailAction {
    OpenWorkspace,
    StartNativeDrag,
    KeepOpen,
    Close,
}

pub struct ThumbnailTimer {
    remaining: Duration,
    last_tick: Instant,
    hovering: bool,
    dragging: bool,
}

impl ThumbnailTimer {
    pub fn new(now: Instant, duration: Duration) -> Self;
    pub fn set_hovering(&mut self, hovering: bool, now: Instant);
    pub fn set_dragging(&mut self, dragging: bool, now: Instant);
    pub fn tick(&mut self, now: Instant) -> bool;
}

pub struct ThumbnailState {
    image_handle: iced::widget::image::Handle,
    saved_path: PathBuf,
    timer: ThumbnailTimer,
    press_origin: Option<Point>,
    hovering: bool,
    dragging: bool,
    native_drag_status: Option<std::sync::Arc<std::sync::atomic::AtomicU8>>,
}

pub fn release_action(start: Point, end: Point, drag_started: bool) -> ThumbnailAction;
```

Render a compact card with the image preview, `Saved`, and `Drag or click`.
Mouse enter/leave pauses/resumes the timer. A release without drag requests the
workspace; timeout requests host exit.

- [ ] **Step 4: Implement the single iced daemon host**

Use:

```rust
pub enum Phase {
    Thumbnail(ThumbnailState),
    Workspace(result_workspace::ResultWorkspace),
}

pub struct MacPostCapture {
    document: Option<ResultDocument>,
    phase: Phase,
    thumbnail_window: Option<iced::window::Id>,
    workspace_window: Option<iced::window::Id>,
}

impl MacPostCapture {
    pub fn from_saved(document: ResultDocument) -> Self;
    pub fn from_unsaved(image: RgbaImage, error: String) -> Self;
    pub fn open_workspace(&mut self);
    pub fn workspace(&self) -> Option<&result_workspace::ResultWorkspace>;
}

pub fn run_saved(path: &Path) -> Result<(), String>;
pub fn run_unsaved(image: RgbaImage, error: String) -> Result<(), String>;
```

`run_saved` losslessly loads the saved PNG and starts with a thumbnail.
`run_unsaved` starts directly with a Result Workspace. Both call one
`iced::daemon` runner. The host opens a frameless, transparent, always-on-top,
non-resizable thumbnail at a deterministic centered position in this task; Task
8 replaces that origin with the active display's lower-right position. On click
it closes that window and opens one decorated Result Workspace window in the
same daemon. On timeout, native drag success, or final workspace close, it
closes all windows and returns `iced::exit()`.

Map the reusable Result Workspace messages, update tasks, view, subscriptions,
and close behavior into the host instead of calling `result_workspace::run`.

- [ ] **Step 5: Route internal child launch modes to the host**

In `main`:

```rust
LaunchMode::PostCaptureSaved(path) => macos_post_capture::run_saved(&path),
LaunchMode::PostCaptureUnsaved { error } => {
    let image = post_capture_ipc::read_unsaved_png(std::io::stdin().lock())?;
    macos_post_capture::run_unsaved(image, error)
}
```

On non-macOS targets, these arms return an explicit unsupported error. If the
saved child cannot load its durable PNG or create the thumbnail, print the
error and exit; do not reinterpret it as an unsaved capture.

- [ ] **Step 6: Run lifecycle and host tests**

Run:

```bash
rtk cargo test -p rollshot-app macos_thumbnail
rtk cargo test -p rollshot-app macos_post_capture
rtk cargo test -p rollshot-app
```

Expected: PASS on Linux using pure lifecycle/phase tests; the actual daemon
runner remains macOS-gated.

Register `macos_thumbnail` and the pure phase model on every target so their
tests run on Linux. Gate AppKit calls and the actual daemon runner to macOS.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/macos_thumbnail.rs crates/rollshot-app/src/macos_post_capture.rs crates/rollshot-app/src/main.rs crates/rollshot-app/src/result_workspace
rtk git commit -m "feat(app): add one-daemon macos post-capture host"
```

## Task 8: Add The AppKit Native File Drag Bridge

**Files:**
- Create: `crates/rollshot-app/src/macos_native_drag.rs`
- Modify: `crates/rollshot-app/src/macos_thumbnail.rs`
- Modify: `crates/rollshot-app/src/macos_post_capture.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `Cargo.lock`
- Test: pure helper tests in `crates/rollshot-app/src/macos_native_drag.rs`

- [ ] **Step 1: Write failing pure bridge-helper tests**

Add:

```rust
#[test]
fn lower_right_origin_uses_screen_frame_and_margin() {
    assert_eq!(
        thumbnail_origin(
            ScreenFrame::new(-1440.0, 0.0, 1440.0, 900.0),
            Size::new(280.0, 220.0),
            24.0,
        ),
        Point::new(-304.0, 24.0)
    );
}

#[test]
fn drag_operation_maps_none_to_cancelled_and_copy_to_success() {
    assert_eq!(drag_result(false), NativeDragResult::Cancelled);
    assert_eq!(drag_result(true), NativeDragResult::Succeeded);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app macos_native_drag
```

Expected: FAIL because the bridge module does not exist.

- [ ] **Step 3: Add the macOS dependencies and isolated unsafe boundary**

Add:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.6"
objc2-app-kit = "0.3"
objc2-foundation = "0.3"
raw-window-handle = "0.6"

[lints.rust]
unsafe_code = "deny"
```

Replace `[lints] workspace = true` in `rollshot-app` because workspace `forbid` cannot be narrowed for the audited bridge. Keep unsafe code limited to functions/modules annotated `#[allow(unsafe_code)]`.

- [ ] **Step 4: Implement active-screen placement and window patching**

Implement:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenFrame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl ScreenFrame {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self;
}

pub fn thumbnail_origin(frame: ScreenFrame, size: iced::Size, margin: f32) -> iced::Point;
pub fn drag_result(succeeded: bool) -> NativeDragResult;
pub fn active_screen_thumbnail_origin(size: iced::Size, margin: f32) -> Result<iced::Point, String>;
pub fn patch_thumbnail_window(handle: &dyn iced::window::Window) -> Result<(), String>;
```

Use `NSEvent::mouseLocation()` and `NSScreen::screens(MainThreadMarker)` to find the screen containing the pointer, then place the card at the lower-right with a 24-point margin. Patch the iced `NSWindow` to remove shadow/title behavior as needed, join all spaces, remain always-on-top, and accept mouse events.

Replace Task 7's centered thumbnail origin with
`active_screen_thumbnail_origin`. After the thumbnail window opens, call
`iced::window::run(id, patch_thumbnail_window)` before accepting interactions.
Treat origin lookup, window creation, or patch failure as thumbnail creation
failure so the durable saved file remains and the child exits with an error.

- [ ] **Step 5: Implement the native drag source**

Expose:

```rust
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeDragResult {
    Pending,
    Succeeded,
    Cancelled,
}

pub fn begin_file_drag(
    handle: &dyn iced::window::Window,
    saved_path: &Path,
    status: Arc<AtomicU8>,
) -> Result<(), String>;
```

Inside the audited AppKit bridge:

1. Convert the iced raw AppKit handle to its `NSView`.
2. Read `NSApplication::currentEvent()` and require a left-mouse drag event.
3. Create an `NSURL::fileURLWithPath`.
4. Create `NSDraggingItem::initWithPasteboardWriter` from the URL.
5. Set the dragging frame to the thumbnail bounds.
6. Create an `NSDraggingSource` object whose source mask is `NSDragOperation::Copy`.
7. In `draggingSession:endedAtPoint:operation:`, store succeeded for non-`None` operations and cancelled for `None`.
8. Call `beginDraggingSessionWithItems:event:source:` on the thumbnail view.

Register `macos_native_drag` on every target so `thumbnail_origin` and
`drag_result` tests run on Linux. Keep AppKit imports and bridge functions under
`#[cfg(target_os = "macos")]`; provide an explicit unsupported stub for
`begin_file_drag` on other targets.

- [ ] **Step 6: Connect drag requests and completion to the thumbnail**

When movement crosses four logical points:

```rust
state.dragging = true;
state.timer.set_dragging(true, now);
let status = Arc::new(AtomicU8::new(NativeDragResult::Pending as u8));
state.native_drag_status = Some(Arc::clone(&status));
return iced::window::run(window_id, move |window| {
    macos_native_drag::begin_file_drag(window, &saved_path, status)
})
.map(macos_post_capture::Message::NativeDragStarted);
```

On host tick, poll the atomic status. Success closes the thumbnail and exits the
daemon. Cancellation or bridge failure clears dragging and restarts the
eight-second countdown.

- [ ] **Step 7: Run local and macOS compile verification**

Run:

```bash
rtk cargo test -p rollshot-app
rtk cargo fmt --check
```

Expected: PASS on the current platform.

On macOS, additionally run:

```bash
rtk cargo test -p rollshot-app
rtk cargo check -p rollshot-app
```

Expected: PASS with the AppKit bridge compiled.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-app/src/macos_native_drag.rs crates/rollshot-app/src/macos_thumbnail.rs crates/rollshot-app/src/macos_post_capture.rs crates/rollshot-app/src/main.rs crates/rollshot-app/Cargo.toml Cargo.lock
rtk git commit -m "feat(app): add macos native thumbnail drag"
```

## Task 9: Verify End-To-End Behavior And Repository Health

**Files:**
- Verify only

- [ ] **Step 1: Verify the refactoring boundaries**

Run:

```bash
rtk rg -n "ResultReview|PostOverlayRequest|OutputAction|PerformOutput|result_handle|result_size" crates/rollshot-iced-overlay
rtk rg -n "arboard|rfd" crates/rollshot-iced-overlay
rtk git diff -- crates/rollshot-tauri-app
```

Expected: no result-review/output symbols or output dependencies remain in `rollshot-iced-overlay`, and no Tauri files changed.

- [ ] **Step 2: Run focused integration suites**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay
rtk cargo test -p rollshot-app
```

Expected: PASS, including direct overlay completion, cancellation, auto-save policy, saved-path updates, close routing, zoom geometry, and thumbnail lifecycle.

- [ ] **Step 3: Run full repository verification**

Run:

```bash
rtk cargo test
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk git diff --check
```

Expected: all commands pass. Stitching benchmarks are not required because no `rollshot-core` stitching path changes.

- [ ] **Step 4: Run Linux runtime verification**

Verify:

1. Screenshot and scrolling capture both close the overlay, auto-save to the resolved desktop directory, and open a decorated saved Result Workspace.
2. A forced auto-save failure opens an unsaved Result Workspace and preserves Copy, Save As, and discard confirmation.
3. Toolbar Close and title-bar close show the same unsaved confirmation.
4. Saved Result Workspace initially shows its saved path and Reveal opens the containing folder.
5. Normal, vertical-long, and horizontal-long images select Fit Window, Fit Width, and Fit Height respectively.
6. Fit Width, Fit Window, Fit Height, 100%, zoom steps, pointer anchoring, Shift-scroll, resize, and thick overflow-only scrollbars behave as specified.

- [ ] **Step 5: Run macOS runtime verification**

Verify:

1. Auto-save writes a unique Desktop PNG before the thumbnail appears.
2. The thumbnail is near the active display's lower-right corner and expires after eight unpaused seconds.
3. Hover and native drag pause the timer; cancelled drag restarts it.
4. Native file drag succeeds into Finder and Notes and closes the thumbnail.
5. Click without dragging closes the thumbnail and opens a saved Result Workspace.
6. Forced auto-save failure skips the thumbnail and opens an unsaved Result Workspace.
7. Cmd-wheel zoom and Cmd-W unsaved-close confirmation work.
8. The original capture process waits while the transient post-capture child is open, so the CLI remains blocked.
9. Thumbnail-to-workspace transition stays inside one child daemon and does not produce `EventLoop can't be recreated`.
10. Concurrent captures create independent child daemons/thumbnails with no shared queue or coordination.
