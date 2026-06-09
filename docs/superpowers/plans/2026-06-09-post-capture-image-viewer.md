# Post-Capture Image Viewer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move completed captures out of the capture overlay, auto-save every successful capture, open a full Result Workspace on Linux or auto-save failure, and show a draggable floating thumbnail before the Result Workspace on macOS.

**Architecture:** `rollshot-iced-overlay` becomes capture-only. Linux keeps its blocking layer-shell runner and returns a plain `CaptureResult`; macOS exposes an embeddable capture component that reports completion to a single product daemon owned by `rollshot-app`. That one macOS daemon transitions through capture overlay, floating thumbnail, and Result Workspace windows without recreating winit's event loop; the Result Workspace keeps document state, system actions, and pure viewport geometry in focused reusable modules, while a narrow AppKit bridge performs native file drag.

**Tech Stack:** Rust 2021, iced 0.14, `image`, `rfd`, `arboard`, `chrono`, `dirs`, macOS `objc2`/AppKit, existing Rollshot capture and overlay crates.

---

## Implementation Assumptions

- Treat Linux `XDG_DESKTOP_DIR` as the value from `${XDG_CONFIG_HOME:-$HOME/.config}/user-dirs.dirs`, expanding only `$HOME` and `${HOME}`. Use `~/Pictures` when that value is absent, malformed, relative, or points to a missing directory. Do not create either directory.
- Use local wall-clock time for `Rollshot YYYY-MM-DD at HH.MM.SS.png`.
- The pinned winit 0.30 backend rejects a second event loop in one process with `EventLoopError::RecreationAttempt`. Linux may start its one ordinary iced Result Workspace after the layer-shell overlay exits. macOS therefore uses one long-lived iced daemon in `rollshot-app`; capture, thumbnail, and Result Workspace are phases/windows inside that daemon.
- The macOS daemon retains the completed `RgbaImage` in memory while auto-save and post-capture UI run. No image serialization, temporary transfer file, child process, or IPC is used.
- The viewer image handle is built once from its `RgbaImage`.
- Use `rfd::AsyncFileDialog` for Result Workspace Save As so the native dialog is not opened synchronously inside iced's event loop.
- Implement unsaved-close confirmation as an iced modal containing the exact prompt `Discard unsaved capture?`; both the toolbar Close action and `window::Event::CloseRequested` route to the same state transition.
- Iced does not expose a named AppKit drag-threshold API. Use a tested four-logical-point threshold before calling AppKit's native `beginDraggingSessionWithItems:event:source:`. The transfer after that threshold is a real native file drag.
- Keep the AppKit bridge inside `rollshot-app`. Change that package's unsafe lint from inherited `forbid` to package-local `deny`, and use one explicitly annotated `#[allow(unsafe_code)]` bridge function/module, matching the existing `rollshot-iced-overlay/src/macos_window.rs` pattern.
- Do not modify `crates/rollshot-tauri-app`.

## macOS Ownership Boundary

The current `rollshot-iced-overlay::macos_runner` owns and terminates an iced
daemon. The long-term architecture moves daemon ownership to `rollshot-app`.
`rollshot-iced-overlay` keeps capture-specific state, rendering, resource
acquisition, passthrough, and controls-window behavior behind an embeddable
`macos_capture` component API. It does not know about auto-save, thumbnails,
Result Workspace, or product process lifetime.

`rollshot-app::macos_product` owns the product-level phase machine and maps
capture-component completion into auto-save and post-capture presentation. This
preserves the approved one-process-per-capture model and removes the need for
IPC.

## Post-Capture Flow

```text
Linux:
run_overlay -> CaptureResult -> auto_save
  +-- error -> unsaved Result Workspace
  +-- saved -> saved Result Workspace

macOS single rollshot-app daemon:
Capture(MacCaptureComponent)
  +-- cancelled -> exit
  +-- failed -> existing capture error behavior
  +-- completed CaptureResult -> auto_save
       +-- error -> Workspace(unsaved image + inline error)
       +-- saved -> Thumbnail(saved image + path)
            +-- click -> Workspace(saved image + path)
            +-- timeout/native drag success -> exit
```

## File Structure

### Create

- `crates/rollshot-app/src/storage.rs`
  - Desktop-directory resolution, timestamped unique path generation, and PNG writing.
- `crates/rollshot-app/src/post_capture.rs`
  - Pure platform presentation policy after capture completion.
- `crates/rollshot-app/src/result_workspace/mod.rs`
  - Result document, workspace state, iced messages/update/view, close routing, and runner.
