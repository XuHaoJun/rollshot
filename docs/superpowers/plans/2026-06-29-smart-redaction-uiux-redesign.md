# Smart Redaction UI/UX Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the Smart Redaction workbench into a stable canvas-plus-agent-panel layout with one bottom candidate review bar and clear numbered confidence overlays.

**Architecture:** Keep the change inside the result workspace UI. Add small pure workbench state helpers for candidate display metadata, use standard iced widgets for the shell/panel/review bar, and keep custom drawing limited to the existing canvas proposal overlay.

**Tech Stack:** Rust, iced 0.14 standard widgets, iced canvas, existing `rollshot_edit_proposal` and `rollshot_image_document` models, `rtk cargo test`.

---

## File Structure

- Modify `crates/rollshot-app/src/result_workspace/workbench/state.rs`
  - Own pure candidate-review presentation helpers: low-confidence threshold, candidate numbering, display rows, and review summary.
  - Add focused tests for candidate numbering, rejected state, low-confidence state, and apply/reject/warning counts.

- Modify `crates/rollshot-app/src/result_workspace/canvas.rs`
  - Replace dashed white proposal overlays with solid confidence-colored overlays.
  - Use the candidate display helpers for sequence number, rejected filtering, selected state, and low-confidence threshold.
  - Add pure helper tests for overlay style selection and sequence lookup.

- Modify `crates/rollshot-app/src/result_workspace/view.rs`
  - Keep the existing result toolbar visible in Workbench mode.
  - Keep Normal mode behavior unchanged.

- Modify `crates/rollshot-app/src/result_workspace/workbench/view.rs`
  - Replace the collapsible left activity drawer and right candidate list/composer scroll with a fixed right Smart Redaction panel.
  - Move candidate rendering into the bottom review bar as chips.
  - Move status/error display into the Smart Redaction panel instead of stacked global banners.

No new files are required.

## Task 1: Add Candidate Presentation Helpers

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/state.rs`
- Test: `crates/rollshot-app/src/result_workspace/workbench/state.rs`

- [ ] **Step 1: Add failing tests for candidate display metadata**

Add these tests inside the existing `#[cfg(test)] mod tests` in `state.rs`:

```rust
#[test]
fn candidate_review_items_number_and_classify_candidates() {
    use rollshot_edit_proposal::{
        ConfidenceSummary, EditProposal, ProposalId, ProposedCandidate, ProposedEdit, Provenance,
        ProvenanceSource,
    };

    let proposal = EditProposal {
        id: ProposalId(1),
        base_document_state_id: 0,
        candidates: vec![
            ProposedCandidate {
                id: cid(10),
                edit: ProposedEdit::AddRedaction {
                    bounds: rect(0.0, 0.0),
                },
                confidence: 0.92,
                label: "url bar".into(),
                rationale: None,
                provenance: Provenance {
                    source: ProvenanceSource::Manual,
                },
            },
            ProposedCandidate {
                id: cid(20),
                edit: ProposedEdit::AddRedaction {
                    bounds: rect(10.0, 10.0),
                },
                confidence: 0.64,
                label: "name".into(),
                rationale: None,
                provenance: Provenance {
                    source: ProvenanceSource::Manual,
                },
            },
        ],
        confidence_summary: ConfidenceSummary::from_confidences(&[0.92, 0.64]),
        rationale_summary: None,
        provenance: Provenance {
            source: ProvenanceSource::Manual,
        },
    };
    let mut review = CandidateReview::from_candidates(&[cid(10), cid(20)]);
    review.mark_rejected(cid(20));

    let items = candidate_review_items(&proposal, &review, Some(cid(10)));

    assert_eq!(items.len(), 2);
    assert_eq!(items[0].sequence, 1);
    assert_eq!(items[0].id, cid(10));
    assert_eq!(items[0].label, "url bar");
    assert_eq!(items[0].confidence_percent, 92);
    assert!(!items[0].low_confidence);
    assert!(!items[0].rejected);
    assert!(items[0].selected);

    assert_eq!(items[1].sequence, 2);
    assert_eq!(items[1].id, cid(20));
    assert_eq!(items[1].confidence_percent, 64);
    assert!(items[1].low_confidence);
    assert!(items[1].rejected);
    assert!(!items[1].selected);
}

#[test]
fn candidate_review_summary_counts_apply_reject_and_warnings() {
    use rollshot_edit_proposal::{
        ConfidenceSummary, EditProposal, ProposalId, ProposedCandidate, ProposedEdit, Provenance,
        ProvenanceSource,
    };

    let proposal = EditProposal {
        id: ProposalId(1),
        base_document_state_id: 0,
        candidates: vec![
            ProposedCandidate {
                id: cid(1),
                edit: ProposedEdit::AddRedaction {
                    bounds: rect(0.0, 0.0),
                },
                confidence: 0.91,
                label: "email".into(),
                rationale: None,
                provenance: Provenance {
                    source: ProvenanceSource::Manual,
                },
            },
            ProposedCandidate {
                id: cid(2),
                edit: ProposedEdit::AddRedaction {
                    bounds: rect(10.0, 10.0),
                },
                confidence: 0.58,
                label: "account".into(),
                rationale: None,
                provenance: Provenance {
                    source: ProvenanceSource::Manual,
                },
            },
        ],
        confidence_summary: ConfidenceSummary::from_confidences(&[0.91, 0.58]),
        rationale_summary: None,
        provenance: Provenance {
            source: ProvenanceSource::Manual,
        },
    };
    let mut review = CandidateReview::from_candidates(&[cid(1), cid(2)]);
    review.mark_rejected(cid(2));

    let summary = candidate_review_summary(Some(&proposal), &review);

    assert_eq!(summary.total, 2);
    assert_eq!(summary.apply, 1);
    assert_eq!(summary.rejected, 1);
    assert_eq!(summary.warnings, 1);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::workbench::state::tests::candidate_review_items_number_and_classify_candidates
rtk cargo test -p rollshot-app result_workspace::workbench::state::tests::candidate_review_summary_counts_apply_reject_and_warnings
```

