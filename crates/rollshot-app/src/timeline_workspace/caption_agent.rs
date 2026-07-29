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
            Self::Durable { projection, .. } => {
                rollshot_action::CaptionProposalOrigin::DurableProject {
                    revision: projection.revision(),
                    projection_digest: projection.digest().to_string(),
                }
            }
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

#[cfg(test)]
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
        CaptionContextRequest::Durable {
            root,
            expected_revision,
        } => {
            let loaded =
                tokio::task::spawn_blocking(move || rollshot_action::project::load_project(&root))
                    .await
                    .map_err(|_| "Project load task panicked.".to_string())?
                    .map_err(|e| e.to_string())?;

            if loaded.manifest.revision != expected_revision {
                return Err(format!(
                    "Project was modified externally (expected revision {expected_revision}, got {}).",
                    loaded.manifest.revision
                ));
            }

            let projection =
                rollshot_action::project::ActionGuideContextProjectionV1::from_loaded_project(
                    &loaded,
                )
                .map_err(|e| format!("Caption context projection failed: {e}"))?;
            let guide = projection
                .to_guide()
                .map_err(|e| format!("Guide from projection failed: {e}"))?;

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
            Ok(PreparedCaptionContext::Ephemeral {
                guide,
                guide_digest,
            })
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

/// User-visible copy for a caption run that ran out of time. Preserved verbatim
/// across the RunBudget migration (plan Task 16).
pub(crate) const TIMEOUT_MESSAGE: &str = "Caption suggestions timed out.";

/// User-visible copy shown while a caption run is in flight. Preserved verbatim
/// across the RunBudget migration (plan Task 16), which rewrites the handler
/// that sets it.
pub(crate) const RUNNING_MESSAGE: &str = "Suggesting captions...";

// ========================================================================
// Caption run wiring (Task 16)
// ========================================================================

/// Stub tool for the caption run. The driver needs an `Arc<dyn Tool>` to
/// validate the model's tool call; this stub accepts any valid payload and
/// returns success.
struct CaptionSubmitTool;

impl rollshot_agent::tools::Tool for CaptionSubmitTool {
    fn name(&self) -> &str {
        "submit_caption_suggestions"
    }
    fn json_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    fn call<'a>(
        &'a self,
        _arguments: &'a serde_json::Value,
    ) -> rollshot_agent::tools::ToolFuture<'a> {
        Box::pin(async move {
            Ok(rollshot_agent::tools::ToolOutcome::Success {
                result_json: serde_json::json!({"submitted": true}),
            })
        })
    }
}

fn caption_tool_stub() -> std::sync::Arc<dyn rollshot_agent::tools::Tool> {
    std::sync::Arc::new(CaptionSubmitTool)
}

/// SHA-256 of a canonicalized project root path. The Action Guide project
/// manifest has no stable identity, so the path is the only one available.
pub(crate) fn project_root_digest(root: &std::path::Path) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(b"rollshot-action-guide-project-root-v1\0");
    hasher.update(canonical.as_os_str().as_encoded_bytes());
    hasher.finalize().into()
}

pub(crate) fn caption_source_binding(
    context: &PreparedCaptionContext,
    project_root: Option<&std::path::Path>,
) -> rollshot_agent::product_task::SourceBinding {
    use rollshot_agent::product_task::SourceBinding;
    match (context, project_root) {
        (PreparedCaptionContext::Durable { projection, .. }, Some(root)) => {
            SourceBinding::ActionGuideProject {
                project_root_sha256: project_root_digest(root),
                revision: projection.revision(),
                projection_digest: projection.digest().to_owned(),
            }
        }
        (PreparedCaptionContext::Durable { projection, .. }, None) => {
            // A durable projection without a root cannot be restored later, so
            // bind it as ephemeral rather than inventing an identity.
            SourceBinding::ActionGuideEphemeralGuide {
                guide_digest: projection.digest().to_owned(),
            }
        }
        (PreparedCaptionContext::Ephemeral { guide_digest, .. }, _) => {
            SourceBinding::ActionGuideEphemeralGuide {
                guide_digest: guide_digest.clone(),
            }
        }
    }
}

