//! Summary-MP4 export: assemble the final guide's reviewed keyframes into a
//! short H.264 MP4. This is a workflow summary, not raw screen recording.

use std::io::{Read, Write};
use std::path::Path;

use ffmpeg_sidecar::command::FfmpegCommand;
use image::{imageops, RgbaImage};

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
        stdin
            .flush()
            .map_err(|source| VideoError::Stdin { source })?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

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
}
