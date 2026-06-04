use iced::window;

/// The backing scale factor of the main display (e.g. 2.0 on Retina).
///
/// iced window sizes are logical points, but the ScreenCaptureKit frame is in
/// physical pixels, so the overlay window must be created at
/// `source_size / scale` to cover the screen 1:1 and keep
/// `map_crop_to_frame`'s `source_size / overlay_logical` ratio equal to the
/// true device scale. Returns `None` off the main thread or when no main
/// screen is available.
pub(crate) fn main_screen_scale_factor() -> Option<f64> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;

    let mtm = MainThreadMarker::new()?;
    let screen = NSScreen::mainScreen(mtm)?;
    Some(screen.backingScaleFactor())
}

pub(crate) fn apply_overlay_window_patch(handle: &dyn window::Window) -> Result<(), String> {
    apply_overlay_window_patch_impl(handle)
}

#[allow(unsafe_code)]
fn apply_overlay_window_patch_impl(handle: &dyn window::Window) -> Result<(), String> {
    use iced::window::raw_window_handle::RawWindowHandle;
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSView, NSWindowCollectionBehavior};

    let raw = handle
        .window_handle()
        .map_err(|err| format!("failed to read macOS window handle: {err}"))?
        .as_raw();

    let RawWindowHandle::AppKit(appkit) = raw else {
        return Err("expected AppKit window handle for macOS iced overlay".to_string());
    };

    let _mtm = MainThreadMarker::new()
        .ok_or_else(|| "macOS window patch must run on the main thread".to_string())?;

    let view = appkit.ns_view.as_ptr() as *mut NSView;
    let view = unsafe {
        Retained::retain(view).ok_or_else(|| "failed to retain iced NSView".to_string())?
    };

    let ns_window = view
        .window()
        .ok_or_else(|| "iced NSView is not attached to an NSWindow".to_string())?;

    ns_window.setHasShadow(false);
    ns_window.setOpaque(false);
    ns_window.setIgnoresMouseEvents(false);
    ns_window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary,
    );

    Ok(())
}
