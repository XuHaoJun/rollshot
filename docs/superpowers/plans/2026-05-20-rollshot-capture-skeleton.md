# Rollshot Capture Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the `rollshot capture` and `rollshot probe` CLI surface through a refactored `rollshot-capture` API (RgbaImage + CaptureError), backed by a fixture-driven fake backend and platform stubs, without writing any real capture code.

**Architecture:** `rollshot-capture` owns a `CaptureBackend` / `FrameStream` trait pair and a `CaptureError` (thiserror). A `FixtureBackend` decodes PNG/JPEG from a directory and feeds them through the trait. `LinuxPortalBackend` and `MacosScreenCaptureKitBackend` stubs expose honest `probe()` output but return `NotImplemented` from `start()`. `rollshot-cli` drives the loop with `clap`-derived subcommands, dumps frames on request, stitches via `rollshot-core`, and writes a PNG. Exit codes are mapped from `CaptureError` variants.

**Tech Stack:** Rust 2021, `image` 0.25, `clap` 4 (derive), `anyhow`, `thiserror`, `serde`/`serde_json`.

**Spec:** `docs/superpowers/specs/2026-05-20-rollshot-capture-skeleton-design.md`

---

## File Map

**Workspace root**
- Modify: `Cargo.toml` (workspace deps)

**rollshot-capture**
- Modify: `crates/rollshot-capture/Cargo.toml` (add deps)
- Modify: `crates/rollshot-capture/src/lib.rs` (re-exports)
- Modify: `crates/rollshot-capture/src/types.rs` (CapturedFrame uses RgbaImage; CaptureProbe gains `details`)
- Modify: `crates/rollshot-capture/src/backend.rs` (trait uses CaptureError; FakeFrameStream moves out)
- Create: `crates/rollshot-capture/src/error.rs`
- Create: `crates/rollshot-capture/src/fake.rs`
- Create: `crates/rollshot-capture/src/fixture.rs`
- Create: `crates/rollshot-capture/src/linux/mod.rs` (cfg linux)
- Create: `crates/rollshot-capture/src/macos/mod.rs` (cfg macos)

**rollshot-cli**
- Modify: `crates/rollshot-cli/Cargo.toml` (add deps)
- Modify: `crates/rollshot-cli/src/main.rs` (map CliError → ExitCode)
- Modify: `crates/rollshot-cli/src/lib.rs` (clap subcommands; thin entry)
- Create: `crates/rollshot-cli/src/cli_error.rs`
- Create: `crates/rollshot-cli/src/args.rs` (clap derive types)
- Create: `crates/rollshot-cli/src/cmd_capture.rs`
- Create: `crates/rollshot-cli/src/cmd_probe.rs`
- Create: `crates/rollshot-cli/src/cmd_stitch_folder.rs` (extracted; behavior unchanged)
- Modify: `crates/rollshot-cli/tests/cli_smoke.rs` (probe assertion updated)
- Create: `crates/rollshot-cli/tests/capture_fixture.rs`
- Create: `crates/rollshot-cli/tests/capture_stubs.rs`
- Create: `crates/rollshot-cli/tests/probe_cli.rs`

---

## Task 1: Add workspace dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Update workspace deps**

Replace the existing `[workspace.dependencies]` block in `Cargo.toml` with:

```toml
[workspace.dependencies]
anyhow = "1"
thiserror = "1"
clap = { version = "4", features = ["derive"] }
image = { version = "0.25", default-features = false, features = ["png", "jpeg"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

- [ ] **Step 2: Verify the workspace still builds**

Run: `cargo build --workspace`
Expected: builds with no errors. New deps will be downloaded but unused by any crate yet (warnings are OK at this stage; do not add deny-unused-deps).

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add anyhow, thiserror, clap, serde workspace deps"
```

---

## Task 2: CaptureError type

**Files:**
- Modify: `crates/rollshot-capture/Cargo.toml`
- Create: `crates/rollshot-capture/src/error.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`

- [ ] **Step 1: Add crate deps**

Edit `crates/rollshot-capture/Cargo.toml` so the `[dependencies]` block reads:

```toml
[dependencies]
anyhow = { workspace = true }
image = { workspace = true }
thiserror = { workspace = true }
```

- [ ] **Step 2: Write the failing test**

Create `crates/rollshot-capture/src/error.rs`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("backend not implemented yet: {backend}")]
    NotImplemented { backend: &'static str },

    #[error("backend unsupported on this host: {message}")]
    Unsupported { message: String },

    #[error("user cancelled capture")]
    UserCancelled,

    #[error("permission denied: {message}")]
    PermissionDenied { message: String },

    #[error("end of frame stream")]
    EndOfStream,

    #[error("invalid configuration: {message}")]
    InvalidConfig { message: String },

    #[error(transparent)]
    Backend(#[from] anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::CaptureError;

    #[test]
    fn not_implemented_includes_backend_name() {
        let err = CaptureError::NotImplemented {
            backend: "linux-portal",
        };
        let text = err.to_string();
        assert!(text.contains("linux-portal"), "text = {text}");
        assert!(text.contains("not implemented"), "text = {text}");
    }

    #[test]
    fn permission_denied_includes_message() {
        let err = CaptureError::PermissionDenied {
            message: "Screen Recording".to_string(),
        };
        assert!(err.to_string().contains("Screen Recording"));
    }

    #[test]
    fn invalid_config_includes_message() {
        let err = CaptureError::InvalidConfig {
            message: "bad region".to_string(),
        };
        assert!(err.to_string().contains("bad region"));
    }

    #[test]
    fn end_of_stream_renders() {
        let err = CaptureError::EndOfStream;
        assert!(err.to_string().contains("end of frame stream"));
    }
}
```

Add to `crates/rollshot-capture/src/lib.rs` (replace its full contents):

```rust
pub mod backend;
pub mod error;
pub mod types;

pub use backend::{CaptureBackend, FakeFrameStream, FrameStream};
pub use error::CaptureError;
pub use types::{
    CaptureOptions, CaptureProbe, CapturedFrame, FrameMetadata, PixelFormat, Region, RegionMode,
    Size,
};
```

- [ ] **Step 3: Run the tests and verify they pass**

Run: `cargo test -p rollshot-capture --lib error::`
Expected: 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-capture/Cargo.toml crates/rollshot-capture/src/error.rs crates/rollshot-capture/src/lib.rs
git commit -m "feat(capture): add CaptureError thiserror enum"
```

---

## Task 3: CapturedFrame uses RgbaImage; trait uses CaptureError; move FakeFrameStream to its own module

This is a single coupled refactor. The capture crate must compile and pass tests at the end of the task; intermediate states may not.

**Files:**
- Modify: `crates/rollshot-capture/src/types.rs`
- Modify: `crates/rollshot-capture/src/backend.rs`
- Create: `crates/rollshot-capture/src/fake.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`

- [ ] **Step 1: Rewrite `crates/rollshot-capture/src/types.rs`**

Replace its full contents with:

```rust
use std::time::SystemTime;

use image::RgbaImage;

#[derive(Debug, Clone)]
pub struct CapturedFrame {
    pub image: RgbaImage,
    pub timestamp: SystemTime,
    pub metadata: FrameMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureOptions {
    pub region: RegionMode,
    pub fps: u32,
    pub show_cursor: bool,
    pub prefer_portal_region: bool,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            region: RegionMode::FullSource,
            fps: 5,
            show_cursor: false,
            prefer_portal_region: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureProbe {
    pub backend: &'static str,
    pub available: bool,
    pub message: String,
    pub details: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameMetadata {
    pub source_size: Option<Size>,
    pub effective_region: Option<Region>,
    pub pixel_format: Option<PixelFormat>,
    pub stride: Option<u32>,
    pub backend: &'static str,
}

impl FrameMetadata {
    pub fn fake() -> Self {
        Self {
            source_size: None,
            effective_region: None,
            pixel_format: Some(PixelFormat::Rgba),
            stride: None,
            backend: "fake",
        }
    }

    pub fn fixture() -> Self {
        Self {
            source_size: None,
            effective_region: None,
            pixel_format: Some(PixelFormat::Rgba),
            stride: None,
            backend: "fixture",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegionMode {
    Manual(Region),
    PortalPicker,
    FullSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Rgba,
    Bgra,
    Bgrx,
    Rgbx,
    Rgb,
}
```

Note: `CapturedFrame` is `Debug + Clone` only; it can no longer be compared with `assert_eq!` because `RgbaImage` does not implement `PartialEq` cheaply.

- [ ] **Step 2: Move FakeFrameStream into its own module**

Create `crates/rollshot-capture/src/fake.rs`:

```rust
use crate::error::CaptureError;
use crate::types::CapturedFrame;
use crate::FrameStream;

/// In-memory frame stream used by unit tests. Not reachable from the CLI.
#[derive(Debug, Clone)]
pub struct FakeFrameStream {
    frames: Vec<CapturedFrame>,
    index: usize,
}

impl FakeFrameStream {
    pub fn new(frames: Vec<CapturedFrame>) -> Self {
        Self { frames, index: 0 }
    }
}

impl FrameStream for FakeFrameStream {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        let frame = self
            .frames
            .get(self.index)
            .cloned()
            .ok_or(CaptureError::EndOfStream)?;
        self.index += 1;
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::FakeFrameStream;
    use crate::error::CaptureError;
    use crate::types::{CapturedFrame, FrameMetadata};
    use crate::FrameStream;
    use image::{Rgba, RgbaImage};
    use std::time::SystemTime;

    fn make_frame(color: [u8; 4]) -> CapturedFrame {
        CapturedFrame {
            image: RgbaImage::from_pixel(1, 1, Rgba(color)),
            timestamp: SystemTime::UNIX_EPOCH,
            metadata: FrameMetadata::fake(),
        }
    }

    #[test]
    fn fake_stream_returns_frames_in_order() {
        let first = make_frame([255, 0, 0, 255]);
        let second = make_frame([0, 255, 0, 255]);
        let mut stream = FakeFrameStream::new(vec![first.clone(), second.clone()]);

        let got_first = stream.next_frame().expect("first frame");
        assert_eq!(got_first.image.get_pixel(0, 0).0, [255, 0, 0, 255]);

        let got_second = stream.next_frame().expect("second frame");
        assert_eq!(got_second.image.get_pixel(0, 0).0, [0, 255, 0, 255]);

        match stream.next_frame() {
            Err(CaptureError::EndOfStream) => {}
            other => panic!("expected EndOfStream, got {other:?}"),
        }
    }
}
```

- [ ] **Step 3: Slim `crates/rollshot-capture/src/backend.rs` to just the traits**

Replace its full contents with:

```rust
use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe, CapturedFrame};

pub trait CaptureBackend {
    fn name(&self) -> &'static str;
    fn probe(&self) -> CaptureProbe;
    fn start(
        &mut self,
        options: CaptureOptions,
    ) -> Result<Box<dyn FrameStream>, CaptureError>;
}

pub trait FrameStream: Send {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError>;
}
```

- [ ] **Step 4: Update `crates/rollshot-capture/src/lib.rs`**

Replace its full contents with:

```rust
pub mod backend;
pub mod error;
pub mod fake;
pub mod types;

pub use backend::{CaptureBackend, FrameStream};
pub use error::CaptureError;
pub use fake::FakeFrameStream;
pub use types::{
    CaptureOptions, CaptureProbe, CapturedFrame, FrameMetadata, PixelFormat, Region, RegionMode,
    Size,
};
```

- [ ] **Step 5: Verify the crate still compiles and tests pass**

Run: `cargo test -p rollshot-capture`
Expected: all tests pass, including the moved FakeFrameStream test.

Run: `cargo build --workspace`
Expected: workspace builds. `rollshot-cli` and `rollshot-core` do not depend on the changed types directly, so they should be unaffected.

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-capture/src
git commit -m "refactor(capture): switch CapturedFrame to RgbaImage and CaptureError"
```

---

## Task 4: FixtureBackend

**Files:**
- Create: `crates/rollshot-capture/src/fixture.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/rollshot-capture/src/fixture.rs`:

```rust
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use image::DynamicImage;

use crate::backend::{CaptureBackend, FrameStream};
use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe, CapturedFrame, FrameMetadata};

