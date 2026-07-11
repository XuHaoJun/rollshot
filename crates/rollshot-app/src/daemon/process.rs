use crate::daemon::core::{ActiveCapture, CaptureId, CaptureKind, CaptureLauncher, DaemonEvent};
use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

pub struct CurrentExeLauncher {
    executable: std::path::PathBuf,
}

pub(crate) struct ProcessGroupCapture {
    pgid: Pid,
}

pub(crate) fn capture_args(kind: CaptureKind) -> &'static [&'static str] {
    match kind {
        CaptureKind::Region => &["capture", "--workflow", "screenshot", "--scope", "region"],
        CaptureKind::Text => &["ocr", "--graphical-feedback"],
    }
}

impl CurrentExeLauncher {
    pub fn new(executable: std::path::PathBuf) -> Self {
        Self { executable }
    }
}

impl CaptureLauncher for CurrentExeLauncher {
    fn launch(
        &mut self,
        id: CaptureId,
        kind: CaptureKind,
        events: Sender<DaemonEvent>,
    ) -> Result<Box<dyn ActiveCapture>, String> {
        let mut command = Command::new(&self.executable);
        command.args(capture_args(kind)).process_group(0);
        let child = command
            .spawn()
            .map_err(|error| format!("failed to spawn capture child: {error}"))?;
        let pgid = Pid::from_raw(child.id() as i32);
        spawn_watcher(id, child, events);
        Ok(Box::new(ProcessGroupCapture { pgid }))
    }
}

pub(crate) fn spawn_watcher(id: CaptureId, mut child: Child, events: Sender<DaemonEvent>) {
    std::thread::spawn(move || {
        let success = match child.wait() {
            Ok(status) => status.success(),
            Err(error) => {
                tracing::warn!(
                    target: "rollshot::daemon::process",
                    %error,
                    "failed to wait for capture child"
                );
                false
            }
        };
        let _ = events.send(DaemonEvent::CaptureExited { id, success });
    });
}

impl ActiveCapture for ProcessGroupCapture {
    fn terminate(&mut self, grace: Duration) -> Result<(), String> {
        terminate_with(
            grace,
            |signal| match killpg(self.pgid, signal) {
                Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
                Err(error) => Err(format!(
                    "failed to signal capture process group with {signal:?}: {error}"
                )),
            },
            || process_group_exists(killpg(self.pgid, None)),
        )
    }
}

fn process_group_exists(probe: Result<(), nix::errno::Errno>) -> Result<bool, String> {
    match probe {
        Ok(()) | Err(nix::errno::Errno::EPERM) => Ok(true),
        Err(nix::errno::Errno::ESRCH) => Ok(false),
        Err(error) => Err(format!("failed to inspect capture process group: {error}")),
    }
}

pub(crate) fn terminate_with(
    grace: Duration,
    mut signal: impl FnMut(Signal) -> Result<(), String>,
    mut group_exists: impl FnMut() -> Result<bool, String>,
) -> Result<(), String> {
    if !group_exists()? {
        return Ok(());
    }
    signal(Signal::SIGTERM)?;
    if wait_for_group_exit(grace, &mut group_exists)? {
        return Ok(());
    }

    signal(Signal::SIGKILL)?;
    if wait_for_group_exit(Duration::from_secs(1), &mut group_exists)? {
        Ok(())
    } else {
        Err("capture process group did not exit after SIGKILL".into())
    }
}

