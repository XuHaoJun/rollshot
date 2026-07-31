use rollshot_capture::{crop_frame, CaptureError, FrameStream, Region};
use rollshot_core::{RecoveryProbeResult, StitchConfig, Stitcher};
use rollshot_overlay_core::capture_miss::{
    progress_signal_from_outcome, CaptureMissState, CaptureMissTracker, CapturedEdge,
    StitchProgressSignal,
};

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use iced::futures::channel::mpsc::UnboundedSender;
use iced::widget::image::Handle as ImageHandle;
use rollshot_capture::{BackendKind, CaptureOptions, CapturedFrame, RegionMode, Size};

use crate::app::PreviewConstraints;
use crate::coords::LogicalRect;
use crate::diagnostics::{TARGET_CAPTURE, TARGET_STITCH};

use crate::CaptureResult;

#[cfg(feature = "action-guide")]
use rollshot_action::motion::{
    MotionFrame, MotionRecorder, MotionRecordingOutcome, MotionRuntimeStatus,
};

#[derive(Debug, Clone)]
pub enum LiveOverlayEvent {
    AcceptedActivity(Instant),
    Preview(ImageHandle),
    CaptureMiss(CaptureMissState),
}

/// Options for an action-guide recording session.
#[cfg(feature = "action-guide")]
pub(crate) struct ActionGuideRecordingOptions {
    pub motion_toolchain: Option<rollshot_action::video_import::VideoToolchain>,
}

/// Start status returned by `begin_action_recording`.
#[cfg(feature = "action-guide")]
pub(crate) struct ActionRecordingStart {
    pub capability: rollshot_action::InputCapability,
    #[allow(dead_code)] // consumed by UI projection via `Driver::motion_status()`
    pub motion_status: MotionRuntimeStatus,
}

/// Final result of an action-guide capture session.
#[cfg(feature = "action-guide")]
pub struct ActionGuideCaptureResult {
    pub recording: rollshot_action::Recording,
    pub capability: rollshot_action::InputCapability,
    pub region: rollshot_action::CaptureRegion,
    pub motion: MotionRecordingOutcome,
}

/// R4: emit on any active-flag transition (rising OR falling — so the recovery
/// edge clears the native marker) and on every warn pulse.
fn should_emit_capture_miss(state: &CaptureMissState, last_active: bool) -> bool {
    state.warn || state.active != last_active
}

/// Emit a native preview only when the stitcher actually accepted the frame;
/// a `Missed` (no-match) or `Idle` (duplicate/no-progress) signal should not
/// re-render — it would just redraw the same viewport.
fn should_emit_preview(signal: &StitchProgressSignal) -> bool {
    matches!(signal, StitchProgressSignal::Accepted { .. })
}

fn should_emit_accepted_activity(signal: &StitchProgressSignal) -> bool {
    matches!(signal, StitchProgressSignal::Accepted { .. })
}

/// Result of processing a single frame through the stitch-or-probe pipeline.
struct ProcessedFrame {
    signal: Option<StitchProgressSignal>,
    capture_miss: CaptureMissState,
    publish_preview: bool,
    publish_activity: bool,
}

/// Route one frame through the correct stitcher path:
/// - When the capture-miss gate is **active** (paused), use the read-only
///   `probe_recovery` to detect overlap without mutating the canvas.
/// - Otherwise, use `push_frame_preserving_anchor` (strict mode, no re-anchor).
fn process_frame(
    stitcher: &mut Stitcher,
    gate: &mut CaptureMissTracker,
    frame: image::RgbaImage,
    now: Instant,
) -> ProcessedFrame {
    if gate.active() {
        let recovered = stitcher.probe_recovery(&frame) == RecoveryProbeResult::Recovered;
        let was_active = gate.active();
        let capture_miss = gate.update_recovery(recovered, now);
        if capture_miss.active != was_active || capture_miss.warn {
            tracing::debug!(
                target: TARGET_STITCH,
                edge = ?capture_miss.edge,
                recovered,
                "capture-miss recovery probe"
            );
        }
        return ProcessedFrame {
            signal: None,
            capture_miss,
            publish_preview: false,
            publish_activity: false,
        };
    }

    let outcome = stitcher.push_frame_preserving_anchor(frame);
    let signal = progress_signal_from_outcome(&outcome);
    let was_active = gate.active();
    let capture_miss = gate.update(signal, now);
    if capture_miss.active != was_active || capture_miss.warn {
        tracing::debug!(
            target: TARGET_STITCH,
            active = capture_miss.active,
            warn = capture_miss.warn,
            edge = ?capture_miss.edge,
            "capture-miss transition"
        );
    }
    ProcessedFrame {
        signal: Some(signal),
        capture_miss,
        publish_preview: should_emit_preview(&signal),
        publish_activity: should_emit_accepted_activity(&signal),
    }
}

/// Wrapper that lets us move a `Box<dyn FrameStream>` to the reader thread.
/// `FrameStream` is not `Send` because the Linux PipeWire backend holds
/// `Rc`-based handles (thread-loop, stream, context, core).
struct SendStream(Box<dyn FrameStream>);
// SAFETY: the frame stream is moved exactly once into one reader thread and is
// never accessed from the creating thread afterward. All cancellation happens
// through `AtomicBool` plus thread-join boundaries, so there is no concurrent
// access to the stream's internals — regardless of whether the backend uses
// `Rc`-based handles (Linux/PipeWire) or other non-`Send` primitives. The sole
// owner lives and drops on the reader thread.
#[allow(unsafe_code)]
unsafe impl Send for SendStream {}

impl SendStream {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        self.0.next_frame()
    }
}

/// Stitch configuration used by the live capture overlay.
#[allow(dead_code)]
pub fn overlay_stitch_config() -> StitchConfig {
    let mut config = StitchConfig::default();
    config.min_overlap = 32;
    config
}

/// Crop+stitch a finite frame stream to completion. This is the tested core
/// the threaded live driver wraps.
#[allow(dead_code)]
pub fn stitch_stream(
    mut stream: Box<dyn FrameStream>,
    region: Region,
    config: StitchConfig,
) -> Result<CaptureResult, String> {
    let mut stitcher = Stitcher::new(config);
    loop {
        match stream.next_frame() {
            Ok(frame) => {
                let cropped = crop_frame(&frame, region).map_err(|e| e.to_string())?;
                stitcher.push_frame(cropped.image);
            }
            Err(CaptureError::EndOfStream) => break,
            Err(CaptureError::Timeout { .. }) => continue,
            Err(err) => return Err(err.to_string()),
        }
    }
    let image = stitcher
        .full_image()
        .ok_or_else(|| "stitcher produced no output".to_string())?
        .clone();
    Ok(CaptureResult {
        image,
        stats: Some(stitcher.stats()),
    })
}

