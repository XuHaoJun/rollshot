# Rollshot macOS Scap Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the macOS capture stub with a real `scap = "0.1.0-beta.1"` backend that captures full-source or manual-region frames, converts BGRA to `RgbaImage`, and documents manual macOS testing in `README.md`.

**Architecture:** `rollshot-capture` keeps all direct scap usage inside `crates/rollshot-capture/src/macos/mod.rs`, compiled only on macOS. The backend maps rollshot `CaptureOptions` to scap `Options`, starts a scap `Capturer`, wraps it in `MacosScapFrameStream`, converts `VideoFrame::BGRA` into `CapturedFrame`, and leaves Linux/fixture behavior untouched. README manual testing is updated as a separate task so backend implementation and operator validation stay aligned.

**Tech Stack:** Rust 2021 with workspace MSRV 1.85, `scap` 0.1.0-beta.1, `image` 0.25, `anyhow`, `thiserror`, existing `rollshot-capture` and `rollshot-cli` APIs.

**Spec:** `docs/superpowers/specs/2026-05-20-rollshot-macos-scap-design.md`

---

## File Map

**Workspace root**
- Modify: `Cargo.toml` (MSRV and workspace dependency)
- Modify: `Cargo.lock` (resolved scap dependency)
- Modify: `README.md` (active macOS manual testing section)
- Modify: `.github/workflows/real-capture.yml` (self-hosted macOS smoke test)

**rollshot-capture**
- Modify: `crates/rollshot-capture/Cargo.toml` (macOS-only scap dependency)
- Modify: `crates/rollshot-capture/src/backend.rs` (remove unused `FrameStream: Send` requirement)
- Modify: `crates/rollshot-capture/src/macos/mod.rs` (backend lifecycle)
- Create: `crates/rollshot-capture/src/macos/pixel.rs` (BGRA to RGBA and frame metadata)
- Create: `crates/rollshot-capture/src/macos/options.rs` (rollshot to scap option mapping)
- Create: `crates/rollshot-capture/tests/macos_sck_smoke.rs` (ignored real-capture smoke test)

**rollshot-cli**
- Modify: `crates/rollshot-cli/tests/capture_stubs.rs` (macOS stub contract replaced by non-interactive real-backend contract)

---

## Task 1: Add Scap Dependency and Raise MSRV

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/rollshot-capture/Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **Step 1: Update the root workspace manifest**

Edit `Cargo.toml` so `[workspace.package]` uses Rust `1.85` and `[workspace.dependencies]` includes scap:

```toml
[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
repository = "https://github.com/xuhaojun/rollshot"
rust-version = "1.85"
```

```toml
[workspace.dependencies]
anyhow = "1"
thiserror = "1"
clap = { version = "4", features = ["derive"] }
image = { version = "0.25", default-features = false, features = ["png", "jpeg"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
scap = "0.1.0-beta.1"
```

- [ ] **Step 2: Add the macOS-only capture dependency**

Edit `crates/rollshot-capture/Cargo.toml` so it reads:

```toml
[package]
name = "rollshot-capture"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
anyhow = { workspace = true }
image = { workspace = true }
thiserror = { workspace = true }

[target.'cfg(target_os = "macos")'.dependencies]
scap = { workspace = true }

[lints]
workspace = true
```

- [ ] **Step 3: Resolve dependencies**

Run:

```bash
rtk cargo fetch
```

Expected: `Cargo.lock` records `scap v0.1.0-beta.1` and its transitive dependencies.

- [ ] **Step 4: Verify Linux/non-macOS still builds without compiling scap**

Run:

```bash
rtk cargo check -p rollshot-capture
```

Expected: PASS on the local host. On Linux, scap is not compiled because it is target-gated.

- [ ] **Step 5: Commit**

```bash
rtk git add Cargo.toml Cargo.lock crates/rollshot-capture/Cargo.toml
rtk git commit -m "chore: add macos-only scap dependency"
```

---

## Task 2: Remove the FrameStream Send Bound

**Files:**
- Modify: `crates/rollshot-capture/src/backend.rs`

- [ ] **Step 1: Update the trait contract**

In `crates/rollshot-capture/src/backend.rs`, replace:

```rust
pub trait FrameStream: Send {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError>;
}
```

with:

```rust
pub trait FrameStream {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError>;
}
```

This matches the current CLI, which consumes the stream synchronously on the same thread that creates it, and avoids requiring native macOS ScreenCaptureKit/scap objects to be `Send`.

- [ ] **Step 2: Verify capture crate tests still pass**

Run:

```bash
rtk cargo test -p rollshot-capture
```

Expected: existing capture crate tests pass.

- [ ] **Step 3: Verify CLI fixture path still passes**

Run:

```bash
rtk cargo test -p rollshot-cli --test capture_fixture
```

Expected: CLI fixture capture still passes with the relaxed stream trait.

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-capture/src/backend.rs
rtk git commit -m "refactor(capture): relax frame stream thread bound"
```

---

## Task 3: Implement the macOS Scap Backend

**Files:**
- Modify: `crates/rollshot-capture/src/macos/mod.rs`
- Create: `crates/rollshot-capture/src/macos/pixel.rs`
- Create: `crates/rollshot-capture/src/macos/options.rs`

- [ ] **Step 1: Write failing pure-helper tests first**

Replace `crates/rollshot-capture/src/macos/mod.rs` with a temporary stub that wires the helper modules:

```rust
#![cfg(target_os = "macos")]

