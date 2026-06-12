#![cfg(target_os = "macos")]

pub mod one_shot;
mod options;
mod pixel;

use anyhow::anyhow;
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::backend::{CaptureBackend, FrameStream};
use crate::diagnostics::TARGET_MACOS_SCK;
use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe, CapturedFrame, Region};

use options::{manual_region, options_to_scap_options, NO_PERMISSION_PROMPT_ENV};
use pixel::captured_frame_from_bgra;

pub(super) const BACKEND_NAME: &str = "macos-sck";

/// The `CGDirectDisplayID` of the display under the cursor, or `None` when the
/// lookup fails. Hosts resolve this once and pass it to both the capture
/// stream ([`crate::CaptureOptions::target_display_id`]) and their overlay
/// window placement so the two cannot disagree.
pub fn display_id_under_cursor() -> Option<u32> {
    rollshot_macos_oneshot::display_id_under_cursor().ok()
}

/// Logical bounds of a display in Core Graphics global coordinates (points,
/// top-left origin — the convention winit window positions use). `None` when
/// the display id is unknown or reports an empty rect.
pub fn display_logical_bounds(display_id: u32) -> Option<Region> {
    let (x, y, width, height) = rollshot_macos_oneshot::display_logical_bounds(display_id).ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some(Region {
        x,
        y,
        width,
        height,
    })
}
const SCAP_VERSION: &str = "0.1.0-beta.1";
const EMPTY_FRAME_LIMIT: u8 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameProcessOutcome {
    Empty,
}

pub struct MacosScreenCaptureKitBackend;

impl MacosScreenCaptureKitBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MacosScreenCaptureKitBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptureBackend for MacosScreenCaptureKitBackend {
    fn name(&self) -> &'static str {
        BACKEND_NAME
    }

    fn probe(&self) -> CaptureProbe {
        let supported = scap::is_supported();
        let permitted = scap::has_permission();
        let available = supported && permitted;

        let message = match (supported, permitted) {
            (false, _) => "scap does not support this macOS host".to_string(),
            (true, false) => "Screen Recording permission is missing".to_string(),
            (true, true) => "scap macOS capture is available".to_string(),
        };

        CaptureProbe {
            backend: BACKEND_NAME,
            available,
            message,
            details: vec![
                ("os".to_string(), "macos".to_string()),
                ("scap_version".to_string(), SCAP_VERSION.to_string()),
                ("scap_supported".to_string(), supported.to_string()),
                (
                    "screen_recording_permission".to_string(),
                    if permitted {
                        "granted".to_string()
                    } else {
                        "missing".to_string()
                    },
                ),
            ],
        }
    }

    fn start(&mut self, options: CaptureOptions) -> Result<Box<dyn FrameStream>, CaptureError> {
        if !scap::is_supported() {
            tracing::warn!(target: TARGET_MACOS_SCK, "scap not supported on this host");
            return Err(CaptureError::Unsupported {
                message: "scap macOS capture requires macOS 12.3 or newer".to_string(),
            });
        }

        if !scap::has_permission()
            && (std::env::var(NO_PERMISSION_PROMPT_ENV).ok().as_deref() == Some("1")
                || !scap::request_permission())
        {
            tracing::warn!(target: TARGET_MACOS_SCK, "Screen Recording permission denied");
            return Err(CaptureError::PermissionDenied {
                message: "Screen Recording permission is required for macOS capture".to_string(),
            });
        }

        let region_category = match &options.region {
            crate::types::RegionMode::Manual(_) => "manual",
            crate::types::RegionMode::PortalPicker => "portal-picker",
            crate::types::RegionMode::FullSource => "full-source",
        };
        tracing::info!(
            target: TARGET_MACOS_SCK,
            fps = options.fps,
            show_cursor = options.show_cursor,
            region = region_category,
            target_display = options.target_display_id.is_some(),
            "capture start requested"
        );

        let effective_region = manual_region(&options.region);
        let scap_options = options_to_scap_options(&options)?;
        let mut capturer = scap::capturer::Capturer::build(scap_options)
            .map_err(capturer_build_error_to_capture_error)?;
        catch_unwind(AssertUnwindSafe(|| capturer.start_capture())).map_err(|payload| {
            tracing::warn!(target: TARGET_MACOS_SCK, "scap failed to start capture");
            CaptureError::Backend(anyhow!(
                "scap failed to start macOS capture: {}",
                panic_payload_to_string(payload)
            ))
        })?;

        tracing::debug!(target: TARGET_MACOS_SCK, "capture started successfully");
        Ok(Box::new(MacosScapFrameStream {
            capturer,
            effective_region,
        }))
    }
}

