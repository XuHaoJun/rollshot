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

## NOT in scope (deferred, with rationale)

- Real agent/provider run wiring — pre-existing SP6 infrastructure; this plan is
  presentation-only.
- Drag-to-reposition candidates from chips (`CandidateMoved`) — not in the
  critique; the existing canvas resize/move gesture is unchanged.
- Keyboard navigation of chips/warnings — accessibility follow-up.
- The inline "preview-only — apply before safe copy/save" reminder is
  intentionally dropped (one of the redundant banners the critique targets). The
  copy/save safety guard (`has_pending_candidates` + the discard/unredacted
  modals in the parent `view.rs`) still enforces apply-before-share, so no safety
  is lost.
- Distribution/CI: N/A — no new artifact; existing `rollshot-app` only.

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

    // cid(1): high-confidence, will apply. cid(2): low-confidence AND rejected
    // — must NOT count as a warning (it will not apply). cid(3): low-confidence
    // and still pending — the only will-apply warning.
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
            ProposedCandidate {
                id: cid(3),
                edit: ProposedEdit::AddRedaction {
                    bounds: rect(20.0, 20.0),
                },
                confidence: 0.60,
                label: "phone".into(),
                rationale: None,
                provenance: Provenance {
                    source: ProvenanceSource::Manual,
                },
            },
        ],
        confidence_summary: ConfidenceSummary::from_confidences(&[0.91, 0.58, 0.60]),
        rationale_summary: None,
        provenance: Provenance {
            source: ProvenanceSource::Manual,
        },
    };
    let mut review = CandidateReview::from_candidates(&[cid(1), cid(2), cid(3)]);
    review.mark_rejected(cid(2));

    let summary = candidate_review_summary(Some(&proposal), &review);

    assert_eq!(summary.total, 3);
    assert_eq!(summary.apply, 2);
    assert_eq!(summary.rejected, 1);
    // Only cid(3): low-confidence and still pending. cid(2) is low-confidence
    // but rejected, so it is excluded from the will-apply warning count.
    assert_eq!(summary.warnings, 1);
}

#[test]
fn confidence_accent_is_shared_by_overlays_and_chips() {
    // Single source of truth so canvas badges and review-bar chips never drift
    // (critique requirement: chips numbered/colored to match the canvas boxes).
    assert_eq!(confidence_accent(false, false), (0.12, 0.55, 0.36)); // green
    assert_eq!(confidence_accent(true, false), (0.76, 0.49, 0.04)); // amber
    assert_eq!(confidence_accent(true, true), (0.13, 0.40, 1.0)); // selected → blue
    assert_eq!(confidence_accent(false, true), (0.13, 0.40, 1.0)); // selected wins
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::workbench::state::tests::candidate_review_items_number_and_classify_candidates
rtk cargo test -p rollshot-app result_workspace::workbench::state::tests::candidate_review_summary_counts_apply_reject_and_warnings
rtk cargo test -p rollshot-app result_workspace::workbench::state::tests::confidence_accent_is_shared_by_overlays_and_chips
```

Expected: all three fail because `candidate_review_items`, `candidate_review_summary`, `confidence_accent`, and their return types do not exist yet.

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

/// Shared accent (border/badge) color for confidence overlays AND review-bar
/// chips, so the canvas boxes and the bottom chips can never drift apart
/// (critique requirement: chips numbered/colored to match the canvas boxes).
/// RGB only — no iced dependency in this module; call sites wrap in
/// `iced::Color::from_rgb`. `selected` (blue) wins over confidence; otherwise
/// amber when low-confidence, else green. The rejected-grey override is a
/// per-surface concern and stays in the chip.
pub fn confidence_accent(low_confidence: bool, selected: bool) -> (f32, f32, f32) {
    if selected {
        (0.13, 0.40, 1.0)
    } else if low_confidence {
        (0.76, 0.49, 0.04)
    } else {
        (0.12, 0.55, 0.36)
    }
}

pub fn is_candidate_rejected(review: &CandidateReview, id: CandidateId) -> bool {
    matches!(
        review.per_candidate.get(&id),
        Some(CandidateReviewState::Rejected)
    )
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
    // Warnings count low-confidence candidates that WILL apply — a rejected
    // low-confidence candidate is already handled and must not inflate the
    // count or be a "Next warning" jump target. (Note: `CandidateReview::
    // warning_count` counts all sub-threshold candidates and is intentionally
    // left unchanged — it is still used by `apply_skip_summary` and the
    // empty/all-low-confidence result-state messaging.)
    let warnings = proposal
        .candidates
        .iter()
        .filter(|c| is_low_confidence(c.confidence) && !is_candidate_rejected(review, c.id))
        .count();
    CandidateReviewSummary {
        total,
        apply: total.saturating_sub(rejected),
        rejected,
        warnings,
    }
}
```

Imports: `state.rs` already has
`use rollshot_edit_proposal::{CandidateId, EditProposal, ProposedEdit};` at the
top (line 5) and the `cid(..)` / `rect(..)` test helpers already exist in
`mod tests`. No import change is needed unless the compiler reports a missing
symbol — leave the existing line as-is.

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
    let low = super::workbench::state::is_low_confidence(confidence);
    // Border + badge come from the shared `confidence_accent` helper so they
    // always match the review-bar chips. The translucent fill is overlay-only.
    let (r, g, b) = super::workbench::state::confidence_accent(low, selected);
    let accent = iced::Color::from_rgb(r, g, b);
    let (fill, border_width) = if selected {
        (iced::Color::from_rgba(0.13, 0.40, 1.0, 0.16), 2.5)
    } else if low {
        (iced::Color::from_rgba(0.88, 0.64, 0.0, 0.20), 2.0)
    } else {
        (iced::Color::from_rgba(0.18, 0.75, 0.44, 0.18), 2.0)
    };
    ProposalOverlayStyle {
        border: accent,
        fill,
        badge: accent,
        border_width,
    }
}
```

