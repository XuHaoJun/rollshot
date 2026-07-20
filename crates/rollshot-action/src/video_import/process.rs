use std::collections::HashMap;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::metrics::LumaPlane;

use super::probe::{parse_probe_json, probe_args, ProbeMetadata, VideoToolchain};
use super::{VideoImportCancellation, VideoImportError, ANALYSIS_FPS, ANALYSIS_WIDTH};

const STDERR_RING_CAPACITY: usize = 64 * 1024;
const PROGRESS_LINE_MAX: usize = 256;
const POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug)]
pub struct CancellableChild {
    child: Child,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    finished: bool,
}

impl CancellableChild {
    pub fn spawn(mut command: Command) -> Result<Self, VideoImportError> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command
            .spawn()
            .map_err(|_| VideoImportError::DecoderUnavailable)?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        Ok(Self {
            child,
            stdout,
            stderr,
            finished: false,
        })
    }

    pub fn take_pipes(&mut self) -> Option<(ChildStdout, ChildStderr)> {
        let stdout = self.stdout.take()?;
        let stderr = self.stderr.take();
        stderr.map(|stderr| (stdout, stderr))
    }

    pub fn wait(&mut self) -> Result<ExitStatus, VideoImportError> {
        let status = self
            .child
            .wait()
            .map_err(|_| VideoImportError::ProbeFailed)?;
        self.finished = true;
        Ok(status)
    }

    pub fn kill_and_wait(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.finished = true;
    }
}

