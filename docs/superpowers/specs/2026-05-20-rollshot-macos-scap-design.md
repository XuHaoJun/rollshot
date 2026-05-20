# Rollshot macOS Scap Backend Design

Date: 2026-05-20

## Scope

This phase replaces the macOS `MacosScreenCaptureKitBackend` stub with a real
backend implemented as a thin adapter over the crates.io `scap` package,
version `0.1.0-beta.1`.

`learn-projects/scap` remains a reference checkout only. It must not be added
as a path dependency, vendored crate, or workspace member in this phase. A fork
is only considered if the crates.io package blocks the MVP through a concrete
bug or missing API.

The backend captures the main display or a manual region, converts scap's BGRA
video frames into `image::RgbaImage`, and returns frames through the existing
`CaptureBackend` / `FrameStream` traits created by the capture skeleton phase.

## Goals

- Add `scap = "0.1.0-beta.1"` as a macOS-only dependency of
  `rollshot-capture`.
- Raise the workspace Rust version from `1.80` to `1.85`, matching scap's
  published MSRV.
- Implement `MacosScreenCaptureKitBackend::probe()` with real scap support and
  Screen Recording permission checks.
- Implement `MacosScreenCaptureKitBackend::start()` by building a scap
  `Capturer`.
- Support `RegionMode::FullSource` and `RegionMode::Manual(Region)`.
- Reject `RegionMode::PortalPicker` on macOS with `CaptureError::InvalidConfig`.
- Request Screen Recording permission once from `start()` when permission is
  missing, then return `CaptureError::PermissionDenied` if the user denies it.
- Convert scap `VideoFrame::BGRA` frames into `RgbaImage`.
- Return `CapturedFrame` metadata with backend, source size, effective region,
  pixel format, and stride.
- Add pure unit tests for BGRA-to-RGBA conversion and region-to-scap option
  mapping.
- Add ignored macOS real-capture smoke tests for self-hosted/manual validation.

## Non-Goals

- No local `learn-projects/scap` dependency.
- No scap fork.
- No custom ScreenCaptureKit implementation.
- No macOS overlay region selector.
- No window picker.
- No multi-display selection UI.
- No audio capture.
- No Linux backend changes.
- No attempt to make real macOS capture pass on hosted CI without Screen
  Recording permission.

## Decision: crates.io Scap, Not Fork

Use crates.io `scap = "0.1.0-beta.1"` first.

The local reference code shows the needed API surface exists:

- `scap::is_supported()`
- `scap::has_permission()`
- `scap::request_permission()`
- `scap::capturer::Capturer`
- `scap::capturer::Options`
- `scap::capturer::Area`, `Point`, `Size`
- `scap::frame::FrameType::BGRAFrame`
- `scap::frame::Frame::Video`
- `scap::frame::VideoFrame::BGRA`

This is enough for the MVP path: capture display frames, crop to a manual
region, hide the cursor by default, and convert BGRA to `RgbaImage`.

A fork becomes justified only if one of these concrete blockers is discovered:

- crates.io scap cannot compile for macOS in the rollshot workspace after the
  MSRV is raised to 1.85.
- `crop_area` is ignored or uses coordinates that cannot be made correct for a
  simple manual region.
- `BGRAFrame` data is malformed, empty after successful capture, or lacks enough
  dimensions/timing information to create `CapturedFrame`.
- `Capturer::stop_capture()` or stream shutdown leaks or panics in normal CLI
  exit paths.

Until then, forking is extra maintenance with no proven benefit.

## Architecture

```text
rollshot-cli
  rollshot capture --backend macos-sck --region "X,Y WxH" --output out.png
    -> BackendKind::MacosScreenCaptureKit
    -> MacosScreenCaptureKitBackend::start(options)
    -> MacosScapFrameStream::next_frame()
    -> CapturedFrame { image: RgbaImage, metadata }
    -> Stitcher::push_frame(frame.image)
    -> output PNG

rollshot-capture
  macos/mod.rs
    MacosScreenCaptureKitBackend
    MacosScapFrameStream
    bgra_to_rgba_image()
    options_to_scap_options()
    region_to_scap_area()
```