- `crates/rollshot-app/src/result_workspace/actions.rs`
  - Clipboard, Save As completion, and Reveal system actions.
- `crates/rollshot-app/src/result_workspace/viewport.rs`
  - Pure zoom, centering, overflow, clamping, and pointer-anchor geometry.
- `crates/rollshot-app/src/macos_thumbnail.rs`
  - macOS floating-thumbnail state, presentation, timeout, hover, click, and drag-request handling.
- `crates/rollshot-app/src/macos_product.rs`
  - macOS-only single iced daemon and product phase machine spanning capture, thumbnail, and Result Workspace.
- `crates/rollshot-app/src/macos_native_drag.rs`
  - macOS-only AppKit window patch, file-drag source, active-screen placement, and native drag completion bridge.
- `crates/rollshot-iced-overlay/src/macos_capture.rs`
  - Embeddable macOS capture component extracted from the current daemon-owning runner: capture state, messages, update/view/subscription, window ownership, and completion effects.

### Modify

- `crates/rollshot-app/src/main.rs`
  - Dispatch Linux to the blocking overlay/post-capture pipeline and macOS to the single product daemon.
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
  - Reduce to a temporary compatibility daemon around `macos_capture`, then delete when `rollshot-app` takes daemon ownership.
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

## Task 6: Add Shared Post-Capture Policy And Linux Flow

**Files:**
- Create: `crates/rollshot-app/src/post_capture.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Delete: `crates/rollshot-app/src/save.rs`
- Test: `crates/rollshot-app/src/post_capture.rs`
- Test: `crates/rollshot-app/src/main.rs`

- [ ] **Step 1: Write failing policy and Linux-flow tests**

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
fn cancelled_linux_capture_has_no_post_capture_presentation() {
    assert!(matches!(
        capture_completion(None),
        CaptureCompletion::Cancelled
    ));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app post_capture
```

Expected: FAIL because the shared policy and Linux flow do not exist.

- [ ] **Step 3: Implement the pure policy and Linux orchestration**

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

#[cfg(target_os = "linux")]
pub fn handle_linux_capture(result: rollshot_iced_overlay::CaptureResult) -> Result<(), String> {
    match select_presentation(Platform::Linux, storage::auto_save(&result.image, Platform::Linux)) {
        Presentation::LinuxSavedWorkspace(path) => {
            result_workspace::run(ResultDocument::saved(result.image, path), None)
        }
        Presentation::LinuxUnsavedWorkspace(error) => {
            result_workspace::run(ResultDocument::unsaved(result.image), Some(error))
        }
        _ => unreachable!("Linux policy returned a macOS presentation"),
    }
}
```

The macOS product daemon added in Task 8 calls the same `select_presentation`
function after its embedded capture component reports completion.

- [ ] **Step 4: Route Linux through the blocking runner**

Use:

```rust
#[cfg(target_os = "linux")]
fn run_product_capture(config: rollshot_iced_overlay::OverlayConfig) -> Result<(), String> {
    match post_capture::capture_completion(
        rollshot_iced_overlay::run_overlay(config).map_err(|e| e.to_string())?,
    ) {
        post_capture::CaptureCompletion::Present(result) => {
            post_capture::handle_linux_capture(result)
        }
        post_capture::CaptureCompletion::Cancelled => Ok(()),
    }
}
```

Keep the current macOS `run_overlay` dispatch temporarily until Task 8 replaces
it with `macos_product::run`. Delete the old `PostOverlayAction::SaveAs`,
`post_overlay_request` fixtures, and `save.rs`.

- [ ] **Step 5: Run app and overlay integration tests**

Run:

```bash
rtk cargo test -p rollshot-app
rtk cargo test -p rollshot-iced-overlay
```

Expected: PASS, with Linux success selecting a saved Result Workspace and
auto-save failure preserving an unsaved in-memory document.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app
rtk git commit -m "feat(app): add shared post-capture policy"
```

## Task 7: Extract An Embeddable macOS Capture Component

**Files:**
- Create: `crates/rollshot-iced-overlay/src/macos_capture.rs`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Modify: `crates/rollshot-iced-overlay/src/app.rs`
- Modify: `crates/rollshot-iced-overlay/src/driver.rs`
- Modify: `crates/rollshot-iced-overlay/src/screenshot.rs`
- Modify: `crates/rollshot-iced-overlay/src/macos_window.rs`
- Modify: `crates/rollshot-iced-overlay/src/macos_runner.rs`
- Test: `crates/rollshot-iced-overlay/src/macos_capture.rs`

