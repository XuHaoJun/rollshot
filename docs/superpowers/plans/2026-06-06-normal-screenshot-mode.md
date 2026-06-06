# Normal Screenshot Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a frozen, one-shot normal screenshot workflow to `rollshot-app` while keeping scrolling capture as the default and preserving an overlay architecture that can support future toolbar mode switching.

**Architecture:** Add a one-shot capture interface beside the existing streaming interface. Linux selects a strict KWin `ScreenShot2.CaptureActiveScreen` backend on KDE and a Screenshot portal backend elsewhere; macOS calls `SCScreenshotManager` through a small unsafe-isolation crate. The iced overlay owns a mode-aware session and emits workflow-independent effects that Linux and macOS runners execute.

**Tech Stack:** Rust workspace, `zbus` 4.x, `ashpd` 0.9, `objc2` 0.6 / `objc2-screen-capture-kit` 0.3, `image` 0.25, iced 0.14, iced_layershell 0.18.

---

## File Structure

- Create `crates/rollshot-capture/src/one_shot.rs`: shared safe one-shot trait, result, target-display metadata, backend selection policy.
- Create `crates/rollshot-capture/src/linux/one_shot.rs`: KDE detection, strict KWin selection, non-KDE portal selection.
- Create `crates/rollshot-capture/src/linux/kwin_screenshot.rs`: `ScreenShot2.CaptureActiveScreen` DBus request and raw-image decoding.
- Create `crates/rollshot-capture/src/linux/portal_screenshot.rs`: `org.freedesktop.portal.Screenshot` request and reliable active-output crop policy.
- Create `crates/rollshot-capture/src/macos/one_shot.rs`: safe adapter from `rollshot-macos-oneshot` to capture types.
- Create `crates/rollshot-macos-oneshot/`: isolated macOS framework bindings; this is the only new crate allowed to contain the unsafe Objective-C calls required by generated ScreenCaptureKit bindings.
- Create `crates/rollshot-iced-overlay/src/session.rs`: mode-specific workflow state and workflow-independent effects.
- Create `crates/rollshot-iced-overlay/src/screenshot.rs`: frozen-image handle creation and final crop.
- Create `packaging/linux/dev.rollshot.io.desktop`: installed desktop entry declaring the restricted KWin screenshot interface.
- Modify launch/config/result types and both platform runners to use the new session boundary.
- Modify retained Tauri reference code and the standalone overlay harness only enough to keep the workspace compiling with the changed shared contracts.

### Task 1: Add the Initial Capture Mode Contract

**Files:**
- Modify: `crates/rollshot-capture/src/types.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`
- Modify: `crates/rollshot-app/src/launch.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Modify: `crates/rollshot-iced-overlay/src/bin/capture_overlay.rs`
- Modify: `crates/rollshot-tauri-app/src-tauri/src/native_capture.rs`

- [ ] **Step 1: Write failing serialization and launch tests**

Add `CaptureMode` tests in `types.rs` and update launch/config tests to require:

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureMode {
    #[default]
    Scrolling,
    Screenshot,
}

#[test]
fn interactive_launch_options_default_initial_mode_for_old_json() {
    let decoded: InteractiveLaunchOptions = serde_json::from_str(
        r#"{"backend":"auto","fps":5,"show_cursor":false,"overlay_mode":"iced"}"#,
    )
    .expect("old payload");
    assert_eq!(decoded.initial_mode, CaptureMode::Scrolling);
}
```

Update the existing round-trip test to use `initial_mode: CaptureMode::Screenshot`. Add assertions that `OverlayConfig.initial_mode` is forwarded unchanged by both `rollshot-app` and retained Tauri `overlay_config`. Add a launch test proving that changing `fps` does not change screenshot-mode startup selection.

- [ ] **Step 2: Run the focused tests and verify failure**

Run:

```bash
rtk cargo test -p rollshot-capture interactive_launch_options
rtk cargo test -p rollshot-app launch
rtk cargo test -p rollshot-tauri-app overlay_config
```

Expected: compilation fails because `CaptureMode` and `initial_mode` do not exist.

- [ ] **Step 3: Implement the launch/config contract**

Add `#[serde(default)] pub initial_mode: CaptureMode` to `InteractiveLaunchOptions`, set it to `Scrolling` in `default_capture`, export `CaptureMode`, and add `pub initial_mode: CaptureMode` to `OverlayConfig`.

