use std::io::Write;
use std::path::{Path, PathBuf};

use image::RgbaImage;

// ---------------------------------------------------------------------------
// Mailbox types
// ---------------------------------------------------------------------------

// Dead code suppression: all types below are API surface consumed by the
// producer loop (future task). Only exercised by tests today.
#[allow(dead_code)]
pub(crate) struct TimedFrame {
    pub at_ms: u64,
    pub image: RgbaImage,
}

// Dead code suppression: future-task API surface (producer loop).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OfferResult {
    Queued,
    ReplacedOldest,
    Disconnected,
}

// Dead code suppression: future-task API surface (producer loop).
#[allow(dead_code)]
pub(crate) struct LatestFrameSender {
    tx: crossbeam_channel::Sender<TimedFrame>,
    rx: crossbeam_channel::Receiver<TimedFrame>,
}

// Dead code suppression: future-task API surface (producer loop).
#[allow(dead_code)]
impl LatestFrameSender {
    /// Offer a frame to the mailbox. Never blocks.
    /// If the queue is full, evicts the oldest frame and inserts the new one.
    pub(crate) fn offer(&self, frame: TimedFrame) -> OfferResult {
        match self.tx.try_send(frame) {
            Ok(()) => OfferResult::Queued,
            Err(crossbeam_channel::TrySendError::Full(frame)) => {
                // Evict oldest to make room.
                let _ = self.rx.try_recv();
                match self.tx.try_send(frame) {
                    Ok(()) => OfferResult::ReplacedOldest,
                    Err(crossbeam_channel::TrySendError::Full(_)) => OfferResult::ReplacedOldest,
                    Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
                        OfferResult::Disconnected
                    }
                }
            }
            Err(crossbeam_channel::TrySendError::Disconnected(_)) => OfferResult::Disconnected,
        }
    }
}

// Dead code suppression: future-task API surface (producer loop).
#[allow(dead_code)]
pub(crate) struct LatestFrameReceiver {
    rx: crossbeam_channel::Receiver<TimedFrame>,
}

impl LatestFrameReceiver {
    /// Block until a frame arrives. Returns `Err` when all senders are dropped.
    pub(crate) fn recv(&self) -> Result<TimedFrame, crossbeam_channel::RecvError> {
        self.rx.recv()
    }
}

/// Create a bounded latest-frame mailbox.
///
/// The sender holds a clone of the receiver solely for eviction; it never
/// performs a blocking send.
// Dead code suppression: future-task API surface (producer loop).
#[allow(dead_code)]
pub(crate) fn latest_frame_mailbox(capacity: usize) -> (LatestFrameSender, LatestFrameReceiver) {
    let (tx, rx) = crossbeam_channel::bounded(capacity);
    let rx_clone = rx.clone();
    (
        LatestFrameSender { tx, rx: rx_clone },
        LatestFrameReceiver { rx },
    )
}

// ---------------------------------------------------------------------------
// CFR scheduler (integer-only arithmetic)
// ---------------------------------------------------------------------------

// Dead code suppression: future-task API surface (producer loop).
#[allow(dead_code)]
pub(crate) struct CfrScheduler {
    fps: u32,
    next_tick: u64,
    frames_written: u64,
}

impl CfrScheduler {
    pub(crate) fn new(fps: u32) -> Self {
        Self {
            fps,
            next_tick: 0,
            frames_written: 0,
        }
    }

    /// Push a new frame at `at_ms` (session-relative).
    ///
    /// Returns the number of output ticks caused by this arrival. The worker
    /// writes the prior image for intermediate ticks and the new image on the
    /// arrival tick. The first push writes exactly one initial frame.
    pub(crate) fn push(&mut self, at_ms: u64) -> u64 {
        // Integer-only comparison: tick_index * 1000 vs at_ms * fps, both u128.
        let target_tick = ((at_ms as u128 * self.fps as u128) / 1000) as u64;

        if self.next_tick == 0 {
            // First push — write the initial frame at tick 0.
            self.next_tick = 1;
            self.frames_written = 1;
            return 1;
        }

        if target_tick < self.next_tick {
            // Frame arrived before the current output position; skip.
            return 0;
        }

        let count = target_tick - self.next_tick + 1;
        self.frames_written += count;
        self.next_tick = target_tick + 1;
        count
    }

    /// Emit the last frame until the encoded timeline reaches `duration_ms`.
    ///
    /// Returns the number of additional ticks written.
    pub(crate) fn finish(&mut self, duration_ms: u64) -> u64 {
        let target = duration_ms as u128 * self.fps as u128;
        let mut count = 0u64;
        while (self.frames_written as u128) * 1000 < target {
            self.frames_written += 1;
            self.next_tick += 1;
            count += 1;
        }
        count
    }

