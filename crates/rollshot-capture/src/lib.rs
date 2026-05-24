pub mod backend;
pub mod crop;
pub mod error;
pub mod fake;
pub mod fixture;
pub mod types;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(all(target_os = "macos", feature = "macos-sck"))]
pub mod macos;

pub use backend::{default_backend, default_backend_for, BackendKind, CaptureBackend, FrameStream};
pub use crop::crop_frame;
pub use error::CaptureError;
pub use fake::FakeFrameStream;
pub use fixture::{FixtureBackend, FixtureFrameStream};
#[cfg(target_os = "linux")]
pub use linux::LinuxPortalBackend;
#[cfg(all(target_os = "macos", feature = "macos-sck"))]
pub use macos::MacosScreenCaptureKitBackend;
pub use types::{
    CaptureOptions, CaptureProbe, CapturedFrame, FrameMetadata, InteractiveLaunchOptions,
    PixelFormat, Region, RegionMode, Size,
};

#[cfg(all(test, target_os = "linux"))]
pub(crate) static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
