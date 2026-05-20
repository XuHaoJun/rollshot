# Rollshot Linux Wayland Portal Backend Design

Date: 2026-05-20

## Scope

This phase replaces the Linux `LinuxPortalBackend` stub with a real Wayland
capture backend built on the XDG Desktop Portal ScreenCast interface and
PipeWire.

The backend is generic for Linux Wayland portal implementations. KDE Plasma 6
Wayland is the first supported and validated target because it is the user's
primary environment and because the MVP specifically needs KDE's portal picker
path. The design must not bake KDE into the core backend architecture.

`learn-projects/obs-studio` remains a reference checkout only. OBS is GPL and
must not be copied into rollshot. It may be used to understand portal lifecycle,
KDE quirks, PipeWire negotiation, and metadata handling. `learn-projects/scap`
may be used as an additional Rust PipeWire reference, but not as the Linux
backend dependency in this phase.

## Goals

- Implement `LinuxPortalBackend::probe()` as real diagnostics for Wayland,
  XDG Desktop Portal ScreenCast, cursor/source capabilities, and PipeWire
  readiness, with a hard 2-second timeout per portal call.
- Implement `LinuxPortalBackend::start()` with the portal lifecycle:
  `CreateSession`, `SelectSources`, `Start`, `OpenPipeWireRemote`.
- Connect a PipeWire input stream to the selected portal stream node using a
  `F_DUPFD_CLOEXEC`-duplicated copy of the fd returned by `OpenPipeWireRemote`,
  with explicit ownership semantics between Rust and PipeWire's C side.
- Define explicit `Drop` order across portal session, PipeWire context/stream,
  and the duplicated fd to prevent leaks and double-closes.
- Make `next_frame()` impossible to hang indefinitely (bounded 5-second wait).
- Support `RegionMode::PortalPicker`, `RegionMode::FullSource`, and
  `RegionMode::Manual(Region)` on Linux Wayland.
- Keep the backend generic for KDE, GNOME, wlroots, Hyprland, and future
  Wayland portal implementations where the standard ScreenCast flow works.
- Treat KDE-specific behavior as capabilities or quirks, not as a separate
  backend.
- Support CPU-readable raw video frames and convert them to `image::RgbaImage`.
- Support BGRA, RGBA, BGRx, RGBx, and RGB input formats.
- Handle stride correctly.
- Request and apply `SPA_META_VideoCrop` when present.
- Detect corrupted and empty PipeWire buffers.
- Reject non-identity video transforms in this MVP instead of returning a
  rotated or flipped image silently.
- Add pure unit tests for pixel conversion, stride, crop, stream selection,
  portal option mapping, fd ownership, drop order, empty-buffer skip,
  header-corruption skip, DMA-BUF rejection, `next_frame` timeout, `probe`
  timeout, and out-of-bounds manual region.
- Add an ignored real Linux capture smoke test for self-hosted/manual KDE
  Wayland validation.
- Update `README.md` with concrete Linux portal manual testing steps and a
  note that the smoke test requires a live human-driven desktop session.
- Update GitHub Actions Linux jobs to install `pkg-config`,
  `libpipewire-0.3-dev`, `libclang-dev`, and `libdbus-1-dev` so the workspace
  continues to build on hosted CI.

## Non-Goals

- No X11 support.
- No grim/slurp backend.
- No custom Wayland overlay selector.
- No GNOME-specific ScreenCast API.
- No DE-specific backend crates or binaries.
- No audio capture.
- No DMA-BUF, GPU texture, modifier, or explicit-sync support.
- No NV12/YUV or 10-bit pixel formats.
- No cursor compositing.
- No restore-token persistence.
- No multi-display selection UI owned by rollshot.
- No attempt to run real portal capture in hosted CI.

## Key Decision: Generic Portal Backend, KDE First

The backend name remains:

```text
linux-portal
```

The implementation should be structured as a generic Linux Wayland portal
backend:

```text
XDG ScreenCast portal
-> PipeWire remote fd
-> PipeWire stream node
-> CPU-readable raw frame
-> RgbaImage
```

KDE is a validation profile and a source of known quirks. It is not a separate
architecture.

The XDG ScreenCast portal standard exposes monitor, window, and virtual source
types. It does not define a portable "rectangular region" source type. KDE's
portal implementation may expose rectangular region selection in its picker UI,
and other Wayland portal implementations may not. This phase therefore treats
region selection as a capability:

