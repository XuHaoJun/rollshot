# Rollshot Capture Skeleton Design

Date: 2026-05-20

## Scope

This phase builds the `rollshot-capture` API surface and the `rollshot capture`
and `rollshot probe` CLI commands without writing any real platform capture
code. It wires the fake/fixture backend end to end through the capture trait,
adds platform stubs that fail cleanly until Phase 2 and Phase 3 fill them in,
and aligns internal types with the rollshot MVP design (`docs/rollshot_mvp_design.md`).

The objective is to lock the capture-layer interfaces and the CLI shape before
introducing portal, PipeWire, or ScreenCaptureKit complexity, so that Phase 2
and Phase 3 become purely additive bodies behind already-tested boundaries.

## Goals

- Refactor `CapturedFrame` to carry an `image::RgbaImage` instead of a flat
  byte buffer, matching the MVP design's "RgbaImage as first-class output"
  principle.
- Replace `Result<_, String>` on `CaptureBackend` and `FrameStream` with a
  dedicated `CaptureError` defined via `thiserror`.
- Add `anyhow` and `thiserror` to the workspace dependencies and use them
  consistently across capture and CLI layers.
- Add a fixture-driven backend that decodes images from a directory and feeds
  them through `FrameStream`, enabling Layer 2 fake integration tests as
  described in the MVP design §18.3.
- Add `LinuxPortalBackend` and `MacosScreenCaptureKitBackend` stubs that expose
  honest `probe()` output and return `CaptureError::NotImplemented` from
  `start()`, so Phase 2 and Phase 3 only need to fill in bodies.
- Add `default_backend()` and a `BackendKind` factory that respects the host
  platform and session type.
- Add a `rollshot capture` CLI subcommand that drives any backend, dumps frames
  on request, stitches frames through `rollshot-core`, and writes a PNG.
- Add a `rollshot probe` CLI subcommand with optional `--json` output.
- Keep `rollshot stitch-folder` as is, because it remains a useful matcher-only
  debug shortcut that bypasses the capture trait.

## Non-Goals

- No DBus, PipeWire, xdg-desktop-portal, or ScreenCaptureKit code.
- No `zbus`, `pipewire-rs`, `objc2`, `core-graphics`, or `scap` dependency.
- No real region selector, overlay, or window picker.
- No clipboard output, no preview window, no progress UI.
- No signal-handling beyond a basic Ctrl-C check; SIGINT support is best
  effort and must not be load-bearing for correctness.
- No multi-monitor handling beyond what `default_backend()` already needs.
- No real-capture smoke tests on hosted CI.

## Reference Projects

This phase studies but does not import code from `learn-projects/`:

- `learn-projects/wayscrollshot/src/capture.rs` for CLI ergonomics.
- `learn-projects/obs-studio/plugins/linux-pipewire/` for the shape of a
  capture lifecycle that the stub `probe()` should describe.
- `learn-projects/scap/src/capturer/engine/mac/` for the same on macOS.

The implementation here is rollshot-native scaffolding. No code is copied.

## Architecture

```text
rollshot-cli
  args parsing
  → BackendKind::resolve(args)
  → backend.probe()      (diagnostic only)
  → backend.start(opts)  → FrameStream
  → loop:
      stream.next_frame()
      → optional dump to disk
      → Stitcher::push_frame(frame.image)
      → break on EndOfStream / max_frames / SIGINT
  → save stitched PNG
  → print summary

rollshot-capture
  trait CaptureBackend  (name, probe, start)
  trait FrameStream     (next_frame)
  enum BackendKind      + factory
  fn default_backend()
  struct CapturedFrame { image: RgbaImage, timestamp, metadata }
  struct CaptureProbe   (diagnostic record)
  enum CaptureError     (thiserror)
  backend impls:
    FixtureBackend     (any platform; reads PNG/JPEG from a directory)
    LinuxPortalBackend (cfg linux; stub start = NotImplemented)
    MacosScreenCaptureKitBackend (cfg macos; stub start = NotImplemented)

rollshot-core
  unchanged
```

The capture crate owns no platform IO in this phase. The fixture backend
performs only filesystem reads and image decoding.

## Capture Crate Modules

