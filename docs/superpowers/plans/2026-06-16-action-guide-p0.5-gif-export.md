# Action Guide P0.5 — Basic Summary GIF Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an opt-in "Export GIF" action to the Action Guide Timeline Workspace that assembles the final guide's keyframes into one infinitely-looping summary GIF.

**Architecture:** A pure transform `export_gif(guide, store, opts, out_path)` in the framework-neutral `rollshot-action` crate (new `src/gif.rs`, sibling of `export.rs`) does the encoding; the shared `rollshot-app` Timeline Workspace module adds a button + file dialog that calls it. GIF export is fully independent of Markdown export — separate button, separate code path, separate output file — and never blocks or alters it.

**Tech Stack:** Rust, `image` 0.25 (GIF codec), `iced` 0.14 (Timeline Workspace UI), `rfd` (save dialog).

**Spec:** `docs/superpowers/specs/2026-06-16-action-guide-p0.5-gif-export-design.md`

---

## File Structure

- `crates/rollshot-action/Cargo.toml` — enable the `image` `gif` codec feature.
- `crates/rollshot-action/src/error.rs` — add `GifError`.
- `crates/rollshot-action/src/gif.rs` — **new**: `GifOptions`, `export_gif`, private `downscale`/`write_atomic` helpers, unit tests.
- `crates/rollshot-action/src/lib.rs` — register `mod gif;`, export `GifError`, `export_gif`, `GifOptions`.
- `crates/rollshot-app/src/timeline_workspace/update.rs` — `ExportGifRequested` / `ExportGifPathChosen` messages, `pick_gif_save_path`, handlers, tests.
- `crates/rollshot-app/src/timeline_workspace/view.rs` — "Export GIF" button in the header.

The Timeline Workspace module is shared by the Linux `run()` path and the macOS `Phase::Timeline` path, so the `update.rs`/`view.rs` edits cover both platforms by construction (AGENTS.md §8). macOS is not runtime-verified on the Linux dev host.

---

## Task 1: `rollshot-action` — `export_gif` engine

**Files:**
- Modify: `crates/rollshot-action/Cargo.toml`
- Modify: `crates/rollshot-action/src/error.rs`
- Create: `crates/rollshot-action/src/gif.rs`
- Modify: `crates/rollshot-action/src/lib.rs`

- [ ] **Step 1: Enable the GIF codec**

In `crates/rollshot-action/Cargo.toml`, change the `image` dependency line:

```toml
image = { workspace = true, features = ["gif"] }
```

(Was `image = { workspace = true }`. Cargo unions features, so this turns on the `gif` encoder/decoder for this crate without editing the workspace default features or adding a new crate.)

- [ ] **Step 2: Add `GifError`**

In `crates/rollshot-action/src/error.rs`, add this enum after `ExportError`:

```rust
/// Summary-GIF export failure. On any error, no file is left at the target path
/// and the editable session stays intact.
#[derive(Debug, thiserror::Error)]
pub enum GifError {
    #[error("cannot export a GIF for a guide with no steps")]
    Empty,
    #[error("step {index} keyframe pixels were not retained")]
    KeyframeMissing { index: usize },
    #[error("failed to encode GIF: {source}")]
    Encode {
        #[source]
        source: image::ImageError,
    },
    #[error("GIF I/O error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}
```

Then add this test inside the existing `#[cfg(test)] mod tests` block in `error.rs`:

```rust
#[test]
fn gif_error_messages_are_descriptive() {
    assert_eq!(
        GifError::Empty.to_string(),
        "cannot export a GIF for a guide with no steps"
    );
    let missing = GifError::KeyframeMissing { index: 2 };
    assert!(missing.to_string().contains("step 2"), "{missing}");
}
```

- [ ] **Step 3: Register the module and exports**

In `crates/rollshot-action/src/lib.rs`:

1. Add `mod gif;` between `mod frame_store;` and `mod guide;`.
2. Change the error re-export line to include `GifError`:

```rust
pub use error::{DetectError, ExportError, GifError};
```

3. Add this line after the `pub use export::{...};` line:

```rust
pub use gif::{export_gif, GifOptions};
```

- [ ] **Step 4: Write the failing tests**

Create `crates/rollshot-action/src/gif.rs` containing ONLY this test module for now (the implementation goes in Step 6):

