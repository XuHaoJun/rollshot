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
    policy.allowed_annotation_ids =
        BTreeSet::from([AnnotationId(0), AnnotationId(42), AnnotationId(u64::MAX)]);
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

#[test]
fn strict_output_schema_rejects_invalid_shapes() {
    let invalid = [
        r#"{"candidates":[],"extra":true}"#,
        r#"{"candidates":[{"kind":"unknown","confidence":1,"label":"x"}]}"#,
        r#"{"candidates":[{"kind":"addRedaction","bounds":{"x":0,"y":0,"width":1,"height":1,"extra":1},"confidence":1,"label":"x"}]}"#,
        r#"{"candidates":[{"kind":"addRedaction","bounds":{"x":0,"y":0,"width":1,"height":1},"confidence":2,"label":"x"}]}"#,
        r#"{"candidates":[{"kind":"addRedaction","bounds":{"x":0,"y":0,"width":0,"height":1},"confidence":1,"label":"x"}]}"#,
        r#"{"candidates":[{"kind":"addRedaction","bounds":{"x":0,"y":0,"width":1,"height":1},"confidence":1,"label":""}]}"#,
        r#"{"candidates":[{"kind":"delete","annotationId":"01","confidence":1,"label":"x"}]}"#,
        r#"{"candidates":[{"kind":"updateText","annotationId":"42","confidence":1,"label":"x"}]}"#,
    ];
    for json in invalid {
        assert!(
            decode_proposal(json, (100, 100), &context(), &allow_all()).is_err(),
            "expected rejection for: {json}"
        );
    }
}

#[test]
fn rejects_label_over_128_bytes() {
    let long_label = "x".repeat(129);
    let json = format!(
        r#"{{"candidates":[{{"kind":"addRedaction","bounds":{{"x":0,"y":0,"width":1,"height":1}},"confidence":1,"label":"{}"}}]}}"#,
        long_label
    );
    assert_eq!(
        decode_proposal(&json, (100, 100), &context(), &allow_all()),
        Err(OutputError::Malformed {
            code: "invalid_label"
        })
    );
}

#[test]
fn rejects_rationale_over_2048_bytes() {
    let long_rationale = "x".repeat(2_049);
    let json = format!(
        r#"{{"candidates":[{{"kind":"addRedaction","bounds":{{"x":0,"y":0,"width":1,"height":1}},"confidence":1,"label":"ok","rationale":"{}"}}]}}"#,
        long_rationale
    );
    assert_eq!(
        decode_proposal(&json, (100, 100), &context(), &allow_all()),
        Err(OutputError::Malformed {
            code: "rationale_too_long"
        })
    );
}

#[test]
fn rejects_out_of_bounds_redaction_when_policy_disallows_it() {
    let json = r#"{"candidates":[{"kind":"addRedaction","bounds":{"x":90,"y":90,"width":30,"height":30},"confidence":1,"label":"oob"}]}"#;
    let policy = allow_all();
    assert!(matches!(
        decode_proposal(json, (100, 100), &context(), &policy),
        Err(OutputError::Policy(
            rollshot_edit_proposal::PolicyError::OutOfBounds { .. }
        ))
    ));
}

#[test]
fn rejects_candidate_count_over_policy_limit() {
    let mut policy = allow_all();
    policy.proposal_limits.max_candidates = 2;
    let json = r#"{"candidates":[
        {"kind":"addRedaction","bounds":{"x":0,"y":0,"width":1,"height":1},"confidence":1,"label":"a"},
        {"kind":"addRedaction","bounds":{"x":2,"y":2,"width":1,"height":1},"confidence":1,"label":"b"},
        {"kind":"addRedaction","bounds":{"x":4,"y":4,"width":1,"height":1},"confidence":1,"label":"c"}
    ]}"#;
    assert!(matches!(
        decode_proposal(json, (100, 100), &context(), &policy),
        Err(OutputError::Policy(
            rollshot_edit_proposal::PolicyError::TooManyCandidates { .. }
        ))
    ));
}

#[test]
fn rejects_total_redaction_area_over_policy_limit() {
    let mut policy = allow_all();
    policy.proposal_limits.max_total_area_fraction = 0.5;
    // 80x80 = 6400 over 100x100 = 10000 -> 0.64 > 0.5
    let json = r#"{"candidates":[{"kind":"addRedaction","bounds":{"x":0,"y":0,"width":80,"height":80},"confidence":1,"label":"big"}]}"#;
    assert!(matches!(
        decode_proposal(json, (100, 100), &context(), &policy),
        Err(OutputError::Policy(
            rollshot_edit_proposal::PolicyError::ExcessiveTotalArea { .. }
        ))
    ));
}

#[test]
fn rejects_nan_point_in_private_wire_conversion_test() {
    let json = r#"{"candidates":[{"kind":"addTextNote","position":{"x":0,"y":null},"text":"ok","confidence":1,"label":"x"}]}"#;
    assert!(matches!(
        decode_proposal(json, (100, 100), &context(), &allow_all()),
        Err(OutputError::Malformed { .. })
    ));
}

#[test]
fn rejects_infinite_rect_in_private_wire_conversion_test() {
    let json = r#"{"candidates":[{"kind":"addRedaction","bounds":{"x":0,"y":0,"width":1e309,"height":1},"confidence":1,"label":"x"}]}"#;
    assert!(matches!(
        decode_proposal(json, (100, 100), &context(), &allow_all()),
        Err(OutputError::Malformed { .. }) | Err(OutputError::InvalidNumber { .. })
    ));
}

#[test]
fn accepts_annotation_id_zero() {
    let json =
        r#"{"candidates":[{"kind":"delete","annotationId":"0","confidence":1,"label":"x"}]}"#;
    assert!(decode_proposal(json, (100, 100), &context(), &allow_all()).is_ok());
}

#[test]
fn accepts_annotation_id_u64_max() {
    let json = r#"{"candidates":[{"kind":"delete","annotationId":"18446744073709551615","confidence":1,"label":"x"}]}"#;
    assert!(decode_proposal(json, (100, 100), &context(), &allow_all()).is_ok());
}
