# Storyboard Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Action Guide Storyboard Export: a single PNG showing the reviewed guide steps as vertically stacked cards with step labels, titles, and keyframes.

**Architecture:** Put the headless PNG renderer in `rollshot-action`, matching the existing GIF and MP4 exporters. Reuse the reviewed `Guide` plus retained `FrameStore` keyframes as the only data source, and keep the app layer limited to save-dialog wiring, a toolbar button, tracing, and success/error banners.

**Tech Stack:** Rust, `image` PNG encoding, existing `rollshot-image-document` deterministic text rasterization/fonts, iced 0.14 app messages/views, `rfd` save dialogs, `tracing` target `rollshot::action::export`.

---

## Simple Spec

### Problem

Action Guide can already export Markdown, GIF, MP4, and Issue Pack assets, but it lacks a low-friction static artifact for chat and issue comments. Users who want to show a short reproduction or walkthrough as one image must manually export keyframes, arrange them, and label each step outside Rollshot.

### Users

- Engineers and QA sharing bug reproduction steps.
- PMs, designers, support, and internal ops sharing a short product flow without sending a video.
- Rollshot users who already reviewed Action Guide steps and want a quick standalone visual summary.

### V1 Behavior

- The Timeline Workspace exposes `Export Storyboard` alongside the existing GIF/MP4/Guide export actions.
- Export uses the current reviewed `Guide` order and each step's current retained keyframe.
- Output is one PNG with a white background and a vertical single-column list of step cards.
- Each card shows a deterministic `Step N` label, the step title when present, and the keyframe image scaled to fit the storyboard width without upscaling.
- Export writes to a user-selected `.png` path, leaves the review workspace open, and shows a success or failure banner.

### Functional Requirements

- Reject empty guides without writing a file.
- Reject steps whose keyframe pixels are no longer retained.
- Bound output memory with `StoryboardOptions::max_canvas_pixels`.
- Keep output deterministic for the same guide, retained frames, and options.
- Do not mutate guide state, selected step, keyframes, titles, or the retained frame store.
- Use stable `rollshot::action::export` tracing in app export paths.

### Acceptance Criteria

- A reviewed guide with two retained steps exports a readable PNG containing two cards.
- Long titles are elided so labels stay inside the card width.
- Missing keyframes, empty guides, oversized canvases, encode errors, and I/O errors return typed errors and surface as non-crashing app banners.
- Cancelling the save dialog is a no-op with no stale success or failure message.
- Focused package tests, workspace tests, formatting, and clippy pass in the final verification task.

### Non-Goals

- No custom templates, grid layouts, pagination, captions, annotations, cursor effects, PDF, HTML, Issue Pack integration, or redaction-aware storyboard export in this plan.
- No new product state model; Storyboard Export is a derived artifact from the already-reviewed `Guide` and `FrameStore`.

## Locked Decisions

- UI/UX does not need a separate design pass for V1.
- Add `Export Storyboard` beside `Export GIF` and `Export MP4`.
- Use a save-file dialog defaulting to `storyboard.png`.
- On success, keep the Timeline Workspace open and show `Storyboard saved to ...`.
- On failure, keep the Timeline Workspace open and show `Storyboard export failed: ...`.
- V1 exports one PNG, single column, white background, light card border, no modal, no user-facing layout options.
- V1 includes `Step N` and the current step title when `StoryboardOptions::show_titles` is true.

## Files

- Modify: `crates/rollshot-image-document/src/text.rs`
  - Add a tiny public wrapper around the existing deterministic text rasterizer.
- Modify: `crates/rollshot-image-document/src/lib.rs`
  - Re-export the public text drawing wrapper.
- Create: `crates/rollshot-image-document/tests/text_export.rs`
  - Proves downstream crates can draw text without accessing private modules.
- Modify: `crates/rollshot-action/Cargo.toml`
  - Add `rollshot-image-document` path dependency for deterministic fonts/text.
- Modify: `crates/rollshot-action/src/error.rs`
  - Add `StoryboardError` with descriptive, recoverable failures.
- Create: `crates/rollshot-action/src/storyboard.rs`
  - Implement layout, rasterization, PNG save, atomic write, and unit tests.
- Modify: `crates/rollshot-action/src/lib.rs`
  - Export `export_storyboard`, `StoryboardOptions`, `StoryboardExportResult`, and `StoryboardError`.
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
  - Add messages, save picker, exporter call, tracing, and update tests.
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`
  - Add the toolbar button.

## Data Flow

```text
Timeline Workspace
  reviewed Guide + retained FrameStore
          |
          v