- `PortalPicker` asks the portal to present its picker. On KDE, this can produce
  the rectangular region workflow needed by the MVP.
- `FullSource` asks the portal to capture a monitor/window source selected by
  the user.
- `Manual(region)` asks the portal to capture a monitor/window source selected
  by the user, then rollshot crops the returned frame locally.

This keeps future GNOME/wlroots support from requiring a rewrite. If a future
portal implementation lacks rectangular region selection, rollshot can still use
the same backend with `FullSource` or `Manual(region)`.

## Architecture

```text
rollshot-cli
  rollshot capture --backend linux-portal --region portal --output out.png
    -> BackendKind::LinuxPortalPipeWire
    -> LinuxPortalBackend::start(options)
    -> portal CreateSession / SelectSources / Start
    -> choose portal stream node id
    -> portal OpenPipeWireRemote
    -> PipeWireFrameStream
    -> raw PipeWire frame to RgbaImage
    -> existing Stitcher
    -> output PNG

rollshot-capture
  linux/mod.rs
    LinuxPortalBackend
    LinuxPortalFrameStream

  linux/portal.rs
    PortalProbe
    PortalSession
    LinuxPortalCapabilities
    LinuxPortalQuirk
    create_session()
    select_sources()
    start()
    open_pipewire_remote()
    choose_stream()

  linux/pipewire.rs
    PipeWireConnection
    PipeWireStream
    PipeWireFrameReceiver
    LinuxFrameMetadata
    VideoCrop
    VideoTransform
    dup_pipewire_fd()
    connect_fd()
    connect_stream()
    next_frame()

  linux/pixel.rs
    LinuxPixelFormat
    LinuxRawFrame
    raw_frame_to_rgba()
```

Note: frame metadata types (`LinuxFrameMetadata`, `VideoCrop`, `VideoTransform`)
live inside `linux/pipewire.rs` rather than a separate `metadata.rs`. They are
~80 LoC total, tightly coupled to PipeWire buffer parsing, and only used from
the process callback — splitting them would be premature abstraction.

The public capture trait remains unchanged:

```rust
pub trait CaptureBackend {
    fn name(&self) -> &'static str;
    fn probe(&self) -> CaptureProbe;
    fn start(&mut self, options: CaptureOptions) -> Result<Box<dyn FrameStream>, CaptureError>;
}
```

Direct portal and PipeWire dependencies must be Linux-only target dependencies.

## Dependency Strategy

Use `ashpd` for the XDG portal API. It covers the full ScreenCast 5.x
lifecycle (`CreateSession`, `SelectSources`, `Start`, `OpenPipeWireRemote`)
with idiomatic async APIs and is the standard Rust portal library. Lock this
in; do not fall back to hand-rolled `zbus`. If a specific MVP behavior is
blocked by `ashpd`, raise it as a design issue rather than widening the
dependency surface.

`ashpd` requires an async runtime. Use `tokio` with the `current_thread`
flavor, built and owned inside `linux/portal.rs`. The async runtime must not
leak into the cross-platform `CaptureBackend` trait or any `rollshot-core`
type. `CaptureBackend::start()` remains synchronous from the caller's
perspective and blocks on the in-backend tokio runtime.

Use the `pipewire` Rust bindings (crate `pipewire = "0.8"`) for PipeWire stream
lifecycle. Version 0.8.x is proven against PipeWire 0.3 in production — the
local `learn-projects/scap` checkout uses `pipewire = "0.8.0"` against a
PipeWire 0.3 system, which confirms the binding works on the target platform.
The implementation plan must verify the workspace builds with this version via
`cargo check` before any further work. Newer pre-1.0 versions of the crate
exist but are unstable; do not chase them.

All direct portal and PipeWire dependencies must be Linux-only target
dependencies:

```toml
[target.'cfg(target_os = "linux")'.dependencies]
ashpd = { version = "0.9", default-features = false, features = ["tokio"] }
pipewire = "0.8"
tokio = { version = "1", features = ["rt", "sync", "time"] }
nix = { version = "0.29", features = ["fs"] }  # for F_DUPFD_CLOEXEC
```

`nix` is needed for the `fcntl(F_DUPFD_CLOEXEC)` call on the PipeWire fd
(see "PipeWire fd Ownership" below).

## Probe Behavior

