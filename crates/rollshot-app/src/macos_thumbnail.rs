//! Floating post-capture thumbnail state, timer, and interaction helpers.
//!
//! Portable/macOS split (registered on EVERY target so the timer + interaction
//! logic is unit-tested on Linux):
//!
//! - PORTABLE (compiled + tested on all targets): [`ThumbnailTimer`],
//!   [`ThumbnailAction`], [`ThumbnailState`], [`release_action`], and the
//!   [`DRAG_THRESHOLD_POINTS`] constant. These hold no macOS-only types — the
//!   image handle / point types come from `iced`, which is a dependency on all
//!   targets. The Linux tests exercise exactly these pieces.
//! - macOS-ONLY ([`view`]): the iced rendering references the product
//!   `Message` enum, which only exists on macOS (its `macos_product` module is
//!   `#[cfg(target_os = "macos")]`). The view is therefore gated; everything the
//!   tests touch stays portable.
//!
//! The portable items have no non-test caller off macOS (their only consumer,
//! `macos_product`, is macOS-gated), so dead-code is allowed on non-macOS to
//! keep the Linux build/clippy clean while the unit tests still exercise them.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::sync::atomic::AtomicU8;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::image::Handle as ImageHandle;
use iced::Point;
use std::path::PathBuf;

/// Pointer travel (in logical points) past which a press+release is treated as
/// a drag intent rather than a click.
pub const DRAG_THRESHOLD_POINTS: f32 = 4.0;

/// How long the floating thumbnail stays on screen before auto-dismissing,
/// counting only time while neither hovered nor dragging.
pub const THUMBNAIL_TIMEOUT: Duration = Duration::from_secs(8);

/// The interaction outcome of releasing a press on the thumbnail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailAction {
    /// Click within the drag threshold: open the saved Result Workspace.
    OpenWorkspace,
    /// Press dragged past the threshold (or a drag was already started): hand
    /// off to the native AppKit drag (Task 9). For now a placeholder no-op.
    StartNativeDrag,
    /// Keep the thumbnail open (e.g. hover begins, drag continues).
    KeepOpen,
    /// Dismiss the thumbnail.
    Close,
}

/// Counts the thumbnail's dismissal timeout, pausing while the user is hovering
/// or dragging. `tick` accumulates only the unpaused time since the last tick.
pub struct ThumbnailTimer {
    remaining: Duration,
    last_tick: Instant,
    hovering: bool,
    dragging: bool,
}

impl ThumbnailTimer {
    pub fn new(now: Instant, duration: Duration) -> Self {
        Self {
            remaining: duration,
            last_tick: now,
            hovering: false,
            dragging: false,
        }
    }

    fn paused(&self) -> bool {
        self.hovering || self.dragging
    }

    /// Fold the elapsed unpaused time since `last_tick` into `remaining`, then
    /// reset `last_tick` to `now`. Called before any pause-state change so paused
    /// time is never counted.
    fn accumulate(&mut self, now: Instant) {
        if !self.paused() {
            let elapsed = now.saturating_duration_since(self.last_tick);
            self.remaining = self.remaining.saturating_sub(elapsed);
        }
        self.last_tick = now;
    }

    pub fn set_hovering(&mut self, hovering: bool, now: Instant) {
        self.accumulate(now);
        self.hovering = hovering;
    }

    pub fn set_dragging(&mut self, dragging: bool, now: Instant) {
        self.accumulate(now);
        self.dragging = dragging;
    }

    /// Advance the timer to `now`. Returns `true` once the unpaused countdown
    /// has reached zero (expired).
    pub fn tick(&mut self, now: Instant) -> bool {
        self.accumulate(now);
        self.remaining.is_zero()
    }
}

/// Live state for the floating thumbnail window.
pub struct ThumbnailState {
    pub image_handle: ImageHandle,
    pub saved_path: PathBuf,
    pub timer: ThumbnailTimer,
    /// Pointer position where the current press began, if any.
    pub press_origin: Option<Point>,
    pub dragging: bool,
    /// Shared status flag the native AppKit drag (Task 9) will publish into.
    // TODO(task-9): read by the AppKit native file drag bridge
    #[allow(dead_code)]
    pub native_drag_status: Option<Arc<AtomicU8>>,
}

impl ThumbnailState {
    pub fn new(image_handle: ImageHandle, saved_path: PathBuf, now: Instant) -> Self {
        Self {
            image_handle,
            saved_path,
            timer: ThumbnailTimer::new(now, THUMBNAIL_TIMEOUT),
            press_origin: None,
            dragging: false,
            native_drag_status: None,
        }
    }
}

