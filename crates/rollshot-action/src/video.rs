//! Summary-MP4 export: assemble the final guide's reviewed keyframes into a
//! short H.264 MP4. This is a workflow summary, not raw screen recording.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use ffmpeg_sidecar::command::FfmpegCommand;
use image::{imageops, RgbaImage};

use crate::error::VideoError;
use crate::export::model::ReviewedGuideExportJob;
use crate::frame_store::FrameStore;
use crate::guide::Guide;
use crate::project::PublishCancellation;

use super::gif::DERIVATIVE_FRAME_PIXEL_CEILING;

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
    let mut stderr_handle = child.take_stderr().map(|mut stderr| {
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
                if let Err(source) = stdin.write_all(frame.as_raw()) {
                    drop(stdin);
                    let _ = child.kill();
                    let _ = child.wait();
                    if let Some(handle) = stderr_handle.take() {
                        let _ = handle.join();
                    }
                    let _ = std::fs::remove_file(&tmp);
                    return Err(VideoError::Stdin { source });
                }
            }
        }
        if let Err(source) = stdin.flush() {
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
            if let Some(handle) = stderr_handle.take() {
                let _ = handle.join();
            }
            let _ = std::fs::remove_file(&tmp);
            return Err(VideoError::Stdin { source });
        }
    }

    let status = child.wait().map_err(|source| VideoError::Io {
        path: tmp.display().to_string(),
        source,
    })?;
    let stderr_text = stderr_handle
        .take()
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

fn kill_process(pid: u32) {
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status();
    }
    #[cfg(not(unix))]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .status();
    }
}

fn descriptor_output_dimensions(
    steps: &[crate::export::model::ReviewedGuideStep],
    max_width: u32,
) -> (u32, u32) {
    let mut out_w: u32 = 2;
    let mut out_h: u32 = 2;
    for step in steps {
        let (w, h) = step.image.dimensions();
        let scaled_w = w.min(max_width);
        let scaled_h = if w > max_width && w > 0 {
            (h as u64 * max_width as u64 / w as u64).max(1) as u32
        } else {
            h
        };
        out_w = out_w.max(scaled_w);
        out_h = out_h.max(scaled_h);
    }
    (even_dimension(out_w), even_dimension(out_h))
}

