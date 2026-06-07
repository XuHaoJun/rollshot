use crate::error::CaptureError;
use crate::one_shot::{DisplayTarget, OneShotCapture};
use crate::types::{Region, Size};

use super::kwin_screenshot::{kwin_raw_to_rgba, KwinRawCapture, KwinScreenshotClient};

/// One-shot capture backend for KDE/KWin using the ScreenShot2 DBus interface.
///
/// This backend does NOT fall back to the portal if KWin fails.
#[allow(dead_code)]
pub struct LinuxKwinOneShotBackend<C: KwinScreenshotClient> {
    client: C,
}

#[allow(dead_code)]
impl<C: KwinScreenshotClient> LinuxKwinOneShotBackend<C> {
    pub fn new(client: C) -> Self {
        Self { client }
    }

    pub fn capture_once(&mut self, show_cursor: bool) -> Result<OneShotCapture, CaptureError> {
        let raw = self.client.capture_active_screen(show_cursor)?;

        let screen_name = if raw.screen_name.is_empty() {
            return Err(CaptureError::Mapping {
                message: "KWin returned empty screen name".to_string(),
            });
        } else {
            raw.screen_name.clone()
        };

        let width = raw.width;
        let height = raw.height;

        let rgba = kwin_raw_to_rgba(&raw)?;

        let scale = if raw.scale > 0.0 {
            raw.scale
        } else {
            return Err(CaptureError::Mapping {
                message: format!("invalid scale from KWin: {}", raw.scale),
            });
        };

        let logical_width = (width as f64 / scale).round() as u32;
        let logical_height = (height as f64 / scale).round() as u32;

        let target = DisplayTarget {
            output_name: Some(screen_name),
            logical_region: Region {
                x: 0,
                y: 0,
                width: logical_width,
                height: logical_height,
            },
            physical_size: Size { width, height },
        };

        OneShotCapture::new(rgba, target)
    }
}

#[cfg(not(test))]
impl crate::one_shot::OneShotCaptureBackend for LinuxKwinOneShotBackend<KwinScreenshotDBusClient> {
    fn capture_once(&mut self, show_cursor: bool) -> Result<OneShotCapture, CaptureError> {
        self.capture_once(show_cursor)
    }
}

/// DBus timeout for KWin screenshot requests.
#[allow(dead_code)]
const KWIN_DBUS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Pipe-read completion timeout.
#[allow(dead_code)]
const KWIN_PIPE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Maximum pixel count (40 megapixels).
#[allow(dead_code)]
const MAX_KWIN_PIXELS: u64 = 40_000_000;

/// Real KWin ScreenShot2 DBus client.
#[cfg(not(test))]
#[allow(dead_code)]
pub struct KwinScreenshotDBusClient;

