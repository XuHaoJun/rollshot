pub mod backend;
pub mod crop;
pub mod error;
pub mod fake;
pub mod fixture;
pub mod one_shot;
pub mod types;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;

pub use backend::{default_backend, default_backend_for, BackendKind, CaptureBackend, FrameStream};
pub use crop::{crop_frame, crop_image};
pub use error::CaptureError;
pub use fake::FakeFrameStream;
pub use fixture::{FixtureBackend, FixtureFrameStream};
#[cfg(target_os = "linux")]
pub use linux::one_shot::LinuxKwinOneShotBackend;
#[cfg(target_os = "linux")]
pub use linux::LinuxPortalBackend;
#[cfg(target_os = "macos")]
pub use macos::{display_id_under_cursor, display_logical_bounds, MacosScreenCaptureKitBackend};
pub use one_shot::{
    one_shot_backend_for, validate_surface_mapping, DisplayTarget, OneShotBackendKind,
    OneShotCapture, OneShotCaptureBackend, MAX_ONE_SHOT_PIXELS,
};
pub use types::{
    CaptureMode, CaptureOptions, CaptureProbe, CapturedFrame, FrameMetadata,
    InteractiveLaunchOptions, OverlayMode, PixelFormat, Region, RegionMode, Size,
};

#[cfg(test)]
pub(crate) static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
