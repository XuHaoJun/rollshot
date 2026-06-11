//! # rollshot-macos-oneshot
//!
//! Unsafe-isolation crate for macOS ScreenCaptureKit one-shot screenshot capture.
//!
//! This crate contains ALL unsafe Objective-C FFI code for SCScreenshotManager.
//! The public API is completely safe. On non-macOS platforms, a stub returns
//! `MacosOneShotError::Unsupported`.
//!
//! ## Design rationale
//!
//! Unsafe code is isolated here so the rest of the workspace can maintain
//! `unsafe_code = "forbid"`. Each unsafe block is documented with the
//! Objective-C ownership and buffer-size invariant it relies on.

use thiserror::Error;

/// Captured display screenshot with RGBA pixel data and logical geometry.
#[derive(Debug, Clone)]
pub struct CapturedDisplay {
    /// RGBA pixel data, tightly packed (no row padding).
    pub rgba: Vec<u8>,
    /// Physical pixel width of the captured image.
    pub width: u32,
    /// Physical pixel height of the captured image.
    pub height: u32,
    /// Logical x origin of the display (in points).
    pub logical_x: i32,
    /// Logical y origin of the display (in points).
    pub logical_y: i32,
    /// Logical width of the display (in points).
    pub logical_width: u32,
    /// Logical height of the display (in points).
    pub logical_height: u32,
    /// Core Graphics display identifier.
    pub display_id: u32,
}

/// Errors from the macOS one-shot capture isolation layer.
#[derive(Debug, Error)]
pub enum MacosOneShotError {
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("timeout: {0}")]
    Timeout(String),
    #[error("capture failed: {0}")]
    Capture(String),
}

/// Maximum pixel count (40 megapixels) for safety ceiling.
#[cfg(any(target_os = "macos", test))]
const MAX_PIXELS: u64 = 40_000_000;