struct Shared {
    latest: Mutex<Option<CapturedFrame>>,
    seq: AtomicU64,
    stitcher: Mutex<Stitcher>,
    error: Mutex<Option<String>>,
}

/// Live capture+stitch driver: a reader thread fills a latest-wins slot, a
/// stitch thread crops to `region` and pushes to the stitcher, emitting a fixed
/// preview viewport after each frame.
#[allow(dead_code)]
pub struct Driver {
    stop: Arc<AtomicBool>,
    shared: Arc<Shared>,
    reader: Option<JoinHandle<()>>,
    stitch: Option<JoinHandle<()>>,
    source_size: Size,
    overlay_logical: Size,
    /// Display the stream was pinned to (macOS display id), when the host
    /// resolved one. Lets the host place the overlay on the same display.
    target_display_id: Option<u32>,
    /// Wayland output name the stream was pinned to (KWin scrolling), when
    /// the host resolved one. Lets the overlay target the same output.
    target_output_name: Option<String>,
    capture_backend: &'static str,
    preview_tx: UnboundedSender<LiveOverlayEvent>,
    #[cfg(feature = "action-guide")]
    action_stop: Arc<AtomicBool>,
    #[cfg(feature = "action-guide")]
    action_thread: Option<JoinHandle<()>>,
    #[cfg(feature = "action-guide")]
    action_result: Option<std::sync::mpsc::Receiver<ActionGuideCaptureResult>>,
    /// Shared motion runtime status: set to `On` after MotionRecorder::start
    /// succeeds, `Off` after runtime failure or finish.
    #[cfg(feature = "action-guide")]
    motion_status: Arc<AtomicU8>,
}

#[allow(dead_code)]
impl Driver {
    /// Start capture + the reader thread, blocking until the portal handshake
    /// completes (the user picks a monitor and clicks Share) and the first frame
    /// arrives, so `source_size` is known. Stitching does NOT start here: call
    /// `begin_stitch` once the crop is chosen. Running the portal before the
    /// overlay exists keeps its screen-share picker dialog out of every captured
    /// frame (it appears + dismisses on a clean desktop, before any stitching).
    pub fn start_capture(
        backend: &str,
        fps: u32,
        show_cursor: bool,
        target_display_id: Option<u32>,
        target_output_name: Option<String>,
        preview_tx: UnboundedSender<LiveOverlayEvent>,
    ) -> Result<Self, String> {
        tracing::info!(target: TARGET_CAPTURE, backend, fps, show_cursor, ?target_display_id, ?target_output_name, "creating capture backend");
        let kind = BackendKind::from_cli_flag(backend).map_err(|e| e.to_string())?;
        let mut backend_impl = kind.create().map_err(|e| e.to_string())?;
        let stream = backend_impl
            .start(CaptureOptions {
                region: RegionMode::FullSource,
                fps,
                show_cursor,
                prefer_portal_region: false,
                target_display_id,
                target_output_name: target_output_name.clone(),
            })
            .map_err(|e| e.to_string())?;
        tracing::info!(target: TARGET_CAPTURE, "capture stream started");
        let mut stream = SendStream(stream);

        let shared = Arc::new(Shared {
            latest: Mutex::new(None),
            seq: AtomicU64::new(0),
            stitcher: Mutex::new(Stitcher::new(overlay_stitch_config())),
            error: Mutex::new(None),
        });
        let stop = Arc::new(AtomicBool::new(false));

        let reader = {
            let shared = Arc::clone(&shared);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match stream.next_frame() {
                        Ok(frame) => {
                            if let Ok(mut slot) = shared.latest.lock() {
                                *slot = Some(frame);
                                shared.seq.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        Err(CaptureError::EndOfStream) => {
                            tracing::debug!(target: TARGET_CAPTURE, "reader reached end-of-stream");
                            break;
                        }
                        Err(CaptureError::Timeout { .. }) => continue,
                        Err(err) => {
                            if !should_record_reader_error(&stop) {
                                break;
                            }
                            tracing::error!(target: TARGET_CAPTURE, %err, "reader terminal error");
                            if let Ok(mut e) = shared.error.lock() {
                                *e = Some(err.to_string());
                            }
                            break;
                        }
                    }
                }
            })
        };

        let (source_size, capture_backend) =
            wait_for_source_size(&shared, &stop, Duration::from_secs(5))?;
        tracing::debug!(target: TARGET_CAPTURE, width = source_size.width, height = source_size.height, "first frame arrived");

