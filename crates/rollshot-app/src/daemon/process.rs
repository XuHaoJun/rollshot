use crate::daemon::core::{ActiveCapture, CaptureId, CaptureLauncher, DaemonEvent};
use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::sync::{mpsc::Sender, Arc, Condvar, Mutex};
use std::time::Duration;

pub struct CurrentExeLauncher {
    executable: std::path::PathBuf,
}

pub(crate) struct ProcessGroupCapture {
    pgid: Pid,
    completed: Arc<(Mutex<bool>, Condvar)>,
}

pub(crate) fn capture_args() -> [&'static str; 5] {
    ["capture", "--workflow", "screenshot", "--scope", "region"]
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
        events: Sender<DaemonEvent>,
    ) -> Result<Box<dyn ActiveCapture>, String> {
        let mut command = Command::new(&self.executable);
        command.args(capture_args()).process_group(0);
        let child = command
            .spawn()
            .map_err(|error| format!("failed to spawn capture child: {error}"))?;
        let pgid = Pid::from_raw(child.id() as i32);
        let completed = Arc::new((Mutex::new(false), Condvar::new()));
        spawn_watcher(id, child, completed.clone(), events);
        Ok(Box::new(ProcessGroupCapture { pgid, completed }))
    }
}

pub(crate) fn spawn_watcher(
    id: CaptureId,
    mut child: Child,
    completed: Arc<(Mutex<bool>, Condvar)>,
    events: Sender<DaemonEvent>,
) {
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
        let (lock, condition) = &*completed;
        *lock.lock().unwrap() = true;
        condition.notify_all();
        let _ = events.send(DaemonEvent::CaptureExited { id, success });
    });
}

impl ActiveCapture for ProcessGroupCapture {
    fn terminate(&mut self, grace: Duration) -> Result<(), String> {
        terminate_with(&self.completed, grace, |signal| {
            match killpg(self.pgid, signal) {
                Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
                Err(error) => Err(format!(
                    "failed to signal capture process group with {signal:?}: {error}"
                )),
            }
        })
    }
}

pub(crate) fn terminate_with(
    completed: &Arc<(Mutex<bool>, Condvar)>,
    grace: Duration,
    mut signal: impl FnMut(Signal) -> Result<(), String>,
) -> Result<(), String> {
    let (lock, condition) = &**completed;
    if *lock.lock().unwrap() {
        return Ok(());
    }
    signal(Signal::SIGTERM)?;
    let completed = lock.lock().unwrap();
    let (completed, _) = condition
        .wait_timeout_while(completed, grace, |completed| !*completed)
        .map_err(|_| "capture completion lock was poisoned".to_string())?;
    if *completed {
        return Ok(());
    }
    drop(completed);

    signal(Signal::SIGKILL)?;
    let (completed, _) = condition
        .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(1), |completed| {
            !*completed
        })
        .map_err(|_| "capture completion lock was poisoned".to_string())?;
    if *completed {
        Ok(())
    } else {
        Err("capture process group did not exit after SIGKILL".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_arguments_are_region_screenshot() {
        assert_eq!(
            capture_args(),
            ["capture", "--workflow", "screenshot", "--scope", "region"]
        );
    }

    #[test]
    fn watcher_reports_exit_for_matching_capture_id() {
        let (tx, rx) = std::sync::mpsc::channel();
        let child = std::process::Command::new("sh")
            .arg("-c")
            .arg("exit 0")
            .spawn()
            .unwrap();
        let completed =
            std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        spawn_watcher(CaptureId(7), child, completed, tx);

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
        let completed =
            std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let (tx, _rx) = std::sync::mpsc::channel();
        spawn_watcher(CaptureId(9), child, completed.clone(), tx);
        let mut capture = ProcessGroupCapture { pgid, completed };

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
    fn graceful_completion_needs_only_sigterm() {
        let completed =
            std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let notify = completed.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(10));
            let (lock, condition) = &*notify;
            *lock.lock().unwrap() = true;
            condition.notify_all();
        });
        let signals = std::sync::Mutex::new(Vec::new());

        terminate_with(&completed, std::time::Duration::from_secs(1), |signal| {
            signals.lock().unwrap().push(signal);
            Ok(())
        })
        .unwrap();

        assert_eq!(*signals.lock().unwrap(), vec![Signal::SIGTERM]);
    }

    #[test]
    fn timeout_escalates_to_sigkill() {
        let completed =
            std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let signals = std::sync::Mutex::new(Vec::new());

        terminate_with(&completed, std::time::Duration::from_millis(1), |signal| {
            signals.lock().unwrap().push(signal);
            if signal == Signal::SIGKILL {
                let (lock, condition) = &*completed;
                *lock.lock().unwrap() = true;
                condition.notify_all();
            }
            Ok(())
        })
        .unwrap();

        assert_eq!(
            *signals.lock().unwrap(),
            vec![Signal::SIGTERM, Signal::SIGKILL]
        );
    }
}
