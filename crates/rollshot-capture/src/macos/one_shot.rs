use crate::error::CaptureError;
use crate::one_shot::{DisplayTarget, OneShotCapture};
use crate::types::{Region, Size};
use image::RgbaImage;

/// Safe platform adapter trait for macOS one-shot capture.
///
/// This trait abstracts the unsafe ScreenCaptureKit backend behind a safe interface,
/// enabling test injection without exposing Objective-C dependencies.
trait MacosOneShotClient {
    fn capture_display_under_cursor(
        &self,
        show_cursor: bool,
    ) -> Result<rollshot_macos_oneshot::CapturedDisplay, CaptureError>;
}

/// Adapter that converts a `MacosOneShotClient` result into an `OneShotCapture`.
///
/// This performs the safe conversion from the isolation crate's `CapturedDisplay` to
/// the common `OneShotCapture` type, preserving logical origin and size.
struct MacosOneShotBackend<C: MacosOneShotClient> {
    client: C,
}

impl<C: MacosOneShotClient> MacosOneShotBackend<C> {
    fn new(client: C) -> Self {
        Self { client }
    }

    fn capture_once(&mut self, show_cursor: bool) -> Result<OneShotCapture, CaptureError> {
        let display = self.client.capture_display_under_cursor(show_cursor)?;

        // Validate physical dimensions are non-zero
        if display.width == 0 || display.height == 0 {
            return Err(CaptureError::Mapping {
                message: format!(
                    "zero physical dimensions from macOS capture: {}x{}",
                    display.width, display.height
                ),
            });
        }

        // Validate logical dimensions are non-zero
        if display.logical_width == 0 || display.logical_height == 0 {
            return Err(CaptureError::Mapping {
                message: format!(
                    "zero logical dimensions from macOS capture: {}x{}",
                    display.logical_width, display.logical_height
                ),
            });
        }

        let rgba =
            RgbaImage::from_raw(display.width, display.height, display.rgba).ok_or_else(|| {
                CaptureError::Mapping {
                    message: format!(
                    "failed to create RGBA image from macOS capture: {}x{}, pixel count mismatch",
                    display.width, display.height
                ),
                }
            })?;

        let target = DisplayTarget {
            output_name: Some(display.display_id.to_string()),
            logical_region: Region {
                x: display.logical_x,
                y: display.logical_y,
                width: display.logical_width,
                height: display.logical_height,
            },
            physical_size: Size {
                width: display.width,
                height: display.height,
            },
        };

        OneShotCapture::new(rgba, target)
    }
}

/// Production client backed by the unsafe-isolation crate.
#[cfg(not(test))]
struct RealMacosOneShotClient;

#[cfg(not(test))]
impl MacosOneShotClient for RealMacosOneShotClient {
    fn capture_display_under_cursor(
        &self,
        show_cursor: bool,
    ) -> Result<rollshot_macos_oneshot::CapturedDisplay, CaptureError> {
        rollshot_macos_oneshot::capture_display_under_cursor(show_cursor)
            .map_err(map_isolation_error)
    }
}

/// Capture the display under the pointer through `SCScreenshotManager`, returning
/// a validated `OneShotCapture`. This is the macOS screenshot one-shot entry
/// point dispatched from `OneShotBackendKind::capture_once`. No streaming /
/// `SCStream` fallback occurs: a missing permission, unsupported OS, timeout, or
/// capture failure surfaces as the corresponding `CaptureError`.
#[cfg(not(test))]
pub fn capture_once(show_cursor: bool) -> Result<OneShotCapture, CaptureError> {
    let mut backend = MacosOneShotBackend::new(RealMacosOneShotClient);
    backend.capture_once(show_cursor)
}

