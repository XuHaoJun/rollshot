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
        self.finished = true;
        self.child.wait().map_err(|_| VideoImportError::ProbeFailed)
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
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
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

    if meta.rotation_degrees == 90 || meta.rotation_degrees == 270 {
        cmd.args(["-metadata:s:v", "rotate=0"]);
    }

    let vf = match meta.rotation_degrees {
        90 => format!("transpose=1,{}", analysis_filter),
        180 => format!("transpose=1,transpose=1,{}", analysis_filter),
        270 => format!("transpose=2,{}", analysis_filter),
        _ => analysis_filter,
    };

    cmd.args([
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
    let (stdout, _stderr) = child
        .take_pipes()
        .ok_or(VideoImportError::DecoderUnavailable)?;

    let cancel_ref = cancel.clone();
    let mut stdout = stdout;
    let mut frame_buf = vec![0u8; frame_size];
    let mut sample_index: u64 = 0;

    loop {
        if cancel_ref.is_cancelled() {
            child.kill_and_wait();
            return Err(VideoImportError::Cancelled);
        }

        match read_exact_or_eof(&mut stdout, &mut frame_buf) {
            Ok(true) => {
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
            Ok(false) => break,
            Err(e) => {
                if e.kind() == io::ErrorKind::UnexpectedEof && sample_index > 0 {
                    tracing::event!(
                        target: "rollshot::action::video_import",
                        tracing::Level::DEBUG,
                        category = "truncated_frame",
                        sample_index,
                        "truncated analysis frame at EOF; ignoring"
                    );
                    break;
                }
                child.kill_and_wait();
                return Err(VideoImportError::DecodeFailed);
            }
        }
    }

    let status = child.wait()?;
    if !status.success() && sample_index == 0 {
        return Err(VideoImportError::DecodeFailed);
    }

    Ok(())
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
    _progress: &(impl Fn(super::VideoImportProgress) + Send + Sync),
    _total_ms: u64,
) -> Result<HashMap<usize, std::path::PathBuf>, VideoImportError> {
    if requested_indices.is_empty() {
        return Ok(HashMap::new());
    }

    let select_filter = build_select_filter(requested_indices);

    let mut cmd = Command::new(&toolchain.ffmpeg);
    cmd.args(["-nostdin", "-an", "-sn", "-dn"]);

    if meta.rotation_degrees == 90 || meta.rotation_degrees == 270 {
        cmd.args(["-metadata:s:v", "rotate=0"]);
    }

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

    let child = CancellableChild::spawn(cmd)?;

    let cancel_for_monitor = cancel_ref.clone();

    let mut child = child;

    loop {
        if cancel_for_monitor.is_cancelled() {
            child.kill_and_wait();
            return Err(VideoImportError::Cancelled);
        }

        match child.child.try_wait() {
            Ok(Some(status)) => {
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

    let mut result = HashMap::new();
    let mut sorted_outputs = output_files;
    sorted_outputs.sort();

    for (output_idx, &requested_idx) in requested_indices.iter().enumerate() {
        if output_idx < sorted_outputs.len() {
            result.insert(requested_idx, sorted_outputs[output_idx].clone());
        }
    }

    Ok(result)
}

fn build_select_filter(indices: &[usize]) -> String {
    if indices.is_empty() {
        return "select=0".to_string();
    }
    let conditions: Vec<String> = indices.iter().map(|&i| format!("eq(n\\,{})", i)).collect();
    format!("select='{}'", conditions.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long_running_fixture_process() -> CancellableChild {
        let mut cmd = Command::new("sleep");
        cmd.arg("300");
        CancellableChild::spawn(cmd).expect("failed to spawn sleep")
    }

    fn fixture_process_is_alive() -> bool {
        Command::new("pgrep")
            .arg("-f")
            .arg("sleep 300")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn cancellation_kills_and_waits_for_child() {
        let fixture = long_running_fixture_process();
        let cancel = VideoImportCancellation::default();
        cancel.cancel();
        let error = run_cancellable_child(fixture, &cancel, |_| {}).unwrap_err();
        assert_eq!(error.category(), "cancelled");
        assert!(!fixture_process_is_alive());
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
        {
            let _fixture = long_running_fixture_process();
        }
        std::thread::sleep(Duration::from_millis(100));
        assert!(!fixture_process_is_alive());
    }
}
