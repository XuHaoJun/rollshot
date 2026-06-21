use rollshot_edit_proposal::{
    validate_policy, CandidateId, ConfidenceSummary, EditProposal, ProposedCandidate, ProposedEdit,
};
use rollshot_image_document::{AnnotationId, ImagePoint, ImageRect};
use serde::Deserialize;

use crate::{AutomationInput, ExecutionPolicy, ProposalContext, ProposedEditKind};

pub(crate) const MAX_LABEL_BYTES: usize = 128;
pub(crate) const MAX_RATIONALE_BYTES: usize = 2_048;
pub(crate) const MAX_TEXT_BYTES: usize = 2_048;
pub(crate) const MAX_CANDIDATE_STRUCTURAL_BYTES: usize = 384;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum OutputError {
    #[error("output exceeds byte limit")]
    TooLarge,
    #[error("malformed output: {code}")]
    Malformed { code: &'static str },
    #[error("invalid annotation id")]
    InvalidAnnotationId { value: String },
    #[error("invalid finite range: {field}")]
    InvalidNumber { field: &'static str },
    #[error("edit kind denied")]
    EditKindDenied { kind: ProposedEditKind },
    #[error("annotation id denied")]
    AnnotationDenied { id: AnnotationId },
    #[error("proposal policy rejected output")]
    Policy(#[from] rollshot_edit_proposal::PolicyError),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputEnvelope {
    candidates: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WirePoint {
    x: f32,
    y: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct WireAddRedaction {
    kind: String,
    bounds: WireRect,
    confidence: f32,
    label: String,
    #[serde(default)]
    rationale: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct WireAddTextNote {
    kind: String,
    position: WirePoint,
    text: String,
    confidence: f32,
    label: String,
    #[serde(default)]
    rationale: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct WireAddNumberCallout {
    kind: String,
    tip: WirePoint,
    bubble: WirePoint,
    confidence: f32,
    label: String,
    #[serde(default)]
    rationale: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct WireUpdateRedactionBounds {
    kind: String,
    #[serde(rename = "annotationId")]
    annotation_id: String,
    bounds: WireRect,
    confidence: f32,
    label: String,
    #[serde(default)]
    rationale: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct WireUpdateTextPosition {
    kind: String,
    #[serde(rename = "annotationId")]
    annotation_id: String,
    position: WirePoint,
    confidence: f32,
    label: String,
    #[serde(default)]
    rationale: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct WireUpdateText {
    kind: String,
    #[serde(rename = "annotationId")]
    annotation_id: String,
    text: String,
    confidence: f32,
    label: String,
    #[serde(default)]
    rationale: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct WireUpdateNumberPoints {
    kind: String,
    #[serde(rename = "annotationId")]
    annotation_id: String,
    tip: WirePoint,
    bubble: WirePoint,
    confidence: f32,
    label: String,
    #[serde(default)]
    rationale: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct WireDelete {
    kind: String,
    #[serde(rename = "annotationId")]
    annotation_id: String,
    confidence: f32,
    label: String,
    #[serde(default)]
    rationale: Option<String>,
}

fn parse_annotation_id(value: &str) -> Result<AnnotationId, OutputError> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(OutputError::InvalidAnnotationId {
            value: value.into(),
        });
    }
    value
        .parse::<u64>()
        .map(AnnotationId)
        .map_err(|_| OutputError::InvalidAnnotationId {
            value: value.into(),
        })
}

fn point(value: WirePoint) -> Result<ImagePoint, OutputError> {
    if !value.x.is_finite() || !value.y.is_finite() {
        return Err(OutputError::InvalidNumber { field: "point" });
    }
    Ok(ImagePoint::new(value.x, value.y))
}

fn rect(value: WireRect) -> Result<ImageRect, OutputError> {
    if !value.x.is_finite()
        || !value.y.is_finite()
        || !value.width.is_finite()
        || !value.height.is_finite()
        || value.width <= 0.0
        || value.height <= 0.0
    {
        return Err(OutputError::InvalidNumber { field: "bounds" });
    }
    Ok(ImageRect::from_corners(
        ImagePoint::new(value.x, value.y),
        ImagePoint::new(value.x + value.width, value.y + value.height),
    ))
}

fn validate_metadata(
    confidence: f32,
    label: &str,
    rationale: Option<&str>,
) -> Result<(), OutputError> {
    if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
        return Err(OutputError::InvalidNumber {
            field: "confidence",
        });
    }
    if label.trim().is_empty() || label.len() > MAX_LABEL_BYTES {
        return Err(OutputError::Malformed {
            code: "invalid_label",
        });
    }
    if rationale.is_some_and(|text| text.len() > MAX_RATIONALE_BYTES) {
        return Err(OutputError::Malformed {
            code: "rationale_too_long",
        });
    }
    Ok(())
}

fn validate_text(text: &str) -> Result<(), OutputError> {
    if text.is_empty() {
        return Err(OutputError::Malformed { code: "empty_text" });
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(OutputError::Malformed {
            code: "text_too_long",
        });
    }
    Ok(())
}

enum DecodedCandidate {
    Add(ProposedEdit, f32, String, Option<String>),
    Update(ProposedEdit, AnnotationId, f32, String, Option<String>),
    Delete(AnnotationId, f32, String, Option<String>),
}

fn decode_one_candidate(raw: &serde_json::Value) -> Result<DecodedCandidate, OutputError> {
    let kind = raw
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or(OutputError::Malformed {
            code: "missing_kind",
        })?;

    match kind {
        "addRedaction" => {
            let w: WireAddRedaction =
                serde_json::from_value(raw.clone()).map_err(|_| OutputError::Malformed {
                    code: "unknown_fields_or_shape",
                })?;
            validate_metadata(w.confidence, &w.label, w.rationale.as_deref())?;
            let bounds = rect(w.bounds)?;
            Ok(DecodedCandidate::Add(
                ProposedEdit::AddRedaction { bounds },
                w.confidence,
                w.label,
                w.rationale,
            ))
        }
        "addTextNote" => {
            let w: WireAddTextNote =
                serde_json::from_value(raw.clone()).map_err(|_| OutputError::Malformed {
                    code: "unknown_fields_or_shape",
                })?;
            validate_metadata(w.confidence, &w.label, w.rationale.as_deref())?;
            let position = point(w.position)?;
            validate_text(&w.text)?;
            Ok(DecodedCandidate::Add(
                ProposedEdit::AddTextNote {
                    position,
                    text: w.text,
                },
                w.confidence,
                w.label,
                w.rationale,
            ))
        }
        "addNumberCallout" => {
            let w: WireAddNumberCallout =
                serde_json::from_value(raw.clone()).map_err(|_| OutputError::Malformed {
                    code: "unknown_fields_or_shape",
                })?;
            validate_metadata(w.confidence, &w.label, w.rationale.as_deref())?;
            let tip = point(w.tip)?;
            let bubble = point(w.bubble)?;
            Ok(DecodedCandidate::Add(
                ProposedEdit::AddNumberCallout { tip, bubble },
                w.confidence,
                w.label,
                w.rationale,
            ))
        }
        "updateRedactionBounds" => {
            let w: WireUpdateRedactionBounds =
                serde_json::from_value(raw.clone()).map_err(|_| OutputError::Malformed {
                    code: "unknown_fields_or_shape",
                })?;
            validate_metadata(w.confidence, &w.label, w.rationale.as_deref())?;
            let id = parse_annotation_id(&w.annotation_id)?;
            let bounds = rect(w.bounds)?;
            Ok(DecodedCandidate::Update(
                ProposedEdit::UpdateRedactionBounds { id, bounds },
                id,
                w.confidence,
                w.label,
                w.rationale,
            ))
        }
        "updateTextPosition" => {
            let w: WireUpdateTextPosition =
                serde_json::from_value(raw.clone()).map_err(|_| OutputError::Malformed {
                    code: "unknown_fields_or_shape",
                })?;
            validate_metadata(w.confidence, &w.label, w.rationale.as_deref())?;
            let id = parse_annotation_id(&w.annotation_id)?;
            let position = point(w.position)?;
            Ok(DecodedCandidate::Update(
                ProposedEdit::UpdateTextPosition { id, position },
                id,
                w.confidence,
                w.label,
                w.rationale,
            ))
        }
        "updateText" => {
            let w: WireUpdateText =
                serde_json::from_value(raw.clone()).map_err(|_| OutputError::Malformed {
                    code: "unknown_fields_or_shape",
                })?;
            validate_metadata(w.confidence, &w.label, w.rationale.as_deref())?;
            let id = parse_annotation_id(&w.annotation_id)?;
            validate_text(&w.text)?;
            Ok(DecodedCandidate::Update(
                ProposedEdit::UpdateText { id, text: w.text },
                id,
                w.confidence,
                w.label,
                w.rationale,
            ))
        }
        "updateNumberPoints" => {
            let w: WireUpdateNumberPoints =
                serde_json::from_value(raw.clone()).map_err(|_| OutputError::Malformed {
                    code: "unknown_fields_or_shape",
                })?;
            validate_metadata(w.confidence, &w.label, w.rationale.as_deref())?;
            let id = parse_annotation_id(&w.annotation_id)?;
            let tip = point(w.tip)?;
            let bubble = point(w.bubble)?;
            Ok(DecodedCandidate::Update(
                ProposedEdit::UpdateNumberPoints { id, tip, bubble },
                id,
                w.confidence,
                w.label,
                w.rationale,
            ))
        }
        "delete" => {
            let w: WireDelete =
                serde_json::from_value(raw.clone()).map_err(|_| OutputError::Malformed {
                    code: "unknown_fields_or_shape",
                })?;
            validate_metadata(w.confidence, &w.label, w.rationale.as_deref())?;
            let id = parse_annotation_id(&w.annotation_id)?;
            Ok(DecodedCandidate::Delete(
                id,
                w.confidence,
                w.label,
                w.rationale,
            ))
        }
        _ => Err(OutputError::Malformed {
            code: "unknown_kind",
        }),
    }
}

fn kind_for_candidate(cand: &DecodedCandidate) -> ProposedEditKind {
    match cand {
        DecodedCandidate::Add(edit, _, _, _) => match edit {
            ProposedEdit::AddRedaction { .. } => ProposedEditKind::AddRedaction,
            ProposedEdit::AddTextNote { .. } => ProposedEditKind::AddTextNote,
            ProposedEdit::AddNumberCallout { .. } => ProposedEditKind::AddNumberCallout,
            _ => unreachable!(),
        },
        DecodedCandidate::Update(edit, _, _, _, _) => match edit {
            ProposedEdit::UpdateRedactionBounds { .. } => ProposedEditKind::UpdateRedactionBounds,
            ProposedEdit::UpdateTextPosition { .. } => ProposedEditKind::UpdateTextPosition,
            ProposedEdit::UpdateText { .. } => ProposedEditKind::UpdateText,
            ProposedEdit::UpdateNumberPoints { .. } => ProposedEditKind::UpdateNumberPoints,
            _ => unreachable!(),
        },
        DecodedCandidate::Delete(_, _, _, _) => ProposedEditKind::Delete,
    }
}

fn annotation_id_for_candidate(cand: &DecodedCandidate) -> Option<AnnotationId> {
    match cand {
        DecodedCandidate::Update(_, id, _, _, _) => Some(*id),
        DecodedCandidate::Delete(id, _, _, _) => Some(*id),
        DecodedCandidate::Add(_, _, _, _) => None,
    }
}

pub fn decode_proposal(
    json: &str,
    input: &AutomationInput,
    context: &ProposalContext,
    policy: &ExecutionPolicy,
) -> Result<EditProposal, OutputError> {
    if json.len() > policy.max_output_bytes {
        return Err(OutputError::TooLarge);
    }

    let envelope: OutputEnvelope =
        serde_json::from_str(json).map_err(|_| OutputError::Malformed {
            code: "invalid_envelope",
        })?;

    let mut decoded = Vec::with_capacity(envelope.candidates.len());
    for raw in &envelope.candidates {
        decoded.push(decode_one_candidate(raw)?);
    }

    let input_annotation_ids = input
        .annotations
        .iter()
        .map(|annotation| annotation.id)
        .collect::<std::collections::BTreeSet<_>>();
    for cand in &decoded {
        let kind = kind_for_candidate(cand);
        if !policy.allowed_edit_kinds.contains(&kind) {
            return Err(OutputError::EditKindDenied { kind });
        }
        if let Some(id) = annotation_id_for_candidate(cand) {
            if !policy.allowed_annotation_ids.contains(&id) || !input_annotation_ids.contains(&id) {
                return Err(OutputError::AnnotationDenied { id });
            }
        }
    }

    let mut candidates = Vec::with_capacity(decoded.len());
    let mut confidences = Vec::with_capacity(decoded.len());

    for (index, cand) in decoded.into_iter().enumerate() {
        let id = CandidateId(index as u64 + 1);
        let (edit, confidence, label, rationale) = match cand {
            DecodedCandidate::Add(e, c, l, r) => (e, c, l, r),
            DecodedCandidate::Update(e, _, c, l, r) => (e, c, l, r),
            DecodedCandidate::Delete(ann_id, c, l, r) => {
                (ProposedEdit::Delete { id: ann_id }, c, l, r)
            }
        };

        confidences.push(confidence);
        candidates.push(ProposedCandidate {
            id,
            edit,
            confidence,
            label,
            rationale,
            provenance: context.provenance.clone(),
        });
    }

    validate_policy(
        &candidates,
        &policy.proposal_limits,
        (input.image_width, input.image_height),
    )?;

    Ok(EditProposal {
        id: context.proposal_id,
        base_document_state_id: context.base_document_state_id,
        candidates,
        confidence_summary: ConfidenceSummary::from_confidences(&confidences),
        rationale_summary: None,
        provenance: context.provenance.clone(),
    })
}