Forward it in:

```rust
let config = rollshot_iced_overlay::OverlayConfig {
    initial_mode: options.initial_mode,
    backend: options.backend,
    fps: options.fps,
    show_cursor: options.show_cursor,
};
```

Update every `InteractiveLaunchOptions` and `OverlayConfig` literal in the workspace. Do not change runtime behavior yet.

- [ ] **Step 4: Run focused and workspace compile tests**

Run:

```bash
rtk cargo test -p rollshot-capture interactive_launch_options
rtk cargo test -p rollshot-app
rtk cargo test -p rollshot-tauri-app overlay_config
rtk cargo check --workspace
```

Expected: all pass; no-argument and old JSON payloads remain scrolling.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-capture/src/types.rs crates/rollshot-capture/src/lib.rs \
  crates/rollshot-app/src/launch.rs crates/rollshot-app/src/main.rs \
  crates/rollshot-iced-overlay/src/lib.rs crates/rollshot-iced-overlay/src/bin/capture_overlay.rs \
  crates/rollshot-tauri-app/src-tauri/src/native_capture.rs
rtk git commit -m "feat(capture): add initial screenshot mode contract"
```

### Task 2: Add the Safe One-Shot Capture Interface and Selection Policy

**Files:**
- Create: `crates/rollshot-capture/src/one_shot.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`
- Modify: `crates/rollshot-capture/src/error.rs`

- [ ] **Step 1: Write failing policy and fake-backend tests**

Create `one_shot.rs` with tests around these public contracts:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayTarget {
    pub output_name: Option<String>,
    pub logical_region: Region,
    pub physical_size: Size,
}

#[derive(Debug, Clone)]
pub struct OneShotCapture {
    pub image: RgbaImage,
    pub target_display: DisplayTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OneShotBackendKind {
    LinuxKwin,
    LinuxPortal,
    MacosScreenshotManager,
    Unsupported,
}

pub trait OneShotCaptureBackend {
    fn capture_once(&mut self, show_cursor: bool) -> Result<OneShotCapture, CaptureError>;
}
```

An unresolved non-KDE portal target uses `output_name: None` and a zero
`logical_region`; the Linux runner must validate it after the active layer-shell
surface opens. Add pure selection tests:

```rust
assert_eq!(
    one_shot_backend_for("linux", Some("wayland"), Some("KDE")),
    OneShotBackendKind::LinuxKwin
);
assert_eq!(
    one_shot_backend_for("linux", Some("wayland"), Some("GNOME")),
    OneShotBackendKind::LinuxPortal
);
assert_eq!(
    one_shot_backend_for("macos", None, None),
    OneShotBackendKind::MacosScreenshotManager
);
```

Also test that `Unsupported` returns `CaptureError::Unsupported`.
Add a test that screenshot backend creation rejects explicit streaming backend
flags such as `linux-portal` and `macos-sck`; screenshot mode accepts `auto`
only, so a streaming backend name can never become an implicit first-frame
fallback.

- [ ] **Step 2: Run the focused test and verify failure**

Run: `rtk cargo test -p rollshot-capture one_shot`

Expected: compilation fails because the module and contracts do not exist.

- [ ] **Step 3: Implement the safe interface and pure backend policy**

Implement `one_shot_backend_for` using exact desktop token matching after splitting `XDG_CURRENT_DESKTOP` on `:`:

```rust
fn is_kde(desktop: Option<&str>) -> bool {
    desktop
        .unwrap_or_default()
        .split(':')
        .any(|part| part.eq_ignore_ascii_case("KDE"))
}
```

Add `create()` dispatch with target-gated constructors. Do not permit any adapter from `CaptureBackend` or `FrameStream`.

Add `CaptureError::Mapping { message: String }` for reliable-output mapping
failures. Add `OneShotBackendKind::from_environment(backend_flag: &str)` and
require `backend_flag == "auto"` before applying the OS/desktop policy.

- [ ] **Step 4: Run tests**

Run:

```bash
rtk cargo test -p rollshot-capture one_shot
rtk cargo test -p rollshot-capture error
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-capture/src/one_shot.rs crates/rollshot-capture/src/lib.rs \
  crates/rollshot-capture/src/error.rs
rtk git commit -m "feat(capture): add one-shot capture interface"
```