```rust
//! Basic summary-GIF export: assemble the final guide's keyframes into one
//! infinitely-looping GIF. A visual companion to `steps.md`, built from the
//! reviewed/edited keyframes only — never from the raw frame stream. One frame
//! per step, fixed dwell, downscaled to a max width for predictable size.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorConfig;
    use crate::frame_store::StoreConfig;
    use crate::models::{CandidateKind, CandidateStep, CaptureRegion, DetectReason};
    use crate::recorder::{ActionRecorder, Recording};
    use image::codecs::gif::GifDecoder;
    use image::{AnimationDecoder, Rgba};
    use std::path::PathBuf;

    fn region() -> CaptureRegion {
        CaptureRegion { x: 0, y: 0, width: 8, height: 8 }
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
    fn temp_path(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "rollshot-gif-{label}-{nanos}-{}.gif",
            std::process::id()
        ))
    }

    /// A real recording with retained frames (mirrors the export.rs fixture).
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

    fn one_step_guide(kf: crate::models::FrameId) -> Guide {
        Guide::from_candidates(vec![CandidateStep {
            id: 0,
            kind: CandidateKind::Click,
            reason: DetectReason::ClickConfirmed,
            at_ms: 0,
            keyframe: kf,
            nearby: vec![kf],
        }])
    }

    fn decode_frames(path: &PathBuf) -> Vec<image::Frame> {
        let file = std::fs::File::open(path).expect("open gif");
        GifDecoder::new(std::io::BufReader::new(file))
            .expect("gif decoder")
            .into_frames()
            .collect_frames()
            .expect("collect frames")
    }

    #[test]
    fn exports_one_frame_per_step() {
        let store = recording().store;
        let kf = store.retained_ids_for_test()[0];
        let guide = Guide::from_candidates(vec![
            CandidateStep {
                id: 0,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 0,
                keyframe: kf,
                nearby: vec![kf],
            },
            CandidateStep {
                id: 1,
                kind: CandidateKind::Scroll,
                reason: DetectReason::ScrollSettled,
                at_ms: 100,
                keyframe: kf,
                nearby: vec![kf],
            },
        ]);
        let path = temp_path("two-steps");
        export_gif(&guide, &store, GifOptions::default(), &path).expect("export");
        assert!(path.exists());
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
        assert_eq!(decode_frames(&path).len(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn downscales_frames_wider_than_max_width() {
        let store = recording().store;
        let guide = one_step_guide(store.retained_ids_for_test()[0]);
        let path = temp_path("downscale");
        export_gif(
            &guide,
            &store,
            GifOptions { frame_dwell_ms: 100, max_width: 4 },
            &path,
        )
        .expect("export");
        assert_eq!(decode_frames(&path)[0].buffer().width(), 4);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn keeps_frames_narrower_than_max_width() {
        let store = recording().store;
        let guide = one_step_guide(store.retained_ids_for_test()[0]);
        let path = temp_path("native");
        export_gif(
            &guide,
            &store,
            GifOptions { frame_dwell_ms: 100, max_width: 100 },
            &path,
        )
        .expect("export");
        assert_eq!(decode_frames(&path)[0].buffer().width(), 8);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_guide_is_an_error() {
        let store = FrameStore::new(StoreConfig::default());
        let guide = Guide::from_candidates(vec![]);
        let path = temp_path("empty");
        let result = export_gif(&guide, &store, GifOptions::default(), &path);
        assert!(matches!(result, Err(GifError::Empty)));
        assert!(!path.exists());
    }
}
```

- [ ] **Step 5: Run the tests to verify they fail**

