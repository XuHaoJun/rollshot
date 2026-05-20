# Rollshot Linux Wayland Portal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Linux `linux-portal` stub with a real generic Wayland XDG Desktop Portal + PipeWire capture backend, validated first on KDE Plasma 6 Wayland.

**Architecture:** Keep the public `CaptureBackend` and `FrameStream` traits synchronous. Hide portal async work behind a Linux-only current-thread tokio runtime in `linux/portal.rs`, then connect the selected portal stream node through PipeWire in `linux/pipewire.rs`; pure pixel conversion and frame queue behavior stay independently testable. KDE behavior is represented as capabilities and quirks, not a separate backend.

**Tech Stack:** Rust 2021, `ashpd = 0.9` with tokio, `pipewire = 0.8`, `tokio = 1`, `nix = 0.29`, `image = 0.25`, existing `anyhow` / `thiserror`.

**Spec:** `docs/superpowers/specs/2026-05-20-rollshot-linux-wayland-portal-design.md`

**Reference-only material:** `learn-projects/obs-studio/plugins/linux-pipewire/screencast-portal.c` and `learn-projects/obs-studio/plugins/linux-pipewire/pipewire.c`. OBS is GPL and must not be copied. Use it only to confirm lifecycle, KDE multiple-stream behavior, cursor mode selection, PipeWire fd duplication, and metadata concepts.

**Assumptions:**
- The workspace remains on Rust 1.85.
- `ashpd 0.9.x` is the intended API line even though newer versions exist; the spec locks this phase to 0.9.
- The ashpd / PipeWire method calls shown in step code blocks are **illustrative**, not literal. ashpd 0.9 uses a builder pattern (`Screencast::new()` returns a proxy builder; `select_sources` takes a session reference plus several positional args). The implementer must reconcile the exact method shapes against the installed ashpd 0.9 docs during implementation; tests pin behavior, not method names.
- Real PipeWire buffer metadata requires a tiny raw-pointer boundary. Keep that boundary in one module and test behavior through safe helper inputs.
- No git worktree is created for execution. If a branch is needed, create it in place with `git checkout -b linux-wayland-portal`.

---

## File Map

**Workspace root**
- Modify: `Cargo.toml` to add Linux-only workspace dependencies.
- Modify: `.github/workflows/ci.yml` to install Linux capture build dependencies.
- Modify: `.github/workflows/real-capture.yml` to run the ignored Linux smoke test on the self-hosted KDE Wayland runner.
- Modify: `README.md` with Linux build prerequisites and manual validation commands.

**rollshot-capture**
- Modify: `crates/rollshot-capture/Cargo.toml` to add Linux target dependencies and scope the unsafe lint decision.
- Modify: `crates/rollshot-capture/src/lib.rs` to expose `linux` submodules through the existing backend export only.
- Modify: `crates/rollshot-capture/src/linux/mod.rs` to become the backend coordinator instead of an environment-only stub.
- Create: `crates/rollshot-capture/src/linux/pixel.rs` for raw frame to `RgbaImage`.
- Create: `crates/rollshot-capture/src/linux/portal.rs` for capabilities, quirks, portal option mapping, stream selection, probe collection, and ashpd lifecycle.
- Create: `crates/rollshot-capture/src/linux/pipewire.rs` for fd duplication, PipeWire connection, frame queue, buffer inspection, metadata, and `next_frame()`.
- Create: `crates/rollshot-capture/tests/linux_portal_smoke.rs` for ignored real capture validation.

---

## Task 1: Add Linux dependency preflight

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/rollshot-capture/Cargo.toml`

- [ ] **Step 1: Add workspace dependencies**

Edit the root `Cargo.toml` `[workspace.dependencies]` block so it includes these new entries after `scap`:

```toml
ashpd = { version = "0.9", default-features = false, features = ["tokio"] }
pipewire = "0.8"
tokio = { version = "1", features = ["rt", "sync", "time"] }
nix = { version = "0.29", features = ["fs"] }
```

`nix` needs the `fs` feature for the file constants module. The `fcntl` module
(`fcntl::fcntl` function used by `dup_pipewire_fd`) is always available in nix
0.29 without a feature gate.

- [ ] **Step 2: Add Linux-only capture deps**

Edit `crates/rollshot-capture/Cargo.toml` so the target dependency sections read:

```toml
[target.'cfg(target_os = "linux")'.dependencies]
ashpd = { workspace = true }
nix = { workspace = true }
pipewire = { workspace = true }
tokio = { workspace = true }

[target.'cfg(target_os = "macos")'.dependencies]
scap = { workspace = true }
```

- [ ] **Step 3: Verify dependency resolution before code work**

Run: `cargo check -p rollshot-capture --target x86_64-unknown-linux-gnu`

Expected: dependencies resolve and the existing stub still compiles. If the host lacks system packages, expected failure mentions `libpipewire-0.3` or `dbus-1` from `pkg-config`; install local dev packages before continuing:

```bash
sudo apt-get install -y pkg-config libpipewire-0.3-dev libclang-dev libdbus-1-dev
```

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock crates/rollshot-capture/Cargo.toml
git commit -m "chore(capture): add linux portal dependencies"
```

---

## Task 2: Create Linux module boundaries and pure portal models

**Files:**
- Modify: `crates/rollshot-capture/src/linux/mod.rs`
- Create: `crates/rollshot-capture/src/linux/portal.rs`
- Create: `crates/rollshot-capture/src/linux/pipewire.rs`
- Create: `crates/rollshot-capture/src/linux/pixel.rs`

- [ ] **Step 1: Split `linux/mod.rs` into submodules**

Replace the top of `crates/rollshot-capture/src/linux/mod.rs` with:

```rust
#![cfg(target_os = "linux")]

mod pipewire;
mod pixel;
mod portal;

use crate::backend::{CaptureBackend, FrameStream};
use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe};

use pipewire::LinuxPortalFrameStream;
use portal::PortalClient;

pub struct LinuxPortalBackend {
    portal: PortalClient,
}

impl LinuxPortalBackend {
    pub fn new() -> Self {
        Self {
            portal: PortalClient::new(),
        }
    }
}
```

Keep the existing `Default` impl. Replace the `CaptureBackend` impl with calls to `self.portal.probe()` and `self.portal.start(options)`, returning `LinuxPortalFrameStream` from the portal start result in a later task. Until Task 6, `start()` may still return `NotImplemented`.

- [ ] **Step 2: Add pure portal model tests**

Create `crates/rollshot-capture/src/linux/portal.rs` with these public-to-module types and tests:

