# Action Guide MP4 Summary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Action Guide `Export MP4` as a reviewed-keyframe summary video with explicit managed FFmpeg download when system FFmpeg is unavailable.

**Architecture:** `rollshot-action` owns platform-neutral MP4 assembly and FFmpeg encoding from retained keyframes. `rollshot-app` owns FFmpeg discovery, managed download/install state, timeline UI, and user consent. Managed FFmpeg uses pinned per-platform metadata and `ffmpeg-sidecar` lower-level download/unpack helpers rather than implicit auto-download.

**Tech Stack:** Rust 2021, `image`, `iced 0.14`, `ffmpeg-sidecar 2.5.2`, `sha2`, `serde_json`, `chrono`, `tracing`, `rfd`.

---

## File Structure

- Modify `Cargo.toml`: add workspace dependency `ffmpeg-sidecar = { version = "2.5.2", default-features = false, features = ["download_ffmpeg"] }`.
- Modify `crates/rollshot-action/Cargo.toml`: depend on workspace `ffmpeg-sidecar`.
- Modify `crates/rollshot-action/src/error.rs`: add `VideoError`.
- Modify `crates/rollshot-action/src/lib.rs`: register `mod video;` and public re-exports.
- Create `crates/rollshot-action/src/video.rs`: pure frame preparation helpers, FFmpeg args builder, and `export_video`.
- Modify `crates/rollshot-app/Cargo.toml`: depend on workspace `ffmpeg-sidecar`; move `sha2 = "0.10"` from dev-dependencies to dependencies.
- Modify `crates/rollshot-app/src/main.rs`: register `managed_ffmpeg` behind `action-guide`.
- Create `crates/rollshot-app/src/managed_ffmpeg.rs`: FFmpeg resolution, validation, pinned metadata, sha256 verification, managed manifest, managed download/install.
- Modify `crates/rollshot-app/src/timeline_workspace/mod.rs`: add FFmpeg setup dialog state.
- Modify `crates/rollshot-app/src/timeline_workspace/update.rs`: add MP4 export messages, FFmpeg setup messages, save picker, download task.
- Modify `crates/rollshot-app/src/timeline_workspace/view.rs`: add `Export MP4` button and FFmpeg-required modal.

## Task 1: Dependencies And Public Error Surface

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/rollshot-action/Cargo.toml`
- Modify: `crates/rollshot-action/src/error.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Modify: `crates/rollshot-app/Cargo.toml`

- [ ] **Step 1: Add dependency declarations**

In workspace `Cargo.toml`, add this line under `[workspace.dependencies]` near the other shared crates:

```toml
ffmpeg-sidecar = { version = "2.5.2", default-features = false, features = ["download_ffmpeg"] }
```

In `crates/rollshot-action/Cargo.toml`, add:

```toml
ffmpeg-sidecar = { workspace = true }
```

In `crates/rollshot-app/Cargo.toml`, add to `[dependencies]`:

```toml
ffmpeg-sidecar = { workspace = true }
sha2 = "0.10"
```

Remove `sha2 = "0.10"` from `[dev-dependencies]` in `crates/rollshot-app/Cargo.toml`.

- [ ] **Step 2: Add `VideoError` test first**

Append to the existing tests in `crates/rollshot-action/src/error.rs`:

```rust
    #[test]
    fn video_error_messages_are_descriptive() {
        assert_eq!(
            VideoError::Empty.to_string(),
            "cannot export an MP4 for a guide with no steps"
        );
        let missing = VideoError::KeyframeMissing { index: 3 };
        assert!(missing.to_string().contains("step 3"), "{missing}");
        let invalid = VideoError::InvalidFfmpeg {
            path: "/missing/ffmpeg".to_string(),
        };
        assert!(
            invalid.to_string().contains("/missing/ffmpeg"),
            "{invalid}"
        );
    }
```

- [ ] **Step 3: Run the focused error test and verify it fails**

Run:

```bash
rtk cargo test -p rollshot-action video_error_messages_are_descriptive
```

Expected: FAIL because `VideoError` is not defined.

- [ ] **Step 4: Add `VideoError`**

In `crates/rollshot-action/src/error.rs`, after `GifError`, add:

```rust
/// Summary-MP4 export failure. On any error, no file is left at the target path
/// and the editable session stays intact.
#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    #[error("cannot export an MP4 for a guide with no steps")]
    Empty,
    #[error("step {index} keyframe pixels were not retained")]
    KeyframeMissing { index: usize },
    #[error("FFmpeg binary is not usable at {path}")]
    InvalidFfmpeg { path: String },
    #[error("failed to spawn FFmpeg at {path}: {source}")]
    Spawn {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write raw frames to FFmpeg stdin: {source}")]
    Stdin {
        #[source]
        source: std::io::Error,
    },
    #[error("FFmpeg exited unsuccessfully with status {status}: {stderr}")]
    Exit { status: String, stderr: String },
    #[error("MP4 I/O error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}
```

In `crates/rollshot-action/src/lib.rs`, change the error re-export to:

```rust
pub use error::{DetectError, ExportError, GifError, VideoError};
```

- [ ] **Step 5: Run the focused test and verify it passes**

Run:

```bash
rtk cargo test -p rollshot-action video_error_messages_are_descriptive
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add Cargo.toml crates/rollshot-action/Cargo.toml crates/rollshot-action/src/error.rs crates/rollshot-action/src/lib.rs crates/rollshot-app/Cargo.toml Cargo.lock
rtk git commit -m "build(action-guide): add ffmpeg video export dependencies"
```

## Task 2: Pure MP4 Frame Preparation Helpers

**Files:**
- Create: `crates/rollshot-action/src/video.rs`
- Modify: `crates/rollshot-action/src/lib.rs`

- [ ] **Step 1: Register the module**

In `crates/rollshot-action/src/lib.rs`, add:

```rust
mod video;
```

and add:

```rust
pub use video::{export_video, VideoOptions};
```

- [ ] **Step 2: Create failing helper tests**

Create `crates/rollshot-action/src/video.rs` with the module header, type, and tests:

```rust
//! Summary-MP4 export: assemble the final guide's reviewed keyframes into a
//! short H.264 MP4. This is a workflow summary, not raw screen recording.

use std::path::Path;

use image::RgbaImage;

use crate::error::VideoError;
use crate::frame_store::FrameStore;
use crate::guide::Guide;

/// Tunables for summary-MP4 assembly.
#[derive(Debug, Clone)]
pub struct VideoOptions {
    /// Per-keyframe display time, milliseconds.
    pub frame_dwell_ms: u32,
    /// Output frame rate.
    pub fps: u32,
    /// Frames wider than this are downscaled; never upscaled.
    pub max_width: u32,
}

impl Default for VideoOptions {
    fn default() -> Self {
        Self {
            frame_dwell_ms: 1500,
            fps: 30,
            max_width: 1280,
        }
    }
}

pub fn export_video(
    guide: &Guide,
    store: &FrameStore,
    opts: VideoOptions,
    ffmpeg_path: &Path,
    out_path: &Path,
) -> Result<(), VideoError> {
    let _ = (guide, store, opts, ffmpeg_path, out_path);
    Err(VideoError::InvalidFfmpeg {
        path: ffmpeg_path.display().to_string(),
    })
}

fn repeat_count(frame_dwell_ms: u32, fps: u32) -> u32 {
    let _ = (frame_dwell_ms, fps);
    0
}

fn even_dimension(value: u32) -> u32 {
    let _ = value;
    0
}

fn downscale(image: &RgbaImage, max_width: u32) -> RgbaImage {
    let _ = max_width;
    image.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn repeat_count_rounds_up_and_never_returns_zero() {
        assert_eq!(repeat_count(1500, 30), 45);
        assert_eq!(repeat_count(1, 30), 1);
        assert_eq!(repeat_count(0, 30), 1);
    }

    #[test]
    fn even_dimension_rounds_odd_values_down_but_keeps_minimum_two() {
        assert_eq!(even_dimension(101), 100);
        assert_eq!(even_dimension(100), 100);
        assert_eq!(even_dimension(1), 2);
        assert_eq!(even_dimension(0), 2);
    }

    #[test]
    fn downscale_preserves_aspect_ratio_and_never_upscales() {
        let wide = RgbaImage::from_pixel(10, 5, Rgba([1, 2, 3, 255]));
        let scaled = downscale(&wide, 4);
        assert_eq!((scaled.width(), scaled.height()), (4, 2));

        let native = downscale(&wide, 20);
        assert_eq!((native.width(), native.height()), (10, 5));
    }
}
```