pub(crate) fn caption_authority(
    task_id: rollshot_agent::product_task::ProductTaskId,
    run_id: rollshot_agent::domain::RunId,
    subject: rollshot_agent::authority::AuthoritySubject,
) -> Result<rollshot_agent::authority::AuthoritySnapshot, String> {
    use rollshot_agent::authority::{
        AuthorityBinding, AuthoritySnapshot, DisclosureCeiling, RunOperation,
    };
    use rollshot_agent::product_task::TaskAttemptId;

    let binding = AuthorityBinding::new(task_id, TaskAttemptId::new(1), run_id, subject);
    AuthoritySnapshot::new(
        binding,
        "rollshot-v1".to_owned(),
        DisclosureCeiling::TextMetadataOnly,
        false,
        std::collections::BTreeSet::new(),
        std::collections::BTreeSet::from([RunOperation::SubmitReviewCandidate]),
    )
    .map_err(|e| format!("build caption authority: {e}"))
}

/// Decode a `SingleSubmitTerminal` into caption drafts or a user-visible error.
///
/// This is the single place that maps the driver terminal onto caption-domain
/// results, so the frozen user-visible copy is exercised end to end.
pub(crate) fn decode_caption_terminal(
    terminal: &rollshot_agent::driver::SingleSubmitTerminal,
) -> Result<Vec<rollshot_action::CaptionSuggestionDraft>, String> {
    match terminal {
        rollshot_agent::driver::SingleSubmitTerminal::Submitted { arguments } => {
            parse_caption_tool_args(arguments)
        }
        rollshot_agent::driver::SingleSubmitTerminal::TextCompleted { text } => {
            parse_caption_response(text)
        }
        rollshot_agent::driver::SingleSubmitTerminal::Cancelled => {
            Err("Caption suggestions cancelled.".to_string())
        }
        rollshot_agent::driver::SingleSubmitTerminal::BudgetExhausted { dimension } => {
            use rollshot_agent::runtime::BudgetDimension;
            if *dimension == BudgetDimension::WallTime {
                Err(TIMEOUT_MESSAGE.to_string())
            } else {
                Err(format!("Caption suggestions exhausted budget: {dimension:?}"))
            }
        }
        rollshot_agent::driver::SingleSubmitTerminal::ProviderFailure => {
            Err("Caption suggestions failed: provider error".to_string())
        }
        rollshot_agent::driver::SingleSubmitTerminal::ProtocolFailure => {
            Err("Caption suggestions failed: agent protocol error".to_string())
        }
        rollshot_agent::driver::SingleSubmitTerminal::AuthorityDenied { operation } => {
            Err(format!(
                "Caption suggestions denied: operation {operation:?} not authorized"
            ))
        }
    }
}

pub(crate) async fn suggest_captions_task(
    run_id: u64,
    store: std::sync::Arc<crate::agent_store::TaskStore>,
    cancellation: rollshot_agent::runtime::RunCancellation,
    model: String,
    provider: String,
    adapter: Box<dyn rollshot_agent::ProviderAdapter>,
    context: PreparedCaptionContext,
) -> Result<rollshot_action::CaptionProposal, String> {
    let project_root = match &context {
        PreparedCaptionContext::Durable { .. } => {
            // The project root is only known to the caller; we extract it from
            // the source binding once the context is prepared.
            None
        }
        PreparedCaptionContext::Ephemeral { .. } => None,
    };
    suggest_captions_with_store(
        run_id,
        store,
        cancellation,
        model,
        provider,
        adapter,
        context,
        project_root,
    )
    .await
}

