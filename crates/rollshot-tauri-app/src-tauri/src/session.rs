use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::JoinHandle;
use std::time::Duration;

use image::RgbaImage;
use rollshot_capture::{
    crop_frame, BackendKind, CaptureOptions, CapturedFrame, InteractiveLaunchOptions, Region,
    RegionMode,
};
use rollshot_core::{StitchConfig, StitchOutcome, StitchStats, Stitcher};

use rollshot_overlay_core::capture_miss::{
    progress_signal_from_outcome, CaptureMissState, CaptureMissTracker, CapturedEdge,
    StitchProgressSignal, CAPTURE_MISS_WARNING,
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionStatus {
    Idle,
    Previewing {
        frame_width: u32,
        frame_height: u32,
        region: Option<RegionDto>,
    },
    Stitching {
        frame_width: u32,
        frame_height: u32,
        region: RegionDto,
        stats: StitchStatsDto,
        last_outcome: Option<String>,
        capture_miss: bool,
        capture_miss_warning: bool,
        capture_miss_edge: rollshot_overlay_core::capture_miss::CapturedEdge,
        capture_miss_message: &'static str,
    },
    Done {
        image_width: u32,
        image_height: u32,
        output_path: Option<String>,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct StitchStatsDto {
    pub frame_count: u32,
    pub total_width: u32,
    pub total_height: u32,
    pub last_append: u32,
}

impl From<StitchStats> for StitchStatsDto {
    fn from(value: StitchStats) -> Self {
        Self {
            frame_count: value.frame_count,
            total_width: value.total_width,
            total_height: value.total_height,
            last_append: value.last_append,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DoneImageDto {
    pub image_width: u32,
    pub image_height: u32,
    pub output_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RegionDto {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum OverlayExclusion {
    Verified,
    Unsupported,
    Unknown,
}

impl From<RegionDto> for Region {
    fn from(value: RegionDto) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }
    }
}

pub struct AppSession {
    latest_frame: Option<CapturedFrame>,
    latest_frame_seq: u64,
    selected_region: Option<Region>,
    stitcher: Option<Stitcher>,
    stitch_stats: StitchStatsDto,
    last_stitch_outcome: Option<String>,
    capture_miss_tracker: CaptureMissTracker,
    capture_miss_state: CaptureMissState,
    spotlight_edge: CapturedEdge,
    final_image: Option<RgbaImage>,
    output_path: Option<String>,
    error: Option<String>,
}

impl Default for AppSession {
    fn default() -> Self {
        Self {
            latest_frame: None,
            latest_frame_seq: 0,
            selected_region: None,
            stitcher: None,
            stitch_stats: StitchStatsDto::from(StitchStats::default()),
            last_stitch_outcome: None,
            capture_miss_tracker: CaptureMissTracker::default(),
            capture_miss_state: CaptureMissState::default(),
            spotlight_edge: CapturedEdge::Unknown,
            final_image: None,
            output_path: None,
            error: None,
        }
    }
}

impl AppSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn status(&self) -> SessionStatus {
        if let Some(message) = &self.error {
            return SessionStatus::Failed {
                message: message.clone(),
            };
        }

        if let Some(image) = &self.final_image {
            return SessionStatus::Done {
                image_width: image.width(),
                image_height: image.height(),
                output_path: self.output_path.clone(),
            };
        }

        match (&self.latest_frame, self.selected_region) {
            (Some(frame), Some(region)) if self.stitcher.is_some() => SessionStatus::Stitching {
                frame_width: frame.image.width(),
                frame_height: frame.image.height(),
                region: RegionDto {
                    x: region.x,
                    y: region.y,
                    width: region.width,
                    height: region.height,
                },
                stats: self.stitch_stats,
                last_outcome: self.last_stitch_outcome.clone(),
                capture_miss: self.capture_miss_state.active,
                capture_miss_warning: self.capture_miss_state.warn,
                capture_miss_edge: self.capture_miss_state.edge,
                capture_miss_message: CAPTURE_MISS_WARNING,
            },
            (Some(frame), region) => SessionStatus::Previewing {
                frame_width: frame.image.width(),
                frame_height: frame.image.height(),
                region: region.map(|region| RegionDto {
                    x: region.x,
                    y: region.y,
                    width: region.width,
                    height: region.height,
                }),
            },
            (None, _) => SessionStatus::Idle,
        }
    }

    pub fn confirm_region(&mut self, region: RegionDto) -> Result<RegionDto, String> {
        let frame = self
            .latest_frame
            .as_ref()
            .ok_or_else(|| "cannot confirm a region before a frame is available".to_string())?;

        if region.x < 0 || region.y < 0 || region.width == 0 || region.height == 0 {
            return Err("region must have non-negative origin and non-zero size".to_string());
        }

        let right = (region.x as u64) + (region.width as u64);
        let bottom = (region.y as u64) + (region.height as u64);
        if right > frame.image.width() as u64 || bottom > frame.image.height() as u64 {
            return Err(format!(
                "region x={},y={},w={},h={} is outside frame bounds {}x{}",
                region.x,
                region.y,
                region.width,
                region.height,
                frame.image.width(),
                frame.image.height()
            ));
        }

        self.selected_region = Some(region.into());
        Ok(region)
    }

    fn start_stitching(&mut self) -> Result<(), String> {
        if self.selected_region.is_none() {
            return Err("confirm a region before starting stitching".to_string());
        }
        let mut config = StitchConfig::default();
        config.min_overlap = 32;
        self.stitcher = Some(Stitcher::new(config));
        self.stitch_stats = StitchStatsDto::from(StitchStats::default());
        self.last_stitch_outcome = None;
        self.capture_miss_tracker = CaptureMissTracker::default();
        self.capture_miss_state = CaptureMissState::default();
        self.spotlight_edge = CapturedEdge::Unknown;
        self.final_image = None;
        self.output_path = None;
        self.error = None;
        Ok(())
    }

    fn push_stitch_frame(&mut self, frame: CapturedFrame) -> Result<(), String> {
        let region = self
            .selected_region
            .ok_or_else(|| "confirm a region before stitching frames".to_string())?;
        let stitcher = self
            .stitcher
            .as_mut()
            .ok_or_else(|| "stitching has not started".to_string())?;
        let cropped = crop_frame(&frame, region).map_err(|err| err.to_string())?;
        let outcome = stitcher.push_frame(cropped.image);
        let signal = progress_signal_from_outcome(&outcome);
        if let StitchProgressSignal::Accepted { edge } = signal {
            if edge != CapturedEdge::Unknown {
                self.spotlight_edge = edge;
            }
        }
        self.capture_miss_state = self
            .capture_miss_tracker
            .update(signal, std::time::Instant::now());
        self.last_stitch_outcome = Some(format_stitch_outcome(&outcome));
        self.stitch_stats = stitcher.stats().into();
        Ok(())
    }

    fn finish_stitching(&mut self) -> Result<DoneImageDto, String> {
        let mut stitcher = self
            .stitcher
            .take()
            .ok_or_else(|| "stitching has not started".to_string())?;
        let image = stitcher
            .full_image()
            .ok_or_else(|| "stitcher produced no output".to_string())?
            .clone();
        let done = DoneImageDto {
            image_width: image.width(),
            image_height: image.height(),
            output_path: self.output_path.clone(),
        };
        self.final_image = Some(image);
        self.stitch_stats = stitcher.stats().into();
        Ok(done)
    }

    fn set_final_image(&mut self, image: RgbaImage, stats: StitchStats) -> DoneImageDto {
        let done = DoneImageDto {
            image_width: image.width(),
            image_height: image.height(),
            output_path: self.output_path.clone(),
        };
        self.final_image = Some(image);
        self.stitch_stats = StitchStatsDto::from(stats);
        self.error = None;
        done
    }

    fn save_image(&mut self, path: &Path) -> Result<DoneImageDto, String> {
        let image = self
            .final_image
            .as_ref()
            .ok_or_else(|| "no final image is available to save".to_string())?;
        image
            .save_with_format(path, image::ImageFormat::Png)
            .map_err(|err| format!("failed to save {}: {err}", path.display()))?;
        self.output_path = Some(path.to_string_lossy().to_string());
        Ok(DoneImageDto {
            image_width: image.width(),
            image_height: image.height(),
            output_path: self.output_path.clone(),
        })
    }

    fn reset_capture_state(&mut self) {
        self.latest_frame = None;
        self.latest_frame_seq = 0;
        self.selected_region = None;
        self.stitcher = None;
        self.stitch_stats = StitchStatsDto::from(StitchStats::default());
        self.last_stitch_outcome = None;
        self.capture_miss_tracker = CaptureMissTracker::default();
        self.capture_miss_state = CaptureMissState::default();
        self.spotlight_edge = CapturedEdge::Unknown;
        self.final_image = None;
        self.output_path = None;
        self.error = None;
    }

    fn clear_capture_miss_warning(&mut self) {
        self.capture_miss_state.warn = false;
    }
}

fn encode_preview_png(frame: &CapturedFrame, max_edge: u32) -> Result<Vec<u8>, String> {
    encode_preview_image_png(&frame.image, max_edge)
}

fn encode_preview_image_png(image: &RgbaImage, max_edge: u32) -> Result<Vec<u8>, String> {
    let max_edge = max_edge.max(1);
    let width = image.width();
    let height = image.height();
    let largest = width.max(height).max(1);
    let scale = (max_edge as f32 / largest as f32).min(1.0);
    let preview_width = ((width as f32 * scale).round() as u32).max(1);
    let preview_height = ((height as f32 * scale).round() as u32).max(1);

    let mut cursor = std::io::Cursor::new(Vec::new());
    if preview_width == width && preview_height == height {
        image
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|err| format!("failed to encode preview png: {err}"))?;
    } else {
        image::imageops::resize(
            image,
            preview_width,
            preview_height,
            image::imageops::FilterType::Nearest,
        )
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|err| format!("failed to encode preview png: {err}"))?;
    }
    Ok(cursor.into_inner())
}

fn encode_rgba_png(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|err| format!("failed to encode preview png: {err}"))?;
    Ok(cursor.into_inner())
}

fn format_stitch_outcome(outcome: &StitchOutcome) -> String {
    match outcome {
        StitchOutcome::FirstFrame => "first frame".to_string(),
        StitchOutcome::Appended {
            direction, added, ..
        } => {
            format!("appended {added}px {direction:?}")
        }
        StitchOutcome::NoProgress { .. } => "no progress".to_string(),
        StitchOutcome::Duplicate => "duplicate frame".to_string(),
        StitchOutcome::NoMatch { reason, .. } => format!("no match: {reason:?}"),
        StitchOutcome::AxisChanged {
            previous_axis,
            new_axis,
            ..
        } => format!("axis changed from {previous_axis:?} to {new_axis:?}"),
    }
}

pub struct SharedSession {
    inner: Mutex<AppSession>,
    stop: AtomicBool,
    reader: Mutex<Option<JoinHandle<()>>>,
    stitch_stop: AtomicBool,
    stitcher: Mutex<Option<JoinHandle<()>>>,
    overlay_exclusion: Mutex<OverlayExclusion>,
}

impl SharedSession {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(AppSession::new()),
            stop: AtomicBool::new(false),
            reader: Mutex::new(None),
            stitch_stop: AtomicBool::new(false),
            stitcher: Mutex::new(None),
            overlay_exclusion: Mutex::new(OverlayExclusion::Unknown),
        }
    }

    pub fn start_capture(
        self: &Arc<Self>,
        options: InteractiveLaunchOptions,
    ) -> Result<(), String> {
        let mut reader = self
            .reader
            .lock()
            .map_err(|_| "reader lock poisoned".to_string())?;
        if reader.is_some() {
            return Err("capture is already running".to_string());
        }

        {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| "session lock poisoned".to_string())?;
            inner.latest_frame = None;
            inner.latest_frame_seq = 0;
            inner.selected_region = None;
            inner.stitcher = None;
            inner.stitch_stats = StitchStatsDto::from(StitchStats::default());
            inner.last_stitch_outcome = None;
            inner.final_image = None;
            inner.output_path = None;
            inner.error = None;
            inner.capture_miss_tracker = CaptureMissTracker::default();
            inner.capture_miss_state = CaptureMissState::default();
            inner.spotlight_edge = CapturedEdge::Unknown;
        }

        self.start_reader(options, &mut reader);
        Ok(())
    }

    fn start_reader(
        self: &Arc<Self>,
        options: InteractiveLaunchOptions,
        reader_slot: &mut Option<JoinHandle<()>>,
    ) {
        self.stop.store(false, Ordering::Relaxed);
        let session = Arc::clone(self);
        *reader_slot = Some(std::thread::spawn(move || {
            let kind = match BackendKind::from_cli_flag(&options.backend) {
                Ok(kind) => kind,
                Err(err) => {
                    session.store_error(err.to_string());
                    return;
                }
            };
            let mut backend = match kind.create() {
                Ok(backend) => backend,
                Err(err) => {
                    session.store_error(err.to_string());
                    return;
                }
            };
            let capture_options = CaptureOptions {
                region: RegionMode::FullSource,
                fps: options.fps,
                show_cursor: options.show_cursor,
                prefer_portal_region: false,
                target_display_id: None,
                target_output_name: None,
            };
            let mut stream = match backend.start(capture_options) {
                Ok(stream) => stream,
                Err(err) => {
                    session.store_error(err.to_string());
                    return;
                }
            };

            while !session.stop.load(Ordering::Relaxed) {
                match stream.next_frame() {
                    Ok(frame) => {
                        if let Ok(mut inner) = session.inner.lock() {
                            inner.latest_frame = Some(frame);
                            inner.latest_frame_seq = inner.latest_frame_seq.wrapping_add(1);
                            inner.error = None;
                        }
                    }
                    Err(rollshot_capture::CaptureError::EndOfStream) => break,
                    Err(rollshot_capture::CaptureError::Timeout { .. }) => continue,
                    Err(err) => {
                        if let Ok(mut inner) = session.inner.lock() {
                            inner.error = Some(err.to_string());
                        }
                        break;
                    }
                }
            }
        }));
    }

    fn store_error(&self, message: String) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.error = Some(message);
        }
    }

    pub fn stop_capture(&self) {
        self.stitch_stop.store(true, Ordering::Relaxed);
        if let Ok(mut stitcher) = self.stitcher.lock() {
            if let Some(handle) = stitcher.take() {
                let _ = handle.join();
            }
        }

        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut reader) = self.reader.lock() {
            if let Some(handle) = reader.take() {
                let _ = handle.join();
            }
        }

        if let Ok(mut inner) = self.inner.lock() {
            inner.reset_capture_state();
        }
    }

    pub fn status(&self) -> Result<SessionStatus, String> {
        // NOTE (R8): single consumer only (the `session_status` command). The
        // capture-miss `warn` flag is a one-shot pulse cleared on read; a second
        // poller would swallow the pulse before the frontend sees it.
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "session lock poisoned".to_string())?;
        let status = inner.status();
        inner.clear_capture_miss_warning();
        Ok(status)
    }

    pub fn confirm_region(&self, region: RegionDto) -> Result<RegionDto, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "session lock poisoned".to_string())?;
        inner.confirm_region(region)
    }

    pub fn stitch_preview_png(
        &self,
        preview_width: u32,
        preview_height: u32,
    ) -> Result<Option<Vec<u8>>, String> {
        let preview = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| "session lock poisoned".to_string())?;
            let region = inner.selected_region;
            let edge = inner.spotlight_edge;
            let preview = match inner.stitcher.as_mut() {
                Some(stitcher) => region.and_then(|region| {
                    if matches!(edge, CapturedEdge::Left | CapturedEdge::Right) {
                        rollshot_overlay_core::preview::viewport_preview(
                            stitcher,
                            rollshot_overlay_core::preview::ViewportPreviewRequest {
                                viewport_width: preview_width,
                                viewport_height: preview_height,
                                frame_width: region.width,
                                frame_height: region.height,
                                edge,
                            },
                        )
                        .map(|preview| (preview.width, preview.height, preview.pixels))
                    } else {
                        rollshot_overlay_core::preview::growing_preview(
                            stitcher,
                            rollshot_overlay_core::preview::GrowingPreviewRequest {
                                fixed_width: preview_width,
                                max_height: preview_height,
                                edge,
                            },
                        )
                        .map(|preview| (preview.width, preview.height, preview.pixels))
                    }
                }),
                None => None,
            };
            preview
        };

        match preview {
            Some((width, height, pixels)) => {
                let image = RgbaImage::from_raw(width, height, pixels)
                    .ok_or_else(|| "invalid stitch preview buffer".to_string())?;
                Ok(Some(encode_rgba_png(&image)?))
            }
            None => Ok(None),
        }
    }

    pub fn latest_preview_png(&self, max_edge: u32) -> Result<Option<Vec<u8>>, String> {
        let frame = {
            let inner = self
                .inner
                .lock()
                .map_err(|_| "session lock poisoned".to_string())?;
            inner.latest_frame.clone()
        };

        frame
            .as_ref()
            .map(|frame| encode_preview_png(frame, max_edge))
            .transpose()
    }

    pub fn start_stitching(self: &Arc<Self>) -> Result<(), String> {
        {
            let mut stitcher = self
                .stitcher
                .lock()
                .map_err(|_| "stitcher lock poisoned".to_string())?;
            if stitcher.is_some() {
                return Err("stitching is already running".to_string());
            }

            let start_seq = {
                let mut inner = self
                    .inner
                    .lock()
                    .map_err(|_| "session lock poisoned".to_string())?;
                inner.start_stitching()?;
                inner.latest_frame_seq
            };

            self.stitch_stop.store(false, Ordering::Relaxed);
            let session = Arc::clone(self);
            *stitcher = Some(std::thread::spawn(move || {
                session.stitch_loop(start_seq);
            }));
        }
        Ok(())
    }

    fn stitch_loop(&self, mut last_seen_seq: u64) {
        while !self.stitch_stop.load(Ordering::Relaxed) {
            let next_frame = {
                let inner = match self.inner.lock() {
                    Ok(inner) => inner,
                    Err(_) => return,
                };
                if inner.latest_frame_seq == last_seen_seq {
                    None
                } else {
                    last_seen_seq = inner.latest_frame_seq;
                    inner.latest_frame.clone()
                }
            };

            if let Some(frame) = next_frame {
                if let Ok(mut inner) = self.inner.lock() {
                    if let Err(err) = inner.push_stitch_frame(frame) {
                        inner.error = Some(err);
                        break;
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn stop_stitching(&self) -> Result<DoneImageDto, String> {
        self.stitch_stop.store(true, Ordering::Relaxed);
        if let Ok(mut stitcher) = self.stitcher.lock() {
            if let Some(handle) = stitcher.take() {
                let _ = handle.join();
            }
        }

        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut reader) = self.reader.lock() {
            if let Some(handle) = reader.take() {
                let _ = handle.join();
            }
        }

        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "session lock poisoned".to_string())?;
        inner.finish_stitching()
    }

    pub fn save_image(&self, path: &Path) -> Result<DoneImageDto, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "session lock poisoned".to_string())?;
        inner.save_image(path)
    }

    pub fn store_capture_result(
        &self,
        image: RgbaImage,
        stats: StitchStats,
    ) -> Result<DoneImageDto, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "session lock poisoned".to_string())?;
        Ok(inner.set_final_image(image, stats))
    }

    pub fn final_preview_png(&self, max_edge: u32) -> Result<Option<Vec<u8>>, String> {
        let image = {
            let inner = self
                .inner
                .lock()
                .map_err(|_| "session lock poisoned".to_string())?;
            inner.final_image.clone()
        };
        image
            .as_ref()
            .map(|image| encode_preview_image_png(image, max_edge))
            .transpose()
    }

    pub fn overlay_exclusion(&self) -> Result<OverlayExclusion, String> {
        self.overlay_exclusion
            .lock()
            .map(|state| *state)
            .map_err(|_| "overlay exclusion lock poisoned".to_string())
    }

    #[cfg(not(target_os = "linux"))]
    pub fn set_overlay_exclusion(&self, state: OverlayExclusion) {
        if let Ok(mut current) = self.overlay_exclusion.lock() {
            *current = state;
        }
    }
}