#[cfg(not(test))]
#[allow(dead_code)]
impl KwinScreenshotDBusClient {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(test))]
impl Default for KwinScreenshotDBusClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(test))]
impl KwinScreenshotClient for KwinScreenshotDBusClient {
    fn capture_active_screen(&self, include_cursor: bool) -> Result<KwinRawCapture, CaptureError> {
        use std::os::fd::AsFd;

        // Create a CLOEXEC pipe so the read/write ends never leak into an
        // unrelated fork+exec while we wait on KWin. KWin receives the write end
        // via DBus FD-passing (which dups), so CLOEXEC does not affect it.
        let (read_fd, write_fd) = nix::unistd::pipe2(nix::fcntl::OFlag::O_CLOEXEC)
            .map_err(|e| CaptureError::Backend(anyhow::anyhow!("pipe2() failed: {e}")))?;

        // Build DBus options map
        let mut options = std::collections::HashMap::new();
        options.insert(
            "include-cursor".to_string(),
            zbus::zvariant::Value::Bool(include_cursor),
        );
        options.insert(
            "native-resolution".to_string(),
            zbus::zvariant::Value::Bool(true),
        );
        options.insert(
            "include-shadow".to_string(),
            zbus::zvariant::Value::Bool(false),
        );

        // Spawn a bounded reader thread before making the DBus request.
        // This prevents a full pipe from deadlocking KWin.
        let max_bytes = (MAX_KWIN_PIXELS * 4) as usize;
        let reader_handle = std::thread::spawn(move || {
            use std::io::Read;
            let mut reader = std::fs::File::from(read_fd);
            let mut bytes = Vec::with_capacity(1024 * 1024); // 1MB initial
            let mut buf = [0u8; 8192];

            let deadline = std::time::Instant::now() + KWIN_PIPE_TIMEOUT;

            loop {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return Err(CaptureError::Timeout {
                        message: "KWin pipe read timed out after 5s".to_string(),
                    });
                }

                // Use nix::poll to check if data is available
                let mut poll_fd =
                    nix::poll::PollFd::new(reader.as_fd(), nix::poll::PollFlags::POLLIN);
                let poll_timeout = (remaining.as_millis() as u16).min(100);
                match nix::poll::poll(std::slice::from_mut(&mut poll_fd), poll_timeout) {
                    Ok(0) => continue, // timeout, try again
                    Ok(_) => {}
                    Err(e) => {
                        return Err(CaptureError::Backend(anyhow::anyhow!("poll failed: {e}")));
                    }
                }

                match reader.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        bytes.extend_from_slice(&buf[..n]);
                        if bytes.len() > max_bytes {
                            return Err(CaptureError::Mapping {
                                message: format!("KWin pipe data exceeds {} byte limit", max_bytes),
                            });
                        }
                    }
                    Err(e) => {
                        return Err(CaptureError::Backend(anyhow::anyhow!(
                            "pipe read failed: {e}"
                        )));
                    }
                }
            }

            Ok(bytes)
        });

        // Make the DBus call
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| CaptureError::Backend(anyhow::anyhow!("tokio runtime: {e}")))?;

        let result: Result<
            std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
            CaptureError,
        > = rt.block_on(async {
            let connection = tokio::time::timeout(KWIN_DBUS_TIMEOUT, zbus::Connection::session())
                .await
                .map_err(|_| CaptureError::Timeout {
                    message: "KWin DBus connection timed out after 5s".to_string(),
                })?
                .map_err(|e| {
                    if e.to_string()
                        .contains("org.freedesktop.DBus.Error.ServiceUnknown")
                    {
                        CaptureError::Unsupported {
                            message: "KWin ScreenShot2 service not available".to_string(),
                        }
                    } else if e
                        .to_string()
                        .contains("org.freedesktop.DBus.Error.AccessDenied")
                    {
                        CaptureError::PermissionDenied {
                            message: format!("KWin DBus access denied: {e}"),
                        }
                    } else {
                        CaptureError::Backend(anyhow::anyhow!("KWin DBus connection: {e}"))
                    }
                })?;

            // Pass the write FD through zbus
            let write_fd_clone = write_fd
                .try_clone()
                .map_err(|e| CaptureError::Backend(anyhow::anyhow!("clone write fd: {e}")))?;

            let result = tokio::time::timeout(
                KWIN_DBUS_TIMEOUT,
                connection.call_method(
                    Some("org.kde.KWin.ScreenShot2"),
                    "/org/kde/KWin/ScreenShot2",
                    Some("org.kde.KWin.ScreenShot2"),
                    "CaptureActiveScreen",
                    &(options, zbus::zvariant::Fd::from(write_fd_clone)),
                ),
            )
            .await
            .map_err(|_| CaptureError::Timeout {
                message: "KWin CaptureActiveScreen timed out after 5s".to_string(),
            })?
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("Cancelled") {
                    CaptureError::UserCancelled
                } else if msg.contains("PermissionDenied") {
                    CaptureError::PermissionDenied {
                        message: format!("KWin screenshot permission denied: {e}"),
                    }
                } else {
                    CaptureError::Backend(anyhow::anyhow!("KWin CaptureActiveScreen: {e}"))
                }
            })?;

            result
                .body()
                .deserialize()
                .map_err(|e| CaptureError::Backend(anyhow::anyhow!("deserialize KWin reply: {e}")))
        });

        // Close the write end so the reader can detect EOF
        drop(write_fd);

        // Wait for the reader thread
        let bytes = reader_handle
            .join()
            .map_err(|_| CaptureError::Backend(anyhow::anyhow!("reader thread panicked")))??;

        // KWin's CaptureActiveScreen returns the metadata as a bare a{sv}
        // vardict body (not a variant-wrapped dict).
        let metadata_map = result?;

        // Extract metadata fields
        let type_str = extract_string(&metadata_map, "type")?;
        if type_str != "raw" {
            return Err(CaptureError::Mapping {
                message: format!("KWin returned unsupported type: {type_str}"),
            });
        }

        let width = extract_u32(&metadata_map, "width")?;
        let height = extract_u32(&metadata_map, "height")?;
        let format = extract_u32(&metadata_map, "format")?;
        let scale = extract_f64(&metadata_map, "scale")?;
        let screen = extract_string(&metadata_map, "screen")?;

        // Validate byte count
        let expected_bytes = (width as u64)
            .checked_mul(height as u64)
            .and_then(|v| v.checked_mul(4))
            .ok_or_else(|| CaptureError::Mapping {
                message: "byte count overflow".to_string(),
            })?;

        if bytes.len() != expected_bytes as usize {
            return Err(CaptureError::Mapping {
                message: format!(
                    "expected {} bytes for {}x{} image, got {}",
                    expected_bytes,
                    width,
                    height,
                    bytes.len()
                ),
            });
        }

        Ok(KwinRawCapture {
            bytes,
            width,
            height,
            qimage_format: format,
            scale,
            screen_name: screen,
        })
    }
}

