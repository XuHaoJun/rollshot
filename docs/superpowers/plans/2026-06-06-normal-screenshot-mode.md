# Normal Screenshot Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a frozen, one-shot normal screenshot workflow to `rollshot-app` while keeping scrolling capture as the default and preserving an overlay architecture that can support future toolbar mode switching.

**Architecture:** Add a one-shot capture interface beside the existing streaming interface. Linux selects a strict KWin `ScreenShot2.CaptureActiveScreen` backend on KDE and a Screenshot portal backend elsewhere; macOS calls `SCScreenshotManager` through a small unsafe-isolation crate. The iced overlay owns a mode-aware session and emits workflow-independent effects that Linux and macOS runners execute.

**Tech Stack:** Rust workspace, `zbus` 4.x, `ashpd` 0.9, `objc2` 0.6 / `objc2-screen-capture-kit` 0.3, `image` 0.25, iced 0.14, iced_layershell 0.18.

---

## Locked Review Decisions

- Normal screenshot mode has a separate one-shot capability and never adapts a
  stream into a screenshot.
- KDE uses `ScreenShot2` exclusively. A KWin failure is returned unchanged and
  never falls back to the portal.
- Non-KDE Screenshot portal support is intentionally single-output-only for
  this MVP. The portal does not provide pointer or output identity, so any
  multi-output or otherwise ambiguous image is rejected.
- The overlay session and runners become mode-aware, but the MVP does not
  implement an unused in-session switch transition. Future toolbar work reuses
  the planned `acquire_resource(mode, ...)` boundary and adds the transition
  only when a UI can exercise it.
- Screenshot finalization crops the borrowed frozen image directly. It must not
  clone the full display image before cropping.
- Every backend applies checked dimension/byte calculations, a 40-megapixel
  decoded-image ceiling, and a bounded request/callback wait. This includes an
  8K display while bounding the frozen image plus iced render buffer to roughly
  320 MiB before the final crop.

## Runtime Data Flow

```text
InteractiveLaunchOptions.initial_mode
                 |
                 v
       acquire_resource(mode)
          /               \
 scrolling                 screenshot
 Driver + stream       OneShotCaptureBackend
          \               /
           v             v
            OverlaySession
                 |
       drag/release effects
          /               \
 begin/finalize stitch     crop borrowed frozen image
          \               /
           CaptureResult { image, stats: Option<_> }
```

## File Structure

- Create `crates/rollshot-capture/src/one_shot.rs`: shared safe one-shot trait, result, target-display metadata, backend selection policy.
- Create `crates/rollshot-capture/src/linux/one_shot.rs`: KDE detection, strict KWin selection, non-KDE portal selection.
- Create `crates/rollshot-capture/src/linux/kwin_screenshot.rs`: `ScreenShot2.CaptureActiveScreen` DBus request and raw-image decoding.
- Create `crates/rollshot-capture/src/linux/portal_screenshot.rs`: `org.freedesktop.portal.Screenshot` request and strict single-output validation.
- Create `crates/rollshot-capture/src/macos/one_shot.rs`: safe adapter from `rollshot-macos-oneshot` to capture types.
- Create `crates/rollshot-macos-oneshot/`: isolated macOS framework bindings; this is the only new crate allowed to contain the unsafe Objective-C calls required by generated ScreenCaptureKit bindings.
- Create `crates/rollshot-iced-overlay/src/session.rs`: mode-specific workflow state and workflow-independent effects.
- Create `crates/rollshot-iced-overlay/src/screenshot.rs`: frozen-image handle creation and final crop.
- Create `packaging/linux/dev.rollshot.io.desktop`: installed desktop entry declaring the restricted KWin screenshot interface.
- Modify `crates/rollshot-capture/src/crop.rs`: share validated borrowed-image cropping between frame and screenshot results.
- Modify `Cargo.lock`: lock direct KWin/macOS binding dependencies introduced by the new backends.
- Modify `.github/workflows/ci.yml`: explicitly check the new macOS isolation crate on macOS.
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

