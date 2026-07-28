use iced::futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;

// ========================================================================
// Two-stage durable caption dispatch types
// ========================================================================

/// Request to prepare caption context. Captured at `SuggestCaptionsRequested`
/// and passed to the async preparation worker.
#[derive(Debug, Clone)]
pub(crate) enum CaptionContextRequest {
    Durable {
        root: PathBuf,
        expected_revision: u64,
    },
    Ephemeral {
        guide: rollshot_action::Guide,
    },
}

/// Prepared caption context returned by the preparation worker. Carries the
/// guide and origin needed to launch the provider request.
#[derive(Debug, Clone)]
pub(crate) enum PreparedCaptionContext {
    Durable {
        guide: rollshot_action::Guide,
        projection: rollshot_action::project::ActionGuideContextProjectionV1,
    },
    Ephemeral {
        guide: rollshot_action::Guide,
        guide_digest: String,
    },
}

impl PreparedCaptionContext {
    pub(super) fn guide(&self) -> &rollshot_action::Guide {
        match self {
            Self::Durable { guide, .. } => guide,
            Self::Ephemeral { guide, .. } => guide,
        }
    }

    pub(super) fn origin(&self) -> rollshot_action::CaptionProposalOrigin {
        match self {
            Self::Durable { projection, .. } => rollshot_action::CaptionProposalOrigin::DurableProject {
                revision: projection.revision(),
                projection_digest: projection.digest().to_string(),
            },
            Self::Ephemeral { guide_digest, .. } => {
                rollshot_action::CaptionProposalOrigin::EphemeralGuide {
                    guide_digest: guide_digest.clone(),
                }
            }
        }
    }

    pub(super) fn is_durable(&self) -> bool {
        matches!(self, Self::Durable { .. })
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CaptionAgentStep {
    pub index: usize,
    pub source: rollshot_action::CandidateId,
    pub keyframe: rollshot_action::FrameId,
    pub title: String,
    pub caption: String,
    pub kind: String,
    pub reason: String,
    pub at_ms: rollshot_action::Millis,
}

#[derive(Debug, Deserialize)]
struct CaptionResponse {
    suggestions: Vec<CaptionResponseSuggestion>,
}

#[derive(Debug, Deserialize)]
struct CaptionResponseSuggestion {
    source: rollshot_action::CandidateId,
    title: Option<String>,
    caption: String,
    confidence: Option<f32>,
    rationale: Option<String>,
}

pub(crate) fn steps_from_guide(guide: &rollshot_action::Guide) -> Vec<CaptionAgentStep> {
    guide
        .steps()
        .iter()
        .map(|step| CaptionAgentStep {
            index: step.index,
            source: step.source,
            keyframe: step.keyframe,
            title: step.title.clone(),
            caption: step.caption.clone(),
            kind: format!("{:?}", step.kind),
            reason: format!("{:?}", step.reason),
            at_ms: step.at_ms,
        })
        .collect()
}

pub(crate) fn build_caption_prompt(steps: &[CaptionAgentStep]) -> String {
    let json = serde_json::to_string(steps).unwrap_or_else(|_| "[]".to_string());
    format!(
        "Suggest concise Action Guide titles and one-sentence captions for these reviewed workflow steps.\n\
Prefer calling the submit_caption_suggestions tool. If tool calling is unavailable, return only JSON in the same schema.\n\
Use the source values exactly. Omit a title by using null when the current title is already good. Do not invent raw typed text.\n\
Steps: {json}"
    )
}

/// Compute a deterministic SHA-256 digest of the guide content for ephemeral
/// provenance. Hashes title, step count, and each step's (source, keyframe,
/// title, caption) with a domain separator.
fn compute_guide_digest(guide: &rollshot_action::Guide) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"rollshot-guide-ephemeral-v1\0");
    hasher.update(guide.title().as_bytes());
    hasher.update((guide.steps().len() as u64).to_le_bytes());
    for step in guide.steps() {
        hasher.update(step.source.to_le_bytes());
        hasher.update(step.keyframe.to_le_bytes());
        hasher.update(step.title.as_bytes());
        hasher.update(step.caption.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// Async preparation worker for two-stage durable caption dispatch.
///
/// For durable input, loads the project from disk and builds an
/// [`ActionGuideContextProjectionV1`]. For ephemeral input, computes
/// a guide digest for provenance.
pub(crate) async fn prepare_caption_context_task(
    run_id: u64,
    request: CaptionContextRequest,
) -> Result<PreparedCaptionContext, String> {
    match request {
        CaptionContextRequest::Durable { root, expected_revision } => {
            let loaded = tokio::task::spawn_blocking(move || {
                rollshot_action::project::load_project(&root)
            })
            .await
            .map_err(|_| "Project load task panicked.".to_string())?
            .map_err(|e| e.to_string())?;

            if loaded.manifest.revision != expected_revision {
                return Err(format!(
                    "Project was modified externally (expected revision {expected_revision}, got {}).",
                    loaded.manifest.revision
                ));
            }

            let projection = rollshot_action::project::ActionGuideContextProjectionV1::from_loaded_project(&loaded)
                .map_err(|e| format!("Caption context projection failed: {e}"))?;
            let guide = projection.to_guide().map_err(|e| format!("Guide from projection failed: {e}"))?;

            tracing::info!(
                target: "rollshot::action::caption_agent",
                run_id,
                revision = projection.revision(),
                step_count = guide.steps().len(),
                "durable caption context prepared"
            );

            Ok(PreparedCaptionContext::Durable { guide, projection })
        }
        CaptionContextRequest::Ephemeral { guide } => {
            let guide_digest = compute_guide_digest(&guide);
            Ok(PreparedCaptionContext::Ephemeral { guide, guide_digest })
        }
    }
}

pub(crate) fn caption_tool_definition() -> rollshot_agent::model::ToolDefinition {
    rollshot_agent::model::ToolDefinition {
        name: "submit_caption_suggestions".to_string(),
        description: "Submit reviewed Action Guide title/caption suggestions.".to_string(),
        parameters: json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["suggestions"],
            "properties": {
                "suggestions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["source", "title", "caption", "confidence", "rationale"],
                        "properties": {
                            "source": { "type": "integer" },
                            "title": { "type": ["string", "null"] },
                            "caption": { "type": "string" },
                            "confidence": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                            "rationale": { "type": ["string", "null"] }
                        }
                    }
                }
            }
        }),
    }
}