mod options;
mod pixel;

use crate::backend::{CaptureBackend, FrameStream};
use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe};

pub(super) const BACKEND_NAME: &str = "macos-sck";

pub struct MacosScreenCaptureKitBackend;

impl MacosScreenCaptureKitBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MacosScreenCaptureKitBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptureBackend for MacosScreenCaptureKitBackend {
    fn name(&self) -> &'static str {
        BACKEND_NAME
    }

    fn probe(&self) -> CaptureProbe {
        CaptureProbe {
            backend: BACKEND_NAME,
            available: true,
            message: "temporary macOS helper-test stub".to_string(),
            details: vec![("os".to_string(), "macos".to_string())],
        }
    }

    fn start(&mut self, _options: CaptureOptions) -> Result<Box<dyn FrameStream>, CaptureError> {
        Err(CaptureError::NotImplemented {
            backend: BACKEND_NAME,
        })
    }
}
```

Create `crates/rollshot-capture/src/macos/pixel.rs` with tests first:

```rust
use image::RgbaImage;

use crate::error::CaptureError;
use crate::types::{CapturedFrame, FrameMetadata, PixelFormat, Region, Size};

use super::BACKEND_NAME;

fn bgra_to_rgba_image(_width: u32, _height: u32, _data: &[u8]) -> Result<RgbaImage, CaptureError> {
    unimplemented!("implemented in Step 3")
}

pub(super) fn captured_frame_from_bgra(
    _frame: scap::frame::BGRAFrame,
    _effective_region: Option<Region>,
) -> Result<CapturedFrame, CaptureError> {
    unimplemented!("implemented in Step 3")
}

#[cfg(test)]
mod tests {
    use super::bgra_to_rgba_image;

    #[test]
    fn bgra_to_rgba_swaps_blue_and_red_channels() {
        let image = bgra_to_rgba_image(
            2,
            1,
            &[
                10, 20, 30, 255, //
                1, 2, 3, 4,
            ],
        )
        .expect("valid image");

        assert_eq!(image.as_raw(), &[30, 20, 10, 255, 3, 2, 1, 4]);
    }

    #[test]
    fn bgra_to_rgba_rejects_invalid_length() {
        let err = bgra_to_rgba_image(2, 1, &[1, 2, 3, 4]).expect_err("invalid length");

        assert!(err.to_string().contains("length mismatch"));
    }

    #[test]
    fn bgra_to_rgba_rejects_empty_dimensions() {
        let err = bgra_to_rgba_image(0, 1, &[]).expect_err("empty width");

        assert!(err.to_string().contains("empty dimensions"));
    }
}
```

Create `crates/rollshot-capture/src/macos/options.rs` with tests first:

```rust
use crate::error::CaptureError;
use crate::types::{CaptureOptions, Region, RegionMode};

pub(super) const NO_PERMISSION_PROMPT_ENV: &str = "ROLLSHOT_NO_PERMISSION_PROMPT";

pub(super) fn options_to_scap_options(
    _options: &CaptureOptions,
) -> Result<scap::capturer::Options, CaptureError> {
    unimplemented!("implemented in Step 3")
}

pub(super) fn region_to_scap_area(
    _region: &RegionMode,
) -> Result<Option<scap::capturer::Area>, CaptureError> {
    unimplemented!("implemented in Step 3")
}

