use rollshot_edit_proposal::{lower, EditProposal, ReviewDecision};
use rollshot_image_document::ImageDocument;
use rollshot_preset::{PresetId, PresetStore, RevisionId, RevisionOrigin, RevisionProvenance};

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
    document
        .apply_batch(ops)
        .map(|_| ())
        .map_err(|_| WorkbenchError::RuntimeFailure)
}

/// Save a validated automation as a new revision and set it active.
pub fn save_revision(
    store: &PresetStore,
    preset_id: &PresetId,
    source: &str,
    parent_rev_id: Option<&RevisionId>,
    session_id: u64,
    now: String,
) -> Result<(), WorkbenchError> {
    let limits = rollshot_automation::ValidationLimits::default();
    let validated = rollshot_automation::validate_source(source, &limits)
        .map_err(|_| WorkbenchError::SourceValidationFailure)?;
    let rev_id = RevisionId(format!("rev-{}", chrono::Utc::now().timestamp_millis()));
    let provenance = RevisionProvenance {
        origin: RevisionOrigin::AgentRun,
        note: None,
        source_run_ref: Some(session_id.to_string()),
    };
    store
        .add_revision(
            preset_id,
            rev_id.clone(),
            parent_rev_id.cloned(),
            validated,
            provenance,
            now.clone(),
        )
        .map_err(|e| WorkbenchError::Store {
            message: e.to_string(),
        })?;
    store
        .set_active_revision(preset_id, &rev_id, now)
        .map_err(|e| WorkbenchError::Store {
            message: e.to_string(),
        })?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct CorrectionEvidence {
    pub rejected_count: usize,
    pub modified_count: usize,
    pub added_count: usize,
}

impl std::fmt::Display for CorrectionEvidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} rejected, {} resized, {} manually added",
            self.rejected_count, self.modified_count, self.added_count
        )
    }
}

/// Assemble correction evidence for Improve Preset (spec §8.2).
pub fn assemble_correction_evidence(
    _proposal: &EditProposal,
    review: &super::state::CandidateReview,
) -> CorrectionEvidence {
    let (_, rejected_ids, modified_pairs) = review.decision_sets();
    CorrectionEvidence {
        rejected_count: rejected_ids.len(),
        modified_count: modified_pairs.len(),
        added_count: 0, // SP6.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_edit_proposal::{
        CandidateId, ConfidenceSummary, EditProposal, ProposalId, ProposedCandidate, ProposedEdit,
        Provenance, ProvenanceSource,
    };
    use rollshot_image_document::ImageRect;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> ImageRect {
        ImageRect {
            x,
            y,
            width: w,
            height: h,
        }
    }
    fn candidate(id: u64, b: ImageRect) -> ProposedCandidate {
        ProposedCandidate {
            id: CandidateId(id),
            edit: ProposedEdit::AddRedaction { bounds: b },
            confidence: 0.9,
            label: "t".into(),
            rationale: None,
            provenance: Provenance {
                source: ProvenanceSource::Manual,
            },
        }
    }
    fn proposal(cands: Vec<ProposedCandidate>) -> EditProposal {
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
    fn restamp_proposal_updates_base_state_id() {
        let p = proposal(vec![candidate(1, rect(0.0, 0.0, 10.0, 10.0))]);
        let r = restamp_proposal(&p, 42);
        assert_eq!(r.base_document_state_id, 42);
        assert_eq!(r.candidates.len(), 1);
    }

    #[test]
    fn build_review_decision_all_pending() {
        let p = proposal(vec![
            candidate(1, rect(0.0, 0.0, 10.0, 10.0)),
            candidate(2, rect(0.0, 0.0, 10.0, 10.0)),
        ]);
        let review = CandidateReview::from_candidates(&[CandidateId(1), CandidateId(2)]);
        let d = build_review_decision(&p, &review, 42);
        assert_eq!(d.accepted.len(), 2);
        assert_eq!(d.rejected.len(), 0);
        assert_eq!(d.modified.len(), 0);
        assert_eq!(d.resulting_document_state_id, 42);
    }

    #[test]
    fn build_review_decision_with_reject_and_modify() {
        let p = proposal(vec![
            candidate(1, rect(0.0, 0.0, 10.0, 10.0)),
            candidate(2, rect(0.0, 0.0, 10.0, 10.0)),
        ]);
        let mut review = CandidateReview::from_candidates(&[CandidateId(1), CandidateId(2)]);
        review.mark_rejected(CandidateId(1));
        review.mark_modified(
            CandidateId(2),
            ProposedEdit::AddRedaction {
                bounds: rect(5.0, 5.0, 20.0, 20.0),
            },
        );
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
        review.mark_modified(
            CandidateId(1),
            ProposedEdit::AddRedaction {
                bounds: rect(70.0, 70.0, 20.0, 20.0),
            },
        );
        let mut doc = ImageDocument::new(image::RgbaImage::new(200, 200));
        apply_candidates(&p, &review, &mut doc).unwrap();
        // Annotation bounds come from the modified edit, not the original.
        use rollshot_image_document::annotation_bounds;
        let b = annotation_bounds(&doc.annotations()[0]);
        assert!((b.x - 70.0).abs() < 1e-5);
    }
}

#[cfg(test)]
mod save_tests {
    use super::*;
    use rollshot_preset::{PresetId, PresetStore};

    #[test]
    fn save_revision_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = PresetStore::open(tmp.path().to_path_buf());
        let preset_id = PresetId("test-preset".into());
        store
            .create_preset(
                preset_id.clone(),
                "Test".into(),
                "intent".into(),
                "2026-01-01T00:00:00Z".into(),
            )
            .unwrap();
        let source = r#"function main(input) { return { candidates: [] }; }"#;
        save_revision(
            &store,
            &preset_id,
            source,
            None,
            42,
            "2026-01-01T00:00:00Z".into(),
        )
        .unwrap();
        let active = store.load_active_revision(&preset_id).unwrap();
        assert!(active.artifact.source.contains("function main"));
    }

    #[test]
    fn save_revision_invalid_source_returns_err() {
        let tmp = tempfile::tempdir().unwrap();
        let store = PresetStore::open(tmp.path().to_path_buf());
        let preset_id = PresetId("test-preset".into());
        store
            .create_preset(
                preset_id.clone(),
                "Test".into(),
                "intent".into(),
                "2026-01-01T00:00:00Z".into(),
            )
            .unwrap();
        // No `main` function → validation fails.
        let bad = r#"function not_main() { return { candidates: [] }; }"#;
        assert!(save_revision(
            &store,
            &preset_id,
            bad,
            None,
            1,
            "2026-01-01T00:00:00Z".into()
        )
        .is_err());
    }
}

