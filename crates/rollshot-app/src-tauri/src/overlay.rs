use crate::session::OverlayExclusion;

pub fn configure_overlay_window(window: &tauri::WebviewWindow) -> OverlayExclusion {
    let _ = window.set_decorations(false);
    let _ = window.set_always_on_top(true);
    let _ = window.set_skip_taskbar(true);
    let _ = window.set_fullscreen(true);
    let _ = window.set_focus();

    platform_overlay_exclusion(window)
}

#[cfg(target_os = "windows")]
fn platform_overlay_exclusion(window: &tauri::WebviewWindow) -> OverlayExclusion {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
    };

    let Ok(handle) = window.window_handle() else {
        return OverlayExclusion::Unknown;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return OverlayExclusion::Unknown;
    };
    let hwnd = handle.hwnd.get() as HWND;
    let ok = unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) != 0 };
    if ok {
        OverlayExclusion::Verified
    } else {
        OverlayExclusion::Unknown
    }
}

#[cfg(target_os = "linux")]
fn platform_overlay_exclusion(_window: &tauri::WebviewWindow) -> OverlayExclusion {
    OverlayExclusion::Unsupported
}

#[cfg(target_os = "macos")]
fn platform_overlay_exclusion(_window: &tauri::WebviewWindow) -> OverlayExclusion {
    OverlayExclusion::Unknown
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn platform_overlay_exclusion(_window: &tauri::WebviewWindow) -> OverlayExclusion {
    OverlayExclusion::Unknown
}