pub(super) fn manual_region(region: &RegionMode) -> Option<Region> {
    match region {
        RegionMode::Manual(region) => Some(*region),
        RegionMode::FullSource | RegionMode::PortalPicker => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{options_to_scap_options, region_to_scap_area, NO_PERMISSION_PROMPT_ENV};
    use crate::error::CaptureError;
    use crate::types::{CaptureOptions, Region, RegionMode};

    #[test]
    fn no_permission_prompt_env_name_is_stable() {
        assert_eq!(NO_PERMISSION_PROMPT_ENV, "ROLLSHOT_NO_PERMISSION_PROMPT");
    }

    #[test]
    fn manual_region_maps_to_scap_area() {
        let area = region_to_scap_area(&RegionMode::Manual(Region {
            x: 10,
            y: 20,
            width: 300,
            height: 200,
        }))
        .expect("valid region")
        .expect("area");

        assert_eq!(area.origin.x, 10.0);
        assert_eq!(area.origin.y, 20.0);
        assert_eq!(area.size.width, 300.0);
        assert_eq!(area.size.height, 200.0);
    }

    #[test]
    fn negative_manual_region_origin_is_rejected() {
        let err = region_to_scap_area(&RegionMode::Manual(Region {
            x: -1,
            y: 0,
            width: 300,
            height: 200,
        }))
        .expect_err("negative origin rejected");

        assert!(matches!(err, CaptureError::InvalidConfig { .. }));
    }

    #[test]
    fn portal_picker_is_rejected_on_macos() {
        let err = region_to_scap_area(&RegionMode::PortalPicker).expect_err("portal rejected");

        assert!(matches!(err, CaptureError::InvalidConfig { .. }));
    }

    #[test]
    fn capture_options_map_to_scap_options() {
        let options = CaptureOptions {
            region: RegionMode::Manual(Region {
                x: 4,
                y: 5,
                width: 640,
                height: 480,
            }),
            fps: 12,
            show_cursor: true,
            prefer_portal_region: true,
        };

        let scap_options = options_to_scap_options(&options).expect("valid options");

        assert_eq!(scap_options.fps, 12);
        assert!(scap_options.show_cursor);
        assert!(!scap_options.show_highlight);
        assert!(!scap_options.captures_audio);
        assert!(!scap_options.exclude_current_process_audio);
        assert!(matches!(
            scap_options.output_type,
            scap::frame::FrameType::BGRAFrame
        ));
        assert!(matches!(
            scap_options.output_resolution,
            scap::capturer::Resolution::Captured
        ));
        assert!(scap_options.crop_area.is_some());
    }
}
```

- [ ] **Step 2: Run helper tests to verify RED**

Run on macOS:

```bash
rtk cargo test -p rollshot-capture macos:: --lib
```

Expected: FAIL because the helper functions still contain `unimplemented!()`.

- [ ] **Step 3: Implement `pixel.rs`**

Replace `crates/rollshot-capture/src/macos/pixel.rs` with:

```rust
use anyhow::anyhow;
use image::RgbaImage;

use crate::error::CaptureError;
use crate::types::{CapturedFrame, FrameMetadata, PixelFormat, Region, Size};

use super::BACKEND_NAME;

pub(super) fn captured_frame_from_bgra(
    frame: scap::frame::BGRAFrame,
    effective_region: Option<Region>,
) -> Result<CapturedFrame, CaptureError> {
    let width = u32::try_from(frame.width).map_err(|_| {
        CaptureError::Backend(anyhow!("invalid negative BGRA frame width: {}", frame.width))
    })?;
    let height = u32::try_from(frame.height).map_err(|_| {
        CaptureError::Backend(anyhow!("invalid negative BGRA frame height: {}", frame.height))
    })?;
    let image = bgra_to_rgba_image(width, height, &frame.data)?;

    Ok(CapturedFrame {
        image,
        timestamp: frame.display_time,
        metadata: FrameMetadata {
            source_size: Some(Size { width, height }),
            effective_region,
            pixel_format: Some(PixelFormat::Bgra),
            stride: Some(width * 4),
            backend: BACKEND_NAME,
        },
    })
}

fn bgra_to_rgba_image(width: u32, height: u32, data: &[u8]) -> Result<RgbaImage, CaptureError> {
    if width == 0 || height == 0 {
        return Err(CaptureError::Backend(anyhow!(
            "BGRA frame has empty dimensions: {width}x{height}"
        )));
    }

    let expected_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| {
            CaptureError::Backend(anyhow!("BGRA frame dimensions overflow: {width}x{height}"))
        })?;

    if data.len() != expected_len {
        return Err(CaptureError::Backend(anyhow!(
            "BGRA frame length mismatch: got {}, expected {} for {}x{}",
            data.len(),
            expected_len,
            width,
            height
        )));
    }

    let mut rgba = vec![0; data.len()];
    for (src, dst) in data.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }

    RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| CaptureError::Backend(anyhow!("failed to create RGBA image")))
}

#[cfg(test)]
mod tests {
    use super::bgra_to_rgba_image;

    #[test]
    fn bgra_to_rgba_swaps_blue_and_red_channels() {
        let image = bgra_to_rgba_image(2, 1, &[10, 20, 30, 255, 1, 2, 3, 4])
            .expect("valid image");
        assert_eq!(image.as_raw(), &[30, 20, 10, 255, 3, 2, 1, 4]);
    }

    #[test]
    fn bgra_to_rgba_rejects_invalid_length() {
        let err = bgra_to_rgba_image(2, 1, &[1, 2, 3, 4]).expect_err("invalid length");
        assert!(err.to_string().contains("length mismatch"));
    }

    #[test]
    fn bgra_to_rgba_rejects_empty_dimensions() {
        let err = bgra_to_rgba_image(0, 1, &[]).expect_err("empty width");
        assert!(err.to_string().contains("empty dimensions"));
    }
}
```

- [ ] **Step 4: Implement `options.rs`**

Replace `crates/rollshot-capture/src/macos/options.rs` with:

```rust
use crate::error::CaptureError;
use crate::types::{CaptureOptions, Region, RegionMode};

pub(super) const NO_PERMISSION_PROMPT_ENV: &str = "ROLLSHOT_NO_PERMISSION_PROMPT";

pub(super) fn options_to_scap_options(
    options: &CaptureOptions,
) -> Result<scap::capturer::Options, CaptureError> {
    let crop_area = region_to_scap_area(&options.region)?;

    Ok(scap::capturer::Options {
        fps: options.fps,
        show_cursor: options.show_cursor,
        show_highlight: false,
        target: None,
        crop_area,
        output_type: scap::frame::FrameType::BGRAFrame,
        output_resolution: scap::capturer::Resolution::Captured,
        excluded_targets: None,
        captures_audio: false,
        exclude_current_process_audio: false,
    })
}

