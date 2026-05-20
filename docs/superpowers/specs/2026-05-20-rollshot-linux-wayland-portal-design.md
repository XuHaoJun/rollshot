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
  readiness.
- Implement `LinuxPortalBackend::start()` with the portal lifecycle:
  `CreateSession`, `SelectSources`, `Start`, `OpenPipeWireRemote`.
- Connect a PipeWire input stream to the selected portal stream node using the
  fd returned by `OpenPipeWireRemote`.
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
- Add pure unit tests for pixel conversion, stride, crop, stream selection, and
  portal option mapping.
- Add an ignored real Linux capture smoke test for self-hosted/manual KDE
  Wayland validation.
- Update `README.md` with concrete Linux portal manual testing steps.

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
    connect_fd()
    connect_stream()
    next_frame()

  linux/pixel.rs
    LinuxPixelFormat
    LinuxRawFrame
    raw_frame_to_rgba()

  linux/metadata.rs
    LinuxFrameMetadata
    VideoCrop
    VideoTransform
```

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

Use `ashpd` first for the XDG portal API if it exposes the needed ScreenCast
flow and PipeWire remote fd cleanly:

- create session
- select sources
- start
- stream response parsing
- open PipeWire remote

If `ashpd` blocks any required MVP behavior, use `zbus` for a focused
ScreenCast wrapper instead of widening the design. The wrapper should remain
inside `linux/portal.rs`.

Use the `pipewire` Rust bindings for PipeWire stream lifecycle. Prefer the
current stable crate version that compiles in the rollshot workspace on Linux.
The implementation plan must verify the version with `cargo check` before
locking the dependency. If the latest `pipewire` crate creates compile or docs
gaps, falling back to a proven version such as `0.8.x` is acceptable; the local
`learn-projects/scap` checkout already demonstrates a Rust PipeWire approach on
that generation.

## Probe Behavior

`probe()` is diagnostic. It must not open an interactive portal picker and must
not request ScreenCast permission.

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

If `VideoCrop` is present and `Manual(region)` is also requested, this phase
applies `VideoCrop` first and then applies the manual crop relative to the
cropped image only if the manual crop is still in bounds. If this proves
confusing in real use, a later phase can refine the CLI contract. The default
MVP path should be `--region portal` on KDE and `--region full` or manual crop
on other DEs.

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

## README Manual Testing

The README should document:

- Linux Wayland requirement.
- KDE Plasma 6 Wayland as the first validated target.
- Required services:
  - PipeWire
  - WirePlumber or equivalent session manager
  - `xdg-desktop-portal`
  - a DE portal implementation such as `xdg-desktop-portal-kde`
- Required development packages for building on common distributions:
  - `pkg-config`
  - PipeWire development headers
  - DBus development headers if required by selected Rust crates
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
  versions. Mitigation: lock a version only after local compile verification and
  keep all direct usage inside `linux/pipewire.rs`.
- **DMA-BUF-only negotiation.** Some environments may prefer DMA-BUF.
  Mitigation: request MemPtr explicitly and return a clear unsupported error if
  CPU-readable buffers are unavailable.
- **Stride/crop mistakes.** PipeWire rows may be padded and portal crop may be
  delivered as metadata. Mitigation: unit-test stride and crop conversion as
  pure helpers.
- **Video transforms.** Rotated monitors may return transform metadata.
  Mitigation: reject non-identity transforms in MVP instead of silently
  producing wrong images.
- **Interactive portal UI.** Real capture requires user interaction and a real
  desktop session. Mitigation: keep it in ignored/self-hosted/manual tests.

## Completion Criteria

- `LinuxPortalBackend::probe()` reports real ScreenCast and PipeWire
  diagnostics.
- `LinuxPortalBackend::start()` runs portal session/start/open-fd lifecycle.
- PipeWire stream connects to the portal-selected node id using the portal fd.
- KDE multiple-stream response uses the last stream.
- Cursor mode selection follows metadata/embedded/hidden rules.
- CPU-readable BGRA/RGBA/BGRx/RGBx/RGB frames convert to `RgbaImage`.
- Stride is handled correctly.
- `SPA_META_VideoCrop` is applied when present.
- Corrupted/empty buffers do not enter the stitcher.
- Non-identity video transforms fail clearly.
- `RegionMode::PortalPicker`, `FullSource`, and `Manual` are supported on Linux
  Wayland.
- Existing macOS and fixture behavior remains unchanged.
- README has Linux manual validation steps.
- Pure tests pass in the default workspace test suite.
- Ignored Linux real-capture smoke test exists for self-hosted/manual KDE
  Wayland validation.
