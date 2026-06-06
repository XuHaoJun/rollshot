use crate::error::CaptureError;
use crate::types::{Region, Size};
use image::RgbaImage;

pub const MAX_ONE_SHOT_PIXELS: u64 = 40_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayTarget {
    pub output_name: Option<String>,
    pub logical_region: Region,
    pub physical_size: Size,
}

#[derive(Debug)]
pub struct OneShotCapture {
    image: RgbaImage,
    target_display: DisplayTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OneShotBackendKind {
    LinuxKwin,
    LinuxPortal,
    MacosScreenshotManager,
    Unsupported,
}

pub trait OneShotCaptureBackend {
    fn capture_once(&mut self, show_cursor: bool) -> Result<OneShotCapture, CaptureError>;
}

pub(crate) fn is_kde(desktop: Option<&str>) -> bool {
    desktop
        .unwrap_or_default()
        .split(':')
        .any(|part| part.eq_ignore_ascii_case("KDE"))
}

pub fn one_shot_backend_for(
    os: &str,
    session_type: Option<&str>,
    desktop: Option<&str>,
) -> OneShotBackendKind {
    match os {
        "linux" => match session_type {
            Some("wayland") => {
                if is_kde(desktop) {
                    OneShotBackendKind::LinuxKwin
                } else {
                    OneShotBackendKind::LinuxPortal
                }
            }
            _ => OneShotBackendKind::Unsupported,
        },
        "macos" => OneShotBackendKind::MacosScreenshotManager,
        _ => OneShotBackendKind::Unsupported,
    }
}

impl OneShotBackendKind {
    pub fn from_environment(backend_flag: &str) -> Result<Self, CaptureError> {
        if backend_flag != "auto" {
            return Err(CaptureError::InvalidConfig {
                message: format!(
                    "screenshot mode only accepts 'auto' backend, got '{backend_flag}'"
                ),
            });
        }

        let os = std::env::consts::OS;
        let session_type = std::env::var("XDG_SESSION_TYPE").ok();
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
        Ok(one_shot_backend_for(
            os,
            session_type.as_deref(),
            desktop.as_deref(),
        ))
    }

    #[cfg(not(test))]
    pub fn capture_once(self, show_cursor: bool) -> Result<OneShotCapture, CaptureError> {
        match self {
            #[cfg(target_os = "linux")]
            OneShotBackendKind::LinuxKwin => {
                let mut backend = crate::LinuxKwinOneShotBackend::new(
                    crate::linux::one_shot::KwinScreenshotDBusClient::new(),
                );
                backend.capture_once(show_cursor)
            }
            #[cfg(target_os = "linux")]
            OneShotBackendKind::LinuxPortal => {
                let backend = crate::linux::portal_screenshot::PortalScreenshotBackend::new(
                    crate::linux::portal_screenshot::AshpdScreenshotClient::new(),
                );
                backend.capture_once(show_cursor)
            }
            #[cfg(target_os = "macos")]
            OneShotBackendKind::MacosScreenshotManager => {
                let _ = show_cursor;
                Err(CaptureError::Unsupported {
                    message: "macOS one-shot capture not yet wired through iced overlay"
                        .to_string(),
                })
            }
            _ => Err(CaptureError::Unsupported {
                message: format!("no one-shot capture backend available for {self:?}"),
            }),
        }
    }
}

fn checked_pixel_count(width: u32, height: u32) -> Result<u64, CaptureError> {
    if width == 0 || height == 0 {
        return Err(CaptureError::Mapping {
            message: format!("zero dimension not allowed: {width}x{height}"),
        });
    }
    let pixels =
        (width as u64)
            .checked_mul(height as u64)
            .ok_or_else(|| CaptureError::Mapping {
                message: format!("pixel count overflow for {width}x{height}"),
            })?;
    if pixels > MAX_ONE_SHOT_PIXELS {
        return Err(CaptureError::Mapping {
            message: format!(
                "image too large: {pixels} pixels exceeds limit of {MAX_ONE_SHOT_PIXELS}"
            ),
        });
    }
    Ok(pixels)
}

impl OneShotCapture {
    pub fn new(image: RgbaImage, target_display: DisplayTarget) -> Result<Self, CaptureError> {
        let img_w = image.width();
        let img_h = image.height();
        let target_w = target_display.physical_size.width;
        let target_h = target_display.physical_size.height;

        if img_w != target_w || img_h != target_h {
            return Err(CaptureError::Mapping {
                message: format!(
                    "image dimensions {img_w}x{img_h} do not match target physical size {target_w}x{target_h}"
                ),
            });
        }

        checked_pixel_count(img_w, img_h)?;

        Ok(Self {
            image,
            target_display,
        })
    }

    pub fn image(&self) -> &RgbaImage {
        &self.image
    }