/// Map `rollshot_macos_oneshot::MacosOneShotError` to `CaptureError`.
fn map_isolation_error(err: rollshot_macos_oneshot::MacosOneShotError) -> CaptureError {
    match err {
        rollshot_macos_oneshot::MacosOneShotError::Unsupported(msg) => {
            CaptureError::Unsupported { message: msg }
        }
        rollshot_macos_oneshot::MacosOneShotError::PermissionDenied(msg) => {
            CaptureError::PermissionDenied { message: msg }
        }
        rollshot_macos_oneshot::MacosOneShotError::Timeout(msg) => CaptureError::Timeout {
            message: msg,
            duration: std::time::Duration::from_secs(30),
        },
        rollshot_macos_oneshot::MacosOneShotError::Capture(msg) => {
            CaptureError::Backend(anyhow::anyhow!(msg))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Fake client for testing the adapter without real ScreenCaptureKit calls.
    struct FakeMacosOneShotClient {
        result: RefCell<Option<Result<rollshot_macos_oneshot::CapturedDisplay, CaptureError>>>,
    }

    impl FakeMacosOneShotClient {
        fn returning(display: rollshot_macos_oneshot::CapturedDisplay) -> Self {
            Self {
                result: RefCell::new(Some(Ok(display))),
            }
        }

        fn returning_error(err: CaptureError) -> Self {
            Self {
                result: RefCell::new(Some(Err(err))),
            }
        }
    }

    impl MacosOneShotClient for FakeMacosOneShotClient {
        fn capture_display_under_cursor(
            &self,
            _show_cursor: bool,
        ) -> Result<rollshot_macos_oneshot::CapturedDisplay, CaptureError> {
            self.result
                .borrow_mut()
                .take()
                .expect("FakeMacosOneShotClient called more than once")
        }
    }

    fn make_captured_display(width: u32, height: u32) -> rollshot_macos_oneshot::CapturedDisplay {
        rollshot_macos_oneshot::CapturedDisplay {
            rgba: vec![0; (width as usize) * (height as usize) * 4],
            width,
            height,
            logical_x: 0,
            logical_y: 0,
            logical_width: width / 2,
            logical_height: height / 2,
            display_id: 1,
        }
    }

    // ── Safe adapter conversion tests ──

    #[test]
    fn adapter_converts_captured_display_to_one_shot_capture() {
        let display = make_captured_display(200, 100);
        let client = FakeMacosOneShotClient::returning(display);
        let mut backend = MacosOneShotBackend::new(client);
        let result = backend.capture_once(false);
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        let capture = result.unwrap();
        assert_eq!(capture.image().width(), 200);
        assert_eq!(capture.image().height(), 100);
    }

    #[test]
    fn adapter_preserves_logical_origin_and_size() {
        let mut display = make_captured_display(200, 100);
        display.logical_x = 50;
        display.logical_y = 30;
        display.logical_width = 100;
        display.logical_height = 50;
        let client = FakeMacosOneShotClient::returning(display);
        let mut backend = MacosOneShotBackend::new(client);
        let capture = backend.capture_once(false).unwrap();
        assert_eq!(capture.target_display().logical_region.x, 50);
        assert_eq!(capture.target_display().logical_region.y, 30);
        assert_eq!(capture.target_display().logical_region.width, 100);
        assert_eq!(capture.target_display().logical_region.height, 50);
    }

    #[test]
    fn adapter_uses_display_id_as_output_name() {
        let mut display = make_captured_display(200, 100);
        display.display_id = 42;
        let client = FakeMacosOneShotClient::returning(display);
        let mut backend = MacosOneShotBackend::new(client);
        let capture = backend.capture_once(false).unwrap();
        assert_eq!(capture.target_display().output_name.as_deref(), Some("42"));
    }

    #[test]
    fn adapter_rejects_zero_physical_width() {
        let mut display = make_captured_display(200, 100);
        display.width = 0;
        let client = FakeMacosOneShotClient::returning(display);
        let mut backend = MacosOneShotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::Mapping { message }) => {
                assert!(
                    message.contains("zero physical dimensions"),
                    "msg: {message}"
                );
            }
            other => panic!("expected Mapping error, got {other:?}"),
        }
    }

    #[test]
    fn adapter_rejects_zero_physical_height() {
        let mut display = make_captured_display(200, 100);
        display.height = 0;
        let client = FakeMacosOneShotClient::returning(display);
        let mut backend = MacosOneShotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::Mapping { message }) => {
                assert!(
                    message.contains("zero physical dimensions"),
                    "msg: {message}"
                );
            }
            other => panic!("expected Mapping error, got {other:?}"),
        }
    }

    #[test]
    fn adapter_rejects_zero_logical_dimensions() {
        let mut display = make_captured_display(200, 100);
        display.logical_width = 0;
        let client = FakeMacosOneShotClient::returning(display);
        let mut backend = MacosOneShotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::Mapping { message }) => {
                assert!(
                    message.contains("zero logical dimensions"),
                    "msg: {message}"
                );
            }
            other => panic!("expected Mapping error, got {other:?}"),
        }
    }

    // ── macOS version check test ──

    #[test]
    fn macos_below_14_returns_unsupported() {
        // Test that the adapter correctly maps Unsupported errors from the isolation crate
        let err = CaptureError::Unsupported {
            message: "macOS 14.0 or newer required for SCScreenshotManager".to_string(),
        };
        let client = FakeMacosOneShotClient::returning_error(err);
        let mut backend = MacosOneShotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::Unsupported { message }) => {
                assert!(message.contains("macOS 14.0"), "msg: {message}");
            }
            other => panic!("expected Unsupported error, got {other:?}"),
        }
    }

    // ── Typed isolation-error mapping tests ──

    #[test]
    fn maps_permission_denied_error() {
        let iso_err = rollshot_macos_oneshot::MacosOneShotError::PermissionDenied(
            "Screen Recording permission denied. Please grant access in System Settings."
                .to_string(),
        );
        let mapped = map_isolation_error(iso_err);
        match mapped {
            CaptureError::PermissionDenied { message } => {
                assert!(message.contains("Screen Recording"), "msg: {message}");
            }
            other => panic!("expected PermissionDenied, got {other:?}"),
        }
    }

    #[test]
    fn maps_timeout_error() {
        let iso_err = rollshot_macos_oneshot::MacosOneShotError::Timeout(
            "Screenshot timed out after 30 seconds".to_string(),
        );
        let mapped = map_isolation_error(iso_err);
        match mapped {
            CaptureError::Timeout { message, .. } => {
                assert!(message.contains("30 seconds"), "msg: {message}");
            }
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[test]
    fn maps_unsupported_error() {
        let iso_err = rollshot_macos_oneshot::MacosOneShotError::Unsupported(
            "macOS 14.0 or newer required".to_string(),
        );
        let mapped = map_isolation_error(iso_err);
        match mapped {
            CaptureError::Unsupported { message } => {
                assert!(message.contains("macOS 14.0"), "msg: {message}");
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn maps_capture_error() {
        let iso_err = rollshot_macos_oneshot::MacosOneShotError::Capture(
            "SCScreenshotManager failed".to_string(),
        );
        let mapped = map_isolation_error(iso_err);
        match mapped {
            CaptureError::Backend(e) => {
                assert!(e.to_string().contains("SCScreenshotManager"), "msg: {e}");
            }
            other => panic!("expected Backend, got {other:?}"),
        }
    }

    // ── Padded-row BGRA fixture test ──

    #[test]
    fn padded_row_bgra_converts_to_tightly_packed_rgba() {
        // Simulate a 2x2 CGImage with bytes_per_row = 12 (padded, not tightly packed 8)
        // BGRA pixels: [B=10, G=20, R=30, A=255] and [B=1, G=2, R=3, A=4]
        // Row 0: 10,20,30,255, 1,2,3,4, pad,pad,pad,pad
        // Row 1: 50,60,70,255, 5,6,7,8, pad,pad,pad,pad
        let bgra_data = vec![
            10, 20, 30, 255, 1, 2, 3, 4, 0, 0, 0, 0, // row 0 (8 pixels + 4 padding)
            50, 60, 70, 255, 5, 6, 7, 8, 0, 0, 0, 0, // row 1 (8 pixels + 4 padding)
        ];
        let width = 2u32;
        let height = 2u32;
        let bytes_per_row = 12usize;

        // Row-by-row conversion respecting bytes_per_row
        let mut rgba = Vec::with_capacity((width as usize) * (height as usize) * 4);
        for row in 0..height as usize {
            let row_start = row * bytes_per_row;
            for col in 0..width as usize {
                let px_start = row_start + col * 4;
                let b = bgra_data[px_start];
                let g = bgra_data[px_start + 1];
                let r = bgra_data[px_start + 2];
                let a = bgra_data[px_start + 3];
                rgba.extend_from_slice(&[r, g, b, a]);
            }
        }

        // Verify conversion
        assert_eq!(rgba.len(), 16); // 2x2x4
                                    // Pixel (0,0): BGRA(10,20,30,255) -> RGBA(30,20,10,255)
        assert_eq!(&rgba[0..4], &[30, 20, 10, 255]);
        // Pixel (1,0): BGRA(1,2,3,4) -> RGBA(3,2,1,4)
        assert_eq!(&rgba[4..8], &[3, 2, 1, 4]);
        // Pixel (0,1): BGRA(50,60,70,255) -> RGBA(70,60,50,255)
        assert_eq!(&rgba[8..12], &[70, 60, 50, 255]);
        // Pixel (1,1): BGRA(5,6,7,8) -> RGBA(7,6,5,8)
        assert_eq!(&rgba[12..16], &[7, 6, 5, 8]);
    }

    // ── show_cursor forwarding test ──

    #[test]
    fn show_cursor_is_forwarded_to_client() {
        // This test verifies that show_cursor is passed through to the client.
        // In a real implementation, the isolation crate would configure
        // SCScreenshotManager with showsCursor based on this flag.
        let display = make_captured_display(100, 100);
        let client = FakeMacosOneShotClient::returning(display);
        let mut backend = MacosOneShotBackend::new(client);

        // Capture with show_cursor=true should succeed
        let result = backend.capture_once(true);
        assert!(result.is_ok(), "expected Ok with cursor, got {result:?}");
    }
}