pub(crate) fn parse_caption_response(
    text: &str,
) -> Result<Vec<rollshot_action::CaptionSuggestionDraft>, String> {
    let parsed: CaptionResponse =
        serde_json::from_str(text.trim()).map_err(|e| format!("invalid caption JSON: {e}"))?;
    response_to_drafts(parsed)
}

pub(crate) fn parse_caption_tool_args(
    value: &Value,
) -> Result<Vec<rollshot_action::CaptionSuggestionDraft>, String> {
    let parsed: CaptionResponse = serde_json::from_value(value.clone())
        .map_err(|e| format!("invalid caption tool arguments: {e}"))?;
    response_to_drafts(parsed)
}

fn response_to_drafts(
    parsed: CaptionResponse,
) -> Result<Vec<rollshot_action::CaptionSuggestionDraft>, String> {
    let mut drafts = Vec::new();
    for item in parsed.suggestions {
        let caption = item.caption.trim();
        if caption.is_empty() {
            return Err("caption suggestion cannot be empty".to_string());
        }
        drafts.push(rollshot_action::CaptionSuggestionDraft {
            step_source: item.source,
            title: item
                .title
                .and_then(|title| (!title.trim().is_empty()).then(|| title.trim().to_string())),
            caption: caption.to_string(),
            confidence: item.confidence.unwrap_or(0.5),
            rationale: item
                .rationale
                .and_then(|text| (!text.trim().is_empty()).then(|| text.trim().to_string())),
        });
    }
    Ok(drafts)
}

pub(crate) async fn suggest_captions_task(
    run_id: u64,
    model: String,
    adapter: Box<dyn rollshot_agent::ProviderAdapter>,
    context: PreparedCaptionContext,
) -> Result<rollshot_action::CaptionProposal, String> {
    suggest_captions_with_timeout(
        run_id,
        model,
        adapter,
        context,
        std::time::Duration::from_secs(30),
    )
    .await
}

