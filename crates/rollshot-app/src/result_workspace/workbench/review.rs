use rollshot_edit_proposal::{EditProposal, ReviewDecision, lower};
use rollshot_image_document::ImageDocument;

use super::state::{CandidateReview, WorkbenchError};

/// Re-stamp a proposal's `base_document_state_id` from the live document.
/// DryRunTool hardcodes 0/1; this corrects it before `lower` (§10.5).
pub fn restamp_proposal(proposal: &EditProposal, doc_state_id: u64) -> EditProposal {
    let mut p = proposal.clone();
    p.base_document_state_id = doc_state_id;
    p
}

/// Build a `ReviewDecision` from the proposal and the user's review state.
pub fn build_review_decision(
    proposal: &EditProposal,
    review: &CandidateReview,
    doc_state_id: u64,
) -> ReviewDecision {
    let (accepted, rejected, modified) = review.decision_sets();
    ReviewDecision {
        proposal_id: proposal.id,
        accepted,
        rejected,
        modified,
        resulting_document_state_id: doc_state_id,
    }
}

/// Lower the proposal to `EditOp`s via the `ReviewDecision`, then apply as
/// one undoable `ImageDocument::apply_batch` transaction. Returns
/// `WorkbenchError::RuntimeFailure` if the document rejects the batch.
pub fn apply_candidates(
    proposal: &EditProposal,
    review: &CandidateReview,
    document: &mut ImageDocument,
) -> Result<(), WorkbenchError> {
    let restamped = restamp_proposal(proposal, document.state_id());
    let decision = build_review_decision(&restamped, review, document.state_id());
    let ops = lower(&restamped, &decision);
    if ops.is_empty() {
        return Ok(());
    }
    document.apply_batch(ops).map(|_| ()).map_err(|_| WorkbenchError::RuntimeFailure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_edit_proposal::{
        CandidateId, ConfidenceSummary, EditProposal, ProposalId, ProposedCandidate,
        ProposedEdit, Provenance, ProvenanceSource,
    };
    use rollshot_image_document::ImageRect;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> ImageRect {
        ImageRect { x, y, width: w, height: h }
    }
    fn candidate(id: u64, b: ImageRect) -> ProposedCandidate {
        ProposedCandidate {
            id: CandidateId(id), edit: ProposedEdit::AddRedaction { bounds: b },
            confidence: 0.9, label: "t".into(), rationale: None,
            provenance: Provenance { source: ProvenanceSource::Manual },
        }
    }
    fn proposal(cands: Vec<ProposedCandidate>) -> EditProposal {
        EditProposal {
            id: ProposalId(1), base_document_state_id: 0, candidates: cands,
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
            rationale_summary: None,
            provenance: Provenance { source: ProvenanceSource::Manual },
        }
    }

    #[test]
    fn restamp_proposal_updates_base_state_id() {
        let p = proposal(vec![candidate(1, rect(0.0, 0.0, 10.0, 10.0))]);
        let r = restamp_proposal(&p, 42);
        assert_eq!(r.base_document_state_id, 42);
        assert_eq!(r.candidates.len(), 1);
    }

    #[test]
    fn build_review_decision_all_pending() {
        let p = proposal(vec![candidate(1, rect(0.0,0.0,10.0,10.0)), candidate(2, rect(0.0,0.0,10.0,10.0))]);
        let review = CandidateReview::from_candidates(&[CandidateId(1), CandidateId(2)]);
        let d = build_review_decision(&p, &review, 42);
        assert_eq!(d.accepted.len(), 2);
        assert_eq!(d.rejected.len(), 0);
        assert_eq!(d.modified.len(), 0);
        assert_eq!(d.resulting_document_state_id, 42);
    }

    #[test]
    fn build_review_decision_with_reject_and_modify() {
        let p = proposal(vec![candidate(1, rect(0.0,0.0,10.0,10.0)), candidate(2, rect(0.0,0.0,10.0,10.0))]);
        let mut review = CandidateReview::from_candidates(&[CandidateId(1), CandidateId(2)]);
        review.mark_rejected(CandidateId(1));
        review.mark_modified(CandidateId(2), ProposedEdit::AddRedaction { bounds: rect(5.0,5.0,20.0,20.0) });
        let d = build_review_decision(&p, &review, 7);
        assert_eq!(d.accepted, vec![CandidateId(2)]);
        assert_eq!(d.rejected, vec![CandidateId(1)]);
        assert_eq!(d.modified.len(), 1);
    }

    #[test]
    fn apply_candidates_commits_one_annotation_per_accepted() {
        let p = proposal(vec![
            candidate(1, rect(10.0, 10.0, 50.0, 50.0)),
            candidate(2, rect(100.0, 100.0, 30.0, 30.0)),
        ]);
        let review = CandidateReview::from_candidates(&[CandidateId(1), CandidateId(2)]);
        let mut doc = ImageDocument::new(image::RgbaImage::new(200, 200));
        assert_eq!(doc.annotations().len(), 0);
        apply_candidates(&p, &review, &mut doc).unwrap();
        assert_eq!(doc.annotations().len(), 2);
    }

    #[test]
    fn apply_candidates_skips_rejected() {
        let p = proposal(vec![
            candidate(1, rect(10.0, 10.0, 50.0, 50.0)),
            candidate(2, rect(100.0, 100.0, 30.0, 30.0)),
        ]);
        let mut review = CandidateReview::from_candidates(&[CandidateId(1), CandidateId(2)]);
        review.mark_rejected(CandidateId(2));
        let mut doc = ImageDocument::new(image::RgbaImage::new(200, 200));
        apply_candidates(&p, &review, &mut doc).unwrap();
        assert_eq!(doc.annotations().len(), 1);
    }

    #[test]
    fn apply_candidates_uses_modified_bounds() {
        let p = proposal(vec![candidate(1, rect(10.0, 10.0, 50.0, 50.0))]);
        let mut review = CandidateReview::from_candidates(&[CandidateId(1)]);
        review.mark_modified(CandidateId(1), ProposedEdit::AddRedaction { bounds: rect(70.0, 70.0, 20.0, 20.0) });
        let mut doc = ImageDocument::new(image::RgbaImage::new(200, 200));
        apply_candidates(&p, &review, &mut doc).unwrap();
        // Annotation bounds come from the modified edit, not the original.
        use rollshot_image_document::annotation_bounds;
        let b = annotation_bounds(&doc.annotations()[0]);
        assert!((b.x - 70.0).abs() < 1e-5);
    }
}
