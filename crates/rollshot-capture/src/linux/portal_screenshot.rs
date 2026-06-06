#![allow(dead_code)]

use crate::error::CaptureError;
use crate::one_shot::{DisplayTarget, OneShotCapture};
use crate::types::{Region, Size};

use crate::one_shot::is_kde;

const PORTAL_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Trait for injecting portal screenshot behavior in tests.
pub(crate) trait PortalScreenshotClient {
    fn request_screenshot(&self) -> Result<ScreenshotResponse, CaptureError>;
}

/// Response from the portal Screenshot interface.
pub(crate) struct ScreenshotResponse {
    pub uri: String,
}

/// One-shot capture backend using the freedesktop Screenshot portal.
///
/// This backend is intentionally limited:
/// - Only accepts provable single-output results
/// - Rejects `show_cursor = true` (portal has no cursor option)
/// - Uses a 60-second timeout for portal requests
pub(crate) struct PortalScreenshotBackend<C: PortalScreenshotClient> {
    client: C,
}

impl<C: PortalScreenshotClient> PortalScreenshotBackend<C> {
    pub fn new(client: C) -> Self {
        Self { client }
    }

    /// Capture a screenshot via the portal. Returns an unresolved `DisplayTarget`
    /// whose physical_size is populated from the decoded image dimensions.
    ///
    /// The Linux runner calls `validate_surface_mapping` after receiving the
    /// layer-surface size and scale to confirm single-output provenance.
    pub fn capture_once(&self, show_cursor: bool) -> Result<OneShotCapture, CaptureError> {
        if show_cursor {
            return Err(CaptureError::Unsupported {
                message: "Screenshot portal has no cursor-inclusion option; \
                          pass show_cursor = false"
                    .to_string(),
            });
        }

        let response = self.client.request_screenshot()?;
        let uri = &response.uri;

        if !uri.starts_with("file://") {
            return Err(CaptureError::Mapping {
                message: format!("portal returned non-file URI: {uri}"),
            });
        }

        let path = uri.strip_prefix("file://").unwrap();
        let img = load_portal_image(path)?;

        let width = img.width();
        let height = img.height();

        let target = DisplayTarget {
            output_name: None,
            logical_region: Region {
                x: 0,
                y: 0,
                width,
                height,
            },
            physical_size: Size { width, height },
        };

        OneShotCapture::new(img, target)
    }
}

/// Load and decode a PNG from a local file path with decoding limits.
fn load_portal_image(path: &str) -> Result<image::RgbaImage, CaptureError> {
    let mut reader = image::ImageReader::open(path)
        .map_err(|e| CaptureError::Mapping {
            message: format!("failed to open portal screenshot at {path}: {e}"),
        })?
        .with_guessed_format()
        .map_err(|e| CaptureError::Mapping {
            message: format!("failed to guess image format at {path}: {e}"),
        })?;

    // Reject oversized images before unbounded decompression.
    // The image crate's default max_alloc is 512 MiB.  We tighten it to
    // MAX_ONE_SHOT_PIXELS × 4 bytes (RGBA) so a huge PNG fails at decode
    // rather than exhausting memory.
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(crate::one_shot::MAX_ONE_SHOT_PIXELS * 4);
    reader.limits(limits);

    let img = reader.decode().map_err(|e| {
        let msg = e.to_string();
        if msg.contains("limit") || msg.contains("too large") || msg.contains("exceeded") {
            CaptureError::Mapping {
                message: format!("portal image exceeds decoding limits: {e}"),
            }
        } else {
            CaptureError::Mapping {
                message: format!("failed to decode portal screenshot: {e}"),
            }
        }
    })?;

    Ok(img.to_rgba8())
}

/// Real portal screenshot client using `ashpd`.
#[cfg(not(test))]
pub(crate) struct AshpdScreenshotClient;

#[cfg(not(test))]
impl AshpdScreenshotClient {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(test))]
impl PortalScreenshotClient for AshpdScreenshotClient {
    fn request_screenshot(&self) -> Result<ScreenshotResponse, CaptureError> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| CaptureError::Backend(anyhow::anyhow!("tokio runtime: {e}")))?;

        rt.block_on(async {
            let request = tokio::time::timeout(
                PORTAL_REQUEST_TIMEOUT,
                ashpd::desktop::screenshot::Screenshot::request()
                    .interactive(false)
                    .modal(false)
                    .send(),
            )
            .await
            .map_err(|_| CaptureError::Timeout {
                message: "Screenshot portal request timed out after 60s".to_string(),
            })?
            .map_err(map_screenshot_ashpd_error)?;

            let response = request.response().map_err(map_screenshot_ashpd_error)?;
            let uri = response.uri().to_string();

            Ok(ScreenshotResponse { uri })
        })
    }
}