- [ ] **Step 3: Run helper tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-action video::tests
```

Expected: FAIL for repeat count, even dimensions, and downscale behavior.

- [ ] **Step 4: Implement the pure helpers**

Replace the helper bodies in `crates/rollshot-action/src/video.rs` with:

```rust
fn repeat_count(frame_dwell_ms: u32, fps: u32) -> u32 {
    let fps = fps.max(1) as u64;
    let dwell = frame_dwell_ms as u64;
    let frames = (dwell * fps).div_ceil(1000);
    frames.max(1) as u32
}

fn even_dimension(value: u32) -> u32 {
    if value <= 2 {
        2
    } else if value % 2 == 1 {
        value - 1
    } else {
        value
    }
}

fn downscale(image: &RgbaImage, max_width: u32) -> RgbaImage {
    let width = image.width();
    if width == 0 || max_width == 0 || width <= max_width {
        return image.clone();
    }
    let height = (image.height() as u64 * max_width as u64 / width as u64).max(1) as u32;
    image::imageops::resize(
        image,
        max_width,
        height,
        image::imageops::FilterType::Triangle,
    )
}
```

- [ ] **Step 5: Run helper tests and verify they pass**

Run:

```bash
rtk cargo test -p rollshot-action video::tests
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-action/src/lib.rs crates/rollshot-action/src/video.rs
rtk git commit -m "feat(action-guide): add mp4 frame preparation helpers"
```

## Task 3: MP4 Exporter And FFmpeg Command Assembly

**Files:**
- Modify: `crates/rollshot-action/src/video.rs`

- [ ] **Step 1: Add failing command and export tests**

Extend `crates/rollshot-action/src/video.rs` tests with:

```rust
    use crate::detector::DetectorConfig;
    use crate::frame_store::StoreConfig;
    use crate::models::{CandidateKind, CandidateStep, CaptureRegion, DetectReason};
    use crate::recorder::{ActionRecorder, Recording};
    use std::path::{Path, PathBuf};

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

    fn white() -> RgbaImage {
        RgbaImage::from_pixel(8, 8, Rgba([255, 255, 255, 255]))
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
            rec.ingest_frame(white(), i * 100);
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

    fn temp_path(label: &str, ext: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "rollshot-video-{label}-{nanos}-{}.{}",
            std::process::id(),
            ext
        ))
    }

    fn fake_ffmpeg(path: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(
                path,
                "#!/bin/sh\nif [ \"$1\" = \"-version\" ]; then echo 'ffmpeg fake'; exit 0; fi\nout=\"$1\"\nfor arg in \"$@\"; do out=\"$arg\"; done\ncat >/dev/null\nprintf 'fake mp4' > \"$out\"\n",
            )
            .unwrap();
            let mut perms = std::fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).unwrap();
        }
    }

    #[test]
    fn ffmpeg_args_describe_raw_rgba_to_h264_mp4() {
        let out = Path::new("/tmp/summary.tmp.mp4");
        let args = ffmpeg_args(640, 480, 30, out);
        assert_eq!(
            args,
            vec![
                "-y",
                "-f",
                "rawvideo",
                "-pixel_format",
                "rgba",
                "-video_size",
                "640x480",
                "-framerate",
                "30",
                "-i",
                "pipe:0",
                "-vf",
                "format=yuv420p",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "23",
                "-movflags",
                "+faststart",
                "-f",
                "mp4",
                "/tmp/summary.tmp.mp4",
            ]
        );
    }

    #[test]
    fn empty_guide_is_an_error() {
        let store = FrameStore::new(StoreConfig::default());
        let guide = Guide::from_candidates(vec![]);
        let path = temp_path("empty", "mp4");
        let result = export_video(
            &guide,
            &store,
            VideoOptions::default(),
            Path::new("/bin/false"),
            &path,
        );
        assert!(matches!(result, Err(VideoError::Empty)));
        assert!(!path.exists());
    }

    #[test]
    fn missing_keyframe_errors_and_leaves_no_file() {
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
                kind: CandidateKind::UiChanged,
                reason: DetectReason::VisualChange,
                at_ms: 100,
                keyframe: 999_999,
                nearby: vec![999_999],
            },
        ]);
        let path = temp_path("missing-keyframe", "mp4");
        let result = export_video(
            &guide,
            &store,
            VideoOptions::default(),
            Path::new("/bin/false"),
            &path,
        );
        assert!(matches!(
            result,
            Err(VideoError::KeyframeMissing { index: 2 })
        ));
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn export_invokes_ffmpeg_and_writes_atomically() {
        let store = recording().store;
        let guide = one_step_guide(store.retained_ids_for_test()[0]);
        let dir = tempfile_dir();
        let ffmpeg = dir.join("ffmpeg");
        fake_ffmpeg(&ffmpeg);
        let path = dir.join("summary.mp4");
        export_video(&guide, &store, VideoOptions::default(), &ffmpeg, &path).expect("export");
        assert_eq!(std::fs::read(&path).unwrap(), b"fake mp4");
        assert!(!temp_mp4_path(&path).exists());
    }

    #[cfg(unix)]
    fn tempfile_dir() -> PathBuf {
        let dir = temp_path("dir", "tmp");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
```

- [ ] **Step 2: Run exporter tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-action video::tests
```

Expected: FAIL because `ffmpeg_args` and the real `export_video` behavior are not implemented.

- [ ] **Step 3: Implement command assembly and exporter**

In `crates/rollshot-action/src/video.rs`, add imports:

```rust
use std::io::{Read, Write};

use ffmpeg_sidecar::command::FfmpegCommand;
use image::{imageops, RgbaImage};
```

Replace the `image::RgbaImage` import if it now duplicates `RgbaImage`.

Add these helpers above `export_video`:

```rust
fn ffmpeg_args(width: u32, height: u32, fps: u32, out_path: &Path) -> Vec<String> {
    vec![
        "-y".to_string(),
        "-f".to_string(),
        "rawvideo".to_string(),
        "-pixel_format".to_string(),
        "rgba".to_string(),
        "-video_size".to_string(),
        format!("{width}x{height}"),
        "-framerate".to_string(),
        fps.max(1).to_string(),
        "-i".to_string(),
        "pipe:0".to_string(),
        "-vf".to_string(),
        "format=yuv420p".to_string(),
        "-c:v".to_string(),
        "libx264".to_string(),
        "-preset".to_string(),
        "veryfast".to_string(),
        "-crf".to_string(),
        "23".to_string(),
        "-movflags".to_string(),
        "+faststart".to_string(),
        "-f".to_string(),
        "mp4".to_string(),
        out_path.display().to_string(),
    ]
}

fn temp_mp4_path(path: &Path) -> std::path::PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "summary.mp4".to_string());
    path.with_file_name(format!("{file_name}.tmp.mp4"))
}

fn normalize_to(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    if image.width() == width && image.height() == height {
        return image.clone();
    }
    imageops::resize(image, width, height, imageops::FilterType::Triangle)
}
```

Replace `export_video` with:

```rust
pub fn export_video(
    guide: &Guide,
    store: &FrameStore,
    opts: VideoOptions,
    ffmpeg_path: &Path,
    out_path: &Path,
) -> Result<(), VideoError> {
    if guide.is_empty() {
        return Err(VideoError::Empty);
    }
    if !ffmpeg_path.exists() {
        return Err(VideoError::InvalidFfmpeg {
            path: ffmpeg_path.display().to_string(),
        });
    }

    let mut images = Vec::with_capacity(guide.steps().len());
    for (i, step) in guide.steps().iter().enumerate() {
        let retained = store
            .retained(step.keyframe)
            .ok_or(VideoError::KeyframeMissing { index: i + 1 })?;
        images.push(downscale(&retained.image, opts.max_width));
    }

    let width = even_dimension(images.iter().map(RgbaImage::width).max().unwrap_or(2));
    let height = even_dimension(images.iter().map(RgbaImage::height).max().unwrap_or(2));
    let repeat = repeat_count(opts.frame_dwell_ms, opts.fps);
    let tmp = temp_mp4_path(out_path);
    let _ = std::fs::remove_file(&tmp);

    let args = ffmpeg_args(width, height, opts.fps, &tmp);
    let mut command = FfmpegCommand::new_with_path(ffmpeg_path);
    command.args(args.iter().map(String::as_str));
    let mut child = command.spawn().map_err(|source| VideoError::Spawn {
        path: ffmpeg_path.display().to_string(),
        source,
    })?;

    // Drain stderr while raw frames are written so FFmpeg cannot block on a full stderr pipe.
    let stderr_handle = child.take_stderr().map(|mut stderr| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        })
    });

    {
        let mut stdin = child.take_stdin().ok_or_else(|| VideoError::Stdin {
            source: std::io::Error::new(std::io::ErrorKind::BrokenPipe, "missing FFmpeg stdin"),
        })?;
        for image in images {
            let frame = normalize_to(&image, width, height);
            for _ in 0..repeat {
                stdin
                    .write_all(frame.as_raw())
                    .map_err(|source| VideoError::Stdin { source })?;
            }
        }
        stdin.flush().map_err(|source| VideoError::Stdin { source })?;
    }

    let status = child.wait().map_err(|source| VideoError::Io {
        path: tmp.display().to_string(),
        source,
    })?;
    let stderr_text = stderr_handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    if !status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(VideoError::Exit {
            status: status.to_string(),
            stderr: stderr_text,
        });
    }

    std::fs::rename(&tmp, out_path).map_err(|source| {
        let _ = std::fs::remove_file(&tmp);
        VideoError::Io {
            path: out_path.display().to_string(),
            source,
        }
    })
}
```

- [ ] **Step 4: Run exporter tests and fix compile issues**

Run:

```bash
rtk cargo test -p rollshot-action video::tests
```

Expected: PASS.

- [ ] **Step 5: Add opt-in real FFmpeg integration test**

Append this test in `crates/rollshot-action/src/video.rs`:

```rust
    #[test]
    fn real_ffmpeg_export_is_opt_in() {
        if std::env::var("ROLLSHOT_TEST_FFMPEG").ok().as_deref() != Some("1") {
            return;
        }
        let ffmpeg = std::env::var("ROLLSHOT_FFMPEG")
            .expect("set ROLLSHOT_FFMPEG to an explicit ffmpeg binary path");
        let store = recording().store;
        let guide = one_step_guide(store.retained_ids_for_test()[0]);
        let path = temp_path("real", "mp4");
        export_video(
            &guide,
            &store,
            VideoOptions::default(),
            Path::new(&ffmpeg),
            &path,
        )
        .expect("real ffmpeg export");
        assert!(path.exists());
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
        let _ = std::fs::remove_file(path);
    }
```

- [ ] **Step 6: Run action crate tests**

Run:

```bash
rtk cargo test -p rollshot-action
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-action/src/video.rs
rtk git commit -m "feat(action-guide): export reviewed keyframes as mp4"
```

## Task 4: Managed FFmpeg Metadata, Manifest, And Resolution

**Files:**
- Modify: `crates/rollshot-app/src/main.rs`
- Create: `crates/rollshot-app/src/managed_ffmpeg.rs`

- [ ] **Step 1: Register app module**

In `crates/rollshot-app/src/main.rs`, add near `timeline_workspace`:

```rust
#[cfg(feature = "action-guide")]
mod managed_ffmpeg;
```

- [ ] **Step 2: Create failing manifest and resolution tests**

Create `crates/rollshot-app/src/managed_ffmpeg.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedFfmpegMetadata {
    pub platform: &'static str,
    pub version: &'static str,
    pub source_url: &'static str,
    pub license: &'static str,
    pub license_url: &'static str,
    pub archive_size: u64,
    pub archive_sha256: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ManagedFfmpegManifest {
    pub schema_version: u32,
    pub platform: String,
    pub version: String,
    pub source_url: String,
    pub license: String,
    pub license_url: String,
    pub archive_sha256: String,
    pub archive_size: u64,
    pub binary_path: PathBuf,
    pub ffmpeg_version_line: String,
    pub installed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FfmpegResolution {
    Available(PathBuf),
    NeedsSetup(FfmpegSetupInfo),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FfmpegSetupInfo {
    pub managed_download: Option<ManagedFfmpegMetadata>,
    pub install_location: PathBuf,
}

pub(crate) const LINUX_X86_64_METADATA: ManagedFfmpegMetadata = ManagedFfmpegMetadata {
    platform: "linux-x86_64",
    version: "6.0.1",
    source_url: "https://johnvansickle.com/ffmpeg/old-releases/ffmpeg-6.0.1-amd64-static.tar.xz",
    license: "GPLv3",
    license_url: "https://www.gnu.org/licenses/gpl-3.0.html",
    archive_size: 41_164_188,
    archive_sha256: "28268bf402f1083833ea269331587f60a242848880073be8016501d864bd07a5",
};

pub(crate) fn pinned_metadata_for_current_platform() -> Option<ManagedFfmpegMetadata> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some(LINUX_X86_64_METADATA)
    } else {
        None
    }
}

pub(crate) fn resolve_ffmpeg() -> FfmpegResolution {
    let root = managed_root().unwrap_or_else(|_| std::env::temp_dir().join("rollshot/ffmpeg"));
    FfmpegResolution::NeedsSetup(FfmpegSetupInfo {
        managed_download: pinned_metadata_for_current_platform(),
        install_location: root,
    })
}

fn managed_root() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("ROLLSHOT_FFMPEG_ROOT").map(PathBuf::from) {
        return Ok(path);
    }
    crate::daemon::config::rollshot_config_dir().map(|dir| dir.join("ffmpeg"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_metadata_is_pinned_and_auditable() {
        let meta = LINUX_X86_64_METADATA;
        assert_eq!(meta.platform, "linux-x86_64");
        assert_eq!(meta.version, "6.0.1");
        assert!(meta.source_url.starts_with("https://johnvansickle.com/"));
        assert_eq!(meta.license, "GPLv3");
        assert_eq!(meta.archive_size, 41_164_188);
        assert_eq!(meta.archive_sha256.len(), 64);
    }

    #[test]
    fn manifest_round_trips_json() {
        let manifest = ManagedFfmpegManifest {
            schema_version: 1,
            platform: "linux-x86_64".to_string(),
            version: "6.0.1".to_string(),
            source_url: LINUX_X86_64_METADATA.source_url.to_string(),
            license: "GPLv3".to_string(),
            license_url: LINUX_X86_64_METADATA.license_url.to_string(),
            archive_sha256: LINUX_X86_64_METADATA.archive_sha256.to_string(),
            archive_size: LINUX_X86_64_METADATA.archive_size,
            binary_path: PathBuf::from("/tmp/ffmpeg"),
            ffmpeg_version_line: "ffmpeg version 6.0.1-static".to_string(),
            installed_at: "2026-07-05T00:00:00+00:00".to_string(),
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let restored: ManagedFfmpegManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, manifest);
    }
}
```

- [ ] **Step 3: Run focused app tests and verify initial behavior**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide managed_ffmpeg
```

Expected: PASS for metadata and manifest, with `resolve_ffmpeg` still returning setup until implemented.

- [ ] **Step 4: Implement path validation and resolution**

In `crates/rollshot-app/src/managed_ffmpeg.rs`, add imports:

```rust
use std::process::Command;
```

Add:

```rust
pub(crate) fn manifest_path(root: &Path) -> PathBuf {
    root.join("managed-ffmpeg.json")
}

pub(crate) fn managed_binary_path(root: &Path) -> PathBuf {
    let mut path = root.join("bin").join("ffmpeg");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    path
}

pub(crate) fn validate_ffmpeg(path: &Path) -> Result<String, String> {
    if !path.exists() {
        return Err(format!("FFmpeg does not exist at {}", path.display()));
    }
    let output = Command::new(path)
        .arg("-version")
        .output()
        .map_err(|error| format!("failed to run FFmpeg at {}: {error}", path.display()))?;
    if !output.status.success() {
        return Err(format!(
            "FFmpeg at {} exited with {}",
            path.display(),
            output.status
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().next().unwrap_or("ffmpeg").to_string())
}

fn find_on_path(binary: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(binary);
        if validate_ffmpeg(&candidate).is_ok() {
            return Some(candidate);
        }
    }
    None
}

fn load_manifest(root: &Path) -> Result<ManagedFfmpegManifest, String> {
    let path = manifest_path(root);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read managed FFmpeg manifest: {error}"))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("failed to parse managed FFmpeg manifest: {error}"))
}
```

Replace `resolve_ffmpeg` with:

```rust
pub(crate) fn resolve_ffmpeg() -> FfmpegResolution {
    if let Some(path) = std::env::var_os("ROLLSHOT_FFMPEG").map(PathBuf::from) {
        if validate_ffmpeg(&path).is_ok() {
            return FfmpegResolution::Available(path);
        }
    }

    let binary = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    if let Some(path) = find_on_path(binary) {
        return FfmpegResolution::Available(path);
    }

    let root = managed_root().unwrap_or_else(|_| std::env::temp_dir().join("rollshot/ffmpeg"));
    if let Ok(manifest) = load_manifest(&root) {
        if manifest.schema_version == 1
            && pinned_metadata_for_current_platform()
                .is_some_and(|meta| manifest.platform == meta.platform)
            && validate_ffmpeg(&manifest.binary_path).is_ok()
        {
            return FfmpegResolution::Available(manifest.binary_path);
        }
    }

    FfmpegResolution::NeedsSetup(FfmpegSetupInfo {
        managed_download: pinned_metadata_for_current_platform(),
        install_location: root,
    })
}
```

- [ ] **Step 5: Add validation tests**

Append tests:

```rust
    #[test]
    fn managed_paths_are_stable_under_root() {
        let root = PathBuf::from("/tmp/rollshot-ffmpeg");
        assert_eq!(manifest_path(&root), root.join("managed-ffmpeg.json"));
        let binary = managed_binary_path(&root);
        assert!(binary.ends_with(if cfg!(windows) {
            Path::new("bin/ffmpeg.exe")
        } else {
            Path::new("bin/ffmpeg")
        }));
    }

    #[test]
    fn managed_root_can_be_overridden_for_tests() {
        let _guard = ENV_LOCK.lock().unwrap();
        let old_root = std::env::var_os("ROLLSHOT_FFMPEG_ROOT");
        let root = PathBuf::from("/tmp/rollshot-managed-test-root");
        std::env::set_var("ROLLSHOT_FFMPEG_ROOT", &root);
        assert_eq!(managed_root().unwrap(), root);
        match old_root {
            Some(value) => std::env::set_var("ROLLSHOT_FFMPEG_ROOT", value),
            None => std::env::remove_var("ROLLSHOT_FFMPEG_ROOT"),
        }
    }

    #[test]
    fn validate_ffmpeg_rejects_missing_path() {
        let result = validate_ffmpeg(Path::new("/definitely/missing/ffmpeg"));
        assert!(result.is_err());
    }

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
```

- [ ] **Step 6: Run focused app tests**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide managed_ffmpeg
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/main.rs crates/rollshot-app/src/managed_ffmpeg.rs
rtk git commit -m "feat(app): resolve system and managed ffmpeg"
```

## Task 5: Managed FFmpeg Download And Manifest Write

**Files:**
- Modify: `crates/rollshot-app/src/managed_ffmpeg.rs`

- [ ] **Step 1: Add failing sha and manifest-write tests**

Add imports:

```rust
use sha2::{Digest, Sha256};
```

Append tests:

```rust
    #[test]
    fn sha256_file_detects_content() {
        let dir = tempdir();
        let path = dir.join("archive.bin");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn verify_archive_sha_rejects_mismatch() {
        let dir = tempdir();
        let path = dir.join("archive.bin");
        std::fs::write(&path, b"abc").unwrap();
        let result = verify_archive_sha(&path, "0000");
        assert!(result.unwrap_err().contains("sha256 mismatch"));
    }

    #[test]
    fn write_manifest_persists_valid_json() {
        let dir = tempdir();
        let binary = dir.join("bin/ffmpeg");
        let manifest = build_manifest(
            LINUX_X86_64_METADATA,
            binary.clone(),
            "ffmpeg version 6.0.1-static".to_string(),
        );
        write_manifest(&dir, &manifest).unwrap();
        let restored = load_manifest(&dir).unwrap();
        assert_eq!(restored.binary_path, binary);
        assert_eq!(restored.archive_sha256, LINUX_X86_64_METADATA.archive_sha256);
    }

    fn tempdir() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "rollshot-managed-ffmpeg-{nanos}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide managed_ffmpeg
```

Expected: FAIL because `sha256_file`, `verify_archive_sha`, `build_manifest`, and `write_manifest` are not defined.

- [ ] **Step 3: Implement sha and manifest helpers**

Add:

```rust
pub(crate) fn sha256_file(path: &Path) -> Result<String, String> {
    use std::io::Read;

    let mut file =
        std::fs::File::open(path).map_err(|error| format!("failed to open archive: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|error| format!("failed to hash archive: {error}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn verify_archive_sha(path: &Path, expected: &str) -> Result<(), String> {
    let actual = sha256_file(path)?;
    if actual != expected {
        return Err(format!(
            "managed FFmpeg sha256 mismatch: expected {expected}, got {actual}"
        ));
    }
    Ok(())
}

pub(crate) fn build_manifest(
    metadata: ManagedFfmpegMetadata,
    binary_path: PathBuf,
    ffmpeg_version_line: String,
) -> ManagedFfmpegManifest {
    ManagedFfmpegManifest {
        schema_version: 1,
        platform: metadata.platform.to_string(),
        version: metadata.version.to_string(),
        source_url: metadata.source_url.to_string(),
        license: metadata.license.to_string(),
        license_url: metadata.license_url.to_string(),
        archive_sha256: metadata.archive_sha256.to_string(),
        archive_size: metadata.archive_size,
        binary_path,
        ffmpeg_version_line,
        installed_at: chrono::Utc::now().to_rfc3339(),
    }
}

pub(crate) fn write_manifest(
    root: &Path,
    manifest: &ManagedFfmpegManifest,
) -> Result<(), String> {
    std::fs::create_dir_all(root)
        .map_err(|error| format!("failed to create managed FFmpeg directory: {error}"))?;
    let path = manifest_path(root);
    let text = serde_json::to_string_pretty(manifest)
        .map_err(|error| format!("failed to encode managed FFmpeg manifest: {error}"))?;
    std::fs::write(&path, text)
        .map_err(|error| format!("failed to write managed FFmpeg manifest: {error}"))
}
```

- [ ] **Step 4: Implement managed download**

Add:

```rust
pub(crate) fn download_managed_ffmpeg() -> Result<PathBuf, String> {
    let metadata = pinned_metadata_for_current_platform()
        .ok_or_else(|| "managed FFmpeg is not available for this platform".to_string())?;
    let root = managed_root()?;
    let download_dir = root.join("downloads");
    std::fs::create_dir_all(&download_dir)
        .map_err(|error| format!("failed to create FFmpeg download directory: {error}"))?;

    let archive = ffmpeg_sidecar::download::download_ffmpeg_package_with_progress(
        metadata.source_url,
        &download_dir,
        |event| {
            match event {
                ffmpeg_sidecar::download::FfmpegDownloadProgressEvent::Starting => {
                    tracing::info!(
                        target: "rollshot::action::ffmpeg",
                        source_url = metadata.source_url,
                        "managed FFmpeg download started"
                    );
                }
                ffmpeg_sidecar::download::FfmpegDownloadProgressEvent::Downloading {
                    total_bytes,
                    downloaded_bytes,
                } => {
                    tracing::info!(
                        target: "rollshot::action::ffmpeg",
                        total_bytes,
                        downloaded_bytes,
                        "managed FFmpeg download progress"
                    );
                }
                ffmpeg_sidecar::download::FfmpegDownloadProgressEvent::UnpackingArchive => {
                    tracing::info!(
                        target: "rollshot::action::ffmpeg",
                        "managed FFmpeg unpacking archive"
                    );
                }
                ffmpeg_sidecar::download::FfmpegDownloadProgressEvent::Done => {
                    tracing::info!(
                        target: "rollshot::action::ffmpeg",
                        "managed FFmpeg download complete"
                    );
                }
            }
        },
    )
    .map_err(|error| format!("failed to download managed FFmpeg: {error}"))?;

    if let Err(error) = verify_archive_sha(&archive, metadata.archive_sha256) {
        let _ = std::fs::remove_file(&archive);
        return Err(error);
    }

    let bin_dir = root.join("bin");
    std::fs::create_dir_all(&bin_dir)
        .map_err(|error| format!("failed to create FFmpeg bin directory: {error}"))?;
    ffmpeg_sidecar::download::unpack_ffmpeg_without_extras(&archive, &bin_dir)
        .map_err(|error| format!("failed to unpack managed FFmpeg: {error}"))?;
    let binary = managed_binary_path(&root);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&binary)
            .map_err(|error| format!("failed to inspect managed FFmpeg: {error}"))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&binary, perms)
            .map_err(|error| format!("failed to set FFmpeg executable bit: {error}"))?;
    }

    let version_line = validate_ffmpeg(&binary)?;
    let manifest = build_manifest(metadata, binary.clone(), version_line);
    write_manifest(&root, &manifest)?;
    Ok(binary)
}
```

- [ ] **Step 5: Run focused app tests**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide managed_ffmpeg
```

Expected: PASS. No real internet download runs in tests.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/managed_ffmpeg.rs
rtk git commit -m "feat(app): install managed ffmpeg with manifest"
```

## Task 6: Timeline MP4 Messages And FFmpeg Setup State

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`

- [ ] **Step 1: Add setup state types**

In `crates/rollshot-app/src/timeline_workspace/mod.rs`, add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FfmpegSetupDialog {
    pub info: crate::managed_ffmpeg::FfmpegSetupInfo,
    pub downloading: bool,
}
```

Add to `TimelineWorkspace`:

```rust
    /// FFmpeg setup/download dialog state, if MP4 export needs FFmpeg.
    pub(crate) ffmpeg_setup: Option<FfmpegSetupDialog>,
```

Initialize it in `TimelineWorkspace::new`:

```rust
            ffmpeg_setup: None,
```

- [ ] **Step 2: Add failing update tests for setup flow**

In `crates/rollshot-app/src/timeline_workspace/update.rs`, extend `Message` with these variants:

```rust
    ExportMp4Requested,
    ExportMp4PathChosen(Option<PathBuf>),
    FfmpegUseSystem,
    FfmpegDownloadManaged,
    FfmpegDownloadFinished(Result<PathBuf, String>),
    FfmpegSetupCancel,
```

Append tests:

```rust
    #[test]
    fn ffmpeg_setup_cancel_closes_dialog() {
        let mut state = ws(recording_from_frames());
        state.ffmpeg_setup = Some(super::FfmpegSetupDialog {
            info: crate::managed_ffmpeg::FfmpegSetupInfo {
                managed_download: None,
                install_location: PathBuf::from("/tmp/ffmpeg"),
            },
            downloading: false,
        });
        let _ = update(&mut state, Message::FfmpegSetupCancel);
        assert!(state.ffmpeg_setup.is_none());
    }

    #[test]
    fn use_system_ffmpeg_sets_actionable_message() {
        let mut state = ws(recording_from_frames());
        state.ffmpeg_setup = Some(super::FfmpegSetupDialog {
            info: crate::managed_ffmpeg::FfmpegSetupInfo {
                managed_download: None,
                install_location: PathBuf::from("/tmp/ffmpeg"),
            },
            downloading: false,
        });
        let _ = update(&mut state, Message::FfmpegUseSystem);
        assert!(state.ffmpeg_setup.is_none());
        assert!(state.message.as_ref().unwrap().contains("ROLLSHOT_FFMPEG"));
    }
```

- [ ] **Step 3: Run update tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide ffmpeg_setup
```

Expected: FAIL until match arms are implemented.

- [ ] **Step 4: Implement setup match arms**

In `update`, add:

```rust
        Message::FfmpegSetupCancel => {
            state.ffmpeg_setup = None;
            Task::none()
        }
        Message::FfmpegUseSystem => {
            state.ffmpeg_setup = None;
            state.message =
                Some("Install FFmpeg or set ROLLSHOT_FFMPEG, then try Export MP4 again.".to_string());
            Task::none()
        }
```

Leave `ExportMp4Requested`, `ExportMp4PathChosen`, `FfmpegDownloadManaged`, and `FfmpegDownloadFinished` returning `Task::none()` for this task:

```rust
        Message::ExportMp4Requested
        | Message::ExportMp4PathChosen(_)
        | Message::FfmpegDownloadManaged
        | Message::FfmpegDownloadFinished(_) => Task::none(),
```

- [ ] **Step 5: Add FFmpeg setup modal view**

In `crates/rollshot-app/src/timeline_workspace/view.rs`, update the body composition:

```rust
    let body = if state.ffmpeg_setup.is_some() {
        ffmpeg_setup_modal(body, state)
    } else {
        body
    };
```

Place it after the Issue Pack modal wrapping and before discard modal wrapping, so discard remains top-most.

Add `button(text("Export MP4"))` beside `Export GIF`:

```rust
        button(text("Export MP4"))
            .on_press(Message::ExportMp4Requested)
            .style(button::secondary),
```

Add:

```rust
fn ffmpeg_setup_modal<'a>(
    base: Element<'a, Message>,
    state: &'a TimelineWorkspace,
) -> Element<'a, Message> {
    let dialog = state.ffmpeg_setup.as_ref().expect("checked by caller");
    let managed = dialog.info.managed_download.as_ref();
    let managed_enabled = managed.is_some() && !dialog.downloading;
    let details = if let Some(meta) = managed {
        column![
            text(format!("Source: {}", meta.source_url)).size(12).width(Length::Fill),
            text(format!("Version: {}", meta.version)).size(12).width(Length::Fill),
            text(format!("License: {} ({})", meta.license, meta.license_url))
                .size(12)
                .width(Length::Fill),
            text(format!("Archive size: {} bytes", meta.archive_size))
                .size(12)
                .width(Length::Fill),
            text(format!("SHA256: {}", meta.archive_sha256))
                .size(12)
                .width(Length::Fill),
            text(format!("Install location: {}", dialog.info.install_location.display()))
                .size(12)
                .width(Length::Fill),
        ]
    } else {
        column![text("Managed FFmpeg is not available for this platform.")]
    };

    let dialog_view = container(
        column![
            text("FFmpeg is required to export MP4").size(18),
            details.spacing(6),
            row![
                button(text("Use system FFmpeg / install manually"))
                    .on_press(Message::FfmpegUseSystem)
                    .style(button::secondary),
                button(text(if dialog.downloading {
                    "Downloading..."
                } else {
                    "Download managed FFmpeg"
                }))
                .on_press_maybe(managed_enabled.then_some(Message::FfmpegDownloadManaged))
                .style(button::primary),
                button(text("Cancel")).on_press(Message::FfmpegSetupCancel),
            ]
            .spacing(8),
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fixed(620.0))
    .style(container::rounded_box);

    let scrim = opaque(
        mouse_area(
            container(opaque(dialog_view))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .style(|_theme: &Theme| container::Style {
                    background: Some(
                        Color {
                            a: 0.8,
                            ..Color::BLACK
                        }
                        .into(),
                    ),
                    ..container::Style::default()
                }),
        )
        .interaction(mouse::Interaction::Idle)
        .on_press(Message::FfmpegSetupCancel),
    );
    stack![base, scrim].into()
}
```

- [ ] **Step 6: Run view and update tests**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "feat(app): add ffmpeg setup flow for mp4 export"
```

## Task 7: Timeline MP4 Export And Managed Download Wiring

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`

- [ ] **Step 1: Add imports and picker helper**

Change the rollshot-action import:

```rust
use rollshot_action::{export_gif, export_guide, export_video, GifOptions, VideoOptions};
```

Add:

```rust
async fn pick_mp4_save_path(default_dir: PathBuf) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_directory(default_dir)
        .set_file_name("summary.mp4")
        .add_filter("MP4 video", &["mp4"])
        .save_file()
        .await
        .map(|handle| handle.path().to_path_buf())
}

