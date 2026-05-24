use rollshot_capture::{CapturedFrame, Region};

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::JoinHandle;

use rollshot_capture::{BackendKind, CaptureOptions, InteractiveLaunchOptions, RegionMode};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionStatus {
    Idle,
    Previewing {
        frame_width: u32,
        frame_height: u32,
        region: Option<RegionDto>,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RegionDto {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
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
    selected_region: Option<Region>,
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

        match &self.latest_frame {
            Some(frame) => SessionStatus::Previewing {
                frame_width: frame.image.width(),
                frame_height: frame.image.height(),
                region: self.selected_region.map(|region| RegionDto {
                    x: region.x,
                    y: region.y,
                    width: region.width,
                    height: region.height,
                }),
            },
            None => SessionStatus::Idle,
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
}

fn encode_preview_png(frame: &CapturedFrame, max_edge: u32) -> Result<Vec<u8>, String> {
    let max_edge = max_edge.max(1);
    let width = frame.image.width();
    let height = frame.image.height();
    let largest = width.max(height).max(1);
    let scale = (max_edge as f32 / largest as f32).min(1.0);
    let preview_width = ((width as f32 * scale).round() as u32).max(1);
    let preview_height = ((height as f32 * scale).round() as u32).max(1);

    let mut cursor = std::io::Cursor::new(Vec::new());
    if preview_width == width && preview_height == height {
        frame
            .image
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|err| format!("failed to encode preview png: {err}"))?;
    } else {
        image::imageops::resize(
            &frame.image,
            preview_width,
            preview_height,
            image::imageops::FilterType::Nearest,
        )
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|err| format!("failed to encode preview png: {err}"))?;
    }
    Ok(cursor.into_inner())
}

pub struct SharedSession {
    inner: Mutex<AppSession>,
    stop: AtomicBool,
    reader: Mutex<Option<JoinHandle<()>>>,
}

impl SharedSession {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(AppSession::new()),
            stop: AtomicBool::new(false),
            reader: Mutex::new(None),
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
            inner.selected_region = None;
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
                            inner.error = None;
                        }
                    }
                    Err(rollshot_capture::CaptureError::EndOfStream) => break,
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
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut reader) = self.reader.lock() {
            if let Some(handle) = reader.take() {
                let _ = handle.join();
            }
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
}

impl Drop for SharedSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
impl AppSession {
    pub fn store_frame_for_test(&mut self, frame: CapturedFrame) {
        self.latest_frame = Some(frame);
    }
}

#[cfg(test)]
mod tests {
    use super::{encode_preview_png, AppSession, RegionDto, SessionStatus};
    use image::{Rgba, RgbaImage};
    use rollshot_capture::{CapturedFrame, FrameMetadata};
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
}
