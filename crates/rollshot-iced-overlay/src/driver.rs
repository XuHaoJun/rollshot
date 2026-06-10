use rollshot_capture::{crop_frame, CaptureError, FrameStream, Region};
use rollshot_core::{StitchConfig, Stitcher};
use rollshot_overlay_core::capture_miss::{
    progress_signal_from_outcome, CaptureMissState, CaptureMissTracker, CapturedEdge,
    StitchProgressSignal,
};

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use iced::futures::channel::mpsc::UnboundedSender;
use iced::widget::image::Handle as ImageHandle;
use rollshot_capture::{BackendKind, CaptureOptions, CapturedFrame, RegionMode, Size};

use crate::app::PreviewConstraints;
use crate::coords::LogicalRect;

use crate::CaptureResult;

#[derive(Debug, Clone)]
pub enum LiveOverlayEvent {
    AcceptedActivity(Instant),
    Preview(ImageHandle),
    CaptureMiss(CaptureMissState),
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

/// StitchConfig the overlay uses (matches the Tauri app default,
/// session.rs:188-190).
#[allow(dead_code)]
pub fn overlay_stitch_config() -> StitchConfig {
    let mut config = StitchConfig::default();
    config.min_overlap = 32;
    config
}

/// Crop+stitch a finite frame stream to completion. This is the tested core
/// the threaded live driver (Task 5) wraps. Mirrors the crop+push+finalize of
/// session.rs:199-212,214-231.
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
    preview_tx: UnboundedSender<LiveOverlayEvent>,
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
        preview_tx: UnboundedSender<LiveOverlayEvent>,
    ) -> Result<Self, String> {
        let kind = BackendKind::from_cli_flag(backend).map_err(|e| e.to_string())?;
        let mut backend_impl = kind.create().map_err(|e| e.to_string())?;
        let stream = backend_impl
            .start(CaptureOptions {
                region: RegionMode::FullSource,
                fps,
                show_cursor,
                prefer_portal_region: false,
            })
            .map_err(|e| e.to_string())?;
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
                        Err(CaptureError::EndOfStream) => break,
                        Err(CaptureError::Timeout { .. }) => continue,
                        Err(err) => {
                            if !should_record_reader_error(&stop) {
                                break;
                            }
                            if let Ok(mut e) = shared.error.lock() {
                                *e = Some(err.to_string());
                            }
                            break;
                        }
                    }
                }
            })
        };

        let source_size = wait_for_source_size(&shared, &stop, Duration::from_secs(5))?;

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
            preview_tx,
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
                            if let Ok(mut e) = shared.error.lock() {
                                *e = Some(err.to_string());
                            }
                            break;
                        }
                    };
                    if let Ok(mut stitcher) = shared.stitcher.lock() {
                        let outcome = stitcher.push_frame(cropped.image);
                        let signal = progress_signal_from_outcome(&outcome);
                        if let StitchProgressSignal::Accepted { edge } = signal {
                            if edge != CapturedEdge::Unknown {
                                spotlight_edge = edge;
                            }
                        }
                        let capture_miss_state =
                            capture_miss_tracker.update(signal, Instant::now());
                        if should_emit_capture_miss(&capture_miss_state, last_capture_miss_active) {
                            let _ = preview_tx
                                .unbounded_send(LiveOverlayEvent::CaptureMiss(capture_miss_state));
                        }
                        last_capture_miss_active = capture_miss_state.active;
                        if should_emit_accepted_activity(&signal) {
                            let _ = preview_tx
                                .unbounded_send(LiveOverlayEvent::AcceptedActivity(Instant::now()));
                        }
                        if should_emit_preview(&signal) {
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
        }));
    }

    pub(crate) fn source_size(&self) -> Size {
        self.source_size
    }

    pub(crate) fn overlay_size(&self) -> Size {
        self.overlay_logical
    }

    /// Signal both threads to stop and join them.
    fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.stitch.take() {
            let _ = h.join();
        }
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
    }

    /// Stop capture without producing a result (user cancelled at/ before the
    /// crop, or the loop exited early). Tears the PipeWire stream down cleanly.
    pub fn cancel(mut self) {
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
        Ok(CaptureResult {
            image,
            stats: Some(stitcher.stats()),
        })
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
) -> Result<Size, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(e) = shared.error.lock() {
            if let Some(msg) = e.as_ref() {
                return Err(msg.clone());
            }
        }
        if let Ok(slot) = shared.latest.lock() {
            if let Some(frame) = slot.as_ref() {
                return Ok(frame.metadata.source_size.unwrap_or(Size {
                    width: frame.image.width(),
                    height: frame.image.height(),
                }));
            }
        }
        if Instant::now() >= deadline {
            stop.store(true, Ordering::Relaxed);
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

#[cfg(test)]
mod tests {
    use super::{
        overlay_stitch_config, preview_handle, should_emit_accepted_activity,
        should_emit_capture_miss, should_emit_preview, stitch_stream, PreviewConstraints,
    };
    use iced::widget::image::Handle as ImageHandle;
    use image::{Rgba, RgbaImage};
    use rollshot_capture::{CapturedFrame, FakeFrameStream, FrameMetadata, Region};
    use rollshot_core::{StitchOutcome, Stitcher};
    use rollshot_overlay_core::capture_miss::{CaptureMissState, StitchProgressSignal};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::SystemTime;

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
}