Expected: both fail because `candidate_review_items`, `candidate_review_summary`, and their return types do not exist yet.

- [ ] **Step 3: Add the presentation helpers**

Add this code in `state.rs` after `proposed_edit_bounds`:

```rust
pub const LOW_CONFIDENCE_THRESHOLD: f32 = 0.75;

#[derive(Debug, Clone, PartialEq)]
pub struct CandidateReviewItem {
    pub id: CandidateId,
    pub sequence: usize,
    pub label: String,
    pub confidence_percent: u8,
    pub low_confidence: bool,
    pub rejected: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CandidateReviewSummary {
    pub total: usize,
    pub apply: usize,
    pub rejected: usize,
    pub warnings: usize,
}

pub fn confidence_percent(confidence: f32) -> u8 {
    (confidence.clamp(0.0, 1.0) * 100.0).round() as u8
}

pub fn is_low_confidence(confidence: f32) -> bool {
    confidence < LOW_CONFIDENCE_THRESHOLD
}

pub fn is_candidate_rejected(review: &CandidateReview, id: CandidateId) -> bool {
    matches!(
        review.per_candidate.get(&id),
        Some(CandidateReviewState::Rejected)
    )
}

pub fn candidate_sequence(
    proposal: &EditProposal,
    id: CandidateId,
) -> Option<usize> {
    proposal
        .candidates
        .iter()
        .position(|candidate| candidate.id == id)
        .map(|index| index + 1)
}

pub fn candidate_review_items(
    proposal: &EditProposal,
    review: &CandidateReview,
    selected: Option<CandidateId>,
) -> Vec<CandidateReviewItem> {
    proposal
        .candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| CandidateReviewItem {
            id: candidate.id,
            sequence: index + 1,
            label: candidate.label.clone(),
            confidence_percent: confidence_percent(candidate.confidence),
            low_confidence: is_low_confidence(candidate.confidence),
            rejected: is_candidate_rejected(review, candidate.id),
            selected: selected == Some(candidate.id),
        })
        .collect()
}

pub fn candidate_review_summary(
    proposal: Option<&EditProposal>,
    review: &CandidateReview,
) -> CandidateReviewSummary {
    let Some(proposal) = proposal else {
        return CandidateReviewSummary::default();
    };
    let total = proposal.candidates.len();
    let rejected = review.rejected_count();
    CandidateReviewSummary {
        total,
        apply: total.saturating_sub(rejected),
        rejected,
        warnings: CandidateReview::warning_count(proposal, LOW_CONFIDENCE_THRESHOLD),
    }
}
```