pub(super) fn region_to_scap_area(
    region: &RegionMode,
) -> Result<Option<scap::capturer::Area>, CaptureError> {
    match region {
        RegionMode::FullSource => Ok(None),
        RegionMode::PortalPicker => Err(CaptureError::InvalidConfig {
            message: "--region portal is only supported with --backend linux-portal".to_string(),
        }),
        RegionMode::Manual(region) => {
            if region.x < 0 || region.y < 0 {
                return Err(CaptureError::InvalidConfig {
                    message: "macOS manual region origin must be non-negative".to_string(),
                });
            }

            Ok(Some(scap::capturer::Area {
                origin: scap::capturer::Point {
                    x: region.x as f64,
                    y: region.y as f64,
                },
                size: scap::capturer::Size {
                    width: region.width as f64,
                    height: region.height as f64,
                },
            }))
        }
    }
}

pub(super) fn manual_region(region: &RegionMode) -> Option<Region> {
    match region {
        RegionMode::Manual(region) => Some(*region),
        RegionMode::FullSource | RegionMode::PortalPicker => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{options_to_scap_options, region_to_scap_area, NO_PERMISSION_PROMPT_ENV};
    use crate::error::CaptureError;
    use crate::types::{CaptureOptions, Region, RegionMode};

    #[test]
    fn no_permission_prompt_env_name_is_stable() {
        assert_eq!(NO_PERMISSION_PROMPT_ENV, "ROLLSHOT_NO_PERMISSION_PROMPT");
    }

    #[test]
    fn manual_region_maps_to_scap_area() {
        let area = region_to_scap_area(&RegionMode::Manual(Region {
            x: 10,
            y: 20,
            width: 300,
            height: 200,
        }))
        .expect("valid region")
        .expect("area");
        assert_eq!(area.origin.x, 10.0);
        assert_eq!(area.origin.y, 20.0);
        assert_eq!(area.size.width, 300.0);
        assert_eq!(area.size.height, 200.0);
    }

    #[test]
    fn negative_manual_region_origin_is_rejected() {
        let err = region_to_scap_area(&RegionMode::Manual(Region {
            x: -1,
            y: 0,
            width: 300,
            height: 200,
        }))
        .expect_err("negative origin rejected");
        assert!(matches!(err, CaptureError::InvalidConfig { .. }));
    }

    #[test]
    fn portal_picker_is_rejected_on_macos() {
        let err = region_to_scap_area(&RegionMode::PortalPicker).expect_err("portal rejected");
        assert!(matches!(err, CaptureError::InvalidConfig { .. }));
    }

    #[test]
    fn capture_options_map_to_scap_options() {
        let options = CaptureOptions {
            region: RegionMode::Manual(Region {
                x: 4,
                y: 5,
                width: 640,
                height: 480,
            }),
            fps: 12,
            show_cursor: true,
            prefer_portal_region: true,
        };
        let scap_options = options_to_scap_options(&options).expect("valid options");
        assert_eq!(scap_options.fps, 12);
        assert!(scap_options.show_cursor);
        assert!(!scap_options.show_highlight);
        assert!(!scap_options.captures_audio);
        assert!(!scap_options.exclude_current_process_audio);
        assert!(matches!(
            scap_options.output_type,
            scap::frame::FrameType::BGRAFrame
        ));
        assert!(matches!(
            scap_options.output_resolution,
            scap::capturer::Resolution::Captured
        ));
        assert!(scap_options.crop_area.is_some());
    }
}
```

- [ ] **Step 5: Replace `mod.rs` with the lifecycle adapter**

Replace `crates/rollshot-capture/src/macos/mod.rs` with:

```rust
#![cfg(target_os = "macos")]

mod options;
mod pixel;

use anyhow::anyhow;
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::backend::{CaptureBackend, FrameStream};
use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe, CapturedFrame, Region};

use options::{manual_region, options_to_scap_options, NO_PERMISSION_PROMPT_ENV};
use pixel::captured_frame_from_bgra;

pub(super) const BACKEND_NAME: &str = "macos-sck";
const SCAP_VERSION: &str = "0.1.0-beta.1";
const EMPTY_FRAME_LIMIT: u8 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameProcessOutcome {
    Audio,
    Empty,
}

pub struct MacosScreenCaptureKitBackend;

impl MacosScreenCaptureKitBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MacosScreenCaptureKitBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptureBackend for MacosScreenCaptureKitBackend {
    fn name(&self) -> &'static str {
        BACKEND_NAME
    }

    fn probe(&self) -> CaptureProbe {
        let supported = scap::is_supported();
        let permitted = scap::has_permission();
        let available = supported && permitted;

        let message = match (supported, permitted) {
            (false, _) => "scap does not support this macOS host".to_string(),
            (true, false) => "Screen Recording permission is missing".to_string(),
            (true, true) => "scap macOS capture is available".to_string(),
        };

        CaptureProbe {
            backend: BACKEND_NAME,
            available,
            message,
            details: vec![
                ("os".to_string(), "macos".to_string()),
                ("scap_version".to_string(), SCAP_VERSION.to_string()),
                ("scap_supported".to_string(), supported.to_string()),
                (
                    "screen_recording_permission".to_string(),
                    if permitted {
                        "granted".to_string()
                    } else {
                        "missing".to_string()
                    },
                ),
            ],
        }
    }

    fn start(&mut self, options: CaptureOptions) -> Result<Box<dyn FrameStream>, CaptureError> {
        if !scap::is_supported() {
            return Err(CaptureError::Unsupported {
                message: "scap macOS capture requires macOS 12.3 or newer".to_string(),
            });
        }

        if !scap::has_permission() {
            if std::env::var(NO_PERMISSION_PROMPT_ENV).ok().as_deref() == Some("1")
                || !scap::request_permission()
            {
                return Err(CaptureError::PermissionDenied {
                    message: "Screen Recording permission is required for macOS capture"
                        .to_string(),
                });
            }
        }

        let effective_region = manual_region(&options.region);
        let scap_options = options_to_scap_options(&options)?;
        let mut capturer = scap::capturer::Capturer::build(scap_options)
            .map_err(capturer_build_error_to_capture_error)?;
        catch_unwind(AssertUnwindSafe(|| capturer.start_capture())).map_err(|payload| {
            CaptureError::Backend(anyhow!(
                "scap failed to start macOS capture: {}",
                panic_payload_to_string(payload)
            ))
        })?;

        Ok(Box::new(MacosScapFrameStream {
            capturer,
            effective_region,
        }))
    }
}

pub struct MacosScapFrameStream {
    capturer: scap::capturer::Capturer,
    effective_region: Option<Region>,
}

impl Drop for MacosScapFrameStream {
    fn drop(&mut self) {
        let _ = catch_unwind(AssertUnwindSafe(|| self.capturer.stop_capture()));
    }
}

impl FrameStream for MacosScapFrameStream {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        let mut empty_frames = 0;

        loop {
            let frame = self
                .capturer
                .get_next_frame()
                .map_err(|_| CaptureError::EndOfStream)?;

            match process_scap_frame(frame, &mut empty_frames, self.effective_region)? {
                Ok(captured) => return Ok(captured),
                Err(FrameProcessOutcome::Audio | FrameProcessOutcome::Empty) => continue,
            }
        }
    }
}

fn process_scap_frame(
    frame: scap::frame::Frame,
    empty_frames: &mut u8,
    effective_region: Option<Region>,
) -> Result<Result<CapturedFrame, FrameProcessOutcome>, CaptureError> {
    match frame {
        scap::frame::Frame::Audio(_) => Ok(Err(FrameProcessOutcome::Audio)),
        scap::frame::Frame::Video(scap::frame::VideoFrame::BGRA(frame)) => {
            if frame.width <= 0 || frame.height <= 0 || frame.data.is_empty() {
                *empty_frames += 1;
                if *empty_frames >= EMPTY_FRAME_LIMIT {
                    return Err(CaptureError::Backend(anyhow!(
                        "macOS stream did not produce a usable video frame"
                    )));
                }
                return Ok(Err(FrameProcessOutcome::Empty));
            }

            captured_frame_from_bgra(frame, effective_region).map(Ok)
        }
        scap::frame::Frame::Video(other) => Err(CaptureError::Backend(anyhow!(
            "unsupported scap video frame type: {other:?}"
        ))),
    }
}

fn capturer_build_error_to_capture_error(err: scap::capturer::CapturerBuildError) -> CaptureError {
    match err {
        scap::capturer::CapturerBuildError::NotSupported => CaptureError::Unsupported {
            message: "scap macOS capture is not supported on this host".to_string(),
        },
        scap::capturer::CapturerBuildError::PermissionNotGranted => {
            CaptureError::PermissionDenied {
                message: "Screen Recording permission is required for macOS capture".to_string(),
            }
        }
    }
}

fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "non-string panic payload".to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        process_scap_frame, FrameProcessOutcome, MacosScreenCaptureKitBackend, EMPTY_FRAME_LIMIT,
        SCAP_VERSION,
    };
    use crate::backend::CaptureBackend;
    use crate::error::CaptureError;

    #[test]
    fn probe_reports_scap_details() {
        let probe = MacosScreenCaptureKitBackend::new().probe();

        assert_eq!(probe.backend, "macos-sck");
        assert!(probe.details.iter().any(|(k, v)| k == "os" && v == "macos"));
        assert!(probe
            .details
            .iter()
            .any(|(k, v)| k == "scap_version" && v == SCAP_VERSION));
        assert!(probe
            .details
            .iter()
            .any(|(k, _)| k == "screen_recording_permission"));
    }

    #[test]
    fn process_scap_frame_skips_audio() {
        let mut empty_frames = 0;
        let outcome = process_scap_frame(
            scap::frame::Frame::Audio(scap::frame::AudioFrame::new(
                scap::frame::AudioFormat::F32,
                2,
                false,
                Vec::new(),
                0,
                48_000,
                std::time::SystemTime::now(),
            )),
            &mut empty_frames,
            None,
        )
        .expect("audio frame handled");

        assert_eq!(outcome, Err(FrameProcessOutcome::Audio));
        assert_eq!(empty_frames, 0);
    }

    #[test]
    fn process_scap_frame_errors_after_empty_frame_limit() {
        let mut empty_frames = EMPTY_FRAME_LIMIT - 1;
        let frame = scap::frame::Frame::Video(scap::frame::VideoFrame::BGRA(
            scap::frame::BGRAFrame {
                display_time: std::time::SystemTime::now(),
                width: 0,
                height: 0,
                data: Vec::new(),
            },
        ));

        let err = process_scap_frame(frame, &mut empty_frames, None)
            .expect_err("empty frame limit reached");

        assert!(matches!(err, CaptureError::Backend(_)));
        assert_eq!(empty_frames, EMPTY_FRAME_LIMIT);
    }
}
```

- [ ] **Step 6: Run macOS unit tests on a macOS host**

Run on macOS:

```bash
rtk cargo test -p rollshot-capture macos:: --lib
```

Expected: macOS unit tests pass.

- [ ] **Step 7: Verify non-macOS behavior still compiles locally**

Run:

```bash
rtk cargo check -p rollshot-capture
```

Expected: PASS on the local host.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-capture/src/macos/mod.rs crates/rollshot-capture/src/macos/pixel.rs crates/rollshot-capture/src/macos/options.rs
rtk git commit -m "feat(capture): implement macos scap backend"
```

---

## Task 4: Update CLI Stub Tests for Real macOS Backend

**Files:**
- Modify: `crates/rollshot-cli/tests/common/mod.rs` (CLI subprocess timeout helper)
- Modify: `crates/rollshot-cli/tests/capture_stubs.rs`

- [ ] **Step 1: Add a CLI subprocess timeout helper**

In `crates/rollshot-cli/tests/common/mod.rs`, add a helper that runs a
`Command` with piped stdout/stderr and panics after 10 seconds with captured
output. Use this helper in the capture stub tests so a regression that reaches
blocking capture code fails fast instead of hanging CI.

- [ ] **Step 2: Replace the macOS NotImplemented test**

In `crates/rollshot-cli/tests/capture_stubs.rs`, replace the full macOS-only test:

```rust
#[test]
#[cfg(target_os = "macos")]
fn macos_sck_backend_exits_with_not_implemented_code() {
    let tempdir = temp_dir("macos-sck");
    let out = tempdir.join("out.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "macos-sck"])
        .args(["--output"])
        .arg(&out)
        .output()
        .expect("run rollshot capture");

    assert_eq!(
        output.status.code(),
        Some(2),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not implemented"), "stderr = {stderr}");
    assert!(stderr.contains("macos-sck"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}
```

with this hosted-CI-safe contract. It must not start ScreenCaptureKit in the
default `cargo test --workspace` path:

```rust
#[test]
#[cfg(target_os = "macos")]
fn macos_sck_backend_rejects_portal_region_without_starting_capture() {
    let tempdir = temp_dir("macos-sck");
    let out = tempdir.join("out.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "macos-sck"])
        .args(["--region", "portal"])
        .args(["--output"])
        .arg(&out)
        .output()
        .expect("run rollshot capture");

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected invalid config before macOS capture starts; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--region portal"), "stderr = {stderr}");
    assert!(stderr.contains("linux-portal"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}
```

- [ ] **Step 3: Update the auto-backend expectation**

In `backend_auto_exits_with_host_appropriate_code`, first replace the command construction:

```rust
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "auto"])
        .args(["--output"])
        .arg(&out)
        .output()
        .expect("run rollshot capture");
```

with:

```rust
    let mut command = Command::new(env!("CARGO_BIN_EXE_rollshot"));
    command
        .arg("capture")
        .args(["--backend", "auto"])
        .args(["--output"])
        .arg(&out);
    if cfg!(target_os = "macos") {
        command.args(["--region", "portal"]);
    } else {
        command.args(["--max-frames", "1"]);
    }
    let output = command.output().expect("run rollshot capture");
```

Then replace:

```rust
    let is_stub_backend = cfg!(target_os = "macos")
        || (cfg!(target_os = "linux")
            && std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland"));
    let expected_code = if is_stub_backend { 2 } else { 4 };
```

with:

```rust
    let expected_code = if cfg!(target_os = "macos") {
        // Hosted macOS CI must not start ScreenCaptureKit. `auto` still
        // resolves to macos-sck, and `portal` fails during argument validation
        // before backend startup.
        1
    } else if cfg!(target_os = "linux")
        && std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland")
    {
        2
    } else {
        4
    };
```

- [ ] **Step 4: Run the CLI stub tests on macOS**

Before running, ensure the macOS test command does not use the real-capture
path. The default macOS test must reject `--region portal` before
ScreenCaptureKit startup:

```rust
        .args(["--region", "portal"])
```

Run on macOS:

```bash
rtk cargo test -p rollshot-cli --test capture_stubs
```

Expected: tests pass without starting real macOS capture. Real capture remains
covered by the ignored `macos_sck_smoke` test.