`probe()` is diagnostic. It must not open an interactive portal picker and must
not request ScreenCast permission. It must also never block the CLI: every
portal property GET in `probe()` runs under a hard **2-second timeout**. On
timeout, the corresponding field becomes `unavailable` and a `probe_error`
entry is appended to `details` describing which call timed out. `probe()` as a
whole must return within ~3 seconds even on a fully broken portal.

Probe should collect:

```text
os = linux
XDG_SESSION_TYPE
XDG_CURRENT_DESKTOP
WAYLAND_DISPLAY
portal_desktop_available = true|false
screencast_available = true|false
screencast_version = <u32 or unavailable>
available_source_types = monitor|window|virtual bitmask
available_cursor_modes = hidden|embedded|metadata bitmask
pipewire_library_version = <string if available>
desktop_profile = kde|gnome|wlroots|hyprland|unknown
quirks = comma-separated known quirks
```

`available` is true only when:

- the session type is Wayland,
- the ScreenCast portal interface is reachable,
- at least one of monitor or window source type is available,
- PipeWire can be initialized well enough for the backend to attempt capture.

KDE is not required for `available = true`. KDE should add diagnostic details
and known quirks.

## Portal Capabilities and Quirks

Internal model:

```rust
pub struct LinuxPortalCapabilities {
    pub desktop: String,
    pub session_type: String,
    pub portal_version: u32,
    pub source_types: SourceTypes,
    pub cursor_modes: CursorModes,
    pub quirks: Vec<LinuxPortalQuirk>,
}

pub enum LinuxPortalQuirk {
    KdeMayReturnMultipleStreams,
    PortalRegionPickerLikelyAvailable,
    RegionPickerMayReturnVideoCrop,
}
```

Initial quirk detection:

- KDE/Plasma desktop strings add `KdeMayReturnMultipleStreams`.
- KDE/Plasma adds `PortalRegionPickerLikelyAvailable`.
- KDE/Plasma adds `RegionPickerMayReturnVideoCrop`.

These quirks are hints for behavior and diagnostics, not separate code paths
unless a behavior requires it.

## Portal Start Behavior

`start()` performs:

```text
validate Linux Wayland session
probe ScreenCast capabilities
validate requested RegionMode
create portal session
select sources
start session
choose stream node id
open PipeWire remote
connect PipeWire stream
return LinuxPortalFrameStream
```

### Region Modes

Supported:

- `RegionMode::PortalPicker`
- `RegionMode::FullSource`
- `RegionMode::Manual(Region)`

For `PortalPicker`, rollshot lets the portal implementation present the best UI
it has. KDE may offer rectangular region selection. Other portal
implementations may present monitor/window choices.

For `FullSource`, rollshot captures the selected monitor/window stream without
local crop.

For `Manual(region)`, rollshot captures the selected monitor/window stream and
applies local crop after raw frame conversion. Negative origins are rejected
with `CaptureError::InvalidConfig`; zero width/height are already rejected by
CLI parsing.

### SelectSources Options

Request:

```text
types = MONITOR | WINDOW
multiple = false
cursor_mode = selected by cursor capability
persist_mode = DoNot for this phase
```

Cursor mode selection:

```text
if Metadata is available:
  use Metadata
else if options.show_cursor && Embedded is available:
  use Embedded
else:
  use Hidden
```

This keeps cursor pixels out of the stream by default, which is better for
stitching. Metadata support leaves room for future cursor handling without
compositing it in this phase.

### Stream Selection

Portal `Start` returns a list of PipeWire streams.

```text
streams.len() == 0 -> CaptureError::Backend("portal returned no streams")
streams.len() == 1 -> use streams[0]
streams.len() > 1  -> use streams.last()
```

Using the last stream is required for KDE compatibility. OBS documents a KDE
portal behavior where more than one stream may be returned even when only one
was expected, and the last stream is the useful one. Rollshot should record
this in diagnostics or a warning detail, but the CLI does not need a warning UI
in this phase.

### Portal Error Mapping

Map portal responses:

- response `0`: success
- response `1`: `CaptureError::UserCancelled`
- response `2`: `CaptureError::Backend("portal interaction ended")`
- other response: `CaptureError::Backend("unexpected portal response")`

Map setup errors:

- missing ScreenCast interface: `CaptureError::Unsupported`
- missing monitor/window source type: `CaptureError::Unsupported`
- DBus connection failure: `CaptureError::Backend`
- `OpenPipeWireRemote` fd failure: `CaptureError::Backend`
- non-Wayland session: `CaptureError::Unsupported`