rollshot_action::export_storyboard
  resolve steps -> resolve retained keyframes -> fit labels
          |
          v
  measure cards -> enforce max canvas pixels -> rasterize PNG
          |
          v
  write sibling temp file -> rename into selected save path
          |
          v
Timeline Workspace banner, session remains open
```

## What Already Exists

- `rollshot-action::export_gif` and `rollshot-action::export_video` already establish the reviewed-keyframes-only export pattern; this plan reuses the same `Guide` + `FrameStore` source instead of inventing a new model.
- `rollshot-action::export_guide` already exports step titles and keyframes to a portable folder; this plan reuses the title/keyframe semantics but creates a standalone PNG artifact.
- `rollshot-image-document::text` already owns deterministic text measurement and glyph rasterization with vendored fonts; this plan exposes a narrow wrapper instead of adding a second font stack.
- `timeline_workspace/update.rs` already has save-dialog flows and success/error banners for GIF/MP4; this plan adds the storyboard branch beside those flows.
- `timeline_workspace/view.rs` already groups export buttons in the header; this plan adds one secondary button in that existing group.

## NOT in Scope

- Grid, pagination, compact mode, PDF, HTML, and template export are deferred because V1 is a single chat-friendly PNG.
- User-editable export options are deferred because V1 uses safe defaults from `StoryboardOptions::default()`.
- Redaction-aware storyboard export is deferred because current Action Guide keyframes are reviewed evidence images, not automatically flattened redaction outputs.
- Issue Pack inclusion is deferred because this plan only adds the standalone Timeline Workspace export action.
- Telemetry is deferred because current GIF/MP4 Action Guide exports do not establish an event pipeline in this workspace.

## Test Coverage

| Task / behavior | Unit | Integ | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| Task 1 / public deterministic text draw wrapper paints pixels | ✓ | ✓ | — | no |
| Task 2 / `StoryboardError` messages are descriptive | ✓ | — | — | no |
| Task 2 / storyboard PNG writes one card per reviewed step | ✓ | — | — | no |
| Task 2 / empty guide rejects export and writes nothing | ✓ | — | — | no |
| Task 2 / missing keyframe rejects export and writes nothing | ✓ | — | — | no |
| Task 2 / long titles are elided to card width | ✓ | — | — | no |
| Task 2 / canvas pixel limit rejects oversized output | ✓ | — | — | no |
| Task 3 / app save-path handler writes PNG and keeps workspace open | ✓ | ✓ | — | no |
| Task 3 / app empty-guide export surfaces an error banner | ✓ | ✓ | — | no |
| Task 3 / cancelled save dialog is a no-op | ✓ | — | — | no |

## Failure Modes

- Empty guide: covered by Task 2 Step 7 and Task 3 Step 1; handled by `StoryboardError::Empty`; user sees `Storyboard export failed: ...`.
- Missing retained keyframe: covered by Task 2 Step 7; handled by `StoryboardError::KeyframeMissing`; user sees `Storyboard export failed: ...`.
- Oversized output: covered by Task 2 Step 7; handled by `StoryboardError::CanvasTooLarge`; user sees `Storyboard export failed: ...`.
- PNG encode failure: error handling exists in `StoryboardError::Encode`; no deterministic unit test is planned because inducing encoder failure without platform permissions is brittle.
- Save/rename failure: error handling exists in `StoryboardError::Io`; app banner handling is covered by the same `Err(error)` branch in Task 3 Step 5.
- Save dialog cancelled: covered by Task 3 Step 1; handled by `ExportStoryboardPathChosen(None)`; user sees no stale or misleading banner.
- Long title: covered by Task 2 Step 7; handled by `fit_label`; user sees a clipped-safe label rather than text running outside the card.

## Execution Strategy

| Task | Modules touched | Depends on |
|---|---|---|
| Task 1: Expose Deterministic Text Drawing | `crates/rollshot-image-document/` | — |
| Task 2: Add Headless Storyboard Exporter | `crates/rollshot-action/`, `crates/rollshot-image-document/` dependency API | Task 1 |
| Task 3: Wire Storyboard Export Into Timeline Workspace | `crates/rollshot-app/src/timeline_workspace/`, `crates/rollshot-action/` public API | Task 2 |
| Task 4: Verification | workspace | Tasks 1-3 |

Sequential execution, no parallelization opportunity: each task consumes API exposed by the previous task and the plan is only four tasks.

## Task 1: Expose Deterministic Text Drawing

**Files:**
- Modify: `crates/rollshot-image-document/src/text.rs`
- Modify: `crates/rollshot-image-document/src/lib.rs`
- Create: `crates/rollshot-image-document/tests/text_export.rs`

- [ ] **Step 1: Write the failing public API test**

Create `crates/rollshot-image-document/tests/text_export.rs`:

```rust
use image::{Rgba, RgbaImage};
use rollshot_image_document::{draw_text_block, ImagePoint, Rgba8};

