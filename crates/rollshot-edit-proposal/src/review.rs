//! Review outcome and lowering of an accepted decision to document ops.

use rollshot_image_document::EditOp;
use serde::{Deserialize, Serialize};

use crate::proposal::{CandidateId, EditProposal, ProposalId, ProposedEdit};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewDecision {
    pub proposal_id: ProposalId,
    pub accepted: Vec<CandidateId>,
    pub rejected: Vec<CandidateId>,
    /// Candidates the user edited before applying (final edit wins over the original).
    pub modified: Vec<(CandidateId, ProposedEdit)>,
    /// `ImageDocument::state_id()` after the lowered batch is applied.
    pub resulting_document_state_id: u64,
}

/// Lower an accepted decision to the document ops to hand to
/// `ImageDocument::apply_batch`. For each accepted candidate (in the proposal's
/// candidate order), use its modified edit if present, else its original; drop
/// rejected and non-accepted candidates.
pub fn lower(proposal: &EditProposal, decision: &ReviewDecision) -> Vec<EditOp> {
    proposal
        .candidates
        .iter()
        .filter(|c| decision.accepted.contains(&c.id))
        .map(|c| {
            let edit = decision
                .modified
                .iter()
                .find(|(mid, _)| *mid == c.id)
                .map(|(_, e)| e)
                .unwrap_or(&c.edit);
            edit.to_edit_op()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CandidateId, ConfidenceSummary, EditProposal, ProposalId, ProposedCandidate, ProposedEdit,
        Provenance, ProvenanceSource,
    };
    use rollshot_image_document::{EditOp, ImagePoint, ImageRect};

    fn rect(x: f32, y: f32, w: f32, h: f32) -> ImageRect {
        ImageRect::from_corners(ImagePoint::new(x, y), ImagePoint::new(x + w, y + h))
    }
    fn candidate(id: u64, edit: ProposedEdit) -> ProposedCandidate {
        ProposedCandidate {
            id: CandidateId(id),
            edit,
            confidence: 0.9,
            label: "test".into(),
            rationale: None,
            provenance: Provenance {
                source: ProvenanceSource::Agent { run_id: "run-00000000-0000-4000-8000-000000000001".to_string() },
            },
        }
    }
    fn proposal(cands: Vec<ProposedCandidate>) -> EditProposal {
        EditProposal {
            id: ProposalId::parse("proposal-00000001-0000-4000-8000-000000000000").unwrap(),
            base_document_state_id: 0,
            candidates: cands,
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
            rationale_summary: None,
            provenance: Provenance {
                source: ProvenanceSource::Agent { run_id: "run-00000000-0000-4000-8000-000000000001".to_string() },
            },
        }
    }

    #[test]
    fn lower_includes_accepted_drops_rejected_preserves_order() {
        let p = proposal(vec![
            candidate(
                1,
                ProposedEdit::AddRedaction {
                    bounds: rect(0.0, 0.0, 5.0, 5.0),
                },
            ),
            candidate(
                2,
                ProposedEdit::AddRedaction {
                    bounds: rect(10.0, 10.0, 5.0, 5.0),
                },
            ),
            candidate(
                3,
                ProposedEdit::AddRedaction {
                    bounds: rect(20.0, 20.0, 5.0, 5.0),
                },
            ),
        ]);
        let decision = ReviewDecision {
            proposal_id: ProposalId::parse("proposal-00000001-0000-4000-8000-000000000000").unwrap(),
            accepted: vec![CandidateId(1), CandidateId(3)],
            rejected: vec![CandidateId(2)],
            modified: vec![],
            resulting_document_state_id: 0,
        };
        let ops = lower(&p, &decision);
        assert_eq!(
            ops,
            vec![
                EditOp::AddRedaction {
                    bounds: rect(0.0, 0.0, 5.0, 5.0)
                },
                EditOp::AddRedaction {
                    bounds: rect(20.0, 20.0, 5.0, 5.0)
                },
            ]
        );
    }

    #[test]
    fn lower_applies_modified_override() {
        let p = proposal(vec![candidate(
            1,
            ProposedEdit::AddRedaction {
                bounds: rect(0.0, 0.0, 5.0, 5.0),
            },
        )]);
        let decision = ReviewDecision {
            proposal_id: ProposalId::parse("proposal-00000001-0000-4000-8000-000000000000").unwrap(),
            accepted: vec![CandidateId(1)],
            rejected: vec![],
            modified: vec![(
                CandidateId(1),
                ProposedEdit::AddRedaction {
                    bounds: rect(30.0, 30.0, 9.0, 9.0),
                },
            )],
            resulting_document_state_id: 0,
        };
        let ops = lower(&p, &decision);
        assert_eq!(
            ops,
            vec![EditOp::AddRedaction {
                bounds: rect(30.0, 30.0, 9.0, 9.0)
            }]
        );
    }

    #[test]
    fn lower_skips_unknown_accepted_and_unaccepted_modified() {
        let p = proposal(vec![candidate(
            1,
            ProposedEdit::AddRedaction {
                bounds: rect(0.0, 0.0, 5.0, 5.0),
            },
        )]);
        let decision = ReviewDecision {
            proposal_id: ProposalId::parse("proposal-00000001-0000-4000-8000-000000000000").unwrap(),
            accepted: vec![CandidateId(1), CandidateId(99)], // 99 absent from proposal -> skipped
            rejected: vec![],
            modified: vec![(
                CandidateId(2),
                ProposedEdit::AddRedaction {
                    bounds: rect(9.0, 9.0, 1.0, 1.0),
                },
            )], // id not accepted -> ignored
            resulting_document_state_id: 0,
        };
        assert_eq!(
            lower(&p, &decision),
            vec![EditOp::AddRedaction {
                bounds: rect(0.0, 0.0, 5.0, 5.0)
            }]
        );
    }

    #[test]
    fn review_decision_serde_round_trip() {
        use rollshot_image_document::AnnotationId;
        let decision = ReviewDecision {
            proposal_id: ProposalId::parse("proposal-00000003-0000-4000-8000-000000000000").unwrap(),
            accepted: vec![CandidateId(1), CandidateId(2)],
            rejected: vec![CandidateId(9)],
            modified: vec![(
                CandidateId(2),
                ProposedEdit::UpdateRedactionBounds {
                    id: AnnotationId(7),
                    bounds: rect(1.0, 1.0, 4.0, 4.0),
                },
            )],
            resulting_document_state_id: 11,
        };
        let json = serde_json::to_string(&decision).unwrap();
        let back: ReviewDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(back, decision);
    }
}
