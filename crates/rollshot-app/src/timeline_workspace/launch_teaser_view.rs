//! Launch teaser review surface view.
//!
//! Renders the Create teaser entry control, the full review layout (shot cards,
//! keyframe preview, controls, validation, provenance, agent diff), and the
//! completion UI. Uses existing Timeline Workspace visual tokens and iced 0.14 APIs.

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Length};

use super::launch_teaser::{
    LaunchTeaserCompletedState, LaunchTeaserEligibility, LaunchTeaserReviewState, LaunchTeaserState,
};
use super::update::Message;
use super::TimelineWorkspace;

/// User-visible disabled reason copy for the Create teaser button.
pub(crate) fn teaser_disabled_reason(ws: &TimelineWorkspace) -> Option<&'static str> {
    ws.launch_teaser_eligibility().disabled_reason()
}

/// Whether the Create teaser button should be shown.
pub(crate) fn can_create_teaser(ws: &TimelineWorkspace) -> bool {
    matches!(ws.launch_teaser_eligibility(), LaunchTeaserEligibility::Eligible)
}

/// The main teaser view element. Returns None when the teaser is Closed.
pub(crate) fn teaser_view<'a>(ws: &'a TimelineWorkspace) -> Option<Element<'a, Message>> {
    match &ws.launch_teaser {
        LaunchTeaserState::Closed => None,
        LaunchTeaserState::Seeding { .. } => Some(seeding_view()),
        LaunchTeaserState::Reviewing(review) => Some(review_view(ws, review)),
        LaunchTeaserState::AgentRunning { review, .. } => Some(agent_running_view(review)),
        LaunchTeaserState::PreviewRendering { review, .. } => {
            Some(rendering_view(review, "Preview rendering..."))
        }
        LaunchTeaserState::FinalRendering { review, .. } => {
            Some(rendering_view(review, "Final rendering..."))
        }
        LaunchTeaserState::Completed(completed) => Some(completion_view(completed)),
    }
}

fn seeding_view<'a>() -> Element<'a, Message> {
    container(text("Creating teaser draft...").size(16))
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn review_view<'a>(ws: &'a TimelineWorkspace, review: &'a LaunchTeaserReviewState) -> Element<'a, Message> {
    let header = row![
        text("Launch Teaser Review").size(20),
        container("  ").width(Length::Fill),
        button("Close").on_press(Message::CloseTeaser),
    ]
    .align_y(iced::Alignment::Center)
    .spacing(8);

    // Stale banner
    let stale_banner = if review.stale {
        Some(
            container(
                text("Plan is stale. Regenerate after guide changes.")
                    .size(14)
                    .style(|_theme| iced::widget::text::Style { color: Some(iced::Color::from_rgb(1.0, 0.8, 0.0)) }),
            )
            .padding(8),
        )
    } else {
        None
    };

    // Validation banner
    let validation_banner = review.validation_message.as_ref().map(|msg| {
        container(text(format!("Validation: {msg}")).size(14))
            .padding(8)
    });

    // Shot cards
    let shots: Element<'a, Message> = review
        .plan
        .shots
        .iter()
        .enumerate()
        .fold(column![].spacing(4), |col, (i, shot)| {
            col.push(
                container(row![
                    text(format!("Shot {}: Step {}", i + 1, shot.reviewed_step_id.0)).size(14),
                    container("").width(Length::Fill),
                    text(format!(
                        "{}ms-{}ms",
                        shot.source_start_ms, shot.source_end_ms
                    ))
                    .size(12),
                ])
                .padding(4)
                .width(Length::Fill),
            )
        })
        .into();

    // Hook/outro
    let hook_outro = column![
        text(format!("Hook: {}", review.plan.hook)).size(14),
        text(format!("Outro: {}", review.plan.outro_text)).size(14),
    ]
    .spacing(4);

    // Content review checkbox area
    let content_review = {
        let checked = review.content_reviewed;
        let label = if checked {
            "Content reviewed ✓"
        } else {
            "Review captured content before rendering"
        };
        button(text(label).size(14))
            .on_press(Message::TeaserSetContentReviewed(!checked))
    };

    // Preview / Render buttons
    let preview_btn = button("Preview")
        .on_press_maybe(if review.render_disabled() {
            None
        } else {
            Some(Message::TeaserPreviewRequested)
        });

    let render_btn = button("Render")
        .on_press_maybe(if review.final_render_gated() {
            None
        } else {
            Some(Message::TeaserRenderRequested)
        });

    let actions = row![preview_btn, render_btn].spacing(8);

    let mut body = column![header].spacing(8);

    if let Some(banner) = stale_banner {
        body = body.push(banner);
    }
    if let Some(banner) = validation_banner {
        body = body.push(banner);
    }

    body = body
        .push(shots)
        .push(hook_outro)
        .push(content_review)
        .push(actions);

    container(scrollable(body).height(Length::Fill))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn agent_running_view<'a>(_review: &'a LaunchTeaserReviewState) -> Element<'a, Message> {
    container(
        column![
            text("Agent improving teaser...").size(16),
            button("Cancel").on_press(Message::CloseTeaser),
        ]
        .spacing(8)
        .align_x(iced::Alignment::Center),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

fn rendering_view<'a>(_review: &'a LaunchTeaserReviewState, status: &'a str) -> Element<'a, Message> {
    container(
        column![
            text(status).size(16),
            button("Cancel").on_press(Message::CloseTeaser),
        ]
        .spacing(8)
        .align_x(iced::Alignment::Center),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

fn completion_view<'a>(completed: &'a LaunchTeaserCompletedState) -> Element<'a, Message> {
    let duration_s = completed.duration_ms as f64 / 1000.0;

    container(
        column![
            text("Teaser Complete").size(20),
            text(format!("Duration: {duration_s:.1}s")).size(14),
            text(format!("{}×{}", completed.width, completed.height)).size(14),
            text(format!(
                "Output: {}",
                completed.output_path.file_name().unwrap_or_default().to_string_lossy()
            ))
            .size(14),
            row![
                button("Open").on_press(Message::TeaserOpenOutput),
                button("Show in Folder").on_press(Message::TeaserShowInFolder),
                button("Close").on_press(Message::CloseTeaser),
            ]
            .spacing(8),
        ]
        .spacing(8)
        .align_x(iced::Alignment::Center),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .padding(16)
    .into()
}

/// Create teaser entry button for the toolbar.
pub(crate) fn create_teaser_button<'a>(ws: &'a TimelineWorkspace) -> Element<'a, Message> {
    let eligibility = ws.launch_teaser_eligibility();
    match eligibility {
        LaunchTeaserEligibility::Eligible => {
            button("Create teaser").on_press(Message::CreateTeaser).into()
        }
        _ => {
            let reason = eligibility
                .disabled_reason()
                .unwrap_or("Cannot create teaser");
            button(text(format!("Create teaser: {reason}")).size(12))
                .into()
        }
    }
}
