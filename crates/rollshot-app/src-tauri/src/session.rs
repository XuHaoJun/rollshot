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

#[derive(Default)]
pub struct AppSession {
    latest_frame: Option<CapturedFrame>,
    latest_frame_seq: u64,
    selected_region: Option<Region>,
    stitcher: Option<Stitcher>,
    stitch_stats: StitchStatsDto,
    last_stitch_outcome: Option<String>,
    final_image: Option<RgbaImage>,
    output_path: Option<String>,
    error: Option<String>,
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
        self.last_stitch_outcome = Some(format_stitch_outcome(&outcome));
        self.stitch_stats = stitcher.stats().into();
        Ok(())
    }

    fn finish_stitching(&mut self) -> Result<DoneImageDto, String> {
        let stitcher = self
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
        self.final_image = None;
        self.output_path = None;
        self.error = None;
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

#[cfg(test)]
fn encode_rgba_png(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|err| format!("failed to encode png: {err}"))?;
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
        let inner = self
            .inner
            .lock()
            .map_err(|_| "session lock poisoned".to_string())?;
        Ok(inner.status())
    }

    pub fn confirm_region(&self, region: RegionDto) -> Result<RegionDto, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "session lock poisoned".to_string())?;
        inner.confirm_region(region)
    }

    pub fn stitch_preview_png(&self, max_edge: u32) -> Result<Option<Vec<u8>>, String> {
        let image = {
            let inner = self
                .inner
                .lock()
                .map_err(|_| "session lock poisoned".to_string())?;
            inner
                .stitcher
                .as_ref()
                .and_then(|s| s.full_image())
                .cloned()
        };
        image
            .as_ref()
            .map(|image| encode_preview_image_png(image, max_edge))
            .transpose()
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

            {
                let mut inner = self
                    .inner
                    .lock()
                    .map_err(|_| "session lock poisoned".to_string())?;
                inner.start_stitching()?;
            }

            self.stitch_stop.store(false, Ordering::Relaxed);
            let session = Arc::clone(self);
            *stitcher = Some(std::thread::spawn(move || {
                session.stitch_loop();
            }));
        }
        Ok(())
    }

    fn stitch_loop(&self) {
        let mut last_seen_seq = 0_u64;
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
    use std::sync::atomic::Ordering;
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
        let tempdir =
            std::env::temp_dir().join(format!("rollshot-app-save-image-{}", std::process::id()));
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
    fn shared_session_defaults_overlay_exclusion_to_unknown() {
        let session = SharedSession::new();

        assert_eq!(
            session.overlay_exclusion().expect("overlay exclusion"),
            OverlayExclusion::Unknown
        );
    }

    #[test]
    fn shared_session_can_store_overlay_exclusion_state() {
        let session = SharedSession::new();

        session.set_overlay_exclusion(OverlayExclusion::Unsupported);

        assert_eq!(
            session.overlay_exclusion().expect("overlay exclusion"),
            OverlayExclusion::Unsupported
        );
    }
}