### Task 3: Implement Strict KDE/KWin One-Shot Capture

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/rollshot-capture/Cargo.toml`
- Create: `crates/rollshot-capture/src/linux/one_shot.rs`
- Create: `crates/rollshot-capture/src/linux/kwin_screenshot.rs`
- Modify: `crates/rollshot-capture/src/linux/mod.rs`

- [ ] **Step 1: Write failing KWin metadata and no-fallback tests**

Define a testable DBus boundary:

```rust
trait KwinScreenshotClient {
    fn capture_active_screen(&self, include_cursor: bool) -> Result<KwinRawCapture, CaptureError>;
}

struct KwinRawCapture {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    stride: u32,
    format: KwinPixelFormat,
    scale: f64,
    screen_name: String,
}
```

Tests must prove:

- raw `RGBA8888` data becomes an `RgbaImage`;
- stride padding is removed correctly;
- missing `screen` metadata is `CaptureError::Mapping`;
- a fake KWin permission error is returned unchanged and never invokes a fake portal client.

- [ ] **Step 2: Run focused tests and verify failure**

Run: `rtk cargo test -p rollshot-capture kwin_screenshot`

Expected: compilation fails because the KWin module does not exist.

- [ ] **Step 3: Implement `ScreenShot2.CaptureActiveScreen`**

Add workspace dependency `zbus = "4.4"` and Linux dependency `zbus.workspace = true`.

Mirror Spectacle's protocol:

```text
service:   org.kde.KWin.ScreenShot2
path:      /org/kde/KWin/ScreenShot2
interface: org.kde.KWin.ScreenShot2
method:    CaptureActiveScreen
args:      options map, write-end Unix FD
reply:     metadata map
```

Use options `include-cursor` from `show_cursor`, `native-resolution = true`, and `include-shadow = false`. Create a CLOEXEC pipe with the existing safe `nix` dependency, pass the write FD through zbus, read exactly `stride * height` bytes from the read FD, and require metadata keys `width`, `height`, `stride`, `format`, `scale`, and `screen`.

Map service absence, access denial, malformed metadata, short reads, and unsupported pixel formats to explicit errors. Do not call the portal from `LinuxKwinOneShotBackend`.

- [ ] **Step 4: Run Linux capture tests**

Run:

```bash
rtk cargo test -p rollshot-capture kwin_screenshot
rtk cargo test -p rollshot-capture one_shot
rtk cargo check -p rollshot-capture
```

Expected: all pass on Linux without making a live DBus request.

- [ ] **Step 5: Commit**

```bash
rtk git add Cargo.toml crates/rollshot-capture/Cargo.toml \
  crates/rollshot-capture/src/linux/mod.rs crates/rollshot-capture/src/linux/one_shot.rs \
  crates/rollshot-capture/src/linux/kwin_screenshot.rs
rtk git commit -m "feat(capture): add strict KWin one-shot backend"
```

### Task 4: Implement Non-KDE Wayland Screenshot Portal Capture

**Files:**
- Create: `crates/rollshot-capture/src/linux/portal_screenshot.rs`
- Modify: `crates/rollshot-capture/src/linux/one_shot.rs`
- Modify: `crates/rollshot-capture/src/linux/mod.rs`

- [ ] **Step 1: Write failing portal response and mapping tests**

Use an injected portal client. Tests must prove:

- portal cancellation becomes `CaptureError::UserCancelled`;
- a local-file URI loads a PNG;
- an image whose physical dimensions match an active overlay's logical size and
  scale is accepted;
- an inconsistent overlay/image aspect or size mapping returns
  `CaptureError::Mapping`;
- KDE selection never constructs this backend.

Use a deterministic validation helper:

```rust
fn validate_active_output_image(
    image_size: Size,
    overlay_logical: Size,
    overlay_scale: f64,
) -> Result<DisplayTarget, CaptureError>;
```

- [ ] **Step 2: Run the focused test and verify failure**

Run: `rtk cargo test -p rollshot-capture portal_screenshot`

Expected: compilation fails because the portal screenshot module does not exist.

- [ ] **Step 3: Implement portal capture and reliable mapping gate**

Call:

```rust
ashpd::desktop::screenshot::Screenshot::request()
    .interactive(false)
    .modal(false)
    .send()