impl Drop for SharedSession {
    fn drop(&mut self) {
        self.stitch_stop.store(true, Ordering::Relaxed);
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
impl AppSession {
    pub fn store_frame_for_test(&mut self, frame: CapturedFrame) {
        self.latest_frame = Some(frame);
    }

    pub fn start_stitching_for_test(&mut self) -> Result<(), String> {
        self.start_stitching()
    }

    pub fn push_stitch_frame_for_test(&mut self, frame: CapturedFrame) -> Result<(), String> {
        self.push_stitch_frame(frame)
    }

    pub fn finish_stitching_for_test(&mut self) -> Result<DoneImageDto, String> {
        self.finish_stitching()
    }

    pub fn save_image_for_test(&mut self, path: &Path) -> Result<DoneImageDto, String> {
        self.save_image(path)
    }

    pub fn final_image_png_for_test(&self) -> Option<Vec<u8>> {
        self.final_image
            .as_ref()
            .and_then(|image| encode_rgba_png(image).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        encode_preview_png, AppSession, OverlayExclusion, RegionDto, SessionStatus, SharedSession,
    };
    use image::{Rgba, RgbaImage};
    use rollshot_capture::{CapturedFrame, FrameMetadata};
    use rollshot_overlay_core::capture_miss::{CapturedEdge, CAPTURE_MISS_WARNING};
    use rollshot_overlay_core::preview::{PREVIEW_MAX_HEIGHT, PREVIEW_WIDTH};
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::time::SystemTime;

    fn make_test_frame(width: u32, height: u32) -> CapturedFrame {
        CapturedFrame {
            image: RgbaImage::from_pixel(width, height, Rgba([10, 20, 30, 255])),
            timestamp: SystemTime::UNIX_EPOCH,
            metadata: FrameMetadata::fake(),
        }
    }

    #[test]
    fn status_reports_latest_frame_size() {
        let mut session = AppSession::new();
        session.store_frame_for_test(make_test_frame(320, 200));

        assert_eq!(
            session.status(),
            SessionStatus::Previewing {
                frame_width: 320,
                frame_height: 200,
                region: None
            }
        );
    }

    #[test]
    fn confirm_region_rejects_region_outside_frame() {
        let mut session = AppSession::new();
        session.store_frame_for_test(make_test_frame(320, 200));

        let err = session
            .confirm_region(RegionDto {
                x: 300,
                y: 10,
                width: 40,
                height: 40,
            })
            .expect_err("region outside frame");

        assert!(err.contains("outside frame bounds"), "err = {err}");
    }

    #[test]
    fn confirm_region_rejects_overflowing_bounds_without_panic() {
        let mut session = AppSession::new();
        session.store_frame_for_test(make_test_frame(320, 200));

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.confirm_region(RegionDto {
                x: i32::MAX,
                y: 10,
                width: u32::MAX,
                height: 40,
            })
        }));

        let err = result
            .expect("region validation should not panic")
            .expect_err("overflowing region should be rejected");
        assert!(err.contains("outside frame bounds"), "err = {err}");
    }

