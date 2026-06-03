use iced::widget::image;
use iced::{Point, Rectangle, Size};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreviewConstraints {
    pub(crate) fixed_width: u32,
    pub(crate) max_height: u32,
}

#[derive(Default)]
pub(crate) struct OverlayState {
    pub(crate) drag_start: Option<Point>,
    pub(crate) crop: Option<Rectangle>,
    pub(crate) crop_confirmed: bool,
    pub(crate) preview: Option<image::Handle>,
    pub(crate) window_size: Option<Size>,
    pub(crate) capture_miss_warn: bool,
    pub(crate) capture_miss_message_expires_at: Option<std::time::Instant>,
}
