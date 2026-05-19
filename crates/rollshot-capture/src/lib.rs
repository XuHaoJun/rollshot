pub mod backend;
pub mod types;

pub use backend::{CaptureBackend, FakeFrameStream, FrameStream};
pub use types::{
    CaptureOptions, CaptureProbe, CapturedFrame, FrameMetadata, PixelFormat, Region, RegionMode,
    Size,
};