#[derive(Debug)]
pub struct OneShotCapture {
    image: RgbaImage,
    target_display: DisplayTarget,
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
Test `OneShotCapture::new` rejects a target whose `physical_size` differs from
the decoded image dimensions.
Test the shared surface-mapping validator at 1x and fractional scale, allowing
at most one physical pixel of rounding difference and rejecting larger or
multi-output mismatches.

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
Add shared checked helpers that reject zero dimensions, multiplication
overflow, and decoded images above `MAX_ONE_SHOT_PIXELS = 40_000_000`.
Unit-test the exact boundary, one pixel above it, and overflowing dimensions.
Do not implement `Clone` for `OneShotCapture`; resource ownership must remain
explicit so a runner cannot accidentally duplicate a full display image.
Require all backends to construct captures through
`OneShotCapture::new(image, target_display)`, which validates image dimensions
against `DisplayTarget.physical_size`. Expose borrowed `image()` and
`target_display()` accessors instead of public mutable fields.
Add public
`validate_surface_mapping(image_size, overlay_logical, overlay_scale)` in this
module so both KWin and portal runners use one explicit geometry policy. Compute
rounded expected physical dimensions with checked math, accept at most one
pixel of per-axis rounding difference, and reject non-positive/non-finite
scale.

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
- Modify: `Cargo.lock`
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
    qimage_format: u32,
    scale: f64,
    screen_name: String,
}
```

Tests must prove:

- captured fixtures using the supported Qt `QImage::Format` values become an
  `RgbaImage`, including channel order and premultiplied-alpha handling;
- missing `screen` metadata is `CaptureError::Mapping`;
- a fake KWin permission error is returned unchanged and never invokes a fake portal client.
- malformed dimensions, unsupported Qt formats, short reads, and a request
  or pipe-read timeout return explicit errors.
- `show_cursor` is forwarded to KWin's `include-cursor` option.

- [ ] **Step 2: Run focused tests and verify failure**

Run: `rtk cargo test -p rollshot-capture kwin_screenshot`

Expected: compilation fails because the KWin module does not exist.

- [ ] **Step 3: Implement `ScreenShot2.CaptureActiveScreen`**

Add workspace dependency `zbus = "4.4"`, enable the existing `nix` dependency's
`poll` feature, and add Linux dependency `zbus.workspace = true`.

Mirror Spectacle's protocol:

```text
service:   org.kde.KWin.ScreenShot2
path:      /org/kde/KWin/ScreenShot2
interface: org.kde.KWin.ScreenShot2
method:    CaptureActiveScreen
args:      options map, write-end Unix FD
reply:     metadata map
```

Use options `include-cursor` from `show_cursor`, `native-resolution = true`, and `include-shadow = false`. Create a CLOEXEC pipe with the existing safe `nix` dependency, pass the write FD through zbus, and require metadata keys `type = "raw"`, `width`, `height`, `format`, `scale`, and `screen`.

Start a bounded reader thread before making the DBus request so a full pipe
cannot deadlock KWin while Rollshot waits for metadata. Use `nix::poll` with a
deadline so the reader exits and can be joined on timeout; do not detach a
permanently blocked reader. Cap bytes read at the 40-megapixel RGBA ceiling,
then validate the final exact byte count after metadata arrives.

`format` is a numeric Qt `QImage::Format`, not a Rollshot pixel-format enum,
and KWin does not return a stride field. Mirror Spectacle's contract: support
and fixture-test only `Format_RGB32`, `Format_ARGB32`,
`Format_ARGB32_Premultiplied`, `Format_RGBX8888`, `Format_RGBA8888`, and
`Format_RGBA8888_Premultiplied`. Convert native-endian ARGB layouts and
unpremultiply alpha where required. These accepted formats are all 32-bit, so
the checked required byte count is `width * height * 4`. Read exactly that many
bytes and reject every other Qt format rather than guessing.

Apply a 5-second DBus timeout and a 5-second pipe-read completion timeout. Map
service absence, access denial, malformed metadata, timeout, short reads,
oversized images, and unsupported Qt formats to explicit errors. Do not call
the portal from `LinuxKwinOneShotBackend`.
Derive the initial logical size from physical size and validated positive
`scale`; use origin `(0, 0)` until the named layer-shell surface opens. After
open, require the surface logical size/scale to agree with the capture before
showing the frozen image.

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
rtk git add Cargo.toml Cargo.lock crates/rollshot-capture/Cargo.toml \
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
- a single-output image whose physical dimensions exactly match the active
  overlay's logical size and scale is accepted;
- a composite/multi-output image or inconsistent overlay/image mapping returns
  `CaptureError::Mapping`;
- an oversized or decompression-limit-exceeding PNG is rejected before it can
  consume unbounded memory;
- a portal request timeout returns `CaptureError::Timeout`;
- `show_cursor = true` returns explicit unsupported because the Screenshot
  portal has no cursor-inclusion option;
- KDE selection never constructs this backend.

Use the shared deterministic validation helper from Task 2:

```rust
pub fn validate_surface_mapping(
    image_size: Size,
    overlay_logical: Size,
    overlay_scale: f64,
) -> Result<(), CaptureError>;
```

- [ ] **Step 2: Run the focused test and verify failure**

Run: `rtk cargo test -p rollshot-capture portal_screenshot`

Expected: compilation fails because the portal screenshot module does not exist.

- [ ] **Step 3: Implement portal capture and strict single-output gate**

Call:

```rust
let request = ashpd::desktop::screenshot::Screenshot::request()
    .interactive(false)
    .modal(false)
    .send()
    .await?;