Run: `rtk cargo test -p rollshot-action gif`
Expected: FAIL to compile — `cannot find function export_gif`, `cannot find type GifOptions`, `RgbaImage`/`Guide`/`FrameStore` not in scope (these come from the not-yet-written implementation's `use` lines).

- [ ] **Step 6: Implement `export_gif`**

Insert this implementation at the TOP of `crates/rollshot-action/src/gif.rs`, immediately after the `//! …` module doc comment and before the `#[cfg(test)]` block:

```rust
use std::path::Path;

use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, RgbaImage};

use crate::error::GifError;
use crate::frame_store::FrameStore;
use crate::guide::Guide;

/// Tunables for summary-GIF assembly. `Default` is the P0.5 "basic" profile.
#[derive(Debug, Clone)]
pub struct GifOptions {
    /// Per-frame display time, milliseconds.
    pub frame_dwell_ms: u32,
    /// Frames wider than this are downscaled (aspect preserved); never upscaled.
    pub max_width: u32,
}

impl Default for GifOptions {
    fn default() -> Self {
        Self {
            frame_dwell_ms: 1500,
            max_width: 800,
        }
    }
}

/// Encode the guide's keyframes into an infinitely-looping GIF at `out_path`.
/// One frame per guide step, in order, using each step's current keyframe.
/// Writes atomically (temp sibling + rename); on any error nothing is left at
/// `out_path` and the editable guide/store are untouched.
pub fn export_gif(
    guide: &Guide,
    store: &FrameStore,
    opts: GifOptions,
    out_path: &Path,
) -> Result<(), GifError> {
    if guide.is_empty() {
        return Err(GifError::Empty);
    }

    // One (possibly downscaled) RGBA frame per step, in order.
    let mut images = Vec::with_capacity(guide.steps().len());
    for (i, step) in guide.steps().iter().enumerate() {
        let retained = store
            .retained(step.keyframe)
            .ok_or(GifError::KeyframeMissing { index: i + 1 })?;
        images.push(downscale(&retained.image, opts.max_width));
    }

    // Encode into an in-memory buffer. The encoder is scoped so it is dropped
    // (and the GIF trailer flushed) before the buffer is read.
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut buf);
        encoder
            .set_repeat(Repeat::Infinite)
            .map_err(|source| GifError::Encode { source })?;
        for image in images {
            let delay = Delay::from_numer_denom_ms(opts.frame_dwell_ms, 1);
            encoder
                .encode_frame(Frame::from_parts(image, 0, 0, delay))
                .map_err(|source| GifError::Encode { source })?;
        }
    }

    write_atomic(out_path, &buf)
}

/// Downscale `image` so its width is at most `max_width`, preserving aspect
/// ratio. Never upscales.
fn downscale(image: &RgbaImage, max_width: u32) -> RgbaImage {
    let width = image.width();
    if width == 0 || width <= max_width {
        return image.clone();
    }
    let height = (image.height() as u64 * max_width as u64 / width as u64).max(1) as u32;
    image::imageops::resize(image, max_width, height, image::imageops::FilterType::Triangle)
}

/// Write `bytes` to `path` atomically: a temp sibling first, then rename.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), GifError> {
    let tmp = path.with_extension("gif.tmp");
    std::fs::write(&tmp, bytes).map_err(|source| GifError::Io {
        path: tmp.display().to_string(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(|source| {
        let _ = std::fs::remove_file(&tmp);
        GifError::Io {
            path: path.display().to_string(),
            source,
        }
    })
}
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `rtk cargo test -p rollshot-action gif`
Expected: PASS — `exports_one_frame_per_step`, `downscales_frames_wider_than_max_width`, `keeps_frames_narrower_than_max_width`, `empty_guide_is_an_error`, and `gif_error_messages_are_descriptive` all pass.

- [ ] **Step 8: Format and lint**

Run: `rtk cargo fmt -p rollshot-action`
Run: `rtk cargo clippy -p rollshot-action --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 9: Commit**

```bash
rtk git add crates/rollshot-action/Cargo.toml crates/rollshot-action/src/error.rs crates/rollshot-action/src/gif.rs crates/rollshot-action/src/lib.rs Cargo.lock
rtk git commit -m "feat(action): add basic summary GIF export

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `rollshot-app` — "Export GIF" button in the Timeline Workspace

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`

- [ ] **Step 1: Write the failing tests**

In `crates/rollshot-app/src/timeline_workspace/update.rs`, add these tests inside the existing `#[cfg(test)] mod tests` block (it already imports `recording_from_frames`, `synthetic_recording`, and defines the `ws(...)` helper):

```rust
#[test]
fn export_gif_path_chosen_writes_file_and_keeps_window_open() {
    let mut state = ws(recording_from_frames());
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("summary.gif");
    let _ = update(&mut state, Message::ExportGifPathChosen(Some(path.clone())));
    assert!(path.exists(), "GIF file should be written");
    assert!(
        state.message.as_ref().is_some_and(|m| m.contains("GIF saved")),
        "success banner expected, got {:?}",
        state.message
    );
}

#[test]
fn export_gif_empty_guide_sets_error_and_writes_nothing() {
    let mut state = ws(synthetic_recording(0));
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("summary.gif");
    let _ = update(&mut state, Message::ExportGifPathChosen(Some(path.clone())));
    assert!(!path.exists(), "empty guide must not write a file");
    assert!(state.message.is_some(), "failure surfaces an inline message");
}

#[test]
fn export_gif_cancelled_picker_is_a_no_op() {
    let mut state = ws(recording_from_frames());
    let _ = update(&mut state, Message::ExportGifPathChosen(None));
    assert!(state.message.is_none());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `rtk cargo test -p rollshot-app --features action-guide export_gif`
Expected: FAIL to compile — `no variant ExportGifPathChosen` on `Message`.

- [ ] **Step 3: Add the import**

In `crates/rollshot-app/src/timeline_workspace/update.rs`, change the existing import line

```rust
use rollshot_action::export_guide;
```

to:

```rust
use rollshot_action::{export_gif, export_guide, GifOptions};
```

- [ ] **Step 4: Add the message variants**

In the same file, add these two variants to the `Message` enum (after `ExportDirChosen(Option<PathBuf>)`):

```rust
    ExportGifRequested,
    ExportGifPathChosen(Option<PathBuf>),
```

- [ ] **Step 5: Add the message handlers**

In the `update` function's `match message { … }`, add these arms (place them after the `Message::ExportDirChosen(Some(dir)) => …` arm):

```rust
        Message::ExportGifRequested => {
            state.message = None;
            Task::perform(
                pick_gif_save_path(picker_default_dir()),
                Message::ExportGifPathChosen,
            )
        }
        Message::ExportGifPathChosen(None) => Task::none(),
        Message::ExportGifPathChosen(Some(path)) => {
            match export_gif(&state.guide, &state.store, GifOptions::default(), &path) {
                Ok(()) => {
                    tracing::info!(
                        target: "rollshot::action::export",
                        path = %path.display(),
                        "gif exported"
                    );
                    state.message = Some(format!("GIF saved to {}", path.display()));
                }
                Err(error) => {
                    tracing::error!(
                        target: "rollshot::action::export",
                        %error,
                        "gif export failed"
                    );
                    state.message = Some(format!("GIF export failed: {error}"));
                }
            }
            // Unlike guide export, GIF export does NOT exit — the user can still
            // Export Guide afterwards.
            Task::none()
        }
```

- [ ] **Step 6: Add the save-dialog helper**

In the same file, add this async helper next to the existing `pick_export_dir`:

```rust
async fn pick_gif_save_path(default_dir: PathBuf) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_directory(default_dir)
        .set_file_name("summary.gif")
        .add_filter("GIF image", &["gif"])
        .save_file()
        .await
        .map(|handle| handle.path().to_path_buf())
}
```

- [ ] **Step 7: Add the button to the view**

In `crates/rollshot-app/src/timeline_workspace/view.rs`, in the `header` function's `row![…]`, add the "Export GIF" button between the "Discard" button and the "Export Guide" button:

```rust
        button(text("Discard"))
            .on_press(Message::DiscardRequested)
            .style(button::secondary),
        button(text("Export GIF"))
            .on_press(Message::ExportGifRequested)
            .style(button::secondary),
        button(text("Export Guide"))
            .on_press(Message::ExportRequested)
            .style(button::primary),
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `rtk cargo test -p rollshot-app --features action-guide export_gif`
Expected: PASS — the three `export_gif_*` tests pass.