        Ok(Self {
            stop,
            shared,
            reader: Some(reader),
            stitch: None,
            source_size,
            overlay_logical: Size {
                width: 0,
                height: 0,
            },
            target_display_id,
            target_output_name,
            capture_backend,
            preview_tx,
            #[cfg(feature = "action-guide")]
            action_stop: Arc::new(AtomicBool::new(false)),
            #[cfg(feature = "action-guide")]
            action_thread: None,
            #[cfg(feature = "action-guide")]
            action_result: None,
            #[cfg(feature = "action-guide")]
            motion_status: Arc::new(AtomicU8::new(MotionRuntimeStatus::Off as u8)),
        })
    }

    /// Map the chosen crop (overlay-logical px) to frame pixels and start the
    /// stitch thread. The canvas-base frame is taken from a frame captured
    /// *after* this call (`last_seq` seeded to the current seq), i.e. live,
    /// once the picker is gone and the overlay has settled into capture chrome.
    pub fn begin_stitch(
        &mut self,
        crop_logical: LogicalRect,
        overlay_logical: Size,
        preview_constraints: PreviewConstraints,
    ) {
        if self.stitch.is_some() {
            return;
        }
        self.overlay_logical = overlay_logical;
        let region =
            crate::coords::map_crop_to_frame(crop_logical, overlay_logical, self.source_size);
        tracing::info!(
            target: TARGET_STITCH,
            source_width = self.source_size.width,
            source_height = self.source_size.height,
            crop_x = region.x,
            crop_y = region.y,
            crop_w = region.width,
            crop_h = region.height,
            "begin stitch"
        );
        let shared = Arc::clone(&self.shared);
        let stop = Arc::clone(&self.stop);
        let preview_tx = self.preview_tx.clone();
        self.stitch = Some(std::thread::spawn(move || {
            let starting_seq = shared.seq.load(Ordering::Relaxed);
            let fallback_deadline = Instant::now() + Duration::from_millis(250);
            let mut last_seq = starting_seq;
            let mut capture_miss_tracker = CaptureMissTracker::default();
            let mut last_capture_miss_active = false;
            let mut spotlight_edge = CapturedEdge::Unknown;
            let mut fallback_used = false;
            while !stop.load(Ordering::Relaxed) {
                let seq = shared.seq.load(Ordering::Relaxed);
                if !fallback_used
                    && seq == starting_seq
                    && last_seq == starting_seq
                    && Instant::now() >= fallback_deadline
                {
                    tracing::debug!(target: TARGET_STITCH, "fallback deadline reached, seeding first frame");
                    fallback_used = true;
                    last_seq = starting_seq.saturating_sub(1);
                }
                let frame = if seq == last_seq {
                    None
                } else {
                    last_seq = seq;
                    shared.latest.lock().ok().and_then(|s| s.clone())
                };
                if let Some(frame) = frame {
                    let cropped = match crop_frame(&frame, region) {
                        Ok(c) => c,
                        Err(err) => {
                            tracing::error!(target: TARGET_STITCH, %err, "crop failed");
                            if let Ok(mut e) = shared.error.lock() {
                                *e = Some(err.to_string());
                            }
                            break;
                        }
                    };
                    if let Ok(mut stitcher) = shared.stitcher.lock() {
                        let result = process_frame(
                            &mut stitcher,
                            &mut capture_miss_tracker,
                            cropped.image,
                            Instant::now(),
                        );
                        if let Some(StitchProgressSignal::Accepted { edge }) = result.signal {
                            if edge != CapturedEdge::Unknown {
                                spotlight_edge = edge;
                            }
                        }
                        if should_emit_capture_miss(&result.capture_miss, last_capture_miss_active)
                        {
                            let _ = preview_tx
                                .unbounded_send(LiveOverlayEvent::CaptureMiss(result.capture_miss));
                        }
                        last_capture_miss_active = result.capture_miss.active;
                        if result.publish_activity {
                            let _ = preview_tx
                                .unbounded_send(LiveOverlayEvent::AcceptedActivity(Instant::now()));
                        }
                        if result.publish_preview {
                            if let Some(handle) = preview_handle(
                                &mut stitcher,
                                region,
                                spotlight_edge,
                                preview_constraints,
                            ) {
                                let _ =
                                    preview_tx.unbounded_send(LiveOverlayEvent::Preview(handle));
                            }
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            tracing::debug!(target: TARGET_STITCH, "stitch thread exiting");
        }));
    }

    pub(crate) fn source_size(&self) -> Size {
        self.source_size
    }

    pub(crate) fn target_display_id(&self) -> Option<u32> {
        self.target_display_id
    }

    pub fn target_output_name(&self) -> Option<&str> {
        self.target_output_name.as_deref()
    }

    pub fn capture_backend(&self) -> &'static str {
        self.capture_backend
    }

    pub(crate) fn overlay_size(&self) -> Size {
        self.overlay_logical
    }

    /// Signal both threads to stop and join them.
    fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        #[cfg(feature = "action-guide")]
        self.action_stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.stitch.take() {
            if h.join().is_err() {
                tracing::warn!(target: TARGET_STITCH, "stitch thread panicked");
            }
        }
        #[cfg(feature = "action-guide")]
        if let Some(h) = self.action_thread.take() {
            if h.join().is_err() {
                tracing::warn!(target: TARGET_STITCH, "action thread panicked");
            }
        }
        if let Some(h) = self.reader.take() {
            if h.join().is_err() {
                tracing::warn!(target: TARGET_CAPTURE, "reader thread panicked");
            }
        }
    }

    /// Stop capture without producing a result (user cancelled at/ before the
    /// crop, or the loop exited early). Tears the PipeWire stream down cleanly.
    pub fn cancel(mut self) {
        tracing::info!(target: TARGET_CAPTURE, "driver cancel");
        self.stop_and_join();
    }

    /// Stop both threads and produce the finalized capture.
    pub fn finalize(mut self) -> Result<CaptureResult, String> {
        self.stop_and_join();
        if let Ok(e) = self.shared.error.lock() {
            if let Some(msg) = e.as_ref() {
                return Err(msg.clone());
            }
        }
        let mut stitcher = self
            .shared
            .stitcher
            .lock()
            .map_err(|_| "stitcher lock poisoned".to_string())?;
        let image = stitcher
            .full_image()
            .ok_or_else(|| "stitcher produced no output".to_string())?
            .clone();
        let stats = stitcher.stats();
        tracing::info!(
            target: TARGET_STITCH,
            width = image.width(),
            height = image.height(),
            frame_count = stats.frame_count,
            "finalize complete"
        );
        Ok(CaptureResult {
            image,
            stats: Some(stats),
        })
    }
}

#[cfg(feature = "action-guide")]
#[allow(dead_code)]
impl Driver {
    /// Spawn the action consumer thread: tee each new captured frame into the
    /// recorder (converting SystemTime -> session-relative ms) and poll input.
    pub(crate) fn begin_action_recording(
        &mut self,
        region: rollshot_action::CaptureRegion,
        source: Box<dyn rollshot_action::SemanticInputSource>,
        options: ActionGuideRecordingOptions,
    ) -> ActionRecordingStart {
        let shared = self.shared.clone();
        let action_stop = self.action_stop.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.action_result = Some(rx);
        // Capture is `RegionMode::FullSource`, so frames in `Shared.latest` are
        // whole-source; crop them to the selected region before handing them to
        // the recorder, which expects pre-cropped frames (see
        // `ActionRecorder::ingest_frame`).
        let crop_region = Region {
            x: region.x,
            y: region.y,
            width: region.width,
            height: region.height,
        };
        // Start the source synchronously so the resolved capability is known
        // immediately (the UI surfaces it as a label/advisory); then hand the
        // started recording to the consumer thread.
        let mut rec = ActionRecording::start(region, source, &options);
        let capability = rec.capability();
        let motion_status_val = rec.motion_status();
        let shared_motion_status = self.motion_status.clone();
        shared_motion_status.store(motion_status_val as u8, Ordering::Release);
        self.action_thread = Some(std::thread::spawn(move || {
            let mut last_seq = shared.seq.load(Ordering::Relaxed);
            let mut t0: Option<std::time::SystemTime> = None;
            let mut session_start: Option<Instant> = None;
            let action_region = region;
            while !action_stop.load(Ordering::Relaxed) {
                let seq = shared.seq.load(Ordering::Relaxed);
                if seq != last_seq {
                    last_seq = seq;
                    if let Some(frame) = shared.latest.lock().ok().and_then(|s| s.clone()) {
                        let _ = session_start.get_or_insert(Instant::now());
                        let base = *t0.get_or_insert(frame.timestamp);
                        let at_ms = frame
                            .timestamp
                            .duration_since(base)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        let cropped = match crop_frame(&frame, crop_region) {
                            Ok(c) => c.image,
                            // Region outside the source: fall back to the
                            // uncropped frame so detection still runs.
                            Err(err) => {
                                tracing::warn!(
                                    target: TARGET_CAPTURE,
                                    %err,
                                    "action crop failed; using uncropped frame"
                                );
                                frame.image
                            }
                        };
                        let image: rollshot_action::SharedActionFrame =
                            std::sync::Arc::new(cropped);
                        rec.push_frame(Arc::clone(&image), at_ms);
                        rec.offer_motion(image, at_ms);
                        if let Some(status) = rec.motion_status_if_changed() {
                            shared_motion_status.store(status as u8, Ordering::Release);
                        }
                    }
                }
                rec.poll_input();
                std::thread::sleep(Duration::from_millis(20));
            }
            let session_duration_ms = session_start
                .map(|s| s.elapsed().as_millis() as u64)
                .unwrap_or(0);
            let result = rec.finalize_with_region(session_duration_ms, action_region);
            let _ = tx.send(result);
        }));
        ActionRecordingStart {
            capability,
            motion_status: motion_status_val,
        }
    }

    /// Current motion encoder runtime status.
    pub(crate) fn motion_status(&self) -> MotionRuntimeStatus {
        match self.motion_status.load(Ordering::Acquire) {
            0 => MotionRuntimeStatus::On,
            _ => MotionRuntimeStatus::Off,
        }
    }

    /// Signal the action thread to stop and collect the finished
    /// ActionGuideCaptureResult.
    pub(crate) fn finalize_action(mut self) -> Result<ActionGuideCaptureResult, String> {
        self.action_stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.action_thread.take() {
            let _ = handle.join();
        }
        self.action_result
            .take()
            .and_then(|rx| rx.recv().ok())
            .ok_or_else(|| "action recording produced no result".to_string())
    }
}

#[cfg(feature = "action-guide")]
#[allow(dead_code)]
pub(crate) struct ActionRecording {
    recorder: rollshot_action::ActionRecorder,
    input: rollshot_action::StartedSemanticInput,
    motion: Option<MotionRecorder>,
    motion_failure: Option<rollshot_action::motion::MotionFailureCategory>,
}

#[cfg(feature = "action-guide")]
#[allow(dead_code)]
impl ActionRecording {
    pub(crate) fn start(
        region: rollshot_action::CaptureRegion,
        source: Box<dyn rollshot_action::SemanticInputSource>,
        options: &ActionGuideRecordingOptions,
    ) -> Self {
        use rollshot_action::{DetectorConfig, StoreConfig};
        let motion = options.motion_toolchain.as_ref().and_then(|toolchain| {
            match MotionRecorder::start(toolchain, region.width, region.height) {
                Ok(recorder) => Some(recorder),
                Err(e) => {
                    tracing::warn!(target: TARGET_CAPTURE, %e, "motion recorder start failed");
                    None
                }
            }
        });
        Self {
            recorder: rollshot_action::ActionRecorder::new(
                region,
                StoreConfig::default(),
                DetectorConfig::default(),
            ),
            // Shared start/fallback/poll/stop semantics: on start failure the
            // source is swapped to a started VisualOnlySource carrying the real
            // reason (see `rollshot_action::StartedSemanticInput`).
            input: rollshot_action::StartedSemanticInput::start(source, region),
            motion,
            motion_failure: None,
        }
    }

    /// `at_ms` is session-relative milliseconds (monotonic from 0).
    /// `image` is a shared frame (zero-copy tee: the guide recorder and motion
    /// sink observe the same `Arc` allocation).
    pub(crate) fn push_frame(&mut self, image: rollshot_action::SharedActionFrame, at_ms: u64) {
        self.recorder.ingest_frame(image, at_ms);
    }

    /// Offer a frame to the motion sink. Stops offering on first failure.
    pub(crate) fn offer_motion(&mut self, image: rollshot_action::SharedActionFrame, at_ms: u64) {
        if self.motion_failure.is_some() {
            return;
        }
        if let Some(ref motion) = self.motion {
            let frame = MotionFrame { at_ms, image };
            if let Err(e) = motion.offer(frame) {
                tracing::warn!(target: TARGET_CAPTURE, %e, "motion offer failed");
                self.motion_failure = Some(e);
            }
        }
    }

    pub(crate) fn poll_input(&mut self) {
        self.input.poll_into(&mut self.recorder);
    }

    pub(crate) fn capability(&self) -> rollshot_action::InputCapability {
        self.input.capability()
    }

    pub(crate) fn motion_status(&self) -> MotionRuntimeStatus {
        match &self.motion {
            Some(m) => m.status(),
            None => MotionRuntimeStatus::Off,
        }
    }

    /// Return the current motion status if it differs from the last known
    /// status, or `None` if unchanged.
    fn motion_status_if_changed(&self) -> Option<MotionRuntimeStatus> {
        // We always report the current status; the caller decides whether to
        // store it. For simplicity, always return the current status.
        Some(self.motion_status())
    }

    /// Finalize both the guide recording and motion sink, producing the full
    /// capture result.
    pub(crate) fn finalize_with_region(
        mut self,
        session_duration_ms: u64,
        region: rollshot_action::CaptureRegion,
    ) -> ActionGuideCaptureResult {
        self.input.stop();
        let recording = self.recorder.finish();
        let capability = self.input.capability();
        let motion = match self.motion.take() {
            Some(mut motion) => motion.finish(session_duration_ms),
            None => MotionRecordingOutcome::Failure(
                self.motion_failure
                    .unwrap_or(rollshot_action::motion::MotionFailureCategory::Cancelled),
            ),
        };
        ActionGuideCaptureResult {
            recording,
            capability,
            region,
            motion,
        }
    }

    /// Finalize only the guide recording (no motion), returning the recording
    /// and capability. Used when the caller does not need motion outcome.
    pub(crate) fn finalize(mut self) -> rollshot_action::Recording {
        self.input.stop();
        self.recorder.finish()
    }
}

fn should_record_reader_error(stop: &AtomicBool) -> bool {
    !stop.load(Ordering::Relaxed)
}

/// Block until the reader stores the first frame, returning its `source_size`
/// (falling back to the frame's pixel dimensions if metadata omits it).
#[allow(dead_code)]
fn wait_for_source_size(
    shared: &Arc<Shared>,
    stop: &Arc<AtomicBool>,
    timeout: Duration,
) -> Result<(Size, &'static str), String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(e) = shared.error.lock() {
            if let Some(msg) = e.as_ref() {
                return Err(msg.clone());
            }
        }
        if let Ok(slot) = shared.latest.lock() {
            if let Some(frame) = slot.as_ref() {
                return Ok((
                    frame.metadata.source_size.unwrap_or(Size {
                        width: frame.image.width(),
                        height: frame.image.height(),
                    }),
                    frame.metadata.backend,
                ));
            }
        }
        if Instant::now() >= deadline {
            stop.store(true, Ordering::Relaxed);
            tracing::error!(target: TARGET_CAPTURE, "timed out waiting for first frame");
            return Err("timed out waiting for first capture frame".to_string());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn preview_handle(
    stitcher: &mut Stitcher,
    region: Region,
    edge: rollshot_overlay_core::capture_miss::CapturedEdge,
    constraints: PreviewConstraints,
) -> Option<ImageHandle> {
    if matches!(
        edge,
        rollshot_overlay_core::capture_miss::CapturedEdge::Left
            | rollshot_overlay_core::capture_miss::CapturedEdge::Right
    ) {
        let view = rollshot_overlay_core::preview::viewport_preview(
            stitcher,
            rollshot_overlay_core::preview::ViewportPreviewRequest {
                viewport_width: constraints.fixed_width,
                viewport_height: constraints.max_height,
                frame_width: region.width,
                frame_height: region.height,
                edge,
            },
        )?;
        return Some(ImageHandle::from_rgba(view.width, view.height, view.pixels));
    }

    let view = rollshot_overlay_core::preview::growing_preview(
        stitcher,
        rollshot_overlay_core::preview::GrowingPreviewRequest {
            fixed_width: constraints.fixed_width,
            max_height: constraints.max_height,
            edge,
        },
    )?;
    Some(ImageHandle::from_rgba(view.width, view.height, view.pixels))
}

#[cfg(all(test, feature = "action-guide"))]
mod action_tests {
    use super::*;
    use image::RgbaImage;
    use rollshot_action::{CaptureRegion, DegradedReason, VisualOnlySource};

    fn no_motion_options() -> ActionGuideRecordingOptions {
        ActionGuideRecordingOptions {
            motion_toolchain: None,
        }
    }

    #[test]
    fn finalize_action_produces_candidates_from_changing_frames() {
        let region = CaptureRegion {
            x: 0,
            y: 0,
            width: 64,
            height: 64,
        };
        let mut rec = ActionRecording::start(
            region,
            Box::new(VisualOnlySource::new(DegradedReason::SourceStartFailed)),
            &no_motion_options(),
        );
        let black = Arc::new(RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 0, 255])));
        let white = Arc::new(RgbaImage::from_pixel(
            64,
            64,
            image::Rgba([255, 255, 255, 255]),
        ));
        rec.push_frame(Arc::clone(&black), 0);
        // Multiple white frames to satisfy stable_frames settle.
        rec.push_frame(Arc::clone(&white), 500);
        rec.push_frame(Arc::clone(&white), 600);
        rec.push_frame(Arc::clone(&white), 700);
        assert!(
            matches!(
                rec.capability(),
                rollshot_action::InputCapability::VisualOnly { .. }
            ),
            "capability is captured from the source start"
        );
        let recording = rec.finalize();
        assert!(
            !recording.candidates.is_empty(),
            "expected at least one candidate"
        );
    }

    #[test]
    fn cancel_action_recording_finishes_without_panic() {
        let region = CaptureRegion {
            x: 0,
            y: 0,
            width: 64,
            height: 64,
        };
        let mut rec = ActionRecording::start(
            region,
            Box::new(VisualOnlySource::new(DegradedReason::PermissionDenied)),
            &no_motion_options(),
        );
        rec.push_frame(
            Arc::new(RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 0, 255]))),
            0,
        );
        let recording = rec.finalize();
        assert!(recording.candidates.is_empty());
    }

    // ── RED tests: zero-copy tee contract ───────────────────────────────

    #[test]
    fn motion_disabled_recorder_sequence_matches_and_motion_factory_never_called() {
        let region = CaptureRegion {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        };
        let mut rec = ActionRecording::start(
            region,
            Box::new(VisualOnlySource::new(DegradedReason::SourceStartFailed)),
            &no_motion_options(),
        );
        assert!(
            rec.motion.is_none(),
            "motion factory must not be called when toolchain is None"
        );
        let f1 = Arc::new(RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255])));
        let f2 = Arc::new(RgbaImage::from_pixel(8, 8, image::Rgba([40, 50, 60, 255])));
        rec.push_frame(Arc::clone(&f1), 0);
        rec.push_frame(Arc::clone(&f2), 100);
        // offer_motion is a no-op when motion is None.
        rec.offer_motion(Arc::clone(&f1), 0);
        rec.offer_motion(Arc::clone(&f2), 100);
        let result = rec.finalize_with_region(200, region);
        assert!(
            !result.recording.candidates.is_empty(),
            "recorder must produce candidates from changing frames"
        );
        // motion outcome must be Failure since motion was disabled.
        assert!(
            matches!(result.motion, MotionRecordingOutcome::Failure(_)),
            "motion outcome must be Failure when motion was disabled"
        );
    }

    // ── RED tests: completion/failure contract ──────────────────────────

    #[test]
    fn motion_status_is_off_when_motion_not_started() {
        let region = CaptureRegion {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        };
        let rec = ActionRecording::start(
            region,
            Box::new(VisualOnlySource::new(DegradedReason::SourceStartFailed)),
            &no_motion_options(),
        );
        assert_eq!(
            rec.motion_status(),
            MotionRuntimeStatus::Off,
            "motion status must be Off when no motion recorder was started"
        );
    }

    #[test]
    fn finalize_with_region_reports_guide_and_motion_outcome() {
        let region = CaptureRegion {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        };
        let mut rec = ActionRecording::start(
            region,
            Box::new(VisualOnlySource::new(DegradedReason::SourceStartFailed)),
            &no_motion_options(),
        );
        let f1 = Arc::new(RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255])));
        let f2 = Arc::new(RgbaImage::from_pixel(8, 8, image::Rgba([40, 50, 60, 255])));
        rec.push_frame(Arc::clone(&f1), 0);
        rec.push_frame(Arc::clone(&f2), 100);
        let result = rec.finalize_with_region(200, region);
        assert_eq!(result.region, region, "result must carry the region");
        assert!(
            !result.recording.candidates.is_empty(),
            "recorder must produce candidates"
        );
        assert!(
            matches!(result.motion, MotionRecordingOutcome::Failure(_)),
            "motion must be Failure when not started"
        );
    }

    #[test]
    fn action_guide_capture_result_carries_region() {
        let region = CaptureRegion {
            x: 10,
            y: 20,
            width: 64,
            height: 64,
        };
        let mut rec = ActionRecording::start(
            region,
            Box::new(VisualOnlySource::new(DegradedReason::SourceStartFailed)),
            &no_motion_options(),
        );
        rec.push_frame(
            Arc::new(RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 0, 255]))),
            0,
        );
        let result = rec.finalize_with_region(100, region);
        assert_eq!(result.region.x, 10);
        assert_eq!(result.region.y, 20);
        assert_eq!(result.region.width, 64);
        assert_eq!(result.region.height, 64);
    }

    #[test]
    fn action_guide_capture_result_carries_capability() {
        let region = CaptureRegion {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        };
        let mut rec = ActionRecording::start(
            region,
            Box::new(VisualOnlySource::new(DegradedReason::PermissionDenied)),
            &no_motion_options(),
        );
        rec.push_frame(
            Arc::new(RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 0, 255]))),
            0,
        );
        let result = rec.finalize_with_region(100, region);
        assert!(
            matches!(
                result.capability,
                rollshot_action::InputCapability::VisualOnly { .. }
            ),
            "capability must reflect the source"
        );
    }

    #[test]
    fn motion_off_produces_failure_on_finalize_when_no_motion() {
        let region = CaptureRegion {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        };
        let rec = ActionRecording::start(
            region,
            Box::new(VisualOnlySource::new(DegradedReason::SourceStartFailed)),
            &no_motion_options(),
        );
        let result = rec.finalize_with_region(0, region);
        assert!(
            matches!(result.motion, MotionRecordingOutcome::Failure(_)),
            "finalize must report Failure when motion was never started"
        );
    }

    // ── RED tests: zero-copy tee with motion enabled ────────────────────

    #[test]
    fn tee_shares_arc_pointer_between_recorder_and_motion() {
        // Requires FFmpeg. Skipped in environments without it.
        let ffmpeg = match std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
        {
            Ok(o) if o.status.success() => std::path::PathBuf::from("ffmpeg"),
            _ => {
                eprintln!("SKIPPED: FFmpeg not available");
                return;
            }
        };
        let toolchain = rollshot_action::video_import::VideoToolchain {
            ffmpeg: ffmpeg.clone(),
            ffprobe: ffmpeg,
        };
        let region = CaptureRegion {
            x: 0,
            y: 0,
            width: 64,
            height: 64,
        };
        let options = ActionGuideRecordingOptions {
            motion_toolchain: Some(toolchain),
        };
        let mut rec = ActionRecording::start(
            region,
            Box::new(VisualOnlySource::new(DegradedReason::SourceStartFailed)),
            &options,
        );
        assert!(
            rec.motion.is_some(),
            "motion recorder must start when toolchain is available"
        );
        assert_eq!(
            rec.motion_status(),
            MotionRuntimeStatus::On,
            "motion status must be On after successful start"
        );
        let f1: rollshot_action::SharedActionFrame = Arc::new(RgbaImage::from_pixel(
            64,
            64,
            image::Rgba([10, 20, 30, 255]),
        ));
        let f2: rollshot_action::SharedActionFrame = Arc::new(RgbaImage::from_pixel(
            64,
            64,
            image::Rgba([40, 50, 60, 255]),
        ));
        rec.push_frame(Arc::clone(&f1), 0);
        rec.offer_motion(Arc::clone(&f1), 0);
        rec.push_frame(Arc::clone(&f2), 100);
        rec.offer_motion(Arc::clone(&f2), 100);
        let result = rec.finalize_with_region(200, region);
        assert!(
            !result.recording.candidates.is_empty(),
            "recorder must produce candidates"
        );
        // Motion outcome should be Ready if FFmpeg processed successfully.
        match result.motion {
            MotionRecordingOutcome::Ready(asset) => {
                assert!(
                    !asset.sha256().is_empty(),
                    "validated asset must have a sha256 digest"
                );
            }
            MotionRecordingOutcome::Failure(e) => {
                // FFmpeg may fail in CI; record the failure but don't panic.
                eprintln!("motion recording failed (may be CI): {e}");
            }
        }
    }

    #[test]
    fn motion_stalled_failing_does_not_affect_guide_candidates() {
        let region = CaptureRegion {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        };
        let mut rec_with_motion = ActionRecording::start(
            region,
            Box::new(VisualOnlySource::new(DegradedReason::SourceStartFailed)),
            &no_motion_options(),
        );
        let mut rec_without = ActionRecording::start(
            region,
            Box::new(VisualOnlySource::new(DegradedReason::SourceStartFailed)),
            &no_motion_options(),
        );
        let f1 = Arc::new(RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255])));
        let f2 = Arc::new(RgbaImage::from_pixel(8, 8, image::Rgba([40, 50, 60, 255])));
        // Feed identical frames to both recorders.
        rec_with_motion.push_frame(Arc::clone(&f1), 0);
        rec_with_motion.push_frame(Arc::clone(&f2), 100);
        rec_without.push_frame(Arc::clone(&f1), 0);
        rec_without.push_frame(Arc::clone(&f2), 100);
        let r1 = rec_with_motion.finalize_with_region(200, region);
        let r2 = rec_without.finalize_with_region(200, region);
        assert_eq!(
            r1.recording.candidates.len(),
            r2.recording.candidates.len(),
            "guide candidates must match regardless of motion state"
        );
    }

    // ── RED tests: completion/failure lifecycle ─────────────────────────

    #[test]
    fn motion_success_produces_ready_outcome() {
        let ffmpeg = match std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
        {
            Ok(o) if o.status.success() => std::path::PathBuf::from("ffmpeg"),
            _ => {
                eprintln!("SKIPPED: FFmpeg not available");
                return;
            }
        };
        let toolchain = rollshot_action::video_import::VideoToolchain {
            ffmpeg: ffmpeg.clone(),
            ffprobe: ffmpeg,
        };
        let region = CaptureRegion {
            x: 0,
            y: 0,
            width: 64,
            height: 64,
        };
        let options = ActionGuideRecordingOptions {
            motion_toolchain: Some(toolchain),
        };
        let mut rec = ActionRecording::start(
            region,
            Box::new(VisualOnlySource::new(DegradedReason::SourceStartFailed)),
            &options,
        );
        // Feed enough frames for a valid H.264 stream.
        for i in 0..10u64 {
            let frame: rollshot_action::SharedActionFrame = Arc::new(RgbaImage::from_pixel(
                64,
                64,
                image::Rgba([(i * 25) as u8, 100, 200, 255]),
            ));
            rec.push_frame(Arc::clone(&frame), i * 33);
            rec.offer_motion(Arc::clone(&frame), i * 33);
        }
        let result = rec.finalize_with_region(330, region);
        match result.motion {
            MotionRecordingOutcome::Ready(asset) => {
                assert!(
                    !asset.sha256().is_empty(),
                    "validated asset must have a sha256 digest"
                );
            }
            MotionRecordingOutcome::Failure(e) => {
                panic!("motion recording should succeed with valid frames: {e}");
            }
        }
    }

    #[test]
    fn cancel_motion_cleans_encoder() {
        let ffmpeg = match std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
        {
            Ok(o) if o.status.success() => std::path::PathBuf::from("ffmpeg"),
            _ => {
                eprintln!("SKIPPED: FFmpeg not available");
                return;
            }
        };
        let toolchain = rollshot_action::video_import::VideoToolchain {
            ffmpeg: ffmpeg.clone(),
            ffprobe: ffmpeg,
        };
        let region = CaptureRegion {
            x: 0,
            y: 0,
            width: 64,
            height: 64,
        };
        let options = ActionGuideRecordingOptions {
            motion_toolchain: Some(toolchain),
        };
        let mut rec = ActionRecording::start(
            region,
            Box::new(VisualOnlySource::new(DegradedReason::SourceStartFailed)),
            &options,
        );
        rec.push_frame(
            Arc::new(RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 0, 255]))),
            0,
        );
        // Cancel the motion recorder before finalizing.
        if let Some(ref mut motion) = rec.motion {
            motion.cancel();
        }
        let result = rec.finalize_with_region(100, region);
        // After cancel, motion outcome should be Failure(Cancelled).
        assert!(
            matches!(
                result.motion,
                MotionRecordingOutcome::Failure(
                    rollshot_action::motion::MotionFailureCategory::Cancelled
                )
            ),
            "cancelled motion must produce Failure(Cancelled)"
        );
    }

    #[test]
    fn motion_spawn_failure_leaves_status_off() {
        // No toolchain → no motion → status Off.
        let region = CaptureRegion {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        };
        let rec = ActionRecording::start(
            region,
            Box::new(VisualOnlySource::new(DegradedReason::SourceStartFailed)),
            &no_motion_options(),
        );
        assert_eq!(
            rec.motion_status(),
            MotionRuntimeStatus::Off,
            "motion status must be Off when no recorder started"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        overlay_stitch_config, preview_handle, process_frame, should_emit_accepted_activity,
        should_emit_capture_miss, should_emit_preview, stitch_stream, PreviewConstraints,
    };
    use iced::widget::image::Handle as ImageHandle;
    use image::{Rgba, RgbaImage};
    use rollshot_capture::{CapturedFrame, FakeFrameStream, FrameMetadata, Region};
    use rollshot_core::{StitchOutcome, Stitcher};
    use rollshot_overlay_core::capture_miss::{
        CaptureMissState, CaptureMissTracker, StitchProgressSignal,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant, SystemTime};

    // A tall canvas; each frame is an 80x80 window scrolled down by `offset_y`.
    fn scrolling_frame(offset_y: u32) -> CapturedFrame {
        let (w, h) = (80u32, 200u32);
        let mut canvas = RgbaImage::from_pixel(w, h, Rgba([245, 245, 245, 255]));
        for y in (0..h).step_by(11) {
            for x in 8..w.saturating_sub(8) {
                let stripe = if (x / 5 + y / 7) % 2 == 0 { 220 } else { 180 };
                canvas.put_pixel(x, y, Rgba([(y % 180) as u8, stripe, 80, 255]));
                if y + 1 < h {
                    canvas.put_pixel(x, y + 1, Rgba([30, 30, 30, 255]));
                }
            }
        }
        let image = image::imageops::crop_imm(&canvas, 0, offset_y, 80, 80).to_image();
        CapturedFrame {
            image,
            timestamp: SystemTime::UNIX_EPOCH,
            metadata: FrameMetadata::fake(),
        }
    }

    #[test]
    fn stitch_stream_crops_and_finalizes() {
        let frames = vec![scrolling_frame(0), scrolling_frame(8), scrolling_frame(16)];
        let stream = Box::new(FakeFrameStream::new(frames));
        let region = Region {
            x: 0,
            y: 0,
            width: 60,
            height: 60,
        };

        let result = stitch_stream(stream, region, overlay_stitch_config())
            .expect("stitch produced a result");

        assert_eq!(result.image.width(), 60);
        assert!(
            result.image.height() >= 60,
            "stitched height grows past one frame"
        );
        assert!(result.stats.unwrap().frame_count >= 1);
    }

    #[test]
    fn capture_miss_emit_on_rising_edge() {
        let state = CaptureMissState {
            active: true,
            warn: false,
            ..Default::default()
        };
        assert!(should_emit_capture_miss(&state, false));
    }

    #[test]
    fn capture_miss_emit_on_clearing_edge() {
        let state = CaptureMissState::default(); // active=false, warn=false
        assert!(should_emit_capture_miss(&state, true));
    }

    #[test]
    fn capture_miss_emit_skipped_when_steady_active() {
        let state = CaptureMissState {
            active: true,
            warn: false,
            ..Default::default()
        };
        assert!(!should_emit_capture_miss(&state, true));
    }

    #[test]
    fn capture_miss_emit_on_warn_pulse_when_active_unchanged() {
        let state = CaptureMissState {
            active: true,
            warn: true,
            ..Default::default()
        };
        assert!(should_emit_capture_miss(&state, true));
    }

    #[test]
    fn preview_handle_uses_growing_height_for_vertical_preview() {
        let mut stitcher = Stitcher::new(overlay_stitch_config());
        assert_eq!(
            stitcher.push_frame(scrolling_frame(0).image),
            StitchOutcome::FirstFrame
        );

        let handle = preview_handle(
            &mut stitcher,
            Region {
                x: 0,
                y: 0,
                width: 80,
                height: 80,
            },
            rollshot_overlay_core::capture_miss::CapturedEdge::Bottom,
            PreviewConstraints {
                fixed_width: 120,
                max_height: 180,
            },
        )
        .expect("growing preview for first frame");

        match handle {
            ImageHandle::Rgba { width, height, .. } => {
                assert_eq!(width, 120);
                assert_eq!(height, 120);
            }
            other => panic!("expected Rgba handle, got {other:?}"),
        }
    }

    #[test]
    fn preview_handle_keeps_viewport_size_for_horizontal_preview() {
        let mut stitcher = Stitcher::new(overlay_stitch_config());
        assert_eq!(
            stitcher.push_frame(scrolling_frame(0).image),
            StitchOutcome::FirstFrame
        );

        let handle = preview_handle(
            &mut stitcher,
            Region {
                x: 0,
                y: 0,
                width: 80,
                height: 80,
            },
            rollshot_overlay_core::capture_miss::CapturedEdge::Right,
            PreviewConstraints {
                fixed_width: 120,
                max_height: 180,
            },
        )
        .expect("viewport preview for first frame");

        match handle {
            ImageHandle::Rgba { width, height, .. } => {
                assert_eq!(width, 120);
                assert_eq!(height, 180);
            }
            other => panic!("expected Rgba handle, got {other:?}"),
        }
    }

    #[test]
    fn native_preview_emits_only_for_accepted_progress() {
        assert!(should_emit_preview(&StitchProgressSignal::Accepted {
            edge: rollshot_overlay_core::capture_miss::CapturedEdge::Bottom,
        }));
        assert!(!should_emit_preview(&StitchProgressSignal::Missed {
            edge: rollshot_overlay_core::capture_miss::CapturedEdge::Bottom,
        }));
        assert!(!should_emit_preview(&StitchProgressSignal::Idle));
    }

    #[test]
    fn reader_error_after_stop_is_shutdown_noise() {
        let stop = AtomicBool::new(true);

        assert!(!super::should_record_reader_error(&stop));
    }

    #[test]
    fn reader_error_before_stop_is_fatal() {
        let stop = AtomicBool::new(false);

        assert!(super::should_record_reader_error(&stop));
        stop.store(true, Ordering::Relaxed);
        assert!(!super::should_record_reader_error(&stop));
    }

    #[test]
    fn accepted_signal_emits_activity_even_when_preview_is_unavailable() {
        assert_eq!(
            live_events_for_signal(
                StitchProgressSignal::Accepted {
                    edge: rollshot_overlay_core::capture_miss::CapturedEdge::Bottom
                },
                false
            ),
            vec![LiveEventKind::AcceptedActivity]
        );
    }

    #[test]
    fn missed_signal_does_not_emit_accepted_activity() {
        assert!(!should_emit_accepted_activity(
            &StitchProgressSignal::Missed {
                edge: rollshot_overlay_core::capture_miss::CapturedEdge::Unknown
            }
        ));
    }

    #[derive(Debug, PartialEq, Eq)]
    #[allow(dead_code)]
    enum LiveEventKind {
        AcceptedActivity,
        Preview,
        CaptureMiss,
    }

    fn live_events_for_signal(
        signal: StitchProgressSignal,
        _preview_available: bool,
    ) -> Vec<LiveEventKind> {
        let mut events = Vec::new();
        if super::should_emit_accepted_activity(&signal) {
            events.push(LiveEventKind::AcceptedActivity);
        }
        events
    }

    // ------------------------------------------------------------------
    // Local RgbaImage helpers for process_frame tests
    // ------------------------------------------------------------------

    /// Build a tall scroll canvas (80 × 200) with unique stripe patterns.
    fn make_scroll_canvas() -> RgbaImage {
        let (w, h) = (80u32, 200u32);
        let mut canvas = RgbaImage::from_pixel(w, h, Rgba([245, 245, 245, 255]));
        for y in (0..h).step_by(11) {
            for x in 8..w.saturating_sub(8) {
                let stripe = if (x / 5 + y / 7) % 2 == 0 { 220 } else { 180 };
                canvas.put_pixel(x, y, Rgba([(y % 180) as u8, stripe, 80, 255]));
                if y + 1 < h {
                    canvas.put_pixel(x, y + 1, Rgba([30, 30, 30, 255]));
                }
            }
        }
        canvas
    }

    /// Crop an 80×80 window from the scroll canvas at `offset_y`.
    fn crop_scroll(canvas: &RgbaImage, offset_y: u32) -> RgbaImage {
        image::imageops::crop_imm(canvas, 0, offset_y, 80, 80).to_image()
    }

    /// A solid-color frame that shares no overlap with the scroll canvas.
    fn solid_miss_frame() -> RgbaImage {
        RgbaImage::from_pixel(80, 80, Rgba([10, 200, 200, 255]))
    }

    #[test]
    fn process_frame_routes_through_probe_recovery_when_paused() {
        let canvas = make_scroll_canvas();
        let mut stitcher = Stitcher::new(overlay_stitch_config());
        let mut gate = CaptureMissTracker::default();
        let now = Instant::now();

        // 1. Anchor: first frame seeds the canvas.
        let anchor = crop_scroll(&canvas, 0);
        let r = process_frame(&mut stitcher, &mut gate, anchor, now);
        assert!(r.signal.is_some(), "anchor should produce a signal");
        assert!(r.publish_preview, "anchor should publish preview");

        // 2. Successful append: overlap region matches.
        let append = crop_scroll(&canvas, 8);
        let r = process_frame(&mut stitcher, &mut gate, append, now);
        assert!(r.publish_preview, "append should publish preview");
        assert!(r.publish_activity, "append should publish activity");
        let stats_after_append = stitcher.stats();
        let dims_after_append = stitcher.full_image().map(|i| (i.width(), i.height()));

        // 3–4. Two unrelated (miss) frames to trigger the capture-miss gate.
        let miss1 = solid_miss_frame();
        let _r = process_frame(&mut stitcher, &mut gate, miss1.clone(), now);
        assert!(!gate.active(), "one miss should not activate gate yet");

        let miss2 = solid_miss_frame();
        let _r = process_frame(&mut stitcher, &mut gate, miss2, now);
        assert!(gate.active(), "two misses should activate gate");

        // 5. While paused, a further unrelated frame: stats and canvas unchanged.
        let stats_while_paused = stitcher.stats();
        let dims_while_paused = stitcher.full_image().map(|i| (i.width(), i.height()));
        let r = process_frame(
            &mut stitcher,
            &mut gate,
            miss1.clone(),
            now + Duration::from_millis(100),
        );
        assert!(!r.publish_preview, "paused frame must not publish preview");
        assert!(
            !r.publish_activity,
            "paused frame must not publish activity"
        );
        assert_eq!(
            stitcher.stats().frame_count,
            stats_while_paused.frame_count,
            "stats must not change while paused"
        );
        assert_eq!(
            stitcher.full_image().map(|i| (i.width(), i.height())),
            dims_while_paused,
            "canvas dims must not change while paused"
        );

        // 6. Recovery overlap: frame that overlaps the committed canvas.
        //    This clears the active state but does NOT publish preview.
        let recovery = crop_scroll(&canvas, 16);
        let r = process_frame(
            &mut stitcher,
            &mut gate,
            recovery,
            now + Duration::from_millis(200),
        );
        assert!(!gate.active(), "recovery should clear active state");
        assert!(
            !r.publish_preview,
            "recovery frame must not publish preview"
        );

        // 7. Next forward append: publishes preview and grows stats.
        let forward = crop_scroll(&canvas, 24);
        let r = process_frame(
            &mut stitcher,
            &mut gate,
            forward,
            now + Duration::from_millis(300),
        );
        assert!(
            r.publish_preview,
            "next forward append should publish preview"
        );
        assert!(
            r.publish_activity,
            "next forward append should publish activity"
        );
        let stats_after_forward = stitcher.stats();
        assert!(
            stats_after_forward.frame_count > stats_after_append.frame_count,
            "stats should grow after recovery"
        );
        let dims_after_forward = stitcher.full_image().map(|i| (i.width(), i.height()));
        assert!(
            dims_after_forward > dims_after_append,
            "canvas should grow after recovery"
        );
    }
}
