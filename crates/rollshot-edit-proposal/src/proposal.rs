//! Agent-flavored edit-proposal model (spec §6.3). Framework-neutral; lowers to
//! `rollshot_image_document::EditOp` on accept. No agent/LLM or UI code here.

use rollshot_image_document::{AnnotationId, EditOp, ImagePoint, ImageRect};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CandidateId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProposalId(pub u64);

/// Where a proposal/candidate came from. Privacy-safe: ids/counts only, never prompts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProvenanceSource {
    Manual,
    Agent { run_id: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub source: ProvenanceSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ConfidenceSummary {
    pub min: f32,
    pub max: f32,
    pub mean: f32,
    pub count: u32,
}

impl ConfidenceSummary {
    /// Aggregate a candidate set's confidences. An empty slice yields zeros.
    pub fn from_confidences(values: &[f32]) -> Self {
        if values.is_empty() {
            return Self {
                min: 0.0,
                max: 0.0,
                mean: 0.0,
                count: 0,
            };
        }
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        let mut sum = 0.0f32;
        for &v in values {
            min = min.min(v);
            max = max.max(v);
            sum += v;
        }
        Self {
            min,
            max,
            mean: sum / values.len() as f32,
            count: values.len() as u32,
        }
    }
}

/// What document change a candidate proposes. Mirrors `EditOp`; v1 mainly
/// produces `AddRedaction`. Lowers to `EditOp` via `to_edit_op`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProposedEdit {
    AddRedaction {
        bounds: ImageRect,
    },
    AddTextNote {
        position: ImagePoint,
        text: String,
    },
    AddNumberCallout {
        tip: ImagePoint,
        bubble: ImagePoint,
    },
    UpdateRedactionBounds {
        id: AnnotationId,
        bounds: ImageRect,
    },
    UpdateTextPosition {
        id: AnnotationId,
        position: ImagePoint,
    },
    UpdateText {
        id: AnnotationId,
        text: String,
    },
    UpdateNumberPoints {
        id: AnnotationId,
        tip: ImagePoint,
        bubble: ImagePoint,
    },
    Delete {
        id: AnnotationId,
    },
}