let response = request.response()?;
let uri = response.uri();
```

Apply a 60-second portal request timeout. Load only a returned local-file URI
through `image::ImageReader` with decoding limits and return an unresolved
`DisplayTarget`. Reject non-file URIs. The Linux runner opens on
`StartMode::Active`, then calls `validate_surface_mapping` after receiving
the layer-surface size and scale. Continue only when the portal image
dimensions prove that the image is exactly the same single output as the active
overlay. A full-desktop multi-monitor image or any ambiguous scaling
relationship returns `CaptureError::Mapping`.

Keep this implementation one-shot only: do not import or call screencast portal or PipeWire modules.
Do not silently ignore `show_cursor`; reject `true` because the Screenshot
portal cannot guarantee cursor inclusion.

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
- Modify: `Cargo.lock`
- Create: `crates/rollshot-macos-oneshot/Cargo.toml`
- Create: `crates/rollshot-macos-oneshot/src/lib.rs`
- Modify: `crates/rollshot-capture/Cargo.toml`
- Create: `crates/rollshot-capture/src/macos/one_shot.rs`
- Modify: `crates/rollshot-capture/src/macos/mod.rs`
- Modify: `.github/workflows/ci.yml`

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
Test typed isolation-error mapping for permission denial, timeout, unsupported
OS, and generic capture failure. In the isolation crate, put callback-result
resolution behind a pure helper and test timeout, oversized dimensions, and a
callback that returns neither an image nor an error.
Add a padded-row BGRA fixture test proving that SDR `CGImage` output is copied
row-by-row and converted to tightly packed RGBA.
Test that `show_cursor` is forwarded to the ScreenCaptureKit configuration.

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

#[derive(Debug, thiserror::Error)]
pub enum MacosOneShotError {
    Unsupported(String),
    PermissionDenied(String),
    Timeout(String),
    Capture(String),
}

pub fn capture_display_under_cursor(
    show_cursor: bool,
) -> Result<CapturedDisplay, MacosOneShotError>;
```

Set `publish = false` because this is an internal unsafe-isolation crate. On
macOS, use the current compatible binding family: `objc2 = "0.6"`,
`objc2-foundation = "0.3"`, `objc2-core-graphics = "0.3"` with `CGWindow`, and
`objc2-screen-capture-kit = "0.3"` with `SCScreenshotManager`,
`SCShareableContent`, `SCStream`, `block2`, `objc2-core-foundation`, and
`objc2-core-graphics`, plus `block2 = "0.6"`. Keep Objective-C/framework
dependencies under `target.'cfg(target_os = "macos")'.dependencies`; the
non-macOS stub and pure buffer/error tests must compile without linking Apple
frameworks.

Inside this crate only:

1. call `CGPreflightScreenCaptureAccess`; when false, call
   `CGRequestScreenCaptureAccess`, then map a still-false result to
   `MacosOneShotError::PermissionDenied` with restart/System Settings guidance;
2. obtain the current pointer location;
3. request `SCShareableContent`;
4. select the `SCDisplay` whose frame contains the pointer;
5. create `SCContentFilter` and screenshot configuration;
6. set native output dimensions and `showsCursor`;
7. call the generated
   `SCScreenshotManager::captureImageWithFilter_configuration_completionHandler`;
8. wait for each ScreenCaptureKit callback with a 30-second timeout;
9. convert the SDR `CGImage` BGRA output to tightly packed RGBA row-by-row,
   respecting its bytes-per-row, using checked dimensions and the same
   documented 40-megapixel ceiling.