```

Load the returned local URI with `image::open(...).into_rgba8()` and return an
unresolved `DisplayTarget`. The Linux runner opens on `StartMode::Active`, then
calls `validate_active_output_image` after receiving the layer-surface size and
scale. Continue only when the portal image dimensions prove that the image is
the same single output as the active overlay. A full-desktop multi-monitor
image or any ambiguous scaling relationship returns `CaptureError::Mapping`.

Keep this implementation one-shot only: do not import or call screencast portal or PipeWire modules.

- [ ] **Step 4: Run tests**

Run:

```bash
rtk cargo test -p rollshot-capture portal_screenshot
rtk cargo test -p rollshot-capture one_shot
rtk cargo check -p rollshot-capture
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-capture/src/linux/portal_screenshot.rs \
  crates/rollshot-capture/src/linux/one_shot.rs crates/rollshot-capture/src/linux/mod.rs
rtk git commit -m "feat(capture): add Wayland portal screenshot backend"
```

### Task 5: Add the Isolated macOS `SCScreenshotManager` Backend

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/rollshot-macos-oneshot/Cargo.toml`
- Create: `crates/rollshot-macos-oneshot/src/lib.rs`
- Modify: `crates/rollshot-capture/Cargo.toml`
- Create: `crates/rollshot-capture/src/macos/one_shot.rs`
- Modify: `crates/rollshot-capture/src/macos/mod.rs`

- [ ] **Step 1: Write failing safe-adapter tests**

In `rollshot-capture/src/macos/one_shot.rs`, inject a safe platform adapter:

```rust
trait MacosOneShotClient {
    fn capture_display_under_cursor(&self, show_cursor: bool)
        -> Result<rollshot_macos_oneshot::CapturedDisplay, CaptureError>;
}
```

Test conversion into `OneShotCapture`, including physical/logical sizes and a failure when the returned display dimensions are zero.
Also test that macOS versions below 14 return `CaptureError::Unsupported`
instead of falling back to scap streaming.

- [ ] **Step 2: Run the focused test and verify failure**

Run: `rtk cargo test -p rollshot-capture macos_one_shot`

Expected: compilation fails because the adapter crate and module do not exist.

- [ ] **Step 3: Create the unsafe-isolation crate**

Add workspace member `crates/rollshot-macos-oneshot`. Its public API is safe:

```rust
pub struct CapturedDisplay {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub logical_x: i32,
    pub logical_y: i32,
    pub logical_width: u32,
    pub logical_height: u32,
    pub display_id: u32,
}

pub fn capture_display_under_cursor(show_cursor: bool) -> Result<CapturedDisplay, String>;
```

On macOS, depend on `objc2`, `objc2-foundation`, `objc2-core-graphics`,
`objc2-screen-capture-kit` with `SCScreenshotManager`, `SCShareableContent`,
and `SCStream` features, plus `block2`.

Inside this crate only:

1. obtain the current pointer location;
2. request `SCShareableContent`;
3. select the `SCDisplay` whose frame contains the pointer;
4. create `SCContentFilter` and screenshot configuration;
5. set native output dimensions and `showsCursor`;
6. call `SCScreenshotManager::captureImageWithFilter...`;
7. convert `CGImage` bytes to tightly packed RGBA.

Keep every unsafe block local and documented with the Objective-C ownership and buffer-size invariant it relies on. Provide a non-macOS stub returning unsupported so workspace checks on Linux succeed.
Set this crate's lint policy explicitly to allow unsafe only here; do not relax
the `unsafe_code = "deny"` policy in `rollshot-capture`.

- [ ] **Step 4: Implement and test the capture adapter**

Convert `CapturedDisplay` to `RgbaImage`, preserve the display's logical origin
and size in `DisplayTarget.logical_region`, use `display_id.to_string()` as
`DisplayTarget.output_name`, and return explicit unsupported, permission, and
capture errors without invoking scap.

Run:

```bash
rtk cargo test -p rollshot-capture macos_one_shot
rtk cargo check -p rollshot-macos-oneshot
rtk cargo check -p rollshot-capture
```

Expected: Linux checks pass through the stub; safe-adapter tests pass.

- [ ] **Step 5: Commit**

