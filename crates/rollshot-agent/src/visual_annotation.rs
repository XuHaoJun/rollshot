//! Bounded multi-primitive visual annotation agent profile.
//!
//! Defines the public types, terminal payload decoder, and tight budget for
//! the single-shot visual annotation suggestion runner. The runner itself
//! lives in `crate::driver` as `AgentRunner::run_visual_annotation_with_provider`.

use std::sync::Arc;

use serde::Deserialize;

use crate::model::ToolDefinition;
use crate::runtime::{BudgetDimension, RunBudget};
use crate::tools::{tool_schema, Tool, ToolFuture, ToolOutcome};

// ---------- Public terminal types ----------

/// Normalized position in image-fraction coordinates `0.0..=1.0`.
#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedPoint {
    pub x: f32,
    pub y: f32,
}

/// Normalized bounding rectangle in image-fraction coordinates `0.0..=1.0`.
#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// One visual annotation suggestion the agent proposed for the reviewed
/// keyframe. Coordinates are normalized image-fraction values in `0.0..=1.0`.
///
/// `Suggested` carries normalized drafts only — never provider payload or
/// image bytes.
#[derive(Debug, Clone, PartialEq)]
pub enum VisualAnnotationDraft {
    NumberCallout {
        id: u32,
        tip: NormalizedPoint,
        bubble: NormalizedPoint,
        confidence: f32,
        rationale: Option<String>,
    },
    TextNote {
        id: u32,
        position: NormalizedPoint,
        text: String,
        confidence: f32,
        rationale: Option<String>,
    },
    OpaqueRedaction {
        id: u32,
        bounds: NormalizedRect,
        confidence: f32,
        rationale: Option<String>,
    },
}

/// Agent reported that no annotation is appropriate for this step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisualAnnotationNoSuggestion {
    NoClearTarget { reason: Option<String> },
}

/// All possible terminal outcomes of one bounded visual annotation run.
///
/// Terminal values carry no provider payload, no prompt text, and no
/// attachment bytes — they are the Rollshot-owned handoff to the app layer.
#[derive(Debug, Clone, PartialEq)]
pub enum VisualAnnotationRunTerminal {
    Suggested(Vec<VisualAnnotationDraft>),
    NoSuggestion(VisualAnnotationNoSuggestion),
    Cancelled,
    BudgetExhausted { dimension: BudgetDimension },
    ProviderFailure,
    ProtocolFailure,
    /// The authority snapshot does not grant the required operation.
    AuthorityDenied {
        operation: crate::authority::RunOperation,
    },
    /// Required audit evidence could not be durably appended.
    AuditFailure {
        category: crate::audit::AuditFailureCategory,
    },
}