Keep every unsafe block local and documented with the Objective-C ownership and buffer-size invariant it relies on. Provide a non-macOS stub returning unsupported so workspace checks on Linux succeed.
Do not inherit the workspace's `unsafe_code = "forbid"` lint in this isolation
crate; set its local lint policy explicitly and document why unsafe is required
only here. Do not relax the `unsafe_code = "deny"` policy in
`rollshot-capture`.

- [ ] **Step 4: Implement and test the capture adapter**

Convert `CapturedDisplay` to `RgbaImage`, preserve the display's logical origin
and size in `DisplayTarget.logical_region`, use `display_id.to_string()` as
`DisplayTarget.output_name`, and map the typed isolation-crate errors to
explicit unsupported, permission, timeout, and capture errors without invoking
scap.

Run:

```bash
rtk cargo test -p rollshot-capture macos_one_shot
rtk cargo check -p rollshot-macos-oneshot
rtk cargo check -p rollshot-capture
```

Expected: Linux checks pass through the stub; safe-adapter tests pass.

Add `cargo check -p rollshot-macos-oneshot --all-targets` to the existing
macOS-only CI target-check step so the isolation crate cannot silently rot.

- [ ] **Step 5: Commit**

```bash
rtk git add Cargo.toml Cargo.lock crates/rollshot-macos-oneshot crates/rollshot-capture/Cargo.toml \
  .github/workflows/ci.yml \
  crates/rollshot-capture/src/macos/mod.rs crates/rollshot-capture/src/macos/one_shot.rs
rtk git commit -m "feat(capture): add macOS one-shot screenshot backend"
```

### Task 6: Introduce the Mode-Aware Overlay Session

**Files:**
- Create: `crates/rollshot-iced-overlay/src/session.rs`
- Create: `crates/rollshot-iced-overlay/src/screenshot.rs`
- Modify: `crates/rollshot-capture/src/crop.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Modify: `crates/rollshot-iced-overlay/src/app.rs`
- Modify: `crates/rollshot-iced-overlay/src/driver.rs`
- Modify: `crates/rollshot-iced-overlay/src/bin/capture_overlay.rs`
- Modify: `crates/rollshot-tauri-app/src-tauri/src/native_capture.rs`

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
    Cancel,
}
```

Tests must prove:

- scrolling drag release emits `StartScrollingCapture`;
- screenshot drag release emits `FinishScreenshot` immediately;
- an empty screenshot click emits `None`;
- `Esc` before a valid selection cancels;
- screenshot state owns its frozen image and scrolling state owns confirmation/preview state;
- screenshot activation creates its iced image handle exactly once; `view()`
  only clones the cheap handle and never copies pixel bytes per redraw;
- screenshot crop returns `CaptureResult { stats: None }`;
- driver finalization returns `stats: Some(...)`.
- session construction accepts either workflow without embedding platform
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
    let region = map_crop_to_frame(crop, overlay_logical, capture.target_display().physical_size);
    let image = crop_image(capture.image(), region).map_err(|e| e.to_string())?;
    Ok(CaptureResult { image, stats: None })
}
```

Extract `crop_image(&RgbaImage, Region) -> Result<RgbaImage, CaptureError>` in
`rollshot-capture/src/crop.rs`, make existing `crop_frame` delegate to it, and
test that both paths preserve the existing bounds validation. This avoids
cloning a full-screen image during screenshot finalization.

Change `CaptureResult.stats` to `Option<StitchStats>`, remove its unused
full-image `Clone` implementation, and update driver results to `Some(stats)`.
Render the frozen image beneath the existing crop canvas only in screenshot
workflow. Build the iced RGBA handle once when constructing the screenshot
workflow, not inside `view()`. Keep workflow construction mode-aware, but do not
add an unused switch message or runtime transition in this MVP.

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
rtk git add crates/rollshot-capture/src/crop.rs crates/rollshot-capture/src/lib.rs \
  crates/rollshot-iced-overlay/src/session.rs crates/rollshot-iced-overlay/src/screenshot.rs \
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
enum CaptureResource {
    Streaming(Driver),
    OneShot(OneShotCapture),
}

fn acquire_resource(
    mode: CaptureMode,
    config: &OverlayConfig,
) -> Result<CaptureResource, OverlayError>;
```

Inject fake streaming and one-shot factories. Tests must prove:

- `Scrolling` calls only the streaming factory;
- `Screenshot` calls only the one-shot factory;
- `acquire_resource` can be called again for another mode after the previous
  resource is dropped, establishing the future toolbar transition boundary;