#[cfg(not(test))]
type KwinMetadata = std::collections::HashMap<String, zbus::zvariant::OwnedValue>;

#[cfg(not(test))]
fn extract_string(map: &KwinMetadata, key: &str) -> Result<String, CaptureError> {
    use zbus::zvariant::Value;
    let value = map.get(key).ok_or_else(|| CaptureError::Mapping {
        message: format!("KWin metadata missing '{key}'"),
    })?;

    match &**value {
        Value::Str(s) => Ok(s.to_string()),
        _ => Err(CaptureError::Mapping {
            message: format!("KWin metadata '{key}' is not a string"),
        }),
    }
}

#[cfg(not(test))]
fn extract_u32(map: &KwinMetadata, key: &str) -> Result<u32, CaptureError> {
    use zbus::zvariant::Value;
    let value = map.get(key).ok_or_else(|| CaptureError::Mapping {
        message: format!("KWin metadata missing '{key}'"),
    })?;

    match &**value {
        Value::U32(v) => Ok(*v),
        Value::I32(v) => {
            if *v >= 0 {
                Ok(*v as u32)
            } else {
                Err(CaptureError::Mapping {
                    message: format!("KWin metadata '{key}' is negative: {v}"),
                })
            }
        }
        _ => Err(CaptureError::Mapping {
            message: format!("KWin metadata '{key}' is not a number"),
        }),
    }
}

