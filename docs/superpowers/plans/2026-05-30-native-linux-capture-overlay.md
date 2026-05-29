# Native Linux Capture Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Linux-only `rollshot-overlay` crate — an `iced_layershell`
capture overlay that owns a minimal capture+stitch driver and returns a
finalized `CaptureResult` — verified by a standalone harness binary on KDE 6
Wayland. Tauri-free; `src-tauri` wiring is Phase 4.

**Architecture:** A new workspace crate `rollshot-overlay`. Pure coordinate
mapping and the crop+stitch driver core are TDD'd cross-platform with
`FakeFrameStream`. The `iced_layershell` UI (ported from the Phase 2 spike
prototype) drives crop selection → live preview → Esc, calling the driver and
returning `CaptureResult` through a blocking `run_overlay()`. One permanent D4
change restricts portal capture to monitors.

**Tech Stack:** Rust; `iced 0.14` + `iced_layershell 0.18` (Linux-gated);
`rollshot-capture` + `rollshot-core` (path deps); `image 0.25`.

**Spec:** `docs/superpowers/specs/2026-05-30-native-linux-capture-overlay-design.md`
(read first — locks P3.1–P3.7). Port source for the UI:
`spikes/layershell-feasibility/src/overlay_app.rs`. Driver pattern to mirror:
`crates/rollshot-app/src-tauri/src/session.rs:199-212,374-561`.

---

## Ground Rules

- **No `std::process::exit`** anywhere in the crate (P3.3) — Phase 4 runs it
  inside the Tauri process; exiting would kill Tauri.
- **Do not touch** `cmd_capture.rs` or `session.rs` (P3.2: no shared-driver
  refactor). The only `rollshot-capture` edit is Task 2.
- `rollshot-core` / `rollshot-capture` MUST NOT depend on `rollshot-overlay`;
  `rollshot-overlay` MUST NOT depend on Tauri.
- All shell commands are prefixed `rtk` per `RTK.md`.
- Hardware for "Run on KDE 6" steps: a KDE 6 Wayland session (dev box:
  Plasma 6.6.5 / KWin 6.6.5 / NVIDIA).

## File Structure

| File | Responsibility |
|------|----------------|
| `crates/rollshot-overlay/Cargo.toml` | manifest; layer-shell deps Linux-gated |
| `crates/rollshot-overlay/src/lib.rs` | `CaptureResult`, `OverlayConfig`, `OverlayError`, `run_overlay`; module wiring |
| `crates/rollshot-overlay/src/coords.rs` | pure `map_crop_to_frame` (R4) |
| `crates/rollshot-overlay/src/driver.rs` | `stitch_stream` core + threaded `Driver` (latest-wins + preview) |
| `crates/rollshot-overlay/src/overlay.rs` | `iced_layershell` app (ported from spike) |
| `crates/rollshot-overlay/src/bin/capture_overlay.rs` | harness binary (KDE 6 acceptance) |
| `Cargo.toml` (root) | add `crates/rollshot-overlay` member |
| `crates/rollshot-capture/src/linux/portal.rs` | D4 monitor-only (permanent) |

---

## Task 1: Scaffold the `rollshot-overlay` crate

**Files:**
- Create: `crates/rollshot-overlay/Cargo.toml`
- Create: `crates/rollshot-overlay/src/lib.rs`
- Modify: `Cargo.toml` (root workspace `members`)

- [ ] **Step 1: Create the manifest**

`crates/rollshot-overlay/Cargo.toml`:

```toml
[package]
name = "rollshot-overlay"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
image = { version = "0.25", features = ["png"] }
rollshot-capture = { path = "../rollshot-capture" }
rollshot-core = { path = "../rollshot-core" }

[target.'cfg(target_os = "linux")'.dependencies]
iced = { version = "0.14", features = ["canvas", "image"] }
iced_layershell = "0.18"
```

- [ ] **Step 2: Create the cross-platform lib skeleton**

`crates/rollshot-overlay/src/lib.rs`:

```rust
//! Native Linux Wayland capture overlay (Phase 3). Linux-only behavior; the
//! crate compiles to a stub on other targets so `cargo build --workspace`
//! works everywhere.

use image::RgbaImage;
use rollshot_core::StitchStats;

/// The finalized capture handed back to the caller (Tauri in Phase 4).
/// Named generically per architecture spec D5 — not "save PNG only".
#[derive(Debug, Clone)]
pub struct CaptureResult {
    pub image: RgbaImage,
    pub stats: StitchStats,
}

/// Inputs for a capture session.
#[derive(Debug, Clone)]
pub struct OverlayConfig {
    /// Backend selector, e.g. "auto" / "linux-portal" (BackendKind::from_cli_flag).
    pub backend: String,
    pub fps: u32,
    pub show_cursor: bool,
}

#[derive(Debug)]
pub enum OverlayError {
    /// Returned on non-Linux targets.
    Unsupported,
    Capture(String),
    Overlay(String),
}

impl std::fmt::Display for OverlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OverlayError::Unsupported => write!(f, "overlay is only supported on Linux"),
            OverlayError::Capture(m) => write!(f, "capture error: {m}"),
            OverlayError::Overlay(m) => write!(f, "overlay error: {m}"),
        }
    }
}

impl std::error::Error for OverlayError {}

#[cfg(target_os = "linux")]
mod coords;
#[cfg(target_os = "linux")]
mod driver;
#[cfg(target_os = "linux")]
mod overlay;

/// Run the capture overlay, blocking the calling thread until the user
/// finishes (Esc) or cancels. `Ok(Some(_))` on finish, `Ok(None)` on cancel.
#[cfg(target_os = "linux")]
pub fn run_overlay(config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    overlay::run(config)
}

#[cfg(not(target_os = "linux"))]
pub fn run_overlay(_config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    Err(OverlayError::Unsupported)
}
```