pub struct FixtureBackend {
    dir: PathBuf,
}

impl FixtureBackend {
    pub fn new<P: Into<PathBuf>>(dir: P) -> Self {
        Self { dir: dir.into() }
    }

    fn collect_frames(&self) -> Result<Vec<PathBuf>, CaptureError> {
        if !self.dir.is_dir() {
            return Err(CaptureError::InvalidConfig {
                message: format!("fixture directory not found: {}", self.dir.display()),
            });
        }

        let entries = fs::read_dir(&self.dir).map_err(|err| {
            CaptureError::InvalidConfig {
                message: format!("failed to read {}: {err}", self.dir.display()),
            }
        })?;

        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|err| CaptureError::InvalidConfig {
                message: format!("failed to read entry in {}: {err}", self.dir.display()),
            })?;
            let path = entry.path();
            let is_file = entry
                .file_type()
                .map(|t| t.is_file())
                .unwrap_or(false);
            if !is_file {
                continue;
            }
            let ext = path
                .extension()
                .and_then(OsStr::to_str)
                .map(str::to_ascii_lowercase);
            if matches!(ext.as_deref(), Some("png" | "jpg" | "jpeg")) {
                paths.push(path);
            }
        }
        paths.sort();

        if paths.is_empty() {
            return Err(CaptureError::InvalidConfig {
                message: format!(
                    "no supported images in {} (expected .png/.jpg/.jpeg)",
                    self.dir.display()
                ),
            });
        }

        Ok(paths)
    }
}

impl CaptureBackend for FixtureBackend {
    fn name(&self) -> &'static str {
        "fixture"
    }

    fn probe(&self) -> CaptureProbe {
        CaptureProbe {
            backend: "fixture",
            available: true,
            message: "directory-based test backend".to_string(),
            details: vec![("dir".to_string(), self.dir.display().to_string())],
        }
    }

    fn start(
        &mut self,
        _options: CaptureOptions,
    ) -> Result<Box<dyn FrameStream>, CaptureError> {
        let paths = self.collect_frames()?;
        Ok(Box::new(FixtureFrameStream {
            paths,
            index: 0,
        }))
    }
}

pub struct FixtureFrameStream {
    paths: Vec<PathBuf>,
    index: usize,
}

impl FrameStream for FixtureFrameStream {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        let path = match self.paths.get(self.index) {
            Some(p) => p.clone(),
            None => return Err(CaptureError::EndOfStream),
        };
        self.index += 1;

        let decoded = image::open(&path).map_err(|err| {
            CaptureError::InvalidConfig {
                message: format!("failed to decode {}: {err}", path.display()),
            }
        })?;
        let image = into_rgba(decoded);

        Ok(CapturedFrame {
            image,
            timestamp: SystemTime::now(),
            metadata: FrameMetadata::fixture(),
        })
    }
}

fn into_rgba(image: DynamicImage) -> image::RgbaImage {
    match image {
        DynamicImage::ImageRgba8(rgba) => rgba,
        other => other.to_rgba8(),
    }
}

