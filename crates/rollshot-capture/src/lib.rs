pub mod backend;
pub mod error;
pub mod fake;
pub mod fixture;
pub mod types;

pub use backend::{CaptureBackend, FrameStream};
pub use error::CaptureError;
pub use fake::FakeFrameStream;
pub use fixture::{FixtureBackend, FixtureFrameStream};
pub use types::{
    CaptureOptions, CaptureProbe, CapturedFrame, FrameMetadata, PixelFormat, Region, RegionMode,
    Size,
};
