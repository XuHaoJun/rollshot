//! AppKit native file-drag bridge for the floating post-capture thumbnail.
//!
//! Portable/macOS split (registered on EVERY target so the pure placement and
//! result helpers compile and unit-test on Linux):
//!
//! - PORTABLE (compiled + tested on all targets): [`ScreenFrame`],
//!   [`thumbnail_origin`], [`NativeDragResult`], and [`drag_result`]. These hold
//!   no AppKit types — the geometry types come from `iced`, a dependency on all
//!   targets. The Linux tests exercise exactly these pieces.
//! - macOS-ONLY: [`active_screen_thumbnail_origin`], [`patch_thumbnail_window`],
//!   and [`begin_file_drag`] reach into AppKit (`NSScreen`, `NSWindow`,
//!   `NSView`, the drag session). All `unsafe` is confined to a single audited
//!   `#[allow(unsafe_code)]` bridge, mirroring
//!   `rollshot-iced-overlay`'s `macos_window.rs`. They cannot be compiled on
//!   Linux (the objc2 AppKit deps are `cfg(target_os = "macos")`), so they are
//!   verified on macOS; the portable helpers are what the Linux tests cover.
//!
//! On non-macOS targets [`begin_file_drag`] is an explicit unsupported stub so
//! the wiring type-checks everywhere.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::Path;
use std::sync::atomic::AtomicU8;
use std::sync::Arc;

use iced::{Point, Size};

/// Outcome of a native drag session, published into the shared atomic the host
/// polls. The discriminants are the wire format stored in the `AtomicU8`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeDragResult {
    /// Drag in flight; no terminal operation reported yet.
    Pending = 0,
    /// The drop was accepted (any non-`None` `NSDragOperation`).
    Succeeded = 1,
    /// The drag was released over nothing (`NSDragOperation::None`).
    Cancelled = 2,
}

/// A display's frame in AppKit's bottom-left logical coordinate space. Origins
/// may be negative for displays left of / below the primary screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenFrame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl ScreenFrame {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// PURE: lower-right placement of a `size`-sized card within `frame`, returned in
/// a TOP-LEFT coordinate frame (origin = the card's top-left corner, Y growing
/// downward from the frame's top edge). This matches the convention winit/iced
/// `Position::Specific` expects, so the result can be fed straight through after
/// `active_screen_thumbnail_origin` maps the active NSScreen frame into the
/// winit/main-display top-left space.
///
/// `x = frame.x + frame.width - size.width - margin` (right-inset; no X flip,
/// both spaces share a left origin)
/// `y = frame.y + frame.height - size.height - margin` (the card's top edge,
/// measured downward, so the card sits `margin` above the frame's bottom)
pub fn thumbnail_origin(frame: ScreenFrame, size: Size, margin: f32) -> Point {
    Point::new(
        frame.x + frame.width - size.width - margin,
        frame.y + frame.height - size.height - margin,
    )
}

/// PURE: map an `NSDragOperation`-was-non-`None` flag to a terminal result.
pub fn drag_result(succeeded: bool) -> NativeDragResult {
    if succeeded {
        NativeDragResult::Succeeded
    } else {
        NativeDragResult::Cancelled
    }
}

/// macOS: resolve the lower-right origin of the thumbnail on the screen
/// currently under the pointer, inset by `margin` (the host passes 24 pt).
#[cfg(target_os = "macos")]
pub fn active_screen_thumbnail_origin(size: Size, margin: f32) -> Result<Point, String> {
    active_screen_thumbnail_origin_impl(size, margin)
}

/// macOS: patch the iced thumbnail `NSWindow` so the floating card has no
/// shadow/opacity chrome, joins all spaces, stays always-on-top, and accepts
/// mouse events.
#[cfg(target_os = "macos")]
pub fn patch_thumbnail_window(handle: &dyn iced::window::Window) -> Result<(), String> {
    patch_thumbnail_window_impl(handle)
}