Note: on Linux this won't compile until Tasks 3–7 create `coords`/`driver`/
`overlay`. To keep Step 4 green now, temporarily comment the three `mod` lines
and the Linux `run_overlay` body (return `Err(OverlayError::Unsupported)`), and
uncomment them in the task that adds each module. Track this in the commit.

- [ ] **Step 3: Register the workspace member**

In root `Cargo.toml`, add `"crates/rollshot-overlay"` to `[workspace] members`
(keep the list alphasorted if it already is).

- [ ] **Step 4: Verify it builds and has no reverse dep**

Run: `rtk cargo build -p rollshot-overlay`
Expected: compiles (stub).

Run: `rtk cargo tree -p rollshot-core | rtk grep -i overlay`
Expected: no output (rollshot-core does not depend on the overlay).

Run: `rtk cargo fmt --check`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-overlay Cargo.toml
rtk git commit -m "feat(overlay): scaffold rollshot-overlay crate (Phase 3)"
```

---

## Task 2: D4 — restrict portal capture to monitors (permanent)

**Files:**
- Modify: `crates/rollshot-capture/src/linux/portal.rs:258-259` (select_sources)
- Modify: `crates/rollshot-capture/src/linux/portal.rs:280-286` (stream-info build)

- [ ] **Step 1: Verify the current source line, then restrict to Monitor**

Read `portal.rs` around line 258 to confirm the current bitmask, then change:

```rust
                screencast.select_sources(
                    &session,
                    cursor_mode,
                    ashpd::desktop::screencast::SourceType::Monitor
                        | ashpd::desktop::screencast::SourceType::Window,
                    false,
                    None,
                    ashpd::desktop::PersistMode::DoNot,
                ),
```

to (drop the `| ... Window`):

```rust
                screencast.select_sources(
                    &session,
                    cursor_mode,
                    ashpd::desktop::screencast::SourceType::Monitor,
                    false,
                    None,
                    ashpd::desktop::PersistMode::DoNot,
                ),