impl ProposedEdit {
    /// Lower this proposal-level edit to the document-level `EditOp`.
    pub fn to_edit_op(&self) -> EditOp {
        match self {
            ProposedEdit::AddRedaction { bounds } => EditOp::AddRedaction { bounds: *bounds },
            ProposedEdit::AddTextNote { position, text } => EditOp::AddTextNote {
                position: *position,
                text: text.clone(),
            },
            ProposedEdit::AddNumberCallout { tip, bubble } => EditOp::AddNumberCallout {
                tip: *tip,
                bubble: *bubble,
            },
            ProposedEdit::UpdateRedactionBounds { id, bounds } => EditOp::UpdateRedactionBounds {
                id: *id,
                bounds: *bounds,
            },
            ProposedEdit::UpdateTextPosition { id, position } => EditOp::UpdateTextPosition {
                id: *id,
                position: *position,
            },
            ProposedEdit::UpdateText { id, text } => EditOp::UpdateText {
                id: *id,
                text: text.clone(),
            },
            ProposedEdit::UpdateNumberPoints { id, tip, bubble } => EditOp::UpdateNumberPoints {
                id: *id,
                tip: *tip,
                bubble: *bubble,
            },
            ProposedEdit::Delete { id } => EditOp::Delete { id: *id },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposedCandidate {
    pub id: CandidateId,
    pub edit: ProposedEdit,
    pub confidence: f32,
    pub rationale: Option<String>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditProposal {
    pub id: ProposalId,
    /// `ImageDocument::state_id()` captured before the proposal is applied
    /// (provenance/staleness — recovery is via the single undo entry).
    pub base_document_state_id: u64,
    pub candidates: Vec<ProposedCandidate>,
    pub confidence_summary: ConfidenceSummary,
    pub rationale_summary: Option<String>,
    pub provenance: Provenance,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_image_document::{EditOp, ImagePoint, ImageRect};

    #[test]
    fn proposed_edit_lowers_to_matching_edit_op() {
        let r = ImageRect::from_corners(ImagePoint::new(0.0, 0.0), ImagePoint::new(8.0, 8.0));
        let pe = ProposedEdit::AddRedaction { bounds: r };
        assert_eq!(pe.to_edit_op(), EditOp::AddRedaction { bounds: r });
    }

    #[test]
    fn confidence_summary_aggregates() {
        let s = ConfidenceSummary::from_confidences(&[0.2, 0.8, 0.5]);
        assert_eq!(s.count, 3);
        assert!((s.min - 0.2).abs() < 1e-6);
        assert!((s.max - 0.8).abs() < 1e-6);
        assert!((s.mean - 0.5).abs() < 1e-6);
    }

    #[test]
    fn proposal_serde_round_trip() {
        let r = ImageRect::from_corners(ImagePoint::new(1.0, 1.0), ImagePoint::new(9.0, 9.0));
        let proposal = EditProposal {
            id: ProposalId(1),
            base_document_state_id: 7,
            candidates: vec![ProposedCandidate {
                id: CandidateId(1),
                edit: ProposedEdit::AddRedaction { bounds: r },
                confidence: 0.9,
                rationale: Some("matches email pattern".into()),
                provenance: Provenance {
                    source: ProvenanceSource::Agent { run_id: 42 },
                },
            }],
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
            rationale_summary: None,
            provenance: Provenance {
                source: ProvenanceSource::Agent { run_id: 42 },
            },
        };
        let json = serde_json::to_string(&proposal).unwrap();
        let back: EditProposal = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, proposal.id);
        assert_eq!(back.candidates.len(), 1);
    }

    #[test]
    fn confidence_summary_empty_slice_is_zeros() {
        let s = ConfidenceSummary::from_confidences(&[]);
        assert_eq!(
            s,
            ConfidenceSummary {
                min: 0.0,
                max: 0.0,
                mean: 0.0,
                count: 0
            }
        );
    }

    #[test]
    fn proposed_edit_lowers_remaining_variants() {
        use rollshot_image_document::AnnotationId;
        let p = ImagePoint::new(1.0, 2.0);
        let r = ImageRect::from_corners(ImagePoint::new(0.0, 0.0), ImagePoint::new(8.0, 8.0));
        assert_eq!(
            ProposedEdit::AddTextNote {
                position: p,
                text: "x".into()
            }
            .to_edit_op(),
            EditOp::AddTextNote {
                position: p,
                text: "x".into()
            }
        );
        assert_eq!(
            ProposedEdit::AddNumberCallout { tip: p, bubble: p }.to_edit_op(),
            EditOp::AddNumberCallout { tip: p, bubble: p }
        );
        assert_eq!(
            ProposedEdit::UpdateRedactionBounds {
                id: AnnotationId(1),
                bounds: r
            }
            .to_edit_op(),
            EditOp::UpdateRedactionBounds {
                id: AnnotationId(1),
                bounds: r
            }
        );
        assert_eq!(
            ProposedEdit::UpdateTextPosition {
                id: AnnotationId(2),
                position: p
            }
            .to_edit_op(),
            EditOp::UpdateTextPosition {
                id: AnnotationId(2),
                position: p
            }
        );
        assert_eq!(
            ProposedEdit::UpdateText {
                id: AnnotationId(3),
                text: "y".into()
            }
            .to_edit_op(),
            EditOp::UpdateText {
                id: AnnotationId(3),
                text: "y".into()
            }
        );
        assert_eq!(
            ProposedEdit::UpdateNumberPoints {
                id: AnnotationId(4),
                tip: p,
                bubble: p
            }
            .to_edit_op(),
            EditOp::UpdateNumberPoints {
                id: AnnotationId(4),
                tip: p,
                bubble: p
            }
        );
        assert_eq!(
            ProposedEdit::Delete {
                id: AnnotationId(5)
            }
            .to_edit_op(),
            EditOp::Delete {
                id: AnnotationId(5)
            }
        );
    }
}
