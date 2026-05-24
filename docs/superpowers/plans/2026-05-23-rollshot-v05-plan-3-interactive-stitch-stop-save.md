# rollshot v0.5 Plan 3: Interactive Stitch, Stop, and Save Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete the interactive capture workflow so a selected GUI region is cropped, stitched while the user scrolls, stopped on user action, previewed as a final image, and saved as a PNG.

**Architecture:** Keep `rollshot-app` as the owner of interactive capture state. The Rust Tauri session keeps the latest full-resolution frame, selected source-pixel region, stitch worker, stitch stats, and final PNG image; the React UI only starts/stops commands, polls status, displays previews, and asks the user for a save path with Tauri's dialog plugin.

**Tech Stack:** Rust 2021, Tauri v2, `rollshot-capture`, `rollshot-core`, `image`, React, TypeScript, Vite, Vitest, shadcn/ui, Lucide React, Tauri dialog plugin.

---

## Source Spec

Plan 3 implements only this section of the replacement spec:

```text
Plan 3: Interactive Stitch, Stop, Save, and Minimal Polish

- crop selected region
- run stitch loop under GUI session state
- stop on user action
- keep final image in backend state
- save PNG
- add only the minimal polish needed to complete and verify the workflow
```

Source: `docs/superpowers/specs/2026-05-23-rollshot-v05-interactive-capture-replacement-design.md`

Reference checked: `learn-projects/snow-shot` uses Tauri `Response::new(Vec<u8>)` for binary image IPC and enables `tauri-plugin-dialog` plus `dialog:allow-save` for frontend save flows. Rollshot should reuse those two patterns, not snow-shot's larger scroll-screenshot service stack.

Coordinate references checked:

- `learn-projects/snow-shot/src-tauri/src-crates/app-utils/src/monitor_info.rs`
  - Converts macOS monitor bounds from logical points into physical pixels with `monitor.scale_factor()`.
  - Clips global crop regions to a monitor, then converts them to monitor-local crop pixels.
- `learn-projects/snow-shot/src-tauri/src-crates/app-utils/src/lib.rs`
  - Checks macOS monitor scale-factor consistency before cross-monitor capture.
  - Sends pixel crop rectangles into platform capture APIs.
- `learn-projects/obs-studio/libobs/obs-scene.c`
  - Treats crop values as source-local pixels after applying item/canvas scale.
  - Rescales existing crop values when source dimensions change.

Rollshot's v0.5 interactive crop should follow the same principle: UI/logical coordinates are temporary input only; the persisted region is source-frame pixels, validated against the actual captured frame dimensions.

## Assumptions

- Plan 1 is implemented: `rollshot capture` launches `rollshot-app --capture <json>`, and `rollshot capture --headless --output out.png ...` still runs the CLI path.
- Plan 2 is implemented: `crates/rollshot-app` is a Tauri app with `start_capture`, `stop_capture`, `session_status`, `confirm_region`, and `get_latest_preview` commands.
- The current Plan 2 app stores `latest_frame` and `selected_region` in `crates/rollshot-app/src-tauri/src/session.rs`; Plan 3 extends that file instead of introducing a new session framework.
- Plan 2 did not prove that the Linux full-screen fallback is required. This plan keeps controls in the existing side panel and does not add a keyboard-only or window-exclusion fallback.
- Clipboard support is out of scope for Plan 3. Save-to-file is the only result action.
- Interactive crop coordinates are platform-neutral after preview conversion. Linux and macOS both crop `CapturedFrame.image` pixels in Rust; neither backend receives the GUI-selected region as a platform capture crop in v0.5.
- Headless/manual backend crop remains backend-specific and is not used by this interactive Plan 3 flow. On Linux, manual crop is already documented as post-portal-VideoCrop frame coordinates; on macOS, existing `scap` manual crop coordinate ambiguity remains outside this plan.

## File Structure

Modify:

- `crates/rollshot-capture/src/lib.rs`
  - Re-export the new crop helper.

- `crates/rollshot-capture/src/types.rs`
  - No behavior change unless the implementation chooses to colocate helper tests here. Prefer a new `crop.rs`.

- `crates/rollshot-app/src-tauri/Cargo.toml`
  - Add `rollshot-core` and `tauri-plugin-dialog`.

- `crates/rollshot-app/src-tauri/capabilities/default.json`
  - Allow save dialogs.

- `crates/rollshot-app/src-tauri/src/lib.rs`
  - Register dialog plugin and new stitch/save commands.

- `crates/rollshot-app/src-tauri/src/commands.rs`
  - Add `start_stitching`, `save_image`, and `get_final_preview`.

- `crates/rollshot-app/src-tauri/src/session.rs`
  - Add stitching state, stitch worker lifecycle, final image storage, final preview encoding, PNG save, and unit tests.

- `crates/rollshot-app/package.json`
  - Add `@tauri-apps/plugin-dialog`.

- `crates/rollshot-app/src/api/capture.ts`
  - Add command wrappers and status types.

- `crates/rollshot-app/src/api/capture.test.ts`
  - Add wrapper tests for save and binary final preview handling.

- `crates/rollshot-app/src/region/geometry.ts`
  - Tighten preview CSS rectangle to source-frame pixel conversion so it preserves selected bounds with floor/ceil edge mapping.

- `crates/rollshot-app/src/region/geometry.test.ts`
  - Add fractional-scale and edge-preservation tests.

- `crates/rollshot-app/src/components/RegionOverlay.tsx`
  - Measure pointer coordinates relative to the rendered image, not the outer wrapper.

- `crates/rollshot-app/src/App.tsx`
  - Add Start Stitching, Stop, Save, and final-preview UI states.

- `crates/rollshot-app/src/App.css`
  - Add minimal status/stats/final-preview styles.

Create:

- `crates/rollshot-capture/src/crop.rs`
  - Safe source-pixel frame crop helper with bounds validation.

## Task 1: Lock The Interactive Coordinate-Space Contract

**Files:**
- Modify: `crates/rollshot-app/src/region/geometry.ts`
- Modify: `crates/rollshot-app/src/region/geometry.test.ts`
- Modify: `crates/rollshot-app/src/components/RegionOverlay.tsx`
- Modify: `crates/rollshot-app/src/App.css`

- [ ] **Step 1: Add failing edge-preservation tests**

Append these tests to `crates/rollshot-app/src/region/geometry.test.ts`:

```ts
  it('preserves fractional CSS edges by flooring origin and ceiling far edge', () => {
    const cssRect: CssRect = { left: 10.25, top: 4.5, width: 20.25, height: 10.25 }
    expect(
      cssRectToSourceRegion(cssRect, {
        renderedWidth: 333,
        renderedHeight: 222,
        sourceWidth: 1000,
        sourceHeight: 666,
      }),
    ).toEqual({ x: 30, y: 13, width: 62, height: 31 })
  })

  it('maps a full rendered preview exactly to the full source frame', () => {
    expect(
      cssRectToSourceRegion(
        { left: 0, top: 0, width: 511.5, height: 287.75 },
        {
          renderedWidth: 511.5,
          renderedHeight: 287.75,
          sourceWidth: 2560,
          sourceHeight: 1440,
        },
      ),
    ).toEqual({ x: 0, y: 0, width: 2560, height: 1440 })
  })
```

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- src/region/geometry.test.ts
```

Expected: FAIL because the current conversion uses `Math.round` independently for origin and size, which can shrink fractional selections.

- [ ] **Step 2: Implement edge-based source-pixel conversion**

Replace `cssRectToSourceRegion` in `crates/rollshot-app/src/region/geometry.ts`:

```ts
export function cssRectToSourceRegion(
  rect: CssRect,
  scale: PreviewScale,
): SourceRegion {
  const xScale = scale.sourceWidth / scale.renderedWidth
  const yScale = scale.sourceHeight / scale.renderedHeight
  const left = Math.floor(rect.left * xScale)
  const top = Math.floor(rect.top * yScale)
  const right = Math.ceil((rect.left + rect.width) * xScale)
  const bottom = Math.ceil((rect.top + rect.height) * yScale)

  return clampSourceRegion(
    {
      x: left,
      y: top,
      width: right - left,
      height: bottom - top,
    },
    { width: scale.sourceWidth, height: scale.sourceHeight },
  )
}
```

- [ ] **Step 3: Measure pointer coordinates against the rendered image**

In `crates/rollshot-app/src/components/RegionOverlay.tsx`, replace `localPoint` with:

```ts
  function localPoint(event: PointerEvent<HTMLDivElement>): Point {
    const image = imageRef.current
    if (!image) {
      return { x: 0, y: 0 }
    }

    const bounds = image.getBoundingClientRect()
    return {
      x: Math.max(0, Math.min(event.clientX - bounds.left, bounds.width)),
      y: Math.max(0, Math.min(event.clientY - bounds.top, bounds.height)),
    }
  }
```

In `publishRegion`, use the same image bounds:

```ts
    const bounds = image.getBoundingClientRect()
    onRegionChange(
      cssRectToSourceRegion(nextRect, {
        renderedWidth: bounds.width,
        renderedHeight: bounds.height,
        sourceWidth,
        sourceHeight,
      }),
    )
```

- [ ] **Step 4: Remove layout-affecting preview borders**

In `crates/rollshot-app/src/App.css`, change `.preview-image`:

```css
.preview-image {
  display: block;
  max-width: 100%;
  max-height: calc(100vh - 48px);
  object-fit: contain;
  border: 0;
  box-shadow: 0 0 0 1px #8b95a5;
  background: #ffffff;
}
```

This keeps a visible outline without changing the coordinate box used for selection math.

- [ ] **Step 5: Verify coordinate tests**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- src/region/geometry.test.ts
```

Expected: PASS.

- [ ] **Step 6: Commit Task 1**

Run:

```bash
rtk git add crates/rollshot-app/src/region/geometry.ts crates/rollshot-app/src/region/geometry.test.ts crates/rollshot-app/src/components/RegionOverlay.tsx crates/rollshot-app/src/App.css
rtk git commit -m "fix(app): preserve source-pixel region conversion"
```

## Task 2: Add A Shared Frame Crop Helper

**Files:**
- Create: `crates/rollshot-capture/src/crop.rs`
- Modify: `crates/rollshot-capture/src/lib.rs`

- [ ] **Step 1: Write failing crop tests**

Create `crates/rollshot-capture/src/crop.rs`:

```rust
use crate::{CapturedFrame, CaptureError, FrameMetadata, Region};

pub fn crop_frame(frame: &CapturedFrame, _region: Region) -> Result<CapturedFrame, CaptureError> {
    Ok(frame.clone())
}

#[cfg(test)]
mod tests {
    use super::crop_frame;
    use crate::{CapturedFrame, FrameMetadata, PixelFormat, Region, Size};
    use image::{Rgba, RgbaImage};
    use std::time::SystemTime;

    fn test_frame() -> CapturedFrame {
        let mut image = RgbaImage::new(4, 3);
        for y in 0..3 {
            for x in 0..4 {
                image.put_pixel(x, y, Rgba([x as u8, y as u8, 200, 255]));
            }
        }

        CapturedFrame {
            image,
            timestamp: SystemTime::UNIX_EPOCH,
            metadata: FrameMetadata {
                source_size: Some(Size { width: 4, height: 3 }),
                effective_region: None,
                pixel_format: Some(PixelFormat::Rgba),
                stride: Some(16),
                backend: "fake",
            },
        }
    }

    #[test]
    fn crop_frame_returns_selected_source_pixels() {
        let cropped = crop_frame(
            &test_frame(),
            Region {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            },
        )
        .expect("crop succeeds");

        assert_eq!(cropped.image.dimensions(), (2, 2));
        assert_eq!(*cropped.image.get_pixel(0, 0), Rgba([1, 1, 200, 255]));
        assert_eq!(*cropped.image.get_pixel(1, 1), Rgba([2, 2, 200, 255]));
        assert_eq!(
            cropped.metadata.effective_region,
            Some(Region {
                x: 1,
                y: 1,
                width: 2,
                height: 2
            })
        );
        assert_eq!(cropped.metadata.stride, Some(8));
    }

    #[test]
    fn crop_frame_rejects_negative_origin() {
        let err = crop_frame(
            &test_frame(),
            Region {
                x: -1,
                y: 0,
                width: 2,
                height: 2,
            },
        )
        .expect_err("negative origin rejected");

        assert!(err.to_string().contains("non-negative"), "err = {err}");
    }

    #[test]
    fn crop_frame_rejects_out_of_bounds_region() {
        let err = crop_frame(
            &test_frame(),
            Region {
                x: 3,
                y: 1,
                width: 2,
                height: 2,
            },
        )
        .expect_err("outside region rejected");

        assert!(err.to_string().contains("outside frame bounds"), "err = {err}");
    }
}
```

- [ ] **Step 2: Register the module and run the failing tests**

Edit `crates/rollshot-capture/src/lib.rs`:

```rust
pub mod backend;
pub mod crop;
pub mod error;
pub mod fake;
pub mod fixture;
pub mod types;
```

Run:

```bash
rtk cargo test -p rollshot-capture crop_frame
```

Expected: FAIL because `crop_frame` returns the original frame instead of cropping.

- [ ] **Step 3: Implement the crop helper**

Replace the stub in `crates/rollshot-capture/src/crop.rs` with:

```rust
use image::RgbaImage;

use crate::{CapturedFrame, CaptureError, FrameMetadata, Region};

pub fn crop_frame(frame: &CapturedFrame, region: Region) -> Result<CapturedFrame, CaptureError> {
    if region.x < 0 || region.y < 0 || region.width == 0 || region.height == 0 {
        return Err(CaptureError::InvalidConfig {
            message: format!(
                "crop region x={},y={},w={},h={} must have non-negative origin and non-zero size",
                region.x, region.y, region.width, region.height
            ),
        });
    }

    let right = region.x as u64 + region.width as u64;
    let bottom = region.y as u64 + region.height as u64;
    if right > frame.image.width() as u64 || bottom > frame.image.height() as u64 {
        return Err(CaptureError::InvalidConfig {
            message: format!(
                "crop region x={},y={},w={},h={} is outside frame bounds {}x{}",
                region.x,
                region.y,
                region.width,
                region.height,
                frame.image.width(),
                frame.image.height()
            ),
        });
    }

    let cropped = image::imageops::crop_imm(
        &frame.image,
        region.x as u32,
        region.y as u32,
        region.width,
        region.height,
    )
    .to_image();

    let mut metadata: FrameMetadata = frame.metadata.clone();
    metadata.effective_region = Some(region);
    metadata.stride = Some(region.width.saturating_mul(4));

    Ok(CapturedFrame {
        image: RgbaImage::from(cropped),
        timestamp: frame.timestamp,
        metadata,
    })
}
```

