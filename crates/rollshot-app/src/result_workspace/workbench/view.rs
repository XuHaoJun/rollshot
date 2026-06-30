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
    let main = row![canvas_area, smart_redaction_panel(wb, &state.message)]
        .spacing(8)
        .height(Length::Fill);

    let content = column![main, review_bar(wb)]
        .spacing(8)
        .height(Length::Fill);

    if wb.disclosure_pending {
        let modal = if wb.pending_run.is_some() {
            disclosure_modal(wb)
        } else {
            let evidence = match wb.pending_proposal.as_ref() {
                Some(proposal) => super::review::assemble_correction_evidence(proposal, &wb.review),
                None => super::review::CorrectionEvidence::default(),
            };
            improve_modal(&evidence)
        };
        iced::widget::stack![content, modal].into()
    } else {
        content.into()
    }
}

fn smart_redaction_panel<'a>(
    wb: &'a WorkbenchState,
    inline_message: &'a Option<super::super::InlineMessage>,
) -> Element<'a, Message> {
    let header = panel_header(wb);
    let activity = scrollable(activity_column(wb))
        .height(Length::Fill)
        .width(Length::Fill);
    let composer = container(composer(wb))
        .padding(8)
        .width(Length::Fill);

    let mut content = column![header].height(Length::Fill);
    if let Some(error) = error_message_banner(wb, inline_message) {
        content = content.push(error);
    }
    if let Some(result_state) = result_state_banner(wb) {
        content = content.push(result_state);
    }
    content = content.push(activity).push(composer);

    container(content)
        .width(Length::Fixed(340.0))
        .height(Length::Fill)
        .style(|_t| iced::widget::container::Style {
            background: Some(iced::Background::Color(iced::Color::from_rgb(
                0.98, 0.98, 0.99,
            ))),
            border: iced::Border {
                color: iced::Color::from_rgb(0.88, 0.88, 0.90),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn panel_header<'a>(wb: &'a WorkbenchState) -> Element<'a, Message> {
    let title = row![
        text("Smart Redaction").size(14),
        text(super::provider_config::provider_model_label(&wb.provider_config)).size(10),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let status = run_status_text(wb);
    let cancel = if wb.run_state.is_running() {
        Some(button(text("Cancel")).on_press(Message::Workbench(WorkbenchMessage::CancelRun)))
    } else {
        None
    };

    let mut status_row = row![text(status).size(11), Space::new().width(Length::Fill)]
        .spacing(8)
        .align_y(Alignment::Center);
    if let Some(cancel) = cancel {
        status_row = status_row.push(cancel);
    }

    container(column![title, status_row].spacing(6))
        .padding(12)
        .width(Length::Fill)
        .into()
}

fn run_status_text(wb: &WorkbenchState) -> String {
    match &wb.run_state {
        super::RunState::Running { .. } => "Running".into(),
        super::RunState::Terminal(terminal) => super::state::terminal_state_label(terminal),
        super::RunState::Idle => "Ready".into(),
    }
}

fn activity_column<'a>(wb: &'a WorkbenchState) -> Element<'a, Message> {
    let mut col = column![].spacing(6).padding(8);
    if wb.live_activity.is_empty() {
        col = col.push(text("(waiting for activity…)").size(11));
    } else {
        for entry in &wb.live_activity {
            col = col.push(activity_entry_view(entry));
        }
    }
    col.into()
}