/// macOS: begin a native AppKit file drag of `saved_path` from the thumbnail
/// view, publishing the terminal result into `status`.
#[cfg(target_os = "macos")]
pub fn begin_file_drag(
    handle: &dyn iced::window::Window,
    saved_path: &Path,
    status: Arc<AtomicU8>,
) -> Result<(), String> {
    begin_file_drag_impl(handle, saved_path, status)
}

/// Non-macOS stub: the native drag is an AppKit-only path.
#[cfg(not(target_os = "macos"))]
pub fn begin_file_drag(
    _handle: &dyn iced::window::Window,
    _saved_path: &Path,
    _status: Arc<AtomicU8>,
) -> Result<(), String> {
    Err("native drag unsupported on this platform".into())
}

// ---------------------------------------------------------------------------
// macOS AppKit bridge — the single audited unsafe boundary.
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn active_screen_thumbnail_origin_impl(size: Size, margin: f32) -> Result<Point, String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSScreen};

    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "NSScreen queries must run on the main thread".to_string())?;

    // `mouseLocation` is in the global (primary-screen bottom-left) space.
    let mouse = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);

    let mut chosen: Option<ScreenFrame> = None;
    for screen in screens.iter() {
        let f = screen.frame();
        let contains = mouse.x >= f.origin.x
            && mouse.x < f.origin.x + f.size.width
            && mouse.y >= f.origin.y
            && mouse.y < f.origin.y + f.size.height;
        let frame = ScreenFrame::new(
            f.origin.x as f32,
            f.origin.y as f32,
            f.size.width as f32,
            f.size.height as f32,
        );
        if contains {
            chosen = Some(frame);
            break;
        }
        if chosen.is_none() {
            chosen = Some(frame);
        }
    }

    let frame =
        chosen.ok_or_else(|| "no NSScreen available for thumbnail placement".to_string())?;

    // COORDINATE CONVERSION (the Y-flip).
    //
    // `frame` is in NSScreen's BOTTOM-LEFT space (Y grows up, primary screen at
    // origin). iced/winit `Position::Specific` is TOP-LEFT: winit computes
    // `cocoa_y = main_display_height - window_height - position.y`, i.e. it treats
    // the passed `y` as the window's TOP edge measured DOWNWARD from the top of
    // the MAIN display (`CGMainDisplayID`) — NOT the active screen.
    //
    // `thumbnail_origin` already returns a lower-right placement expressed in a
    // top-left frame (`y = frame.y + frame.height - size.height - margin`). For
    // the PRIMARY/MAIN display the frame origin is (0, 0) and its height equals
    // the main display height, so `thumbnail_origin` directly yields the correct
    // top-left Y measured from the top of the main display → the card lands in
    // the true lower-right.
    //
    // MULTI-DISPLAY LIMITATION (MVP): for a SECONDARY display, winit still flips
    // against the main display height, and the active screen's top edge is offset
    // from the main display's top by `main_height - (frame.y + frame.height)`.
    // We do not yet apply that offset here, so the card may be mispositioned on
    // secondary displays. Handling this correctly requires the main display
    // height (via `CGMainDisplayID`/`NSScreen::mainScreen`) and is deferred to a
    // follow-up. The primary display — the common case — is correct today.
    Ok(thumbnail_origin(frame, size, margin))
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn patch_thumbnail_window_impl(handle: &dyn iced::window::Window) -> Result<(), String> {
    use iced::window::raw_window_handle::RawWindowHandle;
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSStatusWindowLevel, NSView, NSWindowCollectionBehavior};

    let raw = handle
        .window_handle()
        .map_err(|err| format!("failed to read macOS window handle: {err}"))?
        .as_raw();

    let RawWindowHandle::AppKit(appkit) = raw else {
        return Err("expected AppKit window handle for macOS thumbnail".to_string());
    };

    let _mtm = MainThreadMarker::new()
        .ok_or_else(|| "macOS thumbnail patch must run on the main thread".to_string())?;

    let view = appkit.ns_view.as_ptr() as *mut NSView;
    let view = unsafe {
        Retained::retain(view).ok_or_else(|| "failed to retain thumbnail NSView".to_string())?
    };

    let ns_window = view
        .window()
        .ok_or_else(|| "thumbnail NSView is not attached to an NSWindow".to_string())?;

    ns_window.setHasShadow(false);
    ns_window.setOpaque(false);
    ns_window.setIgnoresMouseEvents(false);
    // Float above normal windows; `NSStatusWindowLevel` keeps it above the
    // dock/menu without becoming a screen-saver-level overlay.
    ns_window.setLevel(NSStatusWindowLevel);
    ns_window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary,
    );

    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn begin_file_drag_impl(
    handle: &dyn iced::window::Window,
    saved_path: &Path,
    status: Arc<AtomicU8>,
) -> Result<(), String> {
    use iced::window::raw_window_handle::RawWindowHandle;
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSDraggingItem, NSEventType, NSView};
    use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString, NSURL};

    let raw = handle
        .window_handle()
        .map_err(|err| format!("failed to read macOS window handle: {err}"))?
        .as_raw();

    let RawWindowHandle::AppKit(appkit) = raw else {
        return Err("expected AppKit window handle for macOS thumbnail".to_string());
    };

    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "native drag must run on the main thread".to_string())?;

    // 1. Retain the iced NSView the drag originates from.
    let view = appkit.ns_view.as_ptr() as *mut NSView;
    let view = unsafe {
        Retained::retain(view).ok_or_else(|| "failed to retain thumbnail NSView".to_string())?
    };

    // 2. The current event must be a left-mouse drag for the session to start.
    let app = NSApplication::sharedApplication(mtm);
    let event = app
        .currentEvent()
        .ok_or_else(|| "no current NSEvent to seed the drag session".to_string())?;
    let kind = event.r#type();
    if kind != NSEventType::LeftMouseDragged && kind != NSEventType::LeftMouseDown {
        return Err(format!(
            "native drag requires a left-mouse event, got {kind:?}"
        ));
    }

    // 3. A file URL for the saved capture (conforms to NSPasteboardWriting).
    let path_str = saved_path
        .to_str()
        .ok_or_else(|| "saved path is not valid UTF-8".to_string())?;
    // `NSURL::fileURLWithPath` is a safe `pub fn` in objc2-foundation 0.3.2 — no
    // `unsafe` wrapper (it would trip `unused_unsafe` under `-D warnings`).
    let url = NSURL::fileURLWithPath(&NSString::from_str(path_str));
    let writer = ProtocolObject::from_ref(&*url);

    // 4. A dragging item carrying the file URL on the pasteboard.
    let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), writer);

    // 5. Frame the drag image at the originating view's bounds.
    let bounds = view.bounds();
    let frame = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(bounds.size.width, bounds.size.height),
    );
    item.setDraggingFrame(frame);

    let items = NSArray::from_retained_slice(&[item]);

    // 6. A dragging source advertising a Copy operation and recording the
    //    terminal operation into the shared atomic.
    let source = DragSource::new(mtm, Arc::clone(&status));
    let source_proto = ProtocolObject::from_ref(&*source);

    // 8. Start the session from the view, seeded with the current event.
    //
    // SAFETY/lifetime: our local `Retained<DragSource>` (`source`) drops when this
    // fn returns, while the drag continues to run asynchronously. This is sound
    // because, per Apple's `NSDraggingSource`/`beginDraggingSessionWithItems:...`
    // contract, AppKit strongly retains the source for the entire lifetime of the
    // dragging session. The `draggingSession:endedAtPoint:operation:` callback —
    // which writes the shared `AtomicU8` the host polls — fires BEFORE AppKit
    // releases the source, so the source (and its ivars/atomic) outlive every use.
    // We therefore do not stash `source` in host state.
    //
    // TODO(macos-verify): confirm drop/cancel reliably updates the atomic on a
    // real macOS run.
    let _session = view.beginDraggingSessionWithItems_event_source(&items, &event, source_proto);

    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod drag_source {
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::Arc;

    use objc2::rc::Retained;
    use objc2::runtime::NSObjectProtocol;
    use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSDragOperation, NSDraggingContext, NSDraggingSession, NSDraggingSource};
    use objc2_foundation::{NSObject, NSPoint};

    use super::drag_result;

    pub(super) struct Ivars {
        pub status: Arc<AtomicU8>,
    }

    define_class!(
        // The drag source must live on the main thread alongside AppKit.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[ivars = Ivars]
        pub(super) struct DragSource;

        unsafe impl NSObjectProtocol for DragSource {}

        unsafe impl NSDraggingSource for DragSource {
            // 7a. Advertise a Copy operation out of the application.
            #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
            fn source_operation_mask(
                &self,
                _session: &NSDraggingSession,
                _context: NSDraggingContext,
            ) -> NSDragOperation {
                NSDragOperation::Copy
            }

            // 7b. Record the terminal operation: non-`None` → Succeeded.
            #[unsafe(method(draggingSession:endedAtPoint:operation:))]
            fn ended_at_point_operation(
                &self,
                _session: &NSDraggingSession,
                _screen_point: NSPoint,
                operation: NSDragOperation,
            ) {
                let succeeded = operation != NSDragOperation::None;
                self.ivars()
                    .status
                    .store(drag_result(succeeded) as u8, Ordering::SeqCst);
            }
        }
    );

    impl DragSource {
        pub(super) fn new(mtm: MainThreadMarker, status: Arc<AtomicU8>) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(Ivars { status });
            unsafe { msg_send![super(this), init] }
        }
    }
}

