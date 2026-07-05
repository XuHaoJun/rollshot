#![allow(dead_code)] // SP6 scaffolding: all types used by later tasks

use rollshot_agent::driver::RunTerminalState;
use rollshot_agent::runtime::{BudgetDimension, RunCancellation, RunEvent};
use rollshot_edit_proposal::{CandidateId, EditProposal, ProposedEdit};
use rollshot_image_document::ImageRect;

/// Where the workbench's run is in its lifecycle.
#[derive(Debug, Clone, Default)]
pub enum RunState {
    #[default]
    Idle,
    Running {
        cancellation: RunCancellation,
        parent_revision_id: Option<rollshot_preset::RevisionId>,
        revision_note: Option<String>,
    },
    Terminal(RunTerminalState),
}

/// Per-candidate review state.
#[derive(Debug, Clone, PartialEq)]
pub enum CandidateReviewState {
    /// Will apply by default.
    Pending,
    /// Explicit user confirm (optional; not required for normal flow).
    Accepted,
    /// Will not apply.
    Rejected,
    /// Will apply with this replacement edit instead of the original.
    Modified(ProposedEdit),
}

/// Per-candidate review map.
#[derive(Debug, Clone, Default)]
pub struct CandidateReview {
    pub per_candidate: std::collections::BTreeMap<CandidateId, CandidateReviewState>,
}

/// Activity entries reconstructed from the RunEvent stream for the drawer.
#[derive(Debug, Clone)]
pub enum ActivityEntry {
    UserMessage(String),
    AssistantText(String),
    ToolCard {
        name: String,
        status: ToolCardStatus,
        summary: String,
    },
    SourceDiff {
        tool: String,
        lines: Vec<String>,
    },
    RunStatus {
        turn: u32,
        budget_summary: String,
        elapsed: std::time::Duration,
    },
    TerminalLabel(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCardStatus {
    Running,
    Success,
    Failed,
}

/// Workbench error model (spec §9.1). Maps each failure to the correct UI
/// retry action. Never stringly-typed at this layer.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkbenchError {
    ProviderFailure {
        message: String,
    },
    SourceValidationFailure,
    RuntimeFailure,
    AgentProtocolFailure {
        message: String,
    },
    BudgetExhausted {
        dimension: BudgetDimension,
    },
    VisionPrepare {
        message: String,
    },
    Store {
        message: String,
    },
    Config,
    CapabilityUnavailable {
        message: String,
    },
    /// `RunTerminalState::Cancelled` is not an error — return to Idle. This
    /// variant is kept for completeness but is never shown as an error.
    Cancelled,
}

impl std::fmt::Display for WorkbenchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProviderFailure { message } => write!(f, "Provider error: {message}"),
            Self::SourceValidationFailure => write!(f, "Automation validation failed"),
            Self::RuntimeFailure => write!(f, "Runtime error"),
            Self::AgentProtocolFailure { message } => write!(f, "Agent error: {message}"),
            Self::BudgetExhausted { dimension } => write!(f, "Budget exhausted: {dimension:?}"),
            Self::VisionPrepare { message } => write!(f, "Vision prepare: {message}"),
            Self::Store { message } => write!(f, "Preset store: {message}"),
            Self::Config => write!(f, "Provider not configured"),
            Self::CapabilityUnavailable { message } => {
                write!(f, "Capability unavailable: {message}")
            }
            Self::Cancelled => write!(f, "Cancelled"),
        }
    }
}

impl WorkbenchError {
    /// Map a terminal state to the workbench error it represents (if any).
    /// `ReadyForReview` / `NeedsUserInput` / `Cancelled` are not errors.
    pub fn from_terminal(terminal: &RunTerminalState) -> Option<Self> {
        match terminal {
            RunTerminalState::ProviderFailure { message } => Some(Self::ProviderFailure {
                message: message.clone(),
            }),
            RunTerminalState::SourceValidationFailure => Some(Self::SourceValidationFailure),
            RunTerminalState::RuntimeFailure => Some(Self::RuntimeFailure),
            RunTerminalState::AgentProtocolFailure { message } => {
                Some(Self::AgentProtocolFailure {
                    message: message.clone(),
                })
            }
            RunTerminalState::BudgetExhausted { dimension } => Some(Self::BudgetExhausted {
                dimension: *dimension,
            }),
            _ => None,
        }
    }
}