## PipeWire fd Ownership

`OpenPipeWireRemote` returns a file descriptor that is owned by the DBus
response (as an `OwnedFd` in `ashpd`). Passing that fd directly to
`pw_context_connect_fd` creates an undefined ownership boundary: PipeWire's C
side closes the fd on `pw_context_destroy`, and Rust's `OwnedFd::drop` also
closes it — leading to double-close (silent UB at best, "Bad file descriptor"
log spam under load, occasional `EBADF` panics in unrelated code).

OBS handles this at `pipewire.c:1135` via:

```c
obs_pw->core = pw_context_connect_fd(
    obs_pw->context,
    fcntl(obs_pw->pipewire_fd, F_DUPFD_CLOEXEC, 5),
    NULL, 0);
```

Rollshot must mirror this. `linux/pipewire.rs::dup_pipewire_fd()` duplicates
the incoming `BorrowedFd` with `F_DUPFD_CLOEXEC` (using `nix::fcntl::fcntl`),
hands the duplicate to PipeWire, and lets the original `OwnedFd` from `ashpd`
drop normally. The `5` minimum-fd argument keeps the duplicate away from stdio
slots.

A pure unit test must assert that `dup_pipewire_fd()` does not consume its
input fd (the borrow remains valid for reading after the call).

## PipeWire Behavior

PipeWire lifecycle:

```text
fd from OpenPipeWireRemote
-> PipeWire context/core connected from fd
-> input stream created with media type video/capture/screen properties
-> stream connected to portal node id
-> format negotiation requests CPU-readable raw video
-> process callback copies usable frames into a bounded queue
-> FrameStream::next_frame() returns converted frames synchronously
```

MVP stream flags:

```text
AUTOCONNECT
MAP_BUFFERS
```

MVP buffer type:

```text
SPA_DATA_MemPtr
```

The implementation must not request or accept DMA-BUF in this phase. If
PipeWire produces DMA-BUF anyway, return a clear backend error. Rollshot needs
CPU-side `RgbaImage`; GPU zero-copy is a future optimization with different
complexity.

### Format Negotiation

Request raw video formats:

```text
BGRA
RGBA
BGRx
RGBx
RGB
```

The backend may specify a framerate from `CaptureOptions.fps`.

Resolution should not be forced in the MVP. The portal-selected stream and
PipeWire negotiation decide the actual size. Returned frame dimensions are
authoritative for stitching.

### Requested Metadata

Request:

```text
SPA_META_Header
SPA_META_VideoCrop
SPA_META_Cursor
SPA_META_VideoTransform
```

MVP handling:

- `Header`: reject or skip corrupted buffers.
- `VideoCrop`: apply crop before returning `RgbaImage`.
- `Cursor`: parse enough to know it exists, but do not composite cursor pixels.
- `VideoTransform`: accept identity/none only. Return backend error for rotated
  or flipped frames in this phase.

## Frame Queue and Threading

`CaptureBackend::start()` remains synchronous from the caller's perspective.
Portal and PipeWire internals may use async runtimes or threads as needed, but
that complexity stays inside `linux/`.

PipeWire callbacks should not run expensive stitching. They should copy or
convert the latest usable frame into a bounded channel.

Recommended queue behavior:

```text
capacity = 2 or 3 frames
when full, drop oldest frame and keep newest
```

Scroll stitching does not need every frame. Keeping the latest frames prevents
unbounded memory growth if capture fps exceeds stitching speed.

`LinuxPortalFrameStream::next_frame()` blocks waiting for the next frame and
returns:

- `Ok(CapturedFrame)` for usable frames,
- `CaptureError::EndOfStream` when the stream shuts down cleanly,
- `CaptureError::Backend` for stream errors.

`next_frame()` must use a bounded wait: block up to **5 seconds** waiting for a
frame to arrive on the bounded queue. If no frame arrives within that window,
return `CaptureError::Backend("PipeWire stream produced no frames within 5s")`.
This prevents a hung CLI when the compositor stalls or the stream silently
falls into a no-data state. The 5s constant is generous enough that healthy
slow-fps capture (e.g. 1fps debug runs) does not trigger it.

Empty buffers should be skipped up to a small consecutive limit, such as ten
empty buffers. After that, return `CaptureError::Backend` with a message that
the PipeWire stream did not produce a usable video frame.

## Pixel Conversion

Pure helper API:

```rust
pub enum LinuxPixelFormat {
    Bgra,
    Rgba,
    Bgrx,
    Rgbx,
    Rgb,
}

pub struct LinuxRawFrame<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: LinuxPixelFormat,
    pub crop: Option<Region>,
}

pub fn raw_frame_to_rgba(frame: LinuxRawFrame<'_>) -> Result<RgbaImage, CaptureError>;
```

Conversion:

```text
BGRA -> [R, G, B, A]
RGBA -> [R, G, B, A]
BGRx -> [R, G, B, 255]
RGBx -> [R, G, B, 255]
RGB  -> [R, G, B, 255]
```

Validation:

```text
width > 0
height > 0
stride >= width * bytes_per_pixel
data.len() >= stride * height
crop width/height > 0
crop is inside source frame
```

The helper must read rows by stride:

```text
for y in crop.y .. crop.y + crop.height:
  row = data[y * stride ..]
  for x in crop.x .. crop.x + crop.width:
    convert pixel at row[x * bytes_per_pixel]
```

It must never assume `stride == width * bytes_per_pixel`.

### Performance Budget

KDE/KWin's compositor output is typically `XRGB8888` (= `SPA_VIDEO_FORMAT_BGRx`)
on x86_64. Converting BGRx to RGBA per frame is a hot loop at 4K and the most
likely chosen format in practice.

Soft budget: `raw_frame_to_rgba()` should complete in **under 10 ms for a 4K
frame** on a typical x86_64 development laptop. At the default 5 fps capture
rate this leaves comfortable headroom for stitching.

If the naive scalar implementation exceeds the budget, vectorize the BGRx and
BGRA paths via 4-byte word swap (`u32` operations). Do not pre-optimize: ship
the readable scalar version first, then bench. A bench harness under
`crates/rollshot-capture/benches/pixel.rs` is acceptable but not required for
MVP — instead include one `#[test]` that converts a synthetic 3840×2160 BGRx
frame and asserts elapsed time is under 20 ms (a loose ceiling that still
catches obvious regressions like accidental double-allocation per row).