#[cfg(test)]
mod evidence_tests {
    use super::*;
    use rollshot_edit_proposal::{
        CandidateId, ConfidenceSummary, EditProposal, ProposalId, ProposedCandidate, ProposedEdit,
        Provenance, ProvenanceSource,
    };
    use rollshot_image_document::ImageRect;

    fn proposal() -> EditProposal {
        EditProposal {
            id: ProposalId(1),
            base_document_state_id: 0,
            candidates: vec![
                ProposedCandidate {
                    id: CandidateId(1),
                    edit: ProposedEdit::AddRedaction {
                        bounds: ImageRect {
                            x: 0.0,
                            y: 0.0,
                            width: 10.0,
                            height: 10.0,
                        },
                    },
                    confidence: 0.9,
                    label: "a".into(),
                    rationale: None,
                    provenance: Provenance {
                        source: ProvenanceSource::Manual,
                    },
                },
                ProposedCandidate {
                    id: CandidateId(2),
                    edit: ProposedEdit::AddRedaction {
                        bounds: ImageRect {
                            x: 0.0,
                            y: 0.0,
                            width: 10.0,
                            height: 10.0,
                        },
                    },
                    confidence: 0.9,
                    label: "b".into(),
                    rationale: None,
                    provenance: Provenance {
                        source: ProvenanceSource::Manual,
                    },
                },
            ],
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9, 0.9]),
            rationale_summary: None,
            provenance: Provenance {
                source: ProvenanceSource::Manual,
            },
        }
    }

    #[test]
    fn correction_evidence_counts_reject_and_modify() {
        let p = proposal();
        let mut review = super::super::state::CandidateReview::from_candidates(&[
            CandidateId(1),
            CandidateId(2),
        ]);
        review.mark_rejected(CandidateId(1));
        review.mark_modified(
            CandidateId(2),
            ProposedEdit::AddRedaction {
                bounds: ImageRect {
                    x: 1.0,
                    y: 1.0,
                    width: 5.0,
                    height: 5.0,
                },
            },
        );
        let e = assemble_correction_evidence(&p, &review);
        assert_eq!(e.rejected_count, 1);
        assert_eq!(e.modified_count, 1);
        assert_eq!(e.added_count, 0);
        assert!(format!("{e}").contains("1 rejected"));
        assert!(format!("{e}").contains("1 resized"));
    }

    #[test]
    fn correction_evidence_all_pending_is_zero() {
        let p = proposal();
        let review = super::super::state::CandidateReview::from_candidates(&[
            CandidateId(1),
            CandidateId(2),
        ]);
        let e = assemble_correction_evidence(&p, &review);
        assert_eq!(e.rejected_count, 0);
        assert_eq!(e.modified_count, 0);
        assert_eq!(e.added_count, 0);
    }
}
