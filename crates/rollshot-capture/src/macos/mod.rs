#![cfg(target_os = "macos")]

mod options;
mod pixel;

use anyhow::anyhow;
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::backend::{CaptureBackend, FrameStream};
use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe, CapturedFrame, Region};

use options::{manual_region, options_to_scap_options, NO_PERMISSION_PROMPT_ENV};
use pixel::captured_frame_from_bgra;

pub(super) const BACKEND_NAME: &str = "macos-sck";
const SCAP_VERSION: &str = "0.1.0-beta.1";
const EMPTY_FRAME_LIMIT: u8 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameProcessOutcome {
    Audio,
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
            return Err(CaptureError::Unsupported {
                message: "scap macOS capture requires macOS 12.3 or newer".to_string(),
            });
        }

        if !scap::has_permission() {
            if std::env::var(NO_PERMISSION_PROMPT_ENV).ok().as_deref() == Some("1")
                || !scap::request_permission()
            {
                return Err(CaptureError::PermissionDenied {
                    message: "Screen Recording permission is required for macOS capture"
                        .to_string(),
                });
            }
        }

        let effective_region = manual_region(&options.region);
        let scap_options = options_to_scap_options(&options)?;
        let mut capturer = scap::capturer::Capturer::build(scap_options)
            .map_err(capturer_build_error_to_capture_error)?;
        catch_unwind(AssertUnwindSafe(|| capturer.start_capture())).map_err(|payload| {
            CaptureError::Backend(anyhow!(
                "scap failed to start macOS capture: {}",
                panic_payload_to_string(payload)
            ))
        })?;

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
        let _ = catch_unwind(AssertUnwindSafe(|| self.capturer.stop_capture()));
    }
}

impl FrameStream for MacosScapFrameStream {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        let mut empty_frames = 0;

        loop {
            let frame = self
                .capturer
                .get_next_frame()
                .map_err(|_| CaptureError::EndOfStream)?;

            match process_scap_frame(frame, &mut empty_frames, self.effective_region)? {
                Ok(captured) => return Ok(captured),
                Err(FrameProcessOutcome::Audio | FrameProcessOutcome::Empty) => continue,
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
        scap::frame::Frame::Audio(_) => Ok(Err(FrameProcessOutcome::Audio)),
        scap::frame::Frame::Video(scap::frame::VideoFrame::BGRA(frame)) => {
            if frame.width <= 0 || frame.height <= 0 || frame.data.is_empty() {
                *empty_frames += 1;
                if *empty_frames >= EMPTY_FRAME_LIMIT {
                    return Err(CaptureError::Backend(anyhow!(
                        "macOS stream did not produce a usable video frame"
                    )));
                }
                return Ok(Err(FrameProcessOutcome::Empty));
            }

            captured_frame_from_bgra(frame, effective_region).map(Ok)
        }
        scap::frame::Frame::Video(other) => Err(CaptureError::Backend(anyhow!(
            "unsupported scap video frame type: {other:?}"
        ))),
    }
}

fn capturer_build_error_to_capture_error(err: scap::capturer::CapturerBuildError) -> CaptureError {
    match err {
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
    fn process_scap_frame_errors_after_empty_frame_limit() {
        let mut empty_frames = EMPTY_FRAME_LIMIT - 1;
        let frame =
            scap::frame::Frame::Video(scap::frame::VideoFrame::BGRA(scap::frame::BGRAFrame {
                display_time: std::time::SystemTime::now(),
                width: 0,
                height: 0,
                data: Vec::new(),
            }));

        let err = process_scap_frame(frame, &mut empty_frames, None)
            .expect_err("empty frame limit reached");

        assert!(matches!(err, CaptureError::Backend(_)));
        assert_eq!(empty_frames, EMPTY_FRAME_LIMIT);
    }
}