fn activity_entry_view<'a>(entry: &'a super::state::ActivityEntry) -> Element<'a, Message> {
    use super::state::ToolCardStatus;
    match entry {
        super::state::ActivityEntry::UserMessage(msg) => {
            container(text(format!("You: {msg}")).size(12))
                .style(|_t| iced::widget::container::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgba(
                        0.2, 0.4, 0.8, 0.15,
                    ))),
                    ..Default::default()
                })
                .padding(6)
                .into()
        }
        super::state::ActivityEntry::AssistantText(t) => text(t).size(12).into(),
        super::state::ActivityEntry::ToolCard {
            name,
            status,
            summary,
        } => {
            let mark = match status {
                ToolCardStatus::Running => "…",
                ToolCardStatus::Success => "✓",
                ToolCardStatus::Failed => "✗",
            };
            let mut line = format!("{mark} {name}");
            if !summary.is_empty() {
                line.push_str(&format!(" — {summary}"));
            }
            text(line).size(11).into()
        }
        super::state::ActivityEntry::SourceDiff { tool, lines } => {
            let mut diff = column![text(format!("Source change: {tool}")).size(11)].spacing(2);
            for line in lines {
                diff = diff.push(text(line).size(10).font(iced::Font::MONOSPACE));
            }
            diff.into()
        }
        super::state::ActivityEntry::RunStatus {
            turn,
            budget_summary,
            elapsed,
        } => text(format!("Turn {turn} · {budget_summary} · {elapsed:?}"))
            .size(10)
            .into(),
        super::state::ActivityEntry::TerminalLabel(label) => text(label).size(12).into(),
    }
}

fn review_bar<'a>(wb: &'a WorkbenchState) -> Element<'a, Message> {
    let proposal = wb.pending_proposal.as_ref();
    let summary = super::state::candidate_review_summary(proposal, &wb.review);

    let summary_text = if summary.total > 0 {
        format!(
            "{} candidates · {} to apply · {} rejected · {} low confidence",
            summary.total, summary.apply, summary.rejected, summary.warnings
        )
    } else {
        "No candidates".to_string()
    };

    let mut chips = row![].spacing(8).align_y(Alignment::Center);
    if let Some(proposal) = proposal {
        for item in
            super::state::candidate_review_items(proposal, &wb.review, wb.selected_candidate)
        {
            chips = chips.push(candidate_chip(item));
        }
    }
    let chips = scrollable(chips)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new(),
        ))
        .width(Length::Fill);

    let revise_enabled =
        wb.active_revision.is_some() && wb.pending_proposal.is_some() && wb.corrections_non_empty;

    let selected_reject = wb.selected_candidate.map(|id| {
        if super::state::is_candidate_rejected(&wb.review, id) {
            button(text("Undo reject"))
                .on_press(Message::Workbench(WorkbenchMessage::CandidateUnrejected(id)))
        } else {
            button(text("Reject"))
                .on_press(Message::Workbench(WorkbenchMessage::CandidateDeleted(id)))
        }
    });

    let mut actions = row![].spacing(8).align_y(Alignment::Center);
    if let Some(reject) = selected_reject {
        actions = actions.push(reject);
    }
    if proposal.is_some() {
        actions = actions.push(
            button(text("Discard all"))
                .on_press(Message::Workbench(WorkbenchMessage::DiscardCandidates)),
        );
    }
    actions = actions.push(button(text("Revise")).on_press_maybe(
        revise_enabled.then_some(Message::Workbench(WorkbenchMessage::AskAgentToRevise)),
    ));
    actions = actions.push(
        button(text(format!("Apply {} redactions", summary.apply)))
            .style(button::primary)
            .on_press_maybe(if summary.apply > 0 {
                Some(Message::Workbench(WorkbenchMessage::ApplyCandidates))
            } else {
                None
            }),
    );

    let mut bar = row![
        column![text(summary_text).size(13), chips]
            .spacing(6)
            .width(Length::Fill),
        actions,
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    if summary.warnings > 0 {
        bar = bar.push(
            button(text("Next warning"))
                .on_press(Message::Workbench(WorkbenchMessage::NextWarning)),
        );
    }

    container(bar)
        .padding(10)
        .width(Length::Fill)
        .height(Length::Fixed(88.0))
        .into()
}