- KWin target output produces `StartMode::TargetScreen(name)`;
- a KWin capture with missing output name is rejected before layer-shell starts;
- an unresolved portal target opens with `StartMode::Active` and must pass the
  post-open mapping gate;
- screenshot finish uses frozen-image crop and never finalizes a driver.
- portal cancellation before the overlay opens returns `Ok(None)`, not a
  user-visible capture error.
- screenshot mode runs with no preview receiver/subscription and does not
  panic.

- [ ] **Step 2: Run focused tests and verify failure**

Run: `rtk cargo test -p rollshot-iced-overlay linux_runner`

Expected: tests fail because Linux startup always constructs `Driver`.

- [ ] **Step 3: Implement workflow-independent Linux runner effects**

At startup, call `acquire_resource(config.initial_mode, &config)`, then create
`OverlaySession`. Keep acquisition separate from iced application startup so a
future toolbar transition can reuse it. Do not install preview subscriptions or
ticks for screenshot mode; an absent preview receiver must not panic.

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
Map `CaptureError::UserCancelled` from the portal acquisition path to
`Ok(None)`. Do not add an unused switch effect; future toolbar work will drop
the current `CaptureResource`, call `acquire_resource` again, and rebuild the
mode-specific overlay session.
After the layer surface opens, validate both KWin and portal captures against
the actual surface logical size/scale before rendering; output-name mismatch or
geometry mismatch is an explicit `CaptureError::Mapping`.

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
- the shared startup shape can reacquire a different resource after the prior
  resource is dropped;
- macOS screenshot window placement resolves the target `display_id` to the
  matching `NSScreen` and preserves signed/negative display origins;
- macOS screenshot window size uses that target display's logical size and
  backing scale;
- screenshot mode never opens the scrolling controls window or enables passthrough;
- valid screenshot release immediately returns a no-stats result.
- screenshot mode runs with no preview receiver/subscription and does not
  panic.

- [ ] **Step 2: Run focused tests and verify failure**

Run: `rtk cargo test -p rollshot-iced-overlay macos_runner`

Expected: tests fail because macOS startup always creates `Driver` and uses the main screen.

- [ ] **Step 3: Implement mode-aware macOS startup and effects**

Add a `macos_window` helper that matches the target CoreGraphics display ID to
`NSScreenNumber` and returns its logical frame/backing scale. Use that resolved
screen for screenshot window placement, preserving negative origins; do not
silently fall back to `NSScreen::mainScreen`. A missing ID or geometry mismatch
is an explicit mapping error. Keep existing scrolling placement and controls
behavior unchanged.

Handle effects as follows:

- `StartScrollingCapture`: begin stitch and enable passthrough/open controls.
- `FinishScrolling`: finalize driver.
- `FinishScreenshot`: crop frozen image, set result, and exit without passthrough.
- `Cancel`: release whichever resource exists and exit.

Factor resource acquisition from iced daemon startup, matching the Linux runner
shape. Future toolbar work can reuse that function and add window repositioning
when it introduces an actual switch message.
Do not install preview subscriptions or the scrolling controls window in
screenshot mode. Ensure an absent preview receiver is a normal mode state, not
an `expect()` panic.

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

Document that KDE exact-display normal screenshot support requires installing
this desktop entry to `$XDG_DATA_HOME/applications` for a user install or
`/usr/share/applications` for a system install, then launching a binary matching
its `Exec` identity; direct development binaries may receive an explicit
permission error. Document the `initial_mode` JSON examples and state plainly
that non-KDE portal screenshot mode accepts only provable single-output results
and rejects multi-monitor composites. Document that non-KDE portal screenshot
mode rejects `show_cursor = true` because the Screenshot portal has no cursor
inclusion option.

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
rtk rg -n "Driver::start_capture|CaptureBackend|FrameStream|SCStream::|screencast::|pipewire::" \
  crates/rollshot-capture/src/one_shot.rs \
  crates/rollshot-capture/src/linux/kwin_screenshot.rs \
  crates/rollshot-capture/src/linux/portal_screenshot.rs \
  crates/rollshot-capture/src/macos/one_shot.rs \
  crates/rollshot-macos-oneshot/src/lib.rs \
  crates/rollshot-iced-overlay/src/screenshot.rs