    pub(crate) fn frames_written(&self) -> u64 {
        self.frames_written
    }

    /// Encoded duration in milliseconds: `frames_written * 1000 / fps`.
    pub(crate) fn duration_ms(&self) -> u64 {
        (self.frames_written * 1000) / self.fps as u64
    }
}

// ---------------------------------------------------------------------------
// FFmpeg process lifecycle
// ---------------------------------------------------------------------------

// Dead code suppression: future-task API surface (producer loop).
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) enum PipelineError {
    Spawn {
        category: String,
    },
    Write {
        category: String,
    },
    Exit {
        status: &'static str,
        category: String,
    },
    Rename {
        category: String,
    },
}

// Dead code suppression: future-task API surface (producer loop).
#[allow(dead_code)]
pub(crate) struct EncoderSummary {
    pub frames_written: u64,
    pub pid: u32,
    pub ffmpeg_exit_status: i32,
    pub source_duration_ms: u64,
    pub encoded_duration_ms: u64,
}

/// Derive the temp sibling path: `<parent>/<stem>.tmp.mp4`.
// Dead code suppression: future-task API surface (producer loop).
#[allow(dead_code)]
pub(crate) fn temp_output_path(output: &Path) -> PathBuf {
    let parent = output.parent().unwrap_or(Path::new("."));
    let stem = output.file_stem().unwrap_or_default();
    parent.join(format!("{}.tmp.mp4", stem.to_string_lossy()))
}

// Dead code suppression: future-task API surface (producer loop).
#[allow(dead_code)]
fn io_error_category(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "not_found".into(),
        std::io::ErrorKind::PermissionDenied => "permission_denied".into(),
        _ => "io_error".into(),
    }
}

// Dead code suppression: future-task API surface (producer loop).
#[allow(dead_code)]
fn cleanup_temp(temp_path: &Path) {
    let _ = std::fs::remove_file(temp_path);
}