- [ ] **Step 4: Replace dashed overlay drawing with solid confidence overlays**

The proposal-overlay block inside `fn draw` currently iterates
`for cand in &proposal.candidates`, then for each candidate: gets `bounds` via
`proposed_edit_bounds` (cull-`continue` if `None`), `continue`s if outside
`self.visible`, `continue`s if `is_rejected`, binds `is_selected`, computes the
scaled `let rect = iced::Rectangle { … }`, draws a dashed border + plain label,
then draws selected resize handles.

Make two changes. **Keep** the `proposed_edit_bounds`/`intersects` cull, the
`is_rejected` skip, the `is_selected` binding, the `let rect = …` computation,
and the trailing `if is_selected { … }` resize-handle block exactly as they are.

1. Change the loop header to carry the 1-based index (the badge number must
   match the chip sequence, which is the candidate's position in
   `proposal.candidates`):

```rust
for (index, cand) in proposal.candidates.iter().enumerate() {
```

2. Replace only the lines from `let border_color = …` through the
   `draw_dashed_rect(&mut frame, rect, 6.0, 4.0, stroke);` call and the old
   `if s > 0.3 { let label = … }` label block with this solid
   fill/border/numbered-badge drawing (the surrounding kept lines above and
   below are unchanged):

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
    let sequence = index + 1;
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

Update the comment above the block to:

```rust
// Smart Redaction candidate overlay. Rejected candidates are skipped.
// Visible candidates use confidence-colored solid borders/fills and numbered
// badges (1-based position) matching the review bar chips.
```

Then remove the now-unused dashed-overlay helpers this change orphans: delete the
`draw_dashed_rect` fn, the `point_on_rect_perimeter` fn, and the
`point_on_rect_perimeter_corners_and_edge_midpoints` test. A repo grep confirms
they are referenced nowhere else (the perimeter test only exercised the dashed
path). If the executor cannot confirm zero remaining references, leave them in
place (the module allows dead code) and note the leftover, rather than risk a
broken build.

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