```bash
rtk git add Cargo.toml crates/rollshot-macos-oneshot crates/rollshot-capture/Cargo.toml \
  crates/rollshot-capture/src/macos/mod.rs crates/rollshot-capture/src/macos/one_shot.rs
rtk git commit -m "feat(capture): add macOS one-shot screenshot backend"
```

### Task 6: Introduce the Mode-Aware Overlay Session

**Files:**
- Create: `crates/rollshot-iced-overlay/src/session.rs`
- Create: `crates/rollshot-iced-overlay/src/screenshot.rs`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Modify: `crates/rollshot-iced-overlay/src/app.rs`
- Modify: `crates/rollshot-iced-overlay/src/driver.rs`

- [ ] **Step 1: Write failing workflow-state and result tests**

Define tests around:

```rust
enum CaptureWorkflow {
    Scrolling(ScrollingWorkflow),
    Screenshot(ScreenshotWorkflow),
}

struct OverlaySession {
    active_mode: CaptureMode,
    workflow: CaptureWorkflow,
}

enum OverlayEffect {
    None,
    StartScrollingCapture { crop: LogicalRect },
    FinishScrolling,
    FinishScreenshot { crop: LogicalRect },
    SwitchWorkflow(CaptureMode),
    Cancel,
}
```

Tests must prove:

- scrolling drag release emits `StartScrollingCapture`;
- screenshot drag release emits `FinishScreenshot` immediately;
- an empty screenshot click emits `None`;
- `Esc` before a valid selection cancels;
- screenshot state owns its frozen image and scrolling state owns confirmation/preview state;
- screenshot crop returns `CaptureResult { stats: None }`;
- driver finalization returns `stats: Some(...)`.
- `request_mode_switch` emits `SwitchWorkflow` without embedding platform
  capture resources in the session.

- [ ] **Step 2: Run overlay tests and verify failure**

Run: `rtk cargo test -p rollshot-iced-overlay`

Expected: compilation fails because session/workflow types and optional stats do not exist.

- [ ] **Step 3: Implement session and frozen-image crop**

Move mode-specific selection fields out of the flat `OverlayState`. Keep shared window identity/size and transient warning state in the app state.

In `screenshot.rs`, implement:

```rust
pub fn finish_screenshot(
    capture: &OneShotCapture,
    crop: LogicalRect,
    overlay_logical: Size,
) -> Result<CaptureResult, String> {
    let region = map_crop_to_frame(crop, overlay_logical, capture.target_display.physical_size);
    let frame = CapturedFrame {
        image: capture.image.clone(),
        timestamp: SystemTime::UNIX_EPOCH,
        metadata: FrameMetadata::fake(),
    };
    let cropped = crop_frame(&frame, region).map_err(|e| e.to_string())?;
    Ok(CaptureResult { image: cropped.image, stats: None })
}
```

Change `CaptureResult.stats` to `Option<StitchStats>` and update driver results to `Some(stats)`. Render the frozen image beneath the existing crop canvas only in screenshot workflow.

Add `OverlaySession::request_mode_switch(CaptureMode) -> OverlayEffect`,
`activate_scrolling()`, and `activate_screenshot(OneShotCapture)`. The MVP has
no toolbar message that calls `request_mode_switch`, but these methods make
future switching a runner-executed resource transition rather than a session
or backend redesign.

- [ ] **Step 4: Update compile-only consumers and run tests**

Update retained Tauri `store_overlay_outcome` to pass `result.stats.unwrap_or_default()` because it is reference-only and its session DTO still requires stats. Update the standalone harness to print frame count only when stats exist.

Run:

```bash
rtk cargo test -p rollshot-iced-overlay
rtk cargo test -p rollshot-tauri-app store_overlay_outcome
rtk cargo check --workspace
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-iced-overlay/src/session.rs crates/rollshot-iced-overlay/src/screenshot.rs \
  crates/rollshot-iced-overlay/src/lib.rs crates/rollshot-iced-overlay/src/app.rs \
  crates/rollshot-iced-overlay/src/driver.rs crates/rollshot-iced-overlay/src/bin/capture_overlay.rs \
  crates/rollshot-tauri-app/src-tauri/src/native_capture.rs
rtk git commit -m "refactor(overlay): add mode-aware capture session"
```

### Task 7: Wire Linux Runner Effects Without Starting a Screenshot Stream

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/linux_runner.rs`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`

- [ ] **Step 1: Write failing runner-initialization tests**