```text
crates/rollshot-capture/src/
  lib.rs            re-exports public API
  backend.rs        CaptureBackend, FrameStream, BackendKind, default_backend
  types.rs          CapturedFrame, CaptureOptions, RegionMode, Region, Size,
                    PixelFormat, FrameMetadata, CaptureProbe
  error.rs          CaptureError (thiserror)
  fixture.rs        FixtureBackend, FixtureFrameStream
  fake.rs           FakeFrameStream (kept for unit tests that need an inline
                    Vec<RgbaImage> source)
  linux/
    mod.rs          cfg(target_os = "linux") LinuxPortalBackend stub
  macos/
    mod.rs          cfg(target_os = "macos") MacosScreenCaptureKitBackend stub
```

`linux/mod.rs` and `macos/mod.rs` may be empty modules on the other platform.
The `mod` declaration in `lib.rs` is gated with `cfg`, so non-host stubs do not
compile into the binary.

## Public Types

```rust
use image::RgbaImage;
use std::time::SystemTime;

pub trait CaptureBackend {
    fn name(&self) -> &'static str;
    fn probe(&self) -> CaptureProbe;
    fn start(&mut self, options: CaptureOptions)
        -> Result<Box<dyn FrameStream>, CaptureError>;
}

pub trait FrameStream: Send {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError>;
}

pub struct CapturedFrame {
    pub image: RgbaImage,
    pub timestamp: SystemTime,
    pub metadata: FrameMetadata,
}

pub struct CaptureOptions {
    pub region: RegionMode,
    pub fps: u32,
    pub show_cursor: bool,
    pub prefer_portal_region: bool,
}

pub enum RegionMode {
    Manual(Region),
    PortalPicker,
    FullSource,
}

pub struct Region { pub x: i32, pub y: i32, pub width: u32, pub height: u32 }
pub struct Size   { pub width: u32, pub height: u32 }

pub struct FrameMetadata {
    pub source_size: Option<Size>,
    pub effective_region: Option<Region>,
    pub pixel_format: Option<PixelFormat>,
    pub stride: Option<u32>,
    pub backend: &'static str,
}

pub enum PixelFormat { Rgba, Bgra, Bgrx, Rgbx, Rgb }

pub struct CaptureProbe {
    pub backend: &'static str,
    pub available: bool,
    pub message: String,
    pub details: Vec<(String, String)>,
}

pub enum BackendKind {
    Fixture,
    LinuxPortalPipeWire,
    MacosScreenCaptureKit,
    Unsupported,
}

pub fn default_backend() -> BackendKind;
```

`CapturedFrame.image.width()` and `image.height()` replace the old `Size` field.
`Size` remains on `FrameMetadata.source_size` because the source resolution is
not always equal to the captured frame size (for example when crop metadata
applies).

## CaptureError

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
```

CLI maps these to exit codes:

```text
0  success
0  UserCancelled       (with a short message on stderr)
2  NotImplemented      (hint: try --backend fixture)
3  PermissionDenied
4  Unsupported
1  everything else via anyhow chain
```

## BackendKind and default_backend

```rust
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

impl BackendKind {
    pub fn create(self) -> Result<Box<dyn CaptureBackend>, CaptureError>;
    pub fn from_cli_flag(s: &str) -> Result<Self, CaptureError>;
    pub fn as_flag(self) -> &'static str;
}
```

`from_cli_flag` accepts `auto`, `fixture`, `linux-portal`, `macos-sck`. `auto`
calls `default_backend()`. `Unsupported.create()` returns
`CaptureError::Unsupported` with a message naming the host platform and
session type.

`BackendKind` deliberately does not include a `Fake` variant. `FakeFrameStream`
is constructed directly from a `Vec<RgbaImage>` by tests that want an inline
frame source and is not reachable through the CLI.

## FixtureBackend

The fixture backend reads a directory of pre-recorded frames and feeds them
through the capture trait. It is the workhorse for Layer 2 integration tests.

Behavior:

- Lists regular files in the directory.
- Keeps files with extensions `png`, `jpg`, `jpeg` (case-insensitive).
- Sorts paths lexicographically.
- Returns `CaptureError::InvalidConfig` if no supported images are found.
- On `start`, validates the directory; on `next_frame`, decodes the next file,
  converts to `RgbaImage`, fills `FrameMetadata` with `backend = "fixture"`
  and `pixel_format = Some(Rgba)`, and returns the `CapturedFrame`.
- Returns `CaptureError::EndOfStream` after the last file.
- Ignores `CaptureOptions.fps`, `show_cursor`, and `region` for now. A noop
  for `region` is acceptable because fixtures are already pre-cropped.

`FakeFrameStream` is kept as an in-memory `Vec<RgbaImage>` source for unit
tests that want to construct frames programmatically.

## Platform Stubs

```rust
// crates/rollshot-capture/src/linux/mod.rs
pub struct LinuxPortalBackend;