Also update imports at the top of `state.rs`:

```rust
use rollshot_edit_proposal::{CandidateId, EditProposal, ProposedEdit};
```

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::workbench::state
```

Expected: all `result_workspace::workbench::state` tests pass.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/state.rs
rtk git commit -m "feat(smart-redaction): add candidate review presentation helpers"
```

## Task 2: Update Canvas Candidate Overlay Styling

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/canvas.rs`
- Test: `crates/rollshot-app/src/result_workspace/canvas.rs`

- [ ] **Step 1: Add failing tests for overlay style decisions**

Add this helper in the canvas test module:

```rust
fn assert_color_close(actual: iced::Color, expected: iced::Color) {
    assert!((actual.r - expected.r).abs() < 0.001);
    assert!((actual.g - expected.g).abs() < 0.001);
    assert!((actual.b - expected.b).abs() < 0.001);
    assert!((actual.a - expected.a).abs() < 0.001);
}
```

Add these tests in the same module:

```rust
#[test]
fn proposal_overlay_style_uses_green_for_high_confidence() {
    let style = proposal_overlay_style(0.92, false);

    assert_color_close(style.border, iced::Color::from_rgb(0.12, 0.55, 0.36));
    assert_color_close(style.fill, iced::Color::from_rgba(0.18, 0.75, 0.44, 0.18));
    assert_color_close(style.badge, iced::Color::from_rgb(0.12, 0.55, 0.36));
    assert_eq!(style.border_width, 2.0);
}

#[test]
fn proposal_overlay_style_uses_amber_for_low_confidence() {
    let style = proposal_overlay_style(0.64, false);

    assert_color_close(style.border, iced::Color::from_rgb(0.76, 0.49, 0.04));
    assert_color_close(style.fill, iced::Color::from_rgba(0.88, 0.64, 0.0, 0.20));
    assert_color_close(style.badge, iced::Color::from_rgb(0.76, 0.49, 0.04));
}

