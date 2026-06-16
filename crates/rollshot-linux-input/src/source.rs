//! `EvdevInputSource`: read-only evdev observation behind the
//! `SemanticInputSource` trait. Discovery, reader threads, and event reads are
//! Linux-only; on other hosts `start` returns `DegradedReason::SourceStartFailed`
//! so the crate compiles in the full workspace build. Reader threads and
//! start/permission paths are manually verified (spec §Manual Verification).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use rollshot_action::{
    CaptureRegion, DegradedReason, InputCapability, SemanticInputSource, TimedSemanticAction,
};

const TARGET: &str = "rollshot::action::linux_input";

/// Hard cap on buffered actions, so a stalled consumer cannot grow memory
/// without bound. Drop-oldest preserves recency (spec: explicit fixed bounds).
// `push`/`MAX_QUEUED` are exercised by the Linux reader threads and the unit
// test; on non-Linux hosts the plain lib build sees neither, so allow dead code.
#[allow(dead_code)]
const MAX_QUEUED: usize = 4096;

#[derive(Default)]
struct Shared {
    queue: Mutex<std::collections::VecDeque<TimedSemanticAction>>,
}

impl Shared {
    #[allow(dead_code)]
    fn push(&self, ev: TimedSemanticAction) {
        if let Ok(mut q) = self.queue.lock() {
            if q.len() >= MAX_QUEUED {
                q.pop_front();
            }
            q.push_back(ev);
        }
    }
}

pub struct EvdevInputSource {
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    readers: Vec<JoinHandle<()>>,
    started_at: Option<Instant>,
}

impl EvdevInputSource {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared::default()),
            stop: Arc::new(AtomicBool::new(false)),
            readers: Vec::new(),
            started_at: None,
        }
    }
}

impl Default for EvdevInputSource {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticInputSource for EvdevInputSource {
    fn start(&mut self, region: CaptureRegion) -> Result<InputCapability, DegradedReason> {
        self.started_at = Some(Instant::now());
        self.start_platform(region)
    }

    fn poll(&mut self) -> Vec<TimedSemanticAction> {
        match self.shared.queue.lock() {
            Ok(mut q) => q.drain(..).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for handle in self.readers.drain(..) {
            let _ = handle.join();
        }
        tracing::debug!(target: TARGET, "evdev source stopped");
    }
}

impl Drop for EvdevInputSource {
    /// Stop observing if the caller dropped the source without calling `stop`
    /// (e.g. a panic between `start` and `stop`). This keeps the spec's "input
    /// observed only between start and stop" guarantee true by construction,
    /// not just on the happy path. `stop` is idempotent.
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(not(target_os = "linux"))]
impl EvdevInputSource {
    fn start_platform(
        &mut self,
        _region: CaptureRegion,
    ) -> Result<InputCapability, DegradedReason> {
        tracing::debug!(target: TARGET, "evdev unavailable on this platform");
        Err(DegradedReason::SourceStartFailed)
    }
}

#[cfg(target_os = "linux")]
impl EvdevInputSource {
    fn start_platform(
        &mut self,
        _region: CaptureRegion,
    ) -> Result<InputCapability, DegradedReason> {
        use crate::classify::EvdevClassifier;
        use nix::fcntl::{fcntl, FcntlArg, OFlag};
        use std::os::fd::AsRawFd;

        let devices: Vec<evdev::Device> = evdev::enumerate().map(|(_path, dev)| dev).collect();

        if devices.is_empty() {
            if std::path::Path::new("/dev/input")
                .read_dir()
                .is_ok_and(|mut d| d.next().is_some())
            {
                tracing::warn!(target: TARGET, "no readable input devices; ACL likely missing");
                return Err(DegradedReason::PermissionDenied);
            }
            tracing::warn!(target: TARGET, "no input devices present");
            return Err(DegradedReason::NoInputDevice);
        }

        let started_at = self
            .started_at
            .expect("start_platform called after stamping start");
        for mut device in devices {
            let fd = device.as_raw_fd();
            let nonblocking_ok = fcntl(fd, FcntlArg::F_GETFL)
                .map(|flags| {
                    fcntl(
                        fd,
                        FcntlArg::F_SETFL(OFlag::from_bits_retain(flags) | OFlag::O_NONBLOCK),
                    )
                    .is_ok()
                })
                .unwrap_or(false);
            if !nonblocking_ok {
                tracing::warn!(target: TARGET, "could not set device non-blocking; skipping");
                continue;
            }
            let shared = Arc::clone(&self.shared);
            let stop = Arc::clone(&self.stop);
            let handle = std::thread::Builder::new()
                .name("rollshot-evdev-reader".into())
                .spawn(move || {
                    let mut classifier = EvdevClassifier::new();
                    while !stop.load(Ordering::Relaxed) {
                        match device.fetch_events() {
                            Ok(events) => {
                                for ev in events {
                                    let raw = crate::source::reduce_event(&ev);
                                    if let Some(action) = classifier.classify(raw) {
                                        let at_ms = started_at.elapsed().as_millis() as u64;
                                        shared.push(TimedSemanticAction { action, at_ms });
                                    }
                                }
                            }
                            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(std::time::Duration::from_millis(15));
                            }
                            Err(err) => {
                                tracing::warn!(target: TARGET, error = %err, "evdev reader stopped");
                                break;
                            }
                        }
                    }
                })
                .map_err(|_| DegradedReason::SourceStartFailed)?;
            self.readers.push(handle);
        }

        if self.readers.is_empty() {
            tracing::warn!(target: TARGET, "no evdev readers could be started");
            return Err(DegradedReason::SourceStartFailed);
        }

        tracing::info!(target: TARGET, readers = self.readers.len(), "evdev source started");
        Ok(InputCapability::SemanticEvents)
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn reduce_event(ev: &evdev::InputEvent) -> crate::classify::RawEvdevEvent {
    crate::classify::RawEvdevEvent {
        ev_type: ev.event_type().0,
        code: ev.code(),
        value: ev.value(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_os = "linux"))]
    use rollshot_action::CaptureRegion;
    use rollshot_action::SemanticInputSource;

    #[cfg(not(target_os = "linux"))]
    fn region() -> CaptureRegion {
        CaptureRegion {
            x: 0,
            y: 0,
            width: 100,
            height: 80,
        }
    }

    #[test]
    fn unstarted_source_polls_empty_and_stops_cleanly() {
        let mut src = EvdevInputSource::new();
        assert!(src.poll().is_empty());
        src.stop(); // no-op before start must not panic
        assert!(src.poll().is_empty());
    }

    #[test]
    fn source_is_send_and_object_safe() {
        fn assert_send<T: Send>() {}
        assert_send::<EvdevInputSource>();
        let _boxed: Box<dyn SemanticInputSource> = Box::new(EvdevInputSource::new());
    }

    #[test]
    fn shared_queue_is_bounded_and_drops_oldest() {
        let shared = Shared::default();
        for i in 0..(MAX_QUEUED as u64 + 10) {
            shared.push(rollshot_action::TimedSemanticAction {
                action: rollshot_action::SemanticAction::TypingActivity,
                at_ms: i,
            });
        }
        let q = shared.queue.lock().unwrap();
        assert_eq!(q.len(), MAX_QUEUED);
        assert_eq!(q.front().unwrap().at_ms, 10, "the 10 oldest are dropped");
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn non_linux_start_degrades_to_source_start_failed() {
        let mut src = EvdevInputSource::new();
        let err = src.start(region()).unwrap_err();
        assert_eq!(err, rollshot_action::DegradedReason::SourceStartFailed);
    }
}