```

- [ ] **Step 2: Defensive check — reject a Window stream if one slips through**

At the stream-info build (~`:280-286`), it currently reads only
`pipe_wire_node_id()`. Add a read of the started stream's source type and fail
if it is a window. Confirm the exact ashpd accessor first:

Run: `rtk grep -rn "fn source_type\|SourceType" ~/.cargo/registry/src/*/ashpd-0.9.3/src/desktop/screencast.rs`
Expected: shows the `Stream::source_type(&self) -> Option<SourceType>` accessor
(or equivalent) — use the real name it prints.

Then, after `let chosen = choose_stream(&stream_infos)?;`, add a guard (adapt
the accessor name to what the grep showed):

```rust
            if streams
                .streams()
                .iter()
                .any(|s| matches!(s.source_type(), Some(ashpd::desktop::screencast::SourceType::Window)))
            {
                return Err(CaptureError::Backend(anyhow::anyhow!(
                    "window capture is not supported; select a monitor"
                )));
            }
```

- [ ] **Step 3: Verify the capture crate still passes**

Run: `rtk cargo test -p rollshot-capture`
Expected: PASS (no behavior the existing tests depend on changed; the source
restriction and window guard are new).

Run: `rtk cargo clippy -p rollshot-capture --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 4: Manual portal check (KDE 6)**

Run any capture that opens the portal picker (e.g. the existing app or
`rollshot capture` without `--headless`), confirm the KDE picker now offers
**only monitors** (no window list). Record the observation in the commit body.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-capture/src/linux/portal.rs
rtk git commit -m "feat(capture): restrict portal sources to Monitor (D4)"
```

---

## Task 3: Coordinate mapping `coords::map_crop_to_frame` (R4) — TDD

**Files:**
- Create: `crates/rollshot-overlay/src/coords.rs`
- Modify: `crates/rollshot-overlay/src/lib.rs` (uncomment `mod coords;`)

- [ ] **Step 1: Write the failing tests**

Create `crates/rollshot-overlay/src/coords.rs`:

```rust
use rollshot_capture::{Region, Size};

/// A crop rectangle in overlay logical pixels (layer-surface-local).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[cfg(test)]
mod tests {
    use super::{map_crop_to_frame, LogicalRect};
    use rollshot_capture::{Region, Size};

    fn rect(x: f32, y: f32, width: f32, height: f32) -> LogicalRect {
        LogicalRect { x, y, width, height }
    }

    #[test]
    fn maps_one_to_one_at_100_percent() {
        let out = map_crop_to_frame(
            rect(100.0, 200.0, 300.0, 400.0),
            Size { width: 1920, height: 1080 },
            Size { width: 1920, height: 1080 },
        );
        assert_eq!(out, Region { x: 100, y: 200, width: 300, height: 400 });
    }

    #[test]
    fn scales_up_at_150_percent() {
        let out = map_crop_to_frame(
            rect(100.0, 100.0, 200.0, 200.0),
            Size { width: 1280, height: 720 }, // logical
            Size { width: 1920, height: 1080 }, // 1.5x device pixels
        );
        assert_eq!(out, Region { x: 150, y: 150, width: 300, height: 300 });
    }

    #[test]
    fn scales_up_at_125_percent() {
        let out = map_crop_to_frame(
            rect(80.0, 40.0, 160.0, 80.0),
            Size { width: 1536, height: 864 },
            Size { width: 1920, height: 1080 },
        );
        assert_eq!(out, Region { x: 100, y: 50, width: 200, height: 100 });
    }

    #[test]
    fn clamps_to_source_bounds() {
        let out = map_crop_to_frame(
            rect(1800.0, 1000.0, 400.0, 400.0),
            Size { width: 1920, height: 1080 },
            Size { width: 1920, height: 1080 },
        );
        assert_eq!(out, Region { x: 1800, y: 1000, width: 120, height: 80 });
    }

    #[test]
    fn zero_overlay_size_yields_empty_region() {
        let out = map_crop_to_frame(
            rect(10.0, 10.0, 20.0, 20.0),
            Size { width: 0, height: 0 },
            Size { width: 1920, height: 1080 },
        );
        assert_eq!(out, Region { x: 0, y: 0, width: 0, height: 0 });
    }
}
```

In `lib.rs`, uncomment `#[cfg(target_os = "linux")] mod coords;`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `rtk cargo test -p rollshot-overlay coords`
Expected: FAIL — `cannot find function map_crop_to_frame`.

- [ ] **Step 3: Implement `map_crop_to_frame`**

Add to `crates/rollshot-overlay/src/coords.rs` (above the `tests` module):

```rust
/// Map a crop rectangle from overlay logical coordinates to captured-frame
/// pixel coordinates, clamped to `source_size`. Implements spec P3.5.
pub fn map_crop_to_frame(crop: LogicalRect, overlay_logical: Size, source_size: Size) -> Region {
    if overlay_logical.width == 0 || overlay_logical.height == 0 {
        return Region { x: 0, y: 0, width: 0, height: 0 };
    }
    let scale_x = source_size.width as f32 / overlay_logical.width as f32;
    let scale_y = source_size.height as f32 / overlay_logical.height as f32;

    let x = (crop.x.max(0.0) * scale_x).round() as i64;
    let y = (crop.y.max(0.0) * scale_y).round() as i64;
    let w = (crop.width.max(0.0) * scale_x).round() as i64;
    let h = (crop.height.max(0.0) * scale_y).round() as i64;

    let sw = source_size.width as i64;
    let sh = source_size.height as i64;
    let x = x.clamp(0, sw);
    let y = y.clamp(0, sh);
    let w = w.clamp(0, sw - x);
    let h = h.clamp(0, sh - y);

    Region {
        x: x as i32,
        y: y as i32,
        width: w as u32,
        height: h as u32,
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `rtk cargo test -p rollshot-overlay coords`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-overlay/src/coords.rs crates/rollshot-overlay/src/lib.rs
rtk git commit -m "feat(overlay): crop->frame coordinate mapping with scale tests (R4)"
```

---

## Task 4: Driver core `stitch_stream` (P3.2) — TDD

**Files:**
- Create: `crates/rollshot-overlay/src/driver.rs`
- Modify: `crates/rollshot-overlay/src/lib.rs` (uncomment `mod driver;`)

- [ ] **Step 1: Write the failing test (in-memory frames via FakeFrameStream)**

Create `crates/rollshot-overlay/src/driver.rs`:

```rust
use rollshot_capture::{crop_frame, CaptureError, FrameStream, Region};
use rollshot_core::{StitchConfig, Stitcher};

use crate::CaptureResult;

/// StitchConfig the overlay uses (matches the Tauri app default,
/// session.rs:188-190).
pub fn overlay_stitch_config() -> StitchConfig {
    let mut config = StitchConfig::default();
    config.min_overlap = 32;
    config
}

#[cfg(test)]
mod tests {
    use super::{overlay_stitch_config, stitch_stream};
    use image::{Rgba, RgbaImage};
    use rollshot_capture::{CapturedFrame, FakeFrameStream, FrameMetadata, Region};
    use std::time::SystemTime;

    // A tall canvas; each frame is an 80x80 window scrolled down by `offset_y`.
    fn scrolling_frame(offset_y: u32) -> CapturedFrame {
        let (w, h) = (80u32, 200u32);
        let mut canvas = RgbaImage::from_pixel(w, h, Rgba([245, 245, 245, 255]));
        for y in (0..h).step_by(11) {
            for x in 8..w.saturating_sub(8) {
                let stripe = if (x / 5 + y / 7) % 2 == 0 { 220 } else { 180 };
                canvas.put_pixel(x, y, Rgba([(y % 180) as u8, stripe, 80, 255]));
                if y + 1 < h {
                    canvas.put_pixel(x, y + 1, Rgba([30, 30, 30, 255]));
                }
            }
        }
        let image = image::imageops::crop_imm(&canvas, 0, offset_y, 80, 80).to_image();
        CapturedFrame {
            image,
            timestamp: SystemTime::UNIX_EPOCH,
            metadata: FrameMetadata::fake(),
        }
    }

    #[test]
    fn stitch_stream_crops_and_finalizes() {
        let frames = vec![scrolling_frame(0), scrolling_frame(8), scrolling_frame(16)];
        let stream = Box::new(FakeFrameStream::new(frames));
        let region = Region { x: 0, y: 0, width: 60, height: 60 };

        let result = stitch_stream(stream, region, overlay_stitch_config())
            .expect("stitch produced a result");

        assert_eq!(result.image.width(), 60);
        assert!(result.image.height() >= 60, "stitched height grows past one frame");
        assert!(result.stats.frame_count >= 1);
    }
}
```

In `lib.rs`, uncomment `#[cfg(target_os = "linux")] mod driver;`.

- [ ] **Step 2: Run the test to verify it fails**

Run: `rtk cargo test -p rollshot-overlay driver`
Expected: FAIL — `cannot find function stitch_stream`.

- [ ] **Step 3: Implement `stitch_stream`**

Add to `crates/rollshot-overlay/src/driver.rs` (above `#[cfg(test)]`):

```rust
/// Crop+stitch a finite frame stream to completion. This is the tested core
/// the threaded live driver (Task 5) wraps. Mirrors the crop+push+finalize of
/// session.rs:199-212,214-231.
pub fn stitch_stream(
    mut stream: Box<dyn FrameStream>,
    region: Region,
    config: StitchConfig,
) -> Result<CaptureResult, String> {
    let mut stitcher = Stitcher::new(config);
    loop {
        match stream.next_frame() {
            Ok(frame) => {
                let cropped = crop_frame(&frame, region).map_err(|e| e.to_string())?;
                stitcher.push_frame(cropped.image);
            }
            Err(CaptureError::EndOfStream) => break,
            Err(CaptureError::Timeout { .. }) => continue,
            Err(err) => return Err(err.to_string()),
        }
    }
    let image = stitcher
        .full_image()
        .ok_or_else(|| "stitcher produced no output".to_string())?
        .clone();
    Ok(CaptureResult {
        image,
        stats: stitcher.stats(),
    })
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `rtk cargo test -p rollshot-overlay driver`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-overlay/src/driver.rs crates/rollshot-overlay/src/lib.rs
rtk git commit -m "feat(overlay): tested crop+stitch driver core (P3.2)"
```

---

## Task 5: Threaded live `Driver` (latest-wins + preview channel)

This wraps the tested `stitch_stream` logic into the live, stoppable form the
overlay needs. It is not unit-tested (it owns threads + a real backend + an
iced preview channel); correctness of the crop+stitch math is covered by Task 4,
and runtime behavior by Task 9. It mirrors `session.rs:374-561`.

> **Post-acceptance reorder (capture before overlay):** Task 9 KDE 6 acceptance
> found the portal screen-share picker dialog baked into frame 0 (top-left of
> the capture). The landed code therefore **splits** the single `Driver::start`
> shown below into `start_capture()` (backend + reader thread + first-frame
> `source_size`, run **before** the overlay in `run_overlay`) and
> `begin_stitch(crop_logical, overlay_logical)` (crop mapping + stitch thread, on
> crop-confirm), and adds `cancel()` (the driver is now live during selection).
> The `Driver::start(...)` signature in Step 1 below is the as-designed
> snapshot; the reader + stitch-loop + finalize internals are unchanged. See
> spec P3.2 + Data Flow and the landed `driver.rs`.

**Files:**
- Modify: `crates/rollshot-overlay/src/driver.rs`

- [ ] **Step 1: Add the threaded driver**

Append to `crates/rollshot-overlay/src/driver.rs`:

```rust
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use iced::futures::channel::mpsc::UnboundedSender;
use iced::widget::image::Handle as ImageHandle;
use rollshot_capture::{BackendKind, CaptureOptions, CapturedFrame, RegionMode, Size};

use crate::coords::LogicalRect;

struct Shared {
    latest: Mutex<Option<CapturedFrame>>,
    seq: AtomicU64,
    stitcher: Mutex<Stitcher>,
    error: Mutex<Option<String>>,
}

/// Live capture+stitch driver: a reader thread fills a latest-wins slot, a
/// stitch thread crops to `region` and pushes to the stitcher, emitting a
/// native-resolution preview viewport after each frame.
pub struct Driver {
    stop: Arc<AtomicBool>,
    shared: Arc<Shared>,
    reader: Option<JoinHandle<()>>,
    stitch: Option<JoinHandle<()>>,
}

impl Driver {
    /// Start capture, wait for the first frame to learn `source_size`, map the
    /// logical crop to frame pixels, then start stitching. `preview_tx` receives
    /// a native-resolution stitch preview viewport after each accepted frame.
    pub fn start(
        backend: &str,
        fps: u32,
        show_cursor: bool,
        crop_logical: LogicalRect,
        overlay_logical: Size,
        preview_tx: UnboundedSender<ImageHandle>,
    ) -> Result<Self, String> {
        let kind = BackendKind::from_cli_flag(backend).map_err(|e| e.to_string())?;
        let mut backend_impl = kind.create().map_err(|e| e.to_string())?;
        let mut stream = backend_impl
            .start(CaptureOptions {
                region: RegionMode::FullSource,
                fps,
                show_cursor,
                prefer_portal_region: false,
            })
            .map_err(|e| e.to_string())?;

        let shared = Arc::new(Shared {
            latest: Mutex::new(None),
            seq: AtomicU64::new(0),
            stitcher: Mutex::new(Stitcher::new(overlay_stitch_config())),
            error: Mutex::new(None),
        });
        let stop = Arc::new(AtomicBool::new(false));

        // Reader thread: latest-wins (session.rs:410-428).
        let reader = {
            let shared = Arc::clone(&shared);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match stream.next_frame() {
                        Ok(frame) => {
                            if let Ok(mut slot) = shared.latest.lock() {
                                *slot = Some(frame);
                                shared.seq.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        Err(CaptureError::EndOfStream) => break,
                        Err(CaptureError::Timeout { .. }) => continue,
                        Err(err) => {
                            if let Ok(mut e) = shared.error.lock() {
                                *e = Some(err.to_string());
                            }
                            break;
                        }
                    }
                }
            })
        };

        // Wait for the first frame so we can read source_size and map the crop.
        let source_size = wait_for_source_size(&shared, &stop, Duration::from_secs(5))?;
        let region = crate::coords::map_crop_to_frame(crop_logical, overlay_logical, source_size);
        let preview_size = Size {
            width: region.width,
            height: region.height,
        };

        // Stitch thread: on a new seq, crop+push, then emit preview
        // (session.rs:535-561).
        let stitch = {
            let shared = Arc::clone(&shared);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut last_seq = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    let seq = shared.seq.load(Ordering::Relaxed);
                    let frame = if seq == last_seq {
                        None
                    } else {
                        last_seq = seq;
                        shared.latest.lock().ok().and_then(|s| s.clone())
                    };
                    if let Some(frame) = frame {
                        let cropped = match crop_frame(&frame, region) {
                            Ok(c) => c,
                            Err(err) => {
                                if let Ok(mut e) = shared.error.lock() {
                                    *e = Some(err.to_string());
                                }
                                break;
                            }
                        };
                        if let Ok(mut stitcher) = shared.stitcher.lock() {
                            stitcher.push_frame(cropped.image);
                            if let Some(preview) = stitcher.full_image() {
                                let handle = preview_handle(preview, preview_size);
                                let _ = preview_tx.unbounded_send(handle);
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            })
        };

        Ok(Self {
            stop,
            shared,
            reader: Some(reader),
            stitch: Some(stitch),
        })
    }

    /// Stop both threads and produce the finalized capture.
    pub fn finalize(mut self) -> Result<CaptureResult, String> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.stitch.take() {
            let _ = h.join();
        }
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
        if let Ok(e) = self.shared.error.lock() {
            if let Some(msg) = e.as_ref() {
                return Err(msg.clone());
            }
        }
        let mut stitcher = self
            .shared
            .stitcher
            .lock()
            .map_err(|_| "stitcher lock poisoned".to_string())?;
        let image = stitcher
            .full_image()
            .ok_or_else(|| "stitcher produced no output".to_string())?
            .clone();
        Ok(CaptureResult {
            image,
            stats: stitcher.stats(),
        })
    }
}

/// Block until the reader stores the first frame, returning its `source_size`
/// (falling back to the frame's pixel dimensions if metadata omits it).
fn wait_for_source_size(
    shared: &Arc<Shared>,
    stop: &Arc<AtomicBool>,
    timeout: Duration,
) -> Result<Size, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(e) = shared.error.lock() {
            if let Some(msg) = e.as_ref() {
                return Err(msg.clone());
            }
        }
        if let Ok(slot) = shared.latest.lock() {
            if let Some(frame) = slot.as_ref() {
                return Ok(frame.metadata.source_size.unwrap_or(Size {
                    width: frame.image.width(),
                    height: frame.image.height(),
                }));
            }
        }
        if Instant::now() >= deadline {
            stop.store(true, Ordering::Relaxed);
            return Err("timed out waiting for first capture frame".to_string());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Wrap a native-resolution viewport of the stitcher's RGBA image as an iced
/// image handle. The UI may scale display size, but preview pixels are never
/// resampled before upload.
fn preview_handle(image: &image::RgbaImage, viewport: Size) -> ImageHandle {
    let width = image.width().min(viewport.width.max(1));
    let height = image.height().min(viewport.height.max(1));
    let x = image.width().saturating_sub(width);
    let y = image.height().saturating_sub(height);
    let viewport = image::imageops::crop_imm(image, x, y, width, height).to_image();
    ImageHandle::from_rgba(viewport.width(), viewport.height(), viewport.into_raw())
}
```

- [ ] **Step 2: Verify it builds and lints**

Run: `rtk cargo build -p rollshot-overlay`
Expected: compiles. (If `BackendKind::create()` / `from_cli_flag` signatures
differ, adapt to the real ones in `crates/rollshot-capture/src/backend.rs`.)

Run: `rtk cargo clippy -p rollshot-overlay --all-targets -- -D warnings`
Expected: clean.

Run: `rtk cargo test -p rollshot-overlay`
Expected: PASS (Task 3 + Task 4 tests still green).

- [ ] **Step 3: Commit**

```bash
rtk git add crates/rollshot-overlay/src/driver.rs
rtk git commit -m "feat(overlay): threaded live driver with preview channel"
```

---

## Task 6: Port the `iced_layershell` overlay UI (R3, R6)

Port `spikes/layershell-feasibility/src/overlay_app.rs` into
`crates/rollshot-overlay/src/overlay.rs`, then adapt it: real driver instead of
the RGB-cycling stub, crop-confirm → coords → `begin_stitch` (capture started
earlier in `run_overlay`, see Task 5 reorder note), Esc → finalize/cancel,
and the R3 chrome rules. The layer settings below are copied verbatim from the
spike (R6 PASS on KDE 6).

**Files:**
- Create: `crates/rollshot-overlay/src/overlay.rs`
- Modify: `crates/rollshot-overlay/src/lib.rs` (uncomment `mod overlay;`)

- [ ] **Step 1: Copy the spike file as the starting point**

Run: `rtk cp spikes/layershell-feasibility/src/overlay_app.rs crates/rollshot-overlay/src/overlay.rs`

- [ ] **Step 2: Replace the preview producer with the real driver**

In `overlay.rs`, the spike's `run()` took a preview `rx` fed by an RGB-cycling
thread. Change the model so the overlay owns:
- an `UnboundedSender`/`Receiver` pair created in `run()` (sender handed to
  `Driver::start`, receiver consumed by the existing `preview_stream`
  subscription — keep the spike's `PREVIEW_RX` static + `Subscription::run`
  pattern, which passed R6);
- the `OverlayConfig` (backend/fps/show_cursor) needed to start the driver.

Keep `subscription`, `preview_stream`, the `CropCanvas`, and the transparent
`style` exactly as in the spike.

- [ ] **Step 3: Wire crop-confirm → coords → driver start**

> **Reorder note:** capture is already live (started in `run_overlay` before the
> overlay), so `Message::Finish` does NOT call `Driver::start`. It calls
> `driver.begin_stitch(crop_logical, overlay_logical)` on the existing driver
> (in `DRIVER_SLOT`), which maps the crop using the `source_size` learned at
> `start_capture`. The `Driver::start(...)` call shown in this step is
> superseded; see spec P3.2.

On `Message::Finish`:
1. Capture the crop rectangle in overlay-logical pixels as a
   `crate::coords::LogicalRect` (`crop_logical`), and read the overlay's logical
   size (`overlay_logical: rollshot_capture::Size`) — for Phase 3 single-output
   the layer surface covers the output, so this is the surface size.
2. Start the driver (it captures the first frame, learns `source_size`, maps the
   crop internally, and begins stitching):

```rust
let driver = crate::driver::Driver::start(
    &cfg.backend, cfg.fps, cfg.show_cursor,
    crop_logical, overlay_logical, preview_tx,
).map_err(OverlayError::Capture)?;
// store `driver` in overlay state for finalize on Esc
```

3. Emit `SetInputRegion` so only the toolbar stays interactive (spike R6).

The crop→`source_size` circularity is resolved **inside** `Driver::start`
(Task 5): it waits for the first frame, reads `FrameMetadata.source_size`
(falling back to the frame's pixel size), and maps the crop with
`coords::map_crop_to_frame` before starting the stitch thread. The overlay only
ever passes logical coordinates.

- [ ] **Step 4: Esc → finalize → store result; Cancel → None**

Replace the spike's `std::process::exit(0)` (BANNED — P3.3): on Esc, call
`driver.finalize()`, store the `Result<CaptureResult, _>` in a shared slot the
`run()` function reads, then request the clean event-loop exit (Task 7 wires the
exact exit action). On `Message::Cancel` (or Esc before any region is
confirmed), store `Ok(None)` and request exit.

> **Reorder note:** because the driver is now live during selection, Cancel /
> Esc-before-confirm also calls `driver.cancel()` to tear down the reader thread
> + PipeWire stream before exiting; Esc with a confirmed crop calls
> `driver.finalize()` as described. See spec P3.2.

- [ ] **Step 5: R3 — draw nothing inside the crop region during capture (P3.4)**

In `view()`, once `crop_confirmed` is true (scrolling phase):
- do NOT draw the crop selection border (the `CropCanvas` stroke);
- position the live-preview `image` widget and the toolbar **outside** the
  confirmed crop rectangle (e.g. anchored to a screen edge the crop does not
  cover);
- keep the crop interior fully transparent.

The capture pipeline crops to the region before stitching, so chrome outside the
region is excluded automatically (spec P3.4). If the confirmed region fills the
output (no room), hide the preview during capture.

In `lib.rs`, uncomment `#[cfg(target_os = "linux")] mod overlay;`.

- [ ] **Step 6: Verify it builds and lints**

Run: `rtk cargo build -p rollshot-overlay`
Expected: compiles.

Run: `rtk cargo clippy -p rollshot-overlay --all-targets -- -D warnings`
Expected: clean.

Run: `rtk cargo fmt --check`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-overlay/src/overlay.rs crates/rollshot-overlay/src/lib.rs
rtk git commit -m "feat(overlay): port layer-shell UI, wire driver + R3 chrome rules"
```

---

## Task 7: `overlay::run` entry + clean event-loop exit (P3.3)

**Files:**
- Modify: `crates/rollshot-overlay/src/overlay.rs`

- [ ] **Step 1: Confirm the clean-exit action in iced_layershell**

The spike used `std::process::exit(0)`; that is banned here. Find the
layer-shell loop-exit / window-close action.

Run: `rtk grep -rniE "fn run|close|exit|AppRunOutcome|RemoveWindow|finish" learn-projects/exwlshelleventloop/iced_layershell/src/build_pattern/application.rs`
And: `rtk grep -rniE "to_layer_message|enum LayerShellActions|Close|Exit" learn-projects/exwlshelleventloop/iced_layershell/src/`
Expected: identifies how a running app exits its loop cleanly (e.g. a
close/exit action emitted as a `Task` from `update`, or `.run()` returning when
the last window closes). Use the actual mechanism it shows.

**Gate:** if no clean exit mechanism exists, STOP and record it in
`crates/rollshot-overlay/NOTES.md` as a Phase 3 blocker — do NOT fall back to
`process::exit`.

- [ ] **Step 2: Implement `pub fn run(config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError>`**

Wrap the iced application build (the spike's `application(...).settings(...).run()`)
so that:
1. it creates the preview channel and a shared result slot
   (`Arc<Mutex<Option<Result<Option<CaptureResult>, String>>>>`);
2. it runs the iced app (blocks);
3. after `.run()` returns, it reads the slot:
   - `Some(Ok(Some(result)))` → `Ok(Some(result))`
   - `Some(Ok(None))` → `Ok(None)` (cancelled)
   - `Some(Err(msg))` → `Err(OverlayError::Capture(msg))`
   - `None` (loop exited without setting it) → `Ok(None)`
4. map any `iced_layershell::Error` from `.run()` to `OverlayError::Overlay`.

- [ ] **Step 3: Verify it builds**

Run: `rtk cargo build -p rollshot-overlay`
Expected: compiles.

Run: `rtk cargo test -p rollshot-overlay`
Expected: PASS (coords + driver tests still green).

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-overlay/src/overlay.rs
rtk git commit -m "feat(overlay): run() entry + clean iced exit, no process::exit (P3.3)"
```

---

## Task 8: Harness binary

**Files:**
- Create: `crates/rollshot-overlay/src/bin/capture_overlay.rs`

- [ ] **Step 1: Write the harness**

`crates/rollshot-overlay/src/bin/capture_overlay.rs`:

```rust
//! Standalone harness for the Phase 3 KDE 6 acceptance checks. Stands in for
//! Tauri: runs the overlay, then saves the finalized image as a PNG.

use rollshot_overlay::{run_overlay, OverlayConfig};

fn main() {
    let backend = std::env::args().nth(1).unwrap_or_else(|| "auto".to_string());
    let config = OverlayConfig {
        backend,
        fps: 5,
        show_cursor: false,
    };

    match run_overlay(config) {
        Ok(Some(result)) => {
            let out = "capture_overlay_result.png";
            match result.image.save(out) {
                Ok(()) => println!(
                    "saved {out}: {}x{} ({} frames)",
                    result.image.width(),
                    result.image.height(),
                    result.stats.frame_count
                ),
                Err(e) => eprintln!("failed to save {out}: {e}"),
            }
        }
        Ok(None) => println!("cancelled"),
        Err(e) => eprintln!("overlay failed: {e}"),
    }
}
```

- [ ] **Step 2: Verify it builds**

Run: `rtk cargo build -p rollshot-overlay --bin capture_overlay`
Expected: compiles.

- [ ] **Step 3: Commit**

```bash
rtk git add crates/rollshot-overlay/src/bin/capture_overlay.rs
rtk git commit -m "feat(overlay): harness binary for KDE 6 acceptance"
```

---

## Task 9: KDE 6 Wayland acceptance (GATE — manual)

The layer-shell surface cannot be unit-tested; this task is the runtime gate,
run on a KDE 6 Wayland session. Record every result in
`crates/rollshot-overlay/NOTES.md`.

> **Acceptance finding (resolved by the reorder):** the first acceptance run
> captured the portal screen-share **picker dialog** into the top-left of the
> saved PNG — it bled into frame 0, which the stitcher bakes as the canvas base.
> Fix: the capture-before-overlay reorder (spec P3.2; `start_capture` runs before
> the overlay, `begin_stitch` on confirm). Re-run after the reorder to confirm
> the picker no longer appears in the output, then record results below.

**Files:**
- Create: `crates/rollshot-overlay/NOTES.md`

- [ ] **Step 1: Run the harness**

Run: `rtk cargo run -p rollshot-overlay --bin capture_overlay`

- [ ] **Step 2: Roadmap Phase 3 acceptance checks** (record PASS/FAIL each):
  1. Overlay appears above fullscreen apps.
  2. A crop region is selectable.
  3. The target content scrolls while stitching is active.
  4. The live stitching preview updates during scrolling.
  5. Esc finishes stitching; the harness saves a PNG matching the scrolled
     content (the handoff fired).

- [ ] **Step 3: Carried-over runtime checks** (record results):
  - **R3 self-capture:** scan the saved PNG / a captured frame for the sentinel
    color **inside the mapped crop region only** → must be 0. (Whole-frame is
    not the question — see FINDINGS R3 caveat.) Record the count.
  - **R4 scaling:** repeat at KDE display scale **100%** and **150%**; confirm
    the saved region matches the on-screen selection with no offset. Record any
    pixel offset.
  - **R5/R7:** single-output is the verified path; if a multi-monitor setup is
    available, confirm the overlay anchors to the captured output; otherwise
    document as a Phase 4 follow-up.

- [ ] **Step 4: Commit the recorded results**

```bash
rtk git add crates/rollshot-overlay/NOTES.md
rtk git commit -m "test(overlay): KDE 6 acceptance results (Phase 3 GATE)"
```

---

## Task 10: Full verify + roadmap handoff to Phase 4

**Files:**
- Modify: `docs/linux-wayland-layer-shell-roadmap.md`

- [ ] **Step 1: Workspace-wide verification**

Run: `rtk cargo test`
Expected: PASS (workspace).

Run: `rtk cargo fmt --check`
Expected: clean.

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 2: Update the roadmap Phase 3 status + Phase 4 carryover**

In `docs/linux-wayland-layer-shell-roadmap.md`, mark Phase 3 done and record the
Phase 4 carryover under Phase 3's existing "Carried over" list: (a) `src-tauri`
Linux branch spawns `run_overlay` on a thread and feeds `CaptureResult.image`
into `AppSession`'s final-image + save flow; (b) the Tauri save dialog handoff
(D5); (c) **R2:** hide / de-focus the Tauri host window during the overlay phase
(FINDINGS §5); (d) any R5/R7 multi-output follow-up from Task 9.

- [ ] **Step 3: Commit**

```bash
rtk git add docs/linux-wayland-layer-shell-roadmap.md
rtk git commit -m "docs(roadmap): Phase 3 overlay done; carry Tauri wiring + R2 into Phase 4"
```

---

## Success Criteria

- `rollshot-overlay` builds in the workspace (Linux real; non-Linux stub).
- `coords` (5 tests) + `driver` core (1+ test) pass in CI without KDE.
- `rollshot-capture` D4 change is permanent; its tests pass; picker is
  monitor-only.
- On KDE 6: crop → scroll → live preview → Esc → `run_overlay` returns a
  `CaptureResult`; harness saves a correct PNG (roadmap Phase 3 flow).
- R3 sentinel count inside the crop region is 0; R4 correct at 100% and 150%.
- No `std::process::exit` in the crate; no Tauri dependency; `cmd_capture.rs` /
  `session.rs` untouched.
