use std::time::Duration;

use crate::backend::FrameStream;
use crate::error::CaptureError;
use crate::types::CapturedFrame;

pub const NEXT_FRAME_TIMEOUT: Duration = Duration::from_secs(5);

pub struct LinuxPortalFrameStream;

impl FrameStream for LinuxPortalFrameStream {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        Err(CaptureError::NotImplemented {
            backend: "linux-portal-pipewire",
        })
    }
}
