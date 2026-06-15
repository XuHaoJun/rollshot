//! `MacosInputSource`: a listen-only `CGEventTap` on a dedicated CFRunLoop
//! thread, behind the `SemanticInputSource` trait. All CoreGraphics /
//! CoreFoundation FFI is isolated here with `// SAFETY` notes; the public API is
//! safe. On non-macOS hosts `start` returns `DegradedReason::SourceStartFailed`.
//! Reimplements the CrossMacro approach (GPLv3 learning reference) without
//! copying its source.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use rollshot_action::{
    CaptureRegion, DegradedReason, InputCapability, SemanticInputSource, TimedSemanticAction,
};

const TARGET: &str = "rollshot::action::macos_input";

/// Hard cap on buffered actions (drop-oldest), mirroring the Linux source so a
/// stalled consumer cannot grow memory without bound.
#[allow(dead_code)]
const MAX_QUEUED: usize = 4096;

#[derive(Default)]
#[allow(dead_code)]
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

pub struct MacosInputSource {
    shared: Arc<Shared>,
    started_at: Option<Instant>,
    #[cfg(target_os = "macos")]
    runloop: Option<macos::RunLoopHandle>,
    thread: Option<JoinHandle<()>>,
}

impl MacosInputSource {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared::default()),
            started_at: None,
            #[cfg(target_os = "macos")]
            runloop: None,
            thread: None,
        }
    }
}

impl Default for MacosInputSource {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticInputSource for MacosInputSource {
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
        self.stop_platform();
    }
}

#[cfg(not(target_os = "macos"))]
impl MacosInputSource {
    fn start_platform(
        &mut self,
        _region: CaptureRegion,
    ) -> Result<InputCapability, DegradedReason> {
        tracing::debug!(target: TARGET, "CGEventTap unavailable on this platform");
        Err(DegradedReason::SourceStartFailed)
    }

    fn stop_platform(&mut self) {
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(target_os = "macos")]
impl MacosInputSource {
    fn start_platform(
        &mut self,
        _region: CaptureRegion,
    ) -> Result<InputCapability, DegradedReason> {
        // Permission gate: Input Monitoring is required for a listen-only tap.
        if !matches!(
            crate::permission::input_monitoring_status(),
            crate::permission::InputMonitoringStatus::Granted
        ) {
            // Prompt once; if still not granted, degrade to visual-only.
            if !matches!(
                crate::permission::request_input_monitoring(),
                crate::permission::InputMonitoringStatus::Granted
            ) {
                tracing::warn!(target: TARGET, "Input Monitoring not granted");
                return Err(DegradedReason::PermissionDenied);
            }
        }

        let shared = Arc::clone(&self.shared);
        let started_at = self
            .started_at
            .expect("start_platform after stamping start");
        let (tx, rx) = std::sync::mpsc::channel::<Result<macos::RunLoopHandle, DegradedReason>>();

        let handle = std::thread::Builder::new()
            .name("rollshot-cgtap".into())
            .spawn(move || macos::run_tap_thread(shared, started_at, tx))
            .map_err(|_| DegradedReason::SourceStartFailed)?;
        self.thread = Some(handle);

        // The thread reports tap-creation success/failure before running the
        // loop, so `start` returns a definite capability.
        match rx.recv() {
            Ok(Ok(runloop)) => {
                self.runloop = Some(runloop);
                tracing::info!(target: TARGET, "CGEventTap started");
                Ok(InputCapability::SemanticEvents)
            }
            Ok(Err(reason)) => {
                if let Some(handle) = self.thread.take() {
                    let _ = handle.join();
                }
                Err(reason)
            }
            Err(_) => Err(DegradedReason::SourceStartFailed),
        }
    }

