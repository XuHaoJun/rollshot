# Fullscreen Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `initial_mode: "fullscreen"` to capture the display containing the pointer without opening selection UI, while renaming the existing crop workflow from `screenshot` to `region`.

**Architecture:** Keep `CaptureMode` as the launch contract, but route `Fullscreen` before constructing active overlay state. Region and fullscreen share the existing one-shot backend; region opens the overlay and crops, while fullscreen converts the untouched one-shot image directly into `CaptureResult` and enters the existing Linux/macOS post-capture presentation.

**Tech Stack:** Rust 1.85, serde/serde_json, iced 0.14, iced_layershell, KWin ScreenShot2 DBus, macOS SCScreenshotManager, Cargo tests/clippy/fmt.

---

## File Structure

- Modify `crates/rollshot-capture/src/types.rs`: define `Scrolling | Region | Fullscreen` JSON contract and legacy `"screenshot"` alias.
- Modify `crates/rollshot-capture/src/one_shot.rs`: add fullscreen-only backend selection and consuming image access.
- Modify `crates/rollshot-capture/src/lib.rs`: export `fullscreen_one_shot_backend_for` (Task 2).
- Rename `crates/rollshot-iced-overlay/src/screenshot.rs` to `crates/rollshot-iced-overlay/src/region.rs`: keep crop-only finalization under region terminology.
- Create `crates/rollshot-iced-overlay/src/fullscreen.rs`: own direct one-shot-to-`CaptureResult` completion.
- Modify `crates/rollshot-iced-overlay/src/lib.rs`: register/export region and fullscreen capture boundaries.
- Modify `crates/rollshot-iced-overlay/src/app.rs`: migrate active overlay terminology to region.
- Modify `crates/rollshot-iced-overlay/src/workspace.rs`: migrate region workflow state/effects.
- Modify `crates/rollshot-iced-overlay/src/toolbar.rs`: expose Region and Scrolling only.
- Modify `crates/rollshot-iced-overlay/src/linux_runner.rs`: route fullscreen before layer-shell application startup.
- Modify `crates/rollshot-iced-overlay/src/macos_capture.rs`: keep the embedded component limited to Region/Scrolling.
- Modify `crates/rollshot-app/src/macos_product.rs`: bootstrap fullscreen directly into existing thumbnail/workspace presentation.
- Modify `crates/rollshot-app/src/launch.rs`: verify the expanded launch JSON contract.
- Modify `crates/rollshot-cli/src/cmd_capture_launcher.rs`: verify the existing launcher still defaults to Scrolling.
- Modify `README.md`: document Region, Fullscreen, legacy alias, and platform limits.

### Task 1: Rename Screenshot Mode to Region and Expand the Launch Contract

**Files:**
- Modify: `crates/rollshot-capture/src/types.rs`
- Modify: `crates/rollshot-iced-overlay/src/app.rs`
- Modify: `crates/rollshot-iced-overlay/src/workspace.rs`
- Modify: `crates/rollshot-iced-overlay/src/toolbar.rs`
- Modify: `crates/rollshot-iced-overlay/src/linux_runner.rs`
- Modify: `crates/rollshot-iced-overlay/src/macos_capture.rs`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Rename: `crates/rollshot-iced-overlay/src/screenshot.rs` to `crates/rollshot-iced-overlay/src/region.rs`
- Modify: `crates/rollshot-app/src/macos_product.rs`
- Test: inline unit tests in the files above

- [ ] **Step 1: Write failing JSON contract tests**

Replace the single round-trip case in `crates/rollshot-capture/src/types.rs` with explicit current-name and compatibility tests:

```rust
#[test]
fn capture_modes_round_trip_with_current_names() {
    for (mode, encoded) in [
        (CaptureMode::Scrolling, "\"scrolling\""),
        (CaptureMode::Region, "\"region\""),
        (CaptureMode::Fullscreen, "\"fullscreen\""),
    ] {
        assert_eq!(serde_json::to_string(&mode).unwrap(), encoded);
        assert_eq!(serde_json::from_str::<CaptureMode>(encoded).unwrap(), mode);
    }
}

#[test]
fn legacy_screenshot_mode_deserializes_as_region() {
    let mode = serde_json::from_str::<CaptureMode>("\"screenshot\"").unwrap();
    assert_eq!(mode, CaptureMode::Region);
    assert_eq!(serde_json::to_string(&mode).unwrap(), "\"region\"");
}
```

- [ ] **Step 2: Run the contract tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-capture types::tests
```

Expected: compilation fails because `CaptureMode::Region` and `CaptureMode::Fullscreen` do not exist.

- [ ] **Step 3: Implement the mode contract**

Change `CaptureMode` in `crates/rollshot-capture/src/types.rs` to:

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureMode {
    #[default]
    Scrolling,
    #[serde(alias = "screenshot")]
    Region,
    Fullscreen,
}
```