/// Callback timeout for SCScreenshotManager (30 seconds).
#[cfg(any(target_os = "macos", test))]
const CALLBACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Resolve a callback result with timeout.
///
/// This pure helper waits for a callback result channel with a bounded timeout.
/// It is separated from unsafe code for testability.
#[cfg(any(target_os = "macos", test))]
fn resolve_callback_with_timeout<T, E>(
    rx: &std::sync::mpsc::Receiver<Result<T, E>>,
    timeout: std::time::Duration,
) -> Result<T, MacosOneShotError>
where
    E: Into<MacosOneShotError>,
{
    match rx.recv_timeout(timeout) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => Err(err.into()),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(MacosOneShotError::Timeout(
            format!("Screenshot callback timed out after {:.0?}", timeout),
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(MacosOneShotError::Capture(
            "Callback channel disconnected".to_string(),
        )),
    }
}

/// Validate dimensions against the 40-megapixel ceiling.
///
/// Returns the pixel count if valid, or an error if dimensions are zero or exceed the limit.
#[cfg(any(target_os = "macos", test))]
fn checked_dimensions(width: u32, height: u32) -> Result<u64, MacosOneShotError> {
    if width == 0 || height == 0 {
        return Err(MacosOneShotError::Capture(format!(
            "zero display dimensions: {width}x{height}"
        )));
    }
    let pixels = (width as u64).checked_mul(height as u64).ok_or_else(|| {
        MacosOneShotError::Capture(format!("pixel count overflow: {width}x{height}"))
    })?;
    if pixels > MAX_PIXELS {
        return Err(MacosOneShotError::Capture(format!(
            "image too large: {pixels} pixels exceeds limit of {MAX_PIXELS}"
        )));
    }
    Ok(pixels)
}

/// Convert BGRA pixel data with padded rows to tightly packed RGBA.
///
/// `bytes_per_row` may be larger than `width * 4` due to CGImage row alignment.
/// Each row is copied respecting its stride, then converted to tightly packed RGBA.
#[cfg(any(target_os = "macos", test))]
fn bgra_padded_to_rgba(
    bgra: &[u8],
    width: u32,
    height: u32,
    bytes_per_row: usize,
) -> Result<Vec<u8>, MacosOneShotError> {
    let w = width as usize;
    let h = height as usize;
    let min_bytes_per_row = w.checked_mul(4).ok_or_else(|| {
        MacosOneShotError::Capture(format!("bytes_per_row overflow: {width} * 4"))
    })?;
    if bytes_per_row < min_bytes_per_row {
        return Err(MacosOneShotError::Capture(format!(
            "bytes_per_row {bytes_per_row} is less than minimum {min_bytes_per_row} for width {width}"
        )));
    }
    let expected_len = bytes_per_row
        .checked_mul(h)
        .ok_or_else(|| MacosOneShotError::Capture("total buffer size overflow".to_string()))?;
    if bgra.len() < expected_len {
        return Err(MacosOneShotError::Capture(format!(
            "BGRA buffer too short: got {} bytes, expected at least {} for {}x{} with bytes_per_row={}",
            bgra.len(), expected_len, width, height, bytes_per_row
        )));
    }

    let mut rgba = Vec::with_capacity(w * h * 4);
    for row in 0..h {
        let row_start = row * bytes_per_row;
        for col in 0..w {
            let px = row_start + col * 4;
            let b = bgra[px];
            let g = bgra[px + 1];
            let r = bgra[px + 2];
            let a = bgra[px + 3];
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    Ok(rgba)
}

/// Logical frame of a display in Core Graphics global coordinates (points,
/// top-left origin, y increasing downward) plus its display id.
///
/// Used to resolve which display the pointer is over. Kept as a pure value type
/// so selection is testable without ScreenCaptureKit.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Copy)]
struct DisplayFrame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    display_id: u32,
}

/// Return the id of the first display whose frame contains the pointer.
///
/// The cursor point and display frames share Core Graphics global coordinates,
/// so a plain half-open point-in-rect test (`[x, x+w) × [y, y+h)`) selects the
/// correct display, including displays with negative origins positioned to the
/// left of or above the primary. Returns `None` when no display contains the
/// point (e.g. a transient off-screen coordinate).
#[cfg(any(target_os = "macos", test))]
fn display_id_containing_point(
    cursor_x: f64,
    cursor_y: f64,
    displays: &[DisplayFrame],
) -> Option<u32> {
    displays
        .iter()
        .find(|d| {
            cursor_x >= d.x
                && cursor_x < d.x + d.width
                && cursor_y >= d.y
                && cursor_y < d.y + d.height
        })
        .map(|d| d.display_id)
}

// ── macOS implementation ──

#[cfg(target_os = "macos")]
mod macos_impl {
    /// Find the CGDirectDisplayID of the display under the current cursor
    /// position without performing a full capture.
    pub fn display_id_under_cursor() -> Result<u32, MacosOneShotError> {
        let (cursor_x, cursor_y) = get_cursor_location()?;
        find_display_under_cursor(cursor_x, cursor_y)
    }

    /// Logical bounds `(x, y, width, height)` of a display in Core Graphics
    /// global coordinates (points, top-left origin).
    pub fn display_logical_bounds(
        display_id: u32,
    ) -> Result<(i32, i32, u32, u32), MacosOneShotError> {
        get_logical_geometry(display_id, 0, 0)
    }

    use super::*;

    /// Capture the display under the current cursor position using SCScreenshotManager.
    ///
    /// # Safety contract
    ///
    /// All Objective-C FFI calls are contained within this function. Unsafe blocks
    /// are documented with their specific invariants.
    pub fn capture_display_under_cursor(
        show_cursor: bool,
    ) -> Result<CapturedDisplay, MacosOneShotError> {
        // 1. Check macOS version (requires 14.0+ for SCScreenshotManager)
        check_macos_version()?;

        // 2. Check/request screen capture permission
        check_permission()?;

        // 3. Get current pointer location
        let (cursor_x, cursor_y) = get_cursor_location()?;

        // 4. Get shareable content and find the display under the cursor
        let display_id = find_display_under_cursor(cursor_x, cursor_y)?;

        // 5. Create filter and configuration on that display, then capture
        let (width, height, rgba) = capture_with_screenshot_manager(display_id, show_cursor)?;

        // 6. Compute logical geometry
        let (logical_x, logical_y, logical_width, logical_height) =
            get_logical_geometry(display_id, width, height)?;

        Ok(CapturedDisplay {
            rgba,
            width,
            height,
            logical_x,
            logical_y,
            logical_width,
            logical_height,
            display_id,
        })
    }

    fn check_macos_version() -> Result<(), MacosOneShotError> {
        // SCScreenshotManager requires macOS 14.0+
        // Use @available check via objc runtime
        // For now, use NSProcessInfo operatingSystemVersion
        unsafe {
            // SAFETY: NSProcessInfo is a standard Foundation class; processInfo returns
            // a valid shared instance. operatingSystemVersion returns a struct by value.
            let process_info: *mut objc2::runtime::AnyObject =
                objc2::msg_send![objc2::class!(NSProcessInfo), processInfo];
            if process_info.is_null() {
                return Err(MacosOneShotError::Capture(
                    "Failed to get NSProcessInfo".to_string(),
                ));
            }
            let version: objc2_foundation::NSOperatingSystemVersion =
                objc2::msg_send![process_info, operatingSystemVersion];
            let major = version.majorVersion;
            if major < 14 {
                return Err(MacosOneShotError::Unsupported(format!(
                    "macOS 14.0 or newer required for SCScreenshotManager, got {major}"
                )));
            }
        }
        Ok(())
    }

    fn check_permission() -> Result<(), MacosOneShotError> {
        use objc2_core_graphics::{CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess};

        let has_access = CGPreflightScreenCaptureAccess();
        if !has_access {
            let requested = CGRequestScreenCaptureAccess();
            if !requested {
                return Err(MacosOneShotError::PermissionDenied(
                    "Screen Recording permission denied. Please grant access in System Settings > Privacy & Security > Screen Recording, then restart the application.".to_string()
                ));
            }
        }
        Ok(())
    }

    fn get_cursor_location() -> Result<(f64, f64), MacosOneShotError> {
        use objc2_core_graphics::CGEvent;

        let event = CGEvent::new(None)
            .ok_or_else(|| MacosOneShotError::Capture("Failed to create CGEvent".to_string()))?;
        let point = CGEvent::location(Some(&event));
        Ok((point.x, point.y))
    }

    /// Resolve the `CGDirectDisplayID` of the display under the pointer.
    ///
    /// `SCShareableContent` is fetched through an asynchronous completion
    /// handler that ScreenCaptureKit invokes on an internal queue. The handler
    /// extracts only plain `Copy` data (display frames and ids) and sends the
    /// chosen id back through a channel, so no Objective-C object crosses the
    /// thread boundary. The pointer falls back to the primary display only when
    /// it is not over any display.
    fn find_display_under_cursor(cursor_x: f64, cursor_y: f64) -> Result<u32, MacosOneShotError> {
        use block2::RcBlock;
        use objc2_foundation::NSError;
        use objc2_screen_capture_kit::SCShareableContent;

        let (tx, rx) = std::sync::mpsc::channel::<Result<u32, MacosOneShotError>>();

        let handler = RcBlock::new(
            move |content: *mut SCShareableContent, error: *mut NSError| {
                let result = (|| {
                    if !error.is_null() {
                        // SAFETY: a non-null NSError pointer from the callback is a
                        // valid, retained object for the duration of this call.
                        let msg = unsafe { (*error).localizedDescription() }.to_string();
                        return Err(MacosOneShotError::Capture(format!(
                            "SCShareableContent error: {msg}"
                        )));
                    }
                    // SAFETY: when error is null, content is a valid SCShareableContent
                    // for the duration of the callback.
                    let content = unsafe { content.as_ref() }.ok_or_else(|| {
                        MacosOneShotError::Capture(
                            "SCShareableContent returned neither content nor error".to_string(),
                        )
                    })?;
                    // SAFETY: displays() returns a retained array of valid SCDisplay.
                    let displays = unsafe { content.displays() };
                    let count = displays.count();
                    if count == 0 {
                        return Err(MacosOneShotError::Capture(
                            "no displays available for capture".to_string(),
                        ));
                    }
                    let mut frames = Vec::with_capacity(count);
                    for i in 0..count {
                        let display = displays.objectAtIndex(i);
                        // SAFETY: frame()/displayID() read scalar properties of a
                        // valid SCDisplay; CGRect is returned by value.
                        let frame = unsafe { display.frame() };
                        let id = unsafe { display.displayID() };
                        frames.push(DisplayFrame {
                            x: frame.origin.x,
                            y: frame.origin.y,
                            width: frame.size.width,
                            height: frame.size.height,
                            display_id: id,
                        });
                    }
                    Ok(display_id_containing_point(cursor_x, cursor_y, &frames)
                        .unwrap_or(frames[0].display_id))
                })();
                let _ = tx.send(result);
            },
        );

        // SAFETY: dispatched as a class method with a block whose signature
        // matches the declared completion handler. The call is asynchronous; the
        // bounded channel wait below turns it into a synchronous result.
        unsafe {
            SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
                false, true, &handler,
            );
        }

        resolve_callback_with_timeout(&rx, CALLBACK_TIMEOUT)
    }

    /// Capture the given display through `SCScreenshotManager` and return its
    /// physical dimensions and tightly packed RGBA pixels.
    ///
    /// Shareable content is fetched again (a cheap call) so the live `SCDisplay`
    /// stays on the callback thread; an `SCContentFilter` and configuration are
    /// built there and the capture is started. The screenshot completion handler
    /// extracts the `CGImage` bytes into an owned `Vec<u8>` before sending them
    /// back, so no Core Graphics or ScreenCaptureKit object crosses the channel.
    fn capture_with_screenshot_manager(
        display_id: u32,
        show_cursor: bool,
    ) -> Result<(u32, u32, Vec<u8>), MacosOneShotError> {
        use block2::RcBlock;
        use objc2::rc::Retained;
        use objc2::AllocAnyThread;
        use objc2_core_graphics::{
            CGDataProvider, CGDisplayCopyDisplayMode, CGDisplayMode, CGImage,
        };
        use objc2_foundation::{NSArray, NSError};
        use objc2_screen_capture_kit::{
            SCContentFilter, SCScreenshotManager, SCShareableContent, SCStreamConfiguration,
            SCWindow,
        };

        // Request the display's native (backing) pixel resolution so the capture
        // is not downscaled to point size on Retina displays.
        let mode = CGDisplayCopyDisplayMode(display_id).ok_or_else(|| {
            MacosOneShotError::Capture(format!(
                "failed to get current display mode for display {display_id}"
            ))
        })?;
        let px_width = CGDisplayMode::pixel_width(Some(&mode));
        let px_height = CGDisplayMode::pixel_height(Some(&mode));
        // Enforce the shared ceiling before allocating any capture buffers.
        checked_dimensions(px_width as u32, px_height as u32)?;

        let (tx, rx) = std::sync::mpsc::channel::<Result<(u32, u32, Vec<u8>), MacosOneShotError>>();

        let content_handler = RcBlock::new(
            move |content: *mut SCShareableContent, error: *mut NSError| {
                let tx = tx.clone();
                let prepared = (|| -> Result<
                    (Retained<SCContentFilter>, Retained<SCStreamConfiguration>),
                    MacosOneShotError,
                > {
                    if !error.is_null() {
                        // SAFETY: non-null NSError is valid during the callback.
                        let msg = unsafe { (*error).localizedDescription() }.to_string();
                        return Err(MacosOneShotError::Capture(format!(
                            "SCShareableContent error: {msg}"
                        )));
                    }
                    // SAFETY: content is valid when error is null.
                    let content = unsafe { content.as_ref() }.ok_or_else(|| {
                        MacosOneShotError::Capture(
                            "SCShareableContent returned neither content nor error".to_string(),
                        )
                    })?;
                    // SAFETY: displays() returns a retained array of valid SCDisplay.
                    let displays = unsafe { content.displays() };
                    let count = displays.count();
                    let mut chosen = None;
                    for i in 0..count {
                        let display = displays.objectAtIndex(i);
                        // SAFETY: displayID() reads a scalar property.
                        if unsafe { display.displayID() } == display_id {
                            chosen = Some(display);
                            break;
                        }
                    }
                    let display = chosen.ok_or_else(|| {
                        MacosOneShotError::Capture(format!(
                            "display {display_id} disappeared before capture"
                        ))
                    })?;

                    let excluded = NSArray::<SCWindow>::new();
                    // SAFETY: initWithDisplay:excludingWindows: takes a freshly
                    // allocated SCContentFilter, a valid SCDisplay, and a valid
                    // (empty) NSArray; it returns an owned, initialized filter.
                    let filter = unsafe {
                        SCContentFilter::initWithDisplay_excludingWindows(
                            SCContentFilter::alloc(),
                            &display,
                            &excluded,
                        )
                    };
                    // SAFETY: new() returns an owned configuration; the setters
                    // assign scalar properties on it.
                    let config = unsafe {
                        let config = SCStreamConfiguration::new();
                        config.setWidth(px_width);
                        config.setHeight(px_height);
                        config.setShowsCursor(show_cursor);
                        // kCVPixelFormatType_32BGRA ('BGRA'); matches the BGRA →
                        // RGBA row conversion applied to the returned CGImage.
                        config.setPixelFormat(0x42475241);
                        config
                    };
                    Ok((filter, config))
                })();

                let (filter, config) = match prepared {
                    Ok(pair) => pair,
                    Err(err) => {
                        let _ = tx.send(Err(err));
                        return;
                    }
                };

                let tx_image = tx.clone();
                let image_handler =
                    RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
                        let result = (|| {
                            if !error.is_null() {
                                // SAFETY: non-null NSError is valid during the callback.
                                let msg = unsafe { (*error).localizedDescription() }.to_string();
                                return Err(MacosOneShotError::Capture(format!(
                                    "SCScreenshotManager error: {msg}"
                                )));
                            }
                            // SAFETY: image is valid when error is null.
                            let image = unsafe { image.as_ref() }.ok_or_else(|| {
                                MacosOneShotError::Capture(
                                    "SCScreenshotManager returned neither image nor error"
                                        .to_string(),
                                )
                            })?;
                            let width = CGImage::width(Some(image)) as u32;
                            let height = CGImage::height(Some(image)) as u32;
                            let bytes_per_row = CGImage::bytes_per_row(Some(image));
                            checked_dimensions(width, height)?;
                            let provider =
                                CGImage::data_provider(Some(image)).ok_or_else(|| {
                                    MacosOneShotError::Capture(
                                        "captured CGImage has no data provider".to_string(),
                                    )
                                })?;
                            let data = CGDataProvider::data(Some(&provider)).ok_or_else(|| {
                                MacosOneShotError::Capture(
                                    "failed to copy captured CGImage pixel data".to_string(),
                                )
                            })?;
                            let len = data.length() as usize;
                            let ptr = data.byte_ptr();
                            if ptr.is_null() || len == 0 {
                                return Err(MacosOneShotError::Capture(
                                    "captured CGImage pixel data is empty".to_string(),
                                ));
                            }
                            // SAFETY: ptr/len describe the bytes of the CFData we own
                            // for the lifetime of `data`; the slice is fully consumed
                            // by bgra_padded_to_rgba before `data` is dropped.
                            let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
                            let rgba = bgra_padded_to_rgba(bytes, width, height, bytes_per_row)?;
                            Ok((width, height, rgba))
                        })();
                        let _ = tx_image.send(result);
                    });

                // SAFETY: class method dispatched with a valid filter, config, and a
                // block matching the declared completion-handler signature. The
                // handler is escaping; ScreenCaptureKit copies it, so dropping the
                // local RcBlock after this call is correct.
                unsafe {
                    SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
                        &filter,
                        &config,
                        Some(&image_handler),
                    );
                }
            },
        );

        // SAFETY: dispatched as a class method with a correctly-typed block.
        unsafe {
            SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
                false, true, &content_handler,
            );
        }

        resolve_callback_with_timeout(&rx, CALLBACK_TIMEOUT)
    }

    fn get_logical_geometry(
        display_id: u32,
        _physical_width: u32,
        _physical_height: u32,
    ) -> Result<(i32, i32, u32, u32), MacosOneShotError> {
        use objc2_core_graphics::CGDisplayBounds;

        let bounds = CGDisplayBounds(display_id);
        Ok((
            bounds.origin.x as i32,
            bounds.origin.y as i32,
            bounds.size.width as u32,
            bounds.size.height as u32,
        ))
    }
}