    fn stop_platform(&mut self) {
        if let Some(runloop) = self.runloop.take() {
            // SAFETY: `runloop` is the run loop the tap thread is blocked in;
            // stopping it unblocks `CFRunLoopRun` so the thread exits and frees
            // its tap/source. The handle is only ever used here, once.
            unsafe { runloop.stop() };
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
        tracing::debug!(target: TARGET, "CGEventTap stopped");
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ptr::NonNull;
    use std::sync::mpsc::Sender;
    use std::sync::Arc;
    use std::time::Instant;

    use rollshot_action::{DegradedReason, TimedSemanticAction};

    use objc2_core_foundation::{CFMachPort, CFRetained, CFRunLoop, CFRunLoopSource};
    use objc2_core_graphics::{
        CGEvent, CGEventField, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
        CGEventType,
    };

    use super::{Shared, TARGET};

    /// A `Send` handle to the tap thread's run loop, used only to stop it.
    pub struct RunLoopHandle(CFRetained<CFRunLoop>);
    // SAFETY: `CFRunLoop` is internally thread-safe for `CFRunLoopStop`, which
    // is the only operation performed through this handle, from `stop_platform`
    // on the owning thread after `start` returned. We never deref it elsewhere.
    unsafe impl Send for RunLoopHandle {}

    impl RunLoopHandle {
        /// # Safety
        /// Caller guarantees the referenced run loop is the tap thread's loop.
        pub unsafe fn stop(&self) {
            self.0.stop();
        }
    }

    /// Context passed to the C callback via the tap's `user_info` pointer.
    /// `tap` is set once, right after creation, so the callback can re-enable
    /// the tap on `TapDisabledByTimeout`. `CallbackCtx` is created and accessed
    /// only on the tap thread, so a single-threaded `OnceCell` is sufficient.
    struct CallbackCtx {
        shared: Arc<Shared>,
        started_at: Instant,
        tap: std::cell::OnceCell<CFRetained<CFMachPort>>,
    }

    /// Reduce a live `CGEvent` to the pure-core `RawCgEvent`.
    ///
    /// # Safety
    /// `event` must be a valid `CGEvent` for the duration of this call (it is,
    /// inside the tap callback).
    unsafe fn reduce(kind: RawCgKind, event: NonNull<CGEvent>) -> crate::classify::RawCgEvent {
        let event_ref = event.as_ref();
        let button_number = if matches!(kind, RawCgKind::OtherMouseDown) {
            CGEvent::integer_value_field(Some(event_ref), CGEventField::MouseEventButtonNumber)
        } else {
            0
        };
        let keycode = if matches!(kind, RawCgKind::KeyDown) {
            CGEvent::integer_value_field(Some(event_ref), CGEventField::KeyboardEventKeycode)
        } else {
            0
        };
        crate::classify::RawCgEvent {
            kind,
            button_number,
            keycode,
        }
    }

    fn kind_of(event_type: CGEventType) -> RawCgKind {
        match event_type {
            CGEventType::LeftMouseDown => RawCgKind::LeftMouseDown,
            CGEventType::RightMouseDown => RawCgKind::RightMouseDown,
            CGEventType::OtherMouseDown => RawCgKind::OtherMouseDown,
            CGEventType::ScrollWheel => RawCgKind::ScrollWheel,
            CGEventType::KeyDown => RawCgKind::KeyDown,
            _ => RawCgKind::Other,
        }
    }

    /// The tap callback. Listen-only, so the returned event pointer is ignored
    /// by the system; we return it unchanged. The body is kept panic-free.
    ///
    /// # Safety
    /// `user_info` is the `*mut CallbackCtx` we passed to `CGEventTapCreate`,
    /// valid for the tap's lifetime; `event` is a live `CGEvent`.
    unsafe extern "C-unwind" fn tap_callback(
        _proxy: objc2_core_graphics::CGEventTapProxy,
        event_type: CGEventType,
        event: NonNull<CGEvent>,
        user_info: *mut std::ffi::c_void,
    ) -> *mut CGEvent {
        let ctx = &*(user_info as *const CallbackCtx);

        // Re-enable after an inactivity timeout (the OS disables the tap). The
        // spec requires this; the tap handle was stored into ctx after creation.
        if matches!(event_type, CGEventType::TapDisabledByTimeout) {
            if let Some(tap) = ctx.tap.get() {
                // SAFETY: `tap` is the live mach port for this very tap.
                CGEvent::tap_enable(tap, true);
            }
            tracing::debug!(target: TARGET, "tap re-enabled after timeout");
            return event.as_ptr();
        }

        let kind = kind_of(event_type);
        if !matches!(kind, RawCgKind::Other) {
            let raw = reduce(kind, event);
            if let Some(action) = crate::classify::classify_cg(raw) {
                let at_ms = ctx.started_at.elapsed().as_millis() as u64;
                ctx.shared.push(TimedSemanticAction { action, at_ms });
            }
        }
        event.as_ptr()
    }

    /// Event mask: mouse downs, scroll, key down. (Key up / flags-changed are
    /// intentionally excluded — they classify to nothing.)
    fn event_mask() -> u64 {
        let bit = |t: CGEventType| 1u64 << (t.0 as u64);
        bit(CGEventType::LeftMouseDown)
            | bit(CGEventType::RightMouseDown)
            | bit(CGEventType::OtherMouseDown)
            | bit(CGEventType::ScrollWheel)
            | bit(CGEventType::KeyDown)
    }

    /// Owns the run loop for the session. Creates the tap, reports
    /// success/failure through `tx`, then blocks in `CFRunLoopRun` until
    /// `RunLoopHandle::stop` is called.
    pub fn run_tap_thread(
        shared: Arc<Shared>,
        started_at: Instant,
        tx: Sender<Result<RunLoopHandle, DegradedReason>>,
    ) {
        let ctx = Box::into_raw(Box::new(CallbackCtx {
            shared,
            started_at,
            tap: std::cell::OnceCell::new(),
        }));

        // SAFETY: standard CGEventTapCreate for a listen-only HID tap. `ctx` is
        // a valid pointer for the tap's lifetime; we free it after the loop.
        let tap = unsafe {
            CGEvent::tap_create(
                CGEventTapLocation::HIDEventTap,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                event_mask(),
                Some(tap_callback),
                ctx as *mut std::ffi::c_void,
            )
        };
        let Some(tap) = tap else {
            // Null tap: free ctx, report failure, do not run a loop.
            // SAFETY: ctx came from Box::into_raw and is not used after this.
            unsafe { drop(Box::from_raw(ctx)) };
            let _ = tx.send(Err(DegradedReason::SourceStartFailed));
            return;
        };

        // Give the callback a handle to its own tap so it can re-enable on
        // timeout. SAFETY: ctx is live; we are still the only thread touching it
        // (the run loop has not started yet).
        unsafe {
            let _ = (*ctx).tap.set(tap.clone());
        }

        // SAFETY: create a run-loop source from the tap mach port and add it to
        // this thread's run loop in common modes, then enable the tap.
        let run_loop = unsafe {
            let source = CFMachPort::new_run_loop_source(None, Some(&tap), 0)
                .expect("run loop source from tap mach port");
            let run_loop = CFRunLoop::current().expect("current run loop");
            run_loop.add_source(Some(&source), objc2_core_foundation::kCFRunLoopCommonModes);
            CGEvent::tap_enable(&tap, true);
            keep_alive(source); // keep the source retained for the loop's life
            run_loop
        };

        if tx.send(Ok(RunLoopHandle(run_loop.clone()))).is_err() {
            return;
        }

        // SAFETY: blocks until CFRunLoopStop is called via RunLoopHandle::stop.
        unsafe { CFRunLoop::run() };

        // Teardown: disable the tap and free the callback context.
        // SAFETY: tap is still valid; ctx came from Box::into_raw.
        unsafe {
            CGEvent::tap_enable(&tap, false);
            drop(Box::from_raw(ctx));
        }
        tracing::debug!(target: TARGET, "tap thread exited");
    }

    /// Hold a retained CF object alive until the thread ends. The run-loop
    /// source must outlive the loop; leaking it for the session is acceptable
    /// because the thread (and process intent) is short-lived per recording.
    fn keep_alive(source: CFRetained<CFRunLoopSource>) {
        std::mem::forget(source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_os = "macos"))]
    use rollshot_action::CaptureRegion;
    use rollshot_action::SemanticInputSource;

    #[cfg(not(target_os = "macos"))]
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
        let mut src = MacosInputSource::new();
        assert!(src.poll().is_empty());
        src.stop(); // no-op before start must not panic
        assert!(src.poll().is_empty());
    }

    #[test]
    fn source_is_send_and_object_safe() {
        fn assert_send<T: Send>() {}
        assert_send::<MacosInputSource>();
        let _boxed: Box<dyn SemanticInputSource> = Box::new(MacosInputSource::new());
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

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_macos_start_degrades_to_source_start_failed() {
        let mut src = MacosInputSource::new();
        assert_eq!(
            src.start(region()).unwrap_err(),
            rollshot_action::DegradedReason::SourceStartFailed
        );
    }
}