async fn suggest_captions_with_timeout(
    run_id: u64,
    model: String,
    adapter: Box<dyn rollshot_agent::ProviderAdapter>,
    context: PreparedCaptionContext,
    timeout: std::time::Duration,
) -> Result<rollshot_action::CaptionProposal, String> {
    let guide = context.guide();
    let origin = context.origin();
    let steps = steps_from_guide(guide);
    if steps.is_empty() {
        return Err("No reviewed steps to caption.".to_string());
    }
    let prompt = build_caption_prompt(&steps);
    let cancellation = rollshot_agent::runtime::RunCancellation::new();
    let deadline = tokio::time::Instant::now() + timeout;
    let bounds = rollshot_agent::StreamBounds::new(cancellation, deadline);
    let request = rollshot_agent::model::ModelRequest {
        model,
        prompt,
        history: Vec::new(),
        turn: 0,
        tool_definitions: vec![caption_tool_definition()],
        system_prompt: Some(
            "You produce compact structured suggestions for Rollshot Action Guide captions."
                .to_string(),
        ),
        max_tokens: Some(1200),
        attachments: vec![],
    };

    let mut stream = tokio::time::timeout_at(deadline, adapter.stream(request, bounds))
        .await
        .map_err(|_| "Caption suggestions timed out.".to_string())?
        .map_err(|e| e.to_string())?;
    let mut text = String::new();
    let mut tool_args = None;
    tokio::time::timeout_at(deadline, async {
        while let Some(event) = stream.next().await {
            match event.map_err(|e| e.to_string())? {
                rollshot_agent::model::ModelStreamEvent::TextDelta(delta) => {
                    text.push_str(&delta);
                }
                rollshot_agent::model::ModelStreamEvent::ToolCallComplete {
                    name,
                    arguments,
                    ..
                } if name == "submit_caption_suggestions" => {
                    tool_args = Some(arguments);
                }
                rollshot_agent::model::ModelStreamEvent::Completed(_) => break,
                rollshot_agent::model::ModelStreamEvent::Error(error) => {
                    return Err(error.to_string());
                }
                rollshot_agent::model::ModelStreamEvent::ToolCallStart { .. }
                | rollshot_agent::model::ModelStreamEvent::ToolCallArgumentDelta { .. }
                | rollshot_agent::model::ModelStreamEvent::ToolCallComplete { .. }
                | rollshot_agent::model::ModelStreamEvent::UsageDelta(_) => {}
            }
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "Caption suggestions timed out.".to_string())??;

    let drafts = match tool_args {
        Some(arguments) => parse_caption_tool_args(&arguments)?,
        None => parse_caption_response(&text)?,
    };
    let proposal = rollshot_action::CaptionProposal::from_agent_drafts(
        rollshot_action::CaptionProposalId(run_id),
        run_id,
        origin,
        guide,
        drafts,
    );
    if proposal.suggestions.is_empty() {
        return Err("Agent returned no usable caption suggestions.".to_string());
    }
    Ok(proposal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps() -> Vec<CaptionAgentStep> {
        vec![
            CaptionAgentStep {
                index: 1,
                source: 10,
                keyframe: 1,
                title: "Click".to_string(),
                caption: String::new(),
                kind: "click".to_string(),
                reason: "click-confirmed".to_string(),
                at_ms: 120,
            },
            CaptionAgentStep {
                index: 2,
                source: 11,
                keyframe: 2,
                title: "Enter text".to_string(),
                caption: String::new(),
                kind: "typing".to_string(),
                reason: "typing-settled".to_string(),
                at_ms: 340,
            },
        ]
    }

    #[test]
    fn parses_strict_caption_json() {
        let json = r#"{
          "suggestions": [
            {
              "source": 10,
              "title": "Open Settings",
              "caption": "The user opens the settings panel.",
              "confidence": 0.81,
              "rationale": "The click begins the flow."
            }
          ]
        }"#;

        let drafts = parse_caption_response(json).unwrap();

        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].step_source, 10);
        assert_eq!(drafts[0].title.as_deref(), Some("Open Settings"));
        assert_eq!(drafts[0].caption, "The user opens the settings panel.");
    }

    #[test]
    fn parser_rejects_missing_caption() {
        let json = r#"{"suggestions":[{"source":10,"confidence":0.5}]}"#;

        assert!(parse_caption_response(json).is_err());
    }

    #[test]
    fn parses_tool_call_arguments() {
        let args = serde_json::json!({
            "suggestions": [
                {
                    "source": 11,
                    "title": null,
                    "caption": "The user enters information into the form.",
                    "confidence": 0.73,
                    "rationale": "Typing activity usually indicates data entry."
                }
            ]
        });

        let drafts = parse_caption_tool_args(&args).unwrap();

        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].step_source, 11);
        assert_eq!(drafts[0].title, None);
        assert_eq!(
            drafts[0].caption,
            "The user enters information into the form."
        );
    }

    #[test]
    fn builds_prompt_without_raw_pixels() {
        let prompt = build_caption_prompt(&steps());

        assert!(prompt.contains("\"source\":10"), "prompt = {prompt}");
        assert!(prompt.contains("\"kind\":\"click\""), "prompt = {prompt}");
        assert!(!prompt.contains("image"), "prompt = {prompt}");
        assert!(!prompt.contains("pixels"), "prompt = {prompt}");
    }

    #[test]
    fn caption_tool_definition_names_schema() {
        let tool = caption_tool_definition();

        assert_eq!(tool.name, "submit_caption_suggestions");
        assert_eq!(tool.parameters["type"], "object");
        assert!(tool.parameters["properties"]["suggestions"].is_object());
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use iced::futures::stream;
    use rollshot_agent::model::{ModelError, ModelRequest, ModelStreamEvent};
    use rollshot_agent::StreamBounds;
    use std::future::Future;
    use std::pin::Pin;

    #[derive(Clone)]
    struct FakeProvider {
        events: Vec<Result<ModelStreamEvent, ModelError>>,
        delay: Option<std::time::Duration>,
    }

    impl rollshot_agent::ProviderAdapter for FakeProvider {
        fn stream(
            &self,
            _request: ModelRequest,
            _bounds: StreamBounds,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<
                            Pin<
                                Box<
                                    dyn iced::futures::Stream<
                                            Item = Result<ModelStreamEvent, ModelError>,
                                        > + Send,
                                >,
                            >,
                            ModelError,
                        >,
                    > + Send
                    + '_,
            >,
        > {
            let events = self.events.clone();
            let delay = self.delay;
            Box::pin(async move {
                if let Some(delay) = delay {
                    tokio::time::sleep(delay).await;
                }
                Ok(Box::pin(stream::iter(events))
                    as Pin<
                        Box<
                            dyn iced::futures::Stream<Item = Result<ModelStreamEvent, ModelError>>
                                + Send,
                        >,
                    >)
            })
        }
    }

    fn guide() -> rollshot_action::Guide {
        rollshot_action::Guide::from_candidates(vec![rollshot_action::CandidateStep {
            id: 10,
            kind: rollshot_action::CandidateKind::Click,
            reason: rollshot_action::DetectReason::ClickConfirmed,
            at_ms: 120,
            keyframe: 1,
            nearby: vec![1],
        }])
    }

    fn ephemeral_context() -> PreparedCaptionContext {
        PreparedCaptionContext::Ephemeral {
            guide: guide(),
            guide_digest: "0".repeat(64),
        }
    }

    fn run<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn runner_accepts_text_json_from_fake_provider() {
        let provider = FakeProvider {
            events: vec![
                Ok(ModelStreamEvent::TextDelta(
                    r#"{"suggestions":[{"source":10,"title":"Open Settings","caption":"The settings panel appears.","confidence":0.8,"rationale":null}]}"#
                        .to_string(),
                )),
                Ok(ModelStreamEvent::Completed(rollshot_agent::model::ModelCompletion {
                    usage: rollshot_agent::model::ModelUsage::default(),
                    stop_reason: rollshot_agent::model::StopReason::EndTurn,
                })),
            ],
            delay: None,
        };

        let proposal = run(suggest_captions_with_timeout(
            42,
            "fake-model".to_string(),
            Box::new(provider),
            ephemeral_context(),
            std::time::Duration::from_secs(1),
        ))
        .unwrap();

        assert_eq!(proposal.suggestions.len(), 1);
        assert_eq!(
            proposal.suggestions[0].suggested_caption,
            "The settings panel appears."
        );
    }

    #[test]
    fn runner_prefers_tool_call_arguments() {
        let provider = FakeProvider {
            events: vec![Ok(ModelStreamEvent::ToolCallComplete {
                id: "call-1".to_string(),
                name: "submit_caption_suggestions".to_string(),
                arguments: serde_json::json!({
                    "suggestions": [{
                        "source": 10,
                        "title": "Open Settings",
                        "caption": "The settings panel appears.",
                        "confidence": 0.8,
                        "rationale": null
                    }]
                }),
            })],
            delay: None,
        };

        let proposal = run(suggest_captions_with_timeout(
            42,
            "fake-model".to_string(),
            Box::new(provider),
            ephemeral_context(),
            std::time::Duration::from_secs(1),
        ))
        .unwrap();

        assert_eq!(
            proposal.suggestions[0].suggested_title.as_deref(),
            Some("Open Settings")
        );
    }

    #[test]
    fn runner_returns_provider_errors() {
        let provider = FakeProvider {
            events: vec![Ok(ModelStreamEvent::Error(ModelError::ProviderFailure(
                "rate limited".to_string(),
            )))],
            delay: None,
        };

        let err = run(suggest_captions_with_timeout(
            42,
            "fake-model".to_string(),
            Box::new(provider),
            ephemeral_context(),
            std::time::Duration::from_secs(1),
        ))
        .unwrap_err();

        assert!(err.contains("rate limited"), "err = {err}");
    }

    #[test]
    fn runner_times_out_quickly_in_tests() {
        let provider = FakeProvider {
            events: Vec::new(),
            delay: Some(std::time::Duration::from_millis(50)),
        };

        let err = run(suggest_captions_with_timeout(
            42,
            "fake-model".to_string(),
            Box::new(provider),
            ephemeral_context(),
            std::time::Duration::from_millis(1),
        ))
        .unwrap_err();

        assert_eq!(err, "Caption suggestions timed out.");
    }
}