#[test]
fn public_text_draw_api_paints_pixels() {
    let mut image = RgbaImage::from_pixel(160, 60, Rgba([255, 255, 255, 255]));

    draw_text_block(
        &mut image,
        ImagePoint::new(8.0, 8.0),
        "Step 1 - Click",
        20.0,
        true,
        Rgba8::new(20, 24, 31, 255),
    );

    let changed = image
        .pixels()
        .filter(|pixel| pixel.0 != [255, 255, 255, 255])
        .count();
    assert!(changed > 20, "expected glyph pixels, got {changed}");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run:

```bash
rtk cargo test -p rollshot-image-document --test text_export public_text_draw_api_paints_pixels
```

Expected: FAIL with an unresolved import for `draw_text_block`.

- [ ] **Step 3: Add the public wrapper**

In `crates/rollshot-image-document/src/text.rs`, add this public function directly above the existing private `draw_block` function:

```rust
/// Rasterize a text block onto `img` with its top-left at `top_left`.
///
/// This is the public wrapper for downstream headless renderers that need the
/// same deterministic vendored-font path as annotation flattening.
pub fn draw_text_block(
    img: &mut RgbaImage,
    top_left: ImagePoint,
    text: &str,
    px: f32,
    bold: bool,
    color: Rgba8,
) {
    draw_block(img, top_left, text, px, bold, color);
}
```

Keep the existing `pub(crate) fn draw_block(...)` unchanged so `flatten.rs` continues to use the internal name.

- [ ] **Step 4: Re-export the wrapper**

In `crates/rollshot-image-document/src/lib.rs`, replace:

```rust
pub use text::measure_block;
```

with:

```rust
pub use text::{draw_text_block, measure_block};
```

- [ ] **Step 5: Run the focused test**

Run:

```bash
rtk cargo test -p rollshot-image-document --test text_export public_text_draw_api_paints_pixels
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-image-document/src/text.rs crates/rollshot-image-document/src/lib.rs crates/rollshot-image-document/tests/text_export.rs
rtk git commit -m "feat(action): expose deterministic text rasterizer"
```

## Task 2: Add Headless Storyboard Exporter

**Files:**
- Modify: `crates/rollshot-action/Cargo.toml`
- Modify: `crates/rollshot-action/src/error.rs`
- Create: `crates/rollshot-action/src/storyboard.rs`
- Modify: `crates/rollshot-action/src/lib.rs`

- [ ] **Step 1: Add the dependency**

In `crates/rollshot-action/Cargo.toml`, add this dependency:

```toml
rollshot-image-document = { path = "../rollshot-image-document" }
```

The dependency block should become:

```toml
[dependencies]
ffmpeg-sidecar = { workspace = true }
image = { workspace = true, features = ["gif"] }
rollshot-image-document = { path = "../rollshot-image-document" }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
```

- [ ] **Step 2: Add failing error API tests**

In `crates/rollshot-action/src/error.rs`, extend the existing `tests` module with:

```rust
    #[test]
    fn storyboard_error_messages_are_descriptive() {
        assert_eq!(
            StoryboardError::Empty.to_string(),
            "cannot export a storyboard for a guide with no steps"
        );
        let missing = StoryboardError::KeyframeMissing { index: 4 };
        assert!(missing.to_string().contains("step 4"), "{missing}");
        let too_large = StoryboardError::CanvasTooLarge;
        assert!(
            too_large.to_string().contains("storyboard canvas is too large"),
            "{too_large}"
        );
    }
```

- [ ] **Step 3: Run the error test to verify it fails**

Run:

```bash
rtk cargo test -p rollshot-action error::tests::storyboard_error_messages_are_descriptive
```

Expected: FAIL because `StoryboardError` is not defined.

- [ ] **Step 4: Define `StoryboardError`**

In `crates/rollshot-action/src/error.rs`, add this enum after `VideoError`:

```rust
/// Storyboard PNG export failure. On any error, no file is left at the target
/// path and the editable session stays intact.
#[derive(Debug, thiserror::Error)]
pub enum StoryboardError {
    #[error("cannot export a storyboard for a guide with no steps")]
    Empty,
    #[error("step {index} keyframe pixels were not retained")]
    KeyframeMissing { index: usize },
    #[error("storyboard canvas is too large to render")]
    CanvasTooLarge,
    #[error("failed to encode storyboard PNG at {path}: {source}")]
    Encode {
        path: String,
        #[source]
        source: image::ImageError,
    },
    #[error("storyboard I/O error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}
```

- [ ] **Step 5: Export the error type**

In `crates/rollshot-action/src/lib.rs`, replace:

```rust
pub use error::{DetectError, ExportError, GifError, VideoError};
```

with:

```rust
pub use error::{DetectError, ExportError, GifError, StoryboardError, VideoError};
```

- [ ] **Step 6: Run the error test**

Run:

```bash
rtk cargo test -p rollshot-action error::tests::storyboard_error_messages_are_descriptive
```

Expected: PASS.

- [ ] **Step 7: Add failing storyboard exporter tests**

Create `crates/rollshot-action/src/storyboard.rs` with the module header, public types, stub function, and tests:

```rust
//! Storyboard PNG export: assemble the final guide's reviewed keyframes into a
//! single vertical, chat-friendly image. This is a static workflow summary, not
//! a raw frame dump.

use std::path::{Path, PathBuf};

use image::RgbaImage;

use crate::error::StoryboardError;
use crate::frame_store::FrameStore;
use crate::guide::Guide;

#[derive(Debug, Clone)]
pub struct StoryboardOptions {
    pub max_width: u32,
    pub max_canvas_pixels: u64,
    pub outer_padding: u32,
    pub card_spacing: u32,
    pub card_padding: u32,
    pub show_titles: bool,
}

impl Default for StoryboardOptions {
    fn default() -> Self {
        Self {
            max_width: 1200,
            max_canvas_pixels: 24_000_000,
            outer_padding: 24,
            card_spacing: 20,
            card_padding: 16,
            show_titles: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryboardExportResult {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub step_count: usize,
}

pub fn export_storyboard(
    _guide: &Guide,
    _store: &FrameStore,
    _opts: StoryboardOptions,
    _out_path: &Path,
) -> Result<StoryboardExportResult, StoryboardError> {
    Err(StoryboardError::CanvasTooLarge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorConfig;
    use crate::frame_store::StoreConfig;
    use crate::models::{CandidateKind, CandidateStep, CaptureRegion, DetectReason};
    use crate::recorder::{ActionRecorder, Recording};
    use image::{ImageReader, Rgba, RgbaImage};

    fn region() -> CaptureRegion {
        CaptureRegion {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        }
    }

    fn black() -> RgbaImage {
        RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]))
    }

    fn quadrant() -> RgbaImage {
        let mut img = black();
        for y in 0..4 {
            for x in 0..4 {
                img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        img
    }

    fn recording() -> Recording {
        let det = DetectorConfig {
            diff_threshold: 0.01,
            area_threshold: 0.05,
            cooldown_ms: 0,
            ..DetectorConfig::default()
        };
        let mut rec = ActionRecorder::new(region(), StoreConfig::default(), det);
        rec.ingest_frame(black(), 0);
        for i in 1..=6 {
            rec.ingest_frame(quadrant(), i * 100);
        }
        let recording = rec.finish();
        assert!(!recording.candidates.is_empty());
        recording
    }

    fn guide_with_steps(keyframe: crate::models::FrameId, count: usize) -> Guide {
        let candidates = (0..count)
            .map(|i| CandidateStep {
                id: i as u64,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: (i as u64) * 100,
                keyframe,
                nearby: vec![keyframe],
            })
            .collect();
        Guide::from_candidates(candidates)
    }

    fn temp_path(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "rollshot-storyboard-{label}-{nanos}-{}.png",
            std::process::id()
        ))
    }

    #[test]
    fn exports_single_png_with_one_card_per_step() {
        let recording = recording();
        let keyframe = recording.store.retained_ids_for_test()[0];
        let mut guide = guide_with_steps(keyframe, 2);
        assert!(guide.rename(1, "Open settings".to_string()));
        assert!(guide.rename(2, "Save changes".to_string()));
        let path = temp_path("ok");

        let result = export_storyboard(
            &guide,
            &recording.store,
            StoryboardOptions {
                max_width: 320,
                max_canvas_pixels: 1_000_000,
                outer_padding: 12,
                card_spacing: 10,
                card_padding: 8,
                show_titles: true,
            },
            &path,
        )
        .expect("export storyboard");

        assert_eq!(result.path, path);
        assert_eq!(result.width, 320);
        assert_eq!(result.step_count, 2);
        assert!(path.exists(), "PNG should be written");
        assert!(
            !path.with_extension("png.tmp").exists(),
            "temporary file should be removed"
        );

        let decoded = ImageReader::open(&path)
            .unwrap()
            .decode()
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded.width(), 320);
        assert_eq!(decoded.height(), result.height);
        assert!(
            decoded.height() > 80,
            "storyboard should include labels, padding, and both images"
        );
        let non_white = decoded
            .pixels()
            .filter(|pixel| pixel.0 != [255, 255, 255, 255])
            .count();
        assert!(non_white > 100, "expected labels/cards/images, got {non_white}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_guide_is_rejected_and_writes_nothing() {
        let recording = recording();
        let guide = Guide::from_candidates(Vec::new());
        let path = temp_path("empty");

        let result = export_storyboard(&guide, &recording.store, StoryboardOptions::default(), &path);

        assert!(matches!(result, Err(StoryboardError::Empty)));
        assert!(!path.exists());
    }

    #[test]
    fn missing_keyframe_is_rejected_and_writes_nothing() {
        let store = FrameStore::new(StoreConfig::default());
        let guide = guide_with_steps(999, 1);
        let path = temp_path("missing");

        let result = export_storyboard(&guide, &store, StoryboardOptions::default(), &path);

        assert!(matches!(
            result,
            Err(StoryboardError::KeyframeMissing { index: 1 })
        ));
        assert!(!path.exists());
    }

    #[test]
    fn long_titles_are_elided_to_fit_card_width() {
        let label = fit_label(
            "Step 1 - This title is intentionally far longer than a narrow storyboard card",
            120.0,
        );

        assert!(label.ends_with("..."), "label should be elided: {label}");
        let (width, _) = measure_block(&label, LABEL_FONT_PX, true);
        assert!(width <= 120.0, "label width {width} exceeded limit");
    }

    #[test]
    fn canvas_pixel_limit_rejects_too_large_output() {
        let recording = recording();
        let keyframe = recording.store.retained_ids_for_test()[0];
        let guide = guide_with_steps(keyframe, 2);
        let path = temp_path("too-large");

        let result = export_storyboard(
            &guide,
            &recording.store,
            StoryboardOptions {
                max_width: 320,
                max_canvas_pixels: 10,
                outer_padding: 12,
                card_spacing: 10,
                card_padding: 8,
                show_titles: true,
            },
            &path,
        );

        assert!(matches!(result, Err(StoryboardError::CanvasTooLarge)));
        assert!(!path.exists());
    }
}
```

- [ ] **Step 8: Register and export the storyboard module**

In `crates/rollshot-action/src/lib.rs`, add the module:

```rust
mod storyboard;
```

Place it near `mod gif;` and `mod video;`.

Also add:

```rust
pub use storyboard::{export_storyboard, StoryboardExportResult, StoryboardOptions};
```

Place it near the `pub use gif::{...};` line.

- [ ] **Step 9: Run storyboard tests to verify they fail for behavior**

Run:

```bash
rtk cargo test -p rollshot-action storyboard
```

Expected: FAIL because the stub exporter has no rendering, label fitting, or pixel-limit behavior yet.

- [ ] **Step 10: Replace the stub with the implementation**

In `crates/rollshot-action/src/storyboard.rs`, replace everything above `#[cfg(test)]` with:

```rust
//! Storyboard PNG export: assemble the final guide's reviewed keyframes into a
//! single vertical, chat-friendly image. This is a static workflow summary, not
//! a raw frame dump.

use std::path::{Path, PathBuf};

use image::{imageops, Rgba, RgbaImage};
use rollshot_image_document::{draw_text_block, measure_block, ImagePoint, Rgba8};

use crate::error::StoryboardError;
use crate::frame_store::FrameStore;
use crate::guide::Guide;

const LABEL_FONT_PX: f32 = 26.0;
const LABEL_GAP: u32 = 10;
const BORDER: Rgba<u8> = Rgba([218, 223, 232, 255]);
const CARD_BACKGROUND: Rgba<u8> = Rgba([250, 251, 253, 255]);
const TEXT_COLOR: Rgba8 = Rgba8::new(20, 24, 31, 255);
const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);

#[derive(Debug, Clone)]
pub struct StoryboardOptions {
    pub max_width: u32,
    pub max_canvas_pixels: u64,
    pub outer_padding: u32,
    pub card_spacing: u32,
    pub card_padding: u32,
    pub show_titles: bool,
}

impl Default for StoryboardOptions {
    fn default() -> Self {
        Self {
            max_width: 1200,
            max_canvas_pixels: 24_000_000,
            outer_padding: 24,
            card_spacing: 20,
            card_padding: 16,
            show_titles: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryboardExportResult {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub step_count: usize,
}

struct StepCard {
    label: String,
    image: RgbaImage,
    height: u32,
}

pub fn export_storyboard(
    guide: &Guide,
    store: &FrameStore,
    opts: StoryboardOptions,
    out_path: &Path,
) -> Result<StoryboardExportResult, StoryboardError> {
    if guide.is_empty() {
        return Err(StoryboardError::Empty);
    }

    let canvas_width = opts.max_width.max(1);
    let image_max_width = canvas_width
        .saturating_sub(opts.outer_padding.saturating_mul(2))
        .saturating_sub(opts.card_padding.saturating_mul(2))
        .max(1);
    let label_max_width = image_max_width as f32;

    let mut cards = Vec::with_capacity(guide.steps().len());
    for (i, step) in guide.steps().iter().enumerate() {
        let retained = store
            .retained(step.keyframe)
            .ok_or(StoryboardError::KeyframeMissing { index: i + 1 })?;
        let image = downscale(&retained.image, image_max_width);
        let raw_label = if opts.show_titles && !step.title.trim().is_empty() {
            format!("Step {} - {}", i + 1, step.title.trim())
        } else {
            format!("Step {}", i + 1)
        };
        let label = fit_label(&raw_label, label_max_width);
        let (_, label_height) = measure_block(&label, LABEL_FONT_PX, true);
        let height = opts
            .card_padding
            .checked_add(label_height.ceil() as u32)
            .and_then(|value| value.checked_add(LABEL_GAP))
            .and_then(|value| value.checked_add(image.height()))
            .and_then(|value| value.checked_add(opts.card_padding))
            .ok_or(StoryboardError::CanvasTooLarge)?;
        cards.push(StepCard {
            label,
            image,
            height,
        });
    }

    let mut canvas_height = opts
        .outer_padding
        .checked_mul(2)
        .ok_or(StoryboardError::CanvasTooLarge)?;
    for (i, card) in cards.iter().enumerate() {
        canvas_height = canvas_height
            .checked_add(card.height)
            .ok_or(StoryboardError::CanvasTooLarge)?;
        if i + 1 < cards.len() {
            canvas_height = canvas_height
                .checked_add(opts.card_spacing)
                .ok_or(StoryboardError::CanvasTooLarge)?;
        }
    }
    if canvas_width as u64 * canvas_height as u64 > opts.max_canvas_pixels {
        return Err(StoryboardError::CanvasTooLarge);
    }

    let mut canvas = RgbaImage::from_pixel(canvas_width, canvas_height.max(1), WHITE);
    render_cards(&mut canvas, &cards, &opts);
    write_png_atomic(out_path, &canvas)?;

    Ok(StoryboardExportResult {
        path: out_path.to_path_buf(),
        width: canvas.width(),
        height: canvas.height(),
        step_count: cards.len(),
    })
}

fn fit_label(label: &str, max_width: f32) -> String {
    if text_width(label) <= max_width {
        return label.to_string();
    }

    let suffix = "...";
    let mut prefix = label.to_string();
    while !prefix.is_empty() {
        prefix.pop();
        let trimmed = prefix.trim_end();
        let candidate = format!("{trimmed}{suffix}");
        if text_width(&candidate) <= max_width {
            return candidate;
        }
        prefix = trimmed.to_string();
    }

    if text_width(suffix) <= max_width {
        suffix.to_string()
    } else {
        String::new()
    }
}

fn text_width(label: &str) -> f32 {
    measure_block(label, LABEL_FONT_PX, true).0
}

fn render_cards(canvas: &mut RgbaImage, cards: &[StepCard], opts: &StoryboardOptions) {
    let card_x = opts.outer_padding;
    let card_width = canvas.width().saturating_sub(opts.outer_padding.saturating_mul(2));
    let mut y = opts.outer_padding;

    for card in cards {
        fill_rect(canvas, card_x, y, card_width, card.height, CARD_BACKGROUND);
        stroke_rect(canvas, card_x, y, card_width, card.height, BORDER);

        let content_x = card_x.saturating_add(opts.card_padding);
        let label_y = y.saturating_add(opts.card_padding);
        draw_text_block(
            canvas,
            ImagePoint::new(content_x as f32, label_y as f32),
            &card.label,
            LABEL_FONT_PX,
            true,
            TEXT_COLOR,
        );

        let (_, label_height) = measure_block(&card.label, LABEL_FONT_PX, true);
        let image_y = label_y
            .saturating_add(label_height.ceil() as u32)
            .saturating_add(LABEL_GAP);
        imageops::overlay(canvas, &card.image, content_x.into(), image_y.into());

        y = y
            .saturating_add(card.height)
            .saturating_add(opts.card_spacing);
    }
}

fn downscale(image: &RgbaImage, max_width: u32) -> RgbaImage {
    let width = image.width();
    if width == 0 || width <= max_width {
        return image.clone();
    }
    let height = (image.height() as u64 * max_width as u64 / width as u64).max(1) as u32;
    imageops::resize(image, max_width, height, imageops::FilterType::Triangle)
}

fn fill_rect(image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, color: Rgba<u8>) {
    let max_x = x.saturating_add(width).min(image.width());
    let max_y = y.saturating_add(height).min(image.height());
    for py in y..max_y {
        for px in x..max_x {
            image.put_pixel(px, py, color);
        }
    }
}

fn stroke_rect(image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, color: Rgba<u8>) {
    if width == 0 || height == 0 {
        return;
    }
    let right = x.saturating_add(width - 1).min(image.width().saturating_sub(1));
    let bottom = y.saturating_add(height - 1).min(image.height().saturating_sub(1));
    for px in x..=right {
        if y < image.height() {
            image.put_pixel(px, y, color);
        }
        if bottom < image.height() {
            image.put_pixel(px, bottom, color);
        }
    }
    for py in y..=bottom {
        if x < image.width() {
            image.put_pixel(x, py, color);
        }
        if right < image.width() {
            image.put_pixel(right, py, color);
        }
    }
}

fn temp_png_path(path: &Path) -> PathBuf {
    path.with_extension("png.tmp")
}

fn write_png_atomic(path: &Path, image: &RgbaImage) -> Result<(), StoryboardError> {
    let tmp = temp_png_path(path);
    let _ = std::fs::remove_file(&tmp);
    if let Err(source) = image.save_with_format(&tmp, image::ImageFormat::Png) {
        let _ = std::fs::remove_file(&tmp);
        return Err(StoryboardError::Encode {
            path: tmp.display().to_string(),
            source,
        });
    }
    std::fs::rename(&tmp, path).map_err(|source| {
        let _ = std::fs::remove_file(&tmp);
        StoryboardError::Io {
            path: path.display().to_string(),
            source,
        }
    })
}
```

- [ ] **Step 11: Run storyboard tests**

Run:

```bash
rtk cargo test -p rollshot-action storyboard
```

Expected: PASS.

- [ ] **Step 12: Run all action tests**

Run:

```bash
rtk cargo test -p rollshot-action
```

Expected: PASS.

- [ ] **Step 13: Commit**

```bash
rtk git add crates/rollshot-action/Cargo.toml crates/rollshot-action/src/error.rs crates/rollshot-action/src/lib.rs crates/rollshot-action/src/storyboard.rs
rtk git commit -m "feat(action): add storyboard png export"
```

## Task 3: Wire Storyboard Export Into Timeline Workspace

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`

- [ ] **Step 1: Add failing app update tests**

In `crates/rollshot-app/src/timeline_workspace/update.rs`, add these tests near the existing GIF export tests:

```rust
    #[test]
    fn export_storyboard_path_chosen_writes_file_and_keeps_window_open() {
        let mut state = ws(recording_from_frames());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("storyboard.png");
        let _ = update(
            &mut state,
            Message::ExportStoryboardPathChosen(Some(path.clone())),
        );
        assert!(path.exists(), "Storyboard PNG should be written");
        assert!(
            state
                .message
                .as_ref()
                .is_some_and(|m| m.contains("Storyboard saved")),
            "success banner expected, got {:?}",
            state.message
        );
    }

    #[test]
    fn export_storyboard_empty_guide_sets_error_and_writes_nothing() {
        let mut state = ws(synthetic_recording(0));
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("storyboard.png");
        let _ = update(
            &mut state,
            Message::ExportStoryboardPathChosen(Some(path.clone())),
        );
        assert!(!path.exists(), "empty guide must not write a storyboard");
        assert!(
            state.message.as_ref().is_some_and(|m| m.contains("failed")),
            "failure banner expected, got {:?}",
            state.message
        );
    }

    #[test]
    fn export_storyboard_cancelled_picker_is_a_no_op() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::ExportStoryboardPathChosen(None));
        assert!(state.message.is_none());
    }