#[cfg(test)]
mod tests {
    use super::FixtureBackend;
    use crate::backend::CaptureBackend;
    use crate::error::CaptureError;
    use crate::types::CaptureOptions;
    use image::{Rgba, RgbaImage};
    use std::path::PathBuf;

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "rollshot-fixture-{label}-{nanos}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    fn write_solid(dir: &std::path::Path, name: &str, color: [u8; 4]) {
        let img = RgbaImage::from_pixel(4, 4, Rgba(color));
        img.save(dir.join(name)).expect("save fixture frame");
    }

    #[test]
    fn missing_directory_returns_invalid_config() {
        let mut backend = FixtureBackend::new("/tmp/rollshot-fixture-does-not-exist");
        let err = backend
            .start(CaptureOptions::default())
            .expect_err("missing dir should fail");
        match err {
            CaptureError::InvalidConfig { message } => {
                assert!(message.contains("fixture directory not found"), "msg = {message}");
            }
            other => panic!("expected InvalidConfig, got {other:?}"),
        }
    }

    #[test]
    fn empty_directory_returns_invalid_config() {
        let dir = temp_dir("empty");
        let mut backend = FixtureBackend::new(&dir);
        let err = backend
            .start(CaptureOptions::default())
            .expect_err("empty dir should fail");
        match err {
            CaptureError::InvalidConfig { message } => {
                assert!(message.contains("no supported images"), "msg = {message}");
            }
            other => panic!("expected InvalidConfig, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn frames_returned_in_sorted_order_then_end_of_stream() {
        let dir = temp_dir("sorted");
        write_solid(&dir, "frame_002.png", [0, 0, 255, 255]);
        write_solid(&dir, "frame_000.png", [255, 0, 0, 255]);
        write_solid(&dir, "frame_001.png", [0, 255, 0, 255]);
        // Non-image file should be ignored
        std::fs::write(dir.join("notes.txt"), b"ignore me").expect("write note");

        let mut backend = FixtureBackend::new(&dir);
        let mut stream = backend
            .start(CaptureOptions::default())
            .expect("start fixture backend");

        let first = stream.next_frame().expect("first frame");
        assert_eq!(first.image.get_pixel(0, 0).0, [255, 0, 0, 255]);

        let second = stream.next_frame().expect("second frame");
        assert_eq!(second.image.get_pixel(0, 0).0, [0, 255, 0, 255]);

        let third = stream.next_frame().expect("third frame");
        assert_eq!(third.image.get_pixel(0, 0).0, [0, 0, 255, 255]);

        match stream.next_frame() {
            Err(CaptureError::EndOfStream) => {}
            other => panic!("expected EndOfStream, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn probe_marks_available() {
        let dir = temp_dir("probe");
        let backend = FixtureBackend::new(&dir);
        let probe = backend.probe();
        assert_eq!(probe.backend, "fixture");
        assert!(probe.available);
        assert!(probe.details.iter().any(|(k, _)| k == "dir"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

- [ ] **Step 2: Export the new module**

Edit `crates/rollshot-capture/src/lib.rs`. Add `pub mod fixture;` and extend the re-export line to include `FixtureBackend` and `FixtureFrameStream`. The full file should now read:

```rust
pub mod backend;
pub mod error;
pub mod fake;
pub mod fixture;
pub mod types;

pub use backend::{CaptureBackend, FrameStream};
pub use error::CaptureError;
pub use fake::FakeFrameStream;
pub use fixture::{FixtureBackend, FixtureFrameStream};
pub use types::{
    CaptureOptions, CaptureProbe, CapturedFrame, FrameMetadata, PixelFormat, Region, RegionMode,
    Size,
};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p rollshot-capture --lib fixture::`
Expected: 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-capture/src
git commit -m "feat(capture): add FixtureBackend reading frames from a directory"
```

---

## Task 5: BackendKind + default_backend + factory

**Files:**
- Modify: `crates/rollshot-capture/src/backend.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`

- [ ] **Step 1: Extend `backend.rs` with BackendKind**

Replace the full contents of `crates/rollshot-capture/src/backend.rs` with:

```rust
use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe, CapturedFrame};

pub trait CaptureBackend {
    fn name(&self) -> &'static str;
    fn probe(&self) -> CaptureProbe;
    fn start(
        &mut self,
        options: CaptureOptions,
    ) -> Result<Box<dyn FrameStream>, CaptureError>;
}

pub trait FrameStream: Send {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Fixture,
    LinuxPortalPipeWire,
    MacosScreenCaptureKit,
    Unsupported,
}

impl BackendKind {
    pub fn as_flag(self) -> &'static str {
        match self {
            BackendKind::Fixture => "fixture",
            BackendKind::LinuxPortalPipeWire => "linux-portal",
            BackendKind::MacosScreenCaptureKit => "macos-sck",
            BackendKind::Unsupported => "unsupported",
        }
    }

    pub fn from_cli_flag(flag: &str) -> Result<Self, CaptureError> {
        match flag {
            "auto" => Ok(default_backend()),
            "fixture" => Ok(BackendKind::Fixture),
            "linux-portal" => Ok(BackendKind::LinuxPortalPipeWire),
            "macos-sck" => Ok(BackendKind::MacosScreenCaptureKit),
            other => Err(CaptureError::InvalidConfig {
                message: format!(
                    "unknown backend '{other}'; expected one of: auto, fixture, linux-portal, macos-sck"
                ),
            }),
        }
    }
}

pub fn default_backend() -> BackendKind {
    if cfg!(target_os = "macos") {
        return BackendKind::MacosScreenCaptureKit;
    }
    if cfg!(target_os = "linux") {
        if std::env::var("XDG_SESSION_TYPE").ok().as_deref() == Some("wayland") {
            return BackendKind::LinuxPortalPipeWire;
        }
        return BackendKind::Unsupported;
    }
    BackendKind::Unsupported
}

#[cfg(test)]
mod tests {
    use super::{default_backend, BackendKind};
    use crate::error::CaptureError;

    #[test]
    fn from_cli_flag_accepts_known_backends() {
        assert_eq!(
            BackendKind::from_cli_flag("fixture").unwrap(),
            BackendKind::Fixture
        );
        assert_eq!(
            BackendKind::from_cli_flag("linux-portal").unwrap(),
            BackendKind::LinuxPortalPipeWire
        );
        assert_eq!(
            BackendKind::from_cli_flag("macos-sck").unwrap(),
            BackendKind::MacosScreenCaptureKit
        );
        // auto must resolve to one of the concrete kinds (never Unsupported on a
        // supported host, but the test only asserts it does not return an error)
        BackendKind::from_cli_flag("auto").expect("auto resolves");
    }

    #[test]
    fn from_cli_flag_rejects_unknown() {
        let err = BackendKind::from_cli_flag("nope").expect_err("unknown rejected");
        match err {
            CaptureError::InvalidConfig { message } => {
                assert!(message.contains("nope"), "msg = {message}");
                assert!(message.contains("fixture"), "msg = {message}");
            }
            other => panic!("expected InvalidConfig, got {other:?}"),
        }
    }

    #[test]
    fn as_flag_round_trips() {
        for kind in [
            BackendKind::Fixture,
            BackendKind::LinuxPortalPipeWire,
            BackendKind::MacosScreenCaptureKit,
        ] {
            assert_eq!(
                BackendKind::from_cli_flag(kind.as_flag()).unwrap(),
                kind
            );
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn default_backend_linux_non_wayland_is_unsupported() {
        // Save and clear XDG_SESSION_TYPE for the duration of this test.
        let prev = std::env::var("XDG_SESSION_TYPE").ok();
        std::env::remove_var("XDG_SESSION_TYPE");
        assert_eq!(default_backend(), BackendKind::Unsupported);
        if let Some(v) = prev {
            std::env::set_var("XDG_SESSION_TYPE", v);
        }
    }
}
```

- [ ] **Step 2: Re-export from `lib.rs`**

Edit `crates/rollshot-capture/src/lib.rs`. The `pub use backend::...` line must include `BackendKind` and `default_backend`. The full file now reads:

```rust
pub mod backend;
pub mod error;
pub mod fake;
pub mod fixture;
pub mod types;

pub use backend::{default_backend, BackendKind, CaptureBackend, FrameStream};
pub use error::CaptureError;
pub use fake::FakeFrameStream;
pub use fixture::{FixtureBackend, FixtureFrameStream};
pub use types::{
    CaptureOptions, CaptureProbe, CapturedFrame, FrameMetadata, PixelFormat, Region, RegionMode,
    Size,
};
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p rollshot-capture --lib backend::`
Expected: 3 (or 4 on Linux) tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-capture/src
git commit -m "feat(capture): add BackendKind enum and default_backend"
```

---

## Task 6: LinuxPortalBackend stub

**Files:**
- Create: `crates/rollshot-capture/src/linux/mod.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/rollshot-capture/src/linux/mod.rs`:

```rust
#![cfg(target_os = "linux")]

use crate::backend::{CaptureBackend, FrameStream};
use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe};

pub struct LinuxPortalBackend;

impl LinuxPortalBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LinuxPortalBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptureBackend for LinuxPortalBackend {
    fn name(&self) -> &'static str {
        "linux-portal"
    }

    fn probe(&self) -> CaptureProbe {
        let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();

        let is_wayland = session_type == "wayland";
        let is_kde = desktop.to_ascii_lowercase().contains("kde")
            || desktop.to_ascii_lowercase().contains("plasma");

        let available = is_wayland && is_kde;
        let message = if available {
            "preconditions look ok; backend is not implemented in v0.1 plumbing phase".to_string()
        } else {
            "linux-portal requires a KDE/Plasma Wayland session".to_string()
        };

        CaptureProbe {
            backend: "linux-portal",
            available,
            message,
            details: vec![
                ("XDG_SESSION_TYPE".to_string(), session_type),
                ("XDG_CURRENT_DESKTOP".to_string(), desktop),
            ],
        }
    }

    fn start(
        &mut self,
        _options: CaptureOptions,
    ) -> Result<Box<dyn FrameStream>, CaptureError> {
        Err(CaptureError::NotImplemented {
            backend: "linux-portal",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::LinuxPortalBackend;
    use crate::backend::CaptureBackend;
    use crate::error::CaptureError;
    use crate::types::CaptureOptions;

    #[test]
    fn probe_reports_env_in_details() {
        let backend = LinuxPortalBackend::new();
        let probe = backend.probe();
        assert_eq!(probe.backend, "linux-portal");
        let keys: Vec<&str> = probe.details.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"XDG_SESSION_TYPE"));
        assert!(keys.contains(&"XDG_CURRENT_DESKTOP"));
    }

    #[test]
    fn start_returns_not_implemented() {
        let mut backend = LinuxPortalBackend::new();
        let err = backend
            .start(CaptureOptions::default())
            .expect_err("stub returns NotImplemented");
        match err {
            CaptureError::NotImplemented { backend } => {
                assert_eq!(backend, "linux-portal");
            }
            other => panic!("expected NotImplemented, got {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Wire into `lib.rs`**

Edit `crates/rollshot-capture/src/lib.rs`. Add the cfg-gated module declaration and re-export. Full file:

```rust
pub mod backend;
pub mod error;
pub mod fake;
pub mod fixture;
pub mod types;

#[cfg(target_os = "linux")]
pub mod linux;

pub use backend::{default_backend, BackendKind, CaptureBackend, FrameStream};
pub use error::CaptureError;
pub use fake::FakeFrameStream;
pub use fixture::{FixtureBackend, FixtureFrameStream};
#[cfg(target_os = "linux")]
pub use linux::LinuxPortalBackend;
pub use types::{
    CaptureOptions, CaptureProbe, CapturedFrame, FrameMetadata, PixelFormat, Region, RegionMode,
    Size,
};
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p rollshot-capture --lib linux::`
Expected: 2 tests pass on Linux. On non-Linux hosts this task's tests don't compile (cfg-gated). On those hosts run `cargo test -p rollshot-capture` and expect green.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-capture/src
git commit -m "feat(capture): add LinuxPortalBackend stub with honest probe"
```

---

## Task 7: MacosScreenCaptureKitBackend stub

**Files:**
- Create: `crates/rollshot-capture/src/macos/mod.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`

- [ ] **Step 1: Write the stub**

Create `crates/rollshot-capture/src/macos/mod.rs`:

```rust
#![cfg(target_os = "macos")]

use crate::backend::{CaptureBackend, FrameStream};
use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe};

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
        "macos-sck"
    }

    fn probe(&self) -> CaptureProbe {
        CaptureProbe {
            backend: "macos-sck",
            available: true,
            message: "macOS host detected; backend is not implemented in v0.1 plumbing phase"
                .to_string(),
            details: vec![("os".to_string(), "macos".to_string())],
        }
    }

    fn start(
        &mut self,
        _options: CaptureOptions,
    ) -> Result<Box<dyn FrameStream>, CaptureError> {
        Err(CaptureError::NotImplemented {
            backend: "macos-sck",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::MacosScreenCaptureKitBackend;
    use crate::backend::CaptureBackend;
    use crate::error::CaptureError;
    use crate::types::CaptureOptions;

    #[test]
    fn probe_reports_macos_in_details() {
        let backend = MacosScreenCaptureKitBackend::new();
        let probe = backend.probe();
        assert_eq!(probe.backend, "macos-sck");
        assert!(probe.details.iter().any(|(k, v)| k == "os" && v == "macos"));
    }

    #[test]
    fn start_returns_not_implemented() {
        let mut backend = MacosScreenCaptureKitBackend::new();
        let err = backend
            .start(CaptureOptions::default())
            .expect_err("stub returns NotImplemented");
        match err {
            CaptureError::NotImplemented { backend } => {
                assert_eq!(backend, "macos-sck");
            }
            other => panic!("expected NotImplemented, got {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Wire into `lib.rs`**

Edit `crates/rollshot-capture/src/lib.rs`. Add the macOS cfg block and re-export. Full file:

```rust
pub mod backend;
pub mod error;
pub mod fake;
pub mod fixture;
pub mod types;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;

pub use backend::{default_backend, BackendKind, CaptureBackend, FrameStream};
pub use error::CaptureError;
pub use fake::FakeFrameStream;
pub use fixture::{FixtureBackend, FixtureFrameStream};
#[cfg(target_os = "linux")]
pub use linux::LinuxPortalBackend;
#[cfg(target_os = "macos")]
pub use macos::MacosScreenCaptureKitBackend;
pub use types::{
    CaptureOptions, CaptureProbe, CapturedFrame, FrameMetadata, PixelFormat, Region, RegionMode,
    Size,
};
```

- [ ] **Step 3: Verify the workspace still builds on Linux**

Run: `cargo build --workspace`
Expected: builds (`macos` module is cfg-gated so its tests will only run on macOS hosts).

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-capture/src
git commit -m "feat(capture): add MacosScreenCaptureKitBackend stub"
```

---

## Task 8: BackendKind::create() factory

**Files:**
- Modify: `crates/rollshot-capture/src/backend.rs`

- [ ] **Step 1: Add the failing test**

Append the following to `crates/rollshot-capture/src/backend.rs` (inside the existing `#[cfg(test)] mod tests` block — and adjust imports as needed):

```rust
    #[test]
    fn fixture_kind_create_requires_path() {
        // create() for Fixture needs no path because it is constructed lazily;
        // callers must call FixtureBackend::new directly when a path is known.
        // The factory therefore returns InvalidConfig for Fixture so the CLI
        // is forced to thread the --fixture path explicitly.
        let err = BackendKind::Fixture
            .create()
            .expect_err("fixture cannot be created without a path");
        match err {
            crate::error::CaptureError::InvalidConfig { message } => {
                assert!(message.contains("--fixture"), "msg = {message}");
            }
            other => panic!("expected InvalidConfig, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_kind_create_returns_unsupported() {
        let err = BackendKind::Unsupported
            .create()
            .expect_err("Unsupported maps to Unsupported error");
        assert!(matches!(
            err,
            crate::error::CaptureError::Unsupported { .. }
        ));
    }
```

- [ ] **Step 2: Add the `create()` factory method**

Inside `impl BackendKind { ... }` in `crates/rollshot-capture/src/backend.rs`, add:

```rust
    /// Construct a boxed backend for this kind, where possible.
    ///
    /// `Fixture` returns `InvalidConfig` because callers must supply the
    /// frames directory and should construct `FixtureBackend::new(path)`
    /// directly. The factory exists so CLI code can map non-fixture kinds
    /// (`linux-portal`, `macos-sck`) into a `Box<dyn CaptureBackend>` for
    /// uniform handling without duplicating cfg gates.
    pub fn create(self) -> Result<Box<dyn CaptureBackend>, CaptureError> {
        match self {
            BackendKind::Fixture => Err(CaptureError::InvalidConfig {
                message: "fixture backend requires --fixture <DIR>".to_string(),
            }),
            BackendKind::LinuxPortalPipeWire => {
                #[cfg(target_os = "linux")]
                {
                    Ok(Box::new(crate::linux::LinuxPortalBackend::new()))
                }
                #[cfg(not(target_os = "linux"))]
                {
                    Err(CaptureError::Unsupported {
                        message: "linux-portal backend requires a Linux host".to_string(),
                    })
                }
            }
            BackendKind::MacosScreenCaptureKit => {
                #[cfg(target_os = "macos")]
                {
                    Ok(Box::new(crate::macos::MacosScreenCaptureKitBackend::new()))
                }
                #[cfg(not(target_os = "macos"))]
                {
                    Err(CaptureError::Unsupported {
                        message: "macos-sck backend requires a macOS host".to_string(),
                    })
                }
            }
            BackendKind::Unsupported => Err(CaptureError::Unsupported {
                message: format!(
                    "no capture backend is available on os={} session={}",
                    std::env::consts::OS,
                    std::env::var("XDG_SESSION_TYPE").unwrap_or_default()
                ),
            }),
        }
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p rollshot-capture --lib backend::`
Expected: all `backend::tests::*` tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-capture/src/backend.rs
git commit -m "feat(capture): add BackendKind::create factory"
```

---

## Task 9: CliError + clap subcommand restructure

This task replaces the hand-rolled `run()` argument parser with `clap` derive, splits commands into modules, and introduces a `CliError` that carries an exit code. The probe / capture subcommand bodies remain almost empty here — they are filled in by later tasks. `stitch-folder` is extracted intact.

**Files:**
- Modify: `crates/rollshot-cli/Cargo.toml`
- Modify: `crates/rollshot-cli/src/main.rs`
- Modify: `crates/rollshot-cli/src/lib.rs`
- Create: `crates/rollshot-cli/src/cli_error.rs`
- Create: `crates/rollshot-cli/src/args.rs`
- Create: `crates/rollshot-cli/src/cmd_capture.rs`
- Create: `crates/rollshot-cli/src/cmd_probe.rs`
- Create: `crates/rollshot-cli/src/cmd_stitch_folder.rs`

- [ ] **Step 1: Update `crates/rollshot-cli/Cargo.toml`**

Replace the `[dependencies]` block with:

```toml
[dependencies]
anyhow = { workspace = true }
clap = { workspace = true }
image = { workspace = true }
rollshot-capture = { path = "../rollshot-capture" }
rollshot-core = { path = "../rollshot-core" }
serde = { workspace = true }
serde_json = { workspace = true }
```

- [ ] **Step 2: Create `crates/rollshot-cli/src/cli_error.rs`**

```rust
use std::fmt;

use rollshot_capture::CaptureError;

#[derive(Debug)]
pub struct CliError {
    pub message: String,
    pub exit_code: u8,
}

impl CliError {
    pub fn new(message: impl Into<String>, exit_code: u8) -> Self {
        Self {
            message: message.into(),
            exit_code,
        }
    }

    pub fn from_capture(err: CaptureError) -> Self {
        match err {
            CaptureError::NotImplemented { backend } => CliError::new(
                format!(
                    "backend not implemented yet: {backend}\nhint: use --backend fixture for offline runs"
                ),
                2,
            ),
            CaptureError::PermissionDenied { message } => {
                CliError::new(format!("permission denied: {message}"), 3)
            }
            CaptureError::Unsupported { message } => {
                CliError::new(format!("unsupported: {message}"), 4)
            }
            CaptureError::UserCancelled => CliError::new("user cancelled", 0),
            CaptureError::EndOfStream => {
                CliError::new("frame stream ended before any frame was captured", 1)
            }
            CaptureError::InvalidConfig { message } => {
                CliError::new(format!("invalid configuration: {message}"), 1)
            }
            CaptureError::Backend(err) => CliError::new(format!("{err:#}"), 1),
        }
    }

    pub fn from_anyhow(err: anyhow::Error) -> Self {
        CliError::new(format!("{err:#}"), 1)
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<CaptureError> for CliError {
    fn from(err: CaptureError) -> Self {
        CliError::from_capture(err)
    }
}

impl From<anyhow::Error> for CliError {
    fn from(err: anyhow::Error) -> Self {
        CliError::from_anyhow(err)
    }
}
```

- [ ] **Step 3: Create `crates/rollshot-cli/src/args.rs`**

```rust
use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "rollshot", version, about = "rollshot: scrollshot stitcher")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Capture frames from a backend and stitch them into a long PNG.
    Capture(CaptureArgs),

    /// Print diagnostics about the host and known capture backends.
    Probe(ProbeArgs),

    /// Stitch a directory of pre-recorded frames without going through a
    /// capture backend. Useful for iterating on the matcher.
    StitchFolder(StitchFolderArgs),
}

#[derive(Debug, clap::Args)]
pub struct CaptureArgs {
    /// Which capture backend to use.
    #[arg(
        long,
        default_value = "auto",
        value_parser = ["auto", "fixture", "linux-portal", "macos-sck"],
    )]
    pub backend: String,

    /// Region selection mode. Accepts `auto`, `portal`, `full`, or `"X,Y WxH"`.
    #[arg(long, default_value = "auto")]
    pub region: String,

    /// Output PNG path.
    #[arg(long)]
    pub output: PathBuf,

    /// Directory of pre-recorded frames; required with --backend fixture.
    #[arg(long)]
    pub fixture: Option<PathBuf>,

    /// Optional directory where every captured frame is written as PNG.
    #[arg(long)]
    pub dump_frames: Option<PathBuf>,

    /// Maximum number of frames to read before stopping.
    #[arg(long, default_value_t = 200)]
    pub max_frames: u32,

    /// Capture frame rate (used by real backends; ignored by fixture).
    #[arg(long, default_value_t = 5)]
    pub fps: u32,

    /// Include the cursor in captured frames.
    #[arg(long, default_value_t = false)]
    pub show_cursor: bool,
}

#[derive(Debug, clap::Args)]
pub struct ProbeArgs {
    /// Emit JSON instead of human-readable text.
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, clap::Args)]
pub struct StitchFolderArgs {
    /// Directory of frames to stitch.
    pub frames_dir: PathBuf,

    /// Output PNG path.
    #[arg(long, short)]
    pub output: PathBuf,
}
```

- [ ] **Step 4: Extract `stitch-folder` to `cmd_stitch_folder.rs`**

Create `crates/rollshot-cli/src/cmd_stitch_folder.rs`:

```rust
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use image::{DynamicImage, ImageFormat};
use rollshot_core::{StitchConfig, StitchOutcome, Stitcher};

use crate::args::StitchFolderArgs;
use crate::cli_error::CliError;

pub fn run(args: &StitchFolderArgs) -> Result<String, CliError> {
    let frames_dir = &args.frames_dir;
    let output = &args.output;

    if !frames_dir.is_dir() {
        return Err(CliError::new(
            format!("frames directory not found: {}", frames_dir.display()),
            1,
        ));
    }

    let frame_paths = collect_frame_paths(frames_dir)?;
    if frame_paths.is_empty() {
        return Err(CliError::new(
            format!(
                "no supported images in {} (expected .png/.jpg/.jpeg)",
                frames_dir.display()
            ),
            1,
        ));
    }

    let mut stitcher = Stitcher::new(StitchConfig::default());
    let mut appended = 0u32;
    let mut duplicates = 0u32;
    let mut no_match = 0u32;
    let mut no_progress = 0u32;

    for path in &frame_paths {
        let img = image::open(path).map_err(|err| {
            CliError::new(format!("failed to decode {}: {err}", path.display()), 1)
        })?;
        let frame = into_rgba(img);

        match stitcher.push_frame(frame) {
            StitchOutcome::FirstFrame => {}
            StitchOutcome::Appended { .. } => appended += 1,
            StitchOutcome::Duplicate => duplicates += 1,
            StitchOutcome::NoMatch { .. } => no_match += 1,
            StitchOutcome::NoProgress => no_progress += 1,
        }
    }

    let stitched = stitcher
        .full_image()
        .ok_or_else(|| CliError::new("no stitched output available", 1))?;
    stitched
        .save_with_format(output, ImageFormat::Png)
        .map_err(|err| CliError::new(format!("failed to save {}: {err}", output.display()), 1))?;

    Ok(format!(
        "stitch-folder: {dir}\n\
         input frames: {input}\n\
         appended: {appended}\n\
         duplicates: {duplicates}\n\
         no-progress: {no_progress}\n\
         no-match: {no_match}\n\
         output: {out} ({w}x{h})\n",
        dir = frames_dir.display(),
        input = frame_paths.len(),
        appended = appended,
        duplicates = duplicates,
        no_progress = no_progress,
        no_match = no_match,
        out = output.display(),
        w = stitched.width(),
        h = stitched.height(),
    ))
}

fn collect_frame_paths(dir: &Path) -> Result<Vec<PathBuf>, CliError> {
    let entries = fs::read_dir(dir)
        .map_err(|err| CliError::new(format!("failed to read {}: {err}", dir.display()), 1))?;

    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| {
            CliError::new(format!("failed to read entry in {}: {err}", dir.display()), 1)
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|err| {
            CliError::new(format!("failed to inspect {}: {err}", path.display()), 1)
        })?;
        if !file_type.is_file() {
            continue;
        }
        if matches!(
            path.extension()
                .and_then(OsStr::to_str)
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("png" | "jpg" | "jpeg")
        ) {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn into_rgba(image: DynamicImage) -> image::RgbaImage {
    match image {
        DynamicImage::ImageRgba8(rgba) => rgba,
        other => other.to_rgba8(),
    }
}
```

- [ ] **Step 5: Create empty command shells**

Create `crates/rollshot-cli/src/cmd_capture.rs`:

```rust
use crate::args::CaptureArgs;
use crate::cli_error::CliError;

pub fn run(_args: &CaptureArgs) -> Result<String, CliError> {
    Err(CliError::new(
        "rollshot capture is not implemented yet (filled in by later tasks)",
        1,
    ))
}
```

Create `crates/rollshot-cli/src/cmd_probe.rs`:

```rust
use crate::args::ProbeArgs;
use crate::cli_error::CliError;

pub fn run(_args: &ProbeArgs) -> Result<String, CliError> {
    Ok("probe: not implemented yet\n".to_string())
}
```

- [ ] **Step 6: Replace `crates/rollshot-cli/src/lib.rs`**

Replace its full contents with:

```rust
pub mod args;
pub mod cli_error;
pub mod cmd_capture;
pub mod cmd_probe;
pub mod cmd_stitch_folder;

use clap::Parser;

pub use cli_error::CliError;

use crate::args::{Cli, Command};

pub fn run<I, S>(args: I) -> Result<String, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::try_parse_from(args).map_err(|err| {
        // clap formats its own messages including --help and --version; we
        // surface them verbatim and let main.rs print and exit 0 for those.
        // Real argument errors map to exit 1 — exit 2 is reserved by the spec
        // for backend NotImplemented errors.
        let exit_code = if err.use_stderr() { 1 } else { 0 };
        CliError::new(err.to_string(), exit_code)
    })?;

    match &cli.command {
        Command::Capture(a) => cmd_capture::run(a),
        Command::Probe(a) => cmd_probe::run(a),
        Command::StitchFolder(a) => cmd_stitch_folder::run(a),
    }
}
```

- [ ] **Step 7: Update `crates/rollshot-cli/src/main.rs`**

Replace its full contents with:

```rust
use std::process::ExitCode;

fn main() -> ExitCode {
    match rollshot_cli::run(std::env::args_os()) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            if error.exit_code == 0 {
                // clap --help/--version path prints to stdout
                print!("{}", error.message);
                ExitCode::SUCCESS
            } else {
                eprintln!("{}", error.message);
                ExitCode::from(error.exit_code)
            }
        }
    }
}
```

- [ ] **Step 8: Update the existing inline tests that lived in `lib.rs`**

The old `mod tests` block inside `lib.rs` is gone. Move/rewrite the smoke-style tests into `crates/rollshot-cli/tests/cli_smoke.rs` later if you want; for this task only fix the `stitch-folder` integration test in `tests/cli_smoke.rs`. Edit it to drop the "real capture: unavailable" assertion from the probe test:

```rust
#[test]
fn rollshot_probe_binary_runs() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("probe")
        .output()
        .expect("run rollshot probe");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("probe"), "stdout = {stdout}");
}
```

Leave the `rollshot_stitch_folder_writes_png` test untouched.

- [ ] **Step 9: Run tests**

Run: `cargo build --workspace`
Expected: clean build.

Run: `cargo test --workspace`
Expected: all tests pass. `rollshot --help`, `--version`, `probe`, and `stitch-folder` all flow through `clap` now.

Smoke-check the binary:

```bash
cargo run -p rollshot-cli -- --help
cargo run -p rollshot-cli -- stitch-folder --help
```

Expected: clap-generated help text for each.

- [ ] **Step 10: Commit**

```bash
git add crates/rollshot-cli
git commit -m "refactor(cli): adopt clap subcommands and CliError"
```

---

## Task 10: rollshot probe (text + JSON)

**Files:**
- Modify: `crates/rollshot-cli/src/cmd_probe.rs`
- Create: `crates/rollshot-cli/tests/probe_cli.rs`

- [ ] **Step 1: Write the failing integration tests**

Create `crates/rollshot-cli/tests/probe_cli.rs`:

```rust
use std::process::Command;

#[test]
fn probe_text_includes_os_and_default_backend() {
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("probe")
        .output()
        .expect("run rollshot probe");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("os:"), "stdout = {stdout}");
    assert!(stdout.contains("default backend:"), "stdout = {stdout}");
    assert!(stdout.contains("fixture"), "stdout = {stdout}");
}