impl Drop for CancellableChild {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

pub fn run_cancellable_child<F>(
    mut child: CancellableChild,
    cancel: &VideoImportCancellation,
    stdout_consumer: F,
) -> Result<(), VideoImportError>
where
    F: FnOnce(ChildStdout) + Send + 'static,
{
    let (stdout, stderr) = child.take_pipes().ok_or(VideoImportError::ProbeFailed)?;

    let consumer_handle: JoinHandle<()> = thread::spawn(move || {
        stdout_consumer(stdout);
    });

    let stderr_handle: JoinHandle<StderrDiagnostics> = thread::spawn(move || drain_stderr(stderr));

    loop {
        if cancel.is_cancelled() {
            child.kill_and_wait();
            let _ = consumer_handle.join();
            let _ = stderr_handle.join();
            return Err(VideoImportError::Cancelled);
        }

        match child.child.try_wait() {
            Ok(Some(status)) => {
                let _ = consumer_handle.join();
                let _diagnostics = stderr_handle.join().unwrap_or_default();

                if !status.success() {
                    tracing::event!(
                        target: "rollshot::action::video_import",
                        tracing::Level::WARN,
                        category = "child_exit",
                        success = false,
                        code = status.code(),
                    );
                    return Err(VideoImportError::ProbeFailed);
                }
                return Ok(());
            }
            Ok(None) => {
                thread::sleep(POLL_INTERVAL);
            }
            Err(_) => {
                child.kill_and_wait();
                let _ = consumer_handle.join();
                let _ = stderr_handle.join();
                return Err(VideoImportError::ProbeFailed);
            }
        }
    }
}

#[derive(Default)]
struct StderrDiagnostics {
    ring: Vec<u8>,
    progress_count: u64,
}

fn drain_stderr(mut stderr: ChildStderr) -> StderrDiagnostics {
    let mut diagnostics = StderrDiagnostics::default();
    let mut buf = [0u8; 4096];

    loop {
        match stderr.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let chunk = &buf[..n];

                for line in chunk.split(|&b| b == b'\n') {
                    if line.len() > PROGRESS_LINE_MAX {
                        continue;
                    }
                    if let Ok(text) = std::str::from_utf8(line) {
                        if text.starts_with("out_time_ms=")
                            || text.starts_with("out_time=")
                            || text.starts_with("frame=")
                            || text.starts_with("speed=")
                            || text.starts_with("progress=")
                        {
                            diagnostics.progress_count += 1;
                        }
                    }
                }

                let remaining = STDERR_RING_CAPACITY.saturating_sub(diagnostics.ring.len());
                if remaining > 0 {
                    let take = chunk.len().min(remaining);
                    diagnostics.ring.extend_from_slice(&chunk[..take]);
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }

    diagnostics
}

pub fn probe_video(
    input: &Path,
    toolchain: &VideoToolchain,
    cancel: &VideoImportCancellation,
) -> Result<ProbeMetadata, VideoImportError> {
    let mut cmd = Command::new(&toolchain.ffprobe);
    let args = probe_args(input);
    // Filter out flags that are ffmpeg-specific and not supported by ffprobe.
    // CancellableChild::spawn already sets stdin to null (-nostdin is redundant).
    let skip = ["-nostdin", "-an", "-sn", "-dn"];
    for arg in args {
        if skip.contains(&arg.as_str()) {
            continue;
        }
        cmd.arg(&arg);
    }

    let child = CancellableChild::spawn(cmd)?;

    let (tx, rx) = std::sync::mpsc::channel();
    run_cancellable_child(child, cancel, move |mut stdout| {
        let mut buf = Vec::with_capacity(super::probe::MAX_PROBE_JSON_BYTES.min(64 * 1024));
        {
            let mut limited = stdout
                .by_ref()
                .take((super::probe::MAX_PROBE_JSON_BYTES + 1) as u64);
            let _ = limited.read_to_end(&mut buf);
        }
        let _ = io::copy(&mut stdout, &mut io::sink());
        let _ = tx.send(buf);
    })?;

    let output = rx.recv().map_err(|_| VideoImportError::ProbeFailed)?;
    parse_probe_json(&output)
}

pub fn run_analysis_pass(
    input: &Path,
    toolchain: &VideoToolchain,
    meta: ProbeMetadata,
    frame_size: usize,
    cancel: VideoImportCancellation,
    mut on_frame: impl FnMut(u64, LumaPlane),
) -> Result<(), VideoImportError> {
    let analysis_filter = format!(
        "fps={},scale={}:-2,format=gray",
        ANALYSIS_FPS, ANALYSIS_WIDTH
    );

    let mut cmd = Command::new(&toolchain.ffmpeg);
    cmd.args(["-nostdin", "-an", "-sn", "-dn"]);

    let vf = match meta.rotation_degrees {
        90 => format!("transpose=1,{}", analysis_filter),
        180 => format!("transpose=1,transpose=1,{}", analysis_filter),
        270 => format!("transpose=2,{}", analysis_filter),
        _ => analysis_filter,
    };

    cmd.args([
        "-noautorotate",
        "-i",
        &input.to_string_lossy(),
        "-vf",
        &vf,
        "-f",
        "rawvideo",
        "-pix_fmt",
        "gray",
        "-progress",
        "pipe:2",
        "pipe:1",
    ]);

    let mut child = CancellableChild::spawn(cmd)?;
    let (stdout, stderr) = child
        .take_pipes()
        .ok_or(VideoImportError::DecoderUnavailable)?;

    let stderr_handle: JoinHandle<StderrDiagnostics> = thread::spawn(move || drain_stderr(stderr));

    let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel(1);
    let reader_handle = thread::spawn(move || {
        let mut stdout = stdout;
        loop {
            let mut frame = vec![0u8; frame_size];
            match read_exact_or_eof(&mut stdout, &mut frame) {
                Ok(true) => {
                    if frame_tx.send(AnalysisRead::Frame(frame)).is_err() {
                        break;
                    }
                }
                Ok(false) => {
                    let _ = frame_tx.send(AnalysisRead::Eof);
                    break;
                }
                Err(error) => {
                    let _ = frame_tx.send(AnalysisRead::Error(error.kind()));
                    break;
                }
            }
        }
    });

    let cancel_ref = cancel.clone();
    let mut sample_index: u64 = 0;
    let mut observed_status = None;

    loop {
        if cancel_ref.is_cancelled() {
            child.kill_and_wait();
            drop(frame_rx);
            let _ = reader_handle.join();
            let _ = stderr_handle.join();
            return Err(VideoImportError::Cancelled);
        }

        match frame_rx.recv_timeout(POLL_INTERVAL) {
            Ok(AnalysisRead::Frame(frame_buf)) => {
                let width = ANALYSIS_WIDTH;
                let height = (frame_size / width as usize) as u32;
                let samples: Vec<f32> = frame_buf.iter().map(|&b| b as f32).collect();
                let luma = LumaPlane {
                    width,
                    height,
                    samples,
                };
                on_frame(sample_index, luma);
                sample_index += 1;
            }
            Ok(AnalysisRead::Eof) => break,
            Ok(AnalysisRead::Error(error_kind)) => {
                child.kill_and_wait();
                drop(frame_rx);
                let _ = reader_handle.join();
                let _ = stderr_handle.join();
                tracing::event!(
                    target: "rollshot::action::video_import",
                    tracing::Level::WARN,
                    category = "analysis_read",
                    ?error_kind,
                    sample_index,
                    "analysis frame read failed"
                );
                return Err(VideoImportError::DecodeFailed);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => match child.child.try_wait() {
                Ok(Some(status)) => observed_status = Some(status),
                Ok(None) => {}
                Err(_) => {
                    child.kill_and_wait();
                    drop(frame_rx);
                    let _ = reader_handle.join();
                    let _ = stderr_handle.join();
                    return Err(VideoImportError::DecodeFailed);
                }
            },
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let status = match observed_status {
        Some(status) => {
            child.finished = true;
            status
        }
        None => child.wait()?,
    };
    let _ = reader_handle.join();
    let _diagnostics = stderr_handle.join().unwrap_or_default();
    validate_analysis_completion(status.success(), false)
}

enum AnalysisRead {
    Frame(Vec<u8>),
    Eof,
    Error(io::ErrorKind),
}

fn validate_analysis_completion(
    status_success: bool,
    truncated_frame: bool,
) -> Result<(), VideoImportError> {
    if status_success && !truncated_frame {
        Ok(())
    } else {
        Err(VideoImportError::DecodeFailed)
    }
}

fn read_exact_or_eof(reader: &mut impl Read, buf: &mut [u8]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => {
                if filled == 0 {
                    return Ok(false);
                }
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated frame",
                ));
            }
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
pub fn run_evidence_pass(
    input: &Path,
    toolchain: &VideoToolchain,
    meta: ProbeMetadata,
    requested_indices: &[usize],
    staging_dir: &Path,
    cancel: &VideoImportCancellation,
    progress: &(impl Fn(super::VideoImportProgress) + Send + Sync),
    total_ms: u64,
) -> Result<HashMap<usize, std::path::PathBuf>, VideoImportError> {
    if requested_indices.is_empty() {
        return Ok(HashMap::new());
    }

    let select_filter = build_select_filter(requested_indices);

    let mut cmd = Command::new(&toolchain.ffmpeg);
    cmd.args(["-nostdin", "-an", "-sn", "-dn"]);

    let scale_filter = format!(
        "scale='min({},iw)':'min({},ih)':force_original_aspect_ratio=decrease",
        super::EVIDENCE_MAX_LONG_EDGE,
        super::EVIDENCE_MAX_LONG_EDGE,
    );

    let vf = if meta.rotation_degrees == 90 {
        format!("transpose=1,{},{},format=rgba", select_filter, scale_filter)
    } else if meta.rotation_degrees == 180 {
        format!(
            "transpose=1,transpose=1,{},{},format=rgba",
            select_filter, scale_filter
        )
    } else if meta.rotation_degrees == 270 {
        format!("transpose=2,{},{},format=rgba", select_filter, scale_filter)
    } else {
        format!("{},{},format=rgba", select_filter, scale_filter)
    };

    cmd.args([
        "-noautorotate",
        "-i",
        &input.to_string_lossy(),
        "-vf",
        &vf,
        "-fps_mode",
        "passthrough",
        "-pix_fmt",
        "rgba",
        staging_dir.join("%05d.png").to_str().unwrap(),
    ]);

    let cancel_ref = cancel.clone();

    let mut child = CancellableChild::spawn(cmd)?;
    let (stdout, stderr) = child
        .take_pipes()
        .ok_or(VideoImportError::DecoderUnavailable)?;
    let stdout_handle = thread::spawn(move || drain_stdout(stdout));
    let stderr_handle = thread::spawn(move || drain_stderr(stderr));
    let cancel_for_monitor = cancel_ref.clone();
    let mut emitted_count = 0usize;

    loop {
        if cancel_for_monitor.is_cancelled() {
            child.kill_and_wait();
            let _ = stdout_handle.join();
            let _ = stderr_handle.join();
            return Err(VideoImportError::Cancelled);
        }

        let current_count = count_png_files(staging_dir);
        if current_count > emitted_count {
            emitted_count = current_count;
            let processed_ms = requested_indices
                .get(current_count.saturating_sub(1))
                .map_or(0, |index| (*index as u64) * 1000 / ANALYSIS_FPS);
            progress(super::VideoImportProgress {
                pass: super::VideoImportPass::Extract,
                processed_ms: processed_ms.min(total_ms),
                total_ms,
                retained_candidates: requested_indices.len(),
            });
        }

        match child.child.try_wait() {
            Ok(Some(status)) => {
                child.finished = true;
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();
                if !status.success() {
                    tracing::event!(
                        target: "rollshot::action::video_import",
                        tracing::Level::WARN,
                        category = "evidence_exit",
                        success = false,
                        code = status.code(),
                    );
                    return Err(VideoImportError::EvidenceMissing);
                }
                break;
            }
            Ok(None) => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                child.kill_and_wait();
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();
                return Err(VideoImportError::EvidenceMissing);
            }
        }
    }

    let output_files: Vec<std::path::PathBuf> = std::fs::read_dir(staging_dir)
        .map_err(|_| VideoImportError::EvidenceMissing)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "png"))
        .map(|e| e.path())
        .collect();

    let mut sorted_outputs = output_files;
    sorted_outputs.sort();
    map_extracted_outputs(requested_indices, sorted_outputs)
}

fn build_select_filter(indices: &[usize]) -> String {
    if indices.is_empty() {
        return format!("fps={ANALYSIS_FPS},select=0");
    }
    let mut conditions = Vec::new();
    let mut start = indices[0];
    let mut end = start;
    for &index in &indices[1..] {
        if index == end.saturating_add(1) {
            end = index;
            continue;
        }
        conditions.push(select_range(start, end));
        start = index;
        end = index;
    }
    conditions.push(select_range(start, end));
    format!("fps={ANALYSIS_FPS},select='{}'", conditions.join("+"))
}

fn select_range(start: usize, end: usize) -> String {
    if start == end {
        format!("eq(n\\,{start})")
    } else {
        format!("between(n\\,{start}\\,{end})")
    }
}

fn drain_stdout(mut stdout: ChildStdout) {
    let _ = io::copy(&mut stdout, &mut io::sink());
}

fn count_png_files(staging_dir: &Path) -> usize {
    std::fs::read_dir(staging_dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
        .count()
}

pub(crate) fn map_extracted_outputs(
    requested_indices: &[usize],
    sorted_outputs: Vec<std::path::PathBuf>,
) -> Result<HashMap<usize, std::path::PathBuf>, VideoImportError> {
    if sorted_outputs.len() != requested_indices.len() {
        tracing::warn!(
            target: "rollshot::action::video_import",
            requested_count = requested_indices.len(),
            output_count = sorted_outputs.len(),
            "evidence extraction returned an unexpected frame count"
        );
        return Err(VideoImportError::EvidenceMissing);
    }
    Ok(requested_indices
        .iter()
        .copied()
        .zip(sorted_outputs)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long_running_fixture_process() -> CancellableChild {
        let mut cmd = Command::new("sleep");
        cmd.arg("300");
        CancellableChild::spawn(cmd).expect("failed to spawn sleep")
    }

    fn fixture_process_is_alive(pid: u32) -> bool {
        Command::new("ps")
            .args(["-p", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn cancellation_kills_and_waits_for_child() {
        let fixture = long_running_fixture_process();
        let pid = fixture.child.id();
        let cancel = VideoImportCancellation::default();
        cancel.cancel();
        let error = run_cancellable_child(fixture, &cancel, |_| {}).unwrap_err();
        assert_eq!(error.category(), "cancelled");
        assert!(!fixture_process_is_alive(pid));
    }

    #[test]
    fn successful_child_returns_ok() {
        let cmd = Command::new("true");
        let child = CancellableChild::spawn(cmd).unwrap();
        let cancel = VideoImportCancellation::default();
        run_cancellable_child(child, &cancel, |mut stdout| {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
        })
        .unwrap();
    }

    #[test]
    fn non_zero_exit_returns_probe_failed() {
        let cmd = Command::new("false");
        let child = CancellableChild::spawn(cmd).unwrap();
        let cancel = VideoImportCancellation::default();
        let err = run_cancellable_child(child, &cancel, |mut stdout| {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
        })
        .unwrap_err();
        assert_eq!(err.category(), "probe_failed");
    }

    #[test]
    fn spawn_failure_returns_decoder_unavailable() {
        let cmd = Command::new("definitely-not-a-real-binary-xyz");
        let err = CancellableChild::spawn(cmd).unwrap_err();
        assert_eq!(err.category(), "decoder_unavailable");
    }

    #[test]
    fn stderr_ring_is_bounded() {
        let mut cmd = Command::new("bash");
        cmd.args(["-c", "for i in $(seq 1 100000); do echo $i >&2; done"]);
        let child = CancellableChild::spawn(cmd).unwrap();
        let cancel = VideoImportCancellation::default();
        run_cancellable_child(child, &cancel, |mut stdout| {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
        })
        .unwrap();
    }

    #[test]
    fn drop_reaps_child_on_early_return() {
        let fixture = long_running_fixture_process();
        let pid = fixture.child.id();
        drop(fixture);
        std::thread::sleep(Duration::from_millis(100));
        assert!(!fixture_process_is_alive(pid));
    }

    #[test]
    fn evidence_filter_uses_the_analysis_sample_domain() {
        let filter = build_select_filter(&[0, 1, 4, 8, 9]);
        assert!(filter.starts_with("fps=2,"), "filter = {filter}");
        assert!(filter.contains("eq(n\\,4)"), "filter = {filter}");
        assert!(filter.contains("between(n\\,0\\,1)"), "filter = {filter}");
        assert!(filter.contains("between(n\\,8\\,9)"), "filter = {filter}");
    }

    #[test]
    fn analysis_rejects_partial_output_when_decoder_exits_non_zero() {
        assert!(validate_analysis_completion(false, false).is_err());
    }

    #[test]
    fn analysis_rejects_a_truncated_final_frame() {
        assert!(validate_analysis_completion(true, true).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn analysis_cancel_interrupts_a_stalled_decoder() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        use std::time::Instant;

        let dir = tempfile::tempdir().unwrap();
        let decoder = dir.path().join("stalled-decoder");
        let mut file = std::fs::File::create(&decoder).unwrap();
        file.write_all(b"#!/bin/sh\nexec sleep 300\n").unwrap();
        let mut permissions = file.metadata().unwrap().permissions();
        permissions.set_mode(0o755);
        drop(file);
        std::fs::set_permissions(&decoder, permissions).unwrap();

        let cancel = VideoImportCancellation::default();
        let trigger = cancel.clone();
        let cancel_thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            trigger.cancel();
        });
        let started = Instant::now();
        let result = run_analysis_pass(
            Path::new("ignored.mp4"),
            &VideoToolchain {
                ffmpeg: decoder,
                ffprobe: Path::new("ignored-ffprobe").to_path_buf(),
            },
            ProbeMetadata {
                duration_ms: 1_000,
                display_width: 384,
                display_height: 2,
                rotation_degrees: 0,
            },
            384 * 2,
            cancel,
            |_, _| {},
        );
        cancel_thread.join().unwrap();

        assert_eq!(result.unwrap_err().category(), "cancelled");
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
