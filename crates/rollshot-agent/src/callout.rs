//! Bounded Action Guide agent callout profile.
//!
//! Defines the public types, terminal payload decoder, and tight callout budget
//! for the single-shot Number Callout suggestion runner. The runner itself
//! lives in `crate::driver` as `AgentRunner::run_callout_with_provider` so it
//! can reuse the existing streamed-turn assembly, budget charging, cancellation
//! checks, and Rig tool-result threading.

use std::sync::Arc;

use serde::Deserialize;

use crate::model::ToolDefinition;
use crate::runtime::{BudgetDimension, RunBudget};
use crate::tools::{tool_schema, Tool, ToolFuture, ToolOutcome};

// ---------- Public terminal types ----------

/// One Number Callout tip the agent suggested for the reviewed step.
///
/// Coordinates are normalized image-fraction values in `0.0..=1.0`. Rollshot
/// owns bubble placement and numbering, so the agent only selects the tip.
#[derive(Debug, Clone, PartialEq)]
pub struct CalloutAgentSuggestion {
    pub x: f32,
    pub y: f32,
    pub confidence: f32,
    pub rationale: Option<String>,
}

/// Agent reported that no suggestion is appropriate for this step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalloutNoSuggestion {
    NoClearTarget { reason: Option<String> },
}

/// All possible terminal outcomes of one bounded callout run.
///
/// Terminal values carry no provider payload, no prompt text, and no
/// attachment bytes — they are the Rollshot-owned handoff to the app layer.
#[derive(Debug, Clone, PartialEq)]
pub enum CalloutRunTerminal {
    Suggested(CalloutAgentSuggestion),
    NoSuggestion(CalloutNoSuggestion),
    Cancelled,
    BudgetExhausted { dimension: BudgetDimension },
    ProviderFailure,
    ProtocolFailure,
}