The macOS module is still compiled only with `#[cfg(target_os = "macos")]`.
Non-macOS builds must not pull in scap or its macOS-only transitive
dependencies.

## Dependency and MSRV Changes

Root workspace:

```toml
[workspace.package]
rust-version = "1.85"

[workspace.dependencies]
scap = "0.1.0-beta.1"
```

Capture crate:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
scap = { workspace = true }
```

The MSRV bump is intentional. Keeping rollshot at 1.80 while using the
published scap crate would make the dependency contract incoherent.

## Backend Behavior

### probe()

`probe()` returns:

- `available = false` when `scap::is_supported()` is false.
- `available = false` when Screen Recording permission is missing.
- `available = true` when scap supports the host and permission is already
  granted.

`details` includes:

```text
os = macos
scap_version = 0.1.0-beta.1
scap_supported = true|false
screen_recording_permission = granted|missing
```

`probe()` must not call `scap::request_permission()`. Probe is diagnostic, not
interactive.

### start()

`start()` performs:

```text
check scap::is_supported()
  false -> CaptureError::Unsupported
check scap::has_permission()
  false -> scap::request_permission()
    false -> CaptureError::PermissionDenied
map CaptureOptions to scap::capturer::Options
build scap::capturer::Capturer
start_capture()
return MacosScapFrameStream
```

The backend requests permission in `start()` because a user who runs
`rollshot capture` expects the command to attempt capture. If permission is
denied, the CLI already maps `CaptureError::PermissionDenied` to exit code 3.

## Region Mapping

Supported:

- `RegionMode::FullSource`
- `RegionMode::Manual(Region)`

Rejected:

- `RegionMode::PortalPicker`

For `FullSource`, scap `Options.crop_area` is `None`.

For `Manual(Region { x, y, width, height })`, scap `Options.crop_area` is:

```rust
scap::capturer::Area {
    origin: scap::capturer::Point {
        x: region.x as f64,
        y: region.y as f64,
    },
    size: scap::capturer::Size {
        width: region.width as f64,
        height: region.height as f64,
    },
}
```

Negative origins are rejected with `CaptureError::InvalidConfig` in this phase.
The CLI already rejects zero width and height when parsing manual regions.

The MVP design notes Retina scaling. This phase relies on scap's existing
ScreenCaptureKit configuration to translate crop area into the physical output
frame size. The returned `RgbaImage` dimensions are treated as authoritative
physical pixels for stitching.

## Scap Options

The backend uses:

```rust
scap::capturer::Options {
    fps: options.fps,
    show_cursor: options.show_cursor,
    show_highlight: false,
    target: None,
    excluded_targets: None,
    output_type: scap::frame::FrameType::BGRAFrame,
    output_resolution: scap::capturer::Resolution::Captured,
    crop_area,
    captures_audio: false,
    exclude_current_process_audio: false,
}
```

`target: None` captures the main display, matching the MVP's first macOS
backend scope.

## Frame Stream

`MacosScapFrameStream` owns the scap `Capturer` and stops capture in `Drop`.

`next_frame()` loops until it receives a video frame:

```text
capturer.get_next_frame()
  Frame::Audio(_) -> continue
  Frame::Video(VideoFrame::BGRA(frame)) -> convert and return
  Frame::Video(other) -> CaptureError::Backend(unsupported frame type)
  Err(_) -> CaptureError::EndOfStream
```

Audio is disabled, but the loop ignores audio defensively because scap's sample
API can represent it.

Empty BGRA frames with width `0`, height `0`, or empty data are skipped up to
ten consecutive times inside `next_frame()`. After ten consecutive empty
frames, `next_frame()` returns `CaptureError::Backend` with a message that the
macOS stream did not produce a usable video frame. This keeps the CLI from
waiting forever before it has counted any captured frames.

Unit tests cover the converter rejecting empty inputs; the smoke test verifies
real frames arrive.

## BGRA to RGBA Conversion

Scap's macOS backend is requested to produce `BGRAFrame`.

Conversion:

```text
for each pixel [B, G, R, A]:
  output [R, G, B, A]
