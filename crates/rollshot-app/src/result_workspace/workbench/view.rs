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

    // Activity drawer: a left pane of streamed ActivityEntry's (spec §6.1).
    // The pane slot is always child 0 of the row so the canvas stays at a
    // stable tree index (iced tracks widgets by position; reordering would
    // reset the canvas scrollable's viewport). When there is nothing to show
    // it collapses to a zero-width space, keeping run-existing full-canvas.
    let show_activity = wb.run_state.is_running() || !wb.live_activity.is_empty();
    let activity: Element<'a, Message> = if show_activity {
        scrollable(activity_column(wb))
            .width(Length::Fixed(260.0))
            .height(Length::Fill)
            .into()
    } else {
        Space::new().width(Length::Fixed(0.0)).into()
    };
    let main = row![activity, canvas_area, right_pane]
        .spacing(4)
        .height(Length::Fill);

    let mut content = column![run_status_row(wb), bar].spacing(8).padding(8);

    if let Some(banner) = error_message_banner(wb, &state.message) {
        content = content.push(banner);
    }
    if let Some(banner) = result_state_banner(wb) {
        content = content.push(banner);
    }
    content = content.push(main);

    if wb.disclosure_pending {
        let modal = if wb.pending_run.is_some() {
            disclosure_modal(wb)
        } else {
            // ImStart sets disclosure_pending without pending_run (SP6 stub).
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

fn run_status_row<'a>(wb: &'a WorkbenchState) -> Element<'a, Message> {
    use super::RunState;
    // While Running: provider/model label + Cancel (spec §6.3, plan addendum F
    // — non-deferrable; without this a hung run is unstoppable until the 30s
    // wall-time budget elapses). On Terminal: the terminal-state label. The
    // CancelRun handler already existed; only the UI affordance was missing.
    let (status, cancel) = match &wb.run_state {
        RunState::Running { .. } => (
            text(format!(
                "Running: {}",
                super::provider_config::provider_model_label(&wb.provider_config)
            ))
            .size(12),
            Some(button(text("Cancel")).on_press(Message::Workbench(WorkbenchMessage::CancelRun))),
        ),
        RunState::Terminal(terminal) => (
            text(super::state::terminal_state_label(terminal)).size(12),
            None,
        ),
        RunState::Idle => (text("Ready").size(12), None),
    };
    let mut r = row![status, Space::new().width(Length::Fill)]
        .spacing(8)
        .align_y(Alignment::Center);
    if let Some(btn) = cancel {
        r = r.push(btn);
    }
    container(r).padding(4).width(Length::Fill).into()
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
    let total = proposal.map_or(0, |p| p.candidates.len());
    let rejected = wb.review.rejected_count();
    let apply = total.saturating_sub(rejected);
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

    // Mirror the reducer guard: needs an active revision to revise *from*, a
    // proposal, and at least one correction. Otherwise the click is a no-op.
    let revise_enabled = wb.active_revision.is_some()
        && wb
            .pending_proposal
            .as_ref()
            .map(|p| !super::review::assemble_correction_evidence(p, &wb.review).is_empty())
            .unwrap_or(false);

    let actions = row![
        text(summary),
        Space::new().width(Length::Fill),
        button(text("Ask agent to revise")).on_press_maybe(
            revise_enabled.then_some(Message::Workbench(WorkbenchMessage::AskAgentToRevise)),
        ),
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