Keep `InteractiveLaunchOptions::default_capture()` on `CaptureMode::Scrolling`.

- [ ] **Step 4: Mechanically migrate active overlay terminology**

Perform these scoped renames only in the listed capture/overlay/app files:

```text
CaptureMode::Screenshot              -> CaptureMode::Region
ToolbarAction::ScreenshotMode        -> ToolbarAction::RegionMode
WorkspaceEffect::FinishScreenshot    -> WorkspaceEffect::FinishRegion
OverlayEffect::FinishScreenshot      -> OverlayEffect::FinishRegion
finish_screenshot                    -> finish_region
crate::screenshot::finish_screenshot -> crate::region::finish_region   # call sites in linux_runner.rs AND macos_capture.rs
validate_screenshot_surface_or_exit  -> validate_region_surface_or_exit  # linux_runner.rs
validate_screenshot_surface          -> validate_region_surface          # macos_capture.rs
screenshot.rs                        -> region.rs
```

Also update the `from_environment` error string in `one_shot.rs`
(`"screenshot mode only accepts 'auto' backend, got '{backend_flag}'"`) to say
`"region mode ..."` — it describes the renamed one-shot/region workflow, not a
platform API.

Update user-visible toolbar text from `"Screenshot Mode"` to `"Region Mode"`.
Update comments and test names that describe the old workflow as screenshot
mode (e.g. `screenshot_calls_only_one_shot_factory`,
`screenshot_surface_validation_*`, `clear_screenshot_globals`,
`finish_screenshot_*`, `screenshot_release_*`). Do **not** rename platform API
names: `SCScreenshotManager`, `KwinScreenshotClient`, the freedesktop
Screenshot portal, the KWin `ScreenShot2` interface, or the
`OneShotBackendKind::MacosScreenshotManager` variant (it names the macOS
ScreenshotManager API, not the workflow).

> **Review note (rename completeness):** the original Step 4 list omitted
> `validate_screenshot_surface_or_exit`, `validate_screenshot_surface`, and the
> `crate::screenshot::` module path. A half-rename still compiles and passes
> tests, so the gap is invisible to the test suite — the Step 4-of-Task-6 grep
> (widened below) is the only guard. Rename all workflow identifiers in one
> pass.

Use:

```bash
rtk git mv crates/rollshot-iced-overlay/src/screenshot.rs crates/rollshot-iced-overlay/src/region.rs
```

Update `crates/rollshot-iced-overlay/src/lib.rs`:

```rust
pub mod region;
```

- [ ] **Step 5: Keep Fullscreen out of active overlay matches**

Where active overlay code matches `CaptureMode`, add an explicit guard before
constructing overlay state rather than treating Fullscreen as an interactive
mode. For pure overlay-only matches, use an unreachable arm with a concrete
message:

```rust
CaptureMode::Fullscreen => {
    unreachable!("fullscreen is routed before active overlay state")
}
```

Rust exhaustiveness checking means the compiler flags every match that needs
this arm; Step 6's build will not pass until all are handled. The known sites
(verify against code — the count may shift):

- `linux_runner::acquire_resource` (one `match mode { Scrolling, Region }`)
- `macos_capture::acquire_resource` (a **separate** `match mode { ... }` — easy
  to miss because it mirrors the Linux one)
- `app.rs` mode dispatch arms: the finish-on-release / Enter / toolbar-finish
  paths (`CaptureMode::Region => { … FinishRegion }`) and the `view` background
  arm (`(Some(handle), CaptureMode::Region)`). These are reached only while the
  interactive overlay is live, so `unreachable!` is correct for Fullscreen.

`toolbar.rs::action_style_fn` uses `matches!(…)` (not an exhaustive `match`), so
it needs no Fullscreen arm. Toolbar actions remain exactly Region, Scrolling,
Finish, and Cancel.

- [ ] **Step 6: Run focused tests**

Run:

```bash
rtk cargo test -p rollshot-capture types::tests
rtk cargo test -p rollshot-iced-overlay
rtk cargo test -p rollshot-app launch::tests
```

Expected: all pass; warnings about obsolete `Screenshot` variants are absent.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-capture/src/types.rs crates/rollshot-iced-overlay/src crates/rollshot-app/src/macos_product.rs
rtk git commit -m "feat(capture): define region and fullscreen modes"
```

### Task 2: Add Fullscreen-Only Backend Selection

**Files:**
- Modify: `crates/rollshot-capture/src/one_shot.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`
- Test: `crates/rollshot-capture/src/one_shot.rs`

- [ ] **Step 1: Write failing backend-policy tests**

Add tests in `crates/rollshot-capture/src/one_shot.rs`:

```rust
#[test]
fn fullscreen_linux_kde_selects_kwin() {
    assert_eq!(
        fullscreen_one_shot_backend_for("auto", "linux", Some("wayland"), Some("KDE")).unwrap(),
        OneShotBackendKind::LinuxKwin
    );
}