    pub fn target_display(&self) -> &DisplayTarget {
        &self.target_display
    }
}

pub fn validate_surface_mapping(
    image_size: Size,
    overlay_logical: Size,
    overlay_scale: f64,
) -> Result<(), CaptureError> {
    if !overlay_scale.is_finite() || overlay_scale <= 0.0 {
        return Err(CaptureError::Mapping {
            message: format!("invalid overlay scale: {overlay_scale}"),
        });
    }

    if overlay_logical.width == 0 || overlay_logical.height == 0 {
        return Err(CaptureError::Mapping {
            message: format!(
                "zero overlay logical dimension: {}x{}",
                overlay_logical.width, overlay_logical.height
            ),
        });
    }

    checked_pixel_count(image_size.width, image_size.height)?;

    let expected_w = (overlay_logical.width as f64 * overlay_scale).round() as i64;
    let expected_h = (overlay_logical.height as f64 * overlay_scale).round() as i64;

    let diff_w = (image_size.width as i64 - expected_w).abs();
    let diff_h = (image_size.height as i64 - expected_h).abs();

    if diff_w > 1 || diff_h > 1 {
        return Err(CaptureError::Mapping {
            message: format!(
                "image size {}x{} does not match expected {}x{} (from logical {}x{} at scale {})",
                image_size.width,
                image_size.height,
                expected_w,
                expected_h,
                overlay_logical.width,
                overlay_logical.height,
                overlay_scale
            ),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Backend selection tests ──

    #[test]
    fn linux_wayland_kde_selects_kwin() {
        assert_eq!(
            one_shot_backend_for("linux", Some("wayland"), Some("KDE")),
            OneShotBackendKind::LinuxKwin
        );
    }

    #[test]
    fn linux_wayland_gnome_selects_portal() {
        assert_eq!(
            one_shot_backend_for("linux", Some("wayland"), Some("GNOME")),
            OneShotBackendKind::LinuxPortal
        );
    }

    #[test]
    fn macos_selects_screenshot_manager() {
        assert_eq!(
            one_shot_backend_for("macos", None, None),
            OneShotBackendKind::MacosScreenshotManager
        );
    }

    #[test]
    fn unsupported_os_returns_unsupported() {
        assert_eq!(
            one_shot_backend_for("windows", None, None),
            OneShotBackendKind::Unsupported
        );
    }

    #[test]
    fn linux_no_wayland_returns_unsupported() {
        assert_eq!(
            one_shot_backend_for("linux", Some("x11"), None),
            OneShotBackendKind::Unsupported
        );
    }

    #[test]
    fn linux_wayland_no_desktop_selects_portal() {
        assert_eq!(
            one_shot_backend_for("linux", Some("wayland"), None),
            OneShotBackendKind::LinuxPortal
        );
    }

    #[test]
    fn linux_wayland_case_insensitive_kde() {
        assert_eq!(
            one_shot_backend_for("linux", Some("wayland"), Some("kde")),
            OneShotBackendKind::LinuxKwin
        );
    }

    #[test]
    fn linux_wayland_multi_desktop_with_kde() {
        assert_eq!(
            one_shot_backend_for("linux", Some("wayland"), Some("sway:KDE")),
            OneShotBackendKind::LinuxKwin
        );
    }

    #[test]
    fn unsupported_kind_returns_capture_error() {
        let result = create_unsupported_capture();
        assert!(matches!(result, Err(CaptureError::Unsupported { .. })));
    }

    fn create_unsupported_capture() -> Result<OneShotCapture, CaptureError> {
        let kind = OneShotBackendKind::Unsupported;
        match kind {
            OneShotBackendKind::Unsupported => Err(CaptureError::Unsupported {
                message: "no one-shot capture backend available".to_string(),
            }),
            _ => unreachable!(),
        }
    }

    // ── Streaming backend rejection tests ──

    #[test]
    fn from_environment_rejects_streaming_backend_linux_portal() {
        let _guard = crate::ENV_MUTEX.lock().unwrap();
        let err = OneShotBackendKind::from_environment("linux-portal").expect_err("should reject");
        assert!(matches!(err, CaptureError::InvalidConfig { .. }));
        assert!(err.to_string().contains("linux-portal"));
    }

    #[test]
    fn from_environment_rejects_streaming_backend_macos_sck() {
        let _guard = crate::ENV_MUTEX.lock().unwrap();
        let err = OneShotBackendKind::from_environment("macos-sck").expect_err("should reject");
        assert!(matches!(err, CaptureError::InvalidConfig { .. }));
        assert!(err.to_string().contains("macos-sck"));
    }

    // ── from_environment env-var integration tests ──

    #[cfg(target_os = "linux")]
    #[test]
    fn from_environment_selects_portal_for_gnome_wayland() {
        let _guard = crate::ENV_MUTEX.lock().unwrap();
        std::env::set_var("XDG_SESSION_TYPE", "wayland");
        std::env::set_var("XDG_CURRENT_DESKTOP", "GNOME");
        let result = OneShotBackendKind::from_environment("auto");
        std::env::remove_var("XDG_SESSION_TYPE");
        std::env::remove_var("XDG_CURRENT_DESKTOP");
        assert_eq!(result.unwrap(), OneShotBackendKind::LinuxPortal);
    }

    // ── OneShotCapture::new validation tests ──

    #[test]
    fn new_rejects_mismatched_dimensions() {
        let image = RgbaImage::new(100, 100);
        let target = DisplayTarget {
            output_name: Some("eDP-1".to_string()),
            logical_region: Region {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            },
            physical_size: Size {
                width: 200,
                height: 200,
            },
        };
        let err = OneShotCapture::new(image, target).expect_err("should reject mismatch");
        assert!(matches!(err, CaptureError::Mapping { .. }));
        assert!(err.to_string().contains("do not match"));
    }

    #[test]
    fn new_accepts_matching_dimensions() {
        let image = RgbaImage::new(200, 200);
        let target = DisplayTarget {
            output_name: Some("eDP-1".to_string()),
            logical_region: Region {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            },
            physical_size: Size {
                width: 200,
                height: 200,
            },
        };
        let capture = OneShotCapture::new(image, target).expect("should accept");
        assert_eq!(capture.image().width(), 200);
        assert_eq!(capture.image().height(), 200);
        assert_eq!(
            capture.target_display().output_name.as_deref(),
            Some("eDP-1")
        );
    }

    // ── Surface mapping validation tests ──

    #[test]
    fn validate_surface_mapping_exact_1x_scale() {
        let result = validate_surface_mapping(
            Size {
                width: 1920,
                height: 1080,
            },
            Size {
                width: 1920,
                height: 1080,
            },
            1.0,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn validate_surface_mapping_fractional_scale_within_tolerance() {
        let result = validate_surface_mapping(
            Size {
                width: 2560,
                height: 1440,
            },
            Size {
                width: 1920,
                height: 1080,
            },
            4.0 / 3.0,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn validate_surface_mapping_rejects_large_mismatch() {
        let result = validate_surface_mapping(
            Size {
                width: 3840,
                height: 2160,
            },
            Size {
                width: 1920,
                height: 1080,
            },
            1.0,
        );
        assert!(matches!(result, Err(CaptureError::Mapping { .. })));
    }

    #[test]
    fn validate_surface_mapping_rejects_zero_logical() {
        let result = validate_surface_mapping(
            Size {
                width: 1920,
                height: 1080,
            },
            Size {
                width: 0,
                height: 1080,
            },
            1.0,
        );
        assert!(matches!(result, Err(CaptureError::Mapping { .. })));
    }

    #[test]
    fn validate_surface_mapping_rejects_non_positive_scale() {
        let result = validate_surface_mapping(
            Size {
                width: 1920,
                height: 1080,
            },
            Size {
                width: 1920,
                height: 1080,
            },
            0.0,
        );
        assert!(matches!(result, Err(CaptureError::Mapping { .. })));
    }

    #[test]
    fn validate_surface_mapping_rejects_negative_scale() {
        let result = validate_surface_mapping(
            Size {
                width: 1920,
                height: 1080,
            },
            Size {
                width: 1920,
                height: 1080,
            },
            -1.0,
        );
        assert!(matches!(result, Err(CaptureError::Mapping { .. })));
    }

    #[test]
    fn validate_surface_mapping_rejects_nan_scale() {
        let result = validate_surface_mapping(
            Size {
                width: 1920,
                height: 1080,
            },
            Size {
                width: 1920,
                height: 1080,
            },
            f64::NAN,
        );
        assert!(matches!(result, Err(CaptureError::Mapping { .. })));
    }

    #[test]
    fn validate_surface_mapping_rejects_infinite_scale() {
        let result = validate_surface_mapping(
            Size {
                width: 1920,
                height: 1080,
            },
            Size {
                width: 1920,
                height: 1080,
            },
            f64::INFINITY,
        );
        assert!(matches!(result, Err(CaptureError::Mapping { .. })));
    }

    // ── Pixel boundary tests ──

    #[test]
    fn pixel_count_exact_boundary() {
        // 5000 × 8000 = 40,000,000 exactly at the limit
        let result = checked_pixel_count(5000, 8000);
        assert!(result.is_ok());
    }

    #[test]
    fn pixel_count_one_above_boundary() {
        // 6325 * 6325 = 40,005,625 > 40,000,000
        let result = checked_pixel_count(6325, 6325);
        assert!(matches!(result, Err(CaptureError::Mapping { .. })));
    }

    #[test]
    fn pixel_count_overflowing_dimensions() {
        let result = checked_pixel_count(u32::MAX, u32::MAX);
        assert!(matches!(result, Err(CaptureError::Mapping { .. })));
    }

    #[test]
    fn pixel_count_zero_dimension() {
        let result = checked_pixel_count(0, 100);
        assert!(matches!(result, Err(CaptureError::Mapping { .. })));
    }

    // ── CaptureError::Mapping test ──

    #[test]
    fn mapping_error_includes_message() {
        let err = CaptureError::Mapping {
            message: "test mapping failure".to_string(),
        };
        assert!(err.to_string().contains("test mapping failure"));
    }
}