```

- [ ] **Step 2: Run the new app tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app timeline_workspace::update::tests::export_storyboard
```

Expected: FAIL because the storyboard message variant does not exist.

- [ ] **Step 3: Import the exporter**

In `crates/rollshot-app/src/timeline_workspace/update.rs`, replace:

```rust
use rollshot_action::{export_gif, export_guide, export_video, GifOptions, VideoOptions};
```

with:

```rust
use rollshot_action::{
    export_gif, export_guide, export_storyboard, export_video, GifOptions, StoryboardOptions,
    VideoOptions,
};
```

- [ ] **Step 4: Add message variants**

In the `Message` enum in `crates/rollshot-app/src/timeline_workspace/update.rs`, add these variants after `ExportGifPathChosen`:

```rust
    ExportStoryboardRequested,
    ExportStoryboardPathChosen(Option<PathBuf>),
```

- [ ] **Step 5: Add update handling**

In the `match message` block in `update`, add this arm after the GIF export arm and before `ExportBugReport`:

```rust
        Message::ExportStoryboardRequested => {
            state.message = None;
            Task::perform(
                pick_storyboard_save_path(picker_default_dir()),
                Message::ExportStoryboardPathChosen,
            )
        }
        Message::ExportStoryboardPathChosen(None) => Task::none(),
        Message::ExportStoryboardPathChosen(Some(path)) => {
            match export_storyboard(
                &state.guide,
                &state.store,
                StoryboardOptions::default(),
                &path,
            ) {
                Ok(result) => {
                    tracing::info!(
                        target: "rollshot::action::export",
                        path = %result.path.display(),
                        steps = result.step_count,
                        width = result.width,
                        height = result.height,
                        "storyboard exported"
                    );
                    state.message = Some(format!("Storyboard saved to {}", result.path.display()));
                }
                Err(error) => {
                    tracing::error!(
                        target: "rollshot::action::export",
                        %error,
                        path = %path.display(),
                        "storyboard export failed"
                    );
                    state.message = Some(format!("Storyboard export failed: {error}"));
                }
            }
            Task::none()
        }
```

