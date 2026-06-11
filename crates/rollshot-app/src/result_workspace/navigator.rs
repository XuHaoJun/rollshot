//! Navigator drawer (spec §8.2): semantic top-to-bottom annotation list with
//! jump-to-annotation. Ordering comes from the document crate; this module
//! owns only the view and the viewport jump math.

use iced::widget::{button, column, container, scrollable, text};
use iced::{Alignment, Element, Length, Size, Vector};
use rollshot_image_document::ImagePoint;

use super::update::Message;
use super::viewport::{clamp_scroll, ViewportGeometry};
use super::ResultWorkspace;

pub(crate) const NAVIGATOR_WIDTH: f32 = 220.0;

/// Absolute scroll offset that centers `target` (image coords) in the
/// viewport, clamped to the scrollable range.
pub(crate) fn jump_offset(
    target: ImagePoint,
    geometry: &ViewportGeometry,
    viewport: Size,
) -> scrollable::AbsoluteOffset {
    let content_x = geometry.image_origin.x + target.x * geometry.scale;
    let content_y = geometry.image_origin.y + target.y * geometry.scale;
    let clamped = clamp_scroll(
        Vector::new(
            content_x - viewport.width / 2.0,
            content_y - viewport.height / 2.0,
        ),
        geometry.max_scroll,
    );
    scrollable::AbsoluteOffset {
        x: clamped.x,
        y: clamped.y,
    }
}

pub(crate) fn navigator_panel(state: &ResultWorkspace) -> Element<'_, Message> {
    let items = &state.editor.navigator_items;
    let mut list = column![].spacing(2);
    if items.is_empty() {
        list = list.push(text("No annotations yet").size(13));
    }
    for item in items {
        let selected = state.editor.selection == Some(item.id);
        let row_btn = button(text(item.label.clone()).size(13))
            .width(Length::Fill)
            .style(if selected {
                button::primary
            } else {
                button::text
            })
            .on_press(Message::NavigatorJump(item.id));
        list = list.push(row_btn);
    }
    container(scrollable(list).height(Length::Fill))
        .width(Length::Fixed(NAVIGATOR_WIDTH))
        .padding(6)
        .align_x(Alignment::Start)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result_workspace::viewport::{geometry_for, ZoomMode};

    #[test]
    fn jump_centers_the_target_and_clamps_to_scroll_range() {
        let geometry = geometry_for(
            ZoomMode::ActualSize,
            Size::new(1000.0, 5000.0),
            Size::new(500.0, 400.0),
        );
        let offset = jump_offset(
            ImagePoint::new(500.0, 2000.0),
            &geometry,
            Size::new(500.0, 400.0),
        );
        assert_eq!(Vector::new(offset.x, offset.y), Vector::new(250.0, 1800.0));
        let top = jump_offset(
            ImagePoint::new(10.0, 10.0),
            &geometry,
            Size::new(500.0, 400.0),
        );
        assert_eq!(Vector::new(top.x, top.y), Vector::new(0.0, 0.0));
    }
}