Extract a pure startup decision:

```rust
enum InitialCaptureResource {
    Streaming,
    OneShot(OneShotCapture),
}
```

Inject fake streaming and one-shot factories. Tests must prove:

- `Scrolling` calls only the streaming factory;
- `Screenshot` calls only the one-shot factory;
- KWin target output produces `StartMode::TargetScreen(name)`;
- a KWin capture with missing output name is rejected before layer-shell starts;
- an unresolved portal target opens with `StartMode::Active` and must pass the
  post-open mapping gate;
- screenshot finish uses frozen-image crop and never finalizes a driver.
- handling `SwitchWorkflow` releases the current resource before acquiring and
  activating the requested workflow.

- [ ] **Step 2: Run focused tests and verify failure**

Run: `rtk cargo test -p rollshot-iced-overlay linux_runner`

Expected: tests fail because Linux startup always constructs `Driver`.

- [ ] **Step 3: Implement workflow-independent Linux runner effects**

At startup, branch only to acquire the initial resource, then create `OverlaySession`.

- Scrolling: retain the current pre-overlay `Driver::start_capture` behavior and `StartMode::Active`.
- Screenshot with a KWin output name: call
  `OneShotBackendKind::from_environment(&config.backend)?.create()?.capture_once(...)`, do not
  create preview channels or a driver, and use
  `StartMode::TargetScreen(output_name)`.
- Screenshot with an unresolved non-KDE portal target: use `StartMode::Active`;
  on the opened layer surface, validate the portal image against the active
  surface size/scale and exit with `CaptureError::Mapping` if it is not a
  provable single-output match.

Map session effects to resource operations. `FinishScreenshot` crops the stored frozen capture and exits immediately. Keep KWin errors unchanged so no portal fallback can occur.
For `SwitchWorkflow`, release the current driver or frozen capture first,
acquire the requested backend resource, and call the matching
`OverlaySession::activate_*` method. No MVP UI emits this effect.

- [ ] **Step 4: Run Linux tests and compile checks**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay linux_runner
rtk cargo test -p rollshot-iced-overlay
rtk cargo check -p rollshot-app
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-iced-overlay/src/linux_runner.rs crates/rollshot-iced-overlay/src/lib.rs
rtk git commit -m "feat(overlay): run Linux screenshot workflow without streaming"
```

### Task 8: Wire macOS Runner Effects and Exact Display Placement

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/macos_runner.rs`
- Modify: `crates/rollshot-iced-overlay/src/macos_window.rs`

- [ ] **Step 1: Write failing startup and placement tests**

Add pure tests proving:

- screenshot startup calls only the one-shot factory;
- scrolling startup calls only `Driver`;
- macOS screenshot window size uses the target display logical size;
- screenshot mode never opens the scrolling controls window or enables passthrough;
- valid screenshot release immediately returns a no-stats result.
- handling `SwitchWorkflow` releases the current resource before acquiring and
  activating the requested workflow on the target display.

- [ ] **Step 2: Run focused tests and verify failure**

Run: `rtk cargo test -p rollshot-iced-overlay macos_runner`

Expected: tests fail because macOS startup always creates `Driver` and uses the main screen.

- [ ] **Step 3: Implement mode-aware macOS startup and effects**

Use the one-shot target display's logical size and display identifier for screenshot window placement. Keep existing scrolling placement and controls behavior unchanged.

Handle effects as follows:

- `StartScrollingCapture`: begin stitch and enable passthrough/open controls.
- `FinishScrolling`: finalize driver.
- `FinishScreenshot`: crop frozen image, set result, and exit without passthrough.
- `SwitchWorkflow`: release the current resource, acquire the requested
  resource, reposition the overlay when the target display changes, and call
  the matching `OverlaySession::activate_*` method.
- `Cancel`: release whichever resource exists and exit.

- [ ] **Step 4: Run overlay and workspace checks**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay macos_runner
rtk cargo test -p rollshot-iced-overlay
rtk cargo check --workspace
```

Expected: all pass on the current host; macOS target/runtime verification remains for Task 10.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-iced-overlay/src/macos_runner.rs \
  crates/rollshot-iced-overlay/src/macos_window.rs
rtk git commit -m "feat(overlay): run macOS screenshot workflow without streaming"
```

