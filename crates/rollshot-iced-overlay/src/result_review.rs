use iced::widget::scrollable;
use iced::{Element, Length, Size};

use crate::app::OverlayMessage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewScrollAxis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone)]
pub struct ReviewLayout {
    pub axis: ReviewScrollAxis,
    pub rendered: Size,
}

pub fn review_layout(result_size: Size, viewport: Size) -> ReviewLayout {
    let result_w = result_size.width;
    let result_h = result_size.height;

    if result_w <= 0.0 || result_h <= 0.0 || viewport.width <= 0.0 || viewport.height <= 0.0 {
        return ReviewLayout {
            axis: ReviewScrollAxis::Vertical,
            rendered: Size::new(1.0, 1.0),
        };
    }

    let aspect = result_w / result_h;
    let is_long_vertical = result_h > result_w * 2.0;
    let is_long_horizontal = result_w > result_h * 2.0;

    if is_long_vertical {
        let rendered_w = viewport.width;
        let rendered_h = rendered_w / aspect;
        ReviewLayout {
            axis: ReviewScrollAxis::Vertical,
            rendered: Size::new(rendered_w, rendered_h),
        }
    } else if is_long_horizontal {
        let rendered_h = viewport.height;
        let rendered_w = rendered_h * aspect;
        ReviewLayout {
            axis: ReviewScrollAxis::Horizontal,
            rendered: Size::new(rendered_w, rendered_h),
        }
    } else {
        let scale_w = viewport.width / result_w;
        let scale_h = viewport.height / result_h;
        let scale = scale_w.min(scale_h);
        let rendered_w = result_w * scale;
        let rendered_h = result_h * scale;
        ReviewLayout {
            axis: ReviewScrollAxis::Vertical,
            rendered: Size::new(rendered_w, rendered_h),
        }
    }
}

pub fn build_result_handle(img: &image::RgbaImage) -> iced::widget::image::Handle {
    iced::widget::image::Handle::from_rgba(img.width(), img.height(), img.clone().into_raw())
}

pub fn view_result_review<'a>(
    handle: &'a iced::widget::image::Handle,
    layout: &ReviewLayout,
) -> Element<'a, OverlayMessage> {
    let img = iced::widget::image(handle.clone())
        .width(Length::Fixed(layout.rendered.width))
        .height(Length::Fixed(layout.rendered.height));

    match layout.axis {
        ReviewScrollAxis::Vertical => scrollable(img)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        ReviewScrollAxis::Horizontal => scrollable(
            iced::widget::row![img]
                .width(Length::Shrink)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new(),
        ))
        .into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_result_fits_width_and_scrolls_vertically() {
        let layout = review_layout(Size::new(800.0, 3000.0), Size::new(600.0, 500.0));
        assert_eq!(layout.axis, ReviewScrollAxis::Vertical);
        assert_eq!(layout.rendered.width, 600.0);
        assert!(layout.rendered.height > 500.0);
    }

    #[test]
    fn horizontal_result_fits_height_and_scrolls_horizontally() {
        let layout = review_layout(Size::new(3000.0, 800.0), Size::new(600.0, 500.0));
        assert_eq!(layout.axis, ReviewScrollAxis::Horizontal);
        assert_eq!(layout.rendered.height, 500.0);
        assert!(layout.rendered.width > 600.0);
    }

    #[test]
    fn normal_image_aspect_fits_in_viewport() {
        let layout = review_layout(Size::new(800.0, 600.0), Size::new(600.0, 500.0));
        assert!(layout.rendered.width <= 600.0);
        assert!(layout.rendered.height <= 500.0);
    }

    #[test]
    fn zero_size_returns_minimum() {
        let layout = review_layout(Size::new(0.0, 0.0), Size::new(600.0, 500.0));
        assert_eq!(layout.rendered.width, 1.0);
        assert_eq!(layout.rendered.height, 1.0);
    }
}