// ── Non-macOS stub ──

#[cfg(not(target_os = "macos"))]
/// Capture the display under the current cursor position.
///
/// On non-macOS platforms, this always returns `Unsupported`.
pub fn capture_display_under_cursor(
    _show_cursor: bool,
) -> Result<CapturedDisplay, MacosOneShotError> {
    Err(MacosOneShotError::Unsupported(
        "macOS one-shot capture requires macOS 14.0 or newer".to_string(),
    ))
}

#[cfg(not(target_os = "macos"))]
/// Stub for non-macOS platforms.
pub fn display_id_under_cursor() -> Result<u32, MacosOneShotError> {
    Err(MacosOneShotError::Unsupported(
        "macOS one-shot capture requires macOS 14.0 or newer".to_string(),
    ))
}

#[cfg(not(target_os = "macos"))]
/// Stub for non-macOS platforms.
pub fn display_logical_bounds(_display_id: u32) -> Result<(i32, i32, u32, u32), MacosOneShotError> {
    Err(MacosOneShotError::Unsupported(
        "macOS one-shot capture requires macOS 14.0 or newer".to_string(),
    ))
}

#[cfg(target_os = "macos")]
/// Capture the display under the current cursor position using SCScreenshotManager.
pub fn capture_display_under_cursor(
    show_cursor: bool,
) -> Result<CapturedDisplay, MacosOneShotError> {
    macos_impl::capture_display_under_cursor(show_cursor)
}