    #[test]
    fn confirm_region_stores_source_pixel_region() {
        let mut session = AppSession::new();
        session.store_frame_for_test(make_test_frame(320, 200));

        let region = session
            .confirm_region(RegionDto {
                x: 10,
                y: 12,
                width: 100,
                height: 80,
            })
            .expect("valid region");

        assert_eq!(region.x, 10);
        assert_eq!(region.y, 12);
        assert_eq!(region.width, 100);
        assert_eq!(region.height, 80);
    }

    #[test]
    fn latest_preview_png_resizes_large_frame() {
        let mut session = AppSession::new();
        session.store_frame_for_test(make_test_frame(800, 400));

        let bytes = encode_preview_png(session.latest_frame.as_ref().expect("preview exists"), 200)
            .expect("encode preview");
        let image = image::load_from_memory(&bytes).expect("decode png");

        assert_eq!(image.width(), 200);
        assert_eq!(image.height(), 100);
    }

    #[test]
    fn stitch_preview_png_uses_growing_vertical_preview_height() {
        let session = SharedSession::new();
        {
            let mut inner = session.inner.lock().expect("session lock");
            inner.store_frame_for_test(make_test_frame(960, 600));
            inner
                .confirm_region(RegionDto {
                    x: 0,
                    y: 0,
                    width: 960,
                    height: 600,
                })
                .expect("confirm region");
            inner.start_stitching().expect("start stitching");
            inner
                .push_stitch_frame(make_test_frame(960, 600))
                .expect("push frame");
        }

        let bytes = session
            .stitch_preview_png(PREVIEW_WIDTH, PREVIEW_MAX_HEIGHT)
            .expect("encode stitch preview")
            .expect("preview exists");
        let image = image::load_from_memory(&bytes).expect("decode png");

        assert_eq!(image.width(), PREVIEW_WIDTH);
        assert_eq!(image.height(), 175);
    }