// ---------- Internal tagged schema (private) ----------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum InternalDraft {
    NumberCallout {
        id: u32,
        tip: InternalPoint,
        bubble: InternalPoint,
        confidence: f32,
        rationale: Option<String>,
    },
    TextNote {
        id: u32,
        position: InternalPoint,
        text: String,
        confidence: f32,
        rationale: Option<String>,
    },
    OpaqueRedaction {
        id: u32,
        bounds: InternalRect,
        confidence: f32,
        rationale: Option<String>,
    },
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct InternalPoint {
    x: f32,
    y: f32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct InternalRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct SubmitVisualAnnotationArgs {
    suggestions: Vec<InternalDraft>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
enum SubmitVisualAnnotationTerminal {
    NoSuggestion { reason: Option<String> },
}

/// Wrapper that accepts either the suggestions batch or the no-suggestion
/// terminal. This mirrors the callout pattern but extends to a batch.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
enum SubmitVisualAnnotationPayload {
    Batch(SubmitVisualAnnotationArgs),
    Terminal(SubmitVisualAnnotationTerminal),
}

// Maximum length of optional `rationale` / `reason` text (trimmed).
const MAX_TEXT_CHARS: usize = 500;

/// Maximum number of suggestions in a single batch.
const MAX_BATCH_SIZE: usize = 10;

/// Advertised tool name.
pub const SUBMIT_VISUAL_ANNOTATION_SUGGESTIONS: &str = "submit_visual_annotation_suggestions";

// ---------- Public budget constant ----------

/// Tight visual annotation budget: same as callout — 2 model calls,
/// 1 attachment, 1 tool call, 30s wall.
pub fn visual_annotation_run_budget() -> RunBudget {
    RunBudget {
        wall_time: std::time::Duration::from_secs(30),
        model_calls: 2,
        input_tokens: 32_000,
        output_tokens: 1_000,
        cost: f64::MAX,
        tool_calls: 1,
        per_tool_calls: 1,
        argument_bytes: 4_096,
        result_bytes: 4_096,
        source_bytes: 0,
        attachments: 1,
        validation_attempts: 0,
        dry_run_attempts: 0,
        capability_calls: 0,
        candidate_count: 0,
        affected_area: 0,
    }
}

// ---------- Public decoder ----------

/// Parse a tool-call payload into normalized visual annotation drafts.
///
/// Validates the entire untrusted batch before it reaches UI — never
/// applies a partial valid subset. Rejects empty batches, oversized
/// batches, out-of-range coordinates, extra fields, and incorrect
/// kind-specific fields.
pub fn parse_visual_annotation_tool_args(
    value: &serde_json::Value,
) -> Result<Vec<VisualAnnotationDraft>, String> {
    let payload: SubmitVisualAnnotationPayload = serde_json::from_value(value.clone())
        .map_err(|e| format!("invalid terminal payload: {e}"))?;

    match payload {
        SubmitVisualAnnotationPayload::Terminal(SubmitVisualAnnotationTerminal::NoSuggestion {
            reason,
        }) => {
            // Return empty — caller maps to NoSuggestion.
            // Actually, we need to distinguish empty-batch from no-suggestion.
            // This function is for batch parsing only; the terminal no_suggestion
            // is handled at a higher level.
            let _ = sanitize_optional_text(reason, "reason")?;
            Err("use decode_visual_annotation_terminal for no_suggestion".into())
        }
        SubmitVisualAnnotationPayload::Batch(args) => {
            if args.suggestions.is_empty() {
                return Err("batch must contain at least one suggestion".to_string());
            }
            if args.suggestions.len() > MAX_BATCH_SIZE {
                return Err(format!(
                    "batch size {} exceeds maximum of {}",
                    args.suggestions.len(),
                    MAX_BATCH_SIZE
                ));
            }

            // Validate the ENTIRE batch before returning any results.
            let mut drafts = Vec::with_capacity(args.suggestions.len());
            for item in args.suggestions {
                drafts.push(validate_draft(item)?);
            }
            Ok(drafts)
        }
    }
}

/// Decode a terminal tool call payload (batch or no-suggestion).
pub fn decode_visual_annotation_terminal(
    value: &serde_json::Value,
) -> Result<VisualAnnotationRunTerminal, String> {
    // Try batch first.
    if value.get("suggestions").is_some() {
        let drafts = parse_visual_annotation_tool_args(value)?;
        return Ok(VisualAnnotationRunTerminal::Suggested(drafts));
    }

    // Try no_suggestion terminal.
    let terminal: SubmitVisualAnnotationTerminal = serde_json::from_value(value.clone())
        .map_err(|e| format!("invalid terminal payload: {e}"))?;
    match terminal {
        SubmitVisualAnnotationTerminal::NoSuggestion { reason } => {
            let reason = sanitize_optional_text(reason, "reason")?;
            Ok(VisualAnnotationRunTerminal::NoSuggestion(
                VisualAnnotationNoSuggestion::NoClearTarget { reason },
            ))
        }
    }
}

fn validate_draft(draft: InternalDraft) -> Result<VisualAnnotationDraft, String> {
    match draft {
        InternalDraft::NumberCallout {
            id,
            tip,
            bubble,
            confidence,
            rationale,
        } => {
            validate_point(&tip, "tip")?;
            validate_point(&bubble, "bubble")?;
            validate_confidence(confidence)?;
            let rationale = sanitize_optional_text(rationale, "rationale")?;
            Ok(VisualAnnotationDraft::NumberCallout {
                id,
                tip: NormalizedPoint { x: tip.x, y: tip.y },
                bubble: NormalizedPoint {
                    x: bubble.x,
                    y: bubble.y,
                },
                confidence,
                rationale,
            })
        }
        InternalDraft::TextNote {
            id,
            position,
            text,
            confidence,
            rationale,
        } => {
            validate_point(&position, "position")?;
            validate_confidence(confidence)?;
            let text = text.trim().to_string();
            if text.is_empty() {
                return Err("text_note text must not be empty".to_string());
            }
            if text.chars().count() > MAX_TEXT_CHARS {
                return Err(format!("text exceeds {MAX_TEXT_CHARS} characters"));
            }
            let rationale = sanitize_optional_text(rationale, "rationale")?;
            Ok(VisualAnnotationDraft::TextNote {
                id,
                position: NormalizedPoint {
                    x: position.x,
                    y: position.y,
                },
                text,
                confidence,
                rationale,
            })
        }
        InternalDraft::OpaqueRedaction {
            id,
            bounds,
            confidence,
            rationale,
        } => {
            validate_rect(&bounds)?;
            validate_confidence(confidence)?;
            let rationale = sanitize_optional_text(rationale, "rationale")?;
            Ok(VisualAnnotationDraft::OpaqueRedaction {
                id,
                bounds: NormalizedRect {
                    x: bounds.x,
                    y: bounds.y,
                    width: bounds.width,
                    height: bounds.height,
                },
                confidence,
                rationale,
            })
        }
    }
}

fn validate_point(point: &InternalPoint, field: &str) -> Result<(), String> {
    if !point.x.is_finite() || !point.y.is_finite() {
        return Err(format!("{field} coordinates must be finite"));
    }
    if !(0.0..=1.0).contains(&point.x) || !(0.0..=1.0).contains(&point.y) {
        return Err(format!("{field} coordinates must be in 0.0..=1.0"));
    }
    Ok(())
}

fn validate_rect(rect: &InternalRect) -> Result<(), String> {
    if !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.width.is_finite()
        || !rect.height.is_finite()
    {
        return Err("bounds coordinates must be finite".to_string());
    }
    if !(0.0..=1.0).contains(&rect.x)
        || !(0.0..=1.0).contains(&rect.y)
        || !(0.0..=1.0).contains(&rect.width)
        || !(0.0..=1.0).contains(&rect.height)
    {
        return Err("bounds values must be in 0.0..=1.0".to_string());
    }
    Ok(())
}

fn validate_confidence(confidence: f32) -> Result<(), String> {
    if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
        return Err("confidence must be finite and within 0..=1".to_string());
    }
    Ok(())
}

fn sanitize_optional_text(value: Option<String>, field: &str) -> Result<Option<String>, String> {
    let Some(s) = value else {
        return Ok(None);
    };
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > MAX_TEXT_CHARS {
        return Err(format!(
            "{field} exceeds {MAX_TEXT_CHARS} characters after trimming"
        ));
    }
    Ok(Some(trimmed.to_string()))
}

// ---------- Public tool definition ----------

/// Build the tool definition Rollshot advertises to the model.
pub fn submit_visual_annotation_suggestions_definition() -> ToolDefinition {
    let mut schema = tool_schema::<SubmitVisualAnnotationPayload>();
    enforce_additional_properties_false(&mut schema);
    ToolDefinition {
        name: SUBMIT_VISUAL_ANNOTATION_SUGGESTIONS.to_string(),
        description:
            "Submit visual annotation suggestions: a batch of NumberCallout, TextNote, and/or OpaqueRedaction drafts, or `no_suggestion` if nothing is appropriate. Do not output any prose outside this call."
                .to_string(),
        parameters: schema,
    }
}

fn enforce_additional_properties_false(value: &mut serde_json::Value) {
    let serde_json::Value::Object(map) = value else {
        return;
    };
    if map.get("type") == Some(&serde_json::Value::String("object".into())) {
        map.entry("additionalProperties".to_string())
            .or_insert(serde_json::Value::Bool(false));
    }
    if let Some(serde_json::Value::Object(props_map)) = map.get_mut("properties") {
        for (_, prop_value) in props_map.iter_mut() {
            enforce_additional_properties_false(prop_value);
        }
    }
}

// ---------- Stub tool used by the runner ----------

pub(crate) struct SubmitVisualAnnotationSuggestionsTool;

impl Tool for SubmitVisualAnnotationSuggestionsTool {
    fn name(&self) -> &str {
        SUBMIT_VISUAL_ANNOTATION_SUGGESTIONS
    }

    fn json_schema(&self) -> serde_json::Value {
        let mut schema = tool_schema::<SubmitVisualAnnotationPayload>();
        enforce_additional_properties_false(&mut schema);
        schema
    }

    fn call<'a>(&'a self, arguments: &'a serde_json::Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let result = decode_visual_annotation_terminal(arguments);
            match result {
                Ok(_) => Ok(ToolOutcome::Success {
                    result_json: serde_json::json!({"submitted": true}),
                }),
                Err(e) => Ok(ToolOutcome::Recoverable {
                    error: format!("invalid terminal payload: {e}"),
                }),
            }
        })
    }
}