#[test]
fn fullscreen_linux_non_kde_is_unsupported() {
    let err = fullscreen_one_shot_backend_for(
        "auto",
        "linux",
        Some("wayland"),
        Some("GNOME"),
    )
    .unwrap_err();
    assert!(matches!(err, CaptureError::Unsupported { .. }));
}

#[test]
fn fullscreen_rejects_explicit_portal_backend() {
    let err = fullscreen_one_shot_backend_for(
        "linux-portal",
        "linux",
        Some("wayland"),
        Some("KDE"),
    )
    .unwrap_err();
    assert!(matches!(err, CaptureError::Unsupported { .. }));
}

#[test]
fn fullscreen_macos_selects_screenshot_manager() {
    assert_eq!(
        fullscreen_one_shot_backend_for("auto", "macos", None, None).unwrap(),
        OneShotBackendKind::MacosScreenshotManager
    );
}
```

Add a consuming-image test:

```rust
#[test]
fn into_image_returns_the_original_pixels() {
    let capture = OneShotCapture::new(
        RgbaImage::from_pixel(1, 1, image::Rgba([1, 2, 3, 255])),
        DisplayTarget {
            output_name: Some("test".to_string()),
            logical_region: Region { x: 0, y: 0, width: 1, height: 1 },
            physical_size: Size { width: 1, height: 1 },
        },
    )
    .unwrap();
    let image = capture.into_image();
    assert_eq!(image.dimensions(), (1, 1));
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-capture one_shot::tests
```

Expected: compilation fails because `fullscreen_one_shot_backend_for` and
`OneShotCapture::into_image` do not exist.

- [ ] **Step 3: Implement fullscreen backend policy**

Add this pure selector beside `one_shot_backend_for`:

```rust
pub fn fullscreen_one_shot_backend_for(
    backend_flag: &str,
    os: &str,
    session_type: Option<&str>,
    desktop: Option<&str>,
) -> Result<OneShotBackendKind, CaptureError> {
    if backend_flag != "auto" {
        return Err(CaptureError::Unsupported {
            message: format!(
                "fullscreen capture only supports backend 'auto', got '{backend_flag}'"
            ),
        });
    }

    match (os, session_type) {
        ("linux", Some("wayland")) if is_kde(desktop) => Ok(OneShotBackendKind::LinuxKwin),
        ("linux", _) => Err(CaptureError::Unsupported {
            message: "fullscreen capture requires KDE/KWin on Linux".to_string(),
        }),
        ("macos", _) => Ok(OneShotBackendKind::MacosScreenshotManager),
        _ => Err(CaptureError::Unsupported {
            message: format!("fullscreen capture is unsupported on {os}"),
        }),
    }
}
```

Add:

```rust
pub fn from_fullscreen_environment(backend_flag: &str) -> Result<Self, CaptureError> {
    let session_type = std::env::var("XDG_SESSION_TYPE").ok();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    fullscreen_one_shot_backend_for(
        backend_flag,
        std::env::consts::OS,
        session_type.as_deref(),
        desktop.as_deref(),
    )
}
```

Keep `OneShotBackendKind::capture_once` exactly as it is — it stays
`#[cfg(not(test))]`. `rollshot-iced-overlay`'s `fullscreen::capture` (Task 3)
and `linux_runner` see `rollshot-capture` as a **non-test** dependency, so the
`#[cfg(not(test))]` method is already present when those crates (including their
test builds) compile. None of this task's new tests call `capture_once` — they
exercise the pure `fullscreen_one_shot_backend_for` selector and `into_image`.
Only relax the gate if a concrete compile error proves it necessary (it
shouldn't); widening it pulls platform dispatch into `rollshot-capture`'s own
test binary for no benefit (§3 surgical changes).

> **Review note (error variant):** `fullscreen_one_shot_backend_for` returns
> `CaptureError::Unsupported` for a non-`auto` backend flag, while the sibling
> `from_environment` returns `CaptureError::InvalidConfig` for the same case.
> This is a deliberate choice — for fullscreen, "you asked for a specific
> backend we can't honor here" reads as an environment/support limitation. The
> Task-2 test `fullscreen_rejects_explicit_portal_backend` pins `Unsupported`.
> Left intentionally divergent; do not "fix" it to match `from_environment`.

Add to `OneShotCapture`:

```rust
pub fn into_image(self) -> RgbaImage {
    self.image
}
```

Export `fullscreen_one_shot_backend_for` from `crates/rollshot-capture/src/lib.rs`.

- [ ] **Step 4: Run focused tests**

Run:

```bash
rtk cargo test -p rollshot-capture one_shot::tests
rtk cargo test -p rollshot-capture
```

Expected: all pass. Existing Region selection still chooses the portal on
non-KDE Wayland; only fullscreen rejects it.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-capture/src/one_shot.rs crates/rollshot-capture/src/lib.rs
rtk git commit -m "feat(capture): restrict fullscreen to native one-shot backends"
```

### Task 3: Build the Shared Direct Fullscreen Completion

**Files:**
- Create: `crates/rollshot-iced-overlay/src/fullscreen.rs`
- Modify: `crates/rollshot-iced-overlay/src/lib.rs`
- Test: `crates/rollshot-iced-overlay/src/fullscreen.rs`

- [ ] **Step 1: Write failing direct-completion tests**

Add to `crates/rollshot-iced-overlay/src/fullscreen.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use rollshot_capture::{CaptureMode, DisplayTarget, Region, Size};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn config(mode: CaptureMode) -> OverlayConfig {
        OverlayConfig {
            backend: "auto".to_string(),
            fps: 5,
            show_cursor: false,
            initial_mode: mode,
            target_output_name: None,
        }
    }

    fn one_shot() -> rollshot_capture::OneShotCapture {
        rollshot_capture::OneShotCapture::new(
            RgbaImage::from_pixel(2, 1, Rgba([1, 2, 3, 255])),
            DisplayTarget {
                output_name: Some("display".to_string()),
                logical_region: Region { x: 0, y: 0, width: 2, height: 1 },
                physical_size: Size { width: 2, height: 1 },
            },
        )
        .unwrap()
    }

    #[test]
    fn fullscreen_returns_the_unchanged_one_shot_image() {
        let result = capture_with(&config(CaptureMode::Fullscreen), |_| Ok(one_shot()))
            .unwrap()
            .unwrap();
        assert_eq!(result.image.dimensions(), (2, 1));
        assert_eq!(result.image.get_pixel(0, 0).0, [1, 2, 3, 255]);
        assert!(result.stats.is_none());
    }

    #[test]
    fn fullscreen_invokes_only_one_shot_acquisition() {
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        CALLS.store(0, Ordering::SeqCst);
        capture_with(&config(CaptureMode::Fullscreen), |_| {
            CALLS.fetch_add(1, Ordering::SeqCst);
            Ok(one_shot())
        })
        .unwrap();
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cancellation_produces_no_result() {
        let result = capture_with(&config(CaptureMode::Fullscreen), |_| {
            Err(rollshot_capture::CaptureError::UserCancelled)
        })
        .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn non_fullscreen_mode_is_rejected() {
        let err = capture_with(&config(CaptureMode::Region), |_| Ok(one_shot())).unwrap_err();
        assert!(matches!(err, OverlayError::Capture(_)));
    }
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay fullscreen::tests
```

Expected: compilation fails because `capture_with` does not exist.

- [ ] **Step 3: Implement direct completion**

Implement `crates/rollshot-iced-overlay/src/fullscreen.rs`. Open it with a
module doc-comment carrying the routing diagram, so the fact that fullscreen
bypasses the overlay on *both* platforms is legible at the call site:

```rust
//! Direct fullscreen completion: one-shot capture straight to `CaptureResult`,
//! no selection overlay, no streaming/stitching. Routed *before* any overlay
//! state on both platforms — this module owns the shared completion; the two
//! platform entry points only decide whether to call it.
//!
//!   launch JSON: initial_mode
//!          │
//!          ├─ "fullscreen" ──┐
//!          │                 ▼
//!          │   ┌─────────────────────────────────────────────┐
//!          │   │ Linux:  linux_runner::run_initial_path       │
//!          │   │ macOS:  MacosProduct::new (initial_capture_  │
//!          │   │         path == Fullscreen)                  │
//!          │   └─────────────────────────────────────────────┘
//!          │                 │ both call
//!          │                 ▼
//!          │       fullscreen::capture(config)
//!          │                 │  from_fullscreen_environment → capture_once
//!          │                 ▼
//!          │       Ok(Some(CaptureResult{ stats: None }))   ── existing
//!          │       Ok(None)  on UserCancelled                  presentation
//!          │       Err(..)   on Unsupported / backend error    (Workspace /
//!          │                                                    thumbnail)
//!          └─ "region" | "scrolling" ─► overlay session (unchanged)
//!
use crate::{CaptureResult, OverlayConfig, OverlayError};
use rollshot_capture::{CaptureError, CaptureMode, OneShotCapture};

pub(crate) fn capture_with<F>(
    config: &OverlayConfig,
    capture_once: F,
) -> Result<Option<CaptureResult>, OverlayError>
where
    F: FnOnce(bool) -> Result<OneShotCapture, CaptureError>,
{
    if config.initial_mode != CaptureMode::Fullscreen {
        return Err(OverlayError::Capture(
            "direct fullscreen completion requires fullscreen mode".to_string(),
        ));
    }

    match capture_once(config.show_cursor) {
        Ok(capture) => Ok(Some(CaptureResult {
            image: capture.into_image(),
            stats: None,
        })),
        Err(CaptureError::UserCancelled) => Ok(None),
        Err(error) => Err(OverlayError::Capture(error.to_string())),
    }
}

pub fn capture(config: &OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    tracing::info!(
        target: crate::diagnostics::TARGET_OVERLAY,
        mode = ?config.initial_mode,
        backend = %config.backend,
        show_cursor = config.show_cursor,
        "direct fullscreen capture starting"
    );
    let kind = rollshot_capture::OneShotBackendKind::from_fullscreen_environment(&config.backend)
        .map_err(|error| OverlayError::Capture(error.to_string()))?;
    let result = capture_with(config, |show_cursor| kind.capture_once(show_cursor));
    tracing::info!(
        target: crate::diagnostics::TARGET_OVERLAY,
        outcome = match &result {
            Ok(Some(_)) => "completed",
            Ok(None) => "cancelled",
            Err(_) => "failed",
        },
        "direct fullscreen capture finished"
    );
    result
}
```

Register the module in `crates/rollshot-iced-overlay/src/lib.rs`:

```rust
pub mod fullscreen;
```

Keep the module public enough for `rollshot-app` to call
`rollshot_iced_overlay::fullscreen::capture`, but keep `capture_with`
`pub(crate)` as the testable injection boundary. Keep diagnostics on the stable
`rollshot::*` overlay target and do not log image content or raw pixels.

- [ ] **Step 4: Run focused tests**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay fullscreen::tests
rtk cargo test -p rollshot-iced-overlay
```

Expected: all pass; direct fullscreen returns full native pixels and no stitch
stats.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-iced-overlay/src/fullscreen.rs crates/rollshot-iced-overlay/src/lib.rs
rtk git commit -m "feat(overlay): add direct fullscreen completion"
```

### Task 4: Route Linux Fullscreen Before Layer-Shell Startup

**Files:**
- Modify: `crates/rollshot-iced-overlay/src/linux_runner.rs`
- Test: `crates/rollshot-iced-overlay/src/linux_runner.rs`

- [ ] **Step 1: Write the failing no-overlay routing test**

Extract a small injectable runner boundary and add:

```rust
#[test]
fn fullscreen_routes_to_direct_capture_before_overlay_startup() {
    let mut config = test_config();
    config.initial_mode = CaptureMode::Fullscreen;
    let direct_calls = std::cell::Cell::new(0);
    let overlay_calls = std::cell::Cell::new(0);

    let result = run_initial_path(
        config,
        |_| {
            direct_calls.set(direct_calls.get() + 1);
            Ok(Some(CaptureResult {
                image: image::RgbaImage::new(2, 2),
                stats: None,
            }))
        },
        |_| {
            overlay_calls.set(overlay_calls.get() + 1);
            Ok(None)
        },
    )
    .unwrap();

    assert!(result.is_some());
    assert_eq!(direct_calls.get(), 1);
    assert_eq!(overlay_calls.get(), 0);
}
```

- [ ] **Step 2: Run the test and verify it fails**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay linux_runner::tests::fullscreen_routes_to_direct_capture_before_overlay_startup
```

Expected: compilation fails because `run_initial_path` does not exist.

- [ ] **Step 3: Extract the routing boundary**

Add:

```rust
fn run_initial_path<Direct, Overlay>(
    config: OverlayConfig,
    direct: Direct,
    overlay: Overlay,
) -> Result<Option<CaptureResult>, OverlayError>
where
    Direct: FnOnce(&OverlayConfig) -> Result<Option<CaptureResult>, OverlayError>,
    Overlay: FnOnce(OverlayConfig) -> Result<Option<CaptureResult>, OverlayError>,
{
    if config.initial_mode == CaptureMode::Fullscreen {
        return direct(&config);
    }
    overlay(config)
}
```

Rename the current `run` body to `run_overlay_session`. Its first statement
must reject `CaptureMode::Fullscreen` by returning
`Err(OverlayError::Capture("fullscreen must not reach the overlay runner"))`
**before** touching any global slot or calling `acquire_resource` — a clean
error here is the systems-over-heroes alternative to letting execution fall
through to `acquire_resource`'s `CaptureMode::Fullscreen => unreachable!()`,
which would panic in production if the routing invariant were ever violated.
Make the public runner:

```rust
pub fn run(config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    run_initial_path(config, crate::fullscreen::capture, run_overlay_session)
}
```

This guarantees fullscreen returns before globals, layer-shell settings, input
regions, or the iced application are initialized.

- [ ] **Step 4: Run Linux runner and package tests**

Run:

```bash
rtk cargo test -p rollshot-iced-overlay linux_runner::tests
rtk cargo test -p rollshot-app post_capture::tests
```

Expected: all pass; existing Linux saved/unsaved Result Workspace policy tests
remain unchanged.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-iced-overlay/src/linux_runner.rs
rtk git commit -m "feat(linux): bypass overlay for fullscreen capture"
```

### Task 5: Bootstrap macOS Fullscreen Into Existing Presentation

**Files:**
- Modify: `crates/rollshot-app/src/macos_product.rs`
- Test: `crates/rollshot-app/src/macos_product.rs`

> Run this task's macOS-specific tests on a macOS host. The module is
> `#[cfg(target_os = "macos")]` and is not compiled by Linux CI.

- [ ] **Step 1: Write failing bootstrap tests**

Add a pure route selector and presentation bootstrap tests:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitialCapturePath {
    Overlay,
    Fullscreen,
}

#[test]
fn fullscreen_selects_direct_initial_path() {
    assert_eq!(
        initial_capture_path(CaptureMode::Fullscreen),
        InitialCapturePath::Fullscreen
    );
    assert_eq!(
        initial_capture_path(CaptureMode::Region),
        InitialCapturePath::Overlay
    );
}

#[test]
fn fullscreen_success_bootstraps_existing_thumbnail_phase() {
    let product = MacosProduct::from_completed_image(
        image(),
        Ok(PathBuf::from("/tmp/fullscreen.png")),
    );
    assert!(matches!(product.phase, Phase::Thumbnail(_)));
}

#[test]
fn fullscreen_auto_save_failure_bootstraps_existing_workspace_phase() {
    let product = MacosProduct::from_completed_image(image(), Err("disk full".to_string()));
    assert!(matches!(product.phase, Phase::Workspace(_)));
}
```

- [ ] **Step 2: Run on macOS and verify the tests fail**

Run on macOS:

```bash
rtk cargo test -p rollshot-app macos_product::tests::fullscreen_
```

Expected: compilation fails because `initial_capture_path` and
`MacosProduct::from_completed_image` do not exist.

- [ ] **Step 3: Extract presentation construction**

Move the existing `rollshot_capture::CaptureMode` import out of `#[cfg(test)]`
because fullscreen routing now uses it in the active macOS product path.

Add:

```rust
fn initial_capture_path(mode: CaptureMode) -> InitialCapturePath {
    match mode {
        CaptureMode::Fullscreen => InitialCapturePath::Fullscreen,
        CaptureMode::Scrolling | CaptureMode::Region => InitialCapturePath::Overlay,
    }
}
```

Extract the body of `apply_capture_completion` into:

```rust
fn from_completed_image(
    image: RgbaImage,
    auto_save: Result<std::path::PathBuf, String>,
) -> Self
```

It must construct the same `Phase::Thumbnail` / `Phase::Workspace`, document,
and empty window-id fields that the existing transition uses. Rewrite
`apply_capture_completion` to assign the state produced by this constructor so
normal Region/Scrolling completion behavior does not diverge.

- [ ] **Step 4: Share presentation-window opening**

Extract the phase-based window-opening match from `complete_capture` into:

```rust
fn open_presentation_window(product: &mut MacosProduct) -> Task<Message>
```

`complete_capture` closes capture-owned windows, applies completion, then calls
this helper. Fullscreen bootstrap calls the same helper without any
capture-window close tasks.

The helper owns the full phase match and records the opened window id on
`product` (`thumbnail_window` / `workspace_window`), matching today's
`complete_capture`. The current thumbnail-settings-failure path early-returns
`Task::batch(close_tasks)` with an `iced::exit()` pushed onto the caller's local
`close_tasks`; since the helper has no access to that vector, it must instead
**return** `iced::exit()` as its own task on settings failure (after logging),
and the caller batches it with its close tasks. Net behavior is unchanged: the
durable saved file remains and the daemon exits. Confirm
`completed_capture_auto_save_*` tests stay green — they assert the resulting
`Phase`, which `from_completed_image` now sets, so the window-open extraction
must not alter phase selection.

- [ ] **Step 5: Route fullscreen before creating `Component`**

At the start of `MacosProduct::new`, branch on `initial_capture_path`:

```rust
InitialCapturePath::Fullscreen => {
    let result = match rollshot_iced_overlay::fullscreen::capture(&config)
        .map_err(|error| error.to_string())?
    {
        Some(result) => result,
        None => return Ok(None),
    };
    let auto_save = storage::auto_save(&result.image, Platform::Macos);
    let mut product = MacosProduct::from_completed_image(result.image, auto_save);
    let open_task = open_presentation_window(&mut product);
    Ok(Some((product, open_task)))
}
InitialCapturePath::Overlay => {
    let component = match Component::new(&config).map_err(|error| error.to_string())? {
        Some(component) => component,
        None => return Ok(None),
    };
    let (component, open_task) = open_capture_window(component, &config)?;
    Ok(Some((
        MacosProduct {
            phase: Phase::Capture(component),
            document: None,
            thumbnail_window: None,
            workspace_window: None,
            thumbnail_cursor: Point::ORIGIN,
        },
        open_task,
    )))
}
```

This branch must execute before `Component::new`, so fullscreen never opens
overlay or controls windows and never starts `SCStream`.

- [ ] **Step 6: Run macOS product tests**

Run on macOS:

```bash
rtk cargo test -p rollshot-app macos_product::tests
rtk cargo test -p rollshot-iced-overlay macos_capture::tests
```

Expected: all pass. Existing completed-capture thumbnail/workspace tests remain
green.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/macos_product.rs
rtk git commit -m "feat(macos): bootstrap fullscreen into product presentation"
```

### Task 6: Verify Launch Compatibility and Document Fullscreen

**Files:**
- Modify: `crates/rollshot-app/src/launch.rs`
- Modify: `crates/rollshot-cli/src/cmd_capture_launcher.rs`
- Modify: `README.md`
- Test: `crates/rollshot-app/src/launch.rs`
- Test: `crates/rollshot-cli/src/cmd_capture_launcher.rs`

- [ ] **Step 1: Add launch and launcher assertions**

Add to `crates/rollshot-app/src/launch.rs`:

```rust
#[test]
fn fullscreen_capture_payload_parses() {
    let mode = parse_launch_args([
        "rollshot-app",
        "--capture",
        r#"{"backend":"auto","fps":5,"show_cursor":false,"initial_mode":"fullscreen"}"#,
    ])
    .unwrap();
    assert!(matches!(
        mode,
        LaunchMode::Capture(options) if options.initial_mode == CaptureMode::Fullscreen
    ));
}

#[test]
fn legacy_screenshot_payload_parses_as_region() {
    let mode = parse_launch_args([
        "rollshot-app",
        "--capture",
        r#"{"backend":"auto","fps":5,"show_cursor":false,"initial_mode":"screenshot"}"#,
    ])
    .unwrap();
    assert!(matches!(
        mode,
        LaunchMode::Capture(options) if options.initial_mode == CaptureMode::Region
    ));
}
```

Update the existing CLI launcher test to assert:

```rust
assert_eq!(options.initial_mode, rollshot_capture::CaptureMode::Scrolling);
```

This confirms the existing `rollshot capture` command remains scrolling; do not
add a new CLI flag in this feature.

- [ ] **Step 2: Run launch tests**

Run:

```bash
rtk cargo test -p rollshot-app launch::tests
rtk cargo test -p rollshot-cli cmd_capture_launcher::tests
```

Expected: all pass.

- [ ] **Step 3: Update README**

In `README.md`:

- Replace region-workflow JSON examples using `"screenshot"` with `"region"`.
- Rename “screenshot mode” descriptions to “region mode” where they refer to
  the crop workflow.
- Add:

```json
{"backend":"auto","fps":5,"show_cursor":false,"initial_mode":"fullscreen"}
```

- State that old `"screenshot"` payloads remain accepted as a legacy alias for
  `"region"`.
- State that fullscreen captures the display containing the pointer, skips the
  selection overlay, supports macOS and KDE/KWin, and returns unsupported on
  other Linux environments without portal fallback.
- Keep the existing non-KDE portal limitations section explicitly scoped to
  Region mode.

- [ ] **Step 4: Run repository-wide terminology checks**

Run:

```bash
# (a) Workflow identifiers that MUST be gone after the rename:
rtk rg -n 'CaptureMode::Screenshot|ScreenshotMode|FinishScreenshot|finish_screenshot|validate_screenshot_surface|crate::screenshot' crates

# (b) Any remaining lowercase "screenshot", minus the intentionally retained
#     platform-API names and the legacy alias test, to eyeball the long tail
#     (comments, test fn names):
rtk rg -n -i 'screenshot' crates \
  | rtk rg -v 'SCScreenshotManager|KwinScreenshot|MacosScreenshotManager|ScreenShot2|portal_screenshot|PortalScreenshot|AshpdScreenshot|initial_mode":"screenshot"'
```

Expected: grep (a) returns nothing — no old Rust workflow identifiers remain.
Grep (b) returns only retained platform-API lines (SCScreenshotManager, KWin
ScreenShot2, freedesktop Screenshot portal, the MacosScreenshotManager variant)
and the explicit legacy `"screenshot"` alias compatibility tests; confirm every
remaining hit is one of those, not a missed workflow rename.

> **Review note:** the original single grep
> (`CaptureMode::Screenshot|initial_mode":"screenshot"|ScreenshotMode|FinishScreenshot`)
> would have passed even with `finish_screenshot`,
> `validate_screenshot_surface_or_exit`, and `crate::screenshot::` left
> un-renamed — those compile fine, so only this widened check catches the drift.

- [ ] **Step 5: Run full verification**

Run:

```bash
rtk cargo fmt --check
rtk cargo test
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all commands succeed.

No stitching benchmark is required because this feature does not change
`rollshot-core` matcher, canvas, verifier, or stitcher paths.

- [ ] **Step 6: Perform platform manual verification**

On KDE/KWin:

```bash
rtk cargo build --release -p rollshot-app
rtk target/release/rollshot-app --capture '{"backend":"auto","fps":5,"show_cursor":false,"initial_mode":"fullscreen"}'
```

Expected: the display containing the pointer is captured without a selection
overlay and opens in the Linux Result Workspace.

On non-KDE Linux, run the same command.

Expected: explicit unsupported error; no portal picker and no selection overlay.

On macOS:

```bash
rtk cargo run -p rollshot-app -- --capture '{"backend":"auto","fps":5,"show_cursor":false,"initial_mode":"fullscreen"}'
```

Expected: the display containing the pointer is captured without a selection
overlay and the existing saved-capture thumbnail appears.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/launch.rs crates/rollshot-cli/src/cmd_capture_launcher.rs README.md
rtk git commit -m "docs(capture): document fullscreen launch mode"
```

## Not in Scope

Considered and deliberately deferred:

- **New CLI flag for fullscreen.** `rollshot capture` stays Scrolling-only
  (Task 6 Step 1 asserts this). Fullscreen is reachable only via the
  `--capture` JSON contract; a dedicated flag is a follow-up if demand appears.
- **Linux portal fullscreen fallback.** Non-KDE Linux returns `Unsupported`
  with no portal fallback (the portal can't prove a single-output fullscreen
  image — same provable-single-output gate the Region path enforces). Adding a
  portal fullscreen path is out of scope.
- **Multi-display / "all displays" fullscreen.** Only the display containing the
  pointer is captured; stitching multiple monitors into one image is not
  attempted.
- **X11 / Windows fullscreen.** Only Wayland-KDE and macOS are supported.
- **`rollshot-core` stitching changes.** Fullscreen returns `stats: None` and
  touches no matcher/canvas/verifier/stitcher path — no benchmark required
  (Task 6 Step 5).
- **Renaming retained platform-API symbols** (`SCScreenshotManager`,
  `KwinScreenshotClient`, KWin `ScreenShot2`, freedesktop Screenshot portal,
  `OneShotBackendKind::MacosScreenshotManager`) — these name external APIs, not
  the workflow.

> **Manual-only verification (no hosted-CI coverage).** Task 5 (macOS bootstrap)
> compiles and unit-tests only on a macOS host; Task 6 Step 6's KDE/KWin,
> non-KDE-Linux, and macOS runs are manual. The `MacosProduct::new` fullscreen
> routing (Task 5 Step 5) calls the real `fullscreen::capture` and so is not
> unit-tested — its pieces (`initial_capture_path`, `from_completed_image`) are.

## Final Acceptance Check

- [ ] `initial_mode: "region"` preserves the existing crop workflow.
- [ ] Legacy `initial_mode: "screenshot"` parses as Region but never serializes back as `"screenshot"`.
- [ ] `initial_mode: "fullscreen"` captures the complete pointer display on KDE/KWin and macOS.
- [ ] Fullscreen creates no overlay or controls windows and starts no streaming/stitching resources.
- [ ] Non-KDE Linux fullscreen returns unsupported without portal fallback.
- [ ] Linux fullscreen enters the existing Result Workspace policy.
- [ ] macOS fullscreen enters the existing saved-thumbnail/unsaved-workspace policy.
- [ ] Default and CLI-launched capture remain Scrolling.
- [ ] Region and Scrolling overlay mode switching remains unchanged.