    #[test]
    fn status_reports_error_when_set() {
        let mut session = AppSession::new();
        session.store_frame_for_test(make_test_frame(320, 200));
        session.error = Some("capture backend crashed".to_string());

        assert_eq!(
            session.status(),
            SessionStatus::Failed {
                message: "capture backend crashed".to_string()
            }
        );
    }

    fn scrolling_frame(y_offset: u8) -> CapturedFrame {
        let canvas_height: u32 = 200;
        let canvas_width: u32 = 80;
        let mut canvas =
            RgbaImage::from_pixel(canvas_width, canvas_height, Rgba([245, 245, 245, 255]));
        for y in (0u32..canvas_height).step_by(11) {
            let accent = ((y / 3) % 180) as u8;
            for x in 8..canvas_width.saturating_sub(8) {
                let stripe = if (x / 5 + y / 7) % 2 == 0 {
                    220u8
                } else {
                    180u8
                };
                canvas.put_pixel(x, y, Rgba([accent, stripe, 80, 255]));
                if y + 1 < canvas_height {
                    canvas.put_pixel(x, y + 1, Rgba([30, 30, 30, 255]));
                }
            }
        }
        for col in [21u32, 47, 73] {
            if col >= canvas_width {
                continue;
            }
            for y in 12..canvas_height.saturating_sub(12) {
                if (y / 13) % 3 != 0 {
                    canvas.put_pixel(col, y, Rgba([20, 20, 20, 255]));
                }
            }
        }
        let offset_y = y_offset as u32;
        let image = image::imageops::crop_imm(&canvas, 0, offset_y, 80, 80).to_image();
        CapturedFrame {
            image,
            timestamp: SystemTime::UNIX_EPOCH,
            metadata: FrameMetadata::fake(),
        }
    }