#[test]
fn probe_json_parses_and_has_expected_shape() {
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("probe")
        .arg("--json")
        .output()
        .expect("run rollshot probe --json");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("probe --json must be valid json");
    assert!(parsed.get("os").is_some(), "missing os: {stdout}");
    assert!(
        parsed.get("default_backend").is_some(),
        "missing default_backend: {stdout}"
    );
    let backends = parsed
        .get("backends")
        .and_then(|v| v.as_array())
        .expect("backends array");
    let names: Vec<&str> = backends
        .iter()
        .filter_map(|b| b.get("name").and_then(|v| v.as_str()))
        .collect();
    assert!(names.contains(&"fixture"), "names = {names:?}");
}
```

- [ ] **Step 2: Run the tests, expect them to fail**

Run: `cargo test -p rollshot-cli --test probe_cli`
Expected: both fail — current `cmd_probe::run` returns the placeholder string.

- [ ] **Step 3: Implement `cmd_probe::run`**

Replace `crates/rollshot-cli/src/cmd_probe.rs` with:

```rust
use std::fmt::Write;

use rollshot_capture::{default_backend, BackendKind, CaptureBackend, CaptureProbe};
use serde::Serialize;

use crate::args::ProbeArgs;
use crate::cli_error::CliError;

pub fn run(args: &ProbeArgs) -> Result<String, CliError> {
    let report = build_report();
    if args.json {
        serde_json::to_string_pretty(&report)
            .map(|mut s| {
                s.push('\n');
                s
            })
            .map_err(|err| CliError::new(format!("failed to render probe json: {err}"), 1))
    } else {
        Ok(render_text(&report))
    }
}