/// Spawn FFmpeg, write raw RGBA frames through the bounded mailbox receiver,
/// and produce an atomic MP4 output.
///
/// On any failure the temp sibling is removed before returning.
// Dead code suppression: future-task API surface (producer loop).
#[allow(dead_code)]
pub(crate) fn run_encoder(
    config: crate::RunConfig,
    receiver: LatestFrameReceiver,
) -> Result<EncoderSummary, PipelineError> {
    let temp_path = temp_output_path(&config.output);

    // Build FFmpeg command with the exact prescribed arguments.
    let mut cmd = std::process::Command::new(&config.ffmpeg);
    cmd.args([
        "-y",
        "-f",
        "rawvideo",
        "-pixel_format",
        "rgba",
        "-video_size",
        &format!("{}x{}", config.width, config.height),
        "-framerate",
        &config.fps.to_string(),
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
    ]);
    cmd.arg(&temp_path);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::null());

    let mut child = cmd.spawn().map_err(|e| {
        cleanup_temp(&temp_path);
        PipelineError::Spawn {
            category: io_error_category(&e),
        }
    })?;

    // Drain stderr on its own thread BEFORE writing frames (deadlock prevention).
    let mut stderr = child.stderr.take().expect("stderr was piped");
    let stderr_thread = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });

    let mut stdin = child.stdin.take().expect("stdin was piped");
    let mut scheduler = CfrScheduler::new(config.fps);
    let mut current_frame: Option<TimedFrame> = None;
    let mut last_at_ms: u64 = 0;

    // --- frame-writing loop ---
    while let Ok(frame) = receiver.recv() {
        let at_ms = frame.at_ms;
        let ticks = scheduler.push(at_ms);
        if ticks > 0 {
            let pixels = frame.image.as_raw();
            for _ in 0..ticks {
                if let Err(e) = stdin.write_all(pixels) {
                    drop(stdin);
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stderr_thread.join();
                    cleanup_temp(&temp_path);
                    return Err(PipelineError::Write {
                        category: io_error_category(&e),
                    });
                }
            }
            last_at_ms = at_ms;
        }
        current_frame = Some(frame);
    }

    // --- finish: hold last frame for remaining ticks ---
    if let Some(frame) = &current_frame {
        let duration_ms = config.duration_secs * 1000;
        let remaining = scheduler.finish(duration_ms);
        if remaining > 0 {
            let pixels = frame.image.as_raw();
            for _ in 0..remaining {
                if let Err(e) = stdin.write_all(pixels) {
                    drop(stdin);
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stderr_thread.join();
                    cleanup_temp(&temp_path);
                    return Err(PipelineError::Write {
                        category: io_error_category(&e),
                    });
                }
            }
        }
    }

    // --- close stdin, wait for exit ---
    drop(stdin);

    let exit_status = child.wait().map_err(|e| {
        cleanup_temp(&temp_path);
        PipelineError::Exit {
            status: "wait_failed",
            category: io_error_category(&e),
        }
    });

    let _stderr_output = stderr_thread.join().unwrap_or_default();

    // At this point stdin is already dropped, so the child should be winding
    // down.  If wait() itself failed we still need to reap & clean up.
    let exit_status = match exit_status {
        Ok(s) => s,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };

    if !exit_status.success() {
        cleanup_temp(&temp_path);
        return Err(PipelineError::Exit {
            status: "non_zero",
            category: format!("exit_{}", exit_status.code().unwrap_or(-1)),
        });
    }

    // --- atomic rename ---
    std::fs::rename(&temp_path, &config.output).map_err(|e| {
        cleanup_temp(&temp_path);
        PipelineError::Rename {
            category: io_error_category(&e),
        }
    })?;

    Ok(EncoderSummary {
        frames_written: scheduler.frames_written(),
        pid: child.id(),
        ffmpeg_exit_status: exit_status.code().unwrap_or(-1),
        source_duration_ms: last_at_ms,
        encoded_duration_ms: scheduler.duration_ms(),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(at_ms: u64) -> TimedFrame {
        TimedFrame {
            at_ms,
            image: RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255])),
        }
    }

    // -- Mailbox tests --

    #[test]
    fn latest_mailbox_replaces_oldest_without_waiting() {
        let (sender, receiver) = latest_frame_mailbox(1);
        assert_eq!(sender.offer(frame(0)), OfferResult::Queued);
        assert_eq!(sender.offer(frame(33)), OfferResult::ReplacedOldest);
        assert_eq!(receiver.recv().unwrap().at_ms, 33);
    }

    // -- CFR scheduler tests --

    #[test]
    fn cfr_scheduler_holds_last_frame_across_timestamp_gap() {
        let mut scheduler = CfrScheduler::new(30);
        assert_eq!(scheduler.push(0), 1);
        assert_eq!(scheduler.push(100), 3);
        assert_eq!(scheduler.finish(134), 1);
        assert_eq!(scheduler.frames_written(), 5);
    }

    #[test]
    fn scheduler_duration_is_within_one_frame() {
        let mut scheduler = CfrScheduler::new(30);
        scheduler.push(0);
        scheduler.push(997);
        scheduler.finish(1_000);
        assert!((scheduler.duration_ms() as i64 - 1_000).abs() <= 34);
    }

    // -- FFmpeg failure-cleanup tests (unix) --

    #[cfg(unix)]
    #[test]
    fn encoder_exits_nonzero_cleans_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake_ffmpeg");
        std::fs::write(&script, "#!/bin/sh\nexit 7\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let output = dir.path().join("output.mp4");
        let config = crate::RunConfig {
            ffmpeg: script,
            ffprobe: "ffprobe".into(),
            output: output.clone(),
            report: dir.path().join("report.json"),
            width: 2,
            height: 2,
            fps: 30,
            duration_secs: 1,
            queue_capacity: 2,
        };

        let (sender, receiver) = latest_frame_mailbox(2);
        drop(sender);

        let result = run_encoder(config.clone(), receiver);
        assert!(matches!(result, Err(PipelineError::Exit { .. })));
        assert!(!config.output.exists());
        assert!(!temp_output_path(&config.output).exists());
    }

    #[cfg(unix)]
    #[test]
    fn nonexistent_ffmpeg_maps_to_spawn_error() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("output.mp4");
        let config = crate::RunConfig {
            ffmpeg: "/nonexistent/ffmpeg".into(),
            ffprobe: "ffprobe".into(),
            output: output.clone(),
            report: dir.path().join("report.json"),
            width: 2,
            height: 2,
            fps: 30,
            duration_secs: 1,
            queue_capacity: 2,
        };

        let (sender, receiver) = latest_frame_mailbox(2);
        drop(sender);

        let result = run_encoder(config.clone(), receiver);
        assert!(matches!(result, Err(PipelineError::Spawn { .. })));
        assert!(!config.output.exists());
        assert!(!temp_output_path(&config.output).exists());
    }
}