- [ ] **Step 5: Run the CLI stub tests on Linux**

Run on Linux:

```bash
rtk cargo test -p rollshot-cli --test capture_stubs
```

Expected: Linux unsupported/stub behavior still passes.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-cli/tests/common/mod.rs crates/rollshot-cli/tests/capture_stubs.rs
rtk git commit -m "test(cli): update macos backend contract"
```

---

## Task 5: Add Ignored macOS Real-Capture Smoke Test

**Files:**
- Create: `crates/rollshot-capture/tests/macos_sck_smoke.rs`

- [ ] **Step 1: Create the ignored smoke test**

Create `crates/rollshot-capture/tests/macos_sck_smoke.rs`:

```rust
#![cfg(target_os = "macos")]

use std::path::Path;

use rollshot_capture::{
    CaptureBackend, CaptureOptions, MacosScreenCaptureKitBackend, PixelFormat, Region, RegionMode,
};

#[test]
#[ignore = "requires macOS Screen Recording permission and an interactive desktop session"]
fn macos_sck_receives_frames() {
    if std::env::var("ROLLSHOT_REAL_CAPTURE").ok().as_deref() != Some("1") {
        eprintln!("set ROLLSHOT_REAL_CAPTURE=1 to run the real macOS capture smoke test");
        return;
    }

    let mut backend = MacosScreenCaptureKitBackend::new();
    let options = CaptureOptions {
        region: RegionMode::Manual(Region {
            x: 0,
            y: 0,
            width: 320,
            height: 240,
        }),
        fps: 5,
        show_cursor: false,
        prefer_portal_region: false,
    };

    let mut stream = backend.start(options).expect("start macOS capture");
    let mut first_frame = None;

    for _ in 0..3 {
        let frame = stream.next_frame().expect("next macOS capture frame");
        assert!(frame.image.width() > 0);
        assert!(frame.image.height() > 0);
        assert_eq!(frame.metadata.backend, "macos-sck");
        assert_eq!(frame.metadata.pixel_format, Some(PixelFormat::Bgra));

        if first_frame.is_none() {
            first_frame = Some(frame);
        }
    }

    let frame = first_frame.expect("first frame captured");
    let artifact_dir = Path::new("target/test-artifacts");
    std::fs::create_dir_all(artifact_dir).expect("create artifact dir");
    frame
        .image
        .save(artifact_dir.join("macos_sck_first_frame.png"))
        .expect("save first frame artifact");
}
```

- [ ] **Step 2: Verify the ignored test is discoverable on macOS**

Run on macOS:

```bash
rtk cargo test -p rollshot-capture --test macos_sck_smoke -- --ignored --list
```

Expected: output lists `macos_sck_receives_frames`.

- [ ] **Step 3: Run the real smoke test manually on macOS**

Run on a macOS machine with Screen Recording permission:

```bash
rtk env ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test macos_sck_smoke -- --ignored --nocapture
```

Expected: PASS and `target/test-artifacts/macos_sck_first_frame.png` exists with non-zero dimensions.

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-capture/tests/macos_sck_smoke.rs
rtk git commit -m "test(capture): add macos scap smoke test"
```

---

## Task 6: Wire the macOS Real-Capture Workflow

**Files:**
- Modify: `.github/workflows/real-capture.yml`

- [ ] **Step 1: Replace the macOS placeholder command**

In `.github/workflows/real-capture.yml`, replace the `macos-screencapturekit` job's final step:

```yaml
      - name: Explain current bootstrap status
        run: |
          echo "Real macOS capture tests are added in the macOS backend phase."
          echo "This workflow reserves the self-hosted ScreenCaptureKit runner path."
```

with:

```yaml
      - name: Run macOS ScreenCaptureKit smoke test
        env:
          ROLLSHOT_REAL_CAPTURE: "1"
        run: cargo test -p rollshot-capture --test macos_sck_smoke -- --ignored --nocapture
```

- [ ] **Step 2: Verify the workflow text**

Run:

```bash
rtk rg -n "Run macOS ScreenCaptureKit smoke test|ROLLSHOT_REAL_CAPTURE|macos_sck_smoke" .github/workflows/real-capture.yml
```

Expected: the macOS self-hosted job contains the real smoke-test command.

- [ ] **Step 3: Commit**

```bash
rtk git add .github/workflows/real-capture.yml
rtk git commit -m "ci: run macos real capture smoke test"
```

---

## Task 7: Update README macOS Manual Testing

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update the project status paragraph**

Replace the first paragraph under `# rollshot` with:

```markdown
`rollshot` is a Rust rewrite of the long screenshot workflow described in
`rollshot_mvp_design.md`. The project has a platform-independent stitching
core, fixture-backed capture tests, and a macOS ScreenCaptureKit backend built
through `scap`. The KDE Wayland backend is still planned for a later phase.
```

- [ ] **Step 2: Update the local development Rust version note**

Replace:

```markdown
Install a stable Rust toolchain with `rustup`, then run:
```

with:

```markdown
Install Rust 1.85 or newer with `rustup`, then run:
```