- [ ] **Step 4: Re-export the helper**

Edit `crates/rollshot-capture/src/lib.rs`:

```rust
pub use crop::crop_frame;
```

- [ ] **Step 5: Verify crop tests pass**

Run:

```bash
rtk cargo test -p rollshot-capture crop_frame
```

Expected: PASS.

- [ ] **Step 6: Commit Task 2**

Run:

```bash
rtk git add crates/rollshot-capture/src/lib.rs crates/rollshot-capture/src/crop.rs
rtk git commit -m "feat(capture): add frame crop helper"
```

## Task 3: Add Rust Session Stitching State

**Files:**
- Modify: `crates/rollshot-app/src-tauri/Cargo.toml`
- Modify: `crates/rollshot-app/src-tauri/src/session.rs`

- [ ] **Step 1: Add the app dependency on the stitcher**

Edit `crates/rollshot-app/src-tauri/Cargo.toml`:

```toml
[dependencies]
image = { workspace = true }
rollshot-capture = { path = "../../rollshot-capture" }
rollshot-core = { path = "../../rollshot-core" }
serde = { workspace = true }
serde_json = { workspace = true }
tauri = { version = "2", features = [] }
thiserror = { workspace = true }
```

- [ ] **Step 2: Add failing session tests for stitch lifecycle**

Append these tests inside the existing `#[cfg(test)] mod tests` in `crates/rollshot-app/src-tauri/src/session.rs`:

```rust
    fn scrolling_frame(y_offset: u8) -> CapturedFrame {
        let mut image = RgbaImage::new(80, 80);
        for y in 0..80 {
            for x in 0..80 {
                image.put_pixel(x, y, Rgba([x as u8, y.wrapping_add(y_offset as u32) as u8, 90, 255]));
            }
        }
        CapturedFrame {
            image,
            timestamp: SystemTime::UNIX_EPOCH,
            metadata: FrameMetadata::fake(),
        }
    }

    #[test]
    fn start_stitching_requires_confirmed_region() {
        let mut session = AppSession::new();
        session.store_frame_for_test(make_test_frame(320, 200));

        let err = session.start_stitching_for_test().expect_err("missing region rejected");

        assert!(err.contains("confirm a region"), "err = {err}");
    }

    #[test]
    fn push_stitch_frame_crops_to_selected_region_and_updates_stats() {
        let mut session = AppSession::new();
        session.store_frame_for_test(scrolling_frame(0));
        session
            .confirm_region(RegionDto {
                x: 10,
                y: 10,
                width: 60,
                height: 60,
            })
            .expect("confirm region");
        session.start_stitching_for_test().expect("start stitching");

        session
            .push_stitch_frame_for_test(scrolling_frame(0))
            .expect("first frame");
        session
            .push_stitch_frame_for_test(scrolling_frame(8))
            .expect("second frame");

        let status = session.status();
        match status {
            SessionStatus::Stitching { stats, .. } => {
                assert_eq!(stats.frame_count, 2);
                assert_eq!(stats.total_width, 60);
                assert!(stats.total_height >= 60);
            }
            other => panic!("expected stitching status, got {other:?}"),
        }
    }

    #[test]
    fn finish_stitching_keeps_final_image_in_session() {
        let mut session = AppSession::new();
        session.store_frame_for_test(scrolling_frame(0));
        session
            .confirm_region(RegionDto {
                x: 0,
                y: 0,
                width: 80,
                height: 80,
            })
            .expect("confirm region");
        session.start_stitching_for_test().expect("start stitching");
        session
            .push_stitch_frame_for_test(scrolling_frame(0))
            .expect("first frame");

        let done = session.finish_stitching_for_test().expect("finish");

        assert_eq!(done.image_width, 80);
        assert_eq!(done.image_height, 80);
        assert!(session.final_image_png_for_test().is_some());
    }
```

Run:

```bash
rtk cargo test -p rollshot-app session::tests::start_stitching_requires_confirmed_region
```

Expected: FAIL because `start_stitching_for_test` and the `Stitching` / `Done` states do not exist yet.

- [ ] **Step 3: Add serializable stitch status types**

In `crates/rollshot-app/src-tauri/src/session.rs`, change the imports near the top to:

```rust
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::JoinHandle;
use std::time::Duration;

use image::RgbaImage;
use rollshot_capture::{
    crop_frame, BackendKind, CaptureOptions, CapturedFrame, InteractiveLaunchOptions, Region,
    RegionMode,
};
use rollshot_core::{StitchConfig, StitchOutcome, StitchStats, Stitcher};
```