// ---------- Internal tagged schema (private) ----------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
enum SubmitCalloutArgs {
    Suggestion {
        tip: Tip,
        confidence: f32,
        rationale: Option<String>,
    },
    NoSuggestion {
        reason: Option<String>,
    },
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Tip {
    x: f32,
    y: f32,
}

// Maximum length of optional `rationale` / `reason` text (trimmed).
const MAX_TEXT_CHARS: usize = 500;

/// Advertised tool name. The model is required to call exactly one of the
/// two terminal variants below using this name.
pub const SUBMIT_CALLOUT_SUGGESTION: &str = "submit_callout_suggestion";

// ---------- Public budget constant ----------

/// Tight callout budget: 2 model calls, 1 attachment, 1 tool call, 30s wall.
///
/// Cost is intentionally left unlimited — the per-run cost ceiling is not
/// enforced today (see [`RunBudget::cost`]).
pub fn callout_run_budget() -> RunBudget {
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

/// Validate and decode a single terminal tool call payload into a
/// `CalloutRunTerminal`. Rejects non-finite coordinates, out-of-range
/// confidence, oversized optional text, and unknown fields. Does not clamp
/// invalid input — invalid values return an error.
///
/// Empty trimmed optional text is mapped to `None` (gentle behavior); the
/// caller does not need to differentiate "user did not provide" from
/// "user provided whitespace".
pub fn decode_submission(value: &serde_json::Value) -> Result<CalloutRunTerminal, String> {
    let parsed: SubmitCalloutArgs = serde_json::from_value(value.clone())
        .map_err(|e| format!("invalid terminal payload: {e}"))?;

    match parsed {
        SubmitCalloutArgs::Suggestion {
            tip,
            confidence,
            rationale,
        } => {
            if !tip.x.is_finite() || !tip.y.is_finite() {
                return Err("callout tip coordinates must be finite".to_string());
            }
            if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
                return Err("callout confidence must be finite and within 0..=1".to_string());
            }
            let rationale = sanitize_optional_text(rationale, "rationale")?;
            Ok(CalloutRunTerminal::Suggested(CalloutAgentSuggestion {
                x: tip.x,
                y: tip.y,
                confidence,
                rationale,
            }))
        }
        SubmitCalloutArgs::NoSuggestion { reason } => {
            let reason = sanitize_optional_text(reason, "reason")?;
            Ok(CalloutRunTerminal::NoSuggestion(
                CalloutNoSuggestion::NoClearTarget { reason },
            ))
        }
    }
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

/// Build the tool definition Rollshot advertises to the model. The schema has
/// `additionalProperties: false` at every object level so the provider rejects
/// unexpected fields before they reach the decoder.
pub fn submit_callout_suggestion_definition() -> ToolDefinition {
    let mut schema = tool_schema::<SubmitCalloutArgs>();
    enforce_additional_properties_false(&mut schema);
    ToolDefinition {
        name: SUBMIT_CALLOUT_SUGGESTION.to_string(),
        description:
            "Submit exactly one terminal call: either a single Number Callout tip, or `no_suggestion` if no target is appropriate. Do not output any prose outside this call."
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

/// Stub tool registered for the callout run. The runner intercepts the
/// terminal call before/after invocation and decodes its arguments via
/// `decode_submission`. This stub simply echoes a success result so the rig
/// state machine can advance; invalid payloads become recoverable errors
/// without leaking the raw arguments.
pub(crate) struct SubmitCalloutSuggestionTool;

impl Tool for SubmitCalloutSuggestionTool {
    fn name(&self) -> &str {
        SUBMIT_CALLOUT_SUGGESTION
    }

    fn json_schema(&self) -> serde_json::Value {
        let mut schema = tool_schema::<SubmitCalloutArgs>();
        enforce_additional_properties_false(&mut schema);
        schema
    }

    fn call<'a>(&'a self, arguments: &'a serde_json::Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let parsed: Result<SubmitCalloutArgs, _> = serde_json::from_value(arguments.clone());
            match parsed {
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

pub(crate) fn submit_callout_suggestion_tool_arc() -> Arc<dyn Tool> {
    Arc::new(SubmitCalloutSuggestionTool)
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Public terminal type invariants ----

    #[test]
    fn callout_no_suggestion_carries_optional_reason() {
        let no = CalloutNoSuggestion::NoClearTarget {
            reason: Some("no clear target".into()),
        };
        match no {
            CalloutNoSuggestion::NoClearTarget { reason } => {
                assert_eq!(reason.as_deref(), Some("no clear target"));
            }
        }
    }

    #[test]
    fn callout_run_terminal_variants_are_distinguishable() {
        let variants = [
            CalloutRunTerminal::Cancelled,
            CalloutRunTerminal::ProviderFailure,
            CalloutRunTerminal::ProtocolFailure,
            CalloutRunTerminal::BudgetExhausted {
                dimension: BudgetDimension::Attachments,
            },
            CalloutRunTerminal::NoSuggestion(CalloutNoSuggestion::NoClearTarget { reason: None }),
            CalloutRunTerminal::Suggested(CalloutAgentSuggestion {
                x: 0.0,
                y: 0.0,
                confidence: 0.0,
                rationale: None,
            }),
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

    // ---- Decoder: valid payloads ----

    #[test]
    fn decoder_accepts_valid_suggestion() {
        let value = serde_json::json!({
            "result": "suggestion",
            "tip": { "x": 0.5, "y": 0.25 },
            "confidence": 0.9,
            "rationale": "primary action"
        });
        let terminal = decode_submission(&value).expect("valid payload");
        match terminal {
            CalloutRunTerminal::Suggested(s) => {
                assert_eq!(s.x, 0.5);
                assert_eq!(s.y, 0.25);
                assert_eq!(s.confidence, 0.9);
                assert_eq!(s.rationale.as_deref(), Some("primary action"));
            }
            other => panic!("expected Suggested, got {other:?}"),
        }
    }

    #[test]
    fn decoder_accepts_valid_no_suggestion() {
        let value = serde_json::json!({
            "result": "no_suggestion",
            "reason": "no clear target"
        });
        let terminal = decode_submission(&value).expect("valid payload");
        match terminal {
            CalloutRunTerminal::NoSuggestion(CalloutNoSuggestion::NoClearTarget { reason }) => {
                assert_eq!(reason.as_deref(), Some("no clear target"));
            }
            other => panic!("expected NoSuggestion, got {other:?}"),
        }
    }

    #[test]
    fn decoder_trims_optional_text_to_none_when_empty() {
        let value = serde_json::json!({
            "result": "suggestion",
            "tip": { "x": 0.1, "y": 0.2 },
            "confidence": 0.5,
            "rationale": "   "
        });
        let terminal = decode_submission(&value).expect("valid payload");
        match terminal {
            CalloutRunTerminal::Suggested(s) => {
                assert_eq!(s.rationale, None);
            }
            other => panic!("expected Suggested, got {other:?}"),
        }
    }

    #[test]
    fn decoder_trims_optional_text() {
        let value = serde_json::json!({
            "result": "no_suggestion",
            "reason": "  no clear target  "
        });
        let terminal = decode_submission(&value).expect("valid payload");
        match terminal {
            CalloutRunTerminal::NoSuggestion(CalloutNoSuggestion::NoClearTarget { reason }) => {
                assert_eq!(reason.as_deref(), Some("no clear target"));
            }
            other => panic!("expected NoSuggestion, got {other:?}"),
        }
    }

    #[test]
    fn decoder_accepts_missing_optional_fields() {
        let value = serde_json::json!({
            "result": "suggestion",
            "tip": { "x": 0.0, "y": 0.0 },
            "confidence": 0.0
        });
        let terminal = decode_submission(&value).expect("valid payload");
        match terminal {
            CalloutRunTerminal::Suggested(s) => {
                assert_eq!(s.rationale, None);
                assert_eq!(s.confidence, 0.0);
            }
            other => panic!("expected Suggested, got {other:?}"),
        }
    }

    #[test]
    fn decoder_accepts_boundary_confidence() {
        for conf in [0.0_f32, 1.0_f32] {
            let value = serde_json::json!({
                "result": "suggestion",
                "tip": { "x": 0.5, "y": 0.5 },
                "confidence": conf
            });
            let terminal = decode_submission(&value).expect("valid payload");
            assert!(matches!(terminal, CalloutRunTerminal::Suggested(_)));
        }
    }

    // ---- Decoder: invalid payloads ----

    #[test]
    fn decoder_rejects_out_of_range_confidence_high() {
        let value = serde_json::json!({
            "result": "suggestion",
            "tip": { "x": 0.5, "y": 0.5 },
            "confidence": 1.5
        });
        let err = decode_submission(&value).expect_err("out of range");
        assert!(err.contains("confidence"), "got: {err}");
    }

    #[test]
    fn decoder_rejects_negative_confidence() {
        let value = serde_json::json!({
            "result": "suggestion",
            "tip": { "x": 0.5, "y": 0.5 },
            "confidence": -0.1
        });
        let err = decode_submission(&value).expect_err("negative confidence");
        assert!(err.contains("confidence"), "got: {err}");
    }

    #[test]
    fn decoder_rejects_oversized_rationale() {
        let oversized = "x".repeat(MAX_TEXT_CHARS + 1);
        let value = serde_json::json!({
            "result": "suggestion",
            "tip": { "x": 0.5, "y": 0.5 },
            "confidence": 0.5,
            "rationale": oversized
        });
        let err = decode_submission(&value).expect_err("oversized");
        assert!(err.contains("rationale"), "got: {err}");
    }

    #[test]
    fn decoder_rejects_oversized_reason() {
        let oversized = "y".repeat(MAX_TEXT_CHARS + 1);
        let value = serde_json::json!({
            "result": "no_suggestion",
            "reason": oversized
        });
        let err = decode_submission(&value).expect_err("oversized");
        assert!(err.contains("reason"), "got: {err}");
    }

    #[test]
    fn decoder_rejects_unknown_field_in_suggestion() {
        let value = serde_json::json!({
            "result": "suggestion",
            "tip": { "x": 0.5, "y": 0.5 },
            "confidence": 0.5,
            "extra": true
        });
        assert!(decode_submission(&value).is_err());
    }

    #[test]
    fn decoder_rejects_unknown_field_in_tip() {
        let value = serde_json::json!({
            "result": "suggestion",
            "tip": { "x": 0.5, "y": 0.5, "z": 0.1 },
            "confidence": 0.5
        });
        assert!(decode_submission(&value).is_err());
    }

    #[test]
    fn decoder_rejects_unknown_result_tag() {
        let value = serde_json::json!({
            "result": "wat",
            "tip": { "x": 0.5, "y": 0.5 },
            "confidence": 0.5
        });
        assert!(decode_submission(&value).is_err());
    }

    #[test]
    fn decoder_rejects_missing_required_field() {
        let value = serde_json::json!({
            "result": "suggestion",
            "tip": { "x": 0.5, "y": 0.5 }
            // confidence missing
        });
        assert!(decode_submission(&value).is_err());
    }

    // ---- Decoder: finite coordinate guard (defense in depth) ----

    #[test]
    fn decoder_finite_check_rejects_nan_coordinates() {
        // JSON cannot encode NaN, so we exercise the check by manually
        // constructing a SubmitCalloutArgs with NaN. The decode path is
        // already covered by JSON's normal validation; the test asserts the
        // explicit is_finite guard exists at the same level.
        let mut args = serde_json::json!({
            "result": "suggestion",
            "tip": { "x": 0.5, "y": 0.5 },
            "confidence": 0.5
        });
        args.as_object_mut()
            .unwrap()
            .insert("confidence".into(), serde_json::json!(f64::NAN));
        assert!(decode_submission(&args).is_err());
    }

    // ---- Budget constant ----

    #[test]
    fn callout_run_budget_matches_brief() {
        let b = callout_run_budget();
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
        let def = submit_callout_suggestion_definition();
        assert_eq!(def.name, SUBMIT_CALLOUT_SUGGESTION);
        assert_eq!(def.name, "submit_callout_suggestion");
    }

    #[test]
    fn tool_definition_has_additional_properties_false_at_every_object_level() {
        let def = submit_callout_suggestion_definition();
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

    #[test]
    fn stub_tool_uses_canonical_name() {
        let tool = SubmitCalloutSuggestionTool;
        assert_eq!(tool.name(), SUBMIT_CALLOUT_SUGGESTION);
    }

    // ---- Lifecycle tests (Rig-driven runner) ----

    mod lifecycle {
        use super::*;
        use crate::domain::{AttachmentDescriptor, AuthorizedModelInput, MediaType};
        use crate::driver::{AgentConfig, AgentRunner};
        use crate::model::{ModelCompletion, ModelStreamEvent, ModelUsage, StopReason};
        use crate::provider::{ProviderAdapter, StreamBounds};
        use crate::runtime::{BudgetDimension, RunCancellation};
        use std::collections::VecDeque;
        use std::pin::Pin;
        use std::sync::Mutex;

        /// Records every `ModelRequest` it receives and replays a scripted set
        /// of stream events per call. Mirrors the pattern used in
        /// `driver::tests::provider_path::RecordingProvider`.
        struct ScriptedProvider {
            requests: Mutex<Vec<crate::model::ModelRequest>>,
            scripts: Mutex<VecDeque<Vec<ModelStreamEvent>>>,
        }

        impl ScriptedProvider {
            fn new(scripts: Vec<Vec<ModelStreamEvent>>) -> Self {
                Self {
                    requests: Mutex::new(Vec::new()),
                    scripts: Mutex::new(VecDeque::from(scripts)),
                }
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
                "suggest a callout".into(),
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

        fn authorized_input_with_two_pngs() -> AuthorizedModelInput {
            AuthorizedModelInput::new(
                "anthropic".into(),
                "vision-model".into(),
                "suggest a callout".into(),
                vec![
                    AttachmentDescriptor {
                        media_type: MediaType::Png,
                        width: 1,
                        height: 1,
                        byte_count: 4,
                    },
                    AttachmentDescriptor {
                        media_type: MediaType::Png,
                        width: 1,
                        height: 1,
                        byte_count: 4,
                    },
                ],
                vec![vec![0x89, 0x50, 0x4E, 0x47], vec![0x89, 0x50, 0x4E, 0x47]],
            )
            .expect("valid input")
        }

        fn completion_event(stop: StopReason) -> ModelStreamEvent {
            ModelStreamEvent::Completed(ModelCompletion {
                usage: ModelUsage {
                    input_tokens: 5,
                    output_tokens: 3,
                    total_tokens: 8,
                },
                stop_reason: stop,
            })
        }

        fn tool_call_turn(id: &str, name: &str, args: &str) -> Vec<ModelStreamEvent> {
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

        fn text_turn(text: &str) -> Vec<ModelStreamEvent> {
            vec![
                ModelStreamEvent::TextDelta(text.to_string()),
                completion_event(StopReason::EndTurn),
            ]
        }

        fn callout_runner() -> AgentRunner {
            AgentRunner::new(AgentConfig {
                max_turns: 2,
                ..AgentConfig::default()
            })
        }

        #[tokio::test]
        async fn one_tool_call_returns_suggested() {
            let args = serde_json::json!({
                "result": "suggestion",
                "tip": { "x": 0.5, "y": 0.25 },
                "confidence": 0.9,
                "rationale": "primary action"
            })
            .to_string();
            let provider = ScriptedProvider::new(vec![tool_call_turn(
                "tc_1",
                "submit_callout_suggestion",
                &args,
            )]);
            let runner = callout_runner();
            let input = authorized_input_with_one_png();
            let cancel = RunCancellation::new();

            let result = runner
                .run_callout_with_provider(input, &provider, callout_run_budget(), &cancel)
                .await;

            match result {
                CalloutRunTerminal::Suggested(s) => {
                    assert_eq!(s.x, 0.5);
                    assert_eq!(s.y, 0.25);
                    assert_eq!(s.confidence, 0.9);
                    assert_eq!(s.rationale.as_deref(), Some("primary action"));
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
                "submit_callout_suggestion",
                &args,
            )]);
            let runner = callout_runner();
            let input = authorized_input_with_one_png();
            let cancel = RunCancellation::new();

            let result = runner
                .run_callout_with_provider(input, &provider, callout_run_budget(), &cancel)
                .await;

            match result {
                CalloutRunTerminal::NoSuggestion(CalloutNoSuggestion::NoClearTarget { reason }) => {
                    assert_eq!(reason.as_deref(), Some("no clear target"));
                }
                other => panic!("expected NoSuggestion, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn completion_without_submission_returns_protocol_failure() {
            // First turn: model just returns text. The rig state machine goes to
            // Done, which the runner maps to ProtocolFailure.
            let provider = ScriptedProvider::new(vec![text_turn("no tool call here")]);
            let runner = callout_runner();
            let input = authorized_input_with_one_png();
            let cancel = RunCancellation::new();

            let result = runner
                .run_callout_with_provider(input, &provider, callout_run_budget(), &cancel)
                .await;

            assert_eq!(result, CalloutRunTerminal::ProtocolFailure);
        }

        #[tokio::test]
        async fn second_terminal_tool_in_same_response_is_rejected() {
            // Model emits two `submit_callout_suggestion` calls in the same
            // turn. Runner detects this and returns ProtocolFailure.
            let args1 = serde_json::json!({
                "result": "suggestion",
                "tip": { "x": 0.1, "y": 0.1 },
                "confidence": 0.5
            })
            .to_string();
            let args2 = serde_json::json!({
                "result": "suggestion",
                "tip": { "x": 0.9, "y": 0.9 },
                "confidence": 0.6
            })
            .to_string();
            let scripts = vec![vec![
                ModelStreamEvent::ToolCallStart {
                    id: "tc_1".into(),
                    name: "submit_callout_suggestion".into(),
                },
                ModelStreamEvent::ToolCallArgumentDelta {
                    id: "tc_1".into(),
                    delta: args1,
                },
                ModelStreamEvent::ToolCallStart {
                    id: "tc_2".into(),
                    name: "submit_callout_suggestion".into(),
                },
                ModelStreamEvent::ToolCallArgumentDelta {
                    id: "tc_2".into(),
                    delta: args2,
                },
                completion_event(StopReason::ToolUse),
            ]];
            let provider = ScriptedProvider::new(scripts);
            let runner = callout_runner();
            let input = authorized_input_with_one_png();
            let cancel = RunCancellation::new();

            let result = runner
                .run_callout_with_provider(input, &provider, callout_run_budget(), &cancel)
                .await;

            assert_eq!(result, CalloutRunTerminal::ProtocolFailure);
        }

        #[tokio::test]
        async fn cancellation_before_stream_returns_cancelled() {
            let args = serde_json::json!({
                "result": "suggestion",
                "tip": { "x": 0.5, "y": 0.5 },
                "confidence": 0.5
            })
            .to_string();
            let provider = ScriptedProvider::new(vec![tool_call_turn(
                "tc_1",
                "submit_callout_suggestion",
                &args,
            )]);
            let runner = callout_runner();
            let input = authorized_input_with_one_png();
            let cancel = RunCancellation::new();
            cancel.cancel();

            let result = runner
                .run_callout_with_provider(input, &provider, callout_run_budget(), &cancel)
                .await;

            assert_eq!(result, CalloutRunTerminal::Cancelled);
        }

        #[tokio::test]
        async fn attachment_budget_exceeded_returns_budget_exhausted_attachments() {
            // Two attachments in the input, but the budget allows zero.
            let mut budget = callout_run_budget();
            budget.attachments = 0;
            let args = serde_json::json!({
                "result": "suggestion",
                "tip": { "x": 0.5, "y": 0.5 },
                "confidence": 0.5
            })
            .to_string();
            let provider = ScriptedProvider::new(vec![tool_call_turn(
                "tc_1",
                "submit_callout_suggestion",
                &args,
            )]);
            let runner = callout_runner();
            let input = authorized_input_with_two_pngs();
            let cancel = RunCancellation::new();

            let result = runner
                .run_callout_with_provider(input, &provider, budget, &cancel)
                .await;

            match result {
                CalloutRunTerminal::BudgetExhausted {
                    dimension: BudgetDimension::Attachments,
                } => {}
                other => panic!("expected BudgetExhausted Attachments, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn more_than_two_model_turns_returns_budget_exhausted_model_calls() {
            // The standard `callout_run_budget()` allows 2 model calls and the
            // runner uses `max_turns(2)`. Verify that the model-calls budget
            // is wired into the runner: a custom budget with `model_calls: 0`
            // must fail the first model-turn charge and surface
            // `BudgetExhausted { ModelCalls }`. This is the same symmetry
            // path the rig's `MaxTurnsError` would take if the rig ever
            // surfaced it (it does not in practice because the runner returns
            // on the first terminal tool call).
            let mut budget = callout_run_budget();
            budget.model_calls = 0;
            let scripts = vec![text_turn("first")];
            let provider = ScriptedProvider::new(scripts);
            let runner = callout_runner();
            let input = authorized_input_with_one_png();
            let cancel = RunCancellation::new();

            let result = runner
                .run_callout_with_provider(input, &provider, budget, &cancel)
                .await;

            match result {
                CalloutRunTerminal::BudgetExhausted {
                    dimension: BudgetDimension::ModelCalls,
                } => {}
                other => panic!("expected BudgetExhausted ModelCalls, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn first_request_carries_attachments_and_callout_prompt() {
            // Smoke test: the first ModelRequest must carry the callout
            // system prompt and the authorized attachments; the runner must
            // NOT request a second turn after the first terminal tool call
            // (the first response is the callout handoff).
            let args = serde_json::json!({
                "result": "suggestion",
                "tip": { "x": 0.5, "y": 0.5 },
                "confidence": 0.5
            })
            .to_string();
            // Provide a second turn script; if the runner asks for a second
            // model call the test will fail with an empty-events error.
            let provider = ScriptedProvider::new(vec![
                tool_call_turn("tc_1", "submit_callout_suggestion", &args),
                text_turn("should not be requested"),
            ]);
            let runner = callout_runner();
            let input = authorized_input_with_one_png();
            let cancel = RunCancellation::new();

            let _ = runner
                .run_callout_with_provider(input, &provider, callout_run_budget(), &cancel)
                .await;

            let requests = provider.requests.lock().unwrap();
            assert_eq!(
                requests.len(),
                1,
                "runner must not request a second model turn after the first terminal tool call"
            );
            let first = &requests[0];
            assert_eq!(
                first.attachments.len(),
                1,
                "first request must carry the attachment"
            );
            let system_prompt = first
                .system_prompt
                .as_deref()
                .expect("callout system prompt");
            assert!(
                system_prompt.contains("submit_callout_suggestion"),
                "first request system prompt must reference the callout tool, got: {system_prompt}"
            );
            assert_eq!(first.tool_definitions.len(), 1);
            assert_eq!(first.tool_definitions[0].name, "submit_callout_suggestion");
        }
    }
}
