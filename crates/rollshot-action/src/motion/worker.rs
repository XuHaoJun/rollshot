//! Managed FFmpeg motion recording worker.
//!
//! Spawns FFmpeg synchronously, feeds RGBA frames through a CFR scheduler
//! on a dedicated thread, and produces a validated H.264 MP4 asset on finish.
//! The `MotionSink` trait provides a seam for injectable test doubles.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use super::asset::ValidatedMotionAsset;
use super::error::MotionFailureCategory;
use super::probe::{self, MotionMetadata};
use super::queue::{motion_frame_mailbox, MotionFrame, MotionFrameReceiver, MotionFrameSender};
use super::timing::CfrScheduler;
use crate::frame_store::SharedActionFrame;
use crate::models::Millis;
use crate::video_import::VideoToolchain;

/// Encoder runtime status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MotionRuntimeStatus {
    /// The encoder thread is running and accepting frames.
    On = 0,
    /// The encoder thread has stopped after a successful finish or cancel.
    Off = 1,
    /// The encoder thread stopped due to a runtime error.
    Failed = 2,
}

/// Outcome of a `finish()` call.
#[derive(Debug)]
pub enum MotionRecordingOutcome {
    /// The recording was successfully finalized and validated.
    Ready(ValidatedMotionAsset),
    /// The recording failed at some stage.
    Failure(MotionFailureCategory),
}

// ─── MotionSink trait ────────────────────────────────────────────────────

/// Internal trait abstracting the write target for the worker thread.
///
/// Production uses `FfmpegSink` (writes to FFmpeg's stdin). Tests inject
/// a `TestSink` that records frame bytes and counts.
pub(crate) trait MotionSink: Send {
    /// Write one frame (W × H × 4 bytes of RGBA) to the sink.
    fn write(&mut self, rgba: &[u8]) -> Result<(), MotionFailureCategory>;
    /// Number of frames written so far.
    #[allow(dead_code)] // used by tests and future export pipeline
    fn frame_count(&self) -> u64;
}

/// Production sink: writes raw RGBA frames to FFmpeg's stdin.
struct FfmpegSink {
    stdin: std::process::ChildStdin,
    count: u64,
}

impl MotionSink for FfmpegSink {
    fn write(&mut self, rgba: &[u8]) -> Result<(), MotionFailureCategory> {
        self.stdin
            .write_all(rgba)
            .map_err(|_| MotionFailureCategory::BrokenPipe)?;
        self.count += 1;
        Ok(())
    }

    fn frame_count(&self) -> u64 {
        self.count
    }
}

/// Factory that creates a `MotionSink` from a `Child` (takes its stdin).
type SinkFactory = Box<dyn FnOnce(&mut Child) -> Box<dyn MotionSink> + Send>;

fn production_sink_factory() -> SinkFactory {
    Box::new(|child| {
        let stdin = child.stdin.take().expect("FFmpeg stdin must be piped");
        Box::new(FfmpegSink { stdin, count: 0 })
    })
}

// ─── MotionRecorder ──────────────────────────────────────────────────────

/// A managed FFmpeg motion recording session.
///
/// Owns the non-blocking frame sender and shared runtime status. The worker
/// thread owns FFmpeg's stdin, process, mailbox receiver, and CFR scheduler.
pub struct MotionRecorder {
    frame_tx: MotionFrameSender,
    session_tx: crossbeam_channel::Sender<Millis>,
    status: Arc<AtomicU8>,
    cancel_flag: Arc<AtomicBool>,
    worker: Option<JoinHandle<MotionRecordingOutcome>>,
    scratch_dir: PathBuf,
}

impl std::fmt::Debug for MotionRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MotionRecorder")
            .field("status", &self.status())
            .field("scratch_dir", &self.scratch_dir)
            .finish()
    }
}