/// Classify a press→release gesture. A release within [`DRAG_THRESHOLD_POINTS`]
/// of the press origin (and where no drag was started) is a click that opens the
/// workspace; anything past the threshold (or a started drag) is a native drag.
pub fn release_action(start: Point, end: Point, drag_started: bool) -> ThumbnailAction {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let distance = (dx * dx + dy * dy).sqrt();
    if drag_started || distance >= DRAG_THRESHOLD_POINTS {
        ThumbnailAction::StartNativeDrag
    } else {
        ThumbnailAction::OpenWorkspace
    }
}

/// Render the compact thumbnail card. macOS-only: it references the product
/// `Message` type, which exists only on that target. The portable timer /
/// action logic above is what the cross-platform tests exercise.
#[cfg(target_os = "macos")]
pub fn view(state: &ThumbnailState) -> iced::Element<'_, crate::macos_product::Message> {
    use crate::macos_product::Message;
    use iced::widget::{column, container, image as image_widget, mouse_area, text};
    use iced::{Length, Padding};

    let preview = image_widget(state.image_handle.clone())
        .width(Length::Fill)
        .height(Length::Fixed(120.0));

    let card = column![
        preview,
        text("Saved").size(13),
        text("Drag or click").size(11),
    ]
    .spacing(4)
    .padding(Padding::from(8));

    mouse_area(container(card).width(Length::Fill).height(Length::Fill))
        .on_press(Message::ThumbnailPressed)
        .on_release(Message::ThumbnailReleased)
        .on_enter(Message::ThumbnailHoverChanged(true))
        .on_exit(Message::ThumbnailHoverChanged(false))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- PORTABLE timer tests (run on Linux) ---------------------------------

    #[test]
    fn thumbnail_expires_after_eight_unpaused_seconds() {
        let start = Instant::now();
        let mut timer = ThumbnailTimer::new(start, Duration::from_secs(8));
        assert!(!timer.tick(start + Duration::from_millis(7_999)));
        assert!(timer.tick(start + Duration::from_secs(8)));
    }

    #[test]
    fn hover_and_drag_pause_timeout() {
        let start = Instant::now();
        let mut timer = ThumbnailTimer::new(start, Duration::from_secs(8));
        timer.set_hovering(true, start + Duration::from_secs(4));
        assert!(!timer.tick(start + Duration::from_secs(20)));
        timer.set_hovering(false, start + Duration::from_secs(20));
        assert!(timer.tick(start + Duration::from_secs(24)));
    }

    #[test]
    fn hover_pause_then_resume_expires_after_remaining_unpaused_time() {
        let start = Instant::now();
        let mut timer = ThumbnailTimer::new(start, Duration::from_secs(8));
        // Spend 3s unpaused, then hover (pause) for a long while.
        assert!(!timer.tick(start + Duration::from_secs(3)));
        timer.set_hovering(true, start + Duration::from_secs(3));
        assert!(!timer.tick(start + Duration::from_secs(100)));
        // Resume: 5s remain. 4s more does not expire; 5s does.
        timer.set_hovering(false, start + Duration::from_secs(100));
        assert!(!timer.tick(start + Duration::from_secs(104)));
        assert!(timer.tick(start + Duration::from_secs(105)));
    }

    #[test]
    fn set_dragging_pauses_and_resuming_counts_again() {
        let start = Instant::now();
        let mut timer = ThumbnailTimer::new(start, Duration::from_secs(8));
        timer.set_dragging(true, start + Duration::from_secs(2));
        // While dragging, no time accrues no matter how long.
        assert!(!timer.tick(start + Duration::from_secs(50)));
        timer.set_dragging(false, start + Duration::from_secs(50));
        // 6s remained at the pause point; 6s of unpaused time expires it.
        assert!(!timer.tick(start + Duration::from_secs(55)));
        assert!(timer.tick(start + Duration::from_secs(56)));
    }

    // -- PORTABLE release_action tests ---------------------------------------

    #[test]
    fn release_within_threshold_opens_workspace() {
        let start = Point::new(10.0, 10.0);
        let end = Point::new(12.0, 11.0); // distance ~2.2 < 4.0
        assert_eq!(
            release_action(start, end, false),
            ThumbnailAction::OpenWorkspace
        );
    }

    #[test]
    fn release_beyond_threshold_starts_native_drag() {
        let start = Point::new(10.0, 10.0);
        let end = Point::new(20.0, 10.0); // distance 10 >= 4.0
        assert_eq!(
            release_action(start, end, false),
            ThumbnailAction::StartNativeDrag
        );
    }

    #[test]
    fn release_with_drag_already_started_is_native_drag() {
        let p = Point::new(10.0, 10.0);
        assert_eq!(release_action(p, p, true), ThumbnailAction::StartNativeDrag);
    }
}
