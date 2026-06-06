use iced::window;

/// Describes a display's geometry in both logical and physical coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScreenGeometry {
    /// Logical origin of the screen (may be negative for secondary displays).
    pub logical_origin: (f64, f64),
    /// Logical size of the screen.
    pub logical_size: (f64, f64),
    /// Backing scale factor (e.g. 2.0 on Retina).
    pub scale_factor: f64,
}

/// Pure helper: given a list of `(display_id, geometry)` entries, find the one
/// matching `target_display_id` and return its geometry.
///
/// Returns `None` if no screen matches the target ID.
pub(crate) fn resolve_display_screen(
    target_display_id: u32,
    screens: &[(u32, ScreenGeometry)],
) -> Option<ScreenGeometry> {
    screens
        .iter()
        .find(|(id, _)| *id == target_display_id)
        .map(|(_, geom)| *geom)
}

/// Query the running macOS session for every `NSScreen` whose
/// `NSScreenNumber` matches `target_display_id`. Returns the matching screen's
/// logical frame and backing scale, preserving signed/negative origins.
///
/// Returns `Err` if the display ID is not found or the geometry is invalid.
#[cfg(target_os = "macos")]
pub(crate) fn display_screen_geometry(target_display_id: u32) -> Result<ScreenGeometry, String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;

    let _mtm = MainThreadMarker::new()
        .ok_or_else(|| "NSScreen queries must run on the main thread".to_string())?;

    let screens = NSScreen::screens(None);
    for screen in screens.iter() {
        let desc = screen.deviceDescription();
        // NSScreenNumber is stored as an NSNumber in the device description.
        // We match against the target display ID.
        if let Some(ns_screen_number) = get_screen_number_from_description(&desc) {
            if ns_screen_number == target_display_id {
                let frame = screen.frame();
                let scale = screen.backingScaleFactor();
                if scale <= 0.0 {
                    return Err(format!(
                        "invalid backing scale factor {scale} for display {target_display_id}"
                    ));
                }
                return Ok(ScreenGeometry {
                    logical_origin: (frame.origin.x, frame.origin.y),
                    logical_size: (frame.size.width, frame.size.height),
                    scale_factor: scale,
                });
            }
        }
    }
    Err(format!(
        "no NSScreen found with display ID {target_display_id}"
    ))
}

/// Extract the `NSScreenNumber` from an `NSDeviceDescription` dictionary.
#[cfg(target_os = "macos")]
fn get_screen_number_from_description(
    desc: &objc2_app_kit::NSDeviceDescription,
) -> Option<u32> {
    use objc2::runtime::AnyObject;
    use objc2_foundation::NSString;

    let key = NSString::from_str("NSScreenNumber");
    let value = desc.objectForKey(&key)?;
    // NSNumber bridged to AnyObject — use integerValue then cast.
    let int_val: i64 = unsafe { msg_send![&*value, integerValue] };
    Some(int_val as u32)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(display_id: u32, x: f64, y: f64, w: f64, h: f64, scale: f64) -> (u32, ScreenGeometry) {
        (
            display_id,
            ScreenGeometry {
                logical_origin: (x, y),
                logical_size: (w, h),
                scale_factor: scale,
            },
        )
    }

    #[test]
    fn resolve_finds_matching_display_by_id() {
        let screens = vec![
            screen(1, 0.0, 0.0, 1920.0, 1080.0, 1.0),
            screen(2, -1920.0, 0.0, 2560.0, 1440.0, 2.0),
        ];
        let geom = resolve_display_screen(2, &screens).expect("should find display 2");
        assert_eq!(geom.logical_origin, (-1920.0, 0.0));
        assert_eq!(geom.logical_size, (2560.0, 1440.0));
        assert_eq!(geom.scale_factor, 2.0);
    }

    #[test]
    fn resolve_returns_none_for_missing_id() {
        let screens = vec![screen(1, 0.0, 0.0, 1920.0, 1080.0, 1.0)];
        assert!(resolve_display_screen(99, &screens).is_none());
    }

    #[test]
    fn resolve_preserves_negative_origin() {
        let screens = vec![screen(3, -2560.0, -100.0, 2560.0, 1440.0, 2.0)];
        let geom = resolve_display_screen(3, &screens).expect("should find display 3");
        assert_eq!(geom.logical_origin, (-2560.0, -100.0));
    }

    #[test]
    fn resolve_preserves_signed_origin_values() {
        let screens = vec![screen(5, -100.0, 200.0, 1280.0, 720.0, 1.5)];
        let geom = resolve_display_screen(5, &screens).expect("should find display 5");
        assert_eq!(geom.logical_origin, (-100.0, 200.0));
        assert_eq!(geom.scale_factor, 1.5);
    }

    #[test]
    fn resolve_empty_screen_list_returns_none() {
        assert!(resolve_display_screen(1, &[]).is_none());
    }

    #[test]
    fn screenshot_window_size_uses_target_display_logical_size_and_scale() {
        let screens = vec![
            screen(1, 0.0, 0.0, 1920.0, 1080.0, 1.0),
            screen(2, -2560.0, 0.0, 2560.0, 1440.0, 2.0),
        ];
        let geom = resolve_display_screen(2, &screens).expect("should find display 2");
        // Window size should be logical size of the target display
        assert_eq!(geom.logical_size, (2560.0, 1440.0));
        // Scale should be the target display's backing scale
        assert_eq!(geom.scale_factor, 2.0);
    }
}