#[cfg(target_os = "macos")]
use drag_source::DragSource;

#[cfg(test)]
mod tests {
    use super::*;

    // -- PORTABLE thumbnail_origin tests (run on Linux) ----------------------

    #[test]
    fn lower_right_origin_uses_screen_frame_and_margin() {
        // Top-left frame: x = -1440 + 1440 - 280 - 24 = -304;
        // y = 0 + 900 - 220 - 24 = 656 (card top edge, margin above the bottom).
        assert_eq!(
            thumbnail_origin(
                ScreenFrame::new(-1440.0, 0.0, 1440.0, 900.0),
                Size::new(280.0, 220.0),
                24.0
            ),
            Point::new(-304.0, 656.0)
        );
    }

    #[test]
    fn lower_right_origin_on_primary_display() {
        // Primary display at origin (0,0), 1920x1080, 300x200 card, 16pt margin.
        // Top-left winit frame: x = 0 + 1920 - 300 - 16 = 1604;
        // y = 0 + 1080 - 200 - 16 = 864 (card top edge, 16pt above the bottom).
        assert_eq!(
            thumbnail_origin(
                ScreenFrame::new(0.0, 0.0, 1920.0, 1080.0),
                Size::new(300.0, 200.0),
                16.0
            ),
            Point::new(1604.0, 864.0)
        );
    }

    // -- PORTABLE drag_result tests ------------------------------------------

    #[test]
    fn drag_operation_maps_none_to_cancelled_and_copy_to_success() {
        assert_eq!(drag_result(false), NativeDragResult::Cancelled);
        assert_eq!(drag_result(true), NativeDragResult::Succeeded);
    }

    #[test]
    fn native_drag_result_round_trips_through_its_u8_repr() {
        for r in [
            NativeDragResult::Pending,
            NativeDragResult::Succeeded,
            NativeDragResult::Cancelled,
        ] {
            let raw = r as u8;
            let back = match raw {
                0 => NativeDragResult::Pending,
                1 => NativeDragResult::Succeeded,
                2 => NativeDragResult::Cancelled,
                other => panic!("unexpected repr {other}"),
            };
            assert_eq!(r, back);
        }
        // Pin the wire format the host atomic relies on.
        assert_eq!(NativeDragResult::Pending as u8, 0);
        assert_eq!(NativeDragResult::Succeeded as u8, 1);
        assert_eq!(NativeDragResult::Cancelled as u8, 2);
    }
}
