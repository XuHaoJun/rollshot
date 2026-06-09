//! Iced capture overlay renderer.
//!
//! Linux currently uses the iced/layer-shell runner. macOS and Windows compile
//! to an unsupported result until their normal-window runners land. The crate is
//! named for the renderer framework so it can coexist with the retained Tauri
//! overlay during validation.

use image::RgbaImage;
use rollshot_core::StitchStats;

/// The finalized capture handed back to the caller. The overlay is capture-only:
/// it selects a region, captures/stitches, and returns the image plus stats.
#[derive(Debug, Clone)]
pub struct CaptureResult {
    pub image: RgbaImage,
    pub stats: Option<StitchStats>,
}

/// Inputs for a capture session.
#[derive(Debug, Clone)]
pub struct OverlayConfig {
    /// Backend selector, e.g. "auto" / "linux-portal" (BackendKind::from_cli_flag).
    pub backend: String,
    pub fps: u32,
    pub show_cursor: bool,
    pub initial_mode: rollshot_capture::CaptureMode,
}

#[derive(Debug)]
pub enum OverlayError {
    /// Returned by the blocking `run_overlay` on non-Linux targets: the blocking
    /// overlay runner is Linux-only. The active macOS path is the embedded
    /// [`macos_capture::Component`] hosted by `rollshot-app`'s product daemon.
    Unsupported,
    Capture(String),
    Overlay(String),
}

impl std::fmt::Display for OverlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OverlayError::Unsupported => write!(
                f,
                "the blocking overlay runner is Linux-only; the active macOS path is the embedded capture component hosted by rollshot-app"
            ),
            OverlayError::Capture(m) => write!(f, "capture error: {m}"),
            OverlayError::Overlay(m) => write!(f, "overlay error: {m}"),
        }
    }
}

impl std::error::Error for OverlayError {}

mod toolbar;
mod workspace;

// TODO: uncomment these mod declarations as Tasks 3–7 land each module.
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod app;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod coords;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod driver;
#[cfg(target_os = "linux")]
mod linux_runner;
#[cfg(target_os = "macos")]
pub mod macos_capture;
#[cfg(target_os = "macos")]
mod macos_window;
pub mod screenshot;

/// Run the blocking capture overlay, blocking the calling thread until the user
/// finishes (Esc) or cancels. `Ok(Some(_))` on finish, `Ok(None)` on cancel.
///
/// This blocking runner is Linux-only. On macOS the active path is the embedded
/// [`macos_capture::Component`] hosted by `rollshot-app`'s single-process
/// product daemon (it owns the event loop and the post-capture flow), so there
/// is no blocking macOS runner here.
pub fn run_overlay(config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    #[cfg(target_os = "linux")]
    {
        linux_runner::run(config)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = config;
        Err(OverlayError::Unsupported)
    }
}