/// Free helper — cannot `impl ProposedEdit` in rollshot-app (orphan rule).
/// Returns the image-space rect a candidate edit targets, if any.
pub fn proposed_edit_bounds(edit: &ProposedEdit) -> Option<ImageRect> {
    match edit {
        ProposedEdit::AddRedaction { bounds } => Some(*bounds),
        ProposedEdit::UpdateRedactionBounds { bounds, .. } => Some(*bounds),
        _ => None,
    }
}

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

impl CandidateReview {
    pub fn from_candidates(candidates: &[CandidateId]) -> Self {
        Self {
            per_candidate: candidates
                .iter()
                .map(|c| (*c, CandidateReviewState::Pending))
                .collect(),
        }
    }
    pub fn mark_rejected(&mut self, id: CandidateId) {
        self.per_candidate
            .insert(id, CandidateReviewState::Rejected);
    }
    pub fn mark_modified(&mut self, id: CandidateId, edit: ProposedEdit) {
        self.per_candidate
            .insert(id, CandidateReviewState::Modified(edit));
    }
    pub fn mark_pending(&mut self, id: CandidateId) {
        self.per_candidate.insert(id, CandidateReviewState::Pending);
    }
    pub fn mark_accepted(&mut self, id: CandidateId) {
        self.per_candidate
            .insert(id, CandidateReviewState::Accepted);
    }
    /// (apply_ids, reject_ids, modified_pairs).
    /// apply = Pending + Accepted + Modified; reject = Rejected.
    pub fn decision_sets(
        &self,
    ) -> (
        Vec<CandidateId>,
        Vec<CandidateId>,
        Vec<(CandidateId, ProposedEdit)>,
    ) {
        let mut apply = Vec::new();
        let mut reject = Vec::new();
        let mut modified = Vec::new();
        for (id, state) in &self.per_candidate {
            match state {
                CandidateReviewState::Pending | CandidateReviewState::Accepted => apply.push(*id),
                CandidateReviewState::Rejected => reject.push(*id),
                CandidateReviewState::Modified(edit) => {
                    apply.push(*id);
                    modified.push((*id, edit.clone()));
                }
            }
        }
        (apply, reject, modified)
    }
    pub fn is_empty(&self) -> bool {
        self.per_candidate.is_empty()
    }
    pub fn pending_count(&self) -> usize {
        self.per_candidate
            .values()
            .filter(|s| matches!(s, CandidateReviewState::Pending))
            .count()
    }
    pub fn rejected_count(&self) -> usize {
        self.per_candidate
            .values()
            .filter(|s| matches!(s, CandidateReviewState::Rejected))
            .count()
    }
    pub fn modified_count(&self) -> usize {
        self.per_candidate
            .values()
            .filter(|s| matches!(s, CandidateReviewState::Modified(_)))
            .count()
    }
    /// Count candidates below `threshold` confidence (spec §5.5 warnings).
    pub fn warning_count(proposal: &EditProposal, threshold: f32) -> usize {
        proposal
            .candidates
            .iter()
            .filter(|c| c.confidence < threshold)
            .count()
    }
}

impl RunState {
    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }
}

/// Whether pending (unapplied) candidates exist. Copy/Save must warn or block.
pub fn has_pending_candidates(wb: &super::WorkbenchState) -> bool {
    wb.pending_proposal.is_some() && !wb.review.is_empty()
}