#[cfg(not(test))]
fn extract_f64(map: &KwinMetadata, key: &str) -> Result<f64, CaptureError> {
    use zbus::zvariant::Value;
    let value = map.get(key).ok_or_else(|| CaptureError::Mapping {
        message: format!("KWin metadata missing '{key}'"),
    })?;

    match &**value {
        Value::F64(v) => Ok(*v),
        Value::I32(v) => Ok(*v as f64),
        Value::U32(v) => Ok(*v as f64),
        _ => Err(CaptureError::Mapping {
            message: format!("KWin metadata '{key}' is not a number"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct FakeKwinClient {
        result: RefCell<Option<Result<KwinRawCapture, CaptureError>>>,
    }

    impl FakeKwinClient {
        fn returning(capture: KwinRawCapture) -> Self {
            Self {
                result: RefCell::new(Some(Ok(capture))),
            }
        }

        fn returning_error(err: CaptureError) -> Self {
            Self {
                result: RefCell::new(Some(Err(err))),
            }
        }
    }

    impl KwinScreenshotClient for FakeKwinClient {
        fn capture_active_screen(
            &self,
            _include_cursor: bool,
        ) -> Result<KwinRawCapture, CaptureError> {
            self.result
                .borrow_mut()
                .take()
                .expect("FakeKwinClient called more than once")
        }
    }

    fn make_1x1_capture(qimage_format: u32, scale: f64) -> KwinRawCapture {
        KwinRawCapture {
            bytes: vec![0, 0, 0, 255], // 1 pixel, 4 bytes
            width: 1,
            height: 1,
            qimage_format,
            scale,
            screen_name: "eDP-1".to_string(),
        }
    }

    #[test]
    fn capture_once_success() {
        let client = FakeKwinClient::returning(make_1x1_capture(24, 1.0)); // RGBA8888
        let mut backend = LinuxKwinOneShotBackend::new(client);
        let result = backend.capture_once(false);
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        let capture = result.unwrap();
        assert_eq!(
            capture.target_display().output_name.as_deref(),
            Some("eDP-1")
        );
        assert_eq!(capture.image().width(), 1);
        assert_eq!(capture.image().height(), 1);
    }

    #[test]
    fn capture_once_with_cursor() {
        let client = FakeKwinClient::returning(make_1x1_capture(24, 1.0));
        let mut backend = LinuxKwinOneShotBackend::new(client);
        let result = backend.capture_once(true);
        assert!(result.is_ok(), "expected Ok with cursor, got {result:?}");
    }

    #[test]
    fn capture_once_empty_screen_name_returns_mapping_error() {
        let mut capture = make_1x1_capture(24, 1.0);
        capture.screen_name = String::new();
        let client = FakeKwinClient::returning(capture);
        let mut backend = LinuxKwinOneShotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::Mapping { message }) => {
                assert!(message.contains("empty screen name"), "msg: {message}");
            }
            other => panic!("expected Mapping error, got {other:?}"),
        }
    }

    #[test]
    fn capture_once_permission_error_not_mapped() {
        let err = CaptureError::PermissionDenied {
            message: "org.kde.KWin.ScreenShot2.Error.PermissionDenied".to_string(),
        };
        let client = FakeKwinClient::returning_error(err);
        let mut backend = LinuxKwinOneShotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::PermissionDenied { message }) => {
                assert!(message.contains("PermissionDenied"), "msg: {message}");
            }
            other => panic!("expected PermissionDenied, got {other:?}"),
        }
    }

    #[test]
    fn capture_once_invalid_scale_returns_mapping_error() {
        let mut capture = make_1x1_capture(24, 0.0);
        capture.scale = -1.0;
        let client = FakeKwinClient::returning(capture);
        let mut backend = LinuxKwinOneShotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::Mapping { message }) => {
                assert!(message.contains("invalid scale"), "msg: {message}");
            }
            other => panic!("expected Mapping error, got {other:?}"),
        }
    }

    #[test]
    fn capture_once_computes_logical_size_from_physical_and_scale() {
        let mut capture = make_1x1_capture(24, 2.0);
        capture.width = 200;
        capture.height = 100;
        capture.bytes = vec![0; 200 * 100 * 4];
        let client = FakeKwinClient::returning(capture);
        let mut backend = LinuxKwinOneShotBackend::new(client);
        let result = backend.capture_once(false).unwrap();
        // logical = physical / scale = 200/2=100, 100/2=50
        assert_eq!(result.target_display().logical_region.width, 100);
        assert_eq!(result.target_display().logical_region.height, 50);
    }

    #[test]
    fn capture_once_logical_origin_is_zero() {
        let client = FakeKwinClient::returning(make_1x1_capture(24, 1.0));
        let mut backend = LinuxKwinOneShotBackend::new(client);
        let result = backend.capture_once(false).unwrap();
        assert_eq!(result.target_display().logical_region.x, 0);
        assert_eq!(result.target_display().logical_region.y, 0);
    }

    #[test]
    fn capture_once_supported_qt_formats() {
        // Test all supported formats succeed with valid data
        for qt_format in [4, 5, 6, 18, 24, 25] {
            let client = FakeKwinClient::returning(make_1x1_capture(qt_format, 1.0));
            let mut backend = LinuxKwinOneShotBackend::new(client);
            let result = backend.capture_once(false);
            assert!(
                result.is_ok(),
                "Qt format {qt_format} should succeed, got {result:?}"
            );
        }
    }

    #[test]
    fn capture_once_unsupported_qt_format_returns_mapping_error() {
        let client = FakeKwinClient::returning(make_1x1_capture(99, 1.0));
        let mut backend = LinuxKwinOneShotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::Mapping { message }) => {
                assert!(
                    message.contains("unsupported Qt image format"),
                    "msg: {message}"
                );
            }
            other => panic!("expected Mapping error, got {other:?}"),
        }
    }

    #[test]
    fn capture_once_malformed_dimensions_returns_mapping_error() {
        let capture = KwinRawCapture {
            bytes: vec![],
            width: 0,
            height: 100,
            qimage_format: 4,
            scale: 1.0,
            screen_name: "eDP-1".to_string(),
        };
        let client = FakeKwinClient::returning(capture);
        let mut backend = LinuxKwinOneShotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::Mapping { message }) => {
                assert!(message.contains("zero dimension"), "msg: {message}");
            }
            other => panic!("expected Mapping error, got {other:?}"),
        }
    }

    #[test]
    fn capture_once_short_read_returns_mapping_error() {
        let capture = KwinRawCapture {
            bytes: vec![0; 2], // too short for 1x1 32-bit image
            width: 1,
            height: 1,
            qimage_format: 4,
            scale: 1.0,
            screen_name: "eDP-1".to_string(),
        };
        let client = FakeKwinClient::returning(capture);
        let mut backend = LinuxKwinOneShotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::Mapping { message }) => {
                assert!(message.contains("expected"), "msg: {message}");
            }
            other => panic!("expected Mapping error, got {other:?}"),
        }
    }

    #[test]
    fn capture_once_oversized_image_returns_mapping_error() {
        let capture = KwinRawCapture {
            bytes: vec![],
            width: 6325,
            height: 6325,
            qimage_format: 4,
            scale: 1.0,
            screen_name: "eDP-1".to_string(),
        };
        let client = FakeKwinClient::returning(capture);
        let mut backend = LinuxKwinOneShotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::Mapping { message }) => {
                assert!(message.contains("too large"), "msg: {message}");
            }
            other => panic!("expected Mapping error, got {other:?}"),
        }
    }

    #[test]
    fn capture_once_timeout_returns_timeout_error() {
        let err = CaptureError::Timeout {
            message: "KWin CaptureActiveScreen timed out after 5s".to_string(),
        };
        let client = FakeKwinClient::returning_error(err);
        let mut backend = LinuxKwinOneShotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::Timeout { message }) => {
                assert!(message.contains("5s"), "msg: {message}");
            }
            other => panic!("expected Timeout error, got {other:?}"),
        }
    }

    #[test]
    fn capture_once_service_absent_returns_unsupported() {
        let err = CaptureError::Unsupported {
            message: "KWin ScreenShot2 service not available".to_string(),
        };
        let client = FakeKwinClient::returning_error(err);
        let mut backend = LinuxKwinOneShotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::Unsupported { message }) => {
                assert!(message.contains("not available"), "msg: {message}");
            }
            other => panic!("expected Unsupported error, got {other:?}"),
        }
    }

    #[test]
    fn capture_once_user_cancelled_returns_user_cancelled() {
        let err = CaptureError::UserCancelled;
        let client = FakeKwinClient::returning_error(err);
        let mut backend = LinuxKwinOneShotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::UserCancelled) => {}
            other => panic!("expected UserCancelled, got {other:?}"),
        }
    }
}