#[test]
fn proposal_overlay_style_uses_blue_for_selected_candidate() {
    let style = proposal_overlay_style(0.64, true);

    assert_color_close(style.border, iced::Color::from_rgb(0.13, 0.40, 1.0));
    assert_color_close(style.badge, iced::Color::from_rgb(0.13, 0.40, 1.0));
    assert_eq!(style.border_width, 2.5);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::canvas::tests::proposal_overlay_style_uses_green_for_high_confidence
rtk cargo test -p rollshot-app result_workspace::canvas::tests::proposal_overlay_style_uses_amber_for_low_confidence
rtk cargo test -p rollshot-app result_workspace::canvas::tests::proposal_overlay_style_uses_blue_for_selected_candidate
```

Expected: tests fail because `proposal_overlay_style` does not exist.

- [ ] **Step 3: Add overlay style helper**

Add this near the existing proposal overlay drawing helpers in `canvas.rs`, before `impl canvas::Program`:

```rust
#[derive(Debug, Clone, Copy)]
struct ProposalOverlayStyle {
    border: iced::Color,
    fill: iced::Color,
    badge: iced::Color,
    border_width: f32,
}

fn proposal_overlay_style(confidence: f32, selected: bool) -> ProposalOverlayStyle {
    if selected {
        return ProposalOverlayStyle {
            border: iced::Color::from_rgb(0.13, 0.40, 1.0),
            fill: iced::Color::from_rgba(0.13, 0.40, 1.0, 0.16),
            badge: iced::Color::from_rgb(0.13, 0.40, 1.0),
            border_width: 2.5,
        };
    }

    if super::workbench::state::is_low_confidence(confidence) {
        ProposalOverlayStyle {
            border: iced::Color::from_rgb(0.76, 0.49, 0.04),
            fill: iced::Color::from_rgba(0.88, 0.64, 0.0, 0.20),
            badge: iced::Color::from_rgb(0.76, 0.49, 0.04),
            border_width: 2.0,
        }
    } else {
        ProposalOverlayStyle {
            border: iced::Color::from_rgb(0.12, 0.55, 0.36),
            fill: iced::Color::from_rgba(0.18, 0.75, 0.44, 0.18),
            badge: iced::Color::from_rgb(0.12, 0.55, 0.36),
            border_width: 2.0,
        }
    }
}
```

- [ ] **Step 4: Replace dashed overlay drawing**

In the proposal overlay block inside `fn draw`, replace the dashed-border section with this solid fill/border/badge drawing:

```rust
let style = proposal_overlay_style(cand.confidence, is_selected);
let rect_path = canvas::Path::rectangle(
    iced::Point::new(rect.x, rect.y),
    iced::Size::new(rect.width, rect.height),
);
frame.fill(&rect_path, style.fill);
frame.stroke(
    &rect_path,
    canvas::Stroke::default()
        .with_color(style.border)
        .with_width(style.border_width),
);

if s > 0.3 {
    let sequence = super::workbench::state::candidate_sequence(proposal, cand.id).unwrap_or(0);
    let badge_center = iced::Point::new(rect.x, rect.y);
    let badge = canvas::Path::circle(badge_center, 11.0);
    frame.fill(&badge, style.badge);
    frame.fill_text(canvas::Text {
        content: sequence.to_string(),
        position: iced::Point::new(badge_center.x - 3.5, badge_center.y + 4.0),
        color: iced::Color::WHITE,
        size: iced::Pixels(11.0),
        ..canvas::Text::default()
    });
}
```

Keep the selected resize handles that already draw after the label block.

Update the comment above the block to:

```rust
// Smart Redaction candidate overlay. Rejected candidates are skipped.
// Visible candidates use confidence-colored solid borders/fills and numbered
// badges matching the review bar chips.
```

- [ ] **Step 5: Run canvas tests**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::canvas
```

Expected: all canvas tests pass.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/canvas.rs
rtk git commit -m "feat(smart-redaction): color candidate canvas overlays"
```

## Task 3: Keep The Result Toolbar In Workbench Mode

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/view.rs`

- [ ] **Step 1: Update `view` so toolbar is rendered in Workbench mode**

In `crates/rollshot-app/src/result_workspace/view.rs`, move the `toolbar(state)` call into each match arm so Workbench mode can render it too.

Replace the start of `pub(crate) fn view` through the `body` assignment with:

```rust
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
```

This intentionally keeps `disclosure`, `message_area`, and `status` out of the Workbench arm because Smart Redaction will own status/error placement in the panel and bottom review bar.

- [ ] **Step 2: Run a focused compile check**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::workbench::state
```

Expected: tests pass and the `view.rs` borrow changes compile.

- [ ] **Step 3: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/view.rs
rtk git commit -m "feat(smart-redaction): keep toolbar in workbench mode"
```

## Task 4: Rebuild The Workbench View Shell

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/view.rs`

- [ ] **Step 1: Update imports**

Replace the import block at the top of `workbench/view.rs` with:

```rust
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Length};

use super::super::{Message, ResultWorkspace};
use super::{WorkbenchMessage, WorkbenchState};
```

If these imports already match after earlier edits, leave them unchanged.

- [ ] **Step 2: Replace `workbench_view` with the stable shell**

Replace the current `workbench_view` body with:

```rust
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
```

- [ ] **Step 3: Add the fixed Smart Redaction panel**

Add this function below `workbench_view`:

```rust
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
```

- [ ] **Step 4: Remove the old left-drawer status function from the layout path**

Delete `run_status_row` if nothing calls it after Step 2. Keep `activity_column` and `activity_entry_view`; the new panel reuses them.

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::workbench::state
```

Expected: compile succeeds. If the compiler reports `run_status_row` is dead but allowed, deletion is still preferred because the old stacked status row is no longer part of the design.

- [ ] **Step 5: Replace `review_bar` with summary, chips, and action hierarchy**

Replace `review_bar` with:

```rust
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
        for item in super::state::candidate_review_items(
            proposal,
            &wb.review,
            wb.selected_candidate,
        ) {
            chips = chips.push(candidate_chip(item));
        }
    }

    let revise_enabled =
        wb.active_revision.is_some() && wb.pending_proposal.is_some() && wb.corrections_non_empty;

    let actions = row![
        button(text("Revise")).on_press_maybe(
            revise_enabled.then_some(Message::Workbench(WorkbenchMessage::AskAgentToRevise)),
        ),
        button(text(format!("Apply {} redactions", summary.apply)))
            .style(button::primary)
            .on_press_maybe(if summary.apply > 0 {
                Some(Message::Workbench(WorkbenchMessage::ApplyCandidates))
            } else {
                None
            }),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let mut bar = row![
        column![text(summary_text).size(13), chips].spacing(6).width(Length::Fill),
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
        .height(Length::Fixed(74.0))
        .into()
}
```

- [ ] **Step 6: Add candidate chip rendering**

Add this function below `review_bar`:

```rust
fn candidate_chip<'a>(item: super::state::CandidateReviewItem) -> Element<'a, Message> {
    let label = if item.rejected {
        format!("{} {} {}%", item.sequence, item.label, item.confidence_percent)
    } else if item.low_confidence {
        format!("{} ⚠ {} {}%", item.sequence, item.label, item.confidence_percent)
    } else {
        format!("{} {} {}%", item.sequence, item.label, item.confidence_percent)
    };

    let border = if item.selected {
        iced::Color::from_rgb(0.13, 0.40, 1.0)
    } else if item.rejected {
        iced::Color::from_rgb(0.82, 0.82, 0.84)
    } else if item.low_confidence {
        iced::Color::from_rgb(0.76, 0.49, 0.04)
    } else {
        iced::Color::from_rgb(0.12, 0.55, 0.36)
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
        .on_press(Message::Workbench(WorkbenchMessage::CandidateSelected(
            item.id,
        )))
        .into()
}
```

- [ ] **Step 7: Remove old candidate list usage**

Delete `candidate_list` if nothing calls it after the review bar rewrite. This removes the old right-pane list and ensures the composer is no longer inside a candidate-list scroll.

- [ ] **Step 8: Run focused workbench tests**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::workbench
```

Expected: all workbench tests pass.

- [ ] **Step 9: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/view.rs
rtk git commit -m "feat(smart-redaction): rebuild workbench review layout"
```

## Task 5: Full Verification And Polish

**Files:**
- Modify only files required by compiler or formatting feedback from previous tasks.

- [ ] **Step 1: Run formatting check**

Run:

```bash
rtk cargo fmt --check
```

Expected: pass. If it fails, run `rtk cargo fmt`, then repeat `rtk cargo fmt --check`.

- [ ] **Step 2: Run focused tests**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::workbench
rtk cargo test -p rollshot-app result_workspace::canvas
```

Expected: both commands pass.

- [ ] **Step 3: Run broader app tests because `result_workspace/view.rs` changed**

Run:

```bash
rtk cargo test -p rollshot-app
```

Expected: all `rollshot-app` tests pass.

- [ ] **Step 4: Manual UI check**

Run:

```bash
rtk cargo run -p rollshot-app
```

Expected: the Rollshot app opens. If the command cannot open a GUI in the
current environment, record that manual runtime verification was not completed
and keep the automated verification results from Steps 1-3.

With the app open, verify:

```text
1. Enter Smart Redaction mode: toolbar remains visible.
2. Before a run starts: the right Smart Redaction panel is visible and canvas width is stable.
3. During a run: activity streams in the right panel and composer is disabled but pinned.
4. After a proposal: bottom review bar shows summary, candidate chips, Revise, Apply, and Next warning when applicable.
5. Canvas overlays are green or amber with numbered badges matching chips.
6. Selecting a chip highlights the matching overlay.
7. Rejecting a candidate removes it from active canvas overlays and updates the summary.
8. Apply is the only primary action and clears pending candidates after success.
```

- [ ] **Step 5: Commit any final fixes**

If formatting or verification required edits, commit them:

```bash
rtk git add crates/rollshot-app/src/result_workspace
rtk git commit -m "fix(smart-redaction): polish redesigned workbench UI"
```

If no files changed after Step 3, skip this commit.