#[derive(Debug, Serialize)]
struct ProbeReport {
    os: &'static str,
    session_type: String,
    desktop: String,
    default_backend: &'static str,
    backends: Vec<ProbeEntry>,
}

#[derive(Debug, Serialize)]
struct ProbeEntry {
    name: &'static str,
    available: bool,
    message: String,
    details: Vec<(String, String)>,
}

impl From<CaptureProbe> for ProbeEntry {
    fn from(p: CaptureProbe) -> Self {
        ProbeEntry {
            name: p.backend,
            available: p.available,
            message: p.message,
            details: p.details,
        }
    }
}

fn build_report() -> ProbeReport {
    let os = std::env::consts::OS;
    let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let default = default_backend();

    let mut backends: Vec<ProbeEntry> = Vec::new();

    // Always advertise fixture.
    backends.push(ProbeEntry {
        name: "fixture",
        available: true,
        message: "directory-based test backend".to_string(),
        details: Vec::new(),
    });

    #[cfg(target_os = "linux")]
    {
        let backend = rollshot_capture::LinuxPortalBackend::new();
        backends.push(backend.probe().into());
    }
    #[cfg(target_os = "macos")]
    {
        let backend = rollshot_capture::MacosScreenCaptureKitBackend::new();
        backends.push(backend.probe().into());
    }

    ProbeReport {
        os,
        session_type,
        desktop,
        default_backend: default.as_flag(),
        backends,
    }
}

