use rollshot_edit_proposal::{lower, CandidateId, EditProposal, ProvenanceSource, ReviewDecision};
use rollshot_image_document::ImageDocument;
use rollshot_image_document::ImageRect;
use rollshot_preset::{
    AutomationRevision, PresetId, PresetStore, RevisionId, RevisionOrigin, RevisionProvenance,
};

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
    provenance_note: Option<&str>,
    session_id: u64,
    now: String,
) -> Result<AutomationRevision, WorkbenchError> {
    let limits = rollshot_automation::ValidationLimits::default();
    let validated = rollshot_automation::validate_source(source, &limits)
        .map_err(|_| WorkbenchError::SourceValidationFailure)?;
    let rev_id = RevisionId(format!("rev-{}", chrono::Utc::now().timestamp_millis()));
    let provenance = RevisionProvenance {
        origin: RevisionOrigin::AgentRun,
        note: provenance_note.map(str::to_owned),
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
    store
        .load_active_revision(preset_id)
        .map_err(|e| WorkbenchError::Store {
            message: e.to_string(),
        })
}

#[derive(Debug, Clone, PartialEq)]
pub struct RejectedCorrection {
    pub id: CandidateId,
    pub label: String,
    pub original_bounds: ImageRect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResizedCorrection {
    pub id: CandidateId,
    pub label: String,
    pub original_bounds: ImageRect,
    pub corrected_bounds: ImageRect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManualAddedCorrection {
    pub id: CandidateId,
    pub bounds: ImageRect,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CorrectionEvidence {
    pub rejected: Vec<RejectedCorrection>,
    pub resized: Vec<ResizedCorrection>,
    pub manual_added: Vec<ManualAddedCorrection>,
}

fn rect_summary(bounds: ImageRect) -> String {
    format!(
        "x={:.1} y={:.1} w={:.1} h={:.1}",
        bounds.x, bounds.y, bounds.width, bounds.height
    )
}

impl CorrectionEvidence {
    pub fn is_empty(&self) -> bool {
        self.rejected.is_empty() && self.resized.is_empty() && self.manual_added.is_empty()
    }

    pub fn summary_line(&self) -> String {
        format!(
            "{} rejected, {} resized, {} manually added",
            self.rejected.len(),
            self.resized.len(),
            self.manual_added.len()
        )
    }

    pub fn to_agent_message(&self) -> String {
        let mut out = String::from(
            "Improve the current Smart Redaction detector using this reviewed evidence.\n\
             Preserve existing useful detections, remove overfires, and add missed targets.\n\n\
             Correction evidence:\n",
        );
        out.push_str(&format!("- Summary: {}\n", self.summary_line()));
        if !self.rejected.is_empty() {
            out.push_str("- Rejected false positives:\n");
            for r in &self.rejected {
                out.push_str(&format!(
                    "  - id={} label={} original={}\n",
                    r.id.0,
                    r.label,
                    rect_summary(r.original_bounds)
                ));
            }
        }
        if !self.resized.is_empty() {
            out.push_str("- Resized target corrections:\n");
            for r in &self.resized {
                out.push_str(&format!(
                    "  - id={} label={} original={} corrected={}\n",
                    r.id.0,
                    r.label,
                    rect_summary(r.original_bounds),
                    rect_summary(r.corrected_bounds)
                ));
            }
        }
        if !self.manual_added.is_empty() {
            out.push_str("- Manually added missed targets:\n");
            for m in &self.manual_added {
                out.push_str(&format!(
                    "  - id={} bounds={}\n",
                    m.id.0,
                    rect_summary(m.bounds)
                ));
            }
        }
        out
    }
}

impl std::fmt::Display for CorrectionEvidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.summary_line())
    }
}

pub fn assemble_correction_evidence(
    proposal: &EditProposal,
    review: &super::state::CandidateReview,
) -> CorrectionEvidence {
    let (_accepted_ids, rejected_ids, modified_pairs) = review.decision_sets();
    let mut evidence = CorrectionEvidence::default();

    for id in rejected_ids {
        if let Some(candidate) = proposal.candidates.iter().find(|c| c.id == id) {
            if let Some(original_bounds) = super::state::proposed_edit_bounds(&candidate.edit) {
                evidence.rejected.push(RejectedCorrection {
                    id,
                    label: candidate.label.clone(),
                    original_bounds,
                });
            }
        }
    }

    for (id, corrected_edit) in modified_pairs {
        let Some(corrected_bounds) = super::state::proposed_edit_bounds(&corrected_edit) else {
            continue;
        };
        let Some(candidate) = proposal.candidates.iter().find(|c| c.id == id) else {
            continue;
        };
        if matches!(candidate.provenance.source, ProvenanceSource::Manual) {
            evidence.manual_added.push(ManualAddedCorrection {
                id,
                bounds: corrected_bounds,
            });
            continue;
        }
        if let Some(original_bounds) = super::state::proposed_edit_bounds(&candidate.edit) {
            evidence.resized.push(ResizedCorrection {
                id,
                label: candidate.label.clone(),
                original_bounds,
                corrected_bounds,
            });
        }
    }

    evidence
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
            None,
            42,
            "2026-01-01T00:00:00Z".into(),
        )
        .unwrap();
        let active = store.load_active_revision(&preset_id).unwrap();
        assert!(active.artifact.source.contains("function main"));
    }

    #[test]
    fn save_revision_records_parent_and_note() {
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
        let parent = rollshot_preset::RevisionId("rev-parent".into());
        save_revision(
            &store,
            &preset_id,
            source,
            Some(&parent),
            Some("improved from rev-parent; 1 rejected, 0 resized, 0 manually added"),
            42,
            "2026-01-01T00:00:00Z".into(),
        )
        .unwrap();
        let active = store.load_active_revision(&preset_id).unwrap();
        assert_eq!(active.parent_id, Some(parent));
        assert_eq!(
            active.provenance.note.as_deref(),
            Some("improved from rev-parent; 1 rejected, 0 resized, 0 manually added")
        );
    }

    #[test]
    fn save_revision_returns_saved_active_revision() {
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
        let saved = save_revision(
            &store,
            &preset_id,
            source,
            None,
            Some("initial author run"),
            42,
            "2026-01-01T00:00:00Z".into(),
        )
        .unwrap();

        assert_eq!(saved.preset_id, preset_id);
        assert_eq!(saved.provenance.note.as_deref(), Some("initial author run"));
        assert_eq!(store.load_active_revision(&preset_id).unwrap().id, saved.id);
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

    fn agent_candidate(id: u64, label: &str, bounds: ImageRect) -> ProposedCandidate {
        ProposedCandidate {
            id: CandidateId(id),
            edit: ProposedEdit::AddRedaction { bounds },
            confidence: 0.9,
            label: label.into(),
            rationale: None,
            provenance: Provenance {
                source: ProvenanceSource::Agent { run_id: 7 },
            },
        }
    }

    fn manual_candidate(id: u64, bounds: ImageRect) -> ProposedCandidate {
        ProposedCandidate {
            id: CandidateId(id),
            edit: ProposedEdit::AddRedaction { bounds },
            confidence: 1.0,
            label: "manual".into(),
            rationale: Some("Manually added missing candidate".into()),
            provenance: Provenance {
                source: ProvenanceSource::Manual,
            },
        }
    }

    #[test]
    fn correction_evidence_records_rejected_resized_and_manual_added_bounds() {
        let original_a = ImageRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let original_b = ImageRect {
            x: 20.0,
            y: 20.0,
            width: 10.0,
            height: 10.0,
        };
        let corrected_b = ImageRect {
            x: 22.0,
            y: 18.0,
            width: 14.0,
            height: 12.0,
        };
        let manual = ImageRect {
            x: 80.0,
            y: 10.0,
            width: 12.0,
            height: 8.0,
        };
        let p = EditProposal {
            id: ProposalId(1),
            base_document_state_id: 0,
            candidates: vec![
                agent_candidate(1, "email", original_a),
                agent_candidate(2, "name", original_b),
                manual_candidate(3, manual),
            ],
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9, 0.9, 1.0]),
            rationale_summary: None,
            provenance: Provenance {
                source: ProvenanceSource::Agent { run_id: 7 },
            },
        };
        let mut review = super::super::state::CandidateReview::from_candidates(&[
            CandidateId(1),
            CandidateId(2),
            CandidateId(3),
        ]);
        review.mark_rejected(CandidateId(1));
        review.mark_modified(
            CandidateId(2),
            ProposedEdit::AddRedaction {
                bounds: corrected_b,
            },
        );
        review.mark_modified(
            CandidateId(3),
            ProposedEdit::AddRedaction { bounds: manual },
        );

        let e = assemble_correction_evidence(&p, &review);
        assert_eq!(e.rejected.len(), 1);
        assert_eq!(e.resized.len(), 1);
        assert_eq!(e.manual_added.len(), 1);
        assert_eq!(e.rejected[0].original_bounds, original_a);
        assert_eq!(e.resized[0].original_bounds, original_b);
        assert_eq!(e.resized[0].corrected_bounds, corrected_b);
        assert_eq!(e.manual_added[0].bounds, manual);
        assert!(!e.is_empty());
    }

    #[test]
    fn rejected_candidate_formats_as_overfire_feedback() {
        let bounds = ImageRect {
            x: 4.0,
            y: 5.0,
            width: 6.0,
            height: 7.0,
        };
        let p = EditProposal {
            id: ProposalId(1),
            base_document_state_id: 0,
            candidates: vec![agent_candidate(1, "url-bar", bounds)],
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
            rationale_summary: None,
            provenance: Provenance {
                source: ProvenanceSource::Agent { run_id: 7 },
            },
        };
        let mut review = super::super::state::CandidateReview::from_candidates(&[CandidateId(1)]);
        review.mark_rejected(CandidateId(1));
        let msg = assemble_correction_evidence(&p, &review).to_agent_message();
        assert!(msg.contains("Rejected false positives"));
        assert!(msg.contains("label=url-bar"));
    }

    #[test]
    fn manual_candidate_formats_as_missed_target_feedback() {
        let bounds = ImageRect {
            x: 44.0,
            y: 55.0,
            width: 66.0,
            height: 77.0,
        };
        let p = EditProposal {
            id: ProposalId(1),
            base_document_state_id: 0,
            candidates: vec![manual_candidate(9, bounds)],
            confidence_summary: ConfidenceSummary::from_confidences(&[1.0]),
            rationale_summary: None,
            provenance: Provenance {
                source: ProvenanceSource::Agent { run_id: 7 },
            },
        };
        let mut review = super::super::state::CandidateReview::from_candidates(&[CandidateId(9)]);
        review.mark_modified(CandidateId(9), ProposedEdit::AddRedaction { bounds });
        let msg = assemble_correction_evidence(&p, &review).to_agent_message();
        assert!(msg.contains("Manually added missed targets"));
        assert!(msg.contains("id=9"));
    }

    #[test]
    fn correction_evidence_agent_message_is_deterministic_and_privacy_safe() {
        let original = ImageRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let p = EditProposal {
            id: ProposalId(1),
            base_document_state_id: 0,
            candidates: vec![agent_candidate(1, "email", original)],
            confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
            rationale_summary: None,
            provenance: Provenance {
                source: ProvenanceSource::Agent { run_id: 7 },
            },
        };
        let mut review = super::super::state::CandidateReview::from_candidates(&[CandidateId(1)]);
        review.mark_rejected(CandidateId(1));

        let e = assemble_correction_evidence(&p, &review);
        let msg = e.to_agent_message();
        assert!(msg.contains("Rejected false positives"));
        assert!(msg.contains("id=1 label=email"));
        assert!(msg.contains("x=0.0 y=0.0 w=10.0 h=10.0"));
        assert!(!msg.contains("data:image"));
        assert!(!msg.contains("authorization"));
    }
}