```rust
use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe, RegionMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxDesktopProfile {
    Kde,
    Gnome,
    Wlroots,
    Hyprland,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxPortalQuirk {
    KdeMayReturnMultipleStreams,
    PortalRegionPickerLikelyAvailable,
    RegionPickerMayReturnVideoCrop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceTypes {
    pub monitor: bool,
    pub window: bool,
    pub virtual_source: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorModes {
    pub hidden: bool,
    pub embedded: bool,
    pub metadata: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxPortalCapabilities {
    pub desktop: String,
    pub session_type: String,
    pub portal_version: Option<u32>,
    pub source_types: SourceTypes,
    pub cursor_modes: CursorModes,
    pub profile: LinuxDesktopProfile,
    pub quirks: Vec<LinuxPortalQuirk>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalCursorMode {
    Hidden,
    Embedded,
    Metadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalSelectSourcesOptions {
    pub monitor: bool,
    pub window: bool,
    pub multiple: bool,
    pub cursor_mode: PortalCursorMode,
    pub persist: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalStreamInfo {
    pub node_id: u32,
}

#[derive(Debug, Clone)]
pub struct PortalStartResult {
    pub node_id: u32,
    pub capabilities: LinuxPortalCapabilities,
}

pub struct PortalClient;

impl PortalClient {
    pub fn new() -> Self {
        Self
    }

    pub fn probe(&self) -> CaptureProbe {
        probe_from_env(std::env::var("XDG_SESSION_TYPE").ok(), std::env::var("XDG_CURRENT_DESKTOP").ok())
    }

    pub fn start(&self, _options: CaptureOptions) -> Result<PortalStartResult, CaptureError> {
        Err(CaptureError::NotImplemented {
            backend: "linux-portal",
        })
    }
}

pub fn classify_desktop(desktop: &str) -> LinuxDesktopProfile {
    let lower = desktop.to_ascii_lowercase();
    if lower.contains("kde") || lower.contains("plasma") {
        LinuxDesktopProfile::Kde
    } else if lower.contains("gnome") {
        LinuxDesktopProfile::Gnome
    } else if lower.contains("hyprland") {
        LinuxDesktopProfile::Hyprland
    } else if lower.contains("sway") || lower.contains("wlroots") {
        LinuxDesktopProfile::Wlroots
    } else {
        LinuxDesktopProfile::Unknown
    }
}

pub fn quirks_for_profile(profile: LinuxDesktopProfile) -> Vec<LinuxPortalQuirk> {
    match profile {
        LinuxDesktopProfile::Kde => vec![
            LinuxPortalQuirk::KdeMayReturnMultipleStreams,
            LinuxPortalQuirk::PortalRegionPickerLikelyAvailable,
            LinuxPortalQuirk::RegionPickerMayReturnVideoCrop,
        ],
        _ => Vec::new(),
    }
}

pub fn choose_cursor_mode(cursors: CursorModes, show_cursor: bool) -> PortalCursorMode {
    if cursors.metadata {
        PortalCursorMode::Metadata
    } else if show_cursor && cursors.embedded {
        PortalCursorMode::Embedded
    } else {
        PortalCursorMode::Hidden
    }
}

pub fn select_sources_options(cursors: CursorModes, show_cursor: bool) -> PortalSelectSourcesOptions {
    PortalSelectSourcesOptions {
        monitor: true,
        window: true,
        multiple: false,
        cursor_mode: choose_cursor_mode(cursors, show_cursor),
        persist: false,
    }
}

pub fn choose_stream(streams: &[PortalStreamInfo]) -> Result<PortalStreamInfo, CaptureError> {
    streams.last().copied().ok_or_else(|| CaptureError::Backend(anyhow::anyhow!("portal returned no streams")))
}

fn probe_from_env(session_type: Option<String>, desktop: Option<String>) -> CaptureProbe {
    let session_type = session_type.unwrap_or_default();
    let desktop = desktop.unwrap_or_default();
    let is_wayland = session_type == "wayland";
    CaptureProbe {
        backend: "linux-portal",
        available: false,
        message: if is_wayland {
            "linux-portal probe needs ScreenCast portal diagnostics".to_string()
        } else {
            "linux-portal requires a Wayland session".to_string()
        },
        details: vec![
            ("os".to_string(), "linux".to_string()),
            ("XDG_SESSION_TYPE".to_string(), session_type),
            ("XDG_CURRENT_DESKTOP".to_string(), desktop),
        ],
    }
}
```

Add tests in the same file for `classify_desktop`, `quirks_for_profile`, all three cursor selection branches, `select_sources_options`, `choose_stream` zero/one/multiple, and non-Wayland probe message.

- [ ] **Step 3: Add placeholder-free module shells**

Create `crates/rollshot-capture/src/linux/pixel.rs`:

```rust
use crate::error::CaptureError;
use crate::types::Region;
use image::RgbaImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxPixelFormat {
    Bgra,
    Rgba,
    Bgrx,
    Rgbx,
    Rgb,
}

#[derive(Debug, Clone, Copy)]
pub struct LinuxRawFrame<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: LinuxPixelFormat,
    pub crop: Option<Region>,
}

pub fn raw_frame_to_rgba(_frame: LinuxRawFrame<'_>) -> Result<RgbaImage, CaptureError> {
    Err(CaptureError::NotImplemented {
        backend: "linux-portal-pixel",
    })
}
```

Create `crates/rollshot-capture/src/linux/pipewire.rs`:

```rust
use std::time::Duration;

use crate::backend::FrameStream;
use crate::error::CaptureError;
use crate::types::CapturedFrame;

pub const NEXT_FRAME_TIMEOUT: Duration = Duration::from_secs(5);

pub struct LinuxPortalFrameStream;

impl FrameStream for LinuxPortalFrameStream {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        Err(CaptureError::NotImplemented {
            backend: "linux-portal-pipewire",
        })
    }
}
```

- [ ] **Step 4: Run focused tests**

Run: `cargo test -p rollshot-capture --lib linux::portal::`