### Task 9: Add KDE Desktop Permission Declaration and User Documentation

**Files:**
- Create: `packaging/linux/dev.rollshot.io.desktop`
- Modify: `README.md`

- [ ] **Step 1: Write a failing desktop-entry contract check**

Run:

```bash
rtk test -f packaging/linux/dev.rollshot.io.desktop
```

Expected: non-zero exit because the desktop entry does not exist.

- [ ] **Step 2: Add the desktop entry**

Create:

```ini
[Desktop Entry]
Type=Application
Name=Rollshot
Exec=rollshot-app
Icon=dev.rollshot.io
Terminal=false
Categories=Graphics;Utility;
X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2
```

Document that KDE exact-display normal screenshot support requires launching an installed desktop entry recognized by KWin; direct development binaries may receive an explicit permission error. Document the `initial_mode` JSON examples and the non-KDE best-effort limitation.

- [ ] **Step 3: Verify declaration and docs**

Run:

```bash
rtk rg -n "X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2" packaging/linux/dev.rollshot.io.desktop
rtk rg -n "initial_mode|ScreenShot2|Screenshot portal" README.md
```

Expected: both commands show the new declarations.

- [ ] **Step 4: Commit**

```bash
rtk git add packaging/linux/dev.rollshot.io.desktop README.md
rtk git commit -m "docs: document normal screenshot platform requirements"
```

### Task 10: Complete Automated and Platform Verification

**Files:**
- Modify only files required to fix failures directly caused by this feature.

- [ ] **Step 1: Run formatting and full automated verification**

Run:

```bash
rtk cargo fmt --check
rtk cargo test --workspace
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all pass.

- [ ] **Step 2: Prove screenshot mode has no streaming fallback**

Run:

```bash
rtk rg -n "Driver::start_capture|\\.start\\(CaptureOptions|SCStream|ScreenCast|PipeWire" \
  crates/rollshot-capture/src/one_shot.rs \
  crates/rollshot-capture/src/linux/kwin_screenshot.rs \
  crates/rollshot-capture/src/linux/portal_screenshot.rs \
  crates/rollshot-capture/src/macos/one_shot.rs \
  crates/rollshot-macos-oneshot/src/lib.rs \
  crates/rollshot-iced-overlay/src/screenshot.rs
```

Expected: no calls that start a stream or construct a scrolling `Driver`.
`SCStream` may appear only as a generated ScreenCaptureKit feature dependency,
not as a constructed or started capture object.

- [ ] **Step 3: Verify KDE/KWin runtime**

Install `packaging/linux/dev.rollshot.io.desktop` and the release binary using
the same `Exec=rollshot-app` identity, then launch the installed binary from a
KDE Wayland session:

```bash
rtk rollshot-app --capture \
  '{"initial_mode":"screenshot","backend":"auto","fps":5,"show_cursor":false,"overlay_mode":"iced"}'
```

Verify:

- exact display under mouse is frozen;
- no portal picker or screen-sharing indicator appears;
- releasing a valid drag immediately opens save;
- saved PNG matches the selected region;
- removing/invalidating KWin permission produces an explicit error and no portal fallback.

- [ ] **Step 4: Verify non-KDE Wayland runtime**

Run the same command on a supported non-KDE Wayland compositor.

Verify:

- Screenshot portal is used;
- cancellation returns cleanly;
- reliable active-output mapping shows the correct frozen output;
- unavailable mapping exits explicitly instead of showing an incorrectly scaled image.

- [ ] **Step 5: Verify macOS runtime**

Run the same command on macOS 14 or newer.

Verify:

- Screen Recording permission failure is explicit;
- after permission is granted, the display under the pointer is frozen;
- no streaming capture indicator/lifecycle is started;
- releasing a valid drag immediately opens save;
- saved PNG matches the selected region at Retina scale.

- [ ] **Step 6: Verify scrolling regression**

Run `rollshot-app` without arguments on Linux and macOS. Verify the existing scrolling workflow, live preview, finish/cancel behavior, and saved stitched result are unchanged.

- [ ] **Step 7: Commit verification fixes**

If verification required direct feature fixes, stage the exact files changed
for those fixes, inspect `rtk git diff --staged`, and commit:

```bash
rtk git commit -m "fix: address normal screenshot verification findings"
```

If no fixes were required, do not create an empty commit.