- [ ] **Step 6: Add the save dialog helper**

In `crates/rollshot-app/src/timeline_workspace/update.rs`, add this helper after `pick_gif_save_path`:

```rust
async fn pick_storyboard_save_path(default_dir: PathBuf) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_directory(default_dir)
        .set_file_name("storyboard.png")
        .add_filter("PNG image", &["png"])
        .save_file()
        .await
        .map(|handle| handle.path().to_path_buf())
}
```

- [ ] **Step 7: Add the toolbar button**

In `crates/rollshot-app/src/timeline_workspace/view.rs`, add the button between `Export GIF` and `Export MP4`:

```rust
        button(text("Export Storyboard"))
            .on_press(Message::ExportStoryboardRequested)
            .style(button::secondary),
```

The export portion of `header` should read:

```rust
        button(text("Export GIF"))
            .on_press(Message::ExportGifRequested)
            .style(button::secondary),
        button(text("Export Storyboard"))
            .on_press(Message::ExportStoryboardRequested)
            .style(button::secondary),
        button(text("Export MP4"))
            .on_press(Message::ExportMp4Requested)
            .style(button::secondary),
        button(text("Export Guide"))
            .on_press(Message::ExportRequested)
            .style(button::primary),
```

- [ ] **Step 8: Run the focused app tests**

