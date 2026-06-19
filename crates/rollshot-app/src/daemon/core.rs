use std::sync::mpsc::Sender;
use std::time::Duration;

const QUIT_GRACE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureId(pub u64);

#[derive(Debug)]
pub enum DaemonEvent {
    CaptureRegion,
    CaptureExited { id: CaptureId, success: bool },
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopAction {
    Continue,
    Exit,
}

pub trait ActiveCapture: Send {
    fn terminate(&mut self, grace: Duration) -> Result<(), String>;
}

pub trait CaptureLauncher {
    fn launch(
        &mut self,
        id: CaptureId,
        events: Sender<DaemonEvent>,
    ) -> Result<Box<dyn ActiveCapture>, String>;
}

struct RunningCapture {
    id: CaptureId,
    process: Box<dyn ActiveCapture>,
}

/// Product state, independent of tray and portal implementations.
///
/// ```text
/// Idle --CaptureRegion--> Capturing --CaptureExited(current id)--> Idle
///   \                         |
///    +-------- Quit ----------+-------------------------------> Exit
/// ```
///
/// A stale `CaptureExited` cannot clear a newer capture because every launch
/// receives a monotonically increasing `CaptureId`.
pub struct DaemonCore<L: CaptureLauncher> {
    launcher: L,
    events: Sender<DaemonEvent>,
    active: Option<RunningCapture>,
    next_id: u64,
}

impl<L: CaptureLauncher> DaemonCore<L> {
    pub fn new(launcher: L, events: Sender<DaemonEvent>) -> Self {
        Self {
            launcher,
            events,
            active: None,
            next_id: 1,
        }
    }

    pub fn is_capturing(&self) -> bool {
        self.active.is_some()
    }

    pub fn handle(&mut self, event: DaemonEvent) -> LoopAction {
        match event {
            DaemonEvent::CaptureRegion if self.active.is_none() => {
                let id = CaptureId(self.next_id);
                self.next_id += 1;
                match self.launcher.launch(id, self.events.clone()) {
                    Ok(process) => {
                        self.active = Some(RunningCapture { id, process });
                    }
                    Err(error) => {
                        tracing::error!(
                            target: "rollshot::daemon::process",
                            %error,
                            "failed to start capture child"
                        );
                    }
                }
                LoopAction::Continue
            }
            DaemonEvent::CaptureRegion => {
                tracing::debug!(
                    target: "rollshot::daemon::core",
                    "capture trigger ignored while capture is active"
                );
                LoopAction::Continue
            }
            DaemonEvent::CaptureExited { id, success } => {
                if self.active.as_ref().is_some_and(|active| active.id == id) {
                    self.active = None;
                    tracing::info!(
                        target: "rollshot::daemon::process",
                        capture_id = id.0,
                        success,
                        "capture child exited"
                    );
                }
                LoopAction::Continue
            }
            DaemonEvent::Quit => {
                if let Some(mut active) = self.active.take() {
                    if let Err(error) = active.process.terminate(QUIT_GRACE) {
                        tracing::warn!(
                            target: "rollshot::daemon::process",
                            %error,
                            "capture child cleanup failed"
                        );
                    }
                }
                LoopAction::Exit
            }
        }
    }
}

impl<L: CaptureLauncher> Drop for DaemonCore<L> {
    fn drop(&mut self) {
        if let Some(mut active) = self.active.take() {
            if let Err(error) = active.process.terminate(QUIT_GRACE) {
                tracing::warn!(
                    target: "rollshot::daemon::process",
                    %error,
                    "capture child cleanup failed while daemon core dropped"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct FakeState {
        launches: usize,
        terminations: usize,
    }

    struct FakeLauncher(Arc<Mutex<FakeState>>);
    struct FakeCapture(Arc<Mutex<FakeState>>);

    impl CaptureLauncher for FakeLauncher {
        fn launch(
            &mut self,
            _id: CaptureId,
            _events: Sender<DaemonEvent>,
        ) -> Result<Box<dyn ActiveCapture>, String> {
            self.0.lock().unwrap().launches += 1;
            Ok(Box::new(FakeCapture(self.0.clone())))
        }
    }

    impl ActiveCapture for FakeCapture {
        fn terminate(&mut self, _grace: Duration) -> Result<(), String> {
            self.0.lock().unwrap().terminations += 1;
            Ok(())
        }
    }

    fn core() -> (DaemonCore<FakeLauncher>, Arc<Mutex<FakeState>>) {
        let state = Arc::new(Mutex::new(FakeState::default()));
        let (events, _receiver) = std::sync::mpsc::channel();
        (DaemonCore::new(FakeLauncher(state.clone()), events), state)
    }

    #[test]
    fn idle_capture_event_launches_one_child() {
        let (mut core, state) = core();
        assert_eq!(
            core.handle(DaemonEvent::CaptureRegion),
            LoopAction::Continue
        );
        assert_eq!(state.lock().unwrap().launches, 1);
        assert!(core.is_capturing());
    }

    #[test]
    fn trigger_while_capturing_is_ignored() {
        let (mut core, state) = core();
        core.handle(DaemonEvent::CaptureRegion);
        core.handle(DaemonEvent::CaptureRegion);
        assert_eq!(state.lock().unwrap().launches, 1);
    }

    #[test]
    fn matching_child_exit_returns_to_idle() {
        let (mut core, _) = core();
        core.handle(DaemonEvent::CaptureRegion);
        core.handle(DaemonEvent::CaptureExited {
            id: CaptureId(1),
            success: true,
        });
        assert!(!core.is_capturing());
    }

    #[test]
    fn nonzero_child_exit_also_returns_to_idle() {
        let (mut core, _) = core();
        core.handle(DaemonEvent::CaptureRegion);
        core.handle(DaemonEvent::CaptureExited {
            id: CaptureId(1),
            success: false,
        });
        assert!(!core.is_capturing());
    }

    #[test]
    fn stale_child_exit_does_not_clear_current_capture() {
        let (mut core, _) = core();
        core.handle(DaemonEvent::CaptureRegion);
        core.handle(DaemonEvent::CaptureExited {
            id: CaptureId(99),
            success: true,
        });
        assert!(core.is_capturing());
    }

    #[test]
    fn quit_terminates_active_capture_and_exits() {
        let (mut core, state) = core();
        core.handle(DaemonEvent::CaptureRegion);
        assert_eq!(core.handle(DaemonEvent::Quit), LoopAction::Exit);
        assert_eq!(state.lock().unwrap().terminations, 1);
    }

    #[test]
    fn dropping_core_terminates_active_capture() {
        let (mut core, state) = core();
        core.handle(DaemonEvent::CaptureRegion);
        drop(core);
        assert_eq!(state.lock().unwrap().terminations, 1);
    }

    struct FailingLauncher;

    impl CaptureLauncher for FailingLauncher {
        fn launch(
            &mut self,
            _id: CaptureId,
            _events: Sender<DaemonEvent>,
        ) -> Result<Box<dyn ActiveCapture>, String> {
            Err("spawn failed".into())
        }
    }

    #[test]
    fn spawn_failure_leaves_core_idle() {
        let (events, _receiver) = std::sync::mpsc::channel();
        let mut core = DaemonCore::new(FailingLauncher, events);

        assert_eq!(
            core.handle(DaemonEvent::CaptureRegion),
            LoopAction::Continue
        );
        assert!(!core.is_capturing());
    }
}
