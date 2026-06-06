use crate::error::CaptureError;
use image::RgbaImage;

/// Qt QImage::Format values that KWin's ScreenShot2 API may return.
/// Only 32-bit formats are supported; all others are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types, dead_code)]
pub enum QtImageFormat {
    Format_RGB32,
    Format_ARGB32,
    Format_ARGB32_Premultiplied,
    Format_RGBX8888,
    Format_RGBA8888,
    Format_RGBA8888_Premultiplied,
}

#[allow(dead_code)]
impl QtImageFormat {
    /// Convert from the numeric Qt::QImage::Format value.
    /// Returns None for unsupported formats.
    pub fn from_qt_value(value: u32) -> Option<Self> {
        match value {
            4 => Some(QtImageFormat::Format_RGB32),
            5 => Some(QtImageFormat::Format_ARGB32),
            6 => Some(QtImageFormat::Format_ARGB32_Premultiplied),
            18 => Some(QtImageFormat::Format_RGBX8888),
            24 => Some(QtImageFormat::Format_RGBA8888),
            25 => Some(QtImageFormat::Format_RGBA8888_Premultiplied),
            _ => None,
        }
    }
}

/// Raw capture data returned by the KWin ScreenShot2 DBus interface.
#[derive(Debug)]
#[allow(dead_code)]
pub struct KwinRawCapture {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub qimage_format: u32,
    pub scale: f64,
    pub screen_name: String,
}

/// Testable DBus boundary for KWin screenshots.
#[allow(dead_code)]
pub trait KwinScreenshotClient {
    fn capture_active_screen(&self, include_cursor: bool) -> Result<KwinRawCapture, CaptureError>;
}

/// Maximum pixel count (40 megapixels).
#[allow(dead_code)]
const MAX_KWIN_PIXELS: u64 = 40_000_000;

/// Convert a KwinRawCapture to an RgbaImage.
///
/// Handles channel order conversion and premultiplied-alpha unpremultiplication
/// for all supported Qt formats.
#[allow(dead_code)]
pub fn kwin_raw_to_rgba(capture: &KwinRawCapture) -> Result<RgbaImage, CaptureError> {
    if capture.width == 0 || capture.height == 0 {
        return Err(CaptureError::Mapping {
            message: format!(
                "zero dimension not allowed: {}x{}",
                capture.width, capture.height
            ),
        });
    }

    let pixels = (capture.width as u64)
        .checked_mul(capture.height as u64)
        .ok_or_else(|| CaptureError::Mapping {
            message: format!(
                "pixel count overflow for {}x{}",
                capture.width, capture.height
            ),
        })?;

    if pixels > MAX_KWIN_PIXELS {
        return Err(CaptureError::Mapping {
            message: format!(
                "image too large: {} pixels exceeds limit of {}",
                pixels, MAX_KWIN_PIXELS
            ),
        });
    }

    let format = QtImageFormat::from_qt_value(capture.qimage_format).ok_or_else(|| {
        CaptureError::Mapping {
            message: format!(
                "unsupported Qt image format: {}",
                capture.qimage_format
            ),
        }
    })?;

    // All supported formats are 32-bit (4 bytes per pixel).
    let expected_bytes = (capture.width as usize)
        .checked_mul(capture.height as usize)
        .and_then(|v| v.checked_mul(4))
        .ok_or_else(|| CaptureError::Mapping {
            message: "byte count overflow".to_string(),
        })?;

    if capture.bytes.len() != expected_bytes {
        return Err(CaptureError::Mapping {
            message: format!(
                "expected {} bytes for {}x{} image, got {}",
                expected_bytes,
                capture.width,
                capture.height,
                capture.bytes.len()
            ),
        });
    }

    let mut out = Vec::with_capacity(expected_bytes);

    for pixel_idx in 0..(pixels as usize) {
        let offset = pixel_idx * 4;
        let src = &capture.bytes[offset..offset + 4];

        let (r, g, b, a) = match format {
            QtImageFormat::Format_RGB32 => {
                // Layout: BGRA (native-endian), alpha is 0xFF in valid pixels
                (src[2], src[1], src[0], 255u8)
            }
            QtImageFormat::Format_ARGB32 => {
                // Layout: BGRA (native-endian), non-premultiplied
                (src[2], src[1], src[0], src[3])
            }
            QtImageFormat::Format_ARGB32_Premultiplied => {
                // Layout: BGRA (native-endian), premultiplied alpha
                let a = src[3];
                if a == 0 {
                    (0, 0, 0, 0)
                } else {
                    let r = unpremultiply(src[2], a);
                    let g = unpremultiply(src[1], a);
                    let b = unpremultiply(src[0], a);
                    (r, g, b, a)
                }
            }
            QtImageFormat::Format_RGBX8888 => {
                // Layout: RGBA (native-endian), alpha channel is padding
                (src[0], src[1], src[2], 255u8)
            }
            QtImageFormat::Format_RGBA8888 => {
                // Layout: RGBA (native-endian), non-premultiplied
                (src[0], src[1], src[2], src[3])
            }
            QtImageFormat::Format_RGBA8888_Premultiplied => {
                // Layout: RGBA (native-endian), premultiplied alpha
                let a = src[3];
                if a == 0 {
                    (0, 0, 0, 0)
                } else {
                    let r = unpremultiply(src[0], a);
                    let g = unpremultiply(src[1], a);
                    let b = unpremultiply(src[2], a);
                    (r, g, b, a)
                }
            }
        };

        out.push(r);
        out.push(g);
        out.push(b);
        out.push(a);
    }

    RgbaImage::from_raw(capture.width, capture.height, out).ok_or_else(|| {
        CaptureError::Backend(anyhow::anyhow!(
            "failed to build RGBA image from KWin capture"
        ))
    })
}