async fn suggest_captions_with_store(
    run_id: u64,
    store: std::sync::Arc<crate::agent_store::TaskStore>,
    cancellation: rollshot_agent::runtime::RunCancellation,
    model: String,
    provider: String,
    adapter: Box<dyn rollshot_agent::ProviderAdapter>,
    context: PreparedCaptionContext,
    project_root: Option<std::path::PathBuf>,
) -> Result<rollshot_action::CaptionProposal, String> {
    use rollshot_agent::captions::caption_run_budget;
    use rollshot_agent::driver::{AgentConfig, AgentRunner, SingleSubmitProfile};
    use rollshot_agent::product_task::{
        ProductTaskSnapshot, RunContractReceiptV1, TaskAttempt, TaskKind,
    };
    use rollshot_agent::skills::bundled_action_guide_captions_use;
    use rollshot_agent::authority::AuthoritySubject;

    let guide = context.guide();
    let origin = context.origin();
    let steps = steps_from_guide(guide);
    if steps.is_empty() {
        return Err("No reviewed steps to caption.".to_string());
    }

    // 1. Build source binding and create the task.
    let source_binding = caption_source_binding(&context, project_root.as_deref());
    let task_id_str = format!("task-{}", uuid::Uuid::new_v4());
    let task_id = rollshot_agent::product_task::ProductTaskId::parse(&task_id_str)
        .map_err(|e| format!("build task id: {e}"))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let created =
        ProductTaskSnapshot::new_v3(task_id.clone(), TaskKind::ActionGuideCaptions, source_binding.clone(), now)
            .map_err(|e| format!("create task: {e}"))?;
    let store_clone = store.clone();
    let created_clone = created.clone();
    tokio::task::spawn_blocking(move || {
        store_clone.create_audited(
            &created_clone,
            rollshot_agent::audit::AuditEventId::new_v4(),
            now,
        )
    })
    .await
    .map_err(|e| format!("spawn_blocking create: {e}"))?
    .map_err(|e| format!("audit create: {e}"))?;

    // 2. Start attempt → transition_audited.
    let run_id_str = format!("run-{}", uuid::Uuid::new_v4());
    let run_id_parsed = rollshot_agent::domain::RunId::parse(&run_id_str)
        .map_err(|e| format!("build run id: {e}"))?;
    let attempt = TaskAttempt::new(
        rollshot_agent::product_task::TaskAttemptId::new(1),
        run_id_parsed.clone(),
        now,
    );
    let running = created
        .start_attempt(attempt, now)
        .map_err(|e| format!("start attempt: {e}"))?;
    let store_clone = store.clone();
    let created_for_attempt = created.clone();
    let running_clone = running.clone();
    tokio::task::spawn_blocking(move || {
        store_clone.transition_audited(
            &created_for_attempt,
            &running_clone,
            rollshot_agent::audit::AuditEventId::new_v4(),
            now,
        )
    })
    .await
    .map_err(|e| format!("spawn_blocking start attempt: {e}"))?
    .map_err(|e| format!("audit start attempt: {e}"))?;

    // 3. Resolve bundled skill; build authority; bind run contract.
    let skill_use = bundled_action_guide_captions_use()
        .ok_or_else(|| "Caption skill not found.".to_string())?;

    let subject = match &source_binding {
        rollshot_agent::product_task::SourceBinding::ActionGuideProject {
            project_root_sha256,
            revision,
            projection_digest,
        } => AuthoritySubject::ActionGuideProject {
            project_root_sha256: *project_root_sha256,
            revision: *revision,
            projection_digest: projection_digest.clone(),
        },
        rollshot_agent::product_task::SourceBinding::ActionGuideEphemeralGuide {
            guide_digest,
        } => AuthoritySubject::ActionGuideEphemeralGuide {
            guide_digest: guide_digest.clone(),
        },
        _ => return Err("unexpected source binding domain for captions".to_string()),
    };

    let authority = caption_authority(task_id.clone(), run_id_parsed.clone(), subject.clone())
        .map_err(|e| format!("build authority: {e}"))?;

    let receipt = authority.receipt(now);
    let run_contract = RunContractReceiptV1 {
        authority: receipt,
        skill_use: skill_use.receipt(),
        bound_at_unix_ms: now,
    };
    let bound = running
        .bind_run_contract(run_contract, now)
        .map_err(|e| format!("bind run contract: {e}"))?;
    let store_clone = store.clone();
    let running_for_bind = running.clone();
    let bound_clone = bound.clone();
    tokio::task::spawn_blocking(move || {
        store_clone.transition_audited(
            &running_for_bind,
            &bound_clone,
            rollshot_agent::audit::AuditEventId::new_v4(),
            now,
        )
    })
    .await
    .map_err(|e| format!("spawn_blocking bind contract: {e}"))?
    .map_err(|e| format!("audit bind contract: {e}"))?;

    // 4. Build profile and authorized input.
    let prompt = format!(
        "{envelope}\n\n<rollshot-skill package=\"{pkg}\" digest=\"{digest}\">\n{body}\n</rollshot-skill>",
        envelope = "You produce compact structured suggestions for Rollshot Action Guide captions.",
        pkg = skill_use.package_id().as_str(),
        digest = skill_use.digest(),
        body = skill_use.body(),
    );
    let profile = SingleSubmitProfile::from_skill(
        &skill_use,
        prompt.clone(),
        caption_tool_definition(),
        caption_tool_stub(),
        rollshot_agent::authority::RunOperation::SubmitReviewCandidate,
        "rollshot::action::caption_agent",
    )
    .map_err(|e| format!("build caption profile: {e:?}"))?;

    let input = rollshot_agent::domain::AuthorizedModelInput::new(
        provider,
        model,
        prompt,
        vec![],
        vec![],
    )
    .map_err(|e| format!("build model input: {e}"))?;

    // 5. Run the single-submit profile.
    let runner = AgentRunner::new(AgentConfig::default());
    let terminal = runner
        .run_single_submit_with_provider(
            profile,
            input,
            adapter.as_ref(),
            caption_run_budget(),
            &cancellation,
            &authority,
            &subject,
            None,
        )
        .await;

    // 6. Map terminal to caption drafts.
    let drafts = decode_caption_terminal(&terminal)?;
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

    /// Today's exact static instruction text, captured before the skill move.
    /// Task 13 asserts the bundled SKILL.md body equals this byte for byte.
    pub(crate) const CAPTION_INSTRUCTION_BASELINE: &str = "Suggest concise Action Guide titles and one-sentence captions for these reviewed workflow steps.\nPrefer calling the submit_caption_suggestions tool. If tool calling is unavailable, return only JSON in the same schema.\nUse the source values exactly. Omit a title by using null when the current title is already good. Do not invent raw typed text.";

    #[test]
    fn prompt_baseline_is_instruction_text_then_steps() {
        let prompt = build_caption_prompt(&steps());

        let (instructions, tail) = prompt
            .split_once("\nSteps: ")
            .expect("prompt must end with a Steps: section");

        assert_eq!(
            instructions, CAPTION_INSTRUCTION_BASELINE,
            "instruction text drifted from the recorded baseline"
        );
        assert!(
            tail.starts_with('['),
            "steps payload must be a JSON array, got {tail}"
        );
    }

    #[test]
    fn timeout_copy_baseline() {
        // Task 16 replaces the timeout with a RunBudget wall_time dimension and
        // must map it back to this exact string.
        assert_eq!(
            super::TIMEOUT_MESSAGE,
            "Caption suggestions timed out.",
            "user-visible timeout copy must not change"
        );
    }

    #[test]
    fn running_copy_baseline() {
        assert_eq!(
            super::RUNNING_MESSAGE,
            "Suggesting captions...",
            "user-visible in-flight copy must not change"
        );
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use rollshot_agent::driver::SingleSubmitTerminal;
    use rollshot_agent::runtime::BudgetDimension;
    use std::future::Future;
    use std::sync::Arc;

    fn test_task_id() -> rollshot_agent::product_task::ProductTaskId {
        rollshot_agent::product_task::ProductTaskId::parse(
            "task-00000000-0000-4000-8000-000000000001",
        )
        .unwrap()
    }

    fn test_run_id() -> rollshot_agent::domain::RunId {
        rollshot_agent::domain::RunId::parse("run-00000000-0000-4000-8000-000000000001").unwrap()
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

    pub(crate) fn ephemeral_context() -> PreparedCaptionContext {
        PreparedCaptionContext::Ephemeral {
            guide: guide(),
            guide_digest: "0".repeat(64),
        }
    }

    /// Build a durable `PreparedCaptionContext` by writing a minimal project
    /// under `root` and loading it back.
    pub(crate) fn durable_context(
        root: &std::path::Path,
    ) -> (PreparedCaptionContext, u64, String) {
        use rollshot_action::project::{
            create_project, load_project, ActionGuideContextProjectionV1,
            EnabledOutputs, ProjectSnapshot, ProjectStep,
            ProjectStepId, SnapshotFrame, SnapshotFramePayload,
        };

        let project_dir = root.join("guide.rollshot-guide");
        let steps = vec![ProjectStep {
            id: ProjectStepId(1),
            order: 1,
            title: "Step 1".into(),
            caption: Some("Caption 1".into()),
            kind: rollshot_action::CandidateKind::Click,
            reason: rollshot_action::DetectReason::ClickConfirmed,
            at_ms: 100,
            keyframe: 1,
            nearby: vec![1],
            annotations: None,
        }];
        let snapshot = ProjectSnapshot {
            base_revision: None,
            title: "Test Guide".into(),
            capture_region: rollshot_action::CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: rollshot_action::InputSourceKind::VisualOnly,
            input_capability: rollshot_action::InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(Arc::new(image::RgbaImage::new(8, 8))),
            }],
            steps,
            import_warnings: Vec::new(),
        };
        create_project(&snapshot, &project_dir).unwrap();
        let loaded = load_project(&project_dir).unwrap();
        let projection =
            ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap();
        let revision = projection.revision();
        let digest = projection.digest().to_owned();
        let guide = projection.to_guide().unwrap();
        (
            PreparedCaptionContext::Durable { guide, projection },
            revision,
            digest,
        )
    }

    fn run<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(future)
    }

    // ---- Authority tests ----

    #[test]
    fn caption_authority_grants_only_submit_and_forbids_images() {
        let subject = rollshot_agent::authority::AuthoritySubject::ActionGuideProject {
            project_root_sha256: [7u8; 32],
            revision: 3,
            projection_digest: "ab".repeat(32),
        };
        let run_id = test_run_id();

        let authority = caption_authority(test_task_id(), run_id.clone(), subject.clone()).unwrap();

        assert_eq!(
            authority.disclosure(),
            rollshot_agent::authority::DisclosureCeiling::TextMetadataOnly
        );
        assert!(authority
            .authorize_tool(
                &run_id,
                &subject,
                rollshot_agent::authority::RunOperation::SubmitReviewCandidate
            )
            .is_ok());
        for forbidden in [
            rollshot_agent::authority::RunOperation::InspectPreparedImage,
            rollshot_agent::authority::RunOperation::ExecuteRestrictedAutomation,
            rollshot_agent::authority::RunOperation::WriteDraft,
            rollshot_agent::authority::RunOperation::ReadDraft,
            rollshot_agent::authority::RunOperation::RequestUserInput,
        ] {
            assert!(
                authority.authorize_tool(&run_id, &subject, forbidden).is_err(),
                "caption runs must never hold {forbidden:?}"
            );
        }
    }

    // ---- Source binding tests ----

    #[test]
    fn source_binding_follows_the_prepared_context_origin() {
        use rollshot_agent::product_task::SourceBinding;

        // Ephemeral origin, with and without a root: always ephemeral.
        let root = tempfile::tempdir().unwrap();
        for project_root in [None, Some(root.path())] {
            match caption_source_binding(&ephemeral_context(), project_root) {
                SourceBinding::ActionGuideEphemeralGuide { guide_digest } => {
                    assert_eq!(guide_digest, "0".repeat(64));
                }
                other => panic!("expected ephemeral, got {other:?}"),
            }
        }

        // Durable origin with a root: project-bound, carrying the projection's
        // own revision and digest, and the path digest — not a placeholder.
        let (context, revision, digest) = durable_context(root.path());
        match caption_source_binding(&context, Some(root.path())) {
            SourceBinding::ActionGuideProject {
                project_root_sha256,
                revision: bound_revision,
                projection_digest,
            } => {
                assert_eq!(project_root_sha256, project_root_digest(root.path()));
                assert_eq!(bound_revision, revision);
                assert_eq!(projection_digest, digest);
            }
            other => panic!("expected project binding, got {other:?}"),
        }

        // Durable origin with no root cannot be restored, so it degrades to
        // ephemeral rather than inventing an identity.
        assert!(matches!(
            caption_source_binding(&context, None),
            SourceBinding::ActionGuideEphemeralGuide { .. }
        ));
    }

    #[test]
    fn project_root_digest_is_path_scoped_and_domain_separated() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();

        assert_eq!(project_root_digest(a.path()), project_root_digest(a.path()));
        assert_ne!(project_root_digest(a.path()), project_root_digest(b.path()));
    }

    // ---- Terminal mapping tests ----

    #[test]
    fn text_completion_still_decodes_captions_without_a_tool_call() {
        // Preserves the pre-migration fallback: a provider that cannot call
        // tools may return the same JSON as assistant text.
        let terminal = SingleSubmitTerminal::TextCompleted {
            text: r#"{"suggestions":[{"source":10,"title":"Open Settings","caption":"The settings panel appears.","confidence":0.8,"rationale":null}]}"#
                .to_string(),
        };

        let drafts = decode_caption_terminal(&terminal).expect("text fallback must decode");

        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].caption, "The settings panel appears.");
    }

    #[test]
    fn wall_time_exhaustion_reports_the_frozen_timeout_copy() {
        let terminal = SingleSubmitTerminal::BudgetExhausted {
            dimension: BudgetDimension::WallTime,
        };

        let err = decode_caption_terminal(&terminal).unwrap_err();

        assert_eq!(err, super::TIMEOUT_MESSAGE);
    }

    #[test]
    fn cancellation_and_protocol_failures_keep_their_existing_copy() {
        let pairs: Vec<(SingleSubmitTerminal, &str)> = vec![
            (
                SingleSubmitTerminal::Cancelled,
                "Caption suggestions cancelled.",
            ),
            (
                SingleSubmitTerminal::ProviderFailure,
                "Caption suggestions failed: provider error",
            ),
            (
                SingleSubmitTerminal::ProtocolFailure,
                "Caption suggestions failed: agent protocol error",
            ),
            (
                SingleSubmitTerminal::BudgetExhausted {
                    dimension: BudgetDimension::ModelCalls,
                },
                "Caption suggestions exhausted budget: ModelCalls",
            ),
        ];

        for (terminal, expected) in &pairs {
            let err = decode_caption_terminal(terminal).unwrap_err();
            assert_eq!(&err.as_str(), expected, "mismatch for {terminal:?}");
        }
    }

    #[test]
    fn authority_denied_reports_the_operation() {
        let terminal = SingleSubmitTerminal::AuthorityDenied {
            operation: rollshot_agent::authority::RunOperation::InspectPreparedImage,
        };

        let err = decode_caption_terminal(&terminal).unwrap_err();

        assert!(
            err.contains("InspectPreparedImage"),
            "authority denial must name the operation, got: {err}"
        );
    }
}
