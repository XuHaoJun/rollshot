use std::io::{self, Read};
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::{VideoImportCancellation, VideoImportError};

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