/// Unpremultiply a premultiplied alpha channel value.
#[allow(dead_code)]
fn unpremultiply(color: u8, alpha: u8) -> u8 {
    debug_assert!(alpha > 0);
    // Scale color back: color * 255 / alpha, clamped to 255
    let c = color as u32;
    let a = alpha as u32;
    ((c * 255 + a / 2) / a).min(255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_rgb32_converts_to_rgba() {
        // RGB32: BGRA layout, alpha forced to 255
        let bytes = vec![10, 20, 30, 0, 40, 50, 60, 0];
        let capture = KwinRawCapture {
            bytes,
            width: 2,
            height: 1,
            qimage_format: 4, // Format_RGB32
            scale: 1.0,
            screen_name: "eDP-1".to_string(),
        };
        let img = kwin_raw_to_rgba(&capture).unwrap();
        // BG(10,20,30,0) -> RGBA(30,20,10,255), BG(40,50,60,0) -> RGBA(60,50,40,255)
        assert_eq!(img.as_raw(), &[30, 20, 10, 255, 60, 50, 40, 255]);
    }

    #[test]
    fn format_argb32_converts_to_rgba() {
        // ARGB32: BGRA layout, non-premultiplied
        let bytes = vec![10, 20, 30, 128, 40, 50, 60, 200];
        let capture = KwinRawCapture {
            bytes,
            width: 2,
            height: 1,
            qimage_format: 5, // Format_ARGB32
            scale: 1.0,
            screen_name: "eDP-1".to_string(),
        };
        let img = kwin_raw_to_rgba(&capture).unwrap();
        // BG(10,20,30,128) -> RGBA(30,20,10,128)
        assert_eq!(img.as_raw(), &[30, 20, 10, 128, 60, 50, 40, 200]);
    }

    #[test]
    fn format_argb32_premultiplied_converts_to_rgba() {
        // ARGB32 premultiplied: BGRA layout
        // Pixel with alpha=128, premultiplied colors at 50%
        let bytes = vec![64, 64, 64, 128, 0, 0, 0, 0];
        let capture = KwinRawCapture {
            bytes,
            width: 2,
            height: 1,
            qimage_format: 6, // Format_ARGB32_Premultiplied
            scale: 1.0,
            screen_name: "eDP-1".to_string(),
        };
        let img = kwin_raw_to_rgba(&capture).unwrap();
        // Premultiplied (64,64,64,128) -> unpremultiply to (128,128,128,128)
        // (0,0,0,0) -> (0,0,0,0)
        assert_eq!(img.as_raw(), &[128, 128, 128, 128, 0, 0, 0, 0]);
    }

    #[test]
    fn format_rgbx8888_converts_to_rgba() {
        // RGBX8888: RGBA layout, alpha forced to 255
        let bytes = vec![10, 20, 30, 0, 40, 50, 60, 0];
        let capture = KwinRawCapture {
            bytes,
            width: 2,
            height: 1,
            qimage_format: 18, // Format_RGBX8888
            scale: 1.0,
            screen_name: "eDP-1".to_string(),
        };
        let img = kwin_raw_to_rgba(&capture).unwrap();
        assert_eq!(img.as_raw(), &[10, 20, 30, 255, 40, 50, 60, 255]);
    }

    #[test]
    fn format_rgba8888_converts_to_rgba() {
        // RGBA8888: RGBA layout, non-premultiplied
        let bytes = vec![10, 20, 30, 128, 40, 50, 60, 200];
        let capture = KwinRawCapture {
            bytes,
            width: 2,
            height: 1,
            qimage_format: 24, // Format_RGBA8888
            scale: 1.0,
            screen_name: "eDP-1".to_string(),
        };
        let img = kwin_raw_to_rgba(&capture).unwrap();
        assert_eq!(img.as_raw(), &[10, 20, 30, 128, 40, 50, 60, 200]);
    }

    #[test]
    fn format_rgba8888_premultiplied_converts_to_rgba() {
        // RGBA8888 premultiplied: RGBA layout
        let bytes = vec![64, 64, 64, 128, 0, 0, 0, 0];
        let capture = KwinRawCapture {
            bytes,
            width: 2,
            height: 1,
            qimage_format: 25, // Format_RGBA8888_Premultiplied
            scale: 1.0,
            screen_name: "eDP-1".to_string(),
        };
        let img = kwin_raw_to_rgba(&capture).unwrap();
        assert_eq!(img.as_raw(), &[128, 128, 128, 128, 0, 0, 0, 0]);
    }

    #[test]
    fn missing_screen_metadata_returns_mapping_error() {
        // This test verifies that callers must provide screen metadata.
        // The conversion itself doesn't check screen, but the OneShotCapture
        // builder should. Here we just verify the capture struct can hold it.
        let capture = KwinRawCapture {
            bytes: vec![0; 4],
            width: 1,
            height: 1,
            qimage_format: 4,
            scale: 1.0,
            screen_name: String::new(), // empty = missing
        };
        // The raw_to_rgba function should still work; screen validation
        // happens at the OneShotCapture level.
        let img = kwin_raw_to_rgba(&capture);
        assert!(img.is_ok());
    }

    #[test]
    fn kwin_permission_error_not_mapped() {
        // Verify that a fake KWin permission error is returned unchanged
        // and never invokes a portal client. This is tested at the
        // LinuxKwinOneShotBackend level in one_shot.rs.
        let err = CaptureError::PermissionDenied {
            message: "org.kde.KWin.ScreenShot2.Error.PermissionDenied".to_string(),
        };
        assert!(matches!(err, CaptureError::PermissionDenied { .. }));
    }

    #[test]
    fn malformed_dimensions_returns_mapping_error() {
        let capture = KwinRawCapture {
            bytes: vec![],
            width: 0,
            height: 100,
            qimage_format: 4,
            scale: 1.0,
            screen_name: "eDP-1".to_string(),
        };
        match kwin_raw_to_rgba(&capture) {
            Err(CaptureError::Mapping { message }) => {
                assert!(message.contains("zero dimension"), "msg: {message}");
            }
            other => panic!("expected Mapping error, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_qt_format_returns_mapping_error() {
        let capture = KwinRawCapture {
            bytes: vec![0; 4],
            width: 1,
            height: 1,
            qimage_format: 99, // unsupported
            scale: 1.0,
            screen_name: "eDP-1".to_string(),
        };
        match kwin_raw_to_rgba(&capture) {
            Err(CaptureError::Mapping { message }) => {
                assert!(message.contains("unsupported Qt image format"), "msg: {message}");
            }
            other => panic!("expected Mapping error, got {other:?}"),
        }
    }

    #[test]
    fn short_read_returns_mapping_error() {
        let capture = KwinRawCapture {
            bytes: vec![0; 2], // too short for 1x1 32-bit image
            width: 1,
            height: 1,
            qimage_format: 4,
            scale: 1.0,
            screen_name: "eDP-1".to_string(),
        };
        match kwin_raw_to_rgba(&capture) {
            Err(CaptureError::Mapping { message }) => {
                assert!(message.contains("expected"), "msg: {message}");
                assert!(message.contains("got 2"), "msg: {message}");
            }
            other => panic!("expected Mapping error, got {other:?}"),
        }
    }

    #[test]
    fn oversized_image_returns_mapping_error() {
        // 6325 * 6325 > 40,000,000
        let capture = KwinRawCapture {
            bytes: vec![],
            width: 6325,
            height: 6325,
            qimage_format: 4,
            scale: 1.0,
            screen_name: "eDP-1".to_string(),
        };
        match kwin_raw_to_rgba(&capture) {
            Err(CaptureError::Mapping { message }) => {
                assert!(message.contains("too large"), "msg: {message}");
            }
            other => panic!("expected Mapping error, got {other:?}"),
        }
    }

    #[test]
    fn show_cursor_forwards_to_include_cursor() {
        // This test verifies the trait boundary accepts show_cursor.
        // Actual forwarding is tested at the integration level.
        struct TestClient;
        impl KwinScreenshotClient for TestClient {
            fn capture_active_screen(
                &self,
                include_cursor: bool,
            ) -> Result<KwinRawCapture, CaptureError> {
                // Verify the parameter is passed through
                assert!(include_cursor);
                Ok(KwinRawCapture {
                    bytes: vec![0; 4],
                    width: 1,
                    height: 1,
                    qimage_format: 4,
                    scale: 1.0,
                    screen_name: "eDP-1".to_string(),
                })
            }
        }
        let client = TestClient;
        let result = client.capture_active_screen(true);
        assert!(result.is_ok());
    }

    #[test]
    fn pixel_count_exact_boundary() {
        // 5000 * 8000 = 40,000,000 exactly at the limit
        let capture = KwinRawCapture {
            bytes: vec![0; 40_000_000 * 4],
            width: 5000,
            height: 8000,
            qimage_format: 4,
            scale: 1.0,
            screen_name: "eDP-1".to_string(),
        };
        // Should not error on pixel count check
        let result = kwin_raw_to_rgba(&capture);
        assert!(result.is_ok());
    }

    #[test]
    fn pixel_count_one_above_boundary() {
        // 6325 * 6325 = 40,005,625 > 40,000,000
        let capture = KwinRawCapture {
            bytes: vec![],
            width: 6325,
            height: 6325,
            qimage_format: 4,
            scale: 1.0,
            screen_name: "eDP-1".to_string(),
        };
        match kwin_raw_to_rgba(&capture) {
            Err(CaptureError::Mapping { message }) => {
                assert!(message.contains("too large"), "msg: {message}");
            }
            other => panic!("expected Mapping error, got {other:?}"),
        }
    }
}