- [ ] **Step 1: Write failing macOS component lifecycle tests**

Add macOS-gated lifecycle tests around capture completion and window ownership:

```rust
#[test]
fn finish_screenshot_reports_completed_result_without_exiting_host() {
    let mut component = capture_component_with_one_shot();
    let effect = component.apply_overlay_effect(OverlayEffect::FinishScreenshot);
    assert!(matches!(effect, HostEffect::Completed(CaptureResult { .. })));
}

#[test]
fn finish_scrolling_disables_passthrough_before_reporting_completion() {
    let mut component = capture_component_with_active_passthrough();
    let effect = component.apply_overlay_effect(OverlayEffect::FinishScrolling);
    assert!(matches!(effect, HostEffect::Task(_)));
    assert!(component.has_pending_completion());
    assert!(matches!(
        component.update(Message(InternalMessage::PassthroughDisabled)),
        HostEffect::Completed(_)
    ));
}

#[test]
fn cancel_reports_cancelled_without_owning_process_exit() {
    let mut component = capture_component();
    assert!(matches!(
        component.apply_overlay_effect(OverlayEffect::Cancel),
        HostEffect::Cancelled
    ));
}

#[test]
fn component_identifies_only_its_capture_windows() {
    let component = capture_component_with_windows();
    assert!(component.owns_window(component.overlay_window().unwrap()));
    assert!(component.owns_window(component.controls_window().unwrap()));
    assert!(!component.owns_window(iced::window::Id::unique()));
}
```

- [ ] **Step 2: Run tests on macOS to verify they fail**

On macOS, run:

```bash
rtk cargo test -p rollshot-iced-overlay macos_capture
```

Expected: FAIL because the embeddable component and host effects do not exist.

- [ ] **Step 3: Define the component boundary**

Expose a macOS-only module from `rollshot-iced-overlay`:

```rust
#[cfg(target_os = "macos")]
pub mod macos_capture;
```

Define:

```rust
pub struct Component {
    overlay: crate::app::OverlayState,
    overlay_window: Option<iced::window::Id>,
    controls_window: Option<iced::window::Id>,
    controls_rect: Option<crate::coords::LogicalRect>,
    driver: Option<crate::driver::Driver>,
    one_shot: Option<rollshot_capture::OneShotCapture>,
    preview_rx: Option<
        iced::futures::channel::mpsc::UnboundedReceiver<crate::driver::LiveOverlayEvent>,
    >,
}

#[derive(Debug, Clone)]
pub struct Message(InternalMessage);

#[derive(Debug, Clone)]
enum InternalMessage {
    Overlay(crate::app::OverlayMessage),
    WindowOpened { id: iced::window::Id, size: iced::Size },
    OverlayWindowReady(iced::window::Id),
    ControlsWindowReady(iced::window::Id),
    WindowPatched(Result<(), String>),
    PassthroughEnabled,
    PassthroughDisabled,
}

pub enum HostEffect {
    None,
    Task(iced::Task<Message>),
    Completed(CaptureResult),
    Cancelled,
    Fatal(String),
}
```

Expose `Component::new`, `owns_window`, `update`, `apply_overlay_effect`, `view`,
`subscription`, `theme`, `style`, `overlay_window`, and `controls_window`.
The product host treats `macos_capture::Message` as opaque and only maps it back
into `Component::update`. Its private `InternalMessage` may reuse existing
overlay/driver types without exposing them across the crate boundary.
Keep capture resources inside `Component`; remove the current macOS runner's
global `RESULT_SLOT`, `DRIVER_SLOT`, `ONE_SHOT_SLOT`, `PREVIEW_RX`, and
`CAPTURE_MODE`.

- [ ] **Step 4: Move capture behavior out of the daemon-owning runner**

Move the current `macos_runner` resource acquisition, screenshot mapping
validation, overlay/controls window setup, passthrough handling, capture update,
view, and subscription logic into `Component`.

Replace calls to `iced::exit()` with a `HostEffect`:

```rust
match finalized_capture {
    Ok(result) if component.mouse_passthrough_active() => {
        component.set_pending_completion(result);
        HostEffect::Task(component.disable_passthrough_task())
    }
    Ok(result) => HostEffect::Completed(result),
    Err(error) => HostEffect::Fatal(error),
}
```

Capture finalization errors that currently remain inline continue to update the
component and return `HostEffect::None`. `Fatal` is reserved for errors where
the existing runner would terminate.

- [ ] **Step 5: Reduce the standalone macOS runner to a temporary adapter**