The helper must not allocate per row. One `Vec<u8>` allocation of size
`width * height * 4` per call is acceptable for MVP (see "Frame Queue and
Threading" — buffer pooling is a future optimization).

## Frame Metadata

Returned `CapturedFrame.metadata`:

```text
backend = "linux-portal"
source_size = Some(Size { width: negotiated_width, height: negotiated_height })
effective_region = VideoCrop if present, else Manual region if applied, else None
scale_factor = None
pixel_format = Some(original PipeWire pixel format)
stride = Some(source stride)
```

### Coordinate Space Rules

`VideoCrop` is portal-driven metadata: it represents a crop that the portal
already applied (or wants the consumer to apply) on the source-monitor frame.
It is always honored.

`RegionMode::Manual(region)` is user-supplied and operates on the **frame
delivered by the portal** — i.e. after any `VideoCrop` has been applied. This
means:

- `RegionMode::PortalPicker` and `RegionMode::Manual(region)` are already
  mutually exclusive at the enum level. The CLI parser must continue to
  enforce this (a `--region portal` flag and a `--region "0,0 800x600"` flag
  cannot both be passed in one invocation).
- For `RegionMode::FullSource`: `VideoCrop` is honored if present; no manual
  crop is applied. The returned image dimensions equal the post-VideoCrop
  dimensions.
- For `RegionMode::Manual(region)`: the manual crop coordinates are
  interpreted relative to the post-VideoCrop frame. If the portal also emits
  a `VideoCrop` for this stream (uncommon for monitor/window sources, but
  possible), the manual crop still operates on the already-VideoCropped
  frame. This matches the principle of "the portal owns its crop; the user
  owns everything after".
- If the manual region falls outside the post-VideoCrop image bounds, return
  `CaptureError::InvalidConfig` with a message identifying both the requested
  region and the available frame size.

The default MVP path should be `--region portal` on KDE and `--region full` or
manual crop on other DEs.

## Resource Lifecycle / Drop Order

PipeWire and the portal allocate three kinds of resources that must be
released in a specific order to avoid hangs, leaks, or double-frees across
multiple captures in one process:

1. **PipeWire thread loop must be stopped before stream destruction.**
   Calling `pw_stream_destroy` while the thread loop is still spinning a
   callback can deadlock. `Drop` on `PipeWireStream` must first
   `pw_thread_loop_stop`, then disconnect the stream, then destroy it.
2. **PipeWire context destruction closes the dup'd fd.** Rollshot must not
   close the dup'd fd directly — `pw_context_destroy` does it. Closing it
   manually would be a double-close.
3. **The original `OwnedFd` from `ashpd` is dropped independently** and closes
   the kernel fd that DBus handed us. The PipeWire side has its own dup, so
   this is safe.
4. **Portal session handle should be closed explicitly.** `ashpd` exposes
   `Session::close()` — call it on `Drop`. If the process exits without
   closing, KDE will eventually garbage-collect, but long-running processes
   (the future `rollshot-app` GUI) would leak portal sessions.

Recommended Drop order on `LinuxPortalFrameStream`:

```text
1. Stop PipeWire thread loop.
2. Disconnect + destroy PipeWire stream.
3. Destroy PipeWire context (closes dup'd fd).
4. Drop the original OwnedFd from ashpd (closes the original fd).
5. Close the portal session via Session::close().
6. Drop the DBus connection / tokio runtime.
```

Each step must be infallible at the Rust level (`Drop` cannot return errors).
Underlying library errors should be logged but not panicked on.

A pure test must spawn and drop a synthetic `LinuxPortalFrameStream` (built
against in-process fakes for the PipeWire stream and portal session) and
assert all fakes observe their close calls in the documented order.

## CLI Behavior

Existing commands stay valid:

```bash
rollshot probe --json
rollshot capture --backend linux-portal --region portal --output out.png
rollshot capture --backend linux-portal --region full --output out.png
rollshot capture --backend linux-portal --region "0,0 900x700" --output out.png
rollshot capture --backend linux-portal --region portal --dump-frames target/test-artifacts/linux-frames --output out.png
```

On Linux Wayland, `--backend auto` continues to resolve to `linux-portal`.

On Linux X11, `linux-portal` returns unsupported with a clear message that
rollshot v0.1 supports Linux capture through Wayland portals only.

## CI Build Dependencies

The `pipewire` crate's `build.rs` invokes `pkg-config` against
`libpipewire-0.3` and uses `bindgen` (which needs `libclang`). Once this phase
lands, **the workspace will not build on a vanilla Linux GitHub Actions
runner** without installing system packages first. This is a hard-stop CI
breakage if not addressed.

The implementation plan must include a task that updates the existing GitHub
Actions workflows to install, on Linux jobs only:

```text
pkg-config
libpipewire-0.3-dev
libclang-dev
libdbus-1-dev   # required by ashpd transitively
```

Concretely, the Linux job should add a step like:

```yaml
- name: Install Linux capture deps
  if: runner.os == 'Linux'
  run: |
    sudo apt-get update
    sudo apt-get install -y pkg-config libpipewire-0.3-dev libclang-dev libdbus-1-dev
```

macOS and Windows jobs are unaffected — the Linux deps live behind a
`cfg(target_os = "linux")` dependency block.

## README Manual Testing

The README should document:

- Linux Wayland requirement.
- KDE Plasma 6 Wayland as the first validated target.
- The smoke test requires a live desktop session with a human available to
  click the portal picker. It cannot run unattended and must not be invoked
  from hosted CI.
- Required services:
  - PipeWire (libpipewire-0.3)
  - WirePlumber or equivalent session manager
  - `xdg-desktop-portal`
  - a DE portal implementation such as `xdg-desktop-portal-kde`
- Required development packages for building on common distributions
  (Debian/Ubuntu names; adapt for Fedora/Arch):
  - `pkg-config`
  - `libpipewire-0.3-dev` (PipeWire development headers)
  - `libclang-dev` (required by `bindgen` in the `pipewire` crate)
  - `libdbus-1-dev` (required by `ashpd` / `zbus`)
- `cargo run -p rollshot-cli -- probe --json`.
- A KDE portal-picker capture command.
- A full-source capture command.
- A manual local-crop command.
- A `--dump-frames` command.
- The ignored smoke-test command:
  `ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test linux_portal_smoke -- --ignored --nocapture`.
- Expected artifact path under `target/test-artifacts/`.

## Testing Strategy

Pure tests on any Linux host that can compile the module:

Happy-path / argument-mapping:

- `choose_stream()` returns error for no streams.
- `choose_stream()` returns the only stream for one stream.
- `choose_stream()` returns the last stream for multiple streams.
- cursor mode selection prefers metadata.
- cursor mode selection uses embedded only when requested and metadata is not
  available.
- cursor mode selection falls back to hidden.
- manual region rejects negative origins.
- portal option mapping uses monitor/window, `multiple=false`, and no
  persistence.
- `raw_frame_to_rgba()` converts BGRA.
- `raw_frame_to_rgba()` preserves RGBA.
- `raw_frame_to_rgba()` converts BGRx/RGBx/RGB with alpha 255.
- `raw_frame_to_rgba()` handles stride larger than row width.
- `raw_frame_to_rgba()` applies crop.
- `raw_frame_to_rgba()` rejects out-of-bounds crop.
- non-identity transform mapping returns backend error.
- `raw_frame_to_rgba()` converts a synthetic 3840×2160 BGRx frame in under
  20 ms (loose perf regression check).

Negative-path / failure-mode tests (close gaps surfaced in design review):

- `dup_pipewire_fd()` does not consume its input — the borrowed fd remains
  valid for `read()` after the call returns.
- `LinuxPortalFrameStream::drop` invokes (1) PipeWire thread-loop stop,
  (2) stream disconnect+destroy, (3) context destroy, (4) original-fd drop,
  (5) portal session close — in that order, against in-process fakes.
- Empty-buffer skip terminates: after 10 consecutive empty buffers from a
  fake PipeWire source, `next_frame()` returns `CaptureError::Backend` with
  a "did not produce a usable video frame" message.
- Header-corruption flag drops the buffer: a fake buffer with
  `SPA_META_HEADER_FLAG_CORRUPTED` set is skipped without erroring; the next
  valid buffer is returned.
- DMA-BUF arrival is rejected cleanly: a fake buffer with
  `type == SPA_DATA_DmaBuf` causes `next_frame()` to return
  `CaptureError::Unsupported` with a message identifying the unexpected
  buffer type.
- `next_frame()` timeout: if no frame is enqueued within 5 seconds (use a
  test-only override of 100 ms via a `const`/cfg), `next_frame()` returns
  `CaptureError::Backend("PipeWire stream produced no frames within …")`.
- `probe()` per-call timeout: when a fake portal proxy sleeps longer than
  2 seconds on a property GET, `probe()` returns within ~3 seconds with
  `screencast_available = false` and a `probe_error` entry in `details`.
- Manual region rejected when outside post-VideoCrop bounds: given a portal
  frame size of 1000×800 and a requested manual region of `(500, 500,
  1000×1000)`, `start()` returns `CaptureError::InvalidConfig` mentioning
  both the requested region and the available frame size.

Default workspace tests:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Real capture smoke test:

```bash
ROLLSHOT_REAL_CAPTURE=1 cargo test -p rollshot-capture --test linux_portal_smoke -- --ignored --nocapture
```

The smoke test runs only when `ROLLSHOT_REAL_CAPTURE=1`. It should:

1. Require `XDG_SESSION_TYPE=wayland`.
2. Start `LinuxPortalBackend` with `RegionMode::PortalPicker`.
3. Let the user select a source in the portal picker.
4. Receive at least three frames.
5. Assert image width and height are non-zero.
6. Assert metadata backend is `linux-portal`.
7. Write one frame artifact to `target/test-artifacts/linux_portal_first_frame.png`.

Hosted CI must not run the real capture smoke test by default.

## OBS Reference Points

OBS behavior used as reference:

- It creates a ScreenCast portal session, selects sources, starts the session,
  opens the PipeWire remote fd, and connects a PipeWire stream to the returned
  node id.
- It handles KDE returning multiple streams by selecting the last stream.
- It selects cursor mode based on available cursor modes, preferring metadata
  when supported.
- It connects PipeWire from the fd returned by `OpenPipeWireRemote`, not from
  the default PipeWire socket.
- It requests `SPA_META_VideoCrop` and reads crop metadata from buffers.
- It requests mapped buffers and supports CPU-readable memory buffers.

Rollshot reimplements the behavior in Rust and keeps only the subset needed for
`RgbaImage` capture. It must not copy OBS source code.

## Risks

- **Portal region selection is not standard.** KDE may offer rectangular region
  selection; GNOME or other DEs may not. Mitigation: keep `PortalPicker`,
  `FullSource`, and `Manual(region)` as separate behaviors and make probe report
  capabilities clearly.
- **PipeWire Rust crate churn.** PipeWire bindings have multiple active
  versions. Mitigation: lock to `pipewire = "0.8"` (proven via `scap`), keep all
  direct usage inside `linux/pipewire.rs`.
- **DMA-BUF-only negotiation.** Some environments may prefer DMA-BUF.
  Mitigation: request MemPtr explicitly and return a clear `Unsupported` error
  if a DMA-BUF buffer arrives anyway; pure test covers this path.
- **fd ownership across the Rust/C boundary.** Naive use of the `OwnedFd`
  returned by `ashpd` would double-close after `pw_context_destroy`. Mitigation:
  `dup_pipewire_fd()` with `F_DUPFD_CLOEXEC`; pure test asserts the original fd
  is not consumed.
- **Resource leaks across multiple captures.** Long-running processes (future
  `rollshot-app`) could leak portal sessions and fds. Mitigation: explicit
  `Drop` order (thread loop → stream → context → fd → session); pure test
  asserts the order against fakes.
- **Probe hangs on a broken portal.** Default DBus timeouts can leave the CLI
  apparently hung for ~25s. Mitigation: 2-second per-call timeout in `probe`;
  total wall-clock bound of ~3s; pure test against a sleeping fake proxy.
- **`next_frame()` hangs on a stalled compositor.** Mitigation: 5-second
  per-call timeout in `next_frame()`; pure test against a silent fake stream.
- **Stride/crop mistakes.** PipeWire rows may be padded and portal crop may be
  delivered as metadata. Mitigation: unit-test stride and crop conversion as
  pure helpers.
- **Video transforms.** Rotated monitors may return transform metadata.
  Mitigation: reject non-identity transforms in MVP instead of silently
  producing wrong images.
- **Interactive portal UI.** Real capture requires user interaction and a real
  desktop session. Mitigation: keep it in ignored/self-hosted/manual tests.
- **CI breakage from new system deps.** The `pipewire` crate needs PipeWire
  dev headers + libclang; the workspace will not build on a vanilla GitHub
  Actions Linux runner without them. Mitigation: dedicated task to update
  Linux CI jobs with the apt install step; tracked in completion criteria.

## Completion Criteria

Backend behavior:

- `LinuxPortalBackend::probe()` reports real ScreenCast and PipeWire
  diagnostics within a 3-second wall-clock bound even on a broken portal.
- `LinuxPortalBackend::start()` runs portal session/start/open-fd lifecycle.
- PipeWire stream connects to the portal-selected node id using a
  `F_DUPFD_CLOEXEC`-duplicated copy of the portal fd; the original `OwnedFd`
  from `ashpd` is dropped independently without double-closing.
- KDE multiple-stream response uses the last stream.
- Cursor mode selection follows metadata/embedded/hidden rules.
- CPU-readable BGRA/RGBA/BGRx/RGBx/RGB frames convert to `RgbaImage`.
- Stride is handled correctly.
- `SPA_META_VideoCrop` is applied when present.
- Corrupted/empty buffers do not enter the stitcher (header flag check +
  10-empty-buffer skip limit).
- DMA-BUF buffers (if delivered despite our MemPtr-only request) produce a
  clear `CaptureError::Unsupported` rather than UB.
- Non-identity video transforms fail clearly.
- `next_frame()` cannot hang indefinitely (5-second per-call timeout).
- `RegionMode::PortalPicker`, `FullSource`, and `Manual` are supported on Linux
  Wayland, with documented coordinate-space rules for `VideoCrop` interaction.
- Existing macOS and fixture behavior remains unchanged.

Resource lifecycle:

- `LinuxPortalFrameStream::drop` releases resources in the documented order
  (thread loop → stream → context → original fd → portal session) and is
  proven by a pure test against in-process fakes.
- No leaked file descriptors across repeated `probe()` + `start()` cycles in
  a single process.

Quality / CI / docs:

- Pure tests pass in the default workspace test suite, including all
  negative-path tests listed in "Testing Strategy".
- 4K BGRx → RGBA conversion completes within the 20 ms regression ceiling.
- GitHub Actions Linux jobs install `pkg-config`, `libpipewire-0.3-dev`,
  `libclang-dev`, and `libdbus-1-dev`; the workspace `cargo build`, `cargo
  clippy`, and `cargo test` jobs all stay green on the hosted runner.
- README has Linux manual validation steps and a note that the smoke test
  requires a live desktop session and a human to drive the picker.
- Ignored Linux real-capture smoke test exists for self-hosted/manual KDE
  Wayland validation.