async fn download_managed_ffmpeg_task() -> Result<PathBuf, String> {
    tokio::task::spawn_blocking(crate::managed_ffmpeg::download_managed_ffmpeg)
        .await
        .map_err(|error| format!("managed FFmpeg download task failed: {error}"))?
}
```

- [ ] **Step 2: Add failing export tests**

Append tests:

```rust
    #[test]
    fn export_mp4_cancelled_picker_is_a_no_op() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::ExportMp4PathChosen(None));
        assert!(state.message.is_none());
    }

    #[test]
    fn export_mp4_missing_ffmpeg_opens_setup_and_writes_nothing() {
        let _guard = ENV_LOCK.lock().unwrap();
        let old_path = std::env::var_os("PATH");
        let old_ffmpeg = std::env::var_os("ROLLSHOT_FFMPEG");
        let old_root = std::env::var_os("ROLLSHOT_FFMPEG_ROOT");
        let root = tempfile::tempdir().unwrap();
        std::env::set_var("PATH", "");
        std::env::set_var("ROLLSHOT_FFMPEG", "/definitely/missing/ffmpeg");
        std::env::set_var("ROLLSHOT_FFMPEG_ROOT", root.path());
        let mut state = ws(recording_from_frames());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("summary.mp4");
        let _ = update(&mut state, Message::ExportMp4PathChosen(Some(path.clone())));
        assert!(!path.exists());
        assert!(state.ffmpeg_setup.is_some());
        assert!(state.message.is_none());
        match old_path {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
        match old_ffmpeg {
            Some(value) => std::env::set_var("ROLLSHOT_FFMPEG", value),
            None => std::env::remove_var("ROLLSHOT_FFMPEG"),
        }
        match old_root {
            Some(value) => std::env::set_var("ROLLSHOT_FFMPEG_ROOT", value),
            None => std::env::remove_var("ROLLSHOT_FFMPEG_ROOT"),
        }
    }
```

- [ ] **Step 3: Run tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide export_mp4
```

Expected: FAIL until message arms are implemented.

- [ ] **Step 4: Implement export and download match arms**

In `update`, replace the temporary MP4/download arm block with:

```rust
        Message::ExportMp4Requested => {
            state.message = None;
            match crate::managed_ffmpeg::resolve_ffmpeg() {
                crate::managed_ffmpeg::FfmpegResolution::Available(_) => Task::perform(
                    pick_mp4_save_path(picker_default_dir()),
                    Message::ExportMp4PathChosen,
                ),
                crate::managed_ffmpeg::FfmpegResolution::NeedsSetup(info) => {
                    state.ffmpeg_setup = Some(super::FfmpegSetupDialog {
                        info,
                        downloading: false,
                    });
                    Task::none()
                }
            }
        }
        Message::ExportMp4PathChosen(None) => Task::none(),
        Message::ExportMp4PathChosen(Some(path)) => {
            let ffmpeg = match crate::managed_ffmpeg::resolve_ffmpeg() {
                crate::managed_ffmpeg::FfmpegResolution::Available(path) => path,
                crate::managed_ffmpeg::FfmpegResolution::NeedsSetup(info) => {
                    state.ffmpeg_setup = Some(super::FfmpegSetupDialog {
                        info,
                        downloading: false,
                    });
                    return Task::none();
                }
            };
            match export_video(
                &state.guide,
                &state.store,
                VideoOptions::default(),
                &ffmpeg,
                &path,
            ) {
                Ok(()) => {
                    tracing::info!(
                        target: "rollshot::action::export",
                        path = %path.display(),
                        ffmpeg = %ffmpeg.display(),
                        "mp4 exported"
                    );
                    state.message = Some(format!("MP4 saved to {}", path.display()));
                }
                Err(error) => {
                    tracing::error!(
                        target: "rollshot::action::export",
                        %error,
                        path = %path.display(),
                        ffmpeg = %ffmpeg.display(),
                        "mp4 export failed"
                    );
                    state.message = Some(format!("MP4 export failed: {error}"));
                }
            }
            Task::none()
        }
        Message::FfmpegDownloadManaged => {
            if let Some(dialog) = &mut state.ffmpeg_setup {
                dialog.downloading = true;
            }
            Task::perform(download_managed_ffmpeg_task(), Message::FfmpegDownloadFinished)
        }
        Message::FfmpegDownloadFinished(Ok(path)) => {
            state.ffmpeg_setup = None;
            state.message = Some(format!("Managed FFmpeg installed at {}", path.display()));
            Task::perform(
                pick_mp4_save_path(picker_default_dir()),
                Message::ExportMp4PathChosen,
            )
        }
        Message::FfmpegDownloadFinished(Err(error)) => {
            if let Some(dialog) = &mut state.ffmpeg_setup {
                dialog.downloading = false;
            }
            state.message = Some(format!("Managed FFmpeg download failed: {error}"));
            Task::none()
        }
```

- [ ] **Step 5: Add setup-request test with controlled environment**

Append:

```rust
    #[test]
    fn export_mp4_requested_opens_setup_when_ffmpeg_missing() {
        let _guard = ENV_LOCK.lock().unwrap();
        let old_path = std::env::var_os("PATH");
        let old_ffmpeg = std::env::var_os("ROLLSHOT_FFMPEG");
        let old_root = std::env::var_os("ROLLSHOT_FFMPEG_ROOT");
        let root = tempfile::tempdir().unwrap();
        std::env::set_var("PATH", "");
        std::env::set_var("ROLLSHOT_FFMPEG", "/definitely/missing/ffmpeg");
        std::env::set_var("ROLLSHOT_FFMPEG_ROOT", root.path());
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::ExportMp4Requested);
        assert!(state.ffmpeg_setup.is_some());
        match old_path {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
        match old_ffmpeg {
            Some(value) => std::env::set_var("ROLLSHOT_FFMPEG", value),
            None => std::env::remove_var("ROLLSHOT_FFMPEG"),
        }
        match old_root {
            Some(value) => std::env::set_var("ROLLSHOT_FFMPEG_ROOT", value),
            None => std::env::remove_var("ROLLSHOT_FFMPEG_ROOT"),
        }
    }

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
```

If an existing test module already defines static items at the bottom, place `ENV_LOCK` near the top of the test module instead.

- [ ] **Step 6: Run MP4 app tests**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide export_mp4
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "feat(app): wire timeline mp4 export"
```

## Task 8: Final Verification

**Files:**
- Verify only.

- [ ] **Step 1: Format**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 2: Run focused crate tests**

Run:

```bash
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-app --features action-guide timeline_workspace
rtk cargo test -p rollshot-app --features action-guide managed_ffmpeg
```

Expected: all PASS.

- [ ] **Step 3: Run workspace tests**

Run:

```bash
rtk cargo test
```

Expected: PASS.

- [ ] **Step 4: Run clippy**

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS. If dependency compilation makes this too slow for the current session, record the exact point reached and the remaining risk in the final handoff.

- [ ] **Step 5: Optional real FFmpeg encode check**

Run only when FFmpeg is available:

```bash
ROLLSHOT_FFMPEG="$(command -v ffmpeg)" ROLLSHOT_TEST_FFMPEG=1 rtk cargo test -p rollshot-action real_ffmpeg_export_is_opt_in
```

Expected: PASS and a non-empty MP4 is produced during the test.

- [ ] **Step 6: Commit verification-only fixes if any**

If formatting or clippy required code changes, commit them:

```bash
rtk git add <changed-files>
rtk git commit -m "fix(action-guide): polish mp4 export implementation"
```

If no files changed, do not create a commit.

## Plan Review Auto Decisions

- Auto decision D1: Drain FFmpeg stderr concurrently in `export_video` - otherwise a verbose FFmpeg process can fill stderr, block, and make raw-frame stdin writes hang.
- Auto decision D2: Add `ROLLSHOT_FFMPEG_ROOT` for managed-FFmpeg tests/dev - this keeps app tests deterministic and prevents writes into a user's real config directory.
- Auto decision D3: Add an explicit `verify_archive_sha` mismatch test - checksum mismatch is the main managed-download safety check and must fail loudly.
- Auto decision D4: Correct the missing-FFmpeg app test expectation - the intended behavior is opening setup UI, not setting an export-failed message.
- Auto decision D5: Assert the exact MP4 temp path via `temp_mp4_path` - `summary.mp4.tmp.mp4` is deliberate, so the atomic-write test must check that path.
- Auto decision D6: Add the required plan-review outputs - scope, existing code reuse, failure modes, test coverage, and parallelization are now explicit.
- Auto decision D7: Make long FFmpeg setup details wrap/fill the modal - source URLs, licenses, SHA256, and install paths are long enough to stress the fixed-width dialog.

## NOT in scope

- Full-fps screen recording - this plan exports reviewed keyframes as a summary video, not a recorder.
- Audio capture - no microphone/system audio permissions, encoding, or timeline sync are introduced.
- Burned-in captions, cursor animation, click rings, or zoom effects - these require text/layout/rendering work outside the first MP4 slice.
- Issue Pack `summary.mp4` attachment - MP4 export is built first; Issue Pack attachment can consume it later.
- Hardware encoder selection - first version uses FFmpeg/libx264 defaults for predictable behavior.
- Automatic FFmpeg download at startup or button render time - download happens only after explicit user action.
- Cross-platform managed binaries beyond pinned Linux x86_64 metadata - unsupported platforms show the manual-install path until metadata is added.
- App updater/plugin-manager style binary lifecycle - managed FFmpeg is a narrow, manifest-backed tool install only.

## What already exists

- `crates/rollshot-action/src/gif.rs` already exports reviewed keyframes as an animation artifact; this plan mirrors its guide/store/keyframe flow instead of building a recording pipeline.
- `Guide`, `CandidateStep`, `FrameStore`, and retained keyframes already model the reviewed workflow; `export_video` consumes those directly.
- `crates/rollshot-app/src/timeline_workspace/update.rs` already owns export requests and save-picker flows; MP4 export extends that message/update pattern.
- `crates/rollshot-app/src/timeline_workspace/view.rs` already has header export controls and modal layering; the FFmpeg setup dialog reuses the existing iced composition style.
- `crate::daemon::config::rollshot_config_dir()` already provides a Rollshot config root; managed FFmpeg stores under it, with a test/dev override.
- Existing tracing targets under `rollshot::action::*` are reused for export and managed-download diagnostics.

## Test Coverage Matrix

| Task / behavior | Unit | Integration / fake process | Real dependency / smoke | Manual only |
|---|---|---|---|---|
| Task 1: `VideoError` API and messages | `video_error_messages_are_actionable` | - | - | - |
| Task 2: repeat count, even dimensions, downscale | `frame_repeat_count_uses_ceil`, `even_dimensions_pad_odd_size`, `resize_preserves_aspect_ratio` | - | - | - |
| Task 3: command args and exporter errors | `ffmpeg_args_describe_raw_rgba_to_h264_mp4`, `empty_guide_is_an_error`, `missing_keyframe_errors_and_leaves_no_file` | `export_invokes_ffmpeg_and_writes_atomically` with fake FFmpeg | opt-in `ROLLSHOT_TEST_FFMPEG=1` real encode | - |
| Task 4: metadata, manifest, path resolution | metadata/manifest round-trip, stable managed paths, missing path validation, root override | `validate_ffmpeg` executes missing/fake paths as needed | - | - |
| Task 5: managed download helpers | `sha256_file_detects_content`, `verify_archive_sha_rejects_mismatch`, `write_manifest_persists_valid_json` | - | download intentionally not run in CI | User consent/download flow only |
| Task 6: setup modal state | cancel/use-system update tests | view compiles with modal/button states | - | Visual polish of modal layout |
| Task 7: export wiring | cancelled picker, missing FFmpeg opens setup, download success/failure state | fake/system path resolution through controlled env | optional real FFmpeg path via env | Save dialog interaction |
| Task 8: verification | crate tests | workspace tests | optional real FFmpeg encode | final UI spot-check if environment permits |

## Failure Modes

| Codepath | Production failure | Covered by | Error handling | User-visible result |
|---|---|---|---|---|
| `export_video` guide input | Empty guide or missing retained keyframe | Task 3 tests | `VideoError::Empty`, `VideoError::KeyframeMissing` | Inline export failure if reached from app |
| FFmpeg process startup | Invalid binary path or spawn permission denied | Task 3 invalid path / app resolution tests | `VideoError::InvalidFfmpeg`, `VideoError::Spawn` | Setup dialog or MP4 export failed message |
| Raw frame pipe | Broken stdin while writing frames | Task 3 fake process coverage plus `VideoError::Stdin` path | `VideoError::Stdin` | MP4 export failed message |
| FFmpeg stderr pipe | Verbose process blocks because stderr is not drained | Auto decision D1 / Task 3 implementation | concurrent stderr drain thread | Avoids hang; exit stderr retained for failure message |
| FFmpeg exit failure | Encoder exits non-zero | Task 3 command/export tests can add fake non-zero if needed | `VideoError::Exit { status, stderr }` | MP4 export failed with stderr context |
| Atomic output | Temp file remains after failed encode | Task 3 atomic-write test | remove temp on failure, rename on success | No partial final MP4 |
| System FFmpeg resolution | Host has no FFmpeg or invalid `ROLLSHOT_FFMPEG` | Task 4/7 env-isolated tests | `FfmpegResolution::NeedsSetup` | Setup dialog with manual/download options |
| Managed manifest | Stale/corrupt manifest points to missing binary | Task 4 manifest/validation tests | ignore invalid manifest and return setup | Setup dialog, no silent failure |
| Managed download | Network/download/unpack failure | Task 7 download-failure state test | `download_managed_ffmpeg` returns `Err(String)` | Inline managed-download failed message |
| Managed checksum | Archive SHA256 mismatch | Task 5 mismatch test | `verify_archive_sha` error and archive removal | Managed-download failed message |
| Unsupported platform | No pinned managed metadata | Task 4 metadata option / modal disabled state | `managed_download: None` | Manual install option remains available |
| User cancellation | Save picker cancelled | Task 7 no-op test | `ExportMp4PathChosen(None)` no-op | Timeline stays open, no error |

No critical gaps: every new codepath has either a focused test, explicit error handling, or a deliberate manual-only surface called out above.

## Parallelization Strategy

| Task | Modules touched | Depends on |
|---|---|---|
| Task 1: Dependencies and `VideoError` | root `Cargo.toml`, `crates/rollshot-action/`, `crates/rollshot-app/` | - |
| Task 2: Pure MP4 frame preparation | `crates/rollshot-action/` | Task 1 |
| Task 3: MP4 exporter and command assembly | `crates/rollshot-action/` | Task 2 |
| Task 4: Managed FFmpeg metadata and resolution | `crates/rollshot-app/` | Task 1 |
| Task 5: Managed FFmpeg download and manifest write | `crates/rollshot-app/` | Task 4 |
| Task 6: Timeline MP4 messages and setup state | `crates/rollshot-app/` | Task 4 |
| Task 7: Timeline MP4 export and managed download wiring | `crates/rollshot-app/`, `crates/rollshot-action/` | Tasks 3, 5, 6 |
| Task 8: Final verification | workspace | Tasks 1-7 |

Parallel lanes after Task 1:

- Lane A: Task 2 -> Task 3, sequential because both touch `crates/rollshot-action/`.
- Lane B: Task 4 -> Task 5, sequential because both touch `crates/rollshot-app/src/managed_ffmpeg.rs`.
- Lane C: Task 6 after Task 4, mostly `timeline_workspace`; can run beside Task 5 with coordination because both are under `crates/rollshot-app/`.
- Lane D: Task 7 after Lanes A, B, and C merge.
- Lane E: Task 8 after all implementation tasks.

Execution order: run Task 1 first because root dependency changes serialize `Cargo.lock`; then Lane A and Lane B can start in parallel. Start Lane C after Task 4 lands. Run Task 7 only after action exporter, managed download, and setup state exist. Run Task 8 last.

Conflict flags: Lanes B and C both touch `crates/rollshot-app/`; assign them sequentially if avoiding merge conflicts is more important than parallel speed.

## Plan Review Completion Summary

Plan reviewed:           `docs/superpowers/plans/2026-07-05-action-guide-mp4-summary.md`
Tasks in plan:           8
Files Create/Modify:     2 create / 9 modify

- Step 0: Scope Challenge   - accepted as-is; no complexity reduction required.
- Architecture Review:      3 issues resolved by D1, D2, and D7.
- Plan Structure + Code Q:  2 issues resolved by D5 and D6.
- Test Review:              table produced, 2 gaps resolved by D3 and D4.
- Performance Review:       1 issue resolved by D1; no stitching benchmark needed.
- NOT in scope:             written.
- What already exists:      written.
- Failure modes:            0 critical gaps flagged.
- Parallelization:          5 lanes, 2 practical parallel lanes after Task 1.
- Unresolved decisions:     0.

Plan is locked in - run `superpowers:executing-plans` or `superpowers:subagent-driven-development` next.

## Self-Review Notes

- Spec coverage: MP4 summary export is covered by Tasks 1-3 and 7; system/managed FFmpeg resolution by Tasks 4-5; user consent UI by Task 6; no silent startup download by Task 7; opt-in real FFmpeg tests by Task 8.
- Type consistency: `VideoOptions`, `VideoError`, `export_video`, `FfmpegResolution`, `FfmpegSetupDialog`, and message names are defined before use.
- Scope check: Issue Pack MP4 attachment, captions, audio, full recording, updater, and plugin manager are not implemented by this plan.