fn candidate_chip<'a>(item: super::state::CandidateReviewItem) -> Element<'a, Message> {
    let label = if item.low_confidence && !item.rejected {
        format!("{} ⚠ {} {}%", item.sequence, item.label, item.confidence_percent)
    } else {
        format!("{} {} {}%", item.sequence, item.label, item.confidence_percent)
    };

    let border = if item.rejected {
        iced::Color::from_rgb(0.82, 0.82, 0.84)
    } else {
        let (r, g, b) = super::state::confidence_accent(item.low_confidence, item.selected);
        iced::Color::from_rgb(r, g, b)
    };
    let background = if item.rejected {
        iced::Color::from_rgb(0.96, 0.96, 0.97)
    } else if item.low_confidence {
        iced::Color::from_rgb(1.0, 0.96, 0.86)
    } else {
        iced::Color::from_rgb(0.94, 0.98, 0.95)
    };

    let chip = container(text(label).size(11))
        .padding([4, 9])
        .style(move |_t| iced::widget::container::Style {
            background: Some(iced::Background::Color(background)),
            border: iced::Border {
                color: border,
                width: if item.selected { 2.0 } else { 1.0 },
                radius: 12.0.into(),
            },
            text_color: Some(if item.rejected {
                iced::Color::from_rgb(0.55, 0.55, 0.58)
            } else {
                iced::Color::from_rgb(0.11, 0.11, 0.12)
            }),
            ..Default::default()
        });

    button(chip)
        .padding(0)
        .style(|_theme, _status| iced::widget::button::Style {
            background: None,
            text_color: iced::Color::from_rgb(0.11, 0.11, 0.12),
            ..Default::default()
        })
        .on_press(Message::Workbench(WorkbenchMessage::CandidateSelected(
            item.id,
        )))
        .into()
}

fn composer<'a>(wb: &'a WorkbenchState) -> Element<'a, Message> {
    // Disabled while Running (spec §6.4). Without on_input/on_submit the
    // text_input is effectively read-only; the Send button renders without
    // on_press so it's non-interactive.
    let running = wb.run_state.is_running();
    let mut input = text_input("Ask the agent…", &wb.composer);
    if !running {
        input = input
            .on_input(|s| Message::Workbench(WorkbenchMessage::ComposerChanged(s)))
            .on_submit(Message::Workbench(WorkbenchMessage::SendRequested));
    }
    let send = if running {
        button(text("Send"))
    } else {
        button(text("Send")).on_press(Message::Workbench(WorkbenchMessage::SendRequested))
    };
    row![input.width(Length::Fill), send].spacing(8).into()
}

fn disclosure_modal<'a>(wb: &'a WorkbenchState) -> Element<'a, Message> {
    use super::PayloadMode;
    let label = super::provider_config::provider_model_label(&wb.provider_config);
    let mut col = column![
        text(format!("Send to {label}")).size(16),
        text("This run will send:").size(13),
    ];
    if matches!(wb.payload_mode, PayloadMode::FullScreenshot) {
        col = col.push(text("  Screenshot image (full-screenshot mode)"));
    }
    let dialog = container(
        col.push(text("  Local OCR/layout summary"))
            .push(text("Privacy mode:").size(13))
            .push(iced::widget::radio(
                "Full screenshot — best accuracy",
                PayloadMode::FullScreenshot,
                Some(wb.payload_mode),
                |m| Message::Workbench(WorkbenchMessage::PayloadModeSelected(m)),
            ))
            .push(iced::widget::radio(
                "OCR/layout only — no image upload",
                PayloadMode::OcrLayoutOnly,
                Some(wb.payload_mode),
                |m| Message::Workbench(WorkbenchMessage::PayloadModeSelected(m)),
            ))
            .push(Space::new().height(Length::Fixed(12.0)))
            .push(
                row![
                    button(text(format!("Send to {}", wb.provider_config.provider)))
                        .on_press(Message::Workbench(WorkbenchMessage::DisclosureConfirmed)),
                    button(text("Cancel"))
                        .on_press(Message::Workbench(WorkbenchMessage::DisclosureCancelled)),
                ]
                .spacing(12),
            )
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

fn improve_modal<'a>(evidence: &super::review::CorrectionEvidence) -> Element<'a, Message> {
    let dialog = container(
        column![
            text("Correction evidence to send:").size(14),
            text(format!("- {evidence}")),
            iced::widget::checkbox(true).label("Include manually added candidates as examples"),
            text("Not available in this release (SP6.1)").size(11),
            Space::new().height(Length::Fixed(12.0)),
            row![
                button(text("Send improvement")),
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
        container(text(format!(
            "{total} candidates found. Review before applying."
        )))
        .padding(12)
        .into(),
    )
}