/// Apply/skip summary for the review bar and the Copy/Save warning.
pub fn apply_skip_summary(wb: &super::WorkbenchState) -> String {
    let total = wb
        .pending_proposal
        .as_ref()
        .map_or(0, |p| p.candidates.len());
    let rejected = wb.review.rejected_count();
    let apply = total.saturating_sub(rejected);
    let warnings = wb
        .pending_proposal
        .as_ref()
        .map_or(0, |p| CandidateReview::warning_count(p, 0.75));
    format!("Apply {apply} redactions, skip {rejected} rejected\n{warnings} warnings included")
}

/// Map a RunEvent to an ActivityEntry for the live drawer. `TurnComplete` is
/// never emitted by the driver (§10.8) so it maps to `None`.
pub fn event_to_activity_entry(event: &RunEvent) -> Option<ActivityEntry> {
    match event {
        RunEvent::TextChunk { text } => Some(ActivityEntry::AssistantText(text.clone())),
        RunEvent::ToolCallStart { name } => Some(ActivityEntry::ToolCard {
            name: name.clone(),
            status: ToolCardStatus::Running,
            summary: String::new(),
        }),
        RunEvent::ToolCallEnd { name, success } => Some(ActivityEntry::ToolCard {
            name: name.clone(),
            status: if *success {
                ToolCardStatus::Success
            } else {
                ToolCardStatus::Failed
            },
            summary: String::new(),
        }),
        RunEvent::SourceChanged { tool, diff } => {
            let mut lines = Vec::with_capacity(diff.lines.len().saturating_add(1));
            lines.push(format!(
                "generation {} -> {}",
                diff.old_generation, diff.new_generation
            ));
            for line in &diff.lines {
                let marker = match line.kind {
                    rollshot_agent::runtime::SourceDiffLineKind::Context => " ",
                    rollshot_agent::runtime::SourceDiffLineKind::Removed => "-",
                    rollshot_agent::runtime::SourceDiffLineKind::Added => "+",
                    rollshot_agent::runtime::SourceDiffLineKind::Omitted => ".",
                };
                lines.push(format!("{marker} {}", line.text));
            }
            Some(ActivityEntry::SourceDiff {
                tool: tool.clone(),
                lines,
            })
        }
        RunEvent::TurnComplete => None,
    }
}

/// Human-readable label for a terminal state (spec §6.3).
pub fn terminal_state_label(state: &RunTerminalState) -> String {
    use rollshot_agent::driver::RunTerminalState::*;
    match state {
        ReadyForReview(_) => "Ready for review".into(),
        NeedsUserInput(_) => "Needs your input".into(),
        Cancelled => "Run cancelled".into(),
        BudgetExhausted { dimension } => format!("Budget exhausted: {dimension:?}"),
        ProviderFailure { message } => format!("Provider error: {message}"),
        SourceValidationFailure => "Validation failed".into(),
        RuntimeFailure => "Runtime error".into(),
        AgentProtocolFailure { message } => format!("Agent error: {message}"),
    }
}