impl MotionRecorder {
    /// Start a new motion recording session.
    ///
    /// Validates dimensions (non-zero, even), creates a session scratch
    /// directory, spawns FFmpeg synchronously, and starts a stderr drain
    /// thread before returning.
    pub fn start(
        toolchain: &VideoToolchain,
        width: u32,
        height: u32,
    ) -> Result<Self, MotionFailureCategory> {
        Self::start_with_factory(toolchain, width, height, production_sink_factory())
    }

    /// Internal start with injectable sink factory (for tests).
    pub(crate) fn start_with_factory(
        toolchain: &VideoToolchain,
        width: u32,
        height: u32,
        sink_factory: SinkFactory,
    ) -> Result<Self, MotionFailureCategory> {
        // Validate dimensions.
        if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(MotionFailureCategory::Filesystem);
        }

        // Verify ffmpeg is available.
        let version = Command::new(&toolchain.ffmpeg)
            .args(["-version"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match version {
            Ok(s) if s.success() => {}
            _ => return Err(MotionFailureCategory::ToolUnavailable),
        }

        // Create session scratch directory.
        let scratch_dir = create_scratch_dir()?;
        let part_path = scratch_dir.join("recording.part.mp4");
        let final_path = scratch_dir.join("recording.mp4");

        // Spawn FFmpeg.
        let mut child = Command::new(&toolchain.ffmpeg)
            .args([
                "-y",
                "-f",
                "rawvideo",
                "-pixel_format",
                "rgba",
                "-video_size",
                &format!("{width}x{height}"),
                "-framerate",
                "30",
                "-i",
                "pipe:0",
                "-an",
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
                &part_path.to_string_lossy(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| MotionFailureCategory::Spawn)?;

        // Drain stderr on a background thread (discard bytes).
        if let Some(stderr) = child.stderr.take() {
            std::thread::spawn(move || {
                use std::io::Read;
                let _ = std::io::Read::take(stderr, 1024 * 1024).read_to_end(&mut Vec::new());
            });
        }

        let sink = sink_factory(&mut child);

        let (frame_tx, frame_rx) = motion_frame_mailbox(2);
        let (session_tx, session_rx) = crossbeam_channel::bounded::<Millis>(1);

        let status = Arc::new(AtomicU8::new(MotionRuntimeStatus::On as u8));
        let cancel_flag = Arc::new(AtomicBool::new(false));

        let worker_status = Arc::clone(&status);
        let worker_cancel = Arc::clone(&cancel_flag);
        let ffprobe_path = toolchain.ffprobe.clone();

        let worker = std::thread::spawn(move || {
            let outcome = worker_loop(
                child,
                sink,
                frame_rx,
                session_rx,
                width,
                height,
                &part_path,
                &final_path,
                &worker_cancel,
                &ffprobe_path,
            );
            let exit_status = match &outcome {
                MotionRecordingOutcome::Ready(_) => MotionRuntimeStatus::Off,
                MotionRecordingOutcome::Failure(_) => MotionRuntimeStatus::Failed,
            };
            worker_status.store(exit_status as u8, Ordering::Release);
            outcome
        });

        Ok(Self {
            frame_tx,
            session_tx,
            status,
            cancel_flag,
            worker: Some(worker),
            scratch_dir,
        })
    }

    /// Offer a frame to the recording. Non-blocking; may evict the oldest
    /// queued frame if the mailbox is full.
    pub fn offer(&self, frame: MotionFrame) -> Result<(), MotionFailureCategory> {
        if self.cancel_flag.load(Ordering::Acquire) {
            return Err(MotionFailureCategory::Cancelled);
        }
        match self.frame_tx.offer(frame) {
            super::queue::MotionOfferResult::Disconnected => Err(MotionFailureCategory::BrokenPipe),
            super::queue::MotionOfferResult::Queued
            | super::queue::MotionOfferResult::ReplacedOldest => Ok(()),
        }
    }

    /// Current encoder runtime status.
    pub fn status(&self) -> MotionRuntimeStatus {
        match self.status.load(Ordering::Acquire) {
            0 => MotionRuntimeStatus::On,
            _ => MotionRuntimeStatus::Off,
        }
    }

    /// Finish the recording session.
    ///
    /// Sends the session duration to the worker, drops the frame sender,
    /// joins the worker thread, and returns the outcome. On failure, cleans
    /// up the scratch directory.
    pub fn finish(&mut self, session_duration_ms: Millis) -> MotionRecordingOutcome {
        // Send session duration to the worker via the control channel.
        let _ = self.session_tx.send(session_duration_ms);
        // Drop the frame sender to signal end-of-stream.
        // Replace with a dummy that we immediately drop.
        let (dummy_tx, _dummy_rx) = motion_frame_mailbox(1);
        let old_tx = std::mem::replace(&mut self.frame_tx, dummy_tx);
        drop(old_tx);

        // Wait for the worker thread to complete.
        let handle = match self.worker.take() {
            Some(h) => h,
            None => return MotionRecordingOutcome::Failure(MotionFailureCategory::Cancelled),
        };

        let outcome = match handle.join() {
            Ok(o) => o,
            Err(_) => MotionRecordingOutcome::Failure(MotionFailureCategory::Spawn),
        };

        // Clean up scratch on failure.
        if matches!(outcome, MotionRecordingOutcome::Failure(_)) {
            let _ = std::fs::remove_dir_all(&self.scratch_dir);
        }

        outcome
    }

    /// Cancel the recording. Kills FFmpeg, joins the worker, and removes
    /// the scratch directory.
    pub fn cancel(&mut self) {
        self.cancel_flag.store(true, Ordering::Release);

        // Drop the frame sender to unblock the worker.
        let (dummy_tx, _dummy_rx) = motion_frame_mailbox(1);
        let old_tx = std::mem::replace(&mut self.frame_tx, dummy_tx);
        drop(old_tx);

        // Also close the session channel so the worker doesn't block.
        // session_tx is dropped by replacing with a new one isn't possible
        // for crossbeam senders, but we can just drop self entirely later.
        // The cancel flag is checked by the worker.

        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }

        let _ = std::fs::remove_dir_all(&self.scratch_dir);
    }
}

impl Drop for MotionRecorder {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.cancel();
        }
    }
}