```

Expected: no matches. `SCStreamConfiguration` is permitted because
`SCScreenshotManager` uses it, but screenshot mode must not construct or start
`SCStream`.

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
- a provable single-output result shows the correct frozen output;
- a multi-monitor composite is rejected explicitly;
- `show_cursor = true` is rejected explicitly rather than silently ignored;
- unavailable mapping exits explicitly instead of showing an incorrectly scaled image.

- [ ] **Step 5: Verify macOS runtime**

Run the same command on macOS 14 or newer.

Verify:

- Screen Recording permission failure is explicit;
- after permission is granted, the display under the pointer is frozen;
- a secondary display positioned left/above the primary opens the overlay on
  that exact display without clamping its negative origin;
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

## Test Coverage Matrix

```text
Task / behavior                                      Unit  Integ  E2E/smoke  Manual
───────────────────────────────────────────────────  ────  ─────  ─────────  ──────
1 / initial_mode default + forwarding                 yes    yes       —        no
2 / backend policy + limits + no stream adapter       yes    —         —        no
3 / KWin metadata, Qt formats, timeout, no fallback   yes    fake DBus —        KDE
4 / portal cancellation, URI, single-output gate      yes    fake portal —      Wayland
5 / macOS typed errors, callback, BGRA conversion     yes    adapter    CI       macOS
6 / workflow effects, borrowed crop, optional stats   yes    crate     —        no
7 / Linux acquisition, geometry gate, no preview      yes    runner    —        Linux
8 / macOS acquisition, placement, no controls         yes    runner    CI       macOS
9 / desktop permission declaration + docs             —      contract  —        KDE
10 / workspace regression + platform behavior         —      yes       yes      all
```

### Test State Diagram

```text
             fake backend / synthetic pixels
                         |
                         v
policy -> acquire_resource(mode) -> OverlaySession -> effect -> CaptureResult
  |              |                    |                |
errors       no wrong factory      interaction      stats/crop
  |              |                    |                |
unsupported, timeout, mapping, permission, cancellation, empty selection