#[cfg(test)]
pub(crate) fn workbench_with_pending_candidate() -> super::WorkbenchState {
    use rollshot_edit_proposal::{
        ConfidenceSummary, ProposalId, ProposedCandidate, Provenance, ProvenanceSource,
    };

    let id = CandidateId(1);
    let proposal = EditProposal {
        id: ProposalId(1),
        base_document_state_id: 0,
        candidates: vec![ProposedCandidate {
            id,
            edit: ProposedEdit::AddRedaction {
                bounds: ImageRect {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
            },
            confidence: 0.9,
            label: "pending redaction".into(),
            rationale: None,
            provenance: Provenance {
                source: ProvenanceSource::Manual,
            },
        }],
        confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
        rationale_summary: None,
        provenance: Provenance {
            source: ProvenanceSource::Manual,
        },
    };

    super::WorkbenchState {
        pending_proposal: Some(proposal),
        review: CandidateReview::from_candidates(&[id]),
        ..super::WorkbenchState::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_edit_proposal::{CandidateId, ProposedEdit};
    use rollshot_image_document::ImageRect;

    fn cid(n: u64) -> CandidateId {
        CandidateId(n)
    }
    fn rect(x: f32, y: f32) -> ImageRect {
        ImageRect {
            x,
            y,
            width: 50.0,
            height: 50.0,
        }
    }

    #[test]
    fn from_candidates_marks_all_pending() {
        let review = CandidateReview::from_candidates(&[cid(1), cid(2), cid(3)]);
        assert_eq!(review.per_candidate.len(), 3);
        assert_eq!(review.pending_count(), 3);
        assert_eq!(review.rejected_count(), 0);
    }

    #[test]
    fn reject_then_undo_returns_to_pending() {
        let mut r = CandidateReview::from_candidates(&[cid(1)]);
        r.mark_rejected(cid(1));
        assert_eq!(r.rejected_count(), 1);
        r.mark_pending(cid(1));
        assert_eq!(r.per_candidate[&cid(1)], CandidateReviewState::Pending);
    }

    #[test]
    fn modify_replaces_edit() {
        let mut r = CandidateReview::from_candidates(&[cid(1)]);
        r.mark_modified(
            cid(1),
            ProposedEdit::AddRedaction {
                bounds: rect(10.0, 20.0),
            },
        );
        match &r.per_candidate[&cid(1)] {
            CandidateReviewState::Modified(ProposedEdit::AddRedaction { bounds }) => {
                assert_eq!(bounds.x, 10.0);
                assert_eq!(bounds.y, 20.0);
            }
            _ => panic!("expected Modified(AddRedaction)"),
        }
    }

    #[test]
    fn decision_sets_partition_correctly() {
        let mut r = CandidateReview::from_candidates(&[cid(1), cid(2), cid(3)]);
        r.mark_rejected(cid(2));
        r.mark_modified(
            cid(3),
            ProposedEdit::AddRedaction {
                bounds: rect(0.0, 0.0),
            },
        );
        let (apply, reject, modified) = r.decision_sets();
        assert!(apply.contains(&cid(1)) && apply.contains(&cid(3)));
        assert_eq!(reject, vec![cid(2)]);
        assert_eq!(modified.len(), 1);
        assert_eq!(modified[0].0, cid(3));
    }

    #[test]
    fn warning_count_counts_low_confidence() {
        use rollshot_edit_proposal::{
            ConfidenceSummary, EditProposal, ProposalId, ProposedCandidate, Provenance,
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
                    confidence: 0.9,
                    label: "a".into(),
                    rationale: None,
                    provenance: Provenance {
                        source: ProvenanceSource::Manual,
                    },
                },
                ProposedCandidate {
                    id: cid(2),
                    edit: ProposedEdit::AddRedaction {
                        bounds: rect(0.0, 0.0),
                    },
                    confidence: 0.5,
                    label: "b".into(),
                    rationale: None,
                    provenance: Provenance {
                        source: ProvenanceSource::Manual,
                    },
                },
            ],
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9, 0.5]),
            rationale_summary: None,
            provenance: Provenance {
                source: ProvenanceSource::Manual,
            },
        };
        assert_eq!(CandidateReview::warning_count(&proposal, 0.75), 1);
    }

    #[test]
    fn event_to_activity_entry_maps_each_variant() {
        use rollshot_agent::runtime::{
            RunEvent, SourceDiffLine, SourceDiffLineKind, SourceDiffSummary,
        };
        let e = event_to_activity_entry(&RunEvent::TextChunk { text: "hi".into() });
        assert!(matches!(e, Some(ActivityEntry::AssistantText(t)) if t == "hi"));
        let e = event_to_activity_entry(&RunEvent::ToolCallStart {
            name: "dry_run".into(),
        });
        assert!(matches!(
            e,
            Some(ActivityEntry::ToolCard {
                status: ToolCardStatus::Running,
                ..
            })
        ));
        let e = event_to_activity_entry(&RunEvent::ToolCallEnd {
            name: "dry_run".into(),
            success: false,
        });
        assert!(matches!(
            e,
            Some(ActivityEntry::ToolCard {
                status: ToolCardStatus::Failed,
                ..
            })
        ));
        let e = event_to_activity_entry(&RunEvent::SourceChanged {
            tool: "edit_source".into(),
            diff: SourceDiffSummary {
                old_generation: 0,
                new_generation: 1,
                old_source_bytes: 3,
                new_source_bytes: 3,
                omitted_lines: 0,
                lines: vec![SourceDiffLine {
                    kind: SourceDiffLineKind::Added,
                    text: "new".into(),
                }],
            },
        });
        assert!(matches!(
            e,
            Some(ActivityEntry::SourceDiff { tool, lines })
                if tool == "edit_source" && lines.iter().any(|line| line == "+ new")
        ));
        assert!(event_to_activity_entry(&RunEvent::TurnComplete).is_none());
    }

    #[test]
    fn candidate_review_items_number_and_classify_candidates() {
        use rollshot_edit_proposal::{
            ConfidenceSummary, EditProposal, ProposalId, ProposedCandidate, ProposedEdit,
            Provenance, ProvenanceSource,
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
            ConfidenceSummary, EditProposal, ProposalId, ProposedCandidate, ProposedEdit,
            Provenance, ProvenanceSource,
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

    #[test]
    fn terminal_label_covers_all_variants() {
        use rollshot_agent::driver::RunTerminalState::*;
        assert_eq!(terminal_state_label(&Cancelled), "Run cancelled");
        assert_eq!(terminal_state_label(&RuntimeFailure), "Runtime error");
        assert_eq!(
            terminal_state_label(&BudgetExhausted {
                dimension: BudgetDimension::WallTime
            }),
            "Budget exhausted: WallTime"
        );
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod gating_tests {
    use super::*;
    use crate::result_workspace::workbench::WorkbenchState;
    use rollshot_edit_proposal::{
        CandidateId, ConfidenceSummary, EditProposal, ProposalId, ProposedCandidate, ProposedEdit,
        Provenance, ProvenanceSource,
    };
    use rollshot_image_document::ImageRect;

    fn proposal(n: usize) -> EditProposal {
        let cands = (0..n)
            .map(|i| ProposedCandidate {
                id: CandidateId(i as u64),
                edit: ProposedEdit::AddRedaction {
                    bounds: ImageRect {
                        x: 0.0,
                        y: 0.0,
                        width: 10.0,
                        height: 10.0,
                    },
                },
                confidence: 0.9,
                label: "t".into(),
                rationale: None,
                provenance: Provenance {
                    source: ProvenanceSource::Manual,
                },
            })
            .collect();
        EditProposal {
            id: ProposalId(1),
            base_document_state_id: 0,
            candidates: cands,
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
            rationale_summary: None,
            provenance: Provenance {
                source: ProvenanceSource::Manual,
            },
        }
    }

    #[test]
    fn has_pending_candidates_false_when_no_proposal() {
        let wb = WorkbenchState::default();
        assert!(!has_pending_candidates(&wb));
    }

    #[test]
    fn has_pending_candidates_true_with_proposal_and_review() {
        let mut wb = WorkbenchState::default();
        wb.pending_proposal = Some(proposal(2));
        wb.review = CandidateReview::from_candidates(&[CandidateId(0), CandidateId(1)]);
        assert!(has_pending_candidates(&wb));
    }

    #[test]
    fn apply_skip_summary_format() {
        let mut wb = WorkbenchState::default();
        wb.pending_proposal = Some(proposal(3));
        wb.review =
            CandidateReview::from_candidates(&[CandidateId(0), CandidateId(1), CandidateId(2)]);
        wb.review.mark_rejected(CandidateId(1));
        let s = apply_skip_summary(&wb);
        assert!(s.contains("Apply 2 redactions"));
        assert!(s.contains("skip 1 rejected"));
    }
}