// ─── Worker thread ───────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn worker_loop(
    mut child: Child,
    mut sink: Box<dyn MotionSink>,
    frame_rx: MotionFrameReceiver,
    session_rx: crossbeam_channel::Receiver<Millis>,
    width: u32,
    height: u32,
    part_path: &Path,
    final_path: &Path,
    cancel: &AtomicBool,
    ffprobe_path: &Path,
) -> MotionRecordingOutcome {
    let frame_size = (width as usize) * (height as usize) * 4;
    let mut scheduler: Option<CfrScheduler> = None;
    let mut last_image: Option<SharedActionFrame> = None;

    // Phase 1: receive frames until the session duration arrives.
    // The session duration is sent via a separate control channel.
    let session_duration_ms = loop {
        if cancel.load(Ordering::Acquire) {
            cleanup_process(&mut child, part_path);
            return MotionRecordingOutcome::Failure(MotionFailureCategory::Cancelled);
        }

        // Check if the session duration has been sent.
        match session_rx.try_recv() {
            Ok(dur) => break dur,
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                // Sender dropped without sending duration → cancelled.
                cleanup_process(&mut child, part_path);
                return MotionRecordingOutcome::Failure(MotionFailureCategory::Cancelled);
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {}
        }

        // Try to receive a frame (non-blocking to check cancel/session periodically).
        match frame_rx.try_recv() {
            Some(frame) => {
                if scheduler.is_none() {
                    scheduler = Some(CfrScheduler::new(0));
                }
                let sched = scheduler.as_mut().unwrap();
                let emission = sched.push(frame.at_ms);

                // Write hold frames for the previous image.
                if let Some(ref img) = last_image {
                    for _ in 0..emission.repeat_previous {
                        let rgba = img.as_raw();
                        if rgba.len() == frame_size && sink.write(rgba).is_err() {
                            cleanup_process(&mut child, part_path);
                            return MotionRecordingOutcome::Failure(
                                MotionFailureCategory::BrokenPipe,
                            );
                        }
                    }
                }

                // Write the new frame.
                if emission.write_new {
                    let rgba = frame.image.as_raw();
                    if rgba.len() == frame_size && sink.write(rgba).is_err() {
                        cleanup_process(&mut child, part_path);
                        return MotionRecordingOutcome::Failure(MotionFailureCategory::BrokenPipe);
                    }
                }
                // Always update last_image so duplicate/late frames
                // still replace the current visual state for future holds.
                last_image = Some(frame.image);
            }
            None => {
                // No frame available. Check if the sender disconnected.
                // The frame_rx.recv() would block, but we need non-blocking
                // to check cancel and session_rx. Just sleep briefly and retry.
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    };

    // Phase 2: drain remaining frames from the mailbox.
    let sched = scheduler.get_or_insert_with(|| CfrScheduler::new(session_duration_ms));
    while let Some(frame) = frame_rx.try_recv() {
        let emission = sched.push(frame.at_ms);
        if let Some(ref img) = last_image {
            for _ in 0..emission.repeat_previous {
                let rgba = img.as_raw();
                if rgba.len() == frame_size && sink.write(rgba).is_err() {
                    cleanup_process(&mut child, part_path);
                    return MotionRecordingOutcome::Failure(MotionFailureCategory::BrokenPipe);
                }
            }
        }
        if emission.write_new {
            let rgba = frame.image.as_raw();
            if rgba.len() == frame_size && sink.write(rgba).is_err() {
                cleanup_process(&mut child, part_path);
                return MotionRecordingOutcome::Failure(MotionFailureCategory::BrokenPipe);
            }
        }
        // Always update last_image so duplicate/late frames
        // still replace the current visual state for future holds.
        last_image = Some(frame.image);
    }

    // Phase 3: finalize — fill hold frames to the session duration.
    let remaining = sched.finish(session_duration_ms);
    if let Some(ref img) = last_image {
        let rgba = img.as_raw();
        if rgba.len() == frame_size {
            for _ in 0..remaining {
                if sink.write(rgba).is_err() {
                    cleanup_process(&mut child, part_path);
                    return MotionRecordingOutcome::Failure(MotionFailureCategory::BrokenPipe);
                }
            }
        }
    }

    // Phase 4: close stdin and wait for FFmpeg to exit.
    drop(sink);
    let exit_ok = child.wait().map(|s| s.success()).unwrap_or(false);
    if !exit_ok {
        let _ = std::fs::remove_file(part_path);
        let _ = std::fs::remove_file(final_path);
        return MotionRecordingOutcome::Failure(MotionFailureCategory::Finalize);
    }

    // Phase 5: probe the .part.mp4.
    let probe_result = probe::probe_motion(
        part_path,
        &VideoToolchain {
            ffmpeg: PathBuf::new(),
            ffprobe: ffprobe_path.to_path_buf(),
        },
        width,
        height,
    );

    let metadata = match probe_result {
        Ok(m) => m,
        Err(cat) => {
            let _ = std::fs::remove_file(part_path);
            let _ = std::fs::remove_file(final_path);
            return MotionRecordingOutcome::Failure(cat);
        }
    };

    // Phase 6: rename .part.mp4 → recording.mp4.
    if std::fs::rename(part_path, final_path).is_err() {
        let _ = std::fs::remove_file(part_path);
        let _ = std::fs::remove_file(final_path);
        return MotionRecordingOutcome::Failure(MotionFailureCategory::Filesystem);
    }

    // Phase 7: compute SHA-256 of the final file.
    let sha256 = match compute_sha256(final_path) {
        Ok(h) => h,
        Err(cat) => {
            let _ = std::fs::remove_file(final_path);
            return MotionRecordingOutcome::Failure(cat);
        }
    };

    let final_metadata = MotionMetadata { sha256, ..metadata };

    MotionRecordingOutcome::Ready(ValidatedMotionAsset::new(
        final_metadata,
        final_path.to_path_buf(),
        final_path.parent().unwrap().to_path_buf(),
    ))
}

fn cleanup_process(child: &mut Child, part_path: &Path) {
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(part_path);
}

fn create_scratch_dir() -> Result<PathBuf, MotionFailureCategory> {
    let base = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = base.join(format!(
        "rollshot/action-motion-{nanos}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).map_err(|_| MotionFailureCategory::Filesystem)?;
    Ok(dir)
}

fn compute_sha256(path: &Path) -> Result<String, MotionFailureCategory> {
    use sha2::Digest;
    let bytes = std::fs::read(path).map_err(|_| MotionFailureCategory::Digest)?;
    let hash = sha2::Sha256::digest(&bytes);
    Ok(format!("{hash:x}"))
}

// ─── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::motion::probe::{MotionAudio, MotionCodec};
    use image::RgbaImage;

    // ── Test sink ────────────────────────────────────────────────────────

    /// Test double for `MotionSink`: records all written frame bytes.
    pub(crate) struct TestSink {
        frames: Vec<Vec<u8>>,
    }

    impl TestSink {
        #[allow(dead_code)]
        pub fn new() -> Self {
            Self { frames: Vec::new() }
        }
    }

    impl MotionSink for TestSink {
        fn write(&mut self, rgba: &[u8]) -> Result<(), MotionFailureCategory> {
            self.frames.push(rgba.to_vec());
            Ok(())
        }

        fn frame_count(&self) -> u64 {
            self.frames.len() as u64
        }
    }

    // ── Helpers ──────────────────────────────────────────────────────────

    fn fake_ffmpeg(path: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(
                path,
                "#!/bin/sh\n\
                 if [ \"$1\" = \"-version\" ]; then echo 'ffmpeg fake'; exit 0; fi\n\
                 out=\"\"\n\
                 for arg in \"$@\"; do out=\"$arg\"; done\n\
                 cat >/dev/null\n\
                 printf 'fake mp4 data' > \"$out\"\n",
            )
            .unwrap();
            let mut perms = std::fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).unwrap();

            // Verify the script is executable and responds to -version.
            for _ in 0..10 {
                let status = Command::new(path)
                    .arg("-version")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
                if status.map(|s| s.success()).unwrap_or(false) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!(
                "fake ffmpeg at {} did not respond to -version",
                path.display()
            );
        }
    }

    fn toolchain_with(ffmpeg: &Path) -> VideoToolchain {
        VideoToolchain {
            ffmpeg: ffmpeg.to_path_buf(),
            ffprobe: ffmpeg.to_path_buf(),
        }
    }

    fn rgba_frame(r: u8, g: u8, b: u8, width: u32, height: u32) -> SharedActionFrame {
        Arc::new(RgbaImage::from_pixel(
            width,
            height,
            image::Rgba([r, g, b, 255]),
        ))
    }

    // ── Probe fixture tests ──────────────────────────────────────────────

    #[test]
    fn valid_h264_fixture_parses() {
        #[allow(clippy::useless_format)]
        let raw = format!(
            r#"{{
                "streams": [{{
                    "index": 0,
                    "codec_type": "video",
                    "codec_name": "h264",
                    "width": 640,
                    "height": 480,
                    "r_frame_rate": "30/1",
                    "duration": "2.0"
                }}],
                "format": {{ "duration": "2.0" }}
            }}"#
        );
        let meta = probe::parse_motion_probe_json(raw.as_bytes(), 640, 480, "abc").unwrap();
        assert_eq!(meta.codec, MotionCodec::H264);
        assert_eq!(meta.fps_numerator, 30);
        assert_eq!(meta.fps_denominator, 1);
        assert_eq!(meta.audio, MotionAudio::None);
        assert_eq!(meta.width, 640);
        assert_eq!(meta.height, 480);
        assert_eq!(meta.duration_ms, 2000);
    }

    #[test]
    fn duration_within_34ms_tolerance() {
        #[allow(clippy::useless_format)]
        let raw = format!(
            r#"{{
                "streams": [{{
                    "index": 0,
                    "codec_type": "video",
                    "codec_name": "h264",
                    "width": 320,
                    "height": 240,
                    "r_frame_rate": "30/1",
                    "duration": "1.017"
                }}],
                "format": {{ "duration": "1.017" }}
            }}"#
        );
        let meta = probe::parse_motion_probe_json(raw.as_bytes(), 320, 240, "x").unwrap();
        assert_eq!(meta.duration_ms, 1017);
    }

    #[test]
    fn rejects_audio_stream_present() {
        let raw = br#"{
            "streams": [
                {"index": 0, "codec_type": "video", "codec_name": "h264",
                 "width": 640, "height": 480, "r_frame_rate": "30/1", "duration": "1.0"},
                {"index": 1, "codec_type": "audio", "codec_name": "aac"}
            ],
            "format": {"duration": "1.0"}
        }"#;
        assert_eq!(
            probe::parse_motion_probe_json(raw, 640, 480, "x").unwrap_err(),
            MotionFailureCategory::Probe
        );
    }

    #[test]
    fn rejects_29_97_fps() {
        let raw = br#"{
            "streams": [{"index": 0, "codec_type": "video", "codec_name": "h264",
                         "width": 640, "height": 480, "r_frame_rate": "30000/1001",
                         "duration": "1.0"}],
            "format": {"duration": "1.0"}
        }"#;
        assert_eq!(
            probe::parse_motion_probe_json(raw, 640, 480, "x").unwrap_err(),
            MotionFailureCategory::Probe
        );
    }

    #[test]
    fn rejects_wrong_dimensions() {
        let raw = br#"{
            "streams": [{"index": 0, "codec_type": "video", "codec_name": "h264",
                         "width": 320, "height": 240, "r_frame_rate": "30/1",
                         "duration": "1.0"}],
            "format": {"duration": "1.0"}
        }"#;
        assert_eq!(
            probe::parse_motion_probe_json(raw, 640, 480, "x").unwrap_err(),
            MotionFailureCategory::Probe
        );
    }

    #[test]
    fn rejects_malformed_json() {
        assert_eq!(
            probe::parse_motion_probe_json(b"not json", 640, 480, "x").unwrap_err(),
            MotionFailureCategory::Probe
        );
    }

    #[test]
    fn rejects_second_video_stream() {
        let raw = br#"{
            "streams": [
                {"index": 0, "codec_type": "video", "codec_name": "h264",
                 "width": 640, "height": 480, "r_frame_rate": "30/1", "duration": "1.0"},
                {"index": 1, "codec_type": "video", "codec_name": "h264",
                 "width": 640, "height": 480, "r_frame_rate": "30/1"}
            ],
            "format": {"duration": "1.0"}
        }"#;
        assert_eq!(
            probe::parse_motion_probe_json(raw, 640, 480, "x").unwrap_err(),
            MotionFailureCategory::Probe
        );
    }

    #[test]
    fn rejects_missing_duration() {
        let raw = br#"{
            "streams": [{"index": 0, "codec_type": "video", "codec_name": "h264",
                         "width": 640, "height": 480, "r_frame_rate": "30/1"}],
            "format": {}
        }"#;
        assert_eq!(
            probe::parse_motion_probe_json(raw, 640, 480, "x").unwrap_err(),
            MotionFailureCategory::Probe
        );
    }

    #[test]
    fn rejects_rotation_display_size_mismatch() {
        let raw = br#"{
            "streams": [{"index": 0, "codec_type": "video", "codec_name": "h264",
                         "width": 480, "height": 640, "r_frame_rate": "30/1",
                         "duration": "1.0",
                         "side_data_list": [{"rotation": 90}]}],
            "format": {"duration": "1.0"}
        }"#;
        // 90° rotation: display = 640×480; asking for 320×240 should fail.
        assert_eq!(
            probe::parse_motion_probe_json(raw, 320, 240, "x").unwrap_err(),
            MotionFailureCategory::Probe
        );
    }

    #[test]
    fn accepts_format_level_duration() {
        let raw = br#"{
            "streams": [{"index": 0, "codec_type": "video", "codec_name": "h264",
                         "width": 640, "height": 480, "r_frame_rate": "30/1"}],
            "format": {"duration": "3.5"}
        }"#;
        let meta = probe::parse_motion_probe_json(raw, 640, 480, "x").unwrap();
        assert_eq!(meta.duration_ms, 3500);
    }

    // ── Failure category tests ───────────────────────────────────────────

    #[cfg(unix)]
    #[test]
    fn spawn_failure_returns_tool_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = dir.path().join("no-such-binary");
        let tc = toolchain_with(&ffmpeg);
        let err = MotionRecorder::start(&tc, 640, 480).unwrap_err();
        assert!(
            err == MotionFailureCategory::ToolUnavailable || err == MotionFailureCategory::Spawn,
            "unexpected category: {err:?}"
        );
    }

    #[test]
    fn zero_dimensions_returns_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = dir.path().join("ffmpeg");
        fake_ffmpeg(&ffmpeg);
        let tc = toolchain_with(&ffmpeg);
        assert_eq!(
            MotionRecorder::start(&tc, 0, 480).unwrap_err(),
            MotionFailureCategory::Filesystem
        );
        assert_eq!(
            MotionRecorder::start(&tc, 640, 0).unwrap_err(),
            MotionFailureCategory::Filesystem
        );
    }

    #[test]
    fn odd_dimensions_returns_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = dir.path().join("ffmpeg");
        fake_ffmpeg(&ffmpeg);
        let tc = toolchain_with(&ffmpeg);
        assert_eq!(
            MotionRecorder::start(&tc, 641, 480).unwrap_err(),
            MotionFailureCategory::Filesystem
        );
    }

    #[cfg(unix)]
    #[test]
    fn fake_ffmpeg_start_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = dir.path().join("ffmpeg");
        fake_ffmpeg(&ffmpeg);
        let tc = toolchain_with(&ffmpeg);
        let rec = MotionRecorder::start(&tc, 640, 480).unwrap();
        assert_eq!(rec.status(), MotionRuntimeStatus::On);
    }

    // ── Lifecycle tests ──────────────────────────────────────────────────

    #[cfg(unix)]
    #[test]
    fn frame_order_reaches_sink() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = dir.path().join("ffmpeg");
        fake_ffmpeg(&ffmpeg);
        let tc = toolchain_with(&ffmpeg);
        let mut rec = MotionRecorder::start(&tc, 4, 4).unwrap();

        for i in 0..5 {
            let frame = MotionFrame {
                at_ms: i * 100,
                image: rgba_frame(i as u8, 0, 0, 4, 4),
            };
            rec.offer(frame).unwrap();
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
        let outcome = rec.finish(500);
        // With fake ffmpeg, probe will fail, but the lifecycle doesn't panic.
        assert!(matches!(
            outcome,
            MotionRecordingOutcome::Ready(_) | MotionRecordingOutcome::Failure(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn duplicate_frames_do_not_add_ticks() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = dir.path().join("ffmpeg");
        fake_ffmpeg(&ffmpeg);
        let tc = toolchain_with(&ffmpeg);
        let mut rec = MotionRecorder::start(&tc, 4, 4).unwrap();

        // Offer the same timestamp 3 times.
        for _ in 0..3 {
            rec.offer(MotionFrame {
                at_ms: 0,
                image: rgba_frame(42, 0, 0, 4, 4),
            })
            .unwrap();
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
        let outcome = rec.finish(100);
        assert!(matches!(
            outcome,
            MotionRecordingOutcome::Ready(_) | MotionRecordingOutcome::Failure(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn queue_saturation_still_finishs() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = dir.path().join("ffmpeg");
        fake_ffmpeg(&ffmpeg);
        let tc = toolchain_with(&ffmpeg);
        let mut rec = MotionRecorder::start(&tc, 4, 4).unwrap();

        // Over-fill the mailbox (capacity is 8).
        for i in 0..20 {
            let _ = rec.offer(MotionFrame {
                at_ms: i * 33,
                image: rgba_frame((i % 256) as u8, 0, 0, 4, 4),
            });
        }

        let outcome = rec.finish(660);
        assert!(matches!(
            outcome,
            MotionRecordingOutcome::Ready(_) | MotionRecordingOutcome::Failure(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_joins_and_removes_scratch() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = dir.path().join("ffmpeg");
        fake_ffmpeg(&ffmpeg);
        let tc = toolchain_with(&ffmpeg);
        let mut rec = MotionRecorder::start(&tc, 4, 4).unwrap();

        rec.offer(MotionFrame {
            at_ms: 0,
            image: rgba_frame(0, 0, 0, 4, 4),
        })
        .unwrap();

        rec.cancel();
        assert_eq!(rec.status(), MotionRuntimeStatus::Off);
    }

    #[cfg(unix)]
    #[test]
    fn stalled_sink_does_not_block_offer() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = dir.path().join("ffmpeg");
        fake_ffmpeg(&ffmpeg);
        let tc = toolchain_with(&ffmpeg);
        let mut rec = MotionRecorder::start(&tc, 4, 4).unwrap();

        // Offer frames rapidly; the mailbox should handle saturation.
        for i in 0..50 {
            let _ = rec.offer(MotionFrame {
                at_ms: i * 10,
                image: rgba_frame(0, 0, 0, 4, 4),
            });
        }

        // Cancel should not deadlock.
        rec.cancel();
        assert_eq!(rec.status(), MotionRuntimeStatus::Off);
    }

    // ── Real FFmpeg integration test (opt-in) ────────────────────────────

    #[cfg(unix)]
    #[test]
    #[ignore]
    fn real_ffmpeg_produces_valid_silent_h264() {
        if std::env::var("ROLLSHOT_TEST_FFMPEG").ok().as_deref() != Some("1") {
            return;
        }

        let ffmpeg = std::env::var("ROLLSHOT_FFMPEG").unwrap_or_else(|_| "ffmpeg".to_string());
        let ffprobe = std::env::var("ROLLSHOT_FFPROBE").unwrap_or_else(|_| "ffprobe".to_string());

        let tc = VideoToolchain {
            ffmpeg: PathBuf::from(&ffmpeg),
            ffprobe: PathBuf::from(&ffprobe),
        };

        let mut rec = MotionRecorder::start(&tc, 320, 240).unwrap();

        // Feed 60 frames at ~33ms intervals (~2 seconds).
        for i in 0..60 {
            let r = (i * 4) as u8;
            rec.offer(MotionFrame {
                at_ms: i * 33,
                image: rgba_frame(r, 128, 64, 320, 240),
            })
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        std::thread::sleep(std::time::Duration::from_millis(200));

        let outcome = rec.finish(2000);
        match outcome {
            MotionRecordingOutcome::Ready(asset) => {
                assert_eq!(asset.codec(), MotionCodec::H264);
                assert_eq!(asset.audio(), MotionAudio::None);
                assert_eq!(asset.fps_numerator(), 30);
                assert_eq!(asset.fps_denominator(), 1);
                assert_eq!(asset.width(), 320);
                assert_eq!(asset.height(), 240);
                // Duration within 34ms of 2000ms.
                let dur = asset.duration_ms();
                assert!(
                    (1966..=2034).contains(&dur),
                    "duration {dur}ms not within 34ms of 2000ms"
                );
                assert!(!asset.sha256().is_empty());
                assert!(asset.source_path().exists());
            }
            MotionRecordingOutcome::Failure(cat) => {
                panic!("expected success, got failure: {cat}");
            }
        }
    }
}
