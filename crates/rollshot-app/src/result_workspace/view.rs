use super::viewport::{geometry_for, ZoomDirection, ZoomMode};
use iced::widget::{
    button, column, container, image as image_widget, mouse_area, row, scrollable, text, Space,
};
use iced::{mouse, Alignment, Element, Length, Size, Vector};

use super::{Message, ResultWorkspace};

const SCROLLBAR_WIDTH: f32 = 14.0;
const SCROLLBAR_SPACING: f32 = 2.0;

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub(crate) fn view(state: &ResultWorkspace) -> Element<'_, Message> {
    let original = state.original_size();

    let title = state
        .document
        .saved_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| super::document::UNSAVED_LABEL.to_string());

    let toolbar = row![
        button(text("Close")).on_press(Message::RequestClose),
        text(title).width(Length::Fill),
        button(text("Copy")).on_press(Message::Copy),
        button(text("Save As")).on_press(Message::SaveAs),
        reveal_button(state),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let message_area = message_row(state);

    let canvas = canvas_view(state, original);

    let status = status_bar(state, original);

    let layout = column![toolbar, message_area, canvas, status]
        .spacing(8)
        .padding(8);

    if state.confirming_discard {
        discard_modal(layout)
    } else {
        layout.into()
    }
}

fn reveal_button(state: &ResultWorkspace) -> Element<'_, Message> {
    let btn = button(text("Reveal"));
    if state.can_reveal() {
        btn.on_press(Message::Reveal).into()
    } else {
        btn.into()
    }
}

fn message_row(state: &ResultWorkspace) -> Element<'_, Message> {
    match &state.message {
        Some(msg) => {
            let mut content = row![text(msg.text().to_owned()).width(Length::Fill)]
                .spacing(8)
                .align_y(Alignment::Center);
            if msg.is_error() {
                content = content.push(button(text("Dismiss")).on_press(Message::DismissMessage));
            }
            container(content).width(Length::Fill).padding(4).into()
        }
        None => Space::new()
            .width(Length::Fill)
            .height(Length::Fixed(0.0))
            .into(),
    }
}

fn canvas_view<'a>(state: &'a ResultWorkspace, image_size: Size) -> Element<'a, Message> {
    let geometry = geometry_for(state.viewport.zoom, image_size, state.viewport_bounds);

    let img = image_widget(state.image_handle.clone())
        .width(Length::Fixed(geometry.rendered_size.width))
        .height(Length::Fixed(geometry.rendered_size.height));

    // Place the (possibly centered) image inside content sized to the geometry.
    let content = container(img)
        .width(Length::Fixed(geometry.content_size.width))
        .height(Length::Fixed(geometry.content_size.height))
        .padding(iced::Padding {
            left: geometry.image_origin.x,
            top: geometry.image_origin.y,
            right: 0.0,
            bottom: 0.0,
        });

    let vertical = if geometry.vertical_overflow {
        thick_scrollbar()
    } else {
        scrollable::Scrollbar::hidden()
    };
    let horizontal = if geometry.horizontal_overflow {
        thick_scrollbar()
    } else {
        scrollable::Scrollbar::hidden()
    };

    let scroller = scrollable(content)
        .direction(scrollable::Direction::Both {
            vertical,
            horizontal,
        })
        .id(state.scrollable_id.clone())
        .on_scroll(|viewport| {
            let off = viewport.absolute_offset();
            let bounds = viewport.bounds();
            Message::ViewportChanged {
                bounds: bounds.size(),
                offset: Vector::new(off.x, off.y),
            }
        })
        .width(Length::Fill)
        .height(Length::Fill);

    mouse_area(scroller)
        .on_move(Message::PointerMoved)
        .on_scroll(Message::WheelScrolled)
        .into()
}

fn thick_scrollbar() -> scrollable::Scrollbar {
    scrollable::Scrollbar::new()
        .width(SCROLLBAR_WIDTH)
        .scroller_width(SCROLLBAR_WIDTH)
        .spacing(SCROLLBAR_SPACING)
}

fn status_bar(state: &ResultWorkspace, image_size: Size) -> Element<'_, Message> {
    let dims = format!("{} × {}", image_size.width as u32, image_size.height as u32);
    let zoom_label = zoom_label(state);

    row![
        text(dims),
        text(zoom_label).width(Length::Fill),
        button(text("Fit Width")).on_press(Message::SetZoom(ZoomMode::FitWidth)),
        button(text("Fit Window")).on_press(Message::SetZoom(ZoomMode::FitWindow)),
        button(text("Fit Height")).on_press(Message::SetZoom(ZoomMode::FitHeight)),
        button(text("100%")).on_press(Message::SetZoom(ZoomMode::ActualSize)),
        button(text("-")).on_press(Message::ZoomStep(ZoomDirection::Out)),
        button(text("+")).on_press(Message::ZoomStep(ZoomDirection::In)),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn zoom_label(state: &ResultWorkspace) -> String {
    match state.viewport.zoom {
        ZoomMode::FitWidth => "Fit Width".to_string(),
        ZoomMode::FitWindow => "Fit Window".to_string(),
        ZoomMode::FitHeight => "Fit Height".to_string(),
        ZoomMode::ActualSize => "100%".to_string(),
        ZoomMode::Custom(p) => format!("{p}%"),
    }
}

fn discard_modal(base: iced::widget::Column<'_, Message>) -> Element<'_, Message> {
    let dialog = container(
        column![
            text(super::document::DISCARD_PROMPT),
            row![
                button(text("Keep")).on_press(Message::KeepUnsaved),
                button(text("Discard")).on_press(Message::ConfirmDiscard),
            ]
            .spacing(8),
        ]
        .spacing(12)
        .align_x(Alignment::Center),
    )
    .padding(20);

    // Full-window scrim that actually blocks the base layer. iced's `stack`
    // only levitates the cursor (suppressing input to lower layers) where the
    // top layer reports a non-`None` `mouse_interaction`. A bare centered
    // container leaves the surrounding area at `Interaction::None`, so toolbar
    // buttons behind it stay clickable. Setting `mouse_area.interaction(Idle)`
    // makes the scrim report a non-`None` interaction over the whole window,
    // and `on_press` captures clicks outside the dialog (mapped to a no-op so
    // an accidental outside-click neither dismisses nor discards).
    let scrim = mouse_area(
        container(dialog)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .interaction(mouse::Interaction::Idle)
    .on_press(Message::ModalScrimPressed);

    iced::widget::stack![base, scrim].into()
}
