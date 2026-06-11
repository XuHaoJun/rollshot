use super::viewport::{geometry_for, ZoomDirection, ZoomMode};
use iced::widget::{
    button, column, container, image as image_widget, mouse_area, row, scrollable, text, text_editor,
    tooltip, Space,
};
use iced::{keyboard, mouse, Alignment, Element, Length, Size, Vector};

use super::canvas::Tool;
use super::{Message, ResultWorkspace};

const SCROLLBAR_WIDTH: f32 = 14.0;
const SCROLLBAR_SPACING: f32 = 2.0;

const ICON_SELECT: &str = "\u{2196}";
const ICON_NUMBER: &str = "\u{2460}";
const ICON_TEXT: &str = "T";
const ICON_REDACT: &str = "\u{2588}";
const ICON_UNDO: &str = "\u{21B6}";
const ICON_REDO: &str = "\u{21B7}";
const ICON_NAVIGATOR: &str = "\u{2261}";

fn shortcut_label(name: &str, key: &str) -> String {
    format!("{name} ({key})")
}

fn icon_button<'a>(
    glyph: &'a str,
    tip: String,
    message: Message,
    active: bool,
) -> Element<'a, Message> {
    let btn = button(text(glyph).size(16))
        .padding([4, 10])
        .on_press(message)
        .style(if active {
            button::primary
        } else {
            button::secondary
        });
    tooltip(btn, text(tip), tooltip::Position::Bottom).into()
}

fn tool_button<'a>(
    glyph: &'a str,
    name: &str,
    key: &str,
    tool: Tool,
    state: &ResultWorkspace,
) -> Element<'a, Message> {
    icon_button(
        glyph,
        shortcut_label(name, key),
        Message::SelectTool(tool),
        state.editor.tool == tool,
    )
}

fn toolbar(state: &ResultWorkspace) -> Element<'_, Message> {
    let undo_btn = button(text(ICON_UNDO).size(16))
        .padding([4, 10])
        .on_press_maybe(state.document.image.can_undo().then_some(Message::Undo));
    let redo_btn = button(text(ICON_REDO).size(16))
        .padding([4, 10])
        .on_press_maybe(state.document.image.can_redo().then_some(Message::Redo));

    row![
        button(text("Close")).on_press(Message::RequestClose),
        text(state.document.display_name()).width(Length::Fill),
        tool_button(ICON_SELECT, "Select", "V", Tool::Select, state),
        tool_button(ICON_NUMBER, "Number", "N", Tool::Number, state),
        tool_button(ICON_TEXT, "Text", "T", Tool::Text, state),
        tool_button(ICON_REDACT, "Redact", "R", Tool::Redact, state),
        tooltip(
            undo_btn,
            text(shortcut_label("Undo", "Ctrl+Z")),
            tooltip::Position::Bottom
        ),
        tooltip(
            redo_btn,
            text(shortcut_label("Redo", "Ctrl+Shift+Z")),
            tooltip::Position::Bottom
        ),
        icon_button(
            ICON_NAVIGATOR,
            "Navigator".to_string(),
            Message::ToggleNavigator,
            state.editor.navigator_open,
        ),
        button(text("Copy")).on_press(Message::Copy),
        button(text("\u{25BE}")).on_press(Message::ToggleCopyMenu),
        button(text("Save As")).on_press(Message::SaveAs),
        reveal_button(state),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub(crate) fn view(state: &ResultWorkspace) -> Element<'_, Message> {
    let original = state.original_size();

    let toolbar = toolbar(state);

    let message_area = message_row(state);

    let canvas_area = canvas_view(state, original);

    let status = status_bar(state, original);

    let workspace_row: Element<'_, Message> = if state.editor.navigator_open {
        row![canvas_area, super::navigator::navigator_panel(state)]
            .spacing(4)
            .into()
    } else {
        canvas_area
    };

    let layout = column![toolbar, message_area, workspace_row, status]
        .spacing(8)
        .padding(8);

    let layout: Element<'_, Message> = if let Some(prompt) = &state.pending_discard {
        discard_modal(layout.into(), prompt.text())
    } else {
        layout.into()
    };
    if state.editor.copy_menu_open {
        copy_menu(layout)
    } else {
        layout
    }
}

fn copy_menu(base: Element<'_, Message>) -> Element<'_, Message> {
    let menu = container(button(text("Copy Original")).on_press(Message::CopyOriginal)).padding(4);

    let positioned = container(menu)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::End)
        .padding(iced::Padding {
            top: 44.0,
            right: 180.0,
            ..Default::default()
        });

    let scrim = mouse_area(positioned)
        .interaction(mouse::Interaction::Idle)
        .on_press(Message::ToggleCopyMenu);

    iced::widget::stack![base, scrim].into()
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

    let overlay = iced::widget::canvas(super::canvas::AnnotationCanvas {
        document: &state.document.image,
        editor: &state.editor,
        scale: geometry.scale,
        visible: super::canvas::visible_image_rect(
            state.viewport.scroll_offset,
            state.viewport_bounds,
            geometry.scale,
            geometry.image_origin,
        ),
    })
    .width(Length::Fixed(geometry.rendered_size.width))
    .height(Length::Fixed(geometry.rendered_size.height));

    let layered = iced::widget::stack![img, overlay];

    let layered: Element<'_, Message> = if let Some(draft) = &state.editor.text_draft {
        let editor = text_editor(&draft.content)
            .id(state.text_editor_id.clone())
            .on_action(Message::TextDraftAction)
            .key_binding(|key_press| {
                use iced::widget::text_editor::{Binding, KeyPress};
                let KeyPress { key, modifiers, .. } = &key_press;
                let commit_modifier = if cfg!(target_os = "macos") {
                    modifiers.command()
                } else {
                    modifiers.control()
                };
                if commit_modifier
                    && matches!(key, keyboard::Key::Named(keyboard::key::Named::Enter))
                {
                    return Some(Binding::Custom(Message::CommitTextDraft));
                }
                Binding::from_key_press(key_press)
            })
            .font(super::canvas::annotation_font())
            .width(280.0);

        let positioned = container(editor).padding(iced::Padding {
            left: draft.position.x * geometry.scale,
            top: draft.position.y * geometry.scale,
            right: 0.0,
            bottom: 0.0,
        });
        iced::widget::stack![layered, positioned].into()
    } else {
        layered.into()
    };

    let content = container(layered)
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

fn discard_modal<'a>(base: Element<'a, Message>, prompt: &'a str) -> Element<'a, Message> {
    let dialog = container(
        column![
            text(prompt),
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