pub struct MacosScapFrameStream {
    capturer: scap::capturer::Capturer,
    effective_region: Option<Region>,
}

impl Drop for MacosScapFrameStream {
    fn drop(&mut self) {
        tracing::debug!(target: TARGET_MACOS_SCK, "stopping capture");
        let _ = catch_unwind(AssertUnwindSafe(|| self.capturer.stop_capture()));
    }
}

impl FrameStream for MacosScapFrameStream {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        let mut empty_frames = 0;

        loop {
            let frame = self.capturer.get_next_frame().map_err(|_| {
                tracing::debug!(target: TARGET_MACOS_SCK, "stream ended");
                CaptureError::EndOfStream
            })?;

            match process_scap_frame(frame, &mut empty_frames, self.effective_region)? {
                Ok(captured) => {
                    tracing::trace!(
                        target: TARGET_MACOS_SCK,
                        width = captured.image.width(),
                        height = captured.image.height(),
                        "frame captured"
                    );
                    return Ok(captured);
                }
                Err(FrameProcessOutcome::Empty) => continue,
            }
        }
    }
}

fn process_scap_frame(
    frame: scap::frame::Frame,
    empty_frames: &mut u8,
    effective_region: Option<Region>,
) -> Result<Result<CapturedFrame, FrameProcessOutcome>, CaptureError> {
    match frame {
        scap::frame::Frame::BGRA(frame) => {
            if frame.width <= 0 || frame.height <= 0 || frame.data.is_empty() {
                *empty_frames += 1;
                if *empty_frames >= EMPTY_FRAME_LIMIT {
                    tracing::trace!(
                        target: TARGET_MACOS_SCK,
                        idle_count = *empty_frames,
                        "SCK idle frame limit reached"
                    );
                    return Err(CaptureError::Timeout {
                        message: format!("{EMPTY_FRAME_LIMIT} consecutive empty (SCK idle) frames"),
                        duration: std::time::Duration::from_secs(EMPTY_FRAME_LIMIT as u64),
                    });
                }
                return Ok(Err(FrameProcessOutcome::Empty));
            }

            captured_frame_from_bgra(frame, effective_region).map(Ok)
        }
        other => Err(CaptureError::Backend(anyhow!(
            "unsupported scap video frame type: {other:?}"
        ))),
    }
}

fn capturer_build_error_to_capture_error(err: anyhow::Error) -> CaptureError {
    let Some(build_error) = err.downcast_ref::<scap::capturer::CapturerBuildError>() else {
        return CaptureError::Backend(err);
    };

    match build_error {
        scap::capturer::CapturerBuildError::NotSupported => CaptureError::Unsupported {
            message: "scap macOS capture is not supported on this host".to_string(),
        },
        scap::capturer::CapturerBuildError::PermissionNotGranted => {
            CaptureError::PermissionDenied {
                message: "Screen Recording permission is required for macOS capture".to_string(),
            }
        }
    }
}

fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "non-string panic payload".to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        process_scap_frame, MacosScreenCaptureKitBackend, EMPTY_FRAME_LIMIT, SCAP_VERSION,
    };
    use crate::backend::CaptureBackend;
    use crate::error::CaptureError;

    #[test]
    fn probe_reports_scap_details() {
        let probe = MacosScreenCaptureKitBackend::new().probe();

        assert_eq!(probe.backend, "macos-sck");
        assert!(probe.details.iter().any(|(k, v)| k == "os" && v == "macos"));
        assert!(probe
            .details
            .iter()
            .any(|(k, v)| k == "scap_version" && v == SCAP_VERSION));
        assert!(probe
            .details
            .iter()
            .any(|(k, _)| k == "screen_recording_permission"));
    }

    #[test]
    fn process_scap_frame_times_out_after_empty_frame_limit() {
        let mut empty_frames = EMPTY_FRAME_LIMIT - 1;
        let frame = scap::frame::Frame::BGRA(scap::frame::BGRAFrame {
            display_time: 0,
            width: 0,
            height: 0,
            data: Vec::new(),
        });

        // SCK delivers Idle (empty) frames while the screen is static; that is
        // a normal steady state, so the stream must yield a retryable Timeout
        // rather than a fatal error.
        let err = process_scap_frame(frame, &mut empty_frames, None)
            .expect_err("empty frame limit reached");

        assert!(matches!(err, CaptureError::Timeout { .. }));
        assert_eq!(empty_frames, EMPTY_FRAME_LIMIT);
    }
}