fn render_text(report: &ProbeReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "rollshot probe");
    let _ = writeln!(out, "  os: {}", report.os);
    let _ = writeln!(out, "  session_type: {}", report.session_type);
    let _ = writeln!(out, "  desktop: {}", report.desktop);
    let _ = writeln!(out, "  default backend: {}", report.default_backend);
    let _ = writeln!(out, "  backends:");
    for b in &report.backends {
        let status = if b.available { "available" } else { "unavailable" };
        let _ = writeln!(out, "    - {} ({status}): {}", b.name, b.message);
        for (k, v) in &b.details {
            let _ = writeln!(out, "        {k}: {v}");
        }
    }
    out
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p rollshot-cli --test probe_cli`
Expected: both tests pass.

Run: `cargo test --workspace`
Expected: all tests pass (the `cli_smoke.rs::rollshot_probe_binary_runs` test should also still pass; its assertion only checks for the string "probe").

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-cli
git commit -m "feat(cli): implement rollshot probe with text and json output"
```

---

## Task 11: rollshot capture (fixture flow)

This is the first version of `capture`. It supports only `--backend fixture --fixture <dir> --output <png>` and stops at end-of-stream. Later tasks add `--dump-frames`, `--max-frames`, `--region`, and stub error mapping.

**Files:**
- Modify: `crates/rollshot-cli/src/cmd_capture.rs`
- Create: `crates/rollshot-cli/tests/capture_fixture.rs`

- [ ] **Step 1: Write the failing integration test**

Create `crates/rollshot-cli/tests/capture_fixture.rs`:

```rust
use std::path::PathBuf;
use std::process::Command;

use image::{imageops, Rgba, RgbaImage};

fn temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "rollshot-capture-{label}-{nanos}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).expect("create temp dir");
    path
}

fn make_scroll_canvas(width: u32, height: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(width, height, Rgba([245, 245, 245, 255]));
    for y in (0..height).step_by(36) {
        let accent = ((y / 3) % 180) as u8;
        for x in 24..width.saturating_sub(24) {
            let stripe = if (x / 7 + y / 11) % 2 == 0 { 220 } else { 180 };
            img.put_pixel(x, y, Rgba([accent, stripe, 80, 255]));
            if y + 1 < height {
                img.put_pixel(x, y + 1, Rgba([30, 30, 30, 255]));
            }
        }
    }
    for col in [21u32, 47, 73, 99, 125] {
        if col >= width {
            continue;
        }
        for y in 12..height.saturating_sub(12) {
            if (y / 13) % 3 != 0 {
                img.put_pixel(col, y, Rgba([20, 20, 20, 255]));
            }
        }
    }
    img
}

fn write_scroll_fixture(dir: &std::path::Path) {
    let canvas = make_scroll_canvas(160, 600);
    for (idx, y) in [0u32, 40, 80, 120].iter().enumerate() {
        let frame = imageops::crop_imm(&canvas, 0, *y, canvas.width(), 160).to_image();
        frame
            .save(dir.join(format!("frame_{idx:03}.png")))
            .expect("save frame");
    }
}

#[test]
fn rollshot_capture_fixture_writes_png() {
    let tempdir = temp_dir("fixture-flow");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .output()
        .expect("run rollshot capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("captured 4 frames"), "stdout = {stdout}");
    assert!(stdout.contains("appended"), "stdout = {stdout}");
    assert!(stdout.contains(output_png.to_string_lossy().as_ref()));

    let bytes = std::fs::read(&output_png).expect("read stitched png");
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    let stitched = image::load_from_memory(&bytes)
        .expect("decode stitched png")
        .to_rgba8();
    assert_eq!(stitched.width(), 160);
    assert!(stitched.height() > 160, "height = {}", stitched.height());

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_capture_fixture_requires_fixture_path() {
    let tempdir = temp_dir("missing-fixture-path");
    let output_png = tempdir.join("out.png");

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "fixture"])
        .args(["--output"])
        .arg(&output_png)
        .output()
        .expect("run rollshot capture");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--fixture"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}
```

