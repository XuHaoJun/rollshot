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

**rollshot-capture**
- Modify: `crates/rollshot-capture/Cargo.toml` (macOS-only scap dependency)
- Modify: `crates/rollshot-capture/src/macos/mod.rs` (real backend)
- Create: `crates/rollshot-capture/tests/macos_sck_smoke.rs` (ignored real-capture smoke test)

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

## Task 2: Implement the macOS Scap Backend

**Files:**
- Modify: `crates/rollshot-capture/src/macos/mod.rs`

- [ ] **Step 1: Replace the macOS stub with the real backend and tests**

Replace the full contents of `crates/rollshot-capture/src/macos/mod.rs` with:

```rust
#![cfg(target_os = "macos")]

use anyhow::anyhow;
use image::RgbaImage;

use crate::backend::{CaptureBackend, FrameStream};
use crate::error::CaptureError;
use crate::types::{
    CaptureOptions, CaptureProbe, CapturedFrame, FrameMetadata, PixelFormat, Region, RegionMode,
    Size,
};

const BACKEND_NAME: &str = "macos-sck";
const SCAP_VERSION: &str = "0.1.0-beta.1";
const EMPTY_FRAME_LIMIT: u8 = 10;

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

        if !scap::has_permission() && !scap::request_permission() {
            return Err(CaptureError::PermissionDenied {
                message: "Screen Recording permission is required for macOS capture".to_string(),
            });
        }

        let effective_region = manual_region(&options.region);
        let scap_options = options_to_scap_options(&options)?;
        let mut capturer = scap::capturer::Capturer::build(scap_options)
            .map_err(capturer_build_error_to_capture_error)?;
        capturer.start_capture();

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
        self.capturer.stop_capture();
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

            match frame {
                scap::frame::Frame::Audio(_) => continue,
                scap::frame::Frame::Video(scap::frame::VideoFrame::BGRA(frame)) => {
                    if frame.width <= 0 || frame.height <= 0 || frame.data.is_empty() {
                        empty_frames += 1;
                        if empty_frames >= EMPTY_FRAME_LIMIT {
                            return Err(CaptureError::Backend(anyhow!(
                                "macOS stream did not produce a usable video frame"
                            )));
                        }
                        continue;
                    }

                    return captured_frame_from_bgra(frame, self.effective_region);
                }
                scap::frame::Frame::Video(other) => {
                    return Err(CaptureError::Backend(anyhow!(
                        "unsupported scap video frame type: {other:?}"
                    )));
                }
            }
        }
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

fn captured_frame_from_bgra(
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

    let mut rgba = Vec::with_capacity(data.len());
    for pixel in data.chunks_exact(4) {
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }

    RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| CaptureError::Backend(anyhow!("failed to create RGBA image")))
}

fn options_to_scap_options(
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

fn region_to_scap_area(
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

fn manual_region(region: &RegionMode) -> Option<Region> {
    match region {
        RegionMode::Manual(region) => Some(*region),
        RegionMode::FullSource | RegionMode::PortalPicker => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{bgra_to_rgba_image, options_to_scap_options, region_to_scap_area, SCAP_VERSION};
    use crate::backend::CaptureBackend;
    use crate::error::CaptureError;
    use crate::macos::MacosScreenCaptureKitBackend;
    use crate::types::{CaptureOptions, Region, RegionMode};

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
    fn full_source_has_no_crop_area() {
        let area = region_to_scap_area(&RegionMode::FullSource).expect("full source");

        assert!(area.is_none());
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

- [ ] **Step 2: Run macOS unit tests on a macOS host**

Run on macOS:

```bash
rtk cargo test -p rollshot-capture macos:: --lib
```

Expected: macOS unit tests pass. On Linux, this module is cfg-gated and this command does not exercise macOS tests.

- [ ] **Step 3: Verify non-macOS behavior still compiles locally**

Run:

```bash
rtk cargo check -p rollshot-capture
```

Expected: PASS on the local host.

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-capture/src/macos/mod.rs
rtk git commit -m "feat(capture): implement macos scap backend"
```

---

## Task 3: Add Ignored macOS Real-Capture Smoke Test

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

## Task 4: Update README macOS Manual Testing

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

## Task 5: CLI and Backend Integration Checks

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

## Task 6: Final Verification

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

Expected: PASS on the local host. The ignored macOS real-capture smoke test does not run unless explicitly requested.

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
