use iced::widget::{
    button, column, container, image, mouse_area, opaque, row, scrollable, stack, text, text_input,
    Space,
};
use iced::{mouse, Alignment, Color, Element, Length, Theme};

use super::{Message, TimelineWorkspace};

pub fn view(state: &TimelineWorkspace) -> Element<'_, Message> {
    let body: Element<Message> = column![
        header(state),
        message_row(state),
        main_area(state),
        strip_row(state),
    ]
    .spacing(8)
    .padding(12)
    .into();

    if state.pending_discard {
        discard_modal(body)
    } else {
        body
    }
}

fn header(state: &TimelineWorkspace) -> Element<'_, Message> {
    let advisory: Element<Message> = match state.capability {
        rollshot_action::InputCapability::VisualOnly { .. } => {
            text("Visual-only detection.").size(13).into()
        }
        rollshot_action::InputCapability::SemanticEvents => Space::new()
            .width(Length::Shrink)
            .height(Length::Shrink)
            .into(),
    };
    row![
        advisory,
        Space::new().width(Length::Fill),
        button(text("Discard"))
            .on_press(Message::DiscardRequested)
            .style(button::secondary),
        button(text("Export GIF"))
            .on_press(Message::ExportGifRequested)
            .style(button::secondary),
        button(text("Export Guide"))
            .on_press(Message::ExportRequested)
            .style(button::primary),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn message_row(state: &TimelineWorkspace) -> Element<'_, Message> {
    match &state.message {
        Some(msg) => container(
            row![
                text(msg.clone()).width(Length::Fill),
                button(text("Dismiss")).on_press(Message::DismissBanner),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding(8)
        .into(),
        None => Space::new()
            .width(Length::Fill)
            .height(Length::Fixed(0.0))
            .into(),
    }
}

fn main_area(state: &TimelineWorkspace) -> Element<'_, Message> {
    row![step_list(state), detail_panel(state)]
        .spacing(8)
        .height(Length::Fill)
        .into()
}

fn step_list(state: &TimelineWorkspace) -> Element<'_, Message> {
    let mut col = column![].spacing(4);
    for step in state.guide.steps() {
        let selected = state.selected == Some(step.index);
        let label = text(format!("{}. {}", step.index, step.title));
        col = col.push(
            button(label)
                .width(Length::Fill)
                .on_press(Message::SelectStep(step.index))
                .style(if selected {
                    button::primary
                } else {
                    button::secondary
                }),
        );
    }
    container(scrollable(col))
        .width(Length::FillPortion(1))
        .height(Length::Fill)
        .into()
}

fn detail_panel(state: &TimelineWorkspace) -> Element<'_, Message> {
    let content: Element<Message> = match state.selected_step() {
        Some(step) => {
            let keyframe: Element<Message> = match &state.keyframe_handle {
                Some(handle) => image(handle.clone()).into(),
                None => text("(keyframe unavailable)").into(),
            };
            column![
                container(keyframe)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::Center)
                    .align_y(Alignment::Center),
                text_input("Step title", &step.title).on_input(Message::TitleChanged),
                button(text("Delete step"))
                    .on_press(Message::DeleteStep)
                    .style(button::danger),
            ]
            .spacing(8)
            .into()
        }
        None => container(text("No steps detected."))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into(),
    };
    container(content)
        .width(Length::FillPortion(4))
        .height(Length::Fill)
        .into()
}

fn strip_row(state: &TimelineWorkspace) -> Element<'_, Message> {
    let current = state.selected_step().map(|s| s.keyframe);
    let mut strip = row![].spacing(6);
    for frame in &state.strip {
        let selected = current == Some(frame.id);
        strip = strip.push(
            button(image(frame.handle.clone()).width(Length::Fixed(96.0)))
                .on_press(Message::ReplaceKeyframe(frame.id))
                .style(if selected {
                    button::primary
                } else {
                    button::secondary
                }),
        );
    }
    container(
        scrollable(strip).direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new(),
        )),
    )
    .height(Length::Fixed(120.0))
    .into()
}

fn discard_modal(base: Element<'_, Message>) -> Element<'_, Message> {
    let dialog = container(
        column![
            text("Discard this guide?").size(18),
            text("The recording and all detected steps will be deleted.").size(13),
            row![
                button(text("Cancel")).on_press(Message::CancelDiscard),
                button(text("Discard"))
                    .on_press(Message::ConfirmDiscard)
                    .style(button::danger),
            ]
            .spacing(8),
        ]
        .spacing(12)
        .align_x(Alignment::Center),
    )
    .padding(20)
    .style(container::rounded_box);

    let scrim = opaque(
        mouse_area(
            container(opaque(dialog))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .style(|_theme: &Theme| container::Style {
                    background: Some(
                        Color {
                            a: 0.8,
                            ..Color::BLACK
                        }
                        .into(),
                    ),
                    ..container::Style::default()
                }),
        )
        .interaction(mouse::Interaction::Idle)
        .on_press(Message::CancelDiscard),
    );

    stack![base, scrim].into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline_workspace::tests::{recording_from_frames, synthetic_recording};
    use crate::timeline_workspace::TimelineWorkspace;
    use rollshot_action::{CaptureRegion, InputCapability, InputSourceKind};

    fn ws(recording: rollshot_action::Recording, capability: InputCapability) -> TimelineWorkspace {
        TimelineWorkspace::new(
            recording,
            CaptureRegion {
                x: 0,
                y: 0,
                width: 32,
                height: 32,
            },
            capability,
            InputSourceKind::LinuxEvdev,
        )
    }

    #[test]
    fn view_builds_for_selected_empty_and_discard_states() {
        // Selected step with real handles + semantic header.
        let selected = ws(recording_from_frames(), InputCapability::SemanticEvents);
        let _ = view(&selected);

        // Visual-only advisory + inline message + discard modal.
        let mut degraded = ws(
            synthetic_recording(2),
            InputCapability::VisualOnly {
                reason: rollshot_action::DegradedReason::PermissionDenied,
            },
        );
        degraded.message = Some("export failed: disk full".to_string());
        degraded.pending_discard = true;
        let _ = view(&degraded);

        // Empty guide / no selection.
        let empty = ws(synthetic_recording(0), InputCapability::SemanticEvents);
        let _ = view(&empty);
    }
}
