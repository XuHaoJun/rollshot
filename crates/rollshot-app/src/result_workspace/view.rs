use super::viewport::{geometry_for, ZoomDirection, ZoomMode};
use iced::widget::{
    button, column, container, image as image_widget, mouse_area, opaque, row, scrollable, text,
    text_editor, tooltip, Space,
};
use iced::{keyboard, mouse, Alignment, Color, Element, Length, Size, Vector};

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

    let copy_label = super::secure_sharing::copy_label(&state.document);
    let save_label = super::secure_sharing::save_label(&state.document);

    row![
        button(text("Close")).on_press(Message::RequestClose),
        text(state.document.display_name()).width(Length::Fill),
        tool_button(ICON_SELECT, "Select", "V", Tool::Select, state),
        tool_button(ICON_NUMBER, "Number", "N", Tool::Number, state),
        tool_button(ICON_TEXT, "Text", "T", Tool::Text, state),
        tool_button(ICON_REDACT, "Redact", "R", Tool::Redact, state),
        button(text("Smart Redaction")).on_press(Message::SmartRedaction),
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
        button(text(copy_label)).on_press(Message::Copy),
        button(text("\u{25BE}")).on_press(Message::ToggleCopyMenu),
        button(text(save_label)).on_press(Message::SaveAs),
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

    let disclosure = retained_original_disclosure(state);
    let message_area = message_row(state);

    let canvas_area = canvas_view(state, original);

    let status = status_bar(state, original);

    let body: Element<'_, Message> = match &state.mode {
        super::workbench::WorkspaceMode::Normal => {
            let workspace_row: Element<'_, Message> = if state.editor.navigator_open {
                row![canvas_area, super::navigator::navigator_panel(state)]
                    .spacing(4)
                    .into()
            } else {
                canvas_area
            };
            column![
                toolbar(state),
                disclosure,
                message_area,
                workspace_row,
                status
            ]
            .spacing(8)
            .padding(8)
            .into()
        }
        super::workbench::WorkspaceMode::Workbench(_) => column![
            toolbar(state),
            super::workbench::view::workbench_view(state)
        ]
        .spacing(8)
        .padding(8)
        .into(),
    };

    let layout: Element<'_, Message> = if let Some(prompt) = &state.pending_discard {
        discard_modal(body, prompt.text())
    } else if let Some(action) = state.pending_unredacted_action {
        unredacted_action_modal(body, action)
    } else {
        body
    };
    if state.editor.copy_menu_open
        && state.pending_discard.is_none()
        && state.pending_unredacted_action.is_none()
    {
        copy_menu(layout, state)
    } else {
        layout
    }
}

fn copy_menu<'a>(base: Element<'a, Message>, state: &'a ResultWorkspace) -> Element<'a, Message> {
    let label = super::secure_sharing::copy_original_label(&state.document);
    let menu = container(button(text(label)).on_press(Message::CopyOriginal)).padding(4);

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
    let action = super::secure_sharing::reveal_action(&state.document);
    let btn = button(text(action.label()));
    if !matches!(action, super::secure_sharing::RevealAction::Disabled) {
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

pub(crate) fn canvas_view<'a>(
    state: &'a ResultWorkspace,
    image_size: Size,
) -> Element<'a, Message> {
    let geometry = geometry_for(state.viewport.zoom, image_size, state.viewport_bounds);

    let img = image_widget(state.image_handle.clone())
        .width(Length::Fixed(geometry.rendered_size.width))
        .height(Length::Fixed(geometry.rendered_size.height));

    let (pending_proposal, review, selected_candidate) = match &state.mode {
        super::workbench::WorkspaceMode::Workbench(wb) => (
            wb.pending_proposal.as_ref(),
            Some(&wb.review),
            wb.selected_candidate,
        ),
        _ => (None, None, None),
    };
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
        pending_proposal,
        review,
        selected_candidate,
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

fn confirmation_dialog_style(theme: &iced::Theme) -> container::Style {
    container::rounded_box(theme)
}

fn confirmation_scrim_style(_theme: &iced::Theme) -> container::Style {
    container::Style::default().background(Color {
        a: 0.8,
        ..Color::BLACK
    })
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
    .padding(20)
    .style(confirmation_dialog_style);

    // Follow iced's official modal pattern: an opaque dialog over a styled
    // full-window scrim. Outside clicks remain a no-op for destructive prompts.
    let scrim = opaque(
        mouse_area(
            container(opaque(dialog))
                .style(confirmation_scrim_style)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
        )
        .interaction(mouse::Interaction::Idle)
        .on_press(Message::ModalScrimPressed),
    );

    iced::widget::stack![base, scrim].into()
}

fn retained_original_disclosure(state: &ResultWorkspace) -> Element<'_, Message> {
    match super::secure_sharing::retained_original_disclosure(&state.document) {
        Some(disclosure) => container(text(disclosure).size(12))
            .width(Length::Fill)
            .padding([2, 4])
            .into(),
        None => Space::new()
            .width(Length::Fill)
            .height(Length::Fixed(0.0))
            .into(),
    }
}

fn unredacted_action_modal<'a>(
    base: Element<'a, Message>,
    action: super::secure_sharing::UnredactedAction,
) -> Element<'a, Message> {
    let dialog = container(
        column![
            text(action.prompt()),
            row![
                button(text("Cancel")).on_press(Message::CancelUnredactedAction),
                button(text(action.confirm_label())).on_press(Message::ConfirmUnredactedAction),
            ]
            .spacing(8),
        ]
        .spacing(12)
        .align_x(Alignment::Center),
    )
    .padding(20)
    .style(confirmation_dialog_style);

    let scrim = opaque(
        mouse_area(
            container(opaque(dialog))
                .style(confirmation_scrim_style)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
        )
        .interaction(mouse::Interaction::Idle)
        .on_press(Message::ModalScrimPressed),
    );
    iced::widget::stack![base, scrim].into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discard_dialog_has_solid_background() {
        let style = confirmation_dialog_style(&iced::Theme::Dark);
        let iced::Background::Color(color) = style.background.expect("dialog background") else {
            panic!("dialog background must be a solid color");
        };
        assert_eq!(color.a, 1.0);
    }

    #[test]
    fn discard_scrim_is_translucent_black() {
        let style = confirmation_scrim_style(&iced::Theme::Dark);
        let iced::Background::Color(color) = style.background.expect("scrim background") else {
            panic!("scrim background must be a solid color");
        };
        assert_eq!(color.r, 0.0);
        assert_eq!(color.g, 0.0);
        assert_eq!(color.b, 0.0);
        assert!(color.a > 0.0 && color.a < 1.0);
    }

    #[test]
    fn unredacted_confirmation_uses_blocking_modal_styles() {
        let dialog = confirmation_dialog_style(&iced::Theme::Dark);
        let scrim = confirmation_scrim_style(&iced::Theme::Dark);
        assert!(dialog.background.is_some());
        assert!(scrim.background.is_some());
    }
}
