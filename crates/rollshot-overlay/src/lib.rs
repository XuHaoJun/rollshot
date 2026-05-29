//! Native Linux Wayland capture overlay (Phase 3). Linux-only behavior; the
//! crate compiles to a stub on other targets so `cargo build --workspace`
//! works everywhere.

use image::RgbaImage;
use rollshot_core::StitchStats;

/// The finalized capture handed back to the caller (Tauri in Phase 4).
/// Named generically per architecture spec D5 — not "save PNG only".
#[derive(Debug, Clone)]
pub struct CaptureResult {
    pub image: RgbaImage,
    pub stats: StitchStats,
}

/// Inputs for a capture session.
#[derive(Debug, Clone)]
pub struct OverlayConfig {
    /// Backend selector, e.g. "auto" / "linux-portal" (BackendKind::from_cli_flag).
    pub backend: String,
    pub fps: u32,
    pub show_cursor: bool,
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

// TODO: uncomment these mod declarations as Tasks 3–7 land each module.
// #[cfg(target_os = "linux")]
// mod coords;
// #[cfg(target_os = "linux")]
// mod driver;
// #[cfg(target_os = "linux")]
// mod overlay;

/// Run the capture overlay, blocking the calling thread until the user
/// finishes (Esc) or cancels. `Ok(Some(_))` on finish, `Ok(None)` on cancel.
pub fn run_overlay(_config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    // TODO: once linux modules land, gate the real impl behind #[cfg(target_os = "linux")]
    // and restore the non-linux stub returning Err(OverlayError::Unsupported).
    Err(OverlayError::Unsupported)
}
