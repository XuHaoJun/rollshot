use iced::widget::{
    button, checkbox, column, container, image, mouse_area, opaque, row, scrollable, stack, text,
    text_input, Space,
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

    let body = if state.issue_pack.is_some() {
        issue_pack_modal(body, state)
    } else {
        body
    };

    let body = if state.storyboard_preview.is_some() {
        storyboard_preview_modal(body, state)
    } else {
        body
    };

    let body = if state.ffmpeg_setup.is_some() {
        ffmpeg_setup_modal(body, state)
    } else {
        body
    };

    if state.pending_discard {
        discard_modal(body)
    } else {
        body
    }
}

fn header(state: &TimelineWorkspace) -> Element<'_, Message> {
    let advisory: Element<Message> = match state.capability {
        rollshot_action::InputCapability::VisualOnly { .. } => {
            #[cfg(target_os = "macos")]
            {
                row![
                    text("Visual-only detection.").size(13),
                    button(text("Open System Settings"))
                        .on_press(Message::OpenInputMonitoringSettings)
                        .style(button::secondary),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            }
            #[cfg(not(target_os = "macos"))]
            {
                text("Visual-only detection.").size(13).into()
            }
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
        button(text("Preview Storyboard"))
            .on_press(Message::PreviewStoryboardRequested)
            .style(button::secondary),
        button(text("Export Storyboard"))
            .on_press(Message::ExportStoryboardRequested)
            .style(button::secondary),
        button(text("Export MP4"))
            .on_press(Message::ExportMp4Requested)
            .style(button::secondary),
        button(text("Export Guide"))
            .on_press(Message::ExportRequested)
            .style(button::primary),
        button(text("Export Bug Report..."))
            .on_press(Message::ExportBugReport)
            .style(button::secondary),
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

fn issue_pack_modal<'a>(
    base: Element<'a, Message>,
    state: &'a TimelineWorkspace,
) -> Element<'a, Message> {
    let dialog = state.issue_pack.as_ref().expect("checked by caller");
    let export_enabled = dialog.review_confirmed && dialog.pending_kind.is_none();
    let steps = state.guide.steps().len();

    let dialog_view = container(
        column![
            text("Issue Pack Export").size(18),
            text(format!(
                "Included: issue.md, manifest.json, {steps} Action Guide steps"
            )),
            text("Safety:"),
            column![
                text("Action Guide keyframes are reviewed evidence images."),
                text("Keyframes are not automatically redacted."),
                text("Review every keyframe before sharing."),
            ],
            checkbox(dialog.include_gif)
                .label("Include guide.gif when GIF export succeeds")
                .on_toggle(Message::IssuePackIncludeGifChanged),
            checkbox(dialog.review_confirmed)
                .label("I reviewed the images and keyframes included in this bug report.")
                .on_toggle(Message::IssuePackReviewChanged),
            row![
                button(text("Export Folder"))
                    .on_press_maybe(export_enabled.then_some(Message::IssuePackExportFolder))
                    .style(button::primary),
                button(text("Export ZIP"))
                    .on_press_maybe(export_enabled.then_some(Message::IssuePackExportZip))
                    .style(button::secondary),
                button(text("Cancel")).on_press(Message::IssuePackCancel),
            ]
            .spacing(8),
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fixed(500.0))
    .style(container::rounded_box);

    let scrim = opaque(
        mouse_area(
            container(opaque(dialog_view))
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
        .on_press(Message::IssuePackCancel),
    );
    stack![base, scrim].into()
}

fn storyboard_preview_modal<'a>(
    base: Element<'a, Message>,
    state: &'a TimelineWorkspace,
) -> Element<'a, Message> {
    let preview = state
        .storyboard_preview
        .as_ref()
        .expect("checked by caller");
    let preview_image = image(preview.handle.clone())
        .width(Length::Fill)
        .height(Length::Shrink);

    let dialog_view = container(
        column![
            row![
                text("Preview Storyboard").size(18),
                Space::new().width(Length::Fill),
                text(format!(
                    "{} steps · {}×{}",
                    preview.step_count, preview.width, preview.height
                ))
                .size(12),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            container(scrollable(preview_image))
                .width(Length::Fill)
                .height(Length::Fixed(520.0))
                .style(container::rounded_box),
            row![
                button(text("Export PNG"))
                    .on_press(Message::ExportStoryboardRequested)
                    .style(button::primary),
                button(text("Close")).on_press(Message::PreviewStoryboardClosed),
            ]
            .spacing(8),
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fixed(760.0))
    .style(container::rounded_box);

    let scrim = opaque(
        mouse_area(
            container(opaque(dialog_view))
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
        .on_press(Message::PreviewStoryboardClosed),
    );

    stack![base, scrim].into()
}

fn ffmpeg_setup_modal<'a>(
    base: Element<'a, Message>,
    state: &'a TimelineWorkspace,
) -> Element<'a, Message> {
    let dialog = state.ffmpeg_setup.as_ref().expect("checked by caller");
    let managed = dialog.info.managed_download.as_ref();
    let managed_enabled = managed.is_some() && !dialog.downloading;
    let details = if let Some(meta) = managed {
        column![
            text(format!("Source: {}", meta.source_url))
                .size(12)
                .width(Length::Fill),
            text(format!("Version: {}", meta.version))
                .size(12)
                .width(Length::Fill),
            text(format!("License: {} ({})", meta.license, meta.license_url))
                .size(12)
                .width(Length::Fill),
            text(format!("Archive size: {} bytes", meta.archive_size))
                .size(12)
                .width(Length::Fill),
            text(format!("SHA256: {}", meta.archive_sha256))
                .size(12)
                .width(Length::Fill),
            text(format!(
                "Install location: {}",
                dialog.info.install_location.display()
            ))
            .size(12)
            .width(Length::Fill),
        ]
    } else {
        column![text("Managed FFmpeg is not available for this platform.")]
    };

    let dialog_view = container(
        column![
            text("FFmpeg is required to export MP4").size(18),
            details.spacing(6),
            row![
                button(text("Use system FFmpeg / install manually"))
                    .on_press(Message::FfmpegUseSystem)
                    .style(button::secondary),
                button(text(if dialog.downloading {
                    "Downloading..."
                } else {
                    "Download managed FFmpeg"
                }))
                .on_press_maybe(managed_enabled.then_some(Message::FfmpegDownloadManaged))
                .style(button::primary),
                button(text("Cancel")).on_press(Message::FfmpegSetupCancel),
            ]
            .spacing(8),
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fixed(620.0))
    .style(container::rounded_box);

    let scrim = opaque(
        mouse_area(
            container(opaque(dialog_view))
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
        .on_press(Message::FfmpegSetupCancel),
    );
    stack![base, scrim].into()
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

        // Storyboard preview modal.
        let mut preview = ws(recording_from_frames(), InputCapability::SemanticEvents);
        crate::timeline_workspace::update::update(
            &mut preview,
            Message::PreviewStoryboardRequested,
        );
        assert!(preview.storyboard_preview.is_some());
        let _ = view(&preview);

        // Empty guide / no selection.
        let empty = ws(synthetic_recording(0), InputCapability::SemanticEvents);
        let _ = view(&empty);
    }
}