- [ ] **Step 2: Run the tests, expect them to fail**

Run: `cargo test -p rollshot-cli --test capture_fixture`
Expected: both fail because `cmd_capture::run` still returns the placeholder.

- [ ] **Step 3: Implement `cmd_capture::run` (fixture-only for now)**

Replace `crates/rollshot-cli/src/cmd_capture.rs` with:

```rust
use std::path::Path;

use image::ImageFormat;
use rollshot_capture::{
    BackendKind, CaptureBackend, CaptureError, CaptureOptions, FixtureBackend, RegionMode,
};
use rollshot_core::{StitchConfig, StitchOutcome, Stitcher};

use crate::args::CaptureArgs;
use crate::cli_error::CliError;

pub fn run(args: &CaptureArgs) -> Result<String, CliError> {
    let kind = BackendKind::from_cli_flag(&args.backend).map_err(CliError::from_capture)?;
    let mut backend = build_backend(kind, args)?;
    let options = CaptureOptions {
        region: RegionMode::FullSource,
        fps: args.fps,
        show_cursor: args.show_cursor,
        prefer_portal_region: true,
    };

    let mut stream = backend.start(options).map_err(CliError::from_capture)?;

    let mut stitcher = Stitcher::new(StitchConfig::default());
    let mut captured: u32 = 0;
    let mut appended: u32 = 0;
    let mut duplicates: u32 = 0;
    let mut no_match: u32 = 0;
    let mut no_progress: u32 = 0;

    loop {
        match stream.next_frame() {
            Ok(frame) => {
                captured += 1;
                match stitcher.push_frame(frame.image) {
                    StitchOutcome::FirstFrame => {}
                    StitchOutcome::Appended { .. } => appended += 1,
                    StitchOutcome::Duplicate => duplicates += 1,
                    StitchOutcome::NoMatch { .. } => no_match += 1,
                    StitchOutcome::NoProgress => no_progress += 1,
                }
            }
            Err(CaptureError::EndOfStream) => break,
            Err(err) => return Err(CliError::from_capture(err)),
        }
    }

    let stitched = stitcher
        .full_image()
        .ok_or_else(|| CliError::new("no frames produced an output image", 1))?;
    save_png(&stitched, &args.output)?;

    Ok(format!(
        "captured {captured} frames, appended {appended} (duplicates {duplicates}, no-progress {no_progress}, no-match {no_match})\noutput: {out} ({w}x{h})\n",
        out = args.output.display(),
        w = stitched.width(),
        h = stitched.height(),
    ))
}

fn build_backend(
    kind: BackendKind,
    args: &CaptureArgs,
) -> Result<Box<dyn CaptureBackend>, CliError> {
    match kind {
        BackendKind::Fixture => {
            let dir = args.fixture.as_ref().ok_or_else(|| {
                CliError::new("--backend fixture requires --fixture <DIR>", 1)
            })?;
            Ok(Box::new(FixtureBackend::new(dir.clone())))
        }
        other => other.create().map_err(CliError::from_capture),
    }
}

fn save_png(image: &image::RgbaImage, path: &Path) -> Result<(), CliError> {
    image
        .save_with_format(path, ImageFormat::Png)
        .map_err(|err| CliError::new(format!("failed to save {}: {err}", path.display()), 1))
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p rollshot-cli --test capture_fixture`
Expected: both tests pass.