Keep `run_overlay` behavior working during the migration by reducing
`macos_runner.rs` to a thin daemon host around `macos_capture::Component`.
It may map `Completed` into its existing result slot and exit, but it must not
retain duplicate capture state, driver resources, or capture-specific update
logic. Mark it as a temporary compatibility path that Task 8 deletes after
`rollshot-app` owns the product daemon.

This keeps every intermediate commit buildable and preserves the existing
macOS capture path until the long-lived product daemon is ready.

- [ ] **Step 6: Run component and overlay verification**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay
rtk cargo fmt --check
```

Expected: PASS on the current platform. The macOS-only component tests are
cfg-gated on other platforms and therefore require a macOS verification run.

On macOS, additionally run:

```bash
rtk cargo test -p rollshot-iced-overlay macos_capture
rtk cargo check -p rollshot-iced-overlay
```

Expected: PASS with the component implementation compiled and the standalone
runner reduced to a thin compatibility adapter.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-iced-overlay
rtk git commit -m "refactor(overlay): expose embeddable macos capture"
```

## Task 8: Build The Single-Process macOS Product Daemon

**Files:**
- Create: `crates/rollshot-app/src/macos_thumbnail.rs`
- Create: `crates/rollshot-app/src/macos_product.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Delete: `crates/rollshot-iced-overlay/src/macos_runner.rs`
- Test: `crates/rollshot-app/src/macos_thumbnail.rs`
- Test: `crates/rollshot-app/src/macos_product.rs`

- [ ] **Step 1: Write failing thumbnail and product-phase tests**

Register `macos_thumbnail` on every target so its timer and interaction helpers
remain portable and testable. Keep `macos_product` macOS-only because it embeds
the macOS capture component. Add:

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
fn completed_capture_auto_save_success_enters_thumbnail() {
    let mut product = product_in_capture_phase();
    product.apply_capture_completion(image(), Ok(PathBuf::from("/tmp/a.png")));
    assert!(matches!(product.phase, Phase::Thumbnail(_)));
}

#[test]
fn completed_capture_auto_save_failure_enters_unsaved_workspace() {
    let mut product = product_in_capture_phase();
    product.apply_capture_completion(image(), Err("disk full".to_string()));
    assert!(matches!(product.phase, Phase::Workspace(_)));
    assert_eq!(product.workspace().unwrap().message_text(), Some("disk full".to_string()));
}

#[test]
fn thumbnail_click_enters_saved_workspace_without_reloading_image() {
    let image = image();
    let pixels = image.as_raw().clone();
    let mut product = product_in_thumbnail_phase(image, PathBuf::from("/tmp/a.png"));
    product.open_workspace();
    assert_eq!(product.workspace().unwrap().document.source_image.as_raw(), &pixels);
}
```

- [ ] **Step 2: Run tests to verify they fail**

On the current platform, run:

```bash
rtk cargo test -p rollshot-app macos_thumbnail
```

Expected: FAIL because the portable thumbnail state does not exist.

On macOS, also run:

```bash
rtk cargo test -p rollshot-app macos_product
```

Expected: FAIL because the product daemon does not exist.

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
pub fn view(state: &ThumbnailState) -> iced::Element<'_, Message>;
```

Render a compact card with the image preview, `Saved`, and `Drag or click`.
Mouse enter/leave pauses/resumes the timer. A release without drag requests the
workspace; timeout requests product exit.

- [ ] **Step 4: Implement the product phase machine**

Use:

```rust
pub enum Phase {
    Capture(rollshot_iced_overlay::macos_capture::Component),
    Thumbnail(ThumbnailState),
    Workspace(result_workspace::ResultWorkspace),
}

pub struct MacosProduct {
    phase: Phase,
    document: Option<ResultDocument>,
    thumbnail_window: Option<iced::window::Id>,
    workspace_window: Option<iced::window::Id>,
}

impl MacosProduct {
    pub fn new(config: rollshot_iced_overlay::OverlayConfig) -> Result<(Self, iced::Task<Message>), String>;
    pub fn apply_capture_completion(&mut self, image: RgbaImage, auto_save: Result<PathBuf, String>);
    pub fn open_workspace(&mut self);
    pub fn workspace(&self) -> Option<&result_workspace::ResultWorkspace>;
}

