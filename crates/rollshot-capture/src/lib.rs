pub mod backend;
pub mod error;
pub mod fake;
pub mod fixture;
pub mod types;

#[cfg(target_os = "linux")]
pub mod linux;

pub use backend::{default_backend, default_backend_for, BackendKind, CaptureBackend, FrameStream};
pub use error::CaptureError;
pub use fake::FakeFrameStream;
pub use fixture::{FixtureBackend, FixtureFrameStream};
#[cfg(target_os = "linux")]
pub use linux::LinuxPortalBackend;
pub use types::{
    CaptureOptions, CaptureProbe, CapturedFrame, FrameMetadata, PixelFormat, Region, RegionMode,
    Size,
};
