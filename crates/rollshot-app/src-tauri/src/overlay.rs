use crate::session::OverlayExclusion;

#[cfg(target_os = "linux")]
pub fn initial_overlay_exclusion() -> OverlayExclusion {
    OverlayExclusion::Unsupported
}

#[cfg(not(target_os = "linux"))]
pub fn initial_overlay_exclusion() -> OverlayExclusion {
    OverlayExclusion::Unknown
}

pub fn configure_overlay_window(window: &tauri::WebviewWindow) -> OverlayExclusion {
    let _ = window.set_decorations(false);
    let _ = window.set_always_on_top(true);
    let _ = window.set_skip_taskbar(true);
    let _ = window.set_fullscreen(true);
    let _ = window.set_focus();
    initial_overlay_exclusion()
}