pub fn run(config: rollshot_iced_overlay::OverlayConfig) -> Result<(), String>;
```

`run` starts exactly one `iced::daemon`. `MacosProduct::new` embeds
`macos_capture::Component` and opens its overlay window. The product update/view
and subscription functions delegate messages for capture-owned windows to the
capture component, thumbnail messages to `macos_thumbnail`, and workspace
messages to the reusable Result Workspace APIs.

- [ ] **Step 5: Transition capture completion without leaving the daemon**

Map capture host effects:

```rust
match capture.update(message) {
    HostEffect::Task(task) => task.map(Message::Capture),
    HostEffect::Completed(result) => {
        state.complete_capture(result);
        state.open_post_capture_window()
    }
    HostEffect::Cancelled => iced::exit(),
    HostEffect::Fatal(error) => {
        eprintln!("{error}");
        iced::exit()
    }
    HostEffect::None => iced::Task::none(),
}
```

`complete_capture` closes all capture-owned windows, keeps the completed
`RgbaImage` in memory, calls `storage::auto_save`, and applies
`post_capture::select_presentation(Platform::Macos, ...)`:

- `MacosSavedThumbnail(path)` creates a saved `ResultDocument` and opens the thumbnail.
- `MacosUnsavedWorkspace(error)` creates an unsaved `ResultDocument` and opens the Result Workspace with the inline error.

Thumbnail click closes the thumbnail window and opens the saved Result
Workspace using the same in-memory document. Timeout exits the daemon.

- [ ] **Step 6: Route macOS launch to the single product daemon**

In `main`:

```rust
#[cfg(target_os = "macos")]
fn run_product_capture(config: rollshot_iced_overlay::OverlayConfig) -> Result<(), String> {
    macos_product::run(config)
}
```

Delete the temporary `rollshot-iced-overlay::macos_runner` adapter and remove
the macOS branch from blocking `run_overlay`. Update
`OverlayError::Unsupported` to state that the blocking overlay runner is
Linux-only; the active macOS path is the embedded component hosted by
`rollshot-app`.

There are no internal child launch modes, image transfer formats, or process
handoffs. The CLI remains blocked because the original `rollshot-app` process
does not exit until the product daemon exits.

- [ ] **Step 7: Run product-daemon verification**

Run:

```bash
rtk cargo test -p rollshot-app macos_thumbnail
rtk cargo test -p rollshot-app macos_product
rtk cargo test -p rollshot-app
rtk cargo test -p rollshot-iced-overlay
rtk cargo fmt --check
```

Expected: PASS on the current platform. The macOS product-daemon tests are
cfg-gated on other platforms and therefore require a macOS verification run.

On macOS, additionally run:

```bash
rtk cargo test -p rollshot-app macos_thumbnail
rtk cargo test -p rollshot-app macos_product
rtk cargo check -p rollshot-app
```

Expected: PASS with the product daemon owning capture-to-post-capture
transitions without a second event loop.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-app/src/macos_thumbnail.rs crates/rollshot-app/src/macos_product.rs crates/rollshot-app/src/main.rs crates/rollshot-app/src/result_workspace crates/rollshot-iced-overlay/src/lib.rs crates/rollshot-iced-overlay/src/macos_runner.rs
rtk git commit -m "feat(app): add single-process macos product daemon"
```

## Task 9: Add The AppKit Native File Drag Bridge

**Files:**
- Create: `crates/rollshot-app/src/macos_native_drag.rs`
- Modify: `crates/rollshot-app/src/macos_thumbnail.rs`
- Modify: `crates/rollshot-app/src/macos_product.rs`
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

Replace Task 8's centered thumbnail origin with
`active_screen_thumbnail_origin`. After the thumbnail window opens, call
`iced::window::run(id, patch_thumbnail_window)` before accepting interactions.
Treat origin lookup, window creation, or patch failure as thumbnail creation
failure so the durable saved file remains and the product daemon exits with an
error.

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
.map(macos_product::Message::NativeDragStarted);
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
rtk git add crates/rollshot-app/src/macos_native_drag.rs crates/rollshot-app/src/macos_thumbnail.rs crates/rollshot-app/src/macos_product.rs crates/rollshot-app/src/main.rs crates/rollshot-app/Cargo.toml Cargo.lock
rtk git commit -m "feat(app): add macos native thumbnail drag"
```

## Task 10: Verify End-To-End Behavior And Repository Health

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
8. The original `rollshot-app` process remains open through capture, thumbnail, and Result Workspace, so the CLI remains blocked.
9. Capture-to-thumbnail and thumbnail-to-workspace transitions stay inside one daemon and do not produce `EventLoop can't be recreated`.
10. Concurrent captures create independent `rollshot-app` processes/thumbnails with no shared queue or coordination.