#[cfg(target_os = "macos")]
/// Return the CGDirectDisplayID of the display under the current cursor
/// position, without performing a full screenshot capture.
pub fn display_id_under_cursor() -> Result<u32, MacosOneShotError> {
    macos_impl::display_id_under_cursor()
}

#[cfg(target_os = "macos")]
/// Logical bounds `(x, y, width, height)` of a display in Core Graphics
/// global coordinates (points, top-left origin).
pub fn display_logical_bounds(display_id: u32) -> Result<(i32, i32, u32, u32), MacosOneShotError> {
    macos_impl::display_logical_bounds(display_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Callback resolution tests ──

    #[test]
    fn resolve_callback_success() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Ok::<_, MacosOneShotError>(42i32)).unwrap();
        let result = resolve_callback_with_timeout(&rx, CALLBACK_TIMEOUT);
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn resolve_callback_error() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Err::<i32, _>(MacosOneShotError::Capture(
            "test error".to_string(),
        )))
        .unwrap();
        let result = resolve_callback_with_timeout(&rx, CALLBACK_TIMEOUT);
        match result {
            Err(MacosOneShotError::Capture(msg)) => {
                assert!(msg.contains("test error"), "msg: {msg}");
            }
            other => panic!("expected Capture error, got {other:?}"),
        }
    }

    #[test]
    fn resolve_callback_timeout() {
        let (_tx, rx) = std::sync::mpsc::channel::<Result<i32, MacosOneShotError>>();
        // Drop sender so no value is ever sent
        let result: Result<i32, MacosOneShotError> =
            resolve_callback_with_timeout(&rx, std::time::Duration::from_millis(10));
        match result {
            Err(MacosOneShotError::Timeout(msg)) => {
                assert!(msg.contains("timed out"), "msg: {msg}");
            }
            other => panic!("expected Timeout error, got {other:?}"),
        }
    }

    #[test]
    fn resolve_callback_disconnected() {
        let (tx, rx) = std::sync::mpsc::channel::<Result<i32, MacosOneShotError>>();
        drop(tx); // Disconnect
        let result: Result<i32, MacosOneShotError> =
            resolve_callback_with_timeout(&rx, CALLBACK_TIMEOUT);
        match result {
            Err(MacosOneShotError::Capture(msg)) => {
                assert!(msg.contains("disconnected"), "msg: {msg}");
            }
            other => panic!("expected Capture error, got {other:?}"),
        }
    }

    // ── Dimension validation tests ──

    #[test]
    fn checked_dimensions_valid() {
        let result = checked_dimensions(1920, 1080);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1920 * 1080);
    }

    #[test]
    fn checked_dimensions_zero_width() {
        let result = checked_dimensions(0, 1080);
        match result {
            Err(MacosOneShotError::Capture(msg)) => {
                assert!(msg.contains("zero display dimensions"), "msg: {msg}");
            }
            other => panic!("expected Capture error, got {other:?}"),
        }
    }

    #[test]
    fn checked_dimensions_zero_height() {
        let result = checked_dimensions(1920, 0);
        match result {
            Err(MacosOneShotError::Capture(msg)) => {
                assert!(msg.contains("zero display dimensions"), "msg: {msg}");
            }
            other => panic!("expected Capture error, got {other:?}"),
        }
    }

    #[test]
    fn checked_dimensions_oversized() {
        // 6325 * 6325 = 40,005,625 > 40,000,000
        let result = checked_dimensions(6325, 6325);
        match result {
            Err(MacosOneShotError::Capture(msg)) => {
                assert!(msg.contains("too large"), "msg: {msg}");
                assert!(msg.contains("40000000"), "msg: {msg}");
            }
            other => panic!("expected Capture error, got {other:?}"),
        }
    }

    #[test]
    fn checked_dimensions_exact_boundary() {
        // 5000 * 8000 = 40,000,000 exactly at the limit
        let result = checked_dimensions(5000, 8000);
        assert!(result.is_ok());
    }

    #[test]
    fn checked_dimensions_overflow() {
        let result = checked_dimensions(u32::MAX, u32::MAX);
        match result {
            Err(MacosOneShotError::Capture(msg)) => {
                assert!(
                    msg.contains("too large") || msg.contains("overflow"),
                    "msg: {msg}"
                );
            }
            other => panic!("expected Capture error, got {other:?}"),
        }
    }

    // ── BGRA padded row conversion tests ──

    #[test]
    fn bgra_padded_to_rgba_tightly_packed() {
        // 1x1, no padding
        let bgra = vec![10, 20, 30, 255];
        let rgba = bgra_padded_to_rgba(&bgra, 1, 1, 4).unwrap();
        assert_eq!(rgba, vec![30, 20, 10, 255]);
    }

    #[test]
    fn bgra_padded_to_rgba_with_row_padding() {
        // 2x2, bytes_per_row=12 (8 real + 4 padding)
        let bgra = vec![
            10, 20, 30, 255, 1, 2, 3, 4, 0, 0, 0, 0, // row 0
            50, 60, 70, 255, 5, 6, 7, 8, 0, 0, 0, 0, // row 1
        ];
        let rgba = bgra_padded_to_rgba(&bgra, 2, 2, 12).unwrap();
        assert_eq!(rgba.len(), 16);
        // Pixel (0,0): BGRA(10,20,30,255) -> RGBA(30,20,10,255)
        assert_eq!(&rgba[0..4], &[30, 20, 10, 255]);
        // Pixel (1,0): BGRA(1,2,3,4) -> RGBA(3,2,1,4)
        assert_eq!(&rgba[4..8], &[3, 2, 1, 4]);
        // Pixel (0,1): BGRA(50,60,70,255) -> RGBA(70,60,50,255)
        assert_eq!(&rgba[8..12], &[70, 60, 50, 255]);
        // Pixel (1,1): BGRA(5,6,7,8) -> RGBA(7,6,5,8)
        assert_eq!(&rgba[12..16], &[7, 6, 5, 8]);
    }

    #[test]
    fn bgra_padded_to_rgba_rejects_bytes_per_row_too_small() {
        let bgra = vec![0; 8];
        let result = bgra_padded_to_rgba(&bgra, 2, 1, 6); // Need at least 8
        match result {
            Err(MacosOneShotError::Capture(msg)) => {
                assert!(msg.contains("less than minimum"), "msg: {msg}");
            }
            other => panic!("expected Capture error, got {other:?}"),
        }
    }

    #[test]
    fn bgra_padded_to_rgba_rejects_buffer_too_short() {
        let bgra = vec![0; 10]; // Too short for 2x2 with bytes_per_row=8 (need 16)
        let result = bgra_padded_to_rgba(&bgra, 2, 2, 8);
        match result {
            Err(MacosOneShotError::Capture(msg)) => {
                assert!(msg.contains("too short"), "msg: {msg}");
            }
            other => panic!("expected Capture error, got {other:?}"),
        }
    }

    // ── Callback that returns neither image nor error ──

    #[test]
    fn resolve_callback_neither_image_nor_error_timeout() {
        // Simulate a callback that never sends anything (hangs)
        let (_tx, rx) = std::sync::mpsc::channel::<Result<i32, MacosOneShotError>>();
        let result: Result<i32, MacosOneShotError> =
            resolve_callback_with_timeout(&rx, std::time::Duration::from_millis(10));
        assert!(matches!(result, Err(MacosOneShotError::Timeout(_))));
    }

    // ── Cursor → display selection tests ──

    #[test]
    fn point_inside_single_display_selects_it() {
        let displays = [DisplayFrame {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
            display_id: 7,
        }];
        assert_eq!(
            display_id_containing_point(100.0, 200.0, &displays),
            Some(7)
        );
    }

    #[test]
    fn point_in_negative_origin_secondary_display_selects_it() {
        // A secondary display placed to the left of / above the primary has a
        // negative origin. The cursor over it must resolve to that display.
        let displays = [
            DisplayFrame {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
                display_id: 1,
            },
            DisplayFrame {
                x: -2560.0,
                y: -300.0,
                width: 2560.0,
                height: 1440.0,
                display_id: 2,
            },
        ];
        assert_eq!(
            display_id_containing_point(-1000.0, -100.0, &displays),
            Some(2)
        );
    }

    #[test]
    fn point_outside_all_displays_returns_none() {
        let displays = [DisplayFrame {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
            display_id: 3,
        }];
        assert_eq!(display_id_containing_point(5000.0, 5000.0, &displays), None);
    }

    #[test]
    fn point_on_right_or_bottom_edge_is_excluded() {
        // Half-open rect: [x, x+w) × [y, y+h). The far edges belong to the
        // neighbouring display, never this one.
        let displays = [DisplayFrame {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
            display_id: 4,
        }];
        assert_eq!(display_id_containing_point(800.0, 300.0, &displays), None);
        assert_eq!(display_id_containing_point(400.0, 600.0, &displays), None);
        assert_eq!(display_id_containing_point(0.0, 0.0, &displays), Some(4));
    }

    // ── Non-macOS stub test ──

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_macos_stub_returns_unsupported() {
        let result = capture_display_under_cursor(false);
        match result {
            Err(MacosOneShotError::Unsupported(msg)) => {
                assert!(msg.contains("macOS 14.0"), "msg: {msg}");
            }
            other => panic!("expected Unsupported error, got {other:?}"),
        }
    }

    // ── Live capture smoke check ──
    //
    // Ignored by default: requires Screen Recording (TCC) permission and a live
    // display, so it cannot run in CI. Run manually with:
    //   cargo test -p rollshot-macos-oneshot live_capture -- --ignored --nocapture
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires Screen Recording permission and a live display"]
    fn live_capture_under_cursor_smoke() {
        let capture = capture_display_under_cursor(false).expect("capture should succeed");
        assert!(
            capture.width > 0 && capture.height > 0,
            "physical dimensions must be non-zero: {}x{}",
            capture.width,
            capture.height
        );
        assert_eq!(
            capture.rgba.len(),
            capture.width as usize * capture.height as usize * 4,
            "RGBA buffer must match physical dimensions",
        );
        assert!(
            capture.rgba.iter().any(|&b| b != 0),
            "a real screen capture must not be entirely zero",
        );
        eprintln!(
            "captured {}x{} px (logical {}x{} at {},{}) display {}",
            capture.width,
            capture.height,
            capture.logical_width,
            capture.logical_height,
            capture.logical_x,
            capture.logical_y,
            capture.display_id,
        );
    }
}
