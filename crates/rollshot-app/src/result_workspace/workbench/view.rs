use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Length};

use super::super::{Message, ResultWorkspace};
use super::{WorkbenchMessage, WorkbenchState};

pub fn workbench_view<'a>(state: &'a ResultWorkspace) -> Element<'a, Message> {
    let wb = match &state.mode {
        super::WorkspaceMode::Workbench(wb) => wb,
        _ => return iced::widget::text("Not in workbench mode").into(),
    };

    let canvas_area = super::super::view::canvas_view(state, state.original_size());

    let bar = review_bar(wb);
    let list = candidate_list(wb);
    let composer = composer(wb);

    let right_pane = scrollable(column![list, composer].spacing(8))
        .width(Length::Fixed(280.0))
        .height(Length::Fill);

    let main = row![canvas_area, right_pane]
        .spacing(4)
        .height(Length::Fill);

    let mut content = column![bar].spacing(8).padding(8);

    if let Some(banner) = error_message_banner(wb, &state.message) {
        content = content.push(banner);
    }
    if let Some(banner) = result_state_banner(wb) {
        content = content.push(banner);
    }
    content = content.push(main);

    if wb.disclosure_pending {
        iced::widget::stack![content, disclosure_modal(wb)].into()
    } else {
        content.into()
    }
}

fn review_bar<'a>(wb: &'a WorkbenchState) -> Element<'a, Message> {
    let proposal = wb.pending_proposal.as_ref();
    let total = proposal.map_or(0, |p| p.candidates.len());
    let rejected = wb.review.rejected_count();
    let apply = total - rejected;
    let warnings = proposal.map_or(0, |p| super::state::CandidateReview::warning_count(p, 0.75));

    let summary = if total > 0 {
        format!("Apply {apply} redactions, skip {rejected} rejected · {warnings} warnings")
    } else {
        "No candidates".to_string()
    };

    let pending_warning = if total > 0 {
        text(format!(
            "{total} proposed redactions are preview-only. Apply before safe copy/save."
        ))
        .size(11)
    } else {
        text("")
    };

    let actions = row![
        text(summary),
        Space::new().width(Length::Fill),
        button(text(format!("Apply {apply} redactions"))).on_press_maybe(if apply > 0 {
            Some(Message::Workbench(WorkbenchMessage::ApplyCandidates))
        } else {
            None
        }),
        button(text("Next warning")).on_press_maybe(if warnings > 0 {
            Some(Message::Workbench(WorkbenchMessage::NextWarning))
        } else {
            None
        }),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    container(column![pending_warning, actions].spacing(4))
        .padding(8)
        .width(Length::Fill)
        .into()
}

fn candidate_list<'a>(wb: &'a WorkbenchState) -> Element<'a, Message> {
    use super::state::CandidateReviewState;
    let Some(proposal) = wb.pending_proposal.as_ref() else {
        return text("No candidates").into();
    };
    let mut col = column![].spacing(4).padding(8);
    for cand in &proposal.candidates {
        let is_rejected = matches!(
            wb.review.per_candidate.get(&cand.id),
            Some(CandidateReviewState::Rejected)
        );
        let r = row![
            text(format!("{} {:.0}%", cand.label, cand.confidence * 100.0)).size(11),
            Space::new().width(Length::Fill),
            button(text("Jump")).on_press(Message::Workbench(WorkbenchMessage::CandidateSelected(
                cand.id
            ))),
            button(text(if is_rejected { "Undo" } else { "Reject" })).on_press(Message::Workbench(
                if is_rejected {
                    WorkbenchMessage::CandidateUnrejected(cand.id)
                } else {
                    WorkbenchMessage::CandidateDeleted(cand.id)
                }
            )),
        ]
        .spacing(8);
        col = col.push(r);
    }
    scrollable(col).height(Length::Fill).into()
}

fn composer<'a>(wb: &'a WorkbenchState) -> Element<'a, Message> {
    let input = text_input("Ask the agent…", &wb.composer)
        .on_input(|s| Message::Workbench(WorkbenchMessage::ComposerChanged(s)))
        .on_submit(Message::Workbench(WorkbenchMessage::SendRequested));
    row![
        input.width(Length::Fill),
        button(text("Send")).on_press(Message::Workbench(WorkbenchMessage::SendRequested))
    ]
    .spacing(8)
    .into()
}

