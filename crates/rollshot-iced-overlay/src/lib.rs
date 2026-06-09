//! Iced capture overlay renderer.
//!
//! Linux currently uses the iced/layer-shell runner. macOS and Windows compile
//! to an unsupported result until their normal-window runners land. The crate is
//! named for the renderer framework so it can coexist with the retained Tauri
//! overlay during validation.

use image::RgbaImage;
use rollshot_core::StitchStats;

/// Post-overlay action the caller should perform after the overlay closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PostOverlayRequest {
    /// No further action — the caller decides what to do with the result.
    #[default]
    None,
    /// Open a native Save As dialog for the captured image.
    SaveAs,
}

/// The finalized capture handed back to the caller (Tauri in Phase 4).
/// Named generically per architecture spec D5 — not "save PNG only".
#[derive(Debug, Clone)]
pub struct CaptureResult {
    pub image: RgbaImage,
    pub stats: Option<StitchStats>,
    pub post_overlay_request: PostOverlayRequest,
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
    /// Returned on non-Linux targets.
    Unsupported,
    Capture(String),
    Overlay(String),
}

impl std::fmt::Display for OverlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OverlayError::Unsupported => write!(f, "overlay is only supported on Linux"),
            OverlayError::Capture(m) => write!(f, "capture error: {m}"),
            OverlayError::Overlay(m) => write!(f, "overlay error: {m}"),
        }
    }
}

impl std::error::Error for OverlayError {}

mod output;
mod result_review;
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
mod macos_runner;
#[cfg(target_os = "macos")]
mod macos_window;
pub mod screenshot;

/// Run the capture overlay, blocking the calling thread until the user
/// finishes (Esc) or cancels. `Ok(Some(_))` on finish, `Ok(None)` on cancel.
pub fn run_overlay(config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    #[cfg(target_os = "linux")]
    {
        linux_runner::run(config)
    }
    #[cfg(target_os = "macos")]
    {
        macos_runner::run(config)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = config;
        Err(OverlayError::Unsupported)
    }
}