```

Validation:

- Width and height must be positive.
- Data length must equal `width * height * 4`.
- Invalid sizes return `CaptureError::Backend`.

Metadata:

```text
backend = "macos-sck"
pixel_format = Some(PixelFormat::Bgra)
stride = Some(width * 4)
source_size = Some(Size { width, height })
effective_region = manual region if provided, otherwise None
```

The `CapturedFrame.image` itself is always RGBA.

## CLI Behavior

The existing CLI shape is unchanged:

```bash
rollshot capture --backend macos-sck --region "100,200 900x700" --output out.png
rollshot capture --backend macos-sck --region full --output out.png
rollshot capture --backend auto --output out.png
rollshot probe --json
```

On macOS, `auto` still resolves to `macos-sck`.

On non-macOS hosts, `--backend macos-sck` remains unsupported and exits with
the existing unsupported exit code.

## Testing Strategy

Pure tests on any host:

- `default_backend_for("macos", _)` remains `MacosScreenCaptureKit`.
- Non-macOS `BackendKind::MacosScreenCaptureKit.create()` still returns
  `Unsupported`.
- CLI parsing behavior for `macos-sck` remains covered by existing tests.

macOS-only unit tests:

- `region_to_scap_area()` maps manual region fields exactly.
- negative manual region origins are rejected.
- `options_to_scap_options()` sets BGRA output, captured resolution,
  `captures_audio = false`, and cursor mode from `CaptureOptions`.
- `bgra_to_rgba_image()` converts channel order correctly.
- `bgra_to_rgba_image()` rejects invalid data length.
- `PortalPicker` is rejected by macOS option mapping.

Ignored real-capture smoke test:

```bash
ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test macos_sck_smoke -- --ignored --nocapture
```

The smoke test runs only on macOS and only when `ROLLSHOT_REAL_CAPTURE=1`.
It starts `MacosScreenCaptureKitBackend` with a small manual region, reads at
least three frames, asserts dimensions are non-zero, asserts metadata uses
`PixelFormat::Bgra`, and writes one PNG artifact to `target/test-artifacts/`.

Workspace verification:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

macOS backend compile verification should be run on macOS:

```bash
cargo test -p rollshot-capture --target aarch64-apple-darwin
```

## Risks

- **Scap MSRV bump.** Scap requires Rust 1.85. Mitigation: explicitly raise
  rollshot's workspace `rust-version` to 1.85 in this phase.
- **Scap beta API churn.** The dependency is a beta release. Mitigation: keep
  all direct scap usage inside `crates/rollshot-capture/src/macos/mod.rs`.
- **Coordinate ambiguity.** Manual region coordinates may be logical points
  while stitching consumes physical pixels. Mitigation: rely on scap output
  dimensions as authoritative, and keep Retina-specific correction out of
  scope until real smoke testing proves a mismatch.
- **Permission UX.** Screen Recording permission may require the user to
  restart the terminal or binary. Mitigation: map denial to
  `PermissionDenied` with a clear message.
- **Hosted CI limits.** Real capture needs an interactive desktop permission
  state. Mitigation: use ignored smoke tests and self-hosted/manual execution.

## Completion Criteria

- `rollshot-capture` uses crates.io `scap = "0.1.0-beta.1"` only on macOS.
- Workspace Rust version is `1.85`.
- `MacosScreenCaptureKitBackend::probe()` reports real scap support and Screen
  Recording permission state.
- `MacosScreenCaptureKitBackend::start()` returns a real frame stream on macOS
  when permission is granted.
- Manual and full-source capture modes are supported.
- Portal picker mode is rejected on macOS with `CaptureError::InvalidConfig`.
- Scap BGRA video frames are converted to `RgbaImage`.
- Pure conversion and option-mapping unit tests pass.
- Existing Linux and fixture behavior remains unchanged.
- Full workspace fmt, clippy, and tests pass on the local host.
- Ignored macOS real-capture smoke test exists for self-hosted/manual runs.