    fn blank_frame(width: u32, height: u32) -> CapturedFrame {
        CapturedFrame {
            image: RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255])),
            timestamp: SystemTime::UNIX_EPOCH,
            metadata: FrameMetadata::fake(),
        }
    }

    fn stitch_frame_count(session: &SharedSession) -> u32 {
        match session.status().expect("status") {
            SessionStatus::Stitching { stats, .. } => stats.frame_count,
            other => panic!("expected stitching status, got {other:?}"),
        }
    }

    fn wait_for_stitch_frame_count(
        session: &SharedSession,
        expected: u32,
        timeout: std::time::Duration,
    ) {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let actual = stitch_frame_count(session);
            if actual == expected {
                return;
            }
            if std::time::Instant::now() >= deadline {
                assert_eq!(actual, expected, "stitch frame count before timeout");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn start_stitching_requires_confirmed_region() {
        let mut session = AppSession::new();
        session.store_frame_for_test(make_test_frame(320, 200));

        let err = session
            .start_stitching_for_test()
            .expect_err("missing region rejected");

        assert!(err.contains("confirm a region"), "err = {err}");
    }

    #[test]
    fn push_stitch_frame_crops_to_selected_region_and_updates_stats() {
        let mut session = AppSession::new();
        session.store_frame_for_test(scrolling_frame(0));
        session
            .confirm_region(RegionDto {
                x: 10,
                y: 10,
                width: 60,
                height: 60,
            })
            .expect("confirm region");
        session.start_stitching_for_test().expect("start stitching");

        session
            .push_stitch_frame_for_test(scrolling_frame(0))
            .expect("first frame");
        session
            .push_stitch_frame_for_test(scrolling_frame(8))
            .expect("second frame");

        let status = session.status();
        match status {
            SessionStatus::Stitching { stats, .. } => {
                assert_eq!(stats.frame_count, 2);
                assert_eq!(stats.total_width, 60);
                assert!(stats.total_height >= 60);
            }
            other => panic!("expected stitching status, got {other:?}"),
        }
    }

    #[test]
    fn finish_stitching_keeps_final_image_in_session() {
        let mut session = AppSession::new();
        session.store_frame_for_test(scrolling_frame(0));
        session
            .confirm_region(RegionDto {
                x: 0,
                y: 0,
                width: 80,
                height: 80,
            })
            .expect("confirm region");
        session.start_stitching_for_test().expect("start stitching");
        session
            .push_stitch_frame_for_test(scrolling_frame(0))
            .expect("first frame");

        let done = session.finish_stitching_for_test().expect("finish");

        assert_eq!(done.image_width, 80);
        assert_eq!(done.image_height, 80);
        assert!(session.final_image_png_for_test().is_some());
    }

    #[test]
    fn save_image_writes_final_png() {
        let tempdir = std::env::temp_dir().join(format!(
            "rollshot-tauri-app-save-image-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&tempdir).expect("create tempdir");
        let output = tempdir.join("stitched.png");

        let mut session = AppSession::new();
        session.store_frame_for_test(scrolling_frame(0));
        session
            .confirm_region(RegionDto {
                x: 0,
                y: 0,
                width: 80,
                height: 80,
            })
            .expect("confirm region");
        session.start_stitching_for_test().expect("start stitching");
        session
            .push_stitch_frame_for_test(scrolling_frame(0))
            .expect("first frame");
        session.finish_stitching_for_test().expect("finish");

        let done = session.save_image_for_test(&output).expect("save png");

        assert_eq!(done.output_path, Some(output.to_string_lossy().to_string()));
        let decoded = image::open(&output).expect("decode saved png");
        assert_eq!(decoded.width(), 80);
        let _ = std::fs::remove_dir_all(&tempdir);
    }

    #[test]
    fn store_capture_result_sets_done_image() {
        use rollshot_core::StitchStats;

        let session = SharedSession::new();
        let image = RgbaImage::from_pixel(40, 90, Rgba([1, 2, 3, 255]));

        let done = session
            .store_capture_result(image, StitchStats::default())
            .expect("store capture result");

        assert_eq!(done.image_width, 40);
        assert_eq!(done.image_height, 90);
        assert_eq!(done.output_path, None);

        match session.status().expect("status") {
            SessionStatus::Done {
                image_width,
                image_height,
                output_path,
            } => {
                assert_eq!(image_width, 40);
                assert_eq!(image_height, 90);
                assert_eq!(output_path, None);
            }
            other => panic!("expected done status, got {other:?}"),
        }
    }

    #[test]
    fn store_capture_result_then_save_writes_png() {
        use rollshot_core::StitchStats;

        let dir = std::env::temp_dir().join(format!("rollshot-native-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create tempdir");
        let out = dir.join("native.png");

        let session = SharedSession::new();
        session
            .store_capture_result(
                RgbaImage::from_pixel(60, 120, Rgba([9, 9, 9, 255])),
                StitchStats::default(),
            )
            .expect("store capture result");

        let saved = session.save_image(&out).expect("save png");

        assert_eq!(saved.output_path, Some(out.to_string_lossy().to_string()));
        let decoded = image::open(&out).expect("decode saved png");
        assert_eq!(decoded.width(), 60);
        assert_eq!(decoded.height(), 120);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stop_capture_resets_preview_state_for_restart() {
        let session = SharedSession::new();
        {
            let mut inner = session.inner.lock().expect("session lock");
            inner.store_frame_for_test(make_test_frame(320, 200));
            inner
                .confirm_region(RegionDto {
                    x: 10,
                    y: 10,
                    width: 80,
                    height: 80,
                })
                .expect("confirm region");
        }

        session.stop_capture();

        assert_eq!(session.status().expect("status"), SessionStatus::Idle);
    }

    #[test]
    fn stop_stitching_stops_capture_reader_and_keeps_final_image() {
        let session = SharedSession::new();
        session.stop.store(false, Ordering::Relaxed);
        {
            let mut inner = session.inner.lock().expect("session lock");
            inner.store_frame_for_test(scrolling_frame(0));
            inner
                .confirm_region(RegionDto {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 80,
                })
                .expect("confirm region");
            inner.start_stitching().expect("start stitching");
            inner
                .push_stitch_frame(scrolling_frame(0))
                .expect("first frame");
        }

        let done = session.stop_stitching().expect("stop stitching");

        assert!(session.stop.load(Ordering::Relaxed));
        assert_eq!(done.image_width, 80);
        assert_eq!(
            session.status().expect("status"),
            SessionStatus::Done {
                image_width: 80,
                image_height: 80,
                output_path: None,
            }
        );
    }

    #[test]
    fn shared_start_stitching_skips_stale_latest_frame() {
        let session = Arc::new(SharedSession::new());
        {
            let mut inner = session.inner.lock().expect("session lock");
            inner.store_frame_for_test(scrolling_frame(0));
            inner.latest_frame_seq = 1;
            inner
                .confirm_region(RegionDto {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 80,
                })
                .expect("confirm region");
        }

        session.start_stitching().expect("start stitching");
        wait_for_stitch_frame_count(&session, 0, std::time::Duration::from_millis(250));

        {
            let mut inner = session.inner.lock().expect("session lock");
            inner.latest_frame = Some(scrolling_frame(8));
            inner.latest_frame_seq = 2;
        }
        wait_for_stitch_frame_count(&session, 1, std::time::Duration::from_secs(1));

        session.stop_capture();
    }

    #[test]
    fn shared_session_defaults_overlay_exclusion_to_unknown() {
        let session = SharedSession::new();

        assert_eq!(
            session.overlay_exclusion().expect("overlay exclusion"),
            OverlayExclusion::Unknown
        );
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn shared_session_can_store_overlay_exclusion_state() {
        let session = SharedSession::new();

        session.set_overlay_exclusion(OverlayExclusion::Unsupported);

        assert_eq!(
            session.overlay_exclusion().expect("overlay exclusion"),
            OverlayExclusion::Unsupported
        );
    }

    #[test]
    fn status_reports_capture_miss_after_no_match() {
        let mut session = AppSession::new();
        session.store_frame_for_test(scrolling_frame(0));
        session
            .confirm_region(RegionDto {
                x: 0,
                y: 0,
                width: 80,
                height: 80,
            })
            .expect("confirm region");
        session.start_stitching_for_test().expect("start stitching");

        session
            .push_stitch_frame_for_test(scrolling_frame(0))
            .expect("first frame");
        session
            .push_stitch_frame_for_test(blank_frame(80, 80))
            .expect("miss frame");

        match session.status() {
            SessionStatus::Stitching {
                capture_miss,
                capture_miss_warning,
                capture_miss_edge,
                capture_miss_message,
                ..
            } => {
                assert!(capture_miss);
                assert!(capture_miss_warning);
                assert_eq!(capture_miss_edge, CapturedEdge::Unknown);
                assert_eq!(capture_miss_message, CAPTURE_MISS_WARNING);
            }
            other => panic!("expected stitching status, got {other:?}"),
        }
    }

    #[test]
    fn stitch_preview_png_returns_requested_viewport_size() {
        let session = SharedSession::new();
        {
            let mut inner = session.inner.lock().expect("session lock");
            inner.store_frame_for_test(blank_frame(80, 80));
            inner
                .confirm_region(RegionDto {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 80,
                })
                .expect("confirm region");
            inner.start_stitching().expect("start stitching");
            inner.push_stitch_frame(scrolling_frame(0)).expect("f0");
            inner.push_stitch_frame(scrolling_frame(20)).expect("f1");
            inner.push_stitch_frame(scrolling_frame(40)).expect("f2");
        }

        let bytes = session
            .stitch_preview_png(180, 260)
            .expect("encode stitch preview")
            .expect("preview exists");
        let image = image::load_from_memory(&bytes).expect("decode png");

        assert_eq!((image.width(), image.height()), (180, 260));
    }

    #[test]
    fn horizontal_stitch_preview_keeps_requested_viewport_size() {
        let session = SharedSession::new();
        {
            let mut inner = session.inner.lock().expect("session lock");
            inner.store_frame_for_test(blank_frame(80, 80));
            inner
                .confirm_region(RegionDto {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 80,
                })
                .expect("confirm region");
            inner.start_stitching().expect("start stitching");
            inner.push_stitch_frame(scrolling_frame(0)).expect("f0");
            inner.spotlight_edge = CapturedEdge::Right;
        }

        let bytes = session
            .stitch_preview_png(180, 260)
            .expect("encode stitch preview")
            .expect("preview exists");
        let image = image::load_from_memory(&bytes).expect("decode png");

        assert_eq!((image.width(), image.height()), (180, 260));
    }

    #[test]
    fn accepted_frame_clears_capture_miss_status() {
        let mut session = AppSession::new();
        session.store_frame_for_test(scrolling_frame(0));
        session
            .confirm_region(RegionDto {
                x: 0,
                y: 0,
                width: 80,
                height: 80,
            })
            .expect("confirm region");
        session.start_stitching_for_test().expect("start stitching");

        session
            .push_stitch_frame_for_test(scrolling_frame(0))
            .expect("first frame");
        session
            .push_stitch_frame_for_test(blank_frame(80, 80))
            .expect("miss frame");
        session
            .push_stitch_frame_for_test(scrolling_frame(8))
            .expect("recovered frame");

        match session.status() {
            SessionStatus::Stitching { capture_miss, .. } => {
                assert!(!capture_miss);
            }
            other => panic!("expected stitching status, got {other:?}"),
        }
    }
}
