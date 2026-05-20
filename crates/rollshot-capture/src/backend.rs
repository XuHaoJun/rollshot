use crate::error::CaptureError;
use crate::types::{CaptureOptions, CaptureProbe, CapturedFrame};

pub trait CaptureBackend {
    fn name(&self) -> &'static str;
    fn probe(&self) -> CaptureProbe;
    fn start(
        &mut self,
        options: CaptureOptions,
    ) -> Result<Box<dyn FrameStream>, CaptureError>;
}

pub trait FrameStream: Send {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError>;
}