fn disclosure_modal<'a>(wb: &'a WorkbenchState) -> Element<'a, Message> {
    use super::PayloadMode;
    let label = super::provider_config::provider_model_label(&wb.provider_config);
    let dialog = container(
        column![
            text(format!("Send to {label}")).size(16),
            text("This run will send:").size(13),
            text("  Screenshot image (full-screenshot mode)"),
            text("  Local OCR/layout summary"),
            text("Privacy mode:").size(13),
            iced::widget::radio(
                "Full screenshot — best accuracy",
                PayloadMode::FullScreenshot,
                Some(wb.payload_mode),
                |m| Message::Workbench(WorkbenchMessage::PayloadModeSelected(m)),
            ),
            iced::widget::radio(
                "OCR/layout only — no image upload",
                PayloadMode::OcrLayoutOnly,
                Some(wb.payload_mode),
                |m| Message::Workbench(WorkbenchMessage::PayloadModeSelected(m)),
            ),
            Space::new().height(Length::Fixed(12.0)),
            row![
                button(text(format!("Send to {}", wb.provider_config.provider)))
                    .on_press(Message::Workbench(WorkbenchMessage::DisclosureConfirmed)),
                button(text("Cancel"))
                    .on_press(Message::Workbench(WorkbenchMessage::DisclosureCancelled)),
            ]
            .spacing(12),
        ]
        .spacing(8)
        .padding(24)
        .max_width(450),
    )
    .style(|_t| iced::widget::container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgba(
            0.0, 0.0, 0.0, 0.7,
        ))),
        ..Default::default()
    })
    .center_x(Length::Fill)
    .center_y(Length::Fill);
    iced::widget::opaque(dialog)
}

fn error_message_banner<'a>(
    wb: &'a WorkbenchState,
    inline_message: &'a Option<super::super::InlineMessage>,
) -> Option<Element<'a, Message>> {
    let mut parts: Vec<Element<'a, Message>> = Vec::new();

    if let Some(err) = &wb.error {
        parts.push(text(format!("{err}")).into());
    }

    if let Some(msg) = inline_message {
        parts.push(
            row![
                text(msg.text()),
                Space::new().width(Length::Fill),
                button(text("Dismiss")).on_press(Message::DismissMessage),
            ]
            .align_y(Alignment::Center)
            .spacing(8)
            .into(),
        );
    }

    if parts.is_empty() {
        return None;
    }

    let mut col = column![].spacing(4).padding(8);
    for p in parts {
        col = col.push(p);
    }
    Some(container(col).width(Length::Fill).into())
}

fn result_state_banner<'a>(wb: &'a WorkbenchState) -> Option<Element<'a, Message>> {
    let proposal = wb.pending_proposal.as_ref()?;
    let total = proposal.candidates.len();
    if total == 0 {
        return Some(
            container(
                column![
                    text("This preset did not find anything on this screenshot."),
                    row![
                        button(text("Improve preset"))
                            .on_press(Message::Workbench(WorkbenchMessage::ImStart)),
                        button(text("Manual redact"))
                            .on_press(Message::SelectTool(super::super::canvas::Tool::Redact)),
                    ]
                    .spacing(8),
                ]
                .spacing(8)
                .padding(12),
            )
            .into(),
        );
    }
    let warnings = super::state::CandidateReview::warning_count(proposal, 0.75);
    if warnings == total {
        return Some(
            container(
                column![
                    text("Only low-confidence matches were found."),
                    row![
                        button(text("Review warnings"))
                            .on_press(Message::Workbench(WorkbenchMessage::NextWarning)),
                        button(text("Improve preset"))
                            .on_press(Message::Workbench(WorkbenchMessage::ImStart)),
                        button(text("Discard"))
                            .on_press(Message::Workbench(WorkbenchMessage::DiscardCandidates)),
                    ]
                    .spacing(8),
                ]
                .spacing(8)
                .padding(12),
            )
            .into(),
        );
    }
    Some(
        container(
            text(format!(
                "{total} candidates found. Review before applying."
            )),
        )
        .padding(12)
        .into(),
    )
}