This already matches the current top of `workbench/view.rs` verbatim — it is a
no-op confirmation step; leave the block unchanged. `scrollable` is already
imported (used by the chips row in Step 5).

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
    // Empty-result and all-low-confidence recovery (Improve preset / Manual
    // redact / Discard / Review warnings) live in the panel now, not as a
    // stacked global banner. `result_state_banner` returns `None` for a normal
    // proposal, so the panel only shows it when recovery is actually needed.
    // This is the sole UI producer of `ImStart` and the empty-state Manual
    // redact / `DiscardCandidates` actions — it must NOT be deleted.
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
        for item in
            super::state::candidate_review_items(proposal, &wb.review, wb.selected_candidate)
        {
            chips = chips.push(candidate_chip(item));
        }
    }
    // Chips scroll horizontally so a busy screenshot's many candidates stay
    // reachable without changing the fixed review-bar height (iced 0.14 API,
    // mirrors timeline_workspace/view.rs).
    let chips = scrollable(chips)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new(),
        ))
        .width(Length::Fill);

    let revise_enabled =
        wb.active_revision.is_some() && wb.pending_proposal.is_some() && wb.corrections_non_empty;

    // Contextual reject/undo for the selected candidate. The chip is
    // select-only, so this is the sole UI producer of CandidateDeleted /
    // CandidateUnrejected — without it there is no way to reject a candidate.
    let selected_reject = wb.selected_candidate.map(|id| {
        if super::state::is_candidate_rejected(&wb.review, id) {
            button(text("Undo reject"))
                .on_press(Message::Workbench(WorkbenchMessage::CandidateUnrejected(id)))
        } else {
            button(text("Reject"))
                .on_press(Message::Workbench(WorkbenchMessage::CandidateDeleted(id)))
        }
    });

    // Secondary/contextual actions first, primary Apply last (action hierarchy).
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

    // 88px (not 74) to leave room for the horizontal chip scrollbar under the
    // summary line; confirm no clipping in the Step-4 manual UI check.
    container(bar)
        .padding(10)
        .width(Length::Fill)
        .height(Length::Fixed(88.0))
        .into()
}
```

- [ ] **Step 6: Add candidate chip rendering**

Add this function below `review_bar`:

```rust
fn candidate_chip<'a>(item: super::state::CandidateReviewItem) -> Element<'a, Message> {
    // ⚠ only for a low-confidence candidate that will still apply; a rejected
    // chip is greyed and needs no warning glyph.
    let label = if item.low_confidence && !item.rejected {
        format!("{} ⚠ {} {}%", item.sequence, item.label, item.confidence_percent)
    } else {
        format!("{} {} {}%", item.sequence, item.label, item.confidence_percent)
    };

    // Rejected grey overrides the accent; otherwise the shared `confidence_accent`
    // helper keeps the chip border identical to the canvas overlay border.
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

    // Transparent button so only the chip's own pill chrome shows — without
    // this, iced's default button background/border draws around the chip.
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
```

- [ ] **Step 7: Remove old candidate list usage**

Delete `candidate_list` if nothing calls it after the review bar rewrite. This removes the old right-pane list and ensures the composer is no longer inside a candidate-list scroll.

Keep these — they are all still referenced by the new layout and must NOT be
deleted: `result_state_banner` (now rendered inside `smart_redaction_panel`,
Step 3 — it is the only UI path to `ImStart` / empty-state Manual redact /
`DiscardCandidates` recovery), `error_message_banner`, `activity_column`,
`activity_entry_view`, `composer`, `disclosure_modal`, and `improve_modal`.

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
3. During a run: activity streams in the right panel and the composer is disabled but pinned to the bottom of the panel.
4. After a proposal: the bottom review bar shows the summary line, candidate chips, Revise, and the primary Apply.
5. Canvas overlays are green or amber with numbered badges matching the chip numbers; the selected one is blue. Chips have no extra button chrome around the pill.
6. Selecting a chip highlights the matching overlay and a contextual "Reject" button appears in the review bar.
7. Reject the selected candidate: it drops from active canvas overlays, the chip greys out, and the summary apply/rejected counts update. The button flips to "Undo reject" and restores it.
8. "Discard all" clears the pending proposal entirely.
9. Empty result (preset finds nothing): the panel shows the "didn't find anything" recovery with Improve preset and Manual redact.
10. All-low-confidence result: the panel shows the Review warnings / Improve preset / Discard recovery.
11. ⚠ shows only on a pending low-confidence chip/overlay; a rejected low-confidence chip shows no ⚠, and the "{n} low confidence" summary counts only will-apply candidates.
12. With many candidates (>8): the chips row scrolls horizontally; none are clipped or unreachable, and the review-bar height stays fixed.
13. "Next warning" appears only when at least one will-apply low-confidence candidate exists.
14. Apply is the only primary (filled) action and clears pending candidates after success.
```

- [ ] **Step 5: Commit any final fixes**

If formatting or verification required edits, commit them:

```bash
rtk git add crates/rollshot-app/src/result_workspace
rtk git commit -m "fix(smart-redaction): polish redesigned workbench UI"
```

If no files changed after Step 3, skip this commit.