pub(crate) fn submit_visual_annotation_suggestions_tool_arc() -> Arc<dyn Tool> {
    Arc::new(SubmitVisualAnnotationSuggestionsTool)
}

// ---------- Tests ----------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // ---- Terminal type invariants ----

    #[test]
    fn visual_annotation_run_terminal_variants_are_distinguishable() {
        let variants = [
            VisualAnnotationRunTerminal::Cancelled,
            VisualAnnotationRunTerminal::ProviderFailure,
            VisualAnnotationRunTerminal::ProtocolFailure,
            VisualAnnotationRunTerminal::BudgetExhausted {
                dimension: BudgetDimension::Attachments,
            },
            VisualAnnotationRunTerminal::NoSuggestion(
                VisualAnnotationNoSuggestion::NoClearTarget { reason: None },
            ),
            VisualAnnotationRunTerminal::Suggested(vec![]),
        ];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    // ---- Decoder: valid batch with all three kinds ----

    #[test]
    fn decodes_normalized_visual_annotation_batch() {
        let drafts = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"number_callout","tip":{"x":0.1,"y":0.2},
                 "bubble":{"x":0.4,"y":0.2},"confidence":0.8,"rationale":null},
                {"id":2,"kind":"text_note","position":{"x":0.3,"y":0.4},
                 "text":"Click Save","confidence":0.9,"rationale":"Visible action"},
                {"id":3,"kind":"opaque_redaction","bounds":{"x":0.5,"y":0.1,"width":0.2,"height":0.1},
                 "confidence":0.7,"rationale":"Account data"}
            ]
        }))
        .unwrap();
        assert_eq!(drafts.len(), 3);
        match &drafts[0] {
            VisualAnnotationDraft::NumberCallout {
                id,
                tip,
                bubble,
                confidence,
                rationale,
            } => {
                assert_eq!(*id, 1);
                assert_eq!(tip.x, 0.1);
                assert_eq!(tip.y, 0.2);
                assert_eq!(bubble.x, 0.4);
                assert_eq!(bubble.y, 0.2);
                assert_eq!(*confidence, 0.8);
                assert_eq!(*rationale, None);
            }
            other => panic!("expected NumberCallout, got {other:?}"),
        }
        match &drafts[1] {
            VisualAnnotationDraft::TextNote {
                id,
                position,
                text,
                confidence,
                rationale,
            } => {
                assert_eq!(*id, 2);
                assert_eq!(position.x, 0.3);
                assert_eq!(position.y, 0.4);
                assert_eq!(text, "Click Save");
                assert_eq!(*confidence, 0.9);
                assert_eq!(rationale.as_deref(), Some("Visible action"));
            }
            other => panic!("expected TextNote, got {other:?}"),
        }
        match &drafts[2] {
            VisualAnnotationDraft::OpaqueRedaction {
                id,
                bounds,
                confidence,
                rationale,
            } => {
                assert_eq!(*id, 3);
                assert_eq!(bounds.x, 0.5);
                assert_eq!(bounds.y, 0.1);
                assert_eq!(bounds.width, 0.2);
                assert_eq!(bounds.height, 0.1);
                assert_eq!(*confidence, 0.7);
                assert_eq!(rationale.as_deref(), Some("Account data"));
            }
            other => panic!("expected OpaqueRedaction, got {other:?}"),
        }
    }

    // ---- Decoder: empty batch rejected ----

    #[test]
    fn decoder_rejects_empty_batch() {
        let err = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": []
        }))
        .unwrap_err();
        assert!(err.contains("at least one"), "got: {err}");
    }

    // ---- Decoder: oversized batch rejected ----

    #[test]
    fn decoder_rejects_oversized_batch() {
        let suggestions: Vec<serde_json::Value> = (0..=MAX_BATCH_SIZE)
            .map(|i| {
                serde_json::json!({
                    "id": i,
                    "kind": "number_callout",
                    "tip": {"x": 0.5, "y": 0.5},
                    "bubble": {"x": 0.6, "y": 0.5},
                    "confidence": 0.5
                })
            })
            .collect();
        let err = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": suggestions
        }))
        .unwrap_err();
        assert!(err.contains("exceeds maximum"), "got: {err}");
    }

    // ---- Decoder: extra fields rejected ----

    #[test]
    fn decoder_rejects_extra_fields_in_number_callout() {
        let err = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"number_callout","tip":{"x":0.5,"y":0.5},
                 "bubble":{"x":0.6,"y":0.5},"confidence":0.5,"extra":true}
            ]
        }))
        .unwrap_err();
        assert!(err.contains("invalid"), "got: {err}");
    }

    #[test]
    fn decoder_rejects_extra_fields_in_tip() {
        let err = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"number_callout","tip":{"x":0.5,"y":0.5,"z":0.1},
                 "bubble":{"x":0.6,"y":0.5},"confidence":0.5}
            ]
        }))
        .unwrap_err();
        assert!(err.contains("invalid"), "got: {err}");
    }

    // ---- Decoder: out-of-range coordinates rejected ----

    #[test]
    fn decoder_rejects_out_of_range_x_in_tip() {
        let err = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"number_callout","tip":{"x":1.5,"y":0.5},
                 "bubble":{"x":0.6,"y":0.5},"confidence":0.5}
            ]
        }))
        .unwrap_err();
        assert!(err.contains("0.0..=1.0"), "got: {err}");
    }

    #[test]
    fn decoder_rejects_negative_y_in_position() {
        let err = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"text_note","position":{"x":0.5,"y":-0.1},
                 "text":"hello","confidence":0.5}
            ]
        }))
        .unwrap_err();
        assert!(err.contains("0.0..=1.0"), "got: {err}");
    }

    #[test]
    fn decoder_rejects_out_of_range_bounds() {
        let err = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"opaque_redaction","bounds":{"x":0.5,"y":0.5,"width":1.5,"height":0.5},
                 "confidence":0.5}
            ]
        }))
        .unwrap_err();
        assert!(err.contains("0.0..=1.0"), "got: {err}");
    }

    // ---- Decoder: incorrect kind-specific fields rejected ----

    #[test]
    fn decoder_rejects_wrong_fields_for_number_callout() {
        // text_note fields on a number_callout
        let err = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"number_callout","position":{"x":0.5,"y":0.5},
                 "text":"hello","confidence":0.5}
            ]
        }))
        .unwrap_err();
        assert!(err.contains("invalid"), "got: {err}");
    }

    // ---- Decoder: out-of-range confidence rejected ----

    #[test]
    fn decoder_rejects_confidence_above_one() {
        let err = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"number_callout","tip":{"x":0.5,"y":0.5},
                 "bubble":{"x":0.6,"y":0.5},"confidence":1.5}
            ]
        }))
        .unwrap_err();
        assert!(err.contains("confidence"), "got: {err}");
    }

    #[test]
    fn decoder_rejects_negative_confidence() {
        let err = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"text_note","position":{"x":0.5,"y":0.5},
                 "text":"hello","confidence":-0.1}
            ]
        }))
        .unwrap_err();
        assert!(err.contains("confidence"), "got: {err}");
    }

    // ---- Decoder: empty text rejected for TextNote ----

    #[test]
    fn decoder_rejects_empty_text_for_text_note() {
        let err = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"text_note","position":{"x":0.5,"y":0.5},
                 "text":"   ","confidence":0.5}
            ]
        }))
        .unwrap_err();
        assert!(err.contains("text"), "got: {err}");
    }

    // ---- Decoder: oversized text rejected ----

    #[test]
    fn decoder_rejects_oversized_text() {
        let oversized = "x".repeat(MAX_TEXT_CHARS + 1);
        let err = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"text_note","position":{"x":0.5,"y":0.5},
                 "text":oversized,"confidence":0.5}
            ]
        }))
        .unwrap_err();
        assert!(err.contains("text"), "got: {err}");
    }

    // ---- Decoder: whole batch rejected if any item invalid ----

    #[test]
    fn decoder_rejects_entire_batch_on_single_invalid_item() {
        let result = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"number_callout","tip":{"x":0.5,"y":0.5},
                 "bubble":{"x":0.6,"y":0.5},"confidence":0.5},
                {"id":2,"kind":"text_note","position":{"x":0.5,"y":0.5},
                 "text":"valid","confidence":1.5}
            ]
        }));
        assert!(result.is_err(), "should reject entire batch");
        // First valid item must NOT be returned.
        assert!(result.unwrap_err().contains("confidence"));
    }

    // ---- Decoder: boundary confidence accepted ----

    #[test]
    fn decoder_accepts_boundary_confidence() {
        for conf in [0.0_f32, 1.0_f32] {
            let result = parse_visual_annotation_tool_args(&serde_json::json!({
                "suggestions": [
                    {"id":1,"kind":"number_callout","tip":{"x":0.5,"y":0.5},
                     "bubble":{"x":0.6,"y":0.5},"confidence":conf}
                ]
            }));
            assert!(result.is_ok(), "confidence {conf} should be accepted");
        }
    }

    // ---- Decoder: boundary coordinates accepted ----

    #[test]
    fn decoder_accepts_boundary_coordinates() {
        let result = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"number_callout","tip":{"x":0.0,"y":1.0},
                 "bubble":{"x":1.0,"y":0.0},"confidence":0.5}
            ]
        }));
        assert!(result.is_ok());
    }

    // ---- Decoder: trim rationale to None when empty ----

    #[test]
    fn decoder_trims_empty_rationale_to_none() {
        let result = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"number_callout","tip":{"x":0.5,"y":0.5},
                 "bubble":{"x":0.6,"y":0.5},"confidence":0.5,"rationale":"  "}
            ]
        }))
        .unwrap();
        match &result[0] {
            VisualAnnotationDraft::NumberCallout { rationale, .. } => {
                assert_eq!(*rationale, None);
            }
            other => panic!("expected NumberCallout, got {other:?}"),
        }
    }

    // ---- Decoder: oversized rationale rejected ----

    #[test]
    fn decoder_rejects_oversized_rationale() {
        let oversized = "x".repeat(MAX_TEXT_CHARS + 1);
        let err = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"number_callout","tip":{"x":0.5,"y":0.5},
                 "bubble":{"x":0.6,"y":0.5},"confidence":0.5,"rationale":oversized}
            ]
        }))
        .unwrap_err();
        assert!(err.contains("rationale"), "got: {err}");
    }

    // ---- Decoder: unknown kind rejected ----

    #[test]
    fn decoder_rejects_unknown_kind() {
        let err = parse_visual_annotation_tool_args(&serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"unknown_kind","x":0.5,"y":0.5}
            ]
        }))
        .unwrap_err();
        assert!(err.contains("invalid"), "got: {err}");
    }

    // ---- Budget constant ----

    #[test]
    fn visual_annotation_run_budget_matches_brief() {
        let b = visual_annotation_run_budget();
        assert_eq!(b.wall_time, std::time::Duration::from_secs(30));
        assert_eq!(b.model_calls, 2);
        assert_eq!(b.input_tokens, 32_000);
        assert_eq!(b.output_tokens, 1_000);
        assert_eq!(b.tool_calls, 1);
        assert_eq!(b.per_tool_calls, 1);
        assert_eq!(b.argument_bytes, 4_096);
        assert_eq!(b.result_bytes, 4_096);
        assert_eq!(b.attachments, 1);
    }

    // ---- Tool definition ----

    #[test]
    fn tool_definition_uses_canonical_name() {
        let def = submit_visual_annotation_suggestions_definition();
        assert_eq!(def.name, SUBMIT_VISUAL_ANNOTATION_SUGGESTIONS);
        assert_eq!(def.name, "submit_visual_annotation_suggestions");
    }

    #[test]
    fn tool_definition_has_additional_properties_false_at_every_object_level() {
        let def = submit_visual_annotation_suggestions_definition();
        assert_every_object_has_additional_properties_false(&def.parameters);
    }

    fn assert_every_object_has_additional_properties_false(value: &serde_json::Value) {
        let serde_json::Value::Object(map) = value else {
            return;
        };
        if map.get("type") == Some(&serde_json::Value::String("object".into())) {
            assert_eq!(
                map.get("additionalProperties"),
                Some(&serde_json::Value::Bool(false)),
                "object level missing additionalProperties: false: {value}"
            );
        }
        if let Some(serde_json::Value::Object(props)) = map.get("properties") {
            for (_, child) in props.iter() {
                assert_every_object_has_additional_properties_false(child);
            }
        }
    }

    // ---- decode_visual_annotation_terminal ----

    #[test]
    fn decode_terminal_batch_returns_suggested() {
        let value = serde_json::json!({
            "suggestions": [
                {"id":1,"kind":"number_callout","tip":{"x":0.1,"y":0.2},
                 "bubble":{"x":0.4,"y":0.2},"confidence":0.8}
            ]
        });
        let terminal = decode_visual_annotation_terminal(&value).unwrap();
        match terminal {
            VisualAnnotationRunTerminal::Suggested(drafts) => {
                assert_eq!(drafts.len(), 1);
            }
            other => panic!("expected Suggested, got {other:?}"),
        }
    }

    #[test]
    fn decode_terminal_no_suggestion() {
        let value = serde_json::json!({
            "result": "no_suggestion",
            "reason": "no clear target"
        });
        let terminal = decode_visual_annotation_terminal(&value).unwrap();
        match terminal {
            VisualAnnotationRunTerminal::NoSuggestion(
                VisualAnnotationNoSuggestion::NoClearTarget { reason },
            ) => {
                assert_eq!(reason.as_deref(), Some("no clear target"));
            }
            other => panic!("expected NoSuggestion, got {other:?}"),
        }
    }

    // ---- Lifecycle tests (Rig-driven runner) ----

    pub(crate) mod lifecycle {
        use super::*;
        use crate::domain::{AttachmentDescriptor, AuthorizedModelInput, MediaType};
        use crate::driver::{AgentConfig, AgentRunner};
        use crate::model::{ModelCompletion, ModelStreamEvent, ModelUsage, StopReason};
        use crate::provider::{ProviderAdapter, StreamBounds};
        use crate::runtime::RunCancellation;
        use std::collections::VecDeque;
        use std::pin::Pin;
        use std::sync::Mutex;

        pub(crate) struct ScriptedProvider {
            requests: Mutex<Vec<crate::model::ModelRequest>>,
            scripts: Mutex<VecDeque<Vec<ModelStreamEvent>>>,
        }

        impl ScriptedProvider {
            pub(crate) fn new(scripts: Vec<Vec<ModelStreamEvent>>) -> Self {
                Self {
                    requests: Mutex::new(Vec::new()),
                    scripts: Mutex::new(VecDeque::from(scripts)),
                }
            }

            /// Number of provider requests made so far.
            pub(crate) fn request_count(&self) -> usize {
                self.requests.lock().unwrap().len()
            }
        }

        impl ProviderAdapter for ScriptedProvider {
            fn stream(
                &self,
                request: crate::model::ModelRequest,
                _bounds: StreamBounds,
            ) -> Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<
                                Pin<
                                    Box<
                                        dyn futures_util::Stream<
                                                Item = Result<
                                                    ModelStreamEvent,
                                                    crate::model::ModelError,
                                                >,
                                            > + Send,
                                    >,
                                >,
                                crate::model::ModelError,
                            >,
                        > + Send
                        + '_,
                >,
            > {
                self.requests.lock().unwrap().push(request);
                let events = self.scripts.lock().unwrap().pop_front().unwrap_or_default();
                Box::pin(async move {
                    let s = futures_util::stream::iter(events.into_iter().map(Ok));
                    Ok(Box::pin(s)
                        as Pin<
                            Box<
                                dyn futures_util::Stream<
                                        Item = Result<ModelStreamEvent, crate::model::ModelError>,
                                    > + Send,
                            >,
                        >)
                })
            }
        }

        fn authorized_input_with_one_png() -> AuthorizedModelInput {
            AuthorizedModelInput::new(
                "anthropic".into(),
                "vision-model".into(),
                "suggest visual annotations".into(),
                vec![AttachmentDescriptor {
                    media_type: MediaType::Png,
                    width: 1,
                    height: 1,
                    byte_count: 4,
                }],
                vec![vec![0x89, 0x50, 0x4E, 0x47]],
            )
            .expect("valid input")
        }

        pub(crate) fn completion_event(stop: StopReason) -> ModelStreamEvent {
            ModelStreamEvent::Completed(ModelCompletion {
                usage: ModelUsage {
                    input_tokens: 5,
                    output_tokens: 3,
                    total_tokens: 8,
                },
                stop_reason: stop,
            })
        }

        pub(crate) fn tool_call_turn(id: &str, name: &str, args: &str) -> Vec<ModelStreamEvent> {
            vec![
                ModelStreamEvent::ToolCallStart {
                    id: id.to_string(),
                    name: name.to_string(),
                },
                ModelStreamEvent::ToolCallArgumentDelta {
                    id: id.to_string(),
                    delta: args.to_string(),
                },
                completion_event(StopReason::ToolUse),
            ]
        }

        pub(crate) fn text_turn(text: &str) -> Vec<ModelStreamEvent> {
            vec![
                ModelStreamEvent::TextDelta(text.to_string()),
                completion_event(StopReason::EndTurn),
            ]
        }

        pub(crate) fn va_runner() -> AgentRunner {
            AgentRunner::new(AgentConfig {
                max_turns: 2,
                ..AgentConfig::default()
            })
        }

        pub(crate) fn va_profile() -> crate::driver::VisualAnnotationProfile<'static> {
            let skill = crate::skills::bundled_action_guide_visual_annotations_use()
                .expect("bundled visual skill must resolve");
            // Leak the SkillUse so the profile can borrow it with 'static.
            let skill: &'static crate::skills::SkillUse = Box::leak(Box::new(skill));
            crate::driver::VisualAnnotationProfile::from_skill(skill)
                .expect("bundled visual skill must be accepted")
        }

        #[tokio::test]
        async fn one_tool_call_returns_suggested() {
            let args = serde_json::json!({
                "suggestions": [
                    {"id":1,"kind":"number_callout","tip":{"x":0.5,"y":0.25},
                     "bubble":{"x":0.6,"y":0.25},"confidence":0.9,"rationale":"primary action"}
                ]
            })
            .to_string();
            let provider = ScriptedProvider::new(vec![tool_call_turn(
                "tc_1",
                "submit_visual_annotation_suggestions",
                &args,
            )]);
            let runner = va_runner();
            let input = authorized_input_with_one_png();
            let cancel = RunCancellation::new();

            let result = runner
                .run_visual_annotation_with_provider(
                    va_profile(),
                    input,
                    &provider,
                    visual_annotation_run_budget(),
                    &cancel,
                )
                .await;

            match result {
                VisualAnnotationRunTerminal::Suggested(drafts) => {
                    assert_eq!(drafts.len(), 1);
                    match &drafts[0] {
                        VisualAnnotationDraft::NumberCallout {
                            tip,
                            confidence,
                            rationale,
                            ..
                        } => {
                            assert_eq!(tip.x, 0.5);
                            assert_eq!(tip.y, 0.25);
                            assert_eq!(*confidence, 0.9);
                            assert_eq!(rationale.as_deref(), Some("primary action"));
                        }
                        other => panic!("expected NumberCallout, got {other:?}"),
                    }
                }
                other => panic!("expected Suggested, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn no_suggestion_returns_successful_terminal() {
            let args = serde_json::json!({
                "result": "no_suggestion",
                "reason": "no clear target"
            })
            .to_string();
            let provider = ScriptedProvider::new(vec![tool_call_turn(
                "tc_1",
                "submit_visual_annotation_suggestions",
                &args,
            )]);
            let runner = va_runner();
            let input = authorized_input_with_one_png();
            let cancel = RunCancellation::new();

            let result = runner
                .run_visual_annotation_with_provider(
                    va_profile(),
                    input,
                    &provider,
                    visual_annotation_run_budget(),
                    &cancel,
                )
                .await;

            match result {
                VisualAnnotationRunTerminal::NoSuggestion(
                    VisualAnnotationNoSuggestion::NoClearTarget { reason },
                ) => {
                    assert_eq!(reason.as_deref(), Some("no clear target"));
                }
                other => panic!("expected NoSuggestion, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn completion_without_submission_returns_protocol_failure() {
            let provider = ScriptedProvider::new(vec![text_turn("no tool call here")]);
            let runner = va_runner();
            let input = authorized_input_with_one_png();
            let cancel = RunCancellation::new();

            let result = runner
                .run_visual_annotation_with_provider(
                    va_profile(),
                    input,
                    &provider,
                    visual_annotation_run_budget(),
                    &cancel,
                )
                .await;

            assert_eq!(result, VisualAnnotationRunTerminal::ProtocolFailure);
        }

        #[tokio::test]
        async fn cancellation_before_stream_returns_cancelled() {
            let args = serde_json::json!({
                "suggestions": [
                    {"id":1,"kind":"number_callout","tip":{"x":0.5,"y":0.5},
                     "bubble":{"x":0.6,"y":0.5},"confidence":0.5}
                ]
            })
            .to_string();
            let provider = ScriptedProvider::new(vec![tool_call_turn(
                "tc_1",
                "submit_visual_annotation_suggestions",
                &args,
            )]);
            let runner = va_runner();
            let input = authorized_input_with_one_png();
            let cancel = RunCancellation::new();
            cancel.cancel();

            let result = runner
                .run_visual_annotation_with_provider(
                    va_profile(),
                    input,
                    &provider,
                    visual_annotation_run_budget(),
                    &cancel,
                )
                .await;

            assert_eq!(result, VisualAnnotationRunTerminal::Cancelled);
        }

        #[tokio::test]
        async fn attachment_budget_exceeded_returns_budget_exhausted() {
            let mut budget = visual_annotation_run_budget();
            budget.attachments = 0;
            let args = serde_json::json!({
                "suggestions": [
                    {"id":1,"kind":"number_callout","tip":{"x":0.5,"y":0.5},
                     "bubble":{"x":0.6,"y":0.5},"confidence":0.5}
                ]
            })
            .to_string();
            let provider = ScriptedProvider::new(vec![tool_call_turn(
                "tc_1",
                "submit_visual_annotation_suggestions",
                &args,
            )]);
            let runner = va_runner();
            let input = authorized_input_with_one_png();
            let cancel = RunCancellation::new();

            let result = runner
                .run_visual_annotation_with_provider(va_profile(), input, &provider, budget, &cancel)
                .await;

            match result {
                VisualAnnotationRunTerminal::BudgetExhausted {
                    dimension: BudgetDimension::Attachments,
                } => {}
                other => panic!("expected BudgetExhausted Attachments, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn first_request_carries_attachments_and_system_prompt() {
            let args = serde_json::json!({
                "suggestions": [
                    {"id":1,"kind":"number_callout","tip":{"x":0.5,"y":0.5},
                     "bubble":{"x":0.6,"y":0.5},"confidence":0.5}
                ]
            })
            .to_string();
            let provider = ScriptedProvider::new(vec![
                tool_call_turn("tc_1", "submit_visual_annotation_suggestions", &args),
                text_turn("should not be requested"),
            ]);
            let runner = va_runner();
            let input = authorized_input_with_one_png();
            let cancel = RunCancellation::new();

            let _ = runner
                .run_visual_annotation_with_provider(
                    va_profile(),
                    input,
                    &provider,
                    visual_annotation_run_budget(),
                    &cancel,
                )
                .await;

            let requests = provider.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            let first = &requests[0];
            assert_eq!(first.attachments.len(), 1);
            let system_prompt = first
                .system_prompt
                .as_deref()
                .expect("visual annotation system prompt");
            assert!(
                system_prompt.contains("submit_visual_annotation_suggestions"),
                "first request system prompt must reference the tool, got: {system_prompt}"
            );
            assert_eq!(
                system_prompt,
                crate::driver::VISUAL_ANNOTATION_SYSTEM_PROMPT_BASELINE,
                "model request system prompt must be byte-identical to the baseline"
            );
            assert_eq!(first.tool_definitions.len(), 1);
            assert_eq!(
                first.tool_definitions[0].name,
                "submit_visual_annotation_suggestions"
            );
        }

        #[tokio::test]
        async fn multi_primitive_batch_returns_all_drafts() {
            let args = serde_json::json!({
                "suggestions": [
                    {"id":1,"kind":"number_callout","tip":{"x":0.1,"y":0.2},
                     "bubble":{"x":0.4,"y":0.2},"confidence":0.8},
                    {"id":2,"kind":"text_note","position":{"x":0.3,"y":0.4},
                     "text":"Click Save","confidence":0.9},
                    {"id":3,"kind":"opaque_redaction","bounds":{"x":0.5,"y":0.1,"width":0.2,"height":0.1},
                     "confidence":0.7}
                ]
            })
            .to_string();
            let provider = ScriptedProvider::new(vec![tool_call_turn(
                "tc_1",
                "submit_visual_annotation_suggestions",
                &args,
            )]);
            let runner = va_runner();
            let input = authorized_input_with_one_png();
            let cancel = RunCancellation::new();

            let result = runner
                .run_visual_annotation_with_provider(
                    va_profile(),
                    input,
                    &provider,
                    visual_annotation_run_budget(),
                    &cancel,
                )
                .await;

            match result {
                VisualAnnotationRunTerminal::Suggested(drafts) => {
                    assert_eq!(drafts.len(), 3);
                    assert!(matches!(
                        &drafts[0],
                        VisualAnnotationDraft::NumberCallout { .. }
                    ));
                    assert!(matches!(&drafts[1], VisualAnnotationDraft::TextNote { .. }));
                    assert!(matches!(
                        &drafts[2],
                        VisualAnnotationDraft::OpaqueRedaction { .. }
                    ));
                }
                other => panic!("expected Suggested with 3 drafts, got {other:?}"),
            }
        }
    }
}