- [ ] **Step 3: Replace the macOS manual testing section**

Replace the full `## Manual Testing: Future macOS ScreenCaptureKit Capture` section with:

```markdown
## Manual Testing: macOS ScreenCaptureKit Capture

Use this checklist after changing the macOS `macos-sck` backend or before
validating a release on macOS:

- [ ] Test machine is running macOS 12.3 or newer.
- [ ] Rust 1.85 or newer is installed.
- [ ] The terminal or test binary has Screen Recording permission:
  `System Settings -> Privacy & Security -> Screen & System Audio Recording`.
- [ ] Main display is visible and unlocked.
- [ ] `cargo run -p rollshot-cli -- probe --json` reports `macos-sck`.
- [ ] `cargo run -p rollshot-cli -- capture --backend macos-sck --region full --max-frames 3 --output target/test-artifacts/macos_full.png` writes a PNG.
- [ ] `cargo run -p rollshot-cli -- capture --backend macos-sck --region "0,0 320x240" --max-frames 3 --output target/test-artifacts/macos_region.png` writes a PNG.
- [ ] `cargo run -p rollshot-cli -- capture --backend macos-sck --region "0,0 320x240" --max-frames 3 --dump-frames target/test-artifacts/macos_frames --output target/test-artifacts/macos_region_stitched.png` writes frame dumps.
- [ ] `ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test macos_sck_smoke -- --ignored --nocapture` passes.
- [ ] `target/test-artifacts/macos_sck_first_frame.png` exists and is visually plausible.

If permission was just granted, restart the terminal before rerunning the
commands. If `probe` reports missing Screen Recording permission, run a capture
command once to trigger the permission prompt, grant access, restart the
terminal, and rerun `probe`.
```

- [ ] **Step 4: Update the self-hosted workflow wording**

Replace:

```markdown
only explain that real backend smoke tests are added in later backend phases.
```

with:

```markdown
run the ignored real-capture smoke tests on machines with the required desktop
permissions.
```

- [ ] **Step 5: Verify README commands are syntactically documented**

Run:

```bash
rtk rg -n "Manual Testing: macOS|macos-sck|ROLLSHOT_REAL_CAPTURE|Screen & System Audio Recording" README.md
```

Expected: the active macOS section and all manual commands are present.

- [ ] **Step 6: Commit**

```bash
rtk git add README.md
rtk git commit -m "docs: document macos manual capture testing"
```

---

## Task 8: CLI and Backend Integration Checks

**Files:**
- No source changes expected.

- [ ] **Step 1: Verify existing non-macOS unsupported behavior still passes**

Run on Linux:

```bash
rtk cargo test -p rollshot-cli --test capture_stubs
```

Expected: existing `macos-sck` unsupported tests still pass on Linux.

- [ ] **Step 2: Verify probe still works on the local host**

Run:

```bash
rtk cargo run -p rollshot-cli -- probe
```

Expected: command exits 0 and prints the local default backend plus known backend probes.

- [ ] **Step 3: Verify fixture capture still works**

Run:

```bash
rtk cargo test -p rollshot-cli --test capture_fixture
```

Expected: fixture capture tests pass. This confirms the macOS changes did not disturb capture trait consumers.

- [ ] **Step 4: Handle compatibility failures**

If any compatibility check fails, stop this task and fix the specific failing file in the smallest possible patch. Commit that patch in the task that owns the changed file, then rerun this task from Step 1.

---

## Task 9: Final Verification

**Files:**
- No source changes expected unless verification exposes a real issue.

- [ ] **Step 1: Format check**

Run:

```bash
rtk cargo fmt --all -- --check
```

Expected: PASS.

- [ ] **Step 2: Clippy**

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS on the local host.

- [ ] **Step 3: Workspace tests**

Run:

```bash
rtk cargo test --workspace
```

Expected: PASS on the local host. Hosted macOS workspace tests do not start
ScreenCaptureKit, and the ignored macOS real-capture smoke test does not run
unless explicitly requested.

- [ ] **Step 4: macOS backend verification on macOS**

Run on macOS:

```bash
rtk cargo test -p rollshot-capture macos:: --lib
rtk cargo test -p rollshot-capture --test macos_sck_smoke -- --ignored --list
rtk env ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test macos_sck_smoke -- --ignored --nocapture
```

Expected: unit tests pass, smoke test is discoverable, real smoke test captures at least three frames, and `target/test-artifacts/macos_sck_first_frame.png` exists.

- [ ] **Step 5: Final status**

Run:

```bash
rtk git status --short
```

Expected: clean worktree after all commits.

---

## Self-Review

- Spec coverage: covers crates.io scap dependency, no local dependency, MSRV 1.85, real probe/start, full/manual regions, portal rejection, BGRA conversion, metadata, ignored smoke test, and README manual testing.
- Placeholder scan: no placeholder tasks; every code-changing step includes exact target content or exact replacement text.
- Type consistency: plan uses existing `CaptureBackend`, `FrameStream`, `CaptureOptions`, `RegionMode`, `CapturedFrame`, `FrameMetadata`, `PixelFormat`, `CaptureProbe`, and `CaptureError` names from the capture skeleton.
