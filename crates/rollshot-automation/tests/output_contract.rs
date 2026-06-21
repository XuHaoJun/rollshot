use std::collections::BTreeSet;
use std::time::Duration;

use rollshot_automation::{
    decode_proposal, ExecutionPolicy, OutputError, ProposalContext, ProposedEditKind,
};
use rollshot_edit_proposal::{ProposalId, ProposedEdit, Provenance, ProvenanceSource};
use rollshot_image_document::AnnotationId;

fn context() -> ProposalContext {
    ProposalContext {
        proposal_id: ProposalId(7),
        base_document_state_id: 11,
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: 42 },
        },
    }
}

fn allow_all() -> ExecutionPolicy {
    let mut policy = ExecutionPolicy::smart_redaction_default(
        Duration::from_secs(1),
        8 * 1024 * 1024,
        256 * 1024,
    );
    policy.allowed_edit_kinds = BTreeSet::from([
        ProposedEditKind::AddRedaction,
        ProposedEditKind::AddTextNote,
        ProposedEditKind::AddNumberCallout,
        ProposedEditKind::UpdateRedactionBounds,
        ProposedEditKind::UpdateTextPosition,
        ProposedEditKind::UpdateText,
        ProposedEditKind::UpdateNumberPoints,
        ProposedEditKind::Delete,
    ]);
    policy.allowed_annotation_ids = BTreeSet::from([AnnotationId(42)]);
    policy
}

#[test]
fn decodes_complete_crud_union_in_output_order() {
    let json = r#"{
      "candidates": [
        {"kind":"addRedaction","bounds":{"x":1.0,"y":2.0,"width":3.0,"height":4.0},"confidence":0.9,"label":"secret"},
        {"kind":"addTextNote","position":{"x":5.0,"y":6.0},"text":"note","confidence":0.8,"label":"note"},
        {"kind":"addNumberCallout","tip":{"x":7.0,"y":8.0},"bubble":{"x":9.0,"y":10.0},"confidence":0.7,"label":"step"},
        {"kind":"updateRedactionBounds","annotationId":"42","bounds":{"x":2.0,"y":3.0,"width":4.0,"height":5.0},"confidence":0.6,"label":"resize"},
        {"kind":"updateTextPosition","annotationId":"42","position":{"x":4.0,"y":5.0},"confidence":0.6,"label":"move"},
        {"kind":"updateText","annotationId":"42","text":"changed","confidence":0.6,"label":"text"},
        {"kind":"updateNumberPoints","annotationId":"42","tip":{"x":1.0,"y":1.0},"bubble":{"x":2.0,"y":2.0},"confidence":0.6,"label":"points"},
        {"kind":"delete","annotationId":"42","confidence":0.5,"label":"remove"}
      ]
    }"#;
    let proposal = decode_proposal(json, (100, 100), &context(), &allow_all()).unwrap();
    assert_eq!(proposal.id, ProposalId(7));
    assert_eq!(proposal.base_document_state_id, 11);
    assert_eq!(proposal.candidates.len(), 8);
    assert!(matches!(
        proposal.candidates[0].edit,
        ProposedEdit::AddRedaction { .. }
    ));
    assert!(matches!(
        proposal.candidates[7].edit,
        ProposedEdit::Delete {
            id: AnnotationId(42)
        }
    ));
    assert_eq!(proposal.candidates[0].label, "secret");
}

#[test]
fn rejects_unknown_fields_and_noncanonical_annotation_ids() {
    let unknown = r#"{"candidates":[{"kind":"delete","annotationId":"42","confidence":0.5,"label":"x","extra":true}]}"#;
    assert!(matches!(
        decode_proposal(unknown, (100, 100), &context(), &allow_all()),
        Err(OutputError::Malformed { .. })
    ));

    for id in ["+42", "042", " 42", "18446744073709551616"] {
        let json = format!(
            r#"{{"candidates":[{{"kind":"delete","annotationId":"{id}","confidence":0.5,"label":"x"}}]}}"#
        );
        assert!(matches!(
            decode_proposal(&json, (100, 100), &context(), &allow_all()),
            Err(OutputError::InvalidAnnotationId { .. })
        ));
    }
}

#[test]
fn rejects_unauthorized_edit_kind_and_annotation_id() {
    let delete =
        r#"{"candidates":[{"kind":"delete","annotationId":"42","confidence":0.5,"label":"x"}]}"#;
    let redaction_only = ExecutionPolicy::smart_redaction_default(
        Duration::from_secs(1),
        8 * 1024 * 1024,
        256 * 1024,
    );
    assert_eq!(
        decode_proposal(delete, (100, 100), &context(), &redaction_only),
        Err(OutputError::EditKindDenied {
            kind: ProposedEditKind::Delete,
        })
    );
}
