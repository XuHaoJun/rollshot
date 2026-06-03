use iced::window;

#[allow(dead_code)]
pub(crate) fn apply_overlay_window_patch(_id: window::Id) {
    // The concrete AppKit calls are isolated here so runner code does not
    // depend on Objective-C symbols directly. This scaffold is a no-op until
    // the macOS runner task replaces it with tested AppKit calls.
}