Run:

```bash
rtk cargo test -p rollshot-app timeline_workspace::update::tests::export_storyboard
```

Expected: PASS.

- [ ] **Step 9: Run all timeline workspace tests**

Run:

```bash
rtk cargo test -p rollshot-app timeline_workspace
```

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "feat(app): wire storyboard export action"
```

## Task 4: Verification

**Files:**
- No source changes expected.

- [ ] **Step 1: Run formatting**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS. If it fails, run `rtk cargo fmt`, then re-run `rtk cargo fmt --check`.

- [ ] **Step 2: Run focused package tests**

Run:

```bash
rtk cargo test -p rollshot-image-document
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-app timeline_workspace
```

Expected: PASS for all three commands.

- [ ] **Step 3: Run workspace tests**

Run:

```bash
rtk cargo test
```

Expected: PASS.

- [ ] **Step 4: Run clippy because this touches public APIs and UI wiring**

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Commit verification-only formatting changes if any**

If `rtk cargo fmt` changed files, commit them:

```bash
rtk git add crates/rollshot-image-document crates/rollshot-action crates/rollshot-app
rtk git commit -m "style: format storyboard export changes"
```

If there were no formatting changes, do not create a commit.

## Self-Review

- PRD FR1 is covered by Task 3 message and toolbar button.
- PRD FR2 is covered by using `state.guide` and `guide.steps()` in `export_storyboard`.
- PRD FR3 is covered by resolving each `step.keyframe` through `FrameStore::retained`.
- PRD FR4 and FR5 are covered by `Step N - title` label rendering.
- PRD FR6 is covered by PNG-only `export_storyboard`.
- PRD FR7 is covered by `StoryboardOptions` and deterministic card layout.
- PRD FR8 is covered by `StoryboardError` and app banner handling.
- PRD FR9 is covered because exporters take immutable `&Guide` and `&FrameStore`, and the app keeps the workspace open.
- No platform-specific capture UI paths are changed; this is Action Guide review/export UI only.
- No stitching path is touched, so the core benchmark workflow is not required.