Expected: portal pure tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-capture/src/linux
git commit -m "feat(capture): add linux portal module boundaries"
```

---

## Task 3: Implement pixel conversion and crop semantics

**Files:**
- Modify: `crates/rollshot-capture/src/linux/pixel.rs`

- [ ] **Step 1: Write pixel conversion tests**

Add tests covering:
- BGRA converts to RGBA.
- RGBA is preserved.
- BGRx, RGBx, and RGB set alpha to 255.
- stride larger than row width is honored.
- crop is applied.
- crop outside bounds returns `CaptureError::InvalidConfig`.
- empty dimensions and too-short data return `InvalidConfig`.
- synthetic 3840x2160 BGRx conversion completes under 20 ms — write this test with `#[cfg_attr(debug_assertions, ignore)]` so it only runs in release builds. Debug bounds-check overhead makes the 20 ms ceiling unreliable, and CI runs `cargo test` in debug. Document the release-mode command in the README perf note.

MVP allocation note: `raw_frame_to_rgba()` allocates one `Vec<u8>` of size `width * height * 4` per call. At 4K BGRx @ 5 fps this is ~160 MB/s of allocation churn. Buffer pooling is a future optimization (see spec); the pure helper API is designed to allow it later (caller can preallocate and pass a `&mut Vec<u8>` if profile shows allocation is the bottleneck).

Use a two-pixel BGRA case like:

```rust
let frame = LinuxRawFrame {
    data: &[10, 20, 30, 40, 50, 60, 70, 80],
    width: 2,
    height: 1,
    stride: 8,
    format: LinuxPixelFormat::Bgra,
    crop: None,
};
let img = raw_frame_to_rgba(frame).unwrap();
assert_eq!(img.as_raw(), &[30, 20, 10, 40, 70, 60, 50, 80]);
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p rollshot-capture --lib linux::pixel::`

Expected: tests fail because `raw_frame_to_rgba()` returns `NotImplemented`.

- [ ] **Step 3: Replace `raw_frame_to_rgba()` with scalar implementation**

Implement:

```rust
fn bytes_per_pixel(format: LinuxPixelFormat) -> u32 {
    match format {
        LinuxPixelFormat::Bgra | LinuxPixelFormat::Rgba | LinuxPixelFormat::Bgrx | LinuxPixelFormat::Rgbx => 4,
        LinuxPixelFormat::Rgb => 3,
    }
}

fn validate_region(frame: LinuxRawFrame<'_>) -> Result<Region, CaptureError> {
    if frame.width == 0 || frame.height == 0 {
        return Err(CaptureError::InvalidConfig {
            message: "PipeWire frame dimensions must be non-zero".to_string(),
        });
    }
    let bpp = bytes_per_pixel(frame.format);
    let min_stride = frame.width.checked_mul(bpp).ok_or_else(|| CaptureError::InvalidConfig {
        message: "PipeWire frame row size overflowed u32".to_string(),
    })?;
    if frame.stride < min_stride {
        return Err(CaptureError::InvalidConfig {
            message: format!("PipeWire frame stride {} is smaller than row size {}", frame.stride, min_stride),
        });
    }
    let required = (frame.stride as usize)
        .checked_mul(frame.height as usize)
        .ok_or_else(|| CaptureError::InvalidConfig {
            message: "PipeWire frame buffer size overflowed usize".to_string(),
        })?;
    if frame.data.len() < required {
        return Err(CaptureError::InvalidConfig {
            message: format!("PipeWire frame buffer has {} bytes but needs at least {}", frame.data.len(), required),
        });
    }
    let region = frame.crop.unwrap_or(Region {
        x: 0,
        y: 0,
        width: frame.width,
        height: frame.height,
    });
    if region.x < 0 || region.y < 0 || region.width == 0 || region.height == 0 {
        return Err(CaptureError::InvalidConfig {
            message: format!("invalid crop region {:?}", region),
        });
    }
    let x2 = region.x as u32 + region.width;
    let y2 = region.y as u32 + region.height;
    if x2 > frame.width || y2 > frame.height {
        return Err(CaptureError::InvalidConfig {
            message: format!(
                "crop region {:?} is outside source frame {}x{}",
                region, frame.width, frame.height
            ),
        });
    }
    Ok(region)
}

pub fn raw_frame_to_rgba(frame: LinuxRawFrame<'_>) -> Result<RgbaImage, CaptureError> {
    let region = validate_region(frame)?;
    let bpp = bytes_per_pixel(frame.format) as usize;
    let mut out = vec![0; region.width as usize * region.height as usize * 4];
    let mut out_index = 0;

    for y in region.y as u32..region.y as u32 + region.height {
        let row_start = y as usize * frame.stride as usize;
        for x in region.x as u32..region.x as u32 + region.width {
            let pixel = row_start + x as usize * bpp;
            let rgba = match frame.format {
                LinuxPixelFormat::Bgra => [frame.data[pixel + 2], frame.data[pixel + 1], frame.data[pixel], frame.data[pixel + 3]],
                LinuxPixelFormat::Rgba => [frame.data[pixel], frame.data[pixel + 1], frame.data[pixel + 2], frame.data[pixel + 3]],
                LinuxPixelFormat::Bgrx => [frame.data[pixel + 2], frame.data[pixel + 1], frame.data[pixel], 255],
                LinuxPixelFormat::Rgbx => [frame.data[pixel], frame.data[pixel + 1], frame.data[pixel + 2], 255],
                LinuxPixelFormat::Rgb => [frame.data[pixel], frame.data[pixel + 1], frame.data[pixel + 2], 255],
            };
            out[out_index..out_index + 4].copy_from_slice(&rgba);
            out_index += 4;
        }
    }

    RgbaImage::from_raw(region.width, region.height, out).ok_or_else(|| CaptureError::Backend(anyhow::anyhow!("failed to build RGBA image from PipeWire frame")))
}
```

- [ ] **Step 4: Run pixel tests**

Run: `cargo test -p rollshot-capture --lib linux::pixel::`

Expected: all correctness pixel tests pass. The 4K perf test is `#[cfg_attr(debug_assertions, ignore)]` and skipped by default. Run it explicitly in release: `cargo test --release -p rollshot-capture --lib linux::pixel::four_k -- --ignored`.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-capture/src/linux/pixel.rs
git commit -m "feat(capture): convert linux raw frames to rgba"
```

---

## Task 4: Implement probe diagnostics with bounded portal calls

**Files:**
- Modify: `crates/rollshot-capture/src/linux/portal.rs`
- Modify: `crates/rollshot-capture/src/linux/mod.rs`

- [ ] **Step 1: Add probe abstraction tests**

Add a `ProbeSource` trait. The trait itself is `pub(super)` (production code uses it via `AshpdProbeSource`); only the fake implementation and the test harness are `#[cfg(test)]`:

```rust
pub(super) trait ProbeSource {
    fn screencast_version(&self) -> Result<u32, String>;
    fn available_source_types(&self) -> Result<SourceTypes, String>;
    fn available_cursor_modes(&self) -> Result<CursorModes, String>;
    fn pipewire_version(&self) -> Result<String, String>;
}
```

Place fake implementations and tests inside `#[cfg(test)] mod tests { ... }` so they don't bloat the production binary.

Tests must verify:
- Wayland + monitor source + PipeWire returns `available = true` without requiring KDE.
- X11 returns `Unsupported`-style probe details and `available = false`.
- a sleeping fake source returns within 300 ms when using a 100 ms test timeout and appends `probe_error` detail.
- missing monitor and window source types returns `available = false`.
- KDE desktop strings include the three quirks in `details`.

- [ ] **Step 2: Implement pure probe assembly**

Add:

```rust
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

fn build_probe_from_source<S: ProbeSource>(
    session_type: String,
    desktop: String,
    source: &S,
    timeout: std::time::Duration,
) -> CaptureProbe {
    let profile = classify_desktop(&desktop);
    let quirks = quirks_for_profile(profile);
    let mut details = vec![
        ("os".to_string(), "linux".to_string()),
        ("XDG_SESSION_TYPE".to_string(), session_type.clone()),
        ("XDG_CURRENT_DESKTOP".to_string(), desktop.clone()),
        ("desktop_profile".to_string(), format!("{profile:?}").to_ascii_lowercase()),
    ];

    let version = call_with_timeout("screencast_version", timeout, || source.screencast_version(), &mut details);
    let source_types = call_with_timeout("available_source_types", timeout, || source.available_source_types(), &mut details);
    let cursor_modes = call_with_timeout("available_cursor_modes", timeout, || source.available_cursor_modes(), &mut details);
    let pipewire = call_with_timeout("pipewire_version", timeout, || source.pipewire_version(), &mut details);

    let has_source = source_types.map(|s| s.monitor || s.window).unwrap_or(false);
    let has_pipewire = pipewire.is_some();

    details.push(("screencast_version".to_string(), version.map(|v| v.to_string()).unwrap_or_else(|| "unavailable".to_string())));
    details.push(("available_source_types".to_string(), source_types.map(format_source_types).unwrap_or_else(|| "unavailable".to_string())));
    details.push(("available_cursor_modes".to_string(), cursor_modes.map(format_cursor_modes).unwrap_or_else(|| "unavailable".to_string())));
    details.push(("pipewire_library_version".to_string(), pipewire.unwrap_or_else(|| "unavailable".to_string())));
    details.push(("quirks".to_string(), format_quirks(&quirks)));

    let available = session_type == "wayland" && has_source && has_pipewire;

    CaptureProbe {
        backend: "linux-portal",
        available,
        message: if available {
            "linux-portal ScreenCast and PipeWire diagnostics look ready".to_string()
        } else if session_type != "wayland" {
            "linux-portal requires a Wayland session".to_string()
        } else {
            "linux-portal ScreenCast or PipeWire diagnostics are incomplete".to_string()
        },
        details,
    }
}
```

Use `std::sync::mpsc` + `std::thread::scope` or a small tokio current-thread runtime for timeout tests. The timeout helper must return `None` and append `("probe_error", "... timed out ...")` when the call exceeds the deadline.

- [ ] **Step 3: Implement real ashpd probe source**

Add `AshpdProbeSource` behind non-test Linux code. It creates a current-thread tokio runtime and calls:

```rust
let proxy = ashpd::desktop::screencast::Screencast::new().await?;
let sources = proxy.available_source_types().await?;
let cursors = proxy.available_cursor_modes().await?;
let version = proxy.version().await?;
```

Map `ashpd::desktop::screencast::SourceType::{Monitor, Window, Virtual}` and `CursorMode::{Hidden, Embedded, Metadata}` into local `SourceTypes` and `CursorModes`. Obtain PipeWire version from `pipewire::get_library_version()`.

- [ ] **Step 4: Wire `PortalClient::probe()`**

Make `PortalClient::probe()` call `build_probe_from_source()` with `PROBE_TIMEOUT`, current env vars, and `AshpdProbeSource`. On DBus construction failure, return a probe with `screencast_available = false`, `pipewire_library_version` if available, and a `probe_error` detail containing the ashpd error text.

- [ ] **Step 5: Run probe tests**

Run: `cargo test -p rollshot-capture --lib linux::portal::`

Expected: portal probe and option tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-capture/src/linux/portal.rs crates/rollshot-capture/src/linux/mod.rs
git commit -m "feat(capture): probe linux screencast portal diagnostics"
```

---

## Task 5: Implement portal start lifecycle with ashpd

**Files:**
- Modify: `crates/rollshot-capture/src/linux/portal.rs`

- [ ] **Step 1: Add lifecycle tests against fakes**

Add fake portal tests that assert:
- non-Wayland start returns `CaptureError::Unsupported`.
- missing monitor/window capability returns `CaptureError::Unsupported`.
- negative manual region returns `CaptureError::InvalidConfig`.
- `SelectSources` uses monitor + window, `multiple = false`, no persistence.
- response cancellation maps to `CaptureError::UserCancelled`.
- response "other" maps to `CaptureError::Backend("portal interaction ended")`.
- multiple streams chooses the last stream.

- [ ] **Step 2: Define real session container**

Add:

```rust
pub struct PortalSession {
    pub node_id: u32,
    pub pipewire_fd: std::os::fd::OwnedFd,
    pub capabilities: LinuxPortalCapabilities,
    // close is declared LAST so it drops AFTER pipewire_fd. The PipeWire
    // teardown in LinuxPortalFrameStream::drop already closed the dup'd fd;
    // pipewire_fd here is the *original* ashpd fd, which must drop before
    // the portal session closes — matches spec drop order steps 4 → 5.
    close: Option<Box<dyn FnOnce() + Send>>,
}