pub fn export_reviewed_video(
    job: &ReviewedGuideExportJob,
    opts: VideoOptions,
    ffmpeg_path: &Path,
    cancel: &PublishCancellation,
    out_path: &Path,
) -> Result<(), VideoError> {
    if job.steps.is_empty() {
        return Err(VideoError::Empty);
    }
    if !ffmpeg_path.exists() {
        return Err(VideoError::InvalidFfmpeg {
            path: ffmpeg_path.display().to_string(),
        });
    }

    let (width, height) = descriptor_output_dimensions(&job.steps, opts.max_width);
    let frame_pixels = (width as u64).checked_mul(height as u64);
    match frame_pixels {
        Some(p) if p <= DERIVATIVE_FRAME_PIXEL_CEILING => {}
        _ => {
            return Err(VideoError::FrameTooLarge {
                pixels: frame_pixels.unwrap_or(u64::MAX),
                ceiling: DERIVATIVE_FRAME_PIXEL_CEILING,
            });
        }
    }

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

    let mut stderr_handle = child.take_stderr().map(|mut stderr| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        })
    });

    let child_pid = child.as_inner().id();
    let done = std::sync::Arc::new(AtomicBool::new(false));
    let done_watchdog = done.clone();
    let cancel_watchdog = cancel.clone();
    let watchdog_handle = std::thread::spawn(move || {
        while !cancel_watchdog.is_cancelled() && !done_watchdog.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if cancel_watchdog.is_cancelled() {
            kill_process(child_pid);
        }
    });

    let result = {
        let mut stdin = child.take_stdin().ok_or_else(|| VideoError::Stdin {
            source: std::io::Error::new(std::io::ErrorKind::BrokenPipe, "missing FFmpeg stdin"),
        })?;

        let mut write_result = Ok(());
        for step in &job.steps {
            if cancel.is_cancelled() {
                write_result = Err(VideoError::Cancelled);
                break;
            }

            let step_result = step
                .image
                .with_flattened_image(cancel.flag(), |image| {
                    let frame = normalize_to(&downscale(image, opts.max_width), width, height);
                    for _ in 0..repeat {
                        if cancel.is_cancelled() {
                            return Err(crate::error::ExportError::Cancelled);
                        }
                        if let Err(source) = stdin.write_all(frame.as_raw()) {
                            return Err(crate::error::ExportError::Io {
                                path: String::new(),
                                source,
                            });
                        }
                    }
                    Ok(())
                })
                .map_err(|error| match error {
                    crate::error::ExportError::Cancelled => VideoError::Cancelled,
                    crate::error::ExportError::Io { source, .. } => VideoError::Stdin { source },
                    other => VideoError::Io {
                        path: String::new(),
                        source: std::io::Error::other(other.to_string()),
                    },
                });

            if let Err(e) = step_result {
                write_result = Err(e);
                break;
            }
        }

        if write_result.is_ok() {
            if cancel.is_cancelled() {
                write_result = Err(VideoError::Cancelled);
            } else if let Err(source) = stdin.flush() {
                write_result = Err(VideoError::Stdin { source });
            }
        }
        write_result
    };

    drop(child.take_stdin());

    if result.is_err() {
        let _ = child.kill();
    }
    let status_result = child.wait();
    done.store(true, Ordering::Relaxed);
    let _ = watchdog_handle.join();
    let stderr_text = stderr_handle
        .take()
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();

    let status = status_result.map_err(|source| VideoError::Io {
        path: tmp.display().to_string(),
        source,
    })?;

    match result {
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
        Ok(()) => {
            if cancel.is_cancelled() {
                let _ = std::fs::remove_file(&tmp);
                return Err(VideoError::Cancelled);
            }
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    use crate::detector::DetectorConfig;
    use crate::frame_store::SharedActionFrame;
    use crate::frame_store::StoreConfig;
    use crate::models::{CandidateKind, CandidateStep, CaptureRegion, DetectReason};
    use crate::recorder::{ActionRecorder, Recording};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    fn region() -> CaptureRegion {
        CaptureRegion {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        }
    }

    fn black() -> SharedActionFrame {
        Arc::new(RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255])))
    }

    fn white() -> SharedActionFrame {
        Arc::new(RgbaImage::from_pixel(8, 8, Rgba([255, 255, 255, 255])))
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

    #[cfg(unix)]
    fn early_exit_ffmpeg(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(
            path,
            "#!/bin/sh\nout=\"\"\nfor arg in \"$@\"; do out=\"$arg\"; done\nprintf 'partial mp4' > \"$out\"\nexit 1\n",
        )
        .unwrap();
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }

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
            Path::new("/bin/sh"),
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
    #[test]
    fn export_cleans_temp_file_when_ffmpeg_closes_stdin_early() {
        let mut store = FrameStore::new(StoreConfig::default());
        let frame = Arc::new(RgbaImage::from_pixel(2048, 2048, Rgba([12, 34, 56, 255])));
        let id = store.ingest(frame, 0);
        store.retain_window(id);
        let guide = one_step_guide(id);
        let dir = tempfile_dir();
        let ffmpeg = dir.join("ffmpeg");
        early_exit_ffmpeg(&ffmpeg);
        let path = dir.join("summary.mp4");

        let result = export_video(&guide, &store, VideoOptions::default(), &ffmpeg, &path);

        assert!(result.is_err());
        assert!(!path.exists());
        assert!(!temp_mp4_path(&path).exists());
    }

    #[cfg(unix)]
    fn tempfile_dir() -> PathBuf {
        let dir = temp_path("dir", "tmp");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

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

    #[cfg(unix)]
    #[test]
    fn reviewed_streaming_cancels_and_leaves_no_file() {
        let cancel = PublishCancellation::new();
        cancel.cancel();
        let image = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]));
        let job = crate::export::model::ReviewedGuideExportJob {
            title: "Test".into(),
            region: crate::models::CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: crate::models::InputSourceKind::VisualOnly,
            input_capability: crate::models::InputCapability::VisualOnly {
                reason: crate::models::DegradedReason::SourceStartFailed,
            },
            steps: vec![
                crate::export::model::ReviewedGuideStep {
                    index: 1,
                    title: "Step 1".into(),
                    caption: None,
                    kind: crate::models::CandidateKind::Click,
                    reason: crate::models::DetectReason::VisualChange,
                    at_ms: 100,
                    image: crate::export::model::ReviewedStepImage::Retained(std::sync::Arc::new(
                        image.clone(),
                    )),
                    hotspots: Vec::new(),
                },
                crate::export::model::ReviewedGuideStep {
                    index: 2,
                    title: "Step 2".into(),
                    caption: None,
                    kind: crate::models::CandidateKind::Click,
                    reason: crate::models::DetectReason::VisualChange,
                    at_ms: 200,
                    image: crate::export::model::ReviewedStepImage::Retained(std::sync::Arc::new(
                        image,
                    )),
                    hotspots: Vec::new(),
                },
            ],
            import_warnings: Vec::new(),
        };
        let dir = tempfile_dir();
        let ffmpeg = dir.join("ffmpeg");
        fake_ffmpeg(&ffmpeg);
        let path = dir.join("summary.mp4");
        let result = export_reviewed_video(&job, VideoOptions::default(), &ffmpeg, &cancel, &path);
        assert!(matches!(result, Err(VideoError::Cancelled)));
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn reviewed_streaming_encodes_all_frames() {
        let cancel = PublishCancellation::new();
        let image = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]));
        let job = crate::export::model::ReviewedGuideExportJob {
            title: "Test".into(),
            region: crate::models::CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: crate::models::InputSourceKind::VisualOnly,
            input_capability: crate::models::InputCapability::VisualOnly {
                reason: crate::models::DegradedReason::SourceStartFailed,
            },
            steps: vec![
                crate::export::model::ReviewedGuideStep {
                    index: 1,
                    title: "Step 1".into(),
                    caption: None,
                    kind: crate::models::CandidateKind::Click,
                    reason: crate::models::DetectReason::VisualChange,
                    at_ms: 100,
                    image: crate::export::model::ReviewedStepImage::Retained(std::sync::Arc::new(
                        image.clone(),
                    )),
                    hotspots: Vec::new(),
                },
                crate::export::model::ReviewedGuideStep {
                    index: 2,
                    title: "Step 2".into(),
                    caption: None,
                    kind: crate::models::CandidateKind::Click,
                    reason: crate::models::DetectReason::VisualChange,
                    at_ms: 200,
                    image: crate::export::model::ReviewedStepImage::Retained(std::sync::Arc::new(
                        image,
                    )),
                    hotspots: Vec::new(),
                },
            ],
            import_warnings: Vec::new(),
        };
        let dir = tempfile_dir();
        let ffmpeg = dir.join("ffmpeg");
        fake_ffmpeg(&ffmpeg);
        let path = dir.join("summary.mp4");
        export_reviewed_video(&job, VideoOptions::default(), &ffmpeg, &cancel, &path)
            .expect("export");
        assert_eq!(std::fs::read(&path).unwrap(), b"fake mp4");
        assert!(!temp_mp4_path(&path).exists());
    }

    #[test]
    fn reviewed_streaming_rejects_oversized_geometry() {
        let cancel = PublishCancellation::new();
        // 100 * 200_000 = 20_000_000 > DERIVATIVE_FRAME_PIXEL_CEILING (16_777_216)
        // Image is already at max_width, so downscaling won't reduce it.
        let big = RgbaImage::from_pixel(100, 200_000, Rgba([0, 0, 0, 255]));
        let job = crate::export::model::ReviewedGuideExportJob {
            title: "Test".into(),
            region: crate::models::CaptureRegion {
                x: 0,
                y: 0,
                width: 100,
                height: 200_000,
            },
            input_source: crate::models::InputSourceKind::VisualOnly,
            input_capability: crate::models::InputCapability::VisualOnly {
                reason: crate::models::DegradedReason::SourceStartFailed,
            },
            steps: vec![crate::export::model::ReviewedGuideStep {
                index: 1,
                title: "Step 1".into(),
                caption: None,
                kind: crate::models::CandidateKind::Click,
                reason: crate::models::DetectReason::VisualChange,
                at_ms: 100,
                image: crate::export::model::ReviewedStepImage::Retained(std::sync::Arc::new(big)),
                hotspots: Vec::new(),
            }],
            import_warnings: Vec::new(),
        };
        let dir = tempfile_dir();
        let ffmpeg = dir.join("ffmpeg");
        fake_ffmpeg(&ffmpeg);
        let path = dir.join("summary.mp4");
        let result = export_reviewed_video(
            &job,
            VideoOptions {
                max_width: 100,
                ..VideoOptions::default()
            },
            &ffmpeg,
            &cancel,
            &path,
        );
        assert!(matches!(result, Err(VideoError::FrameTooLarge { .. })));
        assert!(!path.exists());
    }

    // NOTE: The watchdog-child-kill behavior is tested on unix via shell-script
    // fakes. Non-unix uses the same cross-platform `child.kill()` path but lacks
    // test coverage because the fake-ffmpeg infrastructure requires shell scripts.
    // Verify on Windows/macOS CI when those platforms are added.
    #[cfg(unix)]
    #[test]
    fn reviewed_streaming_cancels_and_terminates_child() {
        use std::os::unix::fs::PermissionsExt;

        let cancel = PublishCancellation::new();
        let image = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]));
        let job = crate::export::model::ReviewedGuideExportJob {
            title: "Test".into(),
            region: crate::models::CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: crate::models::InputSourceKind::VisualOnly,
            input_capability: crate::models::InputCapability::VisualOnly {
                reason: crate::models::DegradedReason::SourceStartFailed,
            },
            steps: vec![
                crate::export::model::ReviewedGuideStep {
                    index: 1,
                    title: "Step 1".into(),
                    caption: None,
                    kind: crate::models::CandidateKind::Click,
                    reason: crate::models::DetectReason::VisualChange,
                    at_ms: 100,
                    image: crate::export::model::ReviewedStepImage::Retained(std::sync::Arc::new(
                        image.clone(),
                    )),
                    hotspots: Vec::new(),
                },
                crate::export::model::ReviewedGuideStep {
                    index: 2,
                    title: "Step 2".into(),
                    caption: None,
                    kind: crate::models::CandidateKind::Click,
                    reason: crate::models::DetectReason::VisualChange,
                    at_ms: 200,
                    image: crate::export::model::ReviewedStepImage::Retained(std::sync::Arc::new(
                        image,
                    )),
                    hotspots: Vec::new(),
                },
            ],
            import_warnings: Vec::new(),
        };

        let dir = tempfile_dir();
        let ffmpeg = dir.join("ffmpeg");
        std::fs::write(
            &ffmpeg,
            "#!/bin/sh\nout=\"\"\nfor arg in \"$@\"; do out=\"$arg\"; done\ncat >/dev/null\nsleep 10\nprintf 'slow mp4' > \"$out\"\n",
        )
        .unwrap();
        let mut perms = std::fs::metadata(&ffmpeg).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&ffmpeg, perms).unwrap();

        let path = dir.join("summary.mp4");
        cancel.cancel();
        let result = export_reviewed_video(&job, VideoOptions::default(), &ffmpeg, &cancel, &path);
        assert!(matches!(result, Err(VideoError::Cancelled)));
        assert!(!path.exists());
        assert!(!temp_mp4_path(&path).exists());
    }

    #[cfg(unix)]
    #[test]
    fn reviewed_streaming_cancels_during_ffmpeg_finalization() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile_dir();
        let marker = dir.join("stdin-closed");
        let ffmpeg = dir.join("ffmpeg-finalizing");
        std::fs::write(
            &ffmpeg,
            format!(
                "#!/bin/sh\nout=\"\"\nfor arg in \"$@\"; do out=\"$arg\"; done\ncat >/dev/null\ntouch '{}'\nsleep 2\nprintf 'late mp4' > \"$out\"\n",
                marker.display()
            ),
        )
        .unwrap();
        let mut perms = std::fs::metadata(&ffmpeg).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&ffmpeg, perms).unwrap();

        let cancel = PublishCancellation::new();
        let worker_cancel = cancel.clone();
        let path = dir.join("finalizing.mp4");
        let worker_path = path.clone();
        let job = crate::export::model::ReviewedGuideExportJob {
            title: "Test".into(),
            region: crate::models::CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: crate::models::InputSourceKind::VisualOnly,
            input_capability: crate::models::InputCapability::SemanticEvents,
            steps: vec![crate::export::model::ReviewedGuideStep {
                index: 1,
                title: "Step".into(),
                caption: None,
                kind: crate::models::CandidateKind::Click,
                reason: crate::models::DetectReason::VisualChange,
                at_ms: 100,
                image: crate::export::model::ReviewedStepImage::Retained(std::sync::Arc::new(
                    RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255])),
                )),
                hotspots: Vec::new(),
            }],
            import_warnings: Vec::new(),
        };
        let worker = std::thread::spawn(move || {
            export_reviewed_video(
                &job,
                VideoOptions::default(),
                &ffmpeg,
                &worker_cancel,
                &worker_path,
            )
        });

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !marker.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(marker.exists(), "fake ffmpeg never entered finalization");
        cancel.cancel();

        let result = worker.join().unwrap();
        assert!(matches!(result, Err(VideoError::Cancelled)));
        assert!(!path.exists());
        assert!(!temp_mp4_path(&path).exists());
    }
}