Real KDE / non-KDE Wayland / macOS runs are reserved for Task 10 manual smoke
tests; automated tests must not require DBus services, portals, GUI state,
Screen Recording permission, sleeping, or real capture.
```

## Failure Modes

| Code path | Production failure | Planned coverage | User-visible result |
|---|---|---|---|
| Backend policy | Unsupported OS/session or explicit streaming backend flag | Task 2 / Steps 1-4 | Explicit unsupported/config error |
| KWin DBus | Service absent, permission denied, timeout, malformed Qt metadata, short pipe read | Task 3 / Steps 1-4 | Explicit capture/mapping/timeout error; no portal fallback |
| Screenshot portal | User cancels, timeout, non-file URI, oversized PNG, multi-output composite | Task 4 / Steps 1-4 and Task 7 | Cancellation returns no result; other cases return explicit error |
| macOS one-shot | macOS <14, Screen Recording denied, callback timeout, invalid BGRA buffer | Task 5 / Steps 1-4 | Typed unsupported/permission/timeout/capture error |
| Frozen crop | Empty/out-of-bounds selection or HiDPI mapping mismatch | Task 6 / Steps 1-4 | Empty stays active; invalid mapping returns explicit error |
| Linux runner | Target output name or opened-surface geometry mismatch | Task 7 / Steps 1-4 | Explicit mapping error before rendering |
| macOS runner | Wrong target geometry or absent preview receiver | Task 8 / Steps 1-4 | Explicit error; no panic |
| KDE permission declaration | Binary launched without installed identity | Task 9 and Task 10 / Step 3 | Explicit KWin permission error documented |

No failure path is intentionally silent. Task 10 runtime checks remain required
because compositor, portal, and TCC behavior cannot be fully proven by unit
tests.

## What Already Exists

- `rollshot-capture::crop_frame` already owns crop bounds validation. Task 6
  extracts `crop_image` from it instead of creating a second validator.
- `rollshot-iced-overlay::coords::map_crop_to_frame` already handles logical to
  physical crop mapping and its HiDPI tests remain the source of truth.
- `rollshot-iced-overlay::Driver` remains the scrolling-only stream/stitch
  owner; screenshot mode does not modify or adapt it.
- Existing Linux portal runtime/error patterns and `CaptureError` variants are
  reused for timeout, cancellation, permission, and backend failures.
- Existing macOS window patching and placement helpers are extended rather than
  replaced.
- Spectacle's `ScreenShot2` request/metadata contract and Flameshot/Spectacle
  desktop permission declaration are reference behavior, not copied runtime
  dependencies.

## Review References

- XDG Screenshot portal API:
  <https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.impl.portal.Screenshot.html>
- Apple `SCScreenshotManager`:
  <https://developer.apple.com/documentation/screencapturekit/scscreenshotmanager>
- Apple `SCContentFilter`:
  <https://developer.apple.com/documentation/screencapturekit/sccontentfilter>
- Local Spectacle protocol reference:
  `learn-projects/spectacle/src/Platforms/ImagePlatformKWin.cpp`
- Local desktop permission references:
  `learn-projects/spectacle/desktop/org.kde.spectacle.desktop.cmake` and
  `learn-projects/flameshot/data/desktopEntry/package/org.flameshot.Flameshot.desktop`

## NOT in Scope

- Toolbar UI and an actual in-session mode transition: acquisition boundaries
  are prepared, but unused switching behavior is deferred until it is testable
  through UI.
- Multi-monitor non-KDE portal cropping: Screenshot portal lacks reliable
  pointer/output identity, so ambiguous composites are rejected.
- Annotations, clipboard output, delayed capture, window/element detection,
  selection adjustment, and cross-display selections: outside the MVP goal.
- Automated package publishing/installers: the desktop entry and manual
  installation requirement are documented; repository-wide release packaging
  is a separate project.
- Supporting every Qt `QImage::Format`: only explicitly fixture-tested 32-bit
  formats are accepted; unknown formats fail explicitly.
- macOS below 14 and any stream-first-frame fallback: both are explicitly
  unsupported.

## Task Dependencies and Execution

| Task | Modules touched | Depends on |
|---|---|---|
| 1 | capture types, app launch, overlay contracts, retained Tauri | — |
| 2 | capture one-shot contract/errors | 1 |
| 3 | capture Linux/KWin, workspace dependencies | 2 |
| 4 | capture Linux/portal | 2, 3 |
| 5 | capture macOS, macOS isolation crate, CI, workspace root | 2 |
| 6 | capture crop, overlay session/app/driver | 1, 2 |
| 7 | overlay Linux runner | 3, 4, 6 |
| 8 | overlay macOS runner/window | 5, 6 |
| 9 | Linux packaging/docs | 3 |
| 10 | whole workspace/platform runtime | 1-9 |

Theoretical parallel lanes:

```text
Lane A: 1 -> 2 -> 3 -> 4 -> 7
Lane B:          5 --------> 8
Lane C:          6 --------> 7/8
Lane D:          9
Final:                       10
```

Tasks 3 and 5 both modify root `Cargo.toml`; Tasks 3-5 share
`crates/rollshot-capture/`; Tasks 7 and 8 share
`crates/rollshot-iced-overlay/`. Because project rules prohibit automatic
worktree setup and these lanes have conflict risk, execute them sequentially in
the numbered order unless a human explicitly coordinates separate worktrees.

## Engineering Review Summary

- Plan reviewed: `docs/superpowers/plans/2026-06-06-normal-screenshot-mode.md`
- Tasks in plan: 10
- Files Create/Modify: 10 create / 21 existing files modify
- Step 0 Scope Challenge: accepted after narrowing non-KDE support to provable
  single-output results and deferring unused runtime switching. Complexity gate
  did not trigger: 10 new files, 1 new crate, 10 tasks.
- Architecture Review: 6 issues auto-applied: KWin metadata contract,
  non-KDE mapping claim, unused switch lifecycle, typed macOS errors, shared
  geometry invariant, exact signed macOS display placement.
- Plan Structure and Code Quality: 4 issues auto-applied: reuse existing crop
  validation, explicit acquisition boundary, explicit CI ownership, complete
  file/Cargo.lock lists.
- Test Review: 6 gaps auto-applied: timeout/limit negatives, preview-absent
  mode, BGRA/Qt format fixtures, cancellation semantics, cursor capability
  behavior, negative-origin macOS placement.
- Performance and Resource Review: 4 issues auto-applied: remove full-image
  finalization clone, prohibit resource cloning, bound decoded pixels for the
  two-buffer render model, require checked row/byte calculations.
- Critical silent failure gaps remaining: 0.
- Parallelization: 4 theoretical lanes; numbered sequential execution
  recommended because of shared crate/root files and the no-worktree rule.
- Unresolved decisions: 0.