impl Drop for PortalSession {
    fn drop(&mut self) {
        if let Some(close) = self.close.take() {
            // Drop closures cannot panic across FFI/async boundaries;
            // catch_unwind isolates any panic from ashpd's async close.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(close));
        }
    }
}
```

The `close` closure captures the `ashpd::desktop::Session` plus an `Arc<tokio::runtime::Runtime>` (or `tokio::runtime::Handle`) and looks like:

```rust
let close: Box<dyn FnOnce() + Send> = Box::new(move || {
    // Sync Drop calling async Session::close — block on the captured runtime.
    // Ignore the result: drop must not return errors, and ashpd may have
    // already cleaned up if the portal disconnected.
    rt.block_on(async { let _ = session.close().await; });
});
```

Notes:
- Field declaration order is load-bearing: Rust drops fields top-to-bottom, so `pipewire_fd` drops before `close` runs. Do not reorder.
- The closure must take ownership of the runtime (or a handle) — calling `block_on` on a runtime that has already been dropped will panic.
- The owning `LinuxPortalFrameStream` keeps `PortalSession` in a field declared AFTER the PipeWire connection so PipeWire teardown (thread loop stop → stream destroy → context destroy) runs first.

- [ ] **Step 3: Implement ashpd lifecycle**

In `PortalClient::start()`:
1. Reject `XDG_SESSION_TYPE != "wayland"` with `CaptureError::Unsupported { message: "linux-portal supports Linux capture through Wayland portals only".to_string() }`.
2. Reject `RegionMode::Manual(region)` when `region.x < 0 || region.y < 0`.
3. Run `probe()` or capability collection and reject if monitor/window are both unavailable.
4. Create an ashpd current-thread runtime.
5. `Screencast::new().await`.
6. `create_session().await`.
7. `select_sources(&session, cursor_mode, SourceType::Monitor | SourceType::Window, false, None, PersistMode::DoNot).await?.response().await?`.
8. `start(&session, &ashpd::WindowIdentifier::default()).await?.response().await?`.
9. Choose the last stream from `response.streams()`.
10. `open_pipe_wire_remote(&session).await`.
11. Return `PortalSession`.

Map `ashpd::desktop::request::ResponseError::Cancelled` to `CaptureError::UserCancelled` and `ResponseError::Other` to `CaptureError::Backend(anyhow::anyhow!("portal interaction ended"))`.

- [ ] **Step 4: Run start lifecycle tests**

Run: `cargo test -p rollshot-capture --lib linux::portal::`

Expected: fake lifecycle tests pass. Real ashpd calls are not made by unit tests.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-capture/src/linux/portal.rs
git commit -m "feat(capture): run xdg screencast portal lifecycle"
```

---

## Task 6: Implement fd duplication, frame queue, and lifecycle fakes

**Files:**
- Modify: `crates/rollshot-capture/src/linux/pipewire.rs`
- Modify: `crates/rollshot-capture/Cargo.toml`

- [ ] **Step 1: Scope the unsafe lint decision**

PipeWire metadata lookup and `OwnedFd::from_raw_fd` need a small unsafe boundary. The workspace currently sets `unsafe_code = "forbid"`, which cannot be overridden by `#[allow]` anywhere in the crate. Replace the inherited `[lints] workspace = true` block in `crates/rollshot-capture/Cargo.toml` with a crate-local policy that keeps unsafe code denied by default but allows it on individual annotated functions:

```toml
[lints.rust]
unsafe_code = "deny"

[lints.clippy]
all = "warn"
```

Then in `linux/pipewire.rs`, every unsafe block must:

1. Sit inside a function annotated `#[allow(unsafe_code)]` (this is the explicit per-callsite escape).
2. Be preceded by a `// SAFETY:` comment describing pointer validity, lifetime, and ownership transfer.
3. Stay in `linux/pipewire.rs` only — portal, pixel, and shared modules must remain `unsafe_code`-clean.

