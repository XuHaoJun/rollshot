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
    let apply = total - rejected;
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
        use rollshot_agent::runtime::RunEvent;
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
        assert!(event_to_activity_entry(&RunEvent::TurnComplete).is_none());
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