Run: `cargo test --workspace`
Expected: green.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-cli
git commit -m "feat(cli): rollshot capture --backend fixture stitches to PNG"
```

---

## Task 12: --dump-frames

**Files:**
- Modify: `crates/rollshot-cli/src/cmd_capture.rs`
- Modify: `crates/rollshot-cli/tests/capture_fixture.rs`

- [ ] **Step 1: Add the failing test**

Append to `crates/rollshot-cli/tests/capture_fixture.rs`:

```rust
#[test]
fn rollshot_capture_dump_frames_writes_each_frame() {
    let tempdir = temp_dir("dump-frames");
    let frames_dir = tempdir.join("frames");
    let dump_dir = tempdir.join("dump");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    std::fs::create_dir_all(&dump_dir).expect("create dump dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .args(["--dump-frames"])
        .arg(&dump_dir)
        .output()
        .expect("run rollshot capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut dumped: Vec<_> = std::fs::read_dir(&dump_dir)
        .expect("read dump dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    dumped.sort();
    assert_eq!(dumped.len(), 4, "dumped = {dumped:?}");
    for (idx, path) in dumped.iter().enumerate() {
        let expected = format!("frame_{:04}.png", idx);
        assert!(
            path.file_name().unwrap().to_string_lossy().contains(&expected),
            "file {} should match {expected}",
            path.display()
        );
    }

    let _ = std::fs::remove_dir_all(&tempdir);
}
```

- [ ] **Step 2: Run, expect failure**

Run: `cargo test -p rollshot-cli --test capture_fixture rollshot_capture_dump_frames_writes_each_frame`
Expected: fail — capture currently ignores `--dump-frames`.

- [ ] **Step 3: Honor `--dump-frames` in `cmd_capture::run`**

In `crates/rollshot-cli/src/cmd_capture.rs`, modify the inner loop. Replace the `Ok(frame) => { captured += 1; match stitcher.push_frame(frame.image) ...}` block with:

```rust
            Ok(frame) => {
                if let Some(dir) = args.dump_frames.as_ref() {
                    write_dump_frame(dir, captured, &frame.image)?;
                }
                captured += 1;
                match stitcher.push_frame(frame.image) {
                    StitchOutcome::FirstFrame => {}
                    StitchOutcome::Appended { .. } => appended += 1,
                    StitchOutcome::Duplicate => duplicates += 1,
                    StitchOutcome::NoMatch { .. } => no_match += 1,
                    StitchOutcome::NoProgress => no_progress += 1,
                }
            }
```

Add this helper at the bottom of the file:

```rust
fn write_dump_frame(dir: &Path, index: u32, image: &image::RgbaImage) -> Result<(), CliError> {
    if !dir.is_dir() {
        std::fs::create_dir_all(dir).map_err(|err| {
            CliError::new(format!("failed to create dump dir {}: {err}", dir.display()), 1)
        })?;
    }
    let path = dir.join(format!("frame_{index:04}.png"));
    image
        .save_with_format(&path, ImageFormat::Png)
        .map_err(|err| CliError::new(format!("failed to save {}: {err}", path.display()), 1))?;
    Ok(())
}
```

Note: `write_dump_frame` is called with `captured` *before* the increment, so the first dumped file is `frame_0000.png`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p rollshot-cli --test capture_fixture`
Expected: all three tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-cli
git commit -m "feat(cli): rollshot capture --dump-frames writes per-frame PNGs"
```

---

## Task 13: --max-frames

**Files:**
- Modify: `crates/rollshot-cli/src/cmd_capture.rs`
- Modify: `crates/rollshot-cli/tests/capture_fixture.rs`

- [ ] **Step 1: Add the failing test**

Append to `crates/rollshot-cli/tests/capture_fixture.rs`:

```rust
#[test]
fn rollshot_capture_respects_max_frames() {
    let tempdir = temp_dir("max-frames");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let dump_dir = tempdir.join("dump");
    std::fs::create_dir_all(&dump_dir).expect("create dump dir");

    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .args(["--dump-frames"])
        .arg(&dump_dir)
        .args(["--max-frames", "2"])
        .output()
        .expect("run rollshot capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let dumped = std::fs::read_dir(&dump_dir).expect("read dump dir").count();
    assert_eq!(dumped, 2, "expected exactly 2 dumped frames");

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("captured 2 frames"), "stdout = {stdout}");

    let _ = std::fs::remove_dir_all(&tempdir);
}
```

- [ ] **Step 2: Run, expect failure**

Run: `cargo test -p rollshot-cli --test capture_fixture rollshot_capture_respects_max_frames`
Expected: fail — loop currently does not stop after N frames.

- [ ] **Step 3: Honor `--max-frames`**

In `crates/rollshot-cli/src/cmd_capture.rs`, change the loop so that after incrementing `captured`, we break if we reached `args.max_frames`. The full loop now reads:

```rust
    loop {
        match stream.next_frame() {
            Ok(frame) => {
                if let Some(dir) = args.dump_frames.as_ref() {
                    write_dump_frame(dir, captured, &frame.image)?;
                }
                captured += 1;
                match stitcher.push_frame(frame.image) {
                    StitchOutcome::FirstFrame => {}
                    StitchOutcome::Appended { .. } => appended += 1,
                    StitchOutcome::Duplicate => duplicates += 1,
                    StitchOutcome::NoMatch { .. } => no_match += 1,
                    StitchOutcome::NoProgress => no_progress += 1,
                }
                if captured >= args.max_frames {
                    break;
                }
            }
            Err(CaptureError::EndOfStream) => break,
            Err(err) => return Err(CliError::from_capture(err)),
        }
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p rollshot-cli --test capture_fixture`
Expected: all four tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-cli
git commit -m "feat(cli): rollshot capture --max-frames stops the loop early"
```

---

## Task 14: --region parsing

The fixture backend ignores `--region`, but the CLI still has to parse it because real backends will need it. This task adds the parser and unit-tests it. Manual region strings are accepted; invalid strings produce `InvalidConfig` exit 1.

**Files:**
- Modify: `crates/rollshot-cli/src/cmd_capture.rs`
- Modify: `crates/rollshot-cli/tests/capture_fixture.rs`

- [ ] **Step 1: Add the failing tests**

Append to `crates/rollshot-cli/tests/capture_fixture.rs`:

```rust
#[test]
fn rollshot_capture_accepts_manual_region_string() {
    let tempdir = temp_dir("region-manual");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .args(["--region", "10,20 100x200"])
        .output()
        .expect("run rollshot capture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
fn rollshot_capture_rejects_garbage_region() {
    let tempdir = temp_dir("region-garbage");
    let frames_dir = tempdir.join("frames");
    std::fs::create_dir_all(&frames_dir).expect("create frames dir");
    write_scroll_fixture(&frames_dir);

    let output_png = tempdir.join("stitched.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "fixture"])
        .args(["--fixture"])
        .arg(&frames_dir)
        .args(["--output"])
        .arg(&output_png)
        .args(["--region", "totally bogus"])
        .output()
        .expect("run rollshot capture");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("region"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}
```

- [ ] **Step 2: Run, expect failure**

Run: `cargo test -p rollshot-cli --test capture_fixture`
Expected: the two new tests fail. The current `cmd_capture::run` ignores `args.region` entirely, so `"totally bogus"` is accepted as well.

- [ ] **Step 3: Implement region parsing**

In `crates/rollshot-cli/src/cmd_capture.rs`, replace the line that builds `CaptureOptions` so the region is parsed from `args.region` and `args.backend`. The full new `run` body (after the `build_backend` call) is:

```rust
    let region = parse_region(&args.region, kind)?;
    let options = CaptureOptions {
        region,
        fps: args.fps,
        show_cursor: args.show_cursor,
        prefer_portal_region: true,
    };
```

Extend the top-of-file `use rollshot_capture::{...}` line so it now imports `Region` as well:

```rust
use rollshot_capture::{
    BackendKind, CaptureBackend, CaptureError, CaptureOptions, FixtureBackend, Region, RegionMode,
};
```

Then add this helper near the bottom of the file:

```rust
fn parse_region(flag: &str, kind: BackendKind) -> Result<RegionMode, CliError> {
    match flag {
        "auto" => Ok(match kind {
            BackendKind::LinuxPortalPipeWire => RegionMode::PortalPicker,
            BackendKind::MacosScreenCaptureKit
            | BackendKind::Fixture
            | BackendKind::Unsupported => RegionMode::FullSource,
        }),
        "portal" => Ok(RegionMode::PortalPicker),
        "full" => Ok(RegionMode::FullSource),
        other => parse_manual_region(other).map(RegionMode::Manual),
    }
}

fn parse_manual_region(s: &str) -> Result<Region, CliError> {
    // Expected shape: "X,Y WxH" e.g. "10,20 100x200"
    let invalid = || {
        CliError::new(
            format!("invalid --region '{s}'; expected auto|portal|full|\"X,Y WxH\""),
            1,
        )
    };

    let mut parts = s.split_whitespace();
    let origin = parts.next().ok_or_else(invalid)?;
    let size = parts.next().ok_or_else(invalid)?;
    if parts.next().is_some() {
        return Err(invalid());
    }

    let (x, y) = origin.split_once(',').ok_or_else(invalid)?;
    let (w, h) = size.split_once('x').ok_or_else(invalid)?;
    let x: i32 = x.parse().map_err(|_| invalid())?;
    let y: i32 = y.parse().map_err(|_| invalid())?;
    let width: u32 = w.parse().map_err(|_| invalid())?;
    let height: u32 = h.parse().map_err(|_| invalid())?;
    if width == 0 || height == 0 {
        return Err(invalid());
    }
    Ok(Region { x, y, width, height })
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p rollshot-cli --test capture_fixture`
Expected: all six tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-cli
git commit -m "feat(cli): parse --region into RegionMode (auto/portal/full/manual)"
```

---

## Task 15: Stub backend exit codes

This task verifies the spec's exit-code contract for non-implemented platform backends. On Linux, `--backend linux-portal` must exit 2 with a NotImplemented message; on macOS, `--backend macos-sck` must do the same. We also check that requesting a host-mismatched backend (e.g. `--backend macos-sck` on Linux) exits 4 with an Unsupported message.

**Files:**
- Create: `crates/rollshot-cli/tests/capture_stubs.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/rollshot-cli/tests/capture_stubs.rs`:

```rust
use std::path::PathBuf;
use std::process::Command;

fn temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "rollshot-stub-{label}-{nanos}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).expect("create temp dir");
    path
}

#[test]
#[cfg(target_os = "linux")]
fn linux_portal_backend_exits_with_not_implemented_code() {
    let tempdir = temp_dir("linux-portal");
    let out = tempdir.join("out.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "linux-portal"])
        .args(["--output"])
        .arg(&out)
        .output()
        .expect("run rollshot capture");

    assert_eq!(output.status.code(), Some(2), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not implemented"), "stderr = {stderr}");
    assert!(stderr.contains("linux-portal"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}

#[test]
#[cfg(target_os = "linux")]
fn macos_sck_backend_on_linux_exits_with_unsupported_code() {
    let tempdir = temp_dir("macos-on-linux");
    let out = tempdir.join("out.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rollshot"))
        .arg("capture")
        .args(["--backend", "macos-sck"])
        .args(["--output"])
        .arg(&out)
        .output()
        .expect("run rollshot capture");

    assert_eq!(output.status.code(), Some(4), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unsupported"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}

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

    assert_eq!(output.status.code(), Some(2), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not implemented"), "stderr = {stderr}");
    assert!(stderr.contains("macos-sck"), "stderr = {stderr}");

    let _ = std::fs::remove_dir_all(&tempdir);
}
```

- [ ] **Step 2: Run the tests, expect them to pass without code changes**

Run (on Linux): `cargo test -p rollshot-cli --test capture_stubs`
Expected: both Linux-gated tests pass. The capture path already calls `BackendKind::from_cli_flag → build_backend → backend.start`, which returns the right `CaptureError`, which `CliError::from_capture` maps to exit codes 2 and 4.

If a test fails, the issue is almost certainly in the exit-code mapping inside `CliError::from_capture` (Task 9). Read the stderr from the failing case and adjust the variant mapping to match the spec.

- [ ] **Step 3: Run the full workspace test suite**

Run: `cargo fmt --all -- --check`
Run: `cargo clippy --workspace --all-targets -- -D warnings`
Run: `cargo test --workspace`
Expected: all green.

- [ ] **Step 4: Commit**

```bash
git add crates/rollshot-cli/tests/capture_stubs.rs
git commit -m "test(cli): pin exit codes for not-implemented and unsupported backends"
```

---

## Final Verification

- [ ] **Step 1: Workspace fmt**

Run: `cargo fmt --all -- --check`
Expected: no diff.

- [ ] **Step 2: Workspace clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 3: Workspace tests**

Run: `cargo test --workspace`
Expected: all tests pass.

- [ ] **Step 4: Manual CLI smoke**

```bash
cargo run -p rollshot-cli -- --help
cargo run -p rollshot-cli -- probe
cargo run -p rollshot-cli -- probe --json | python3 -m json.tool
cargo run -p rollshot-cli -- capture --help
```

Expected: clap help text; probe outputs OS + backends; probe --json parses; capture --help lists `--backend`, `--region`, `--output`, `--fixture`, `--dump-frames`, `--max-frames`, `--fps`, `--show-cursor`.

- [ ] **Step 5: Verify spec coverage**

Walk through the spec's Completion Criteria (`docs/superpowers/specs/2026-05-20-rollshot-capture-skeleton-design.md`) and confirm each line is true of the merged branch. If any item is missing, file a follow-up task.