Rationale: `deny` keeps unsafe out of the rest of the crate while still allowing the documented per-function exceptions. `forbid` cannot be relaxed (that's the entire point of `forbid` vs `deny` in rustc). Don't blanket-allow unsafe at the crate level — that loses the safety net everywhere except where you actually need it.

- [ ] **Step 2: Add fd ownership tests**

In `pipewire.rs`, add `dup_pipewire_fd(input: BorrowedFd<'_>) -> Result<OwnedFd, CaptureError>` and a Linux unit test:

```rust
#[test]
fn dup_pipewire_fd_does_not_consume_input() {
    use std::io::{Read, Write};
    use std::os::fd::AsFd;

    let (mut reader, mut writer) = std::os::unix::net::UnixStream::pair().unwrap();
    let duplicated = dup_pipewire_fd(reader.as_fd()).unwrap();
    drop(duplicated);
    writer.write_all(b"x").unwrap();
    let mut byte = [0; 1];
    reader.read_exact(&mut byte).unwrap();
    assert_eq!(byte, [b'x']);
}
```

Implementation:

```rust
#[allow(unsafe_code)] // OwnedFd::from_raw_fd is the documented bridge from a raw kernel fd.
pub fn dup_pipewire_fd(input: std::os::fd::BorrowedFd<'_>) -> Result<std::os::fd::OwnedFd, CaptureError> {
    use nix::fcntl::{fcntl, FcntlArg};
    use std::os::fd::FromRawFd;

    let duplicated = fcntl(input, FcntlArg::F_DUPFD_CLOEXEC(5))
        .map_err(|err| CaptureError::Backend(anyhow::anyhow!("failed to duplicate PipeWire fd: {err}")))?;
    // SAFETY: fcntl(F_DUPFD_CLOEXEC) returns a fresh, exclusively-owned RawFd
    // with the CLOEXEC flag set. No other code path in this crate touches that
    // numeric fd before this OwnedFd is constructed, so ownership transfer is
    // unambiguous. The original `input` BorrowedFd is unaffected by F_DUPFD.
    Ok(unsafe { std::os::fd::OwnedFd::from_raw_fd(duplicated) })
}
```

- [ ] **Step 3: Add bounded latest-frame queue tests**

Add `FrameEvent` and queue tests:

```rust
enum FrameEvent {
    Frame(CapturedFrame),
    End,
    Error(String),
}
```

Queue primitive: `std::sync::Mutex<VecDeque<FrameEvent>>` with logical capacity 3, paired with a `std::sync::Condvar` (no new dependency; `sync_channel`'s drop-oldest semantics require a fragile sender/receiver dance, the Mutex+VecDeque approach matches what scap does on Linux).

Producer `push(event)`:
1. Lock the mutex.
2. If `len() >= 3`, `pop_front()` (drop the oldest frame).
3. `push_back(event)`.
4. `notify_one()` on the condvar; drop the lock.

Consumer `next_frame_with_timeout(timeout)`:
1. Lock the mutex.
2. `wait_timeout_while` on the condvar until the deque is non-empty or the timeout elapses.
3. If timed out with empty deque → `CaptureError::Backend("PipeWire stream produced no frames within {timeout:?}")`.
4. Otherwise `pop_front()` and translate the `FrameEvent`:
   - `Frame(f)` → `Ok(f)`
   - `End` → `Err(CaptureError::EndOfStream)`
   - `Error(msg)` → `Err(CaptureError::Backend(anyhow::anyhow!(msg)))`

Poison handling: a `PoisonError` from `lock()` means the producer thread panicked. Treat this as stream-fatal — log via `eprintln!` (no `tracing` dep yet) and return `CaptureError::EndOfStream` so the caller exits cleanly rather than retrying into the panic.

Tests must assert:
- Producing 5 frames into a capacity-3 queue retains only the newest 3 (frames 3, 4, 5).
- `next_frame_with_timeout(Duration::from_millis(100))` returns `CaptureError::Backend` containing "no frames within" when the queue is empty and no producer pushes.
- After producer enqueues `FrameEvent::End`, `next_frame()` returns `CaptureError::EndOfStream` (not `Backend`, not a frame).
- After producer enqueues `FrameEvent::Error(msg)`, `next_frame()` returns `CaptureError::Backend` whose message contains `msg`.

- [ ] **Step 4: Add drop order fake test**

Add fake resource traits or a `DropRecorder` used only in tests. Assert drop order is:

```text
pipewire_thread_loop_stop
pipewire_stream_disconnect_destroy
pipewire_context_destroy
original_fd_drop
portal_session_close
```

Do this through a `LinuxPortalFrameStream::from_test_resources(...)` constructor under `#[cfg(test)]`.

- [ ] **Step 5: Run pipewire pure tests**

Run: `cargo test -p rollshot-capture --lib linux::pipewire::`

Expected: fd duplication, queue timeout, latest-frame retention, and drop-order tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-capture/Cargo.toml crates/rollshot-capture/src/linux/pipewire.rs
git commit -m "feat(capture): add pipewire fd and frame queue primitives"
```

---

## Task 7: Connect real PipeWire stream and parse usable frames

**Files:**
- Modify: `crates/rollshot-capture/src/linux/pipewire.rs`
- Modify: `crates/rollshot-capture/src/linux/pixel.rs`

- [ ] **Step 1: Add pure buffer mapping tests**

Create safe input structs for unit tests:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxVideoTransform {
    Normal,
    Rotated90,
    Rotated180,
    Rotated270,
    Flipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoCrop {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxFrameMetadata {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: LinuxPixelFormat,
    pub crop: Option<VideoCrop>,
    pub transform: LinuxVideoTransform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxBufferType {
    MemPtr,
    DmaBuf,
}
```

Tests must verify:
- DMA-BUF maps to `CaptureError::Unsupported`.
- corrupted header skips the buffer without error.
- chunk corrupted flag skips the buffer without error.
- 10 consecutive empty buffers return backend error containing "did not produce a usable video frame". The empty-buffer counter lives **in the pure mapper as a `&mut u8` parameter**, mirroring macOS's `process_scap_frame(frame, empty_frames: &mut u8, ...)` pattern (see `crates/rollshot-capture/src/macos/mod.rs`). This keeps the loop state outside callbacks and unit-testable; the PipeWire `process` callback owns one `u8` field and passes a mutable borrow to the mapper.
- non-identity transform returns backend error naming the transform.
- manual crop outside post-VideoCrop frame returns `InvalidConfig` mentioning requested region and available size.

- [ ] **Step 2: Add format mapping**

Map `pipewire::spa::param::video::VideoFormat::{BGRA, RGBA, BGRx, RGBx, RGB}` to `LinuxPixelFormat`. Return `CaptureError::Unsupported` for every other negotiated format with message `unsupported PipeWire raw video format: <format>`.

- [ ] **Step 3: Build PipeWire connection from portal fd**

Implement `PipeWireConnection::connect_fd(portal_fd: OwnedFd, node_id: u32, options: CaptureOptions) -> Result<Self, CaptureError>`:
1. `pipewire::init()`.
2. `let thread_loop = pipewire::thread_loop::ThreadLoop::new(Some("rollshot-pipewire".to_string()), None)?;`
3. `let context = pipewire::context::Context::new(&thread_loop)?;`
4. Duplicate `portal_fd.as_fd()` via `dup_pipewire_fd()`.
5. Pass the duplicate `OwnedFd` to `context.connect_fd(dup_fd, None)?`.
6. Create a `pipewire::stream::Stream::new(&core, "rollshot-screen", properties! { MEDIA_TYPE => "Video", MEDIA_CATEGORY => "Capture", MEDIA_ROLE => "Screen" })?`.
7. Register `state_changed`, `param_changed`, and `process` listeners.
8. Connect the stream with `Direction::Input`, `Some(node_id)`, `AUTOCONNECT | MAP_BUFFERS`, and an enum-format pod listing BGRA, RGBA, BGRx, RGBx, RGB. Include framerate from `CaptureOptions.fps`.
9. Start the thread loop after listeners are registered.

- [ ] **Step 4: Request metadata and MemPtr buffers**

In `param_changed`, when a raw video format is negotiated, call `stream.update_params()` with:
- `SPA_PARAM_Meta` for Header.
- `SPA_PARAM_Meta` for VideoCrop.
- `SPA_PARAM_Meta` for Cursor.
- `SPA_PARAM_Meta` for VideoTransform when supported by available spa constants.
- `SPA_PARAM_Buffers` with `dataType = 1 << SPA_DATA_MemPtr`.

Do not request DMA-BUF. If a dequeued data plane reports `DataType::DmaBuf`, enqueue `FrameEvent::Error("unexpected PipeWire DMA-BUF buffer")`.

- [ ] **Step 5: Process callbacks**

In `process`, dequeue a buffer, reject empty/corrupted/DMA-BUF buffers through the pure mapper, convert `MemPtr` data to `RgbaImage` using `raw_frame_to_rgba()`, and enqueue `FrameEvent::Frame(CapturedFrame { image, timestamp: SystemTime::now(), metadata })`. Keep callbacks limited to copy/convert/enqueue; no stitching.

- [ ] **Step 6: Run tests**

Run: `cargo test -p rollshot-capture --lib linux::pipewire:: linux::pixel::`

Expected: pure PipeWire and pixel tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/rollshot-capture/src/linux/pipewire.rs crates/rollshot-capture/src/linux/pixel.rs
git commit -m "feat(capture): connect pipewire portal stream"
```

---

## Task 8: Wire LinuxPortalBackend start and manual crop behavior

**Files:**
- Modify: `crates/rollshot-capture/src/linux/mod.rs`
- Modify: `crates/rollshot-capture/src/linux/portal.rs`
- Modify: `crates/rollshot-capture/src/linux/pipewire.rs`

Do not modify `crates/rollshot-capture/src/types.rs` in this task. The spec mentions `scale_factor` but the current `FrameMetadata` has no such field and this plan deliberately defers adding it. If a later task or reviewer requires metadata parity, add `types.rs` to a follow-up task's Files list at that point.

- [ ] **Step 1: Add backend integration unit tests with fakes**

Tests must verify:
- `LinuxPortalBackend::name()` remains `linux-portal`.
- `start(RegionMode::PortalPicker)` passes no manual crop to PipeWire.
- `start(RegionMode::FullSource)` passes no manual crop to PipeWire.
- `start(RegionMode::Manual(region))` passes local crop after portal crop.
- manual region outside a 1000x800 post-VideoCrop frame returns `InvalidConfig` containing both the requested region and `1000x800`.

- [ ] **Step 2: Add constructors for production and tests**

Keep `LinuxPortalBackend::new()` as the production constructor. Add a `#[cfg(test)]` constructor that accepts fake portal and fake PipeWire builders so unit tests never open DBus or PipeWire.

- [ ] **Step 3: Implement production `start()`**

`LinuxPortalBackend::start(options)`:
1. `let portal_session = self.portal.start(options.clone())?;`
2. `let stream = LinuxPortalFrameStream::connect(portal_session, options)?;`
3. `Ok(Box::new(stream))`.

`LinuxPortalFrameStream::connect()` owns the portal session so drop order stays in one object. The struct's field order is **load-bearing** — Rust drops fields top-to-bottom, so PipeWire teardown (loop stop → stream destroy → context destroy) must precede portal-session close per the spec's drop order:

```rust
pub struct LinuxPortalFrameStream {
    // 1. PipeWire connection drops FIRST: stops thread loop, disconnects/destroys
    //    stream, destroys context (which closes the dup'd fd from F_DUPFD_CLOEXEC).
    pipewire: PipeWireConnection,
    // 2. PortalSession drops SECOND: drops original ashpd OwnedFd, then runs the
    //    block_on(session.close().await) closure (see Task 5 Step 2).
    portal: PortalSession,
    // 3. Tokio runtime (if owned at stream scope rather than inside PortalSession)
    //    drops LAST. Keep this field at the bottom or in PortalSession's close
    //    closure. Never drop the runtime before block_on-ing the session close.
}
```

Do not reorder these fields. Add a comment in the source pinning this requirement.

- [ ] **Step 4: Apply frame metadata rules**

For every returned `CapturedFrame.metadata`:
- `backend = "linux-portal"`.
- `source_size = Some(Size { width: negotiated_width, height: negotiated_height })`.
- `effective_region = VideoCrop if present, else Manual region if applied, else None`.
- `pixel_format = Some(original PipeWire pixel format)`.
- `stride = Some(source stride)`.

If `FrameMetadata` lacks `scale_factor`, leave it unchanged; the spec mentions `scale_factor = None`, but the current type has no field. Add the field only if a previous task or reviewer explicitly requires metadata parity.

- [ ] **Step 5: Run Linux backend tests**

Run: `cargo test -p rollshot-capture --lib linux::`

Expected: Linux unit tests pass without a live portal.

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-capture/src/linux crates/rollshot-capture/src/types.rs
git commit -m "feat(capture): wire linux portal backend start"
```

---

## Task 9: Add ignored real Linux smoke test and self-hosted workflow

**Files:**
- Create: `crates/rollshot-capture/tests/linux_portal_smoke.rs`
- Modify: `.github/workflows/real-capture.yml`

- [ ] **Step 1: Add ignored smoke test**

Create `crates/rollshot-capture/tests/linux_portal_smoke.rs`:

```rust
#![cfg(target_os = "linux")]

use std::path::PathBuf;

use rollshot_capture::{CaptureBackend, CaptureOptions, LinuxPortalBackend, RegionMode};

#[test]
#[ignore = "requires live Linux Wayland desktop and human portal picker interaction"]
fn captures_linux_portal_frames() {
    if std::env::var("ROLLSHOT_REAL_CAPTURE").as_deref() != Ok("1") {
        eprintln!("set ROLLSHOT_REAL_CAPTURE=1 to run real Linux portal capture");
        return;
    }
    assert_eq!(
        std::env::var("XDG_SESSION_TYPE").as_deref(),
        Ok("wayland"),
        "linux portal smoke test requires XDG_SESSION_TYPE=wayland"
    );

    let mut backend = LinuxPortalBackend::new();
    let mut stream = backend
        .start(CaptureOptions {
            region: RegionMode::PortalPicker,
            fps: 5,
            show_cursor: false,
            prefer_portal_region: true,
        })
        .expect("start linux portal capture");

    let mut first = None;
    for _ in 0..3 {
        let frame = stream.next_frame().expect("next portal frame");
        assert!(frame.image.width() > 0);
        assert!(frame.image.height() > 0);
        assert_eq!(frame.metadata.backend, "linux-portal");
        first.get_or_insert(frame);
    }

    let artifact = PathBuf::from("target/test-artifacts/linux_portal_first_frame.png");
    std::fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    first.unwrap().image.save(&artifact).unwrap();
}
```

- [ ] **Step 2: Update self-hosted workflow**

Replace the Linux job's explanatory step in `.github/workflows/real-capture.yml` with:

```yaml
      - name: Install Linux capture deps
        run: |
          sudo apt-get update
          sudo apt-get install -y pkg-config libpipewire-0.3-dev libclang-dev libdbus-1-dev

      - name: Run Linux portal smoke test
        env:
          ROLLSHOT_REAL_CAPTURE: "1"
        run: cargo test -p rollshot-capture --test linux_portal_smoke -- --ignored --nocapture
```

- [ ] **Step 3: Run ignored-test compile**

Run: `cargo test -p rollshot-capture --test linux_portal_smoke --no-run`

Expected: smoke test compiles; it does not open the portal picker.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-capture/tests/linux_portal_smoke.rs .github/workflows/real-capture.yml
git commit -m "test(capture): add linux portal smoke test"
```

---

## Task 10: Update hosted CI Linux dependencies

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add apt install step between Rust setup and Format**

In `.github/workflows/ci.yml`, insert the install step **after** the `Install Rust` step and **before** the `Format` step. (Format itself doesn't need the libs, but Clippy and Test will fail to build `pipewire-sys` without them, so the deps must be present before any cargo invocation reaches the capture crate.)

```yaml
      - name: Install Linux capture deps
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y pkg-config libpipewire-0.3-dev libclang-dev libdbus-1-dev
```

- [ ] **Step 2: Validate workflow syntax locally**

Run: `ruby -e 'require "yaml"; YAML.load_file(".github/workflows/ci.yml"); puts "ok"'`

Expected: prints `ok`.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: install linux portal build dependencies"
```

---

## Task 11: Document Linux portal validation

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Replace future Linux section**

Replace `## Manual Testing: Future Linux KDE Wayland Capture` with `## Manual Testing: Linux Wayland Portal Capture` and include:

```markdown
## Manual Testing: Linux Wayland Portal Capture

Linux capture uses the XDG Desktop Portal ScreenCast interface and PipeWire.
KDE Plasma 6 on Wayland is the first validated target. Other Wayland desktops
can work when their portal implements the standard ScreenCast flow, but
rectangular portal-region picking is desktop-specific.

Install development packages on Debian/Ubuntu:

```bash
sudo apt-get install -y pkg-config libpipewire-0.3-dev libclang-dev libdbus-1-dev
```

Required services:

- PipeWire (`libpipewire-0.3`)
- WirePlumber or equivalent session manager
- `xdg-desktop-portal`
- a desktop portal implementation such as `xdg-desktop-portal-kde`

Manual checks:

- [ ] `XDG_SESSION_TYPE=wayland`.
- [ ] PipeWire and WirePlumber are running.
- [ ] `xdg-desktop-portal` is running.
- [ ] On KDE, `xdg-desktop-portal-kde` is running.
- [ ] `cargo run -p rollshot-cli -- probe --json` reports `linux-portal` availability.
- [ ] `cargo run -p rollshot-cli -- capture --backend linux-portal --region portal --max-frames 3 --output target/test-artifacts/linux_portal.png` opens the portal picker and writes a PNG.
- [ ] `cargo run -p rollshot-cli -- capture --backend linux-portal --region full --max-frames 3 --output target/test-artifacts/linux_full.png` writes a PNG.
- [ ] `cargo run -p rollshot-cli -- capture --backend linux-portal --region "0,0 900x700" --max-frames 3 --output target/test-artifacts/linux_manual.png` writes a locally cropped PNG.
- [ ] `cargo run -p rollshot-cli -- capture --backend linux-portal --region portal --max-frames 3 --dump-frames target/test-artifacts/linux-frames --output target/test-artifacts/linux_dumped.png` writes frame dumps.
- [ ] `ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test linux_portal_smoke -- --ignored --nocapture` captures at least three frames and writes `target/test-artifacts/linux_portal_first_frame.png`.

The smoke test requires a live human-driven desktop session because the portal
picker must be clicked. Hosted CI must not run it.
```

- [ ] **Step 2: Update top-level status**

Change the README intro sentence that says "The KDE Wayland backend is still planned for a later phase" to say the Linux Wayland portal backend is available on systems with ScreenCast portal and PipeWire support.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: add linux portal manual testing"
```

---

## Task 12: Final verification

**Files:**
- Read-only verification across workspace.

- [ ] **Step 1: Format**

Run: `cargo fmt --all -- --check`

Expected: no formatting diffs.

- [ ] **Step 2: Clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`

Expected: no warnings. If clippy flags unsafe docs, add precise safety comments to the unsafe blocks in `linux/pipewire.rs`.

- [ ] **Step 3: Tests**

Run: `cargo test --workspace`

Expected: all non-ignored tests pass. The Linux portal smoke test compiles but does not run because it is ignored.

- [ ] **Step 4: Probe command**

Run: `cargo run -p rollshot-cli -- probe --json`

Expected: JSON includes `linux-portal` on Linux with details for `XDG_SESSION_TYPE`, `XDG_CURRENT_DESKTOP`, portal source/cursor capability fields, PipeWire version, desktop profile, and quirks.

- [ ] **Step 5: Real KDE Wayland manual smoke**

On a live KDE Plasma 6 Wayland desktop:

```bash
ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test linux_portal_smoke -- --ignored --nocapture
```

Expected: portal picker opens, at least three frames are received, and `target/test-artifacts/linux_portal_first_frame.png` is written.

- [ ] **Step 6: Commit any verification-only fixes**

If verification required code or doc changes:

```bash
git add <changed-files>
git commit -m "fix(capture): address linux portal verification"
```

---

## Self-Review

**Spec coverage:** This plan covers real `probe()`, `CreateSession` / `SelectSources` / `Start` / `OpenPipeWireRemote`, `F_DUPFD_CLOEXEC`, drop order, 5-second `next_frame()` timeout, `PortalPicker` / `FullSource` / `Manual`, CPU-readable raw formats, stride, `VideoCrop`, corrupted/empty buffers, non-identity transforms, pure tests, ignored KDE smoke test, README, and CI apt packages.

**Known design issue surfaced:** The workspace forbids unsafe code globally. The plan scopes the required `OwnedFd::from_raw_fd` boundary to `rollshot-capture/src/linux/pipewire.rs` and changes that crate's lint policy from `forbid` to `deny` (not `allow`), so individual functions can opt in with `#[allow(unsafe_code)]` while the rest of the crate stays unsafe-clean. If reviewers reject that tradeoff, the alternative is a tiny Linux-only helper crate with no workspace lint inheritance and a safe API exposed back to `rollshot-capture`.

**ashpd API specifics:** Method calls in the plan (`Screencast::new()`, `select_sources(..., PersistMode::DoNot)`, `WindowIdentifier::default()`, etc.) are illustrative. ashpd 0.9 uses a builder pattern with positional cursor/source/persist args; the implementer reconciles exact shapes during Task 5 against installed ashpd 0.9 documentation. Tests pin behavior, not method names.

**Placeholder scan:** The plan intentionally avoids deferred requirements. Every task has concrete files, commands, expected outcomes, and commits. The pipewire queue, drop order, probe assembly, and pixel conversion are all specified concretely with both the pure helper signature and the test fixture shape.

**Drop order invariants (encoded in code, not just docs):**
- `LinuxPortalFrameStream` field order: `pipewire` first, then `portal` — Rust drops fields top-to-bottom.
- `PortalSession` field order: `pipewire_fd` (the original ashpd `OwnedFd`) before `close` (the boxed `block_on(session.close())` closure).
- Reordering either struct silently violates the spec's required drop order.

**Completion handoff:** Execute tasks in order. Do not start Task 7 until Tasks 3, 5, and 6 pass, because real PipeWire processing depends on the pure pixel, portal, fd, queue, and lifecycle helpers.