impl CaptureBackend for LinuxPortalBackend {
    fn name(&self) -> &'static str { "linux-portal" }

    fn probe(&self) -> CaptureProbe {
        // honest probe: report XDG_SESSION_TYPE, XDG_CURRENT_DESKTOP,
        // whether the portal dbus name is reachable later.
        // For now check only env vars (no dbus dependency yet).
    }

    fn start(&mut self, _options: CaptureOptions)
        -> Result<Box<dyn FrameStream>, CaptureError>
    {
        Err(CaptureError::NotImplemented { backend: "linux-portal" })
    }
}
```

The macOS stub follows the same shape. Stubs must compile only on their host
platform.

`probe()` for both backends should be honest about availability:

- Linux probe reports `available = true` only when `XDG_SESSION_TYPE=wayland`
  and `XDG_CURRENT_DESKTOP` mentions KDE or Plasma. `details` includes the
  raw env values so probe output is useful in issue reports.
- macOS probe reports `available = true` when running on macOS. A future
  phase will refine this with a Screen Recording permission check.

`available = true` does not mean the backend works; it means the preconditions
listed in MVP design §12.10 and §13.1 are satisfied. `start()` still returns
`NotImplemented` until Phase 2 and Phase 3.

## CLI Behavior

`rollshot capture` is the primary new command.

```bash
rollshot capture [OPTIONS] --output <PATH>

Options:
  --backend <auto|fixture|linux-portal|macos-sck>        default: auto
  --region  <portal|full|"X,Y WxH">                       default: auto
  --output  <PATH>                                        required
  --fixture <DIR>                                         required with --backend fixture
  --dump-frames <DIR>                                     optional
  --max-frames  <N>                                       default: 200
  --fps <N>                                               default: 5
  --show-cursor                                           default: false
```

Region defaults:

- `auto` resolves to `PortalPicker` when backend is `linux-portal`,
  `FullSource` for `macos-sck`, `FullSource` for `fixture`.
- `portal` is only legal with `linux-portal`.
- `"X,Y WxH"` parses to `RegionMode::Manual(Region { x, y, width, height })`
  with `width > 0` and `height > 0`. Invalid strings yield
  `CaptureError::InvalidConfig`.

Flow:

```text
parse args
  → BackendKind::from_cli_flag(--backend)
  → backend.start(options)
  → frame_count = 0
  → loop:
      next_frame()
        EndOfStream → break
        Err(e)      → return e
      Ok(frame):
        if --dump-frames:
          write frame.image to <dir>/frame_<NNNN>.png
        stitcher.push_frame(frame.image)
        frame_count += 1
        if frame_count >= max_frames: break
  → save stitcher.full_image() to --output as PNG
  → print summary: frames captured, frames appended, output path
```

Summary line example:

```text
captured 12 frames, appended 9, wrote /tmp/out.png (1920x4320)
```

`rollshot probe`:

```bash
rollshot probe [--json]
```

- Default text output enumerates: OS, session type, desktop, default backend,
  and a one-line probe per known backend (`fixture` plus the host's real
  backend stub).
- `--json` emits a stable JSON document for issue reports. The schema is
  fixed for this phase:

```json
{
  "os": "linux",
  "session_type": "wayland",
  "desktop": "KDE",
  "default_backend": "linux-portal",
  "backends": [
    {
      "name": "fixture",
      "available": true,
      "message": "directory-based test backend",
      "details": []
    },
    {
      "name": "linux-portal",
      "available": true,
      "message": "preconditions look ok; backend is not implemented in v0.1 plumbing phase",
      "details": [
        ["XDG_SESSION_TYPE", "wayland"],
        ["XDG_CURRENT_DESKTOP", "KDE"]
      ]
    }
  ]
}
```

`rollshot stitch-folder` is preserved unchanged. It remains the matcher-only
debug shortcut.

## Dependencies

Add to `[workspace.dependencies]`:

```toml
anyhow = "1"
thiserror = "1"
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Crate-level additions:

- `rollshot-capture` depends on `image`, `anyhow`, `thiserror`.
- `rollshot-cli` depends on `clap`, `anyhow`, `serde`, `serde_json`, and
  the existing `image`, `rollshot-capture`, `rollshot-core`.

No DBus, PipeWire, or ScreenCaptureKit dependencies are added in this phase.

## Testing Strategy

Layer 1 (core) is unchanged and remains green.

Layer 2 (capture and CLI through fake/fixture) is the focus of this phase.

`rollshot-capture` unit tests:

- `FixtureBackend` returns files in sorted order.
- `FixtureBackend` reports `InvalidConfig` for an empty directory.
- `FixtureBackend` returns `EndOfStream` after the last file.
- `BackendKind::from_cli_flag` accepts all four strings (`auto`, `fixture`,
  `linux-portal`, `macos-sck`) and rejects others.
- `BackendKind::create()` returns `Unsupported` on a non-Linux non-macOS
  target (or simulates this via a small abstraction; acceptable to test the
  enum mapping only).
- `CaptureError` `Display` strings include the backend name where relevant.
- Linux stub `probe()` echoes env vars into `details` (test via env override).
- Linux/macOS stub `start()` returns `NotImplemented`.

`rollshot-cli` end-to-end tests:

- `rollshot capture --backend fixture --fixture <prebuilt-dir>
  --output <tmp>`
  produces a PNG whose dimensions match the expected stitched output for a
  known synthetic fixture (reuse the generator from the core stitching tests
  or write a small one in the CLI tests crate).
- `--dump-frames <dir>` writes N PNGs with the right naming scheme.
- `rollshot probe` exits 0 with non-empty text output.
- `rollshot probe --json` exits 0 and the output parses via `serde_json`.
- On Linux, `rollshot capture --backend linux-portal --output <tmp>` exits 2
  with a "not implemented" message.
- `rollshot capture --backend fixture --fixture <dir> --output <tmp>
  --max-frames 3` stops after 3 frames even if the fixture has more.

The existing `stitch-folder` end-to-end test remains.

Workspace verification:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Risks

- **Trait churn.** Migrating `CapturedFrame` from `Vec<u8>` to `RgbaImage` is
  a breaking change in the capture crate's public surface, but no consumer
  outside this workspace exists yet, and Phase 2 and Phase 3 would have had
  to make the same change. Doing it now is cheaper.
- **Stub probe accuracy.** A misleading `available = true` from a stub could
  set wrong expectations. Mitigation: stubs always return
  `CaptureError::NotImplemented` from `start()`, and the probe message
  explicitly says "not implemented in v0.1 plumbing phase."
- **Overlap between `stitch-folder` and `capture --backend fixture`.** Both
  read frames from disk and produce a stitched PNG. Accepted, because
  `stitch-folder` bypasses the capture trait, which is the cleanest way to
  iterate on the matcher in isolation.
- **`clap` derive footprint.** Adds a small compile-time cost. Acceptable
  given how much CLI surface this phase introduces.

## Completion Criteria

- `rollshot-capture` exposes `CapturedFrame { image: RgbaImage, .. }` and
  `CaptureError` is the only error type across the trait.
- `FixtureBackend` decodes a directory of PNG/JPEG into a frame stream.
- `LinuxPortalBackend` and `MacosScreenCaptureKitBackend` stubs exist on
  their host platforms and return `NotImplemented` from `start()`.
- `default_backend()` and `BackendKind::create()` are implemented and tested.
- `rollshot capture` produces a PNG when run with `--backend fixture`.
- `rollshot probe` works in both text and JSON modes.
- `rollshot stitch-folder` continues to work unchanged.
- `cargo fmt`, `cargo clippy -D warnings`, and `cargo test --workspace`
  all pass on Linux and macOS hosted CI without any real capture backend.