Extend `SessionStatus`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionStatus {
    Idle,
    Previewing {
        frame_width: u32,
        frame_height: u32,
        region: Option<RegionDto>,
    },
    Stitching {
        frame_width: u32,
        frame_height: u32,
        region: RegionDto,
        stats: StitchStatsDto,
        last_outcome: Option<String>,
    },
    Done {
        image_width: u32,
        image_height: u32,
        output_path: Option<String>,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct StitchStatsDto {
    pub frame_count: u32,
    pub total_width: u32,
    pub total_height: u32,
    pub last_append: u32,
}

impl From<StitchStats> for StitchStatsDto {
    fn from(value: StitchStats) -> Self {
        Self {
            frame_count: value.frame_count,
            total_width: value.total_width,
            total_height: value.total_height,
            last_append: value.last_append,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DoneImageDto {
    pub image_width: u32,
    pub image_height: u32,
    pub output_path: Option<String>,
}
```

- [ ] **Step 4: Extend `AppSession` fields**

Replace the current `AppSession` struct with:

```rust
#[derive(Default)]
pub struct AppSession {
    latest_frame: Option<CapturedFrame>,
    latest_frame_seq: u64,
    selected_region: Option<Region>,
    stitcher: Option<Stitcher>,
    stitch_stats: StitchStatsDto,
    last_stitch_outcome: Option<String>,
    final_image: Option<RgbaImage>,
    output_path: Option<String>,
    error: Option<String>,
}
```

In the reset block inside `SharedSession::start_capture`, clear the new fields:

```rust
inner.latest_frame = None;
inner.latest_frame_seq = 0;
inner.selected_region = None;
inner.stitcher = None;
inner.stitch_stats = StitchStatsDto::from(StitchStats::default());
inner.last_stitch_outcome = None;
inner.final_image = None;
inner.output_path = None;
inner.error = None;
```

In the reader thread where `inner.latest_frame = Some(frame);` is currently assigned, increment the sequence:

```rust
inner.latest_frame = Some(frame);
inner.latest_frame_seq = inner.latest_frame_seq.wrapping_add(1);
inner.error = None;
```

- [ ] **Step 5: Implement pure session stitching methods**

Add these private methods to `impl AppSession`:

```rust
    fn start_stitching(&mut self) -> Result<(), String> {
        if self.selected_region.is_none() {
            return Err("confirm a region before starting stitching".to_string());
        }
        self.stitcher = Some(Stitcher::new(StitchConfig::default()));
        self.stitch_stats = StitchStatsDto::from(StitchStats::default());
        self.last_stitch_outcome = None;
        self.final_image = None;
        self.output_path = None;
        self.error = None;
        Ok(())
    }

    fn push_stitch_frame(&mut self, frame: CapturedFrame) -> Result<(), String> {
        let region = self
            .selected_region
            .ok_or_else(|| "confirm a region before stitching frames".to_string())?;
        let stitcher = self
            .stitcher
            .as_mut()
            .ok_or_else(|| "stitching has not started".to_string())?;
        let cropped = crop_frame(&frame, region).map_err(|err| err.to_string())?;
        let outcome = stitcher.push_frame(cropped.image);
        self.last_stitch_outcome = Some(format_stitch_outcome(&outcome));
        self.stitch_stats = stitcher.stats().into();
        Ok(())
    }

    fn finish_stitching(&mut self) -> Result<DoneImageDto, String> {
        let stitcher = self
            .stitcher
            .take()
            .ok_or_else(|| "stitching has not started".to_string())?;
        let image = stitcher
            .full_image()
            .ok_or_else(|| "stitcher produced no output".to_string())?
            .clone();
        let done = DoneImageDto {
            image_width: image.width(),
            image_height: image.height(),
            output_path: self.output_path.clone(),
        };
        self.final_image = Some(image);
        self.stitch_stats = stitcher.stats().into();
        Ok(done)
    }

```

Append these test-only proxies to the existing `#[cfg(test)] impl AppSession` block:

```rust
    pub fn start_stitching_for_test(&mut self) -> Result<(), String> {
        self.start_stitching()
    }

    pub fn push_stitch_frame_for_test(&mut self, frame: CapturedFrame) -> Result<(), String> {
        self.push_stitch_frame(frame)
    }

    pub fn finish_stitching_for_test(&mut self) -> Result<DoneImageDto, String> {
        self.finish_stitching()
    }

    pub fn final_image_png_for_test(&self) -> Option<Vec<u8>> {
        self.final_image
            .as_ref()
            .and_then(|image| encode_rgba_png(image).ok())
    }
```

Add these helper functions below `encode_preview_png`:

```rust
#[cfg(test)]
fn encode_rgba_png(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|err| format!("failed to encode png: {err}"))?;
    Ok(cursor.into_inner())
}

fn format_stitch_outcome(outcome: &StitchOutcome) -> String {
    match outcome {
        StitchOutcome::FirstFrame => "first frame".to_string(),
        StitchOutcome::Appended { direction, added, .. } => {
            format!("appended {added}px {direction:?}")
        }
        StitchOutcome::NoProgress { .. } => "no progress".to_string(),
        StitchOutcome::Duplicate => "duplicate frame".to_string(),
        StitchOutcome::NoMatch { reason, .. } => format!("no match: {reason:?}"),
        StitchOutcome::AxisChanged {
            previous_axis,
            new_axis,
            ..
        } => format!("axis changed from {previous_axis:?} to {new_axis:?}"),
    }
}
```

- [ ] **Step 6: Update status reporting**

Replace `AppSession::status` with:

```rust
    pub fn status(&self) -> SessionStatus {
        if let Some(message) = &self.error {
            return SessionStatus::Failed {
                message: message.clone(),
            };
        }

        if let Some(image) = &self.final_image {
            return SessionStatus::Done {
                image_width: image.width(),
                image_height: image.height(),
                output_path: self.output_path.clone(),
            };
        }

        match (&self.latest_frame, self.selected_region) {
            (Some(frame), Some(region)) if self.stitcher.is_some() => SessionStatus::Stitching {
                frame_width: frame.image.width(),
                frame_height: frame.image.height(),
                region: RegionDto {
                    x: region.x,
                    y: region.y,
                    width: region.width,
                    height: region.height,
                },
                stats: self.stitch_stats,
                last_outcome: self.last_stitch_outcome.clone(),
            },
            (Some(frame), region) => SessionStatus::Previewing {
                frame_width: frame.image.width(),
                frame_height: frame.image.height(),
                region: region.map(|region| RegionDto {
                    x: region.x,
                    y: region.y,
                    width: region.width,
                    height: region.height,
                }),
            },
            (None, _) => SessionStatus::Idle,
        }
    }
```

- [ ] **Step 7: Verify focused session tests pass**

Run:

```bash
rtk cargo test -p rollshot-app session::tests
```

Expected: PASS for the existing session tests and the new stitch lifecycle tests.

- [ ] **Step 8: Commit Task 3**

Run:

```bash
rtk git add crates/rollshot-app/src-tauri/Cargo.toml crates/rollshot-app/src-tauri/src/session.rs
rtk git commit -m "feat(app): add interactive stitch session state"
```

## Task 4: Add Tauri Stitch, Stop, Final Preview, and Save Commands

**Files:**
- Modify: `crates/rollshot-app/src-tauri/src/session.rs`
- Modify: `crates/rollshot-app/src-tauri/src/commands.rs`
- Modify: `crates/rollshot-app/src-tauri/src/lib.rs`

- [ ] **Step 1: Add failing command-level session tests**

Append this test to `crates/rollshot-app/src-tauri/src/session.rs`:

```rust
    #[test]
    fn save_image_writes_final_png() {
        let tempdir = std::env::temp_dir().join(format!(
            "rollshot-app-save-image-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&tempdir).expect("create tempdir");
        let output = tempdir.join("stitched.png");

        let mut session = AppSession::new();
        session.store_frame_for_test(scrolling_frame(0));
        session
            .confirm_region(RegionDto {
                x: 0,
                y: 0,
                width: 80,
                height: 80,
            })
            .expect("confirm region");
        session.start_stitching_for_test().expect("start stitching");
        session
            .push_stitch_frame_for_test(scrolling_frame(0))
            .expect("first frame");
        session.finish_stitching_for_test().expect("finish");

        let done = session.save_image_for_test(&output).expect("save png");

        assert_eq!(done.output_path, Some(output.to_string_lossy().to_string()));
        let decoded = image::open(&output).expect("decode saved png");
        assert_eq!(decoded.width(), 80);
        let _ = std::fs::remove_dir_all(&tempdir);
    }
```

Run:

```bash
rtk cargo test -p rollshot-app session::tests::save_image_writes_final_png
```

Expected: FAIL because `save_image_for_test` does not exist.

- [ ] **Step 2: Add worker lifecycle fields**

Replace `SharedSession` with:

```rust
pub struct SharedSession {
    inner: Mutex<AppSession>,
    stop: AtomicBool,
    reader: Mutex<Option<JoinHandle<()>>>,
    stitch_stop: AtomicBool,
    stitcher: Mutex<Option<JoinHandle<()>>>,
}
```

Update `SharedSession::new`:

```rust
Self {
    inner: Mutex::new(AppSession::new()),
    stop: AtomicBool::new(false),
    reader: Mutex::new(None),
    stitch_stop: AtomicBool::new(false),
    stitcher: Mutex::new(None),
}
```

- [ ] **Step 3: Add shared stitching methods**

Add these methods to `impl SharedSession`:

```rust
    pub fn start_stitching(self: &Arc<Self>) -> Result<(), String> {
        {
            let mut stitcher = self
                .stitcher
                .lock()
                .map_err(|_| "stitcher lock poisoned".to_string())?;
            if stitcher.is_some() {
                return Err("stitching is already running".to_string());
            }

            {
                let mut inner = self
                    .inner
                    .lock()
                    .map_err(|_| "session lock poisoned".to_string())?;
                inner.start_stitching()?;
            }

            self.stitch_stop.store(false, Ordering::Relaxed);
            let session = Arc::clone(self);
            *stitcher = Some(std::thread::spawn(move || {
                session.stitch_loop();
            }));
        }
        Ok(())
    }

    fn stitch_loop(&self) {
        let mut last_seen_seq = 0_u64;
        while !self.stitch_stop.load(Ordering::Relaxed) {
            let next_frame = {
                let inner = match self.inner.lock() {
                    Ok(inner) => inner,
                    Err(_) => return,
                };
                if inner.latest_frame_seq == last_seen_seq {
                    None
                } else {
                    last_seen_seq = inner.latest_frame_seq;
                    inner.latest_frame.clone()
                }
            };

            if let Some(frame) = next_frame {
                if let Ok(mut inner) = self.inner.lock() {
                    if let Err(err) = inner.push_stitch_frame(frame) {
                        inner.error = Some(err);
                        break;
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn stop_stitching(&self) -> Result<DoneImageDto, String> {
        self.stitch_stop.store(true, Ordering::Relaxed);
        if let Ok(mut stitcher) = self.stitcher.lock() {
            if let Some(handle) = stitcher.take() {
                let _ = handle.join();
            }
        }

        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "session lock poisoned".to_string())?;
        inner.finish_stitching()
    }

    pub fn save_image(&self, path: &Path) -> Result<DoneImageDto, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "session lock poisoned".to_string())?;
        inner.save_image(path)
    }

    pub fn final_preview_png(&self, max_edge: u32) -> Result<Option<Vec<u8>>, String> {
        let image = {
            let inner = self
                .inner
                .lock()
                .map_err(|_| "session lock poisoned".to_string())?;
            inner.final_image.clone()
        };
        image
            .as_ref()
            .map(|image| encode_preview_image_png(image, max_edge))
            .transpose()
    }
```

Change `stop_capture` so it stops the stitch worker before the reader:

```rust
    pub fn stop_capture(&self) {
        self.stitch_stop.store(true, Ordering::Relaxed);
        if let Ok(mut stitcher) = self.stitcher.lock() {
            if let Some(handle) = stitcher.take() {
                let _ = handle.join();
            }
        }

        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut reader) = self.reader.lock() {
            if let Some(handle) = reader.take() {
                let _ = handle.join();
            }
        }
    }
```

Update `Drop`:

```rust
impl Drop for SharedSession {
    fn drop(&mut self) {
        self.stitch_stop.store(true, Ordering::Relaxed);
        self.stop.store(true, Ordering::Relaxed);
    }
}
```

- [ ] **Step 4: Add save and final-preview helpers**

Add this private method to `impl AppSession`:

```rust
    fn save_image(&mut self, path: &Path) -> Result<DoneImageDto, String> {
        let image = self
            .final_image
            .as_ref()
            .ok_or_else(|| "no final image is available to save".to_string())?;
        image
            .save_with_format(path, image::ImageFormat::Png)
            .map_err(|err| format!("failed to save {}: {err}", path.display()))?;
        self.output_path = Some(path.to_string_lossy().to_string());
        Ok(DoneImageDto {
            image_width: image.width(),
            image_height: image.height(),
            output_path: self.output_path.clone(),
        })
    }
```

Replace `encode_preview_png` with a small wrapper and reusable image helper:

```rust
fn encode_preview_png(frame: &CapturedFrame, max_edge: u32) -> Result<Vec<u8>, String> {
    encode_preview_image_png(&frame.image, max_edge)
}

fn encode_preview_image_png(image: &RgbaImage, max_edge: u32) -> Result<Vec<u8>, String> {
    let max_edge = max_edge.max(1);
    let width = image.width();
    let height = image.height();
    let largest = width.max(height).max(1);
    let scale = (max_edge as f32 / largest as f32).min(1.0);
    let preview_width = ((width as f32 * scale).round() as u32).max(1);
    let preview_height = ((height as f32 * scale).round() as u32).max(1);

    let mut cursor = std::io::Cursor::new(Vec::new());
    if preview_width == width && preview_height == height {
        image
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|err| format!("failed to encode preview png: {err}"))?;
    } else {
        image::imageops::resize(
            image,
            preview_width,
            preview_height,
            image::imageops::FilterType::Nearest,
        )
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|err| format!("failed to encode preview png: {err}"))?;
    }
    Ok(cursor.into_inner())
}
```

Append this test-only proxy to the existing `#[cfg(test)] impl AppSession` block:

```rust
    pub fn save_image_for_test(&mut self, path: &Path) -> Result<DoneImageDto, String> {
        self.save_image(path)
    }
```

- [ ] **Step 5: Add Tauri commands**

In `crates/rollshot-app/src-tauri/src/commands.rs`, add `PathBuf` import:

```rust
use std::path::PathBuf;
```

Add commands:

```rust
#[tauri::command]
pub fn start_stitching(session: tauri::State<'_, Arc<SharedSession>>) -> Result<(), String> {
    session.inner().start_stitching()
}

#[tauri::command]
pub fn stop_stitching(
    session: tauri::State<'_, Arc<SharedSession>>,
) -> Result<crate::session::DoneImageDto, String> {
    session.stop_stitching()
}

#[tauri::command]
pub fn save_image(
    session: tauri::State<'_, Arc<SharedSession>>,
    path: PathBuf,
) -> Result<crate::session::DoneImageDto, String> {
    session.save_image(&path)
}

#[tauri::command]
pub fn get_final_preview(
    session: tauri::State<'_, Arc<SharedSession>>,
    max_edge: u32,
) -> Result<Response, String> {
    let bytes = session.final_preview_png(max_edge)?.unwrap_or_default();
    Ok(Response::new(bytes))
}
```

- [ ] **Step 6: Register commands**

In `crates/rollshot-app/src-tauri/src/lib.rs`, add the commands to the handler:

```rust
.invoke_handler(tauri::generate_handler![
    commands::launch_options,
    commands::start_capture,
    commands::stop_capture,
    commands::session_status,
    commands::confirm_region,
    commands::get_latest_preview,
    commands::start_stitching,
    commands::stop_stitching,
    commands::save_image,
    commands::get_final_preview
])
```

- [ ] **Step 7: Verify Rust app command support**

Run:

```bash
rtk cargo test -p rollshot-app session::tests::save_image_writes_final_png
```

Expected: PASS.

Then run:

```bash
rtk cargo test -p rollshot-app
```

Expected: PASS.

- [ ] **Step 8: Commit Task 4**

Run:

```bash
rtk git add crates/rollshot-app/src-tauri/src/session.rs crates/rollshot-app/src-tauri/src/commands.rs crates/rollshot-app/src-tauri/src/lib.rs
rtk git commit -m "feat(app): expose interactive stitch commands"
```

## Task 5: Add Save Dialog Integration And Frontend API Tests

**Files:**
- Modify: `crates/rollshot-app/package.json`
- Modify: `crates/rollshot-app/src-tauri/Cargo.toml`
- Modify: `crates/rollshot-app/src-tauri/capabilities/default.json`
- Modify: `crates/rollshot-app/src-tauri/src/lib.rs`
- Modify: `crates/rollshot-app/src/api/capture.ts`
- Create: `crates/rollshot-app/src/api/capture.test.ts`

- [ ] **Step 1: Add failing frontend API tests**

Create `crates/rollshot-app/src/api/capture.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest'

const invokeMock = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}))

describe('capture api wrappers', () => {
  beforeEach(() => {
    invokeMock.mockReset()
  })

  it('sends start_stitching without payload', async () => {
    const { startStitching } = await import('./capture')
    invokeMock.mockResolvedValueOnce(undefined)

    await startStitching()

    expect(invokeMock).toHaveBeenCalledWith('start_stitching')
  })

  it('saves final image to selected path', async () => {
    const { saveImage } = await import('./capture')
    invokeMock.mockResolvedValueOnce({
      image_width: 100,
      image_height: 400,
      output_path: '/tmp/out.png',
    })

    await expect(saveImage('/tmp/out.png')).resolves.toEqual({
      image_width: 100,
      image_height: 400,
      output_path: '/tmp/out.png',
    })
    expect(invokeMock).toHaveBeenCalledWith('save_image', { path: '/tmp/out.png' })
  })

  it('sends stop_stitching and returns done image dto', async () => {
    const { stopStitching } = await import('./capture')
    invokeMock.mockResolvedValueOnce({
      image_width: 200,
      image_height: 600,
      output_path: null,
    })

    await expect(stopStitching()).resolves.toEqual({
      image_width: 200,
      image_height: 600,
      output_path: null,
    })
    expect(invokeMock).toHaveBeenCalledWith('stop_stitching')
  })

  it('returns null when final preview is not available yet', async () => {
    const { getFinalPreview } = await import('./capture')
    invokeMock.mockResolvedValueOnce(new ArrayBuffer(0))

    await expect(getFinalPreview(1200)).resolves.toBeNull()
  })
})
```

Run:

```bash
rtk pnpm --dir crates/rollshot-app test
```

Expected: FAIL because the new wrapper functions do not exist.

- [ ] **Step 2: Add dialog dependencies**

Edit `crates/rollshot-app/package.json` dependencies:

```json
"@tauri-apps/plugin-dialog": "^2.0.0",
```

Edit `crates/rollshot-app/src-tauri/Cargo.toml`:

```toml
tauri-plugin-dialog = "2"
```

Run:

```bash
rtk pnpm --dir crates/rollshot-app install
```

Expected: `crates/rollshot-app/pnpm-lock.yaml` updates and no npm/yarn/bun lockfile is created.

- [ ] **Step 3: Enable dialog plugin and permission**

In `crates/rollshot-app/src-tauri/src/lib.rs`, register the plugin before `.manage(...)`:

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
```

In `crates/rollshot-app/src-tauri/capabilities/default.json`, replace permissions with:

```json
"permissions": ["core:default", "dialog:default", "dialog:allow-save"]
```

- [ ] **Step 4: Extend frontend API types and wrappers**

Edit `crates/rollshot-app/src/api/capture.ts` so `SessionStatus` includes the new states:

```ts
export type StitchStatsDto = {
  frame_count: number
  total_width: number
  total_height: number
  last_append: number
}

export type DoneImageDto = {
  image_width: number
  image_height: number
  output_path: string | null
}

export type SessionStatus =
  | { state: 'idle' }
  | {
      state: 'previewing'
      frame_width: number
      frame_height: number
      region: RegionDto | null
    }
  | {
      state: 'stitching'
      frame_width: number
      frame_height: number
      region: RegionDto
      stats: StitchStatsDto
      last_outcome: string | null
    }
  | {
      state: 'done'
      image_width: number
      image_height: number
      output_path: string | null
    }
  | { state: 'failed'; message: string }
```

Add wrappers:

```ts
export async function startStitching(): Promise<void> {
  await invoke('start_stitching')
}

export async function stopStitching(): Promise<DoneImageDto> {
  return await invoke<DoneImageDto>('stop_stitching')
}

export async function saveImage(path: string): Promise<DoneImageDto> {
  return await invoke<DoneImageDto>('save_image', { path })
}

export async function getFinalPreview(maxEdge: number): Promise<Blob | null> {
  const bytes = await invoke<ArrayBuffer>('get_final_preview', { maxEdge })
  if (bytes.byteLength === 0) {
    return null
  }
  return new Blob([bytes], { type: 'image/png' })
}
```

- [ ] **Step 5: Verify frontend API tests pass**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test
```

Expected: PASS.

- [ ] **Step 6: Commit Task 5**

Run:

```bash
rtk git add crates/rollshot-app/package.json crates/rollshot-app/pnpm-lock.yaml crates/rollshot-app/src-tauri/Cargo.toml crates/rollshot-app/src-tauri/capabilities/default.json crates/rollshot-app/src-tauri/src/lib.rs crates/rollshot-app/src/api/capture.ts crates/rollshot-app/src/api/capture.test.ts
rtk git commit -m "feat(app): add save dialog integration"
```

## Task 6: Wire The Interactive UI Flow

**Files:**
- Modify: `crates/rollshot-app/src/App.tsx`
- Modify: `crates/rollshot-app/src/App.css`

- [ ] **Step 1: Update imports and state**

In `crates/rollshot-app/src/App.tsx`, replace the icon import:

```ts
import { Check, Play, Save, Square, Wand2 } from 'lucide-react'
```

Add new API imports:

```ts
  getFinalPreview,
  saveImage,
  startStitching,
  stopStitching,
```

Add the dialog import:

```ts
import { save } from '@tauri-apps/plugin-dialog'
```

Add final preview state below `previewUrl`:

```ts
  const [finalPreviewUrl, setFinalPreviewUrl] = useState<string | null>(null)
```

Add a ref for cleanup:

```ts
  const finalPreviewUrlRef = useRef<string | null>(null)
```

Add this effect:

```ts
  useEffect(() => {
    finalPreviewUrlRef.current = finalPreviewUrl
  }, [finalPreviewUrl])
```

Extend the unmount cleanup:

```ts
      if (finalPreviewUrlRef.current) {
        URL.revokeObjectURL(finalPreviewUrlRef.current)
      }
```

Update the preview poll effect so it also fetches live frames during stitching (otherwise the preview freezes and the user has no visual feedback that frames are being captured). Change the condition in the polling timer:

```ts
        if (nextStatus.state === 'previewing' || nextStatus.state === 'stitching') {
```

- [ ] **Step 2: Add stitch, stop, and save handlers**

Add these functions in `App.tsx` after `onConfirmRegion`:

```ts
  async function onStartStitching() {
    try {
      setMessage('Stitching started. Scroll the selected content, then stop.')
      await startStitching()
    } catch (error) {
      setMessage(String(error))
    }
  }

  async function refreshFinalPreview() {
    const blob = await getFinalPreview(1400)
    if (!blob) {
      return
    }
    const nextUrl = URL.createObjectURL(blob)
    setFinalPreviewUrl((oldUrl) => {
      if (oldUrl) {
        URL.revokeObjectURL(oldUrl)
      }
      return nextUrl
    })
  }
```

Replace `onStop` with:

```ts
  async function onStop() {
    try {
      if (status.state === 'stitching') {
        const done = await stopStitching()
        setMessage(`Stitched ${done.image_width}x${done.image_height}`)
        await refreshFinalPreview()
        return
      }

      await stopCapture()
      setMessage('Capture stopped')
    } catch (error) {
      setMessage(String(error))
    }
  }
```

Add save handler:

```ts
  async function onSave() {
    try {
      const selected = await save({
        title: 'Save stitched PNG',
        defaultPath: 'rollshot.png',
        filters: [{ name: 'PNG image', extensions: ['png'] }],
      })
      if (!selected) {
        return
      }

      const done = await saveImage(selected)
      setMessage(done.output_path ? `Saved ${done.output_path}` : 'Saved image')
    } catch (error) {
      setMessage(String(error))
    }
  }
```

- [ ] **Step 3: Add button guards and status text**

Replace the `canConfirm` constant block with:

```ts
  const canConfirm =
    status.state === 'previewing' &&
    pendingRegion !== null &&
    pendingRegion.width > 0 &&
    pendingRegion.height > 0
  const canStartStitching = status.state === 'previewing' && status.region !== null
  const canSave = status.state === 'done'
  const statsText =
    status.state === 'stitching'
      ? `${status.stats.frame_count} frames, ${status.stats.total_width}x${status.stats.total_height}`
      : null
```

- [ ] **Step 4: Update the preview surface**

Replace the preview surface body in `App.tsx` with:

```tsx
        {status.state === 'done' && finalPreviewUrl ? (
          <img className="final-preview-image" src={finalPreviewUrl} alt="Stitched result" />
        ) : status.state === 'previewing' && previewUrl ? (
          <RegionOverlay
            imageUrl={previewUrl}
            sourceWidth={status.frame_width}
            sourceHeight={status.frame_height}
            onRegionChange={setPendingRegion}
          />
        ) : status.state === 'stitching' && previewUrl ? (
          <img className="preview-image" src={previewUrl} alt="Live capture preview" />
        ) : (
          <div className="empty-preview">No preview yet</div>
        )}
```

- [ ] **Step 5: Update the control panel**

Replace the control buttons in `App.tsx` with:

```tsx
        <Button type="button" onClick={onStart} disabled={status.state === 'stitching'}>
          <Play className="size-4" aria-hidden="true" />
          Start
        </Button>
        <Button
          type="button"
          variant="secondary"
          disabled={!canConfirm}
          onClick={onConfirmRegion}
        >
          <Check className="size-4" aria-hidden="true" />
          Confirm Region
        </Button>
        <Button
          type="button"
          variant="secondary"
          disabled={!canStartStitching}
          onClick={onStartStitching}
        >
          <Wand2 className="size-4" aria-hidden="true" />
          Start Stitching
        </Button>
        <Button type="button" variant="outline" onClick={onStop}>
          <Square className="size-4" aria-hidden="true" />
          Stop
        </Button>
        <Button type="button" disabled={!canSave} onClick={onSave}>
          <Save className="size-4" aria-hidden="true" />
          Save
        </Button>
        {statsText ? <p className="stats-text">{statsText}</p> : null}
        {status.state === 'stitching' && status.last_outcome ? (
          <p className="stats-text">{status.last_outcome}</p>
        ) : null}
```

- [ ] **Step 6: Add minimal CSS**

Append to `crates/rollshot-app/src/App.css`:

```css
.stats-text {
  margin: 0;
  color: #334155;
  font-size: 13px;
  line-height: 1.35;
}

.final-preview-image {
  display: block;
  max-width: 100%;
  max-height: calc(100vh - 48px);
  object-fit: contain;
  border: 1px solid #8b95a5;
  background: #ffffff;
}
```

- [ ] **Step 7: Verify frontend typecheck and tests**

Run:

```bash
rtk pnpm --dir crates/rollshot-app run typecheck
```

Expected: PASS.

Run:

```bash
rtk pnpm --dir crates/rollshot-app test
```

Expected: PASS.

- [ ] **Step 8: Commit Task 6**

Run:

```bash
rtk git add crates/rollshot-app/src/App.tsx crates/rollshot-app/src/App.css
rtk git commit -m "feat(app): wire interactive stitch controls"
```

## Task 7: Full Verification And Manual Linux Check

**Files:**
- No source edits expected.

- [ ] **Step 1: Run Rust tests**

Run:

```bash
rtk cargo test
```

Expected: PASS.

- [ ] **Step 2: Check Rust formatting**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS. If this fails, run `rtk cargo fmt`, inspect the diff, and rerun `rtk cargo fmt --check`.

- [ ] **Step 3: Run clippy because this touches threaded app state**

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Run frontend checks**

Run:

```bash
rtk pnpm --dir crates/rollshot-app run typecheck
```

Expected: PASS.

Run:

```bash
rtk pnpm --dir crates/rollshot-app test
```

Expected: PASS.

Run:

```bash
rtk pnpm --dir crates/rollshot-app run build
```

Expected: PASS.

- [ ] **Step 5: Verify headless capture still works**

Run:

```bash
rtk cargo test -p rollshot-cli rollshot_capture_fixture_writes_png
```

Expected: PASS.

- [ ] **Step 6: Manual KDE 6 Wayland verification**

Run:

```bash
rtk cargo run -p rollshot-cli -- capture
```

Expected manual result:

1. `rollshot-app` opens from the CLI launcher.
2. Portal source selection succeeds.
3. Live preview appears.
4. Dragging and confirming a region enables Start Stitching.
5. Start Stitching begins stats updates only after region confirmation.
6. Manual scrolling changes frame count or total stitched dimensions.
7. Stop produces a final preview.
8. Save opens a native save dialog and writes a PNG.
9. The saved PNG decodes and matches the selected region width.
10. GUI controls do not appear inside the selected region for a normal region capture.

- [ ] **Step 7: Record verification outcome**

If every command and manual check passes, no commit is needed for Task 7. If a verification fix was required, make the smallest source edit for that failing check, rerun the failing command, then commit only the files touched by that fix with a message that names the fixed behavior.

## Self-Review

- Spec coverage:
  - Coordinate conversion and source-pixel contract: Task 1.
  - Crop selected region: Task 2 and Task 3.
  - Run stitch loop under GUI session state: Task 3 and Task 4.
  - Stop on user action: Task 4 and Task 6.
  - Keep final image in backend state: Task 3 and Task 4.
  - Save PNG: Task 4, Task 5, and Task 6.
  - Minimal polish: Task 6 stats/final-preview UI only.
  - Full-screen Linux fallback: intentionally omitted because Plan 2 did not prove it was needed.
- Placeholder scan:
  - The plan contains concrete files, commands, snippets, and expected outcomes.
  - The only intentionally incomplete code is the Task 2 crop stub, which is replaced in the next step.
- Type consistency:
  - Rust status fields use snake_case through serde and match TypeScript names: `frame_width`, `image_width`, `output_path`, `last_outcome`.
  - Command names match frontend wrappers: `start_stitching`, `stop_stitching`, `save_image`, `get_final_preview`.
  - The final image stays in Rust as `AppSession::final_image`; the frontend receives only a resized PNG preview and save status.

---

## Engineering Review Notes

Applied during eng review on 2026-05-24. Changes marked with D1-D4.

### D1: Preview freezes during stitching (Architecture)

The poll timer only fetched preview during `previewing` state. Once stitching started, the user saw a frozen image with no visual feedback that frames were being captured. Fixed by extending the poll guard in Task 6 Step 1 to also match `stitching`.

### D2: Test-only methods leaked into release binary (Code Quality)

`start_stitching_for_test`, `push_stitch_frame_for_test`, `finish_stitching_for_test`, `final_image_png_for_test`, `save_image_for_test`, and `encode_rgba_png` were placed in the regular `impl AppSession` block. Fixed by moving them to `#[cfg(test)] impl AppSession` blocks in Task 3 Step 5 and Task 4 Step 4.

### D3: Missing `Default` derive on `StitchStatsDto` (Code Quality — compile error)

`AppSession` derives `Default`, which requires all fields to implement `Default`. `StitchStatsDto` was missing it. Fixed by adding `Default` to its derive list in Task 3 Step 3.

### D4: Missing `stopStitching` frontend API test (Tests)

The test file covered `startStitching`, `saveImage`, and `getFinalPreview` but not `stopStitching`. Added a test in Task 5 Step 1 for completeness.

### NOT in scope

- Clipboard support (copy stitched image to clipboard) — deferred per spec, save-to-file only for v0.5.
- Full-screen Linux fallback / keyboard-only controls — Plan 2 did not prove it was needed.
- `Arc<CapturedFrame>` zero-copy frame sharing between reader and stitch threads — current `clone()` is bounded by capture FPS (~5fps, ~70MB/s temporary allocations on 2560x1440). Acceptable for v0.5; optimize if profiling shows pressure.
- Memory ceiling / paging for the stitched canvas — `LinearCanvas` grows unboundedly during long scrolls. Pre-existing in `rollshot-core`, not introduced by this plan.
- macOS interactive testing — plan covers Linux KDE 6 Wayland only; macOS path is structurally identical (same Rust crop/stitch/save) but untested manually.

### What already exists

| Sub-problem | Existing code | Reused? |
|---|---|---|
| Frame capture + reader thread | `SharedSession::start_reader` | Yes — extended with seq counter |
| Region selection UI | `RegionOverlay.tsx` + `geometry.ts` | Yes — conversion fix only |
| Stitcher engine | `rollshot_core::Stitcher` | Yes — used directly |
| Preview encoding | `encode_preview_png` in `session.rs` | Yes — refactored to `encode_preview_image_png` |
| Region validation | `AppSession::confirm_region` | Yes — reused as-is |

No unnecessary rebuilds detected.

### Failure modes

| Codepath | Failure mode | Test? | Error handling? | User visibility |
|---|---|---|---|---|
| `crop_frame` | Region outside frame bounds | Task 2 `crop_frame_rejects_out_of_bounds_region` | `Err(CaptureError::InvalidConfig)` | Error propagates to status |
| `crop_frame` | Negative origin | Task 2 `crop_frame_rejects_negative_origin` | `Err(CaptureError::InvalidConfig)` | Error propagates to status |
| `start_stitching` | No confirmed region | Task 3 `start_stitching_requires_confirmed_region` | `Err("confirm a region...")` | Error shown in message |
| `push_stitch_frame` | Crop error (frame size changed mid-session) | No dedicated test | `inner.error = Some(err)` via stitch_loop | Status becomes `Failed` |
| `finish_stitching` | Stitcher produced no output (0 frames pushed) | No dedicated test | `Err("stitcher produced no output")` | Error shown in message |
| `save_image` | Permission denied / invalid path | No dedicated test | `Err("failed to save ...")` via `image::save_with_format` | Error shown in message |
| `final_preview_png` | Cloning very large final image | No test | OOM possible for extremely long scrolls | Process crash (pre-existing `LinearCanvas` concern) |
| Stitch loop | Mutex poisoned (panic in another thread) | No test | `stitch_loop` returns silently; `inner.lock()` returns `Err` | Stitching stops silently |
| Dialog cancelled | User clicks Cancel in save dialog | N/A (frontend logic) | `if (!selected) return` | No action, no error |

No critical gaps — all failure modes either have tests, explicit error handling, or are pre-existing concerns in `rollshot-core` outside this plan's scope.

### Parallelization strategy

| Task | Modules touched | Depends on |
|---|---|---|
| Task 1 | `crates/rollshot-app/src/` (frontend) | — |
| Task 2 | `crates/rollshot-capture/` | — |
| Task 3 | `crates/rollshot-app/src-tauri/` | Task 2 (uses `crop_frame`) |
| Task 4 | `crates/rollshot-app/src-tauri/` | Task 3 |
| Task 5 | `crates/rollshot-app/` (both frontend + Cargo.toml) | Task 4 |
| Task 6 | `crates/rollshot-app/src/` (frontend) | Task 5 |
| Task 7 | None (verification) | Task 6 |

**Lane A:** Task 1 (frontend geometry fix) — independent, frontend-only
**Lane B:** Task 2 → Task 3 → Task 4 → Task 5 → Task 6 (sequential, shared `src-tauri/` state)

Launch A + B in parallel. Task 1 and Task 2 can run concurrently. Tasks 3-6 are sequential. Task 7 waits for all.

### Completion summary

```
Plan reviewed:           docs/superpowers/plans/2026-05-23-rollshot-v05-plan-3-interactive-stitch-stop-save.md
Tasks in plan:           7
Files Create/Modify:     2 create / 11 modify

- Step 0: Scope Challenge   — accepted as-is (no complexity smell)
- Architecture Review:        1 issue (D1: frozen preview during stitching)
- Plan Structure + Code Q:    2 issues (D2: test methods in release binary, D3: missing Default derive)
- Test Review:                table produced, 1 gap (D4: missing stopStitching test)
- Performance Review:         0 issues (frame clone is bounded by capture FPS, acceptable for v0.5)
- NOT in scope:               written
- What already exists:        written
- Failure modes:              0 critical gaps
- Parallelization:            2 lanes, 1 parallel / 1 sequential
- Unresolved decisions:       0
```

Plan is locked in — run `superpowers:subagent-driven-development` with Lane A (Task 1) and Lane B (Tasks 2-6) in parallel, then Task 7 after both merge.