#[cfg(not(test))]
fn map_screenshot_ashpd_error(e: ashpd::Error) -> CaptureError {
    match e {
        ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled) => {
            CaptureError::UserCancelled
        }
        ashpd::Error::Response(ashpd::desktop::ResponseError::Other) => {
            CaptureError::Backend(anyhow::anyhow!("portal interaction ended"))
        }
        other => CaptureError::Backend(anyhow::anyhow!("{other}")),
    }
}

/// Determine whether KDE should use the portal screenshot backend.
///
/// Returns `false` for KDE — KDE uses the KWin backend instead.
pub(crate) fn should_use_portal_screenshot(desktop: Option<&str>) -> bool {
    !is_kde(desktop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct FakeScreenshotClient {
        result: RefCell<Option<Result<ScreenshotResponse, CaptureError>>>,
    }

    impl FakeScreenshotClient {
        fn returning_uri(uri: &str) -> Self {
            Self {
                result: RefCell::new(Some(Ok(ScreenshotResponse {
                    uri: uri.to_string(),
                }))),
            }
        }

        fn returning_error(err: CaptureError) -> Self {
            Self {
                result: RefCell::new(Some(Err(err))),
            }
        }
    }

    impl PortalScreenshotClient for FakeScreenshotClient {
        fn request_screenshot(&self) -> Result<ScreenshotResponse, CaptureError> {
            self.result
                .borrow_mut()
                .take()
                .expect("FakeScreenshotClient called more than once")
        }
    }

    fn write_test_png(path: &std::path::Path, width: u32, height: u32) {
        use image::ImageBuffer;
        let img: image::RgbaImage =
            ImageBuffer::from_pixel(width, height, image::Rgba([0, 0, 0, 255]));
        img.save(path).expect("failed to write test PNG");
    }

    #[test]
    fn portal_cancelled_becomes_user_cancelled() {
        let client = FakeScreenshotClient::returning_error(CaptureError::UserCancelled);
        let backend = PortalScreenshotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::UserCancelled) => {}
            other => panic!("expected UserCancelled, got {other:?}"),
        }
    }

    #[test]
    fn local_file_uri_loads_png() {
        let dir = std::env::temp_dir().join("rollshot_portal_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_screenshot.png");
        write_test_png(&path, 10, 10);

        let uri = format!("file://{}", path.display());
        let client = FakeScreenshotClient::returning_uri(&uri);
        let backend = PortalScreenshotBackend::new(client);
        let result = backend.capture_once(false);
        let _ = std::fs::remove_file(&path);

        assert!(result.is_ok(), "expected Ok, got {result:?}");
        let capture = result.unwrap();
        assert_eq!(capture.image().width(), 10);
        assert_eq!(capture.image().height(), 10);
    }

    #[test]
    fn single_output_accepted_when_matches_overlay() {
        let dir = std::env::temp_dir().join("rollshot_portal_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_single_output.png");
        write_test_png(&path, 1920, 1080);

        let uri = format!("file://{}", path.display());
        let client = FakeScreenshotClient::returning_uri(&uri);
        let backend = PortalScreenshotBackend::new(client);
        let result = backend.capture_once(false);
        let _ = std::fs::remove_file(&path);

        let capture = result.unwrap();
        let mapping = crate::one_shot::validate_surface_mapping(
            Size {
                width: capture.image().width(),
                height: capture.image().height(),
            },
            Size {
                width: 1920,
                height: 1080,
            },
            1.0,
        );
        assert!(mapping.is_ok(), "expected mapping OK, got {mapping:?}");
    }

    #[test]
    fn composite_multi_output_returns_mapping_error() {
        let dir = std::env::temp_dir().join("rollshot_portal_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_composite.png");
        // 3840x2160 is two 1920x1080 outputs — but overlay is 1920x1080 at scale 1.0
        write_test_png(&path, 3840, 2160);

        let uri = format!("file://{}", path.display());
        let client = FakeScreenshotClient::returning_uri(&uri);
        let backend = PortalScreenshotBackend::new(client);
        let result = backend.capture_once(false);
        let _ = std::fs::remove_file(&path);

        let capture = result.unwrap();
        let mapping = crate::one_shot::validate_surface_mapping(
            Size {
                width: capture.image().width(),
                height: capture.image().height(),
            },
            Size {
                width: 1920,
                height: 1080,
            },
            1.0,
        );
        assert!(
            matches!(mapping, Err(CaptureError::Mapping { .. })),
            "expected Mapping for composite image, got {mapping:?}"
        );
    }

    #[test]
    fn inconsistent_overlay_image_mapping_returns_mapping_error() {
        let dir = std::env::temp_dir().join("rollshot_portal_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_inconsistent.png");
        // 2560x1440 doesn't match overlay 1920x1080 at scale 1.0
        write_test_png(&path, 2560, 1440);

        let uri = format!("file://{}", path.display());
        let client = FakeScreenshotClient::returning_uri(&uri);
        let backend = PortalScreenshotBackend::new(client);
        let result = backend.capture_once(false);
        let _ = std::fs::remove_file(&path);

        let capture = result.unwrap();
        let mapping = crate::one_shot::validate_surface_mapping(
            Size {
                width: capture.image().width(),
                height: capture.image().height(),
            },
            Size {
                width: 1920,
                height: 1080,
            },
            1.0,
        );
        assert!(
            matches!(mapping, Err(CaptureError::Mapping { .. })),
            "expected Mapping for inconsistent overlay/image, got {mapping:?}"
        );
    }

    #[test]
    fn oversized_png_rejected_before_unbounded_memory() {
        let dir = std::env::temp_dir().join("rollshot_portal_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_oversized.png");
        // Create a PNG that claims to be huge. The decoder should reject it.
        // We can't actually write a 6325x6325 PNG easily, so we test via the
        // pixel count check in OneShotCapture::new. Instead, test the direct
        // path with a reasonable image but bogus dimensions that exceed limits.
        //
        // For this test, we verify that an image exceeding MAX_ONE_SHOT_PIXELS
        // is rejected. We'll use the load_portal_image function directly with
        // a valid small PNG and verify the limit works via OneShotCapture::new.
        // Actually, let's write a real but small PNG and test the pixel count limit
        // through the capture path — the limit is enforced in OneShotCapture::new.
        write_test_png(&path, 6325, 6325);
        // Note: the PNG writer may fail for very large images, so if it does,
        // we test the error path through load_portal_image.
        let uri = format!("file://{}", path.display());
        let client = FakeScreenshotClient::returning_uri(&uri);
        let backend = PortalScreenshotBackend::new(client);
        let result = backend.capture_once(false);
        let _ = std::fs::remove_file(&path);

        // Should fail either from decoding limits or from pixel count check
        assert!(
            result.is_err(),
            "expected Err for oversized image, got {result:?}"
        );
        match result {
            Err(CaptureError::Mapping { message }) => {
                assert!(
                    message.contains("too large")
                        || message.contains("limit")
                        || message.contains("exceeds")
                        || message.contains("pixel"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("expected Mapping error, got {other:?}"),
        }
    }

    #[test]
    fn portal_request_timeout_returns_timeout() {
        let client = FakeScreenshotClient::returning_error(CaptureError::Timeout {
            message: "Screenshot portal request timed out after 60s".to_string(),
        });
        let backend = PortalScreenshotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::Timeout { message }) => {
                assert!(message.contains("60s"), "unexpected message: {message}");
            }
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[test]
    fn show_cursor_true_returns_unsupported() {
        let client = FakeScreenshotClient::returning_uri("file:///dev/null");
        let backend = PortalScreenshotBackend::new(client);
        match backend.capture_once(true) {
            Err(CaptureError::Unsupported { message }) => {
                assert!(
                    message.contains("cursor-inclusion"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn kde_never_uses_portal_screenshot() {
        assert!(!should_use_portal_screenshot(Some("KDE")));
        assert!(!should_use_portal_screenshot(Some("kde")));
        assert!(!should_use_portal_screenshot(Some("sway:KDE")));
    }

    #[test]
    fn kde_plasma_without_colon_uses_portal_screenshot() {
        // "KDE Plasma" (without colon) is not caught by is_kde — this matches
        // the existing one_shot_backend_for behavior.
        assert!(should_use_portal_screenshot(Some("KDE Plasma")));
    }

    #[test]
    fn non_kde_uses_portal_screenshot() {
        assert!(should_use_portal_screenshot(Some("GNOME")));
        assert!(should_use_portal_screenshot(Some("sway")));
        assert!(should_use_portal_screenshot(Some("Hyprland")));
        assert!(should_use_portal_screenshot(None));
    }

    #[test]
    fn non_file_uri_returns_mapping_error() {
        let client = FakeScreenshotClient::returning_uri("https://example.com/screenshot.png");
        let backend = PortalScreenshotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::Mapping { message }) => {
                assert!(
                    message.contains("non-file URI"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("expected Mapping, got {other:?}"),
        }
    }

    #[test]
    fn missing_file_returns_mapping_error() {
        let client = FakeScreenshotClient::returning_uri("file:///nonexistent/path/screenshot.png");
        let backend = PortalScreenshotBackend::new(client);
        match backend.capture_once(false) {
            Err(CaptureError::Mapping { message }) => {
                assert!(
                    message.contains("failed to open") || message.contains("failed to decode"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("expected Mapping, got {other:?}"),
        }
    }

    #[test]
    fn invalid_png_returns_mapping_error() {
        let dir = std::env::temp_dir().join("rollshot_portal_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_invalid.png");
        std::fs::write(&path, b"not a png").expect("write fake png");

        let uri = format!("file://{}", path.display());
        let client = FakeScreenshotClient::returning_uri(&uri);
        let backend = PortalScreenshotBackend::new(client);
        let result = backend.capture_once(false);
        let _ = std::fs::remove_file(&path);

        match result {
            Err(CaptureError::Mapping { message }) => {
                assert!(
                    message.contains("failed to decode") || message.contains("failed to guess"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("expected Mapping, got {other:?}"),
        }
    }
}
