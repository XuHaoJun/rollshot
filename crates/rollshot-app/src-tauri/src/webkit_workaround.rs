//! Startup workaround for the Tauri/WebKitGTK + NVIDIA + Wayland crash.
//!
//! WebKitGTK's DMABUF renderer is incompatible with NVIDIA's Wayland driver,
//! leading to `Gdk-Message: Error 71 (Protocol error) dispatching to Wayland
//! display` before the main window is shown. Setting
//! `WEBKIT_DISABLE_DMABUF_RENDERER=1` before WebKitGTK initializes forces
//! the GL renderer path and avoids the crash.
//!
//! References:
//! - <https://github.com/tauri-apps/tauri/issues/10702>

fn is_wayland() -> bool {
    !std::env::var("WAYLAND_DISPLAY")
        .unwrap_or_default()
        .is_empty()
}

fn is_nvidia() -> bool {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let vendor_path = entry.path().join("device/vendor");
        let Ok(vendor_id) = std::fs::read_to_string(&vendor_path) else {
            continue;
        };
        if vendor_id.trim() == "0x10de" {
            return true;
        }
    }
    false
}

pub fn apply() {
    if !is_wayland() || !is_nvidia() {
        return;
    }
    std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    eprintln!(
        "rollshot: NVIDIA + Wayland detected, disabling DMABUF renderer \
         (see https://github.com/tauri-apps/tauri/issues/10702)"
    );
}