- [ ] **Step 9: Run the full Timeline Workspace + action suites**

Run: `rtk cargo test -p rollshot-app --features action-guide`
Run: `rtk cargo test -p rollshot-action`
Expected: PASS (no regressions). The existing `view_builds_for_selected_empty_and_discard_states` test now also renders the new button.

- [ ] **Step 10: Format and lint**

Run: `rtk cargo fmt -p rollshot-app`
Run: `rtk cargo clippy -p rollshot-app --features action-guide --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 11: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "feat(app): add Export GIF button to action guide timeline

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Final verification

- [ ] `rtk cargo test -p rollshot-action` — green
- [ ] `rtk cargo test -p rollshot-app --features action-guide` — green
- [ ] `rtk cargo fmt --check` — clean
- [ ] `rtk cargo clippy -p rollshot-action --all-targets -- -D warnings` — clean
- [ ] `rtk cargo clippy -p rollshot-app --features action-guide --all-targets -- -D warnings` — clean

## Manual verification (Linux, optional but recommended)

1. `rtk cargo run -p rollshot-cli --features action-guide -- action-guide` (or the app's `--action-guide` launch), record a short workflow, finish.
2. In the Timeline Workspace, click **Export GIF**, choose a path, confirm `summary.gif` is written and loops.
3. Confirm **Export Guide** still works afterwards (window stayed open), and that an export of an emptied guide surfaces an inline error.

## Notes / risks

- **macOS not runtime-verified.** Changes are in shared code (`gif.rs` + shared `timeline_workspace/`), so the macOS `Phase::Timeline` path is covered by construction, but it is not exercised at runtime on the Linux dev host (AGENTS.md §8).
- **GIF palette.** `image`'s `GifEncoder` quantizes each frame to 256 colors independently (NeuQuant). Acceptable for the P0.5 "basic" profile; richer palette/markers/captions are deferred to Phase 4.