fn wait_for_group_exit(
    timeout: Duration,
    group_exists: &mut impl FnMut() -> Result<bool, String>,
) -> Result<bool, String> {
    let deadline = Instant::now() + timeout;
    while group_exists()? {
        let now = Instant::now();
        if now >= deadline {
            return Ok(false);
        }
        std::thread::sleep((deadline - now).min(Duration::from_millis(10)));
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_arguments_are_region_screenshot() {
        assert_eq!(
            capture_args(CaptureKind::Region),
            &["capture", "--workflow", "screenshot", "--scope", "region"]
        );
    }

    #[test]
    fn capture_text_arguments_are_ocr_graphical_feedback() {
        assert_eq!(
            capture_args(CaptureKind::Text),
            &["ocr", "--graphical-feedback"]
        );
    }

    #[test]
    fn permission_denied_probe_still_means_process_group_exists() {
        assert!(process_group_exists(Err(nix::errno::Errno::EPERM)).unwrap());
    }

    #[test]
    fn watcher_reports_exit_for_matching_capture_id() {
        let (tx, rx) = std::sync::mpsc::channel();
        let child = std::process::Command::new("sh")
            .arg("-c")
            .arg("exit 0")
            .spawn()
            .unwrap();
        spawn_watcher(CaptureId(7), child, tx);

        assert!(matches!(
            rx.recv().unwrap(),
            DaemonEvent::CaptureExited {
                id: CaptureId(7),
                success: true
            }
        ));
    }

    #[test]
    fn termination_reaches_descendant_processes_in_the_group() {
        use std::os::unix::process::CommandExt;

        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("descendant.pid");
        let mut command = std::process::Command::new("sh");
        command
            .env("ROLLSHOT_TEST_PID_FILE", &pid_file)
            .arg("-c")
            .arg("sleep 60 & echo $! > \"$ROLLSHOT_TEST_PID_FILE\"; wait")
            .process_group(0);
        let child = command.spawn().unwrap();
        let pgid = Pid::from_raw(child.id() as i32);
        let (tx, _rx) = std::sync::mpsc::channel();
        spawn_watcher(CaptureId(9), child, tx);
        let mut capture = ProcessGroupCapture { pgid };

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !pid_file.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if !pid_file.exists() {
            let _ = capture.terminate(std::time::Duration::from_secs(1));
            panic!("descendant process did not publish its pid");
        }
        let descendant: i32 = match std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|text| text.trim().parse().ok())
        {
            Some(pid) => pid,
            None => {
                let _ = capture.terminate(std::time::Duration::from_secs(1));
                panic!("descendant pid file was invalid");
            }
        };

        capture
            .terminate(std::time::Duration::from_secs(2))
            .unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            match nix::sys::signal::kill(Pid::from_raw(descendant), None) {
                Err(nix::errno::Errno::ESRCH) => break,
                Ok(()) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                result => panic!("descendant process survived group shutdown: {result:?}"),
            }
        }
    }

    #[test]
    fn completed_leader_does_not_hide_surviving_process_group() {
        use std::os::unix::process::CommandExt;

        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("descendant.pid");
        let mut command = std::process::Command::new("sh");
        command
            .env("ROLLSHOT_TEST_PID_FILE", &pid_file)
            .arg("-c")
            .arg("trap '' TERM; sleep 60 & echo $! > \"$ROLLSHOT_TEST_PID_FILE\"; exit 0")
            .process_group(0);
        let child = command.spawn().unwrap();
        let pgid = Pid::from_raw(child.id() as i32);
        let (tx, rx) = std::sync::mpsc::channel();
        spawn_watcher(CaptureId(10), child, tx);
        let mut capture = ProcessGroupCapture { pgid };

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !pid_file.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if !pid_file.exists() {
            let _ = killpg(pgid, Signal::SIGKILL);
            panic!("descendant process did not publish its pid");
        }
        rx.recv_timeout(std::time::Duration::from_secs(2))
            .expect("group leader did not exit");

        capture
            .terminate(std::time::Duration::from_millis(10))
            .unwrap();

        let group_survived = killpg(pgid, None).is_ok();
        if group_survived {
            let _ = killpg(pgid, Signal::SIGKILL);
        }
        assert!(!group_survived, "process group survived daemon shutdown");
    }

    #[test]
    fn graceful_completion_needs_only_sigterm() {
        let signals = std::sync::Mutex::new(Vec::new());
        let checks = std::cell::Cell::new(0);

        terminate_with(
            std::time::Duration::from_secs(1),
            |signal| {
                signals.lock().unwrap().push(signal);
                Ok(())
            },
            || {
                checks.set(checks.get() + 1);
                Ok(checks.get() < 3)
            },
        )
        .unwrap();

        assert_eq!(*signals.lock().unwrap(), vec![Signal::SIGTERM]);
    }

    #[test]
    fn timeout_escalates_to_sigkill() {
        let signals = std::sync::Mutex::new(Vec::new());
        let killed = std::cell::Cell::new(false);

        terminate_with(
            std::time::Duration::from_millis(1),
            |signal| {
                signals.lock().unwrap().push(signal);
                if signal == Signal::SIGKILL {
                    killed.set(true);
                }
                Ok(())
            },
            || Ok(!killed.get()),
        )
        .unwrap();

        assert_eq!(
            *signals.lock().unwrap(),
            vec![Signal::SIGTERM, Signal::SIGKILL]
        );
    }
}
