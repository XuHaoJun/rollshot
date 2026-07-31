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
pub(crate) fn compute_guide_digest(guide: &rollshot_action::Guide) -> String {
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
                tokio::task::spawn_blocking(move || rollshot_action::project::load_project(&root, None))
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

#[cfg(test)]
pub(crate) fn caption_authority(
    task_id: rollshot_agent::product_task::ProductTaskId,
    run_id: rollshot_agent::domain::RunId,
    subject: rollshot_agent::authority::AuthoritySubject,
) -> Result<rollshot_agent::authority::AuthoritySnapshot, String> {
    caption_authority_with_submit_grant(task_id, run_id, subject, true)
}

fn caption_authority_with_submit_grant(
    task_id: rollshot_agent::product_task::ProductTaskId,
    run_id: rollshot_agent::domain::RunId,
    subject: rollshot_agent::authority::AuthoritySubject,
    grant_submit: bool,
) -> Result<rollshot_agent::authority::AuthoritySnapshot, String> {
    use rollshot_agent::authority::{
        AuthorityBinding, AuthoritySnapshot, DisclosureCeiling, RunOperation,
    };
    use rollshot_agent::product_task::TaskAttemptId;

    let grants = if grant_submit {
        std::collections::BTreeSet::from([RunOperation::SubmitReviewCandidate])
    } else {
        std::collections::BTreeSet::new()
    };
    let binding = AuthorityBinding::new(task_id, TaskAttemptId::new(1), run_id, subject);
    AuthoritySnapshot::new(
        binding,
        "rollshot-v1".to_owned(),
        DisclosureCeiling::TextMetadataOnly,
        false,
        std::collections::BTreeSet::new(),
        grants,
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
                Err(format!(
                    "Caption suggestions exhausted budget: {dimension:?}"
                ))
            }
        }
        rollshot_agent::driver::SingleSubmitTerminal::ProviderFailure => {
            Err("Caption suggestions failed: provider error".to_string())
        }
        rollshot_agent::driver::SingleSubmitTerminal::AuditFailure { category } => {
            Err(format!("Caption suggestions failed: audit {category:?}"))
        }
        rollshot_agent::driver::SingleSubmitTerminal::ProtocolFailure => {
            Err("Caption suggestions failed: agent protocol error".to_string())
        }
        rollshot_agent::driver::SingleSubmitTerminal::AuthorityDenied { operation } => Err(
            format!("Caption suggestions denied: operation {operation:?} not authorized"),
        ),
    }
}

// ========================================================================
// Caption artifact promotion (Task 17)
// ========================================================================

/// Serialize the caption proposal's suggestions as a review artifact payload.
/// Carries only the suggestions — the whole guide is not included.
pub(crate) fn caption_artifact_payload(proposal: &rollshot_action::CaptionProposal) -> Vec<u8> {
    #[derive(serde::Serialize)]
    struct Suggestion<'a> {
        id: u64,
        step_source: rollshot_action::CandidateId,
        suggested_title: &'a Option<String>,
        suggested_caption: &'a str,
        confidence: f32,
        rationale: &'a Option<String>,
    }
    #[derive(serde::Serialize)]
    struct Payload<'a> {
        suggestions: Vec<Suggestion<'a>>,
    }
    let payload = Payload {
        suggestions: proposal
            .suggestions
            .iter()
            .map(|s| Suggestion {
                id: s.id.0,
                step_source: s.base.source,
                suggested_title: &s.suggested_title,
                suggested_caption: &s.suggested_caption,
                confidence: s.confidence,
                rationale: &s.rationale,
            })
            .collect(),
    };
    serde_json::to_vec(&payload).expect("caption payload is always serializable")
}

/// Build a [`ReviewReceipt`] for the caption proposal by partitioning
/// each suggestion into applied/rejected by its current status.
///
/// `Stale` suggestions are recorded as rejected — they were valid at
/// proposal time but no longer match the guide.
pub(crate) fn caption_review_receipt(
    proposal: &rollshot_action::CaptionProposal,
    metadata: &rollshot_agent::product_task::ProductArtifactMetadata,
    now: i64,
) -> Result<rollshot_agent::product_task::ReviewReceipt, String> {
    use rollshot_action::CaptionSuggestionStatus;
    use rollshot_agent::product_task::{LocalReviewDeltaV1, ReviewReceipt};

    let narrow = |id: u64| -> Result<u32, String> {
        u32::try_from(id).map_err(|_| format!("caption suggestion id {id} exceeds u32"))
    };

    let mut applied = Vec::new();
    let mut rejected = Vec::new();
    for suggestion in &proposal.suggestions {
        match suggestion.status {
            CaptionSuggestionStatus::Accepted => applied.push(narrow(suggestion.id.0)?),
            CaptionSuggestionStatus::Rejected | CaptionSuggestionStatus::Stale => {
                rejected.push(narrow(suggestion.id.0)?)
            }
            CaptionSuggestionStatus::Pending => {}
        }
    }

    Ok(ReviewReceipt {
        artifact_id: metadata.artifact_id().clone(),
        artifact_revision: metadata.artifact_revision(),
        proposal_id: metadata.proposal_id().to_owned(),
        applied_candidates: applied,
        rejected_candidates: rejected,
        // Captions have no move or manual-add review editing:
        // CaptionProposal::apply has no edit-then-accept path.
        local_delta: LocalReviewDeltaV1 {
            moved_candidates: Vec::new(),
            manual_additions: Vec::new(),
        },
        resulting_document_state_id: None,
        resulting_document_digest: None,
        decided_at_unix_ms: now,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct CaptionRunSuccess {
    pub task_id: rollshot_agent::product_task::ProductTaskId,
    pub proposal: rollshot_action::CaptionProposal,
    pub snapshot: rollshot_agent::product_task::ProductTaskSnapshot,
    pub provider_id: String,
    pub model_id: String,
}

fn promote_caption_ready_for_review(
    store: &crate::agent_store::TaskStore,
    task_id: &rollshot_agent::product_task::ProductTaskId,
    proposal: &rollshot_action::CaptionProposal,
    provider_id: &str,
    model_id: &str,
) -> Result<rollshot_agent::product_task::ProductTaskSnapshot, String> {
    use rollshot_agent::product_task::{
        ArtifactId, ArtifactKind, ArtifactRevision, ArtifactSummary, ProductArtifactMetadata,
    };
    use sha2::{Digest, Sha256};

    let snapshot = store
        .load(task_id)
        .map_err(|e| format!("load caption task: {e}"))?;
    let last_attempt = snapshot
        .attempts()
        .last()
        .ok_or("caption task has no attempt".to_string())?;
    let artifact_payload = caption_artifact_payload(proposal);
    let proposal_payload =
        serde_json::to_vec(proposal).map_err(|e| format!("serialize caption proposal: {e}"))?;
    let meta = ProductArtifactMetadata::new_v3(
        ArtifactId::parse(format!(
            "artifact-{}",
            task_id
                .as_str()
                .strip_prefix("task-")
                .unwrap_or(task_id.as_str())
        ))
        .map_err(|e| format!("build caption artifact id: {e}"))?,
        ArtifactRevision::new(snapshot.snapshot_revision() + 1),
        ArtifactKind::ActionGuideCaptions,
        1,
        format!("{:x}", Sha256::digest(&artifact_payload)),
        snapshot.source_binding().clone(),
        task_id.clone(),
        last_attempt.attempt_id(),
        last_attempt.run_id().clone(),
        proposal.id.0.to_string(),
        provider_id.to_owned(),
        model_id.to_owned(),
        String::new(),
        ArtifactSummary::ActionGuideCaptions {
            suggestion_count: proposal.suggestions.len() as u32,
        },
        chrono::Utc::now().timestamp_millis(),
    );
    let now = chrono::Utc::now().timestamp_millis();
    let promoted = snapshot
        .record_ready_for_review(meta, artifact_payload, Some(proposal_payload), now)
        .map_err(|e| format!("record ready: {e}"))?;
    store
        .transition_audited(
            &snapshot,
            &promoted,
            rollshot_agent::audit::AuditEventId::new_v4(),
            now,
        )
        .map_err(|e| format!("persist promotion: {e}"))?;
    Ok(promoted)
}

async fn persist_caption_terminal(
    store: std::sync::Arc<crate::agent_store::TaskStore>,
    task_id: rollshot_agent::product_task::ProductTaskId,
    terminal: rollshot_agent::product_task::TaskTerminal,
    now: i64,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let snapshot = store
            .load(&task_id)
            .map_err(|e| format!("load caption task for terminal: {e}"))?;
        let terminal_snapshot = snapshot
            .record_terminal(terminal, now)
            .map_err(|e| format!("record caption terminal: {e}"))?;
        store
            .transition_audited(
                &snapshot,
                &terminal_snapshot,
                rollshot_agent::audit::AuditEventId::new_v4(),
                now,
            )
            .map_err(|e| format!("persist caption terminal: {e}"))?;
        Ok(())
    })
    .await
    .map_err(|e| format!("spawn_blocking caption terminal: {e}"))?
}

async fn fail_caption_run(
    store: std::sync::Arc<crate::agent_store::TaskStore>,
    task_id: rollshot_agent::product_task::ProductTaskId,
    terminal: rollshot_agent::product_task::TaskTerminal,
    message: String,
) -> Result<CaptionRunSuccess, String> {
    let now = chrono::Utc::now().timestamp_millis();
    persist_caption_terminal(store, task_id, terminal, now)
        .await
        .map_err(|persist_error| format!("{message}; {persist_error}"))?;
    Err(message)
}

async fn fail_attempt_start_if_running(
    store: std::sync::Arc<crate::agent_store::TaskStore>,
    task_id: rollshot_agent::product_task::ProductTaskId,
    terminal: rollshot_agent::product_task::TaskTerminal,
    message: String,
) -> Result<CaptionRunSuccess, String> {
    let store_for_load = store.clone();
    let task_id_for_load = task_id.clone();
    let status = tokio::task::spawn_blocking(move || {
        store_for_load
            .load(&task_id_for_load)
            .map(|snapshot| snapshot.status().clone())
    })
    .await;
    match status {
        Ok(Ok(rollshot_agent::product_task::TaskStatus::Running)) => {
            fail_caption_run(store, task_id, terminal, message).await
        }
        Ok(Ok(_)) => Err(message),
        Ok(Err(error)) => Err(format!("{message}; inspect caption attempt: {error}")),
        Err(error) => Err(format!(
            "{message}; spawn_blocking inspect caption attempt: {error}"
        )),
    }
}

fn caption_task_terminal(
    terminal: &rollshot_agent::driver::SingleSubmitTerminal,
) -> rollshot_agent::product_task::TaskTerminal {
    use rollshot_agent::driver::SingleSubmitTerminal;
    use rollshot_agent::product_task::TaskTerminal;

    match terminal {
        SingleSubmitTerminal::Cancelled => TaskTerminal::Cancelled,
        SingleSubmitTerminal::BudgetExhausted { dimension } => TaskTerminal::BudgetExhausted {
            dimension: format!("{dimension:?}"),
        },
        SingleSubmitTerminal::ProviderFailure => TaskTerminal::ProviderFailure,
        SingleSubmitTerminal::AuditFailure { category } => TaskTerminal::AuditFailure {
            category: format!("{category:?}"),
        },
        SingleSubmitTerminal::AuthorityDenied { .. }
        | SingleSubmitTerminal::ProtocolFailure
        | SingleSubmitTerminal::Submitted { .. }
        | SingleSubmitTerminal::TextCompleted { .. } => TaskTerminal::AgentProtocolFailure,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn suggest_captions_task(
    run_id: u64,
    store: std::sync::Arc<crate::agent_store::TaskStore>,
    cancellation: rollshot_agent::runtime::RunCancellation,
    model: String,
    provider: String,
    adapter: Box<dyn rollshot_agent::ProviderAdapter>,
    context: PreparedCaptionContext,
    project_root: Option<PathBuf>,
) -> Result<CaptionRunSuccess, String> {
    suggest_captions_with_store(
        run_id,
        store,
        cancellation,
        model,
        provider,
        adapter,
        context,
        project_root,
        rollshot_agent::captions::caption_run_budget(),
        true,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn suggest_captions_with_store(
    run_id: u64,
    store: std::sync::Arc<crate::agent_store::TaskStore>,
    cancellation: rollshot_agent::runtime::RunCancellation,
    model: String,
    provider: String,
    adapter: Box<dyn rollshot_agent::ProviderAdapter>,
    context: PreparedCaptionContext,
    project_root: Option<std::path::PathBuf>,
    budget: rollshot_agent::runtime::RunBudget,
    grant_submit: bool,
) -> Result<CaptionRunSuccess, String> {
    use rollshot_agent::authority::AuthoritySubject;
    use rollshot_agent::driver::{AgentConfig, AgentRunner, SingleSubmitProfile};
    use rollshot_agent::product_task::{
        ProductTaskSnapshot, RunContractReceiptV1, TaskAttempt, TaskKind,
    };
    use rollshot_agent::skills::bundled_action_guide_captions_use;

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

    let created = ProductTaskSnapshot::new_v3(
        task_id.clone(),
        TaskKind::ActionGuideCaptions,
        source_binding.clone(),
        now,
    )
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
    let attempt_result = tokio::task::spawn_blocking(move || {
        store_clone.transition_audited(
            &created_for_attempt,
            &running_clone,
            rollshot_agent::audit::AuditEventId::new_v4(),
            now,
        )
    })
    .await;
    match attempt_result {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            return fail_attempt_start_if_running(
                store,
                task_id,
                rollshot_agent::product_task::TaskTerminal::AuditFailure {
                    category: format!("{:?}", error.audit_failure_category()),
                },
                format!("audit start attempt: {error}"),
            )
            .await;
        }
        Err(error) => {
            return fail_attempt_start_if_running(
                store,
                task_id,
                rollshot_agent::product_task::TaskTerminal::RuntimeFailure,
                format!("spawn_blocking start attempt: {error}"),
            )
            .await;
        }
    }

    // 3. Resolve bundled skill; build authority; bind run contract.
    let Some(skill_use) = bundled_action_guide_captions_use() else {
        return fail_caption_run(
            store,
            task_id,
            rollshot_agent::product_task::TaskTerminal::RuntimeFailure,
            "Caption skill not found.".to_string(),
        )
        .await;
    };

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
        rollshot_agent::product_task::SourceBinding::ActionGuideEphemeralGuide { guide_digest } => {
            AuthoritySubject::ActionGuideEphemeralGuide {
                guide_digest: guide_digest.clone(),
            }
        }
        _ => {
            return fail_caption_run(
                store,
                task_id,
                rollshot_agent::product_task::TaskTerminal::SourceValidationFailure,
                "unexpected source binding domain for captions".to_string(),
            )
            .await;
        }
    };

    let authority = match caption_authority_with_submit_grant(
        task_id.clone(),
        run_id_parsed.clone(),
        subject.clone(),
        grant_submit,
    ) {
        Ok(authority) => authority,
        Err(error) => {
            return fail_caption_run(
                store,
                task_id,
                rollshot_agent::product_task::TaskTerminal::AgentProtocolFailure,
                format!("build authority: {error}"),
            )
            .await;
        }
    };

    let receipt = authority.receipt(now);
    let run_contract = RunContractReceiptV1 {
        authority: receipt,
        skill_use: skill_use.receipt(),
        bound_at_unix_ms: now,
    };
    let bound = match running.bind_run_contract(run_contract, now) {
        Ok(bound) => bound,
        Err(error) => {
            return fail_caption_run(
                store,
                task_id,
                rollshot_agent::product_task::TaskTerminal::RuntimeFailure,
                format!("bind run contract: {error}"),
            )
            .await;
        }
    };
    let store_clone = store.clone();
    let running_for_bind = running.clone();
    let bound_clone = bound.clone();
    let bind_result = tokio::task::spawn_blocking(move || {
        store_clone.transition_audited(
            &running_for_bind,
            &bound_clone,
            rollshot_agent::audit::AuditEventId::new_v4(),
            now,
        )
    })
    .await;
    match bind_result {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            return fail_caption_run(
                store,
                task_id,
                rollshot_agent::product_task::TaskTerminal::AuditFailure {
                    category: format!("{:?}", error.audit_failure_category()),
                },
                format!("audit bind contract: {error}"),
            )
            .await;
        }
        Err(error) => {
            return fail_caption_run(
                store,
                task_id,
                rollshot_agent::product_task::TaskTerminal::RuntimeFailure,
                format!("spawn_blocking bind contract: {error}"),
            )
            .await;
        }
    }

    // 4. Build profile and authorized input.
    let prompt = format!(
        "{envelope}\n\n<rollshot-skill package=\"{pkg}\" digest=\"{digest}\">\n{body}\n</rollshot-skill>",
        envelope = rollshot_agent::driver::CAPTION_SYSTEM_ENVELOPE,
        pkg = skill_use.package_id().as_str(),
        digest = skill_use.digest(),
        body = skill_use.body(),
    );
    let profile = match SingleSubmitProfile::from_skill(
        &skill_use,
        prompt.clone(),
        caption_tool_definition(),
        caption_tool_stub(),
        rollshot_agent::authority::RunOperation::SubmitReviewCandidate,
        "rollshot::action::caption_agent",
    ) {
        Ok(profile) => profile,
        Err(error) => {
            return fail_caption_run(
                store,
                task_id,
                rollshot_agent::product_task::TaskTerminal::AgentProtocolFailure,
                format!("build caption profile: {error:?}"),
            )
            .await;
        }
    };

    let input = match rollshot_agent::domain::AuthorizedModelInput::new(
        provider.clone(),
        model.clone(),
        prompt,
        vec![],
        vec![],
    ) {
        Ok(input) => input,
        Err(error) => {
            return fail_caption_run(
                store,
                task_id,
                rollshot_agent::product_task::TaskTerminal::AgentProtocolFailure,
                format!("build model input: {error}"),
            )
            .await;
        }
    };

    // 5. Run the single-submit profile with durable authority-denial evidence.
    let runner = AgentRunner::new(AgentConfig::default());
    let audit_sink = crate::agent_store::audit_store::TaskAuditSink::new(store.clone());
    let terminal = runner
        .run_single_submit_with_provider(
            profile,
            input,
            adapter.as_ref(),
            budget,
            &cancellation,
            &authority,
            &subject,
            Some(&audit_sink),
        )
        .await;

    // 6. Decode, promote, and return only after ReadyForReview is durable.
    let drafts = match decode_caption_terminal(&terminal) {
        Ok(drafts) => drafts,
        Err(message) => {
            return fail_caption_run(store, task_id, caption_task_terminal(&terminal), message)
                .await;
        }
    };
    let proposal = rollshot_action::CaptionProposal::from_agent_drafts(
        rollshot_action::CaptionProposalId(run_id),
        run_id,
        origin,
        guide,
        drafts,
    );
    if proposal.suggestions.is_empty() {
        return fail_caption_run(
            store,
            task_id,
            rollshot_agent::product_task::TaskTerminal::AgentProtocolFailure,
            "Agent returned no usable caption suggestions.".to_string(),
        )
        .await;
    }
    let store_clone = store.clone();
    let task_id_for_promotion = task_id.clone();
    let proposal_for_promotion = proposal.clone();
    let provider_for_promotion = provider.clone();
    let model_for_promotion = model.clone();
    let promotion = tokio::task::spawn_blocking(move || {
        promote_caption_ready_for_review(
            &store_clone,
            &task_id_for_promotion,
            &proposal_for_promotion,
            &provider_for_promotion,
            &model_for_promotion,
        )
    })
    .await;
    let snapshot = match promotion {
        Ok(Ok(snapshot)) => snapshot,
        Ok(Err(message)) => {
            return fail_caption_run(
                store,
                task_id,
                rollshot_agent::product_task::TaskTerminal::RuntimeFailure,
                message,
            )
            .await;
        }
        Err(error) => {
            return fail_caption_run(
                store,
                task_id,
                rollshot_agent::product_task::TaskTerminal::RuntimeFailure,
                format!("spawn_blocking promotion: {error}"),
            )
            .await;
        }
    };

    Ok(CaptionRunSuccess {
        task_id,
        proposal,
        snapshot,
        provider_id: provider,
        model_id: model,
    })
}

// ========================================================================
// Caption proposal restore (Task 18)
// ========================================================================

/// Look for a durable caption task ready for review against this binding.
///
/// Identity and freshness are both checked by `reconcile_for_source`, which also
/// marks a same-identity stale task through its audited path. No provider call
/// is made: the proposal comes from the stored payload.
pub(crate) fn restore_caption_proposal(
    store: &crate::agent_store::TaskStore,
    binding: &rollshot_agent::product_task::SourceBinding,
    now: i64,
) -> Option<(
    rollshot_agent::product_task::ProductTaskSnapshot,
    rollshot_action::CaptionProposal,
)> {
    let snapshot = store.reconcile_for_source(binding, now).ok().flatten()?;
    if snapshot.kind() != rollshot_agent::product_task::TaskKind::ActionGuideCaptions {
        return None;
    }
    let payload = snapshot.pending_proposal_payload()?;
    match serde_json::from_slice::<rollshot_action::CaptionProposal>(payload) {
        Ok(proposal) => Some((snapshot, proposal)),
        Err(error) => {
            tracing::warn!(
                target: "rollshot::action::caption_agent",
                error = %error,
                task_id = snapshot.task_id().as_str(),
                "stored caption proposal failed to decode; not restoring"
            );
            None
        }
    }
}

#[cfg(test)]
pub(crate) mod restore_test_helpers {
    use super::*;

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

    fn caption_proposal_fixture() -> rollshot_action::CaptionProposal {
        let g = guide();
        let drafts = vec![rollshot_action::CaptionSuggestionDraft {
            step_source: 10,
            title: Some("Open Settings".into()),
            caption: "The user opens the settings panel.".into(),
            confidence: 0.85,
            rationale: Some("Click begins the flow.".into()),
        }];
        rollshot_action::CaptionProposal::from_agent_drafts(
            rollshot_action::CaptionProposalId(42),
            42,
            rollshot_action::CaptionProposalOrigin::DurableProject {
                revision: 3,
                projection_digest: "ab".repeat(32),
            },
            &g,
            drafts,
        )
    }

    /// Durable `ActionGuideProject` binding fixture for restore tests.
    pub fn action_guide_binding_fixture() -> rollshot_agent::product_task::SourceBinding {
        rollshot_agent::product_task::SourceBinding::ActionGuideProject {
            project_root_sha256: [0xAA; 32],
            revision: 3,
            projection_digest: "ab".repeat(32),
        }
    }

    #[allow(dead_code)]
    fn promote_caption_task_for_tests(
        binding: &rollshot_agent::product_task::SourceBinding,
        proposal: &rollshot_action::CaptionProposal,
    ) -> rollshot_agent::product_task::ProductTaskSnapshot {
        use rollshot_agent::product_task::{
            ArtifactId, ArtifactKind, ArtifactRevision, ArtifactSummary, ProductArtifactMetadata,
            ProductTaskSnapshot, RunContractReceiptV1, TaskAttempt, TaskAttemptId, TaskKind,
        };
        use sha2::{Digest, Sha256};

        let task_id = test_task_id();
        let run_id = test_run_id();
        let now: i64 = 5_000;

        let created = ProductTaskSnapshot::new_v3(
            task_id.clone(),
            TaskKind::ActionGuideCaptions,
            binding.clone(),
            now,
        )
        .unwrap();
        let attempt = TaskAttempt::new(TaskAttemptId::new(1), run_id.clone(), now);
        let running = created.start_attempt(attempt, now).unwrap();

        let subject = match binding {
            rollshot_agent::product_task::SourceBinding::ActionGuideProject {
                project_root_sha256,
                revision,
                projection_digest,
            } => rollshot_agent::authority::AuthoritySubject::ActionGuideProject {
                project_root_sha256: *project_root_sha256,
                revision: *revision,
                projection_digest: projection_digest.clone(),
            },
            rollshot_agent::product_task::SourceBinding::ActionGuideEphemeralGuide {
                guide_digest,
            } => rollshot_agent::authority::AuthoritySubject::ActionGuideEphemeralGuide {
                guide_digest: guide_digest.clone(),
            },
            _ => panic!("unexpected binding domain"),
        };
        let authority = caption_authority(task_id.clone(), run_id.clone(), subject).unwrap();
        let run_contract = RunContractReceiptV1 {
            authority: authority.receipt(now),
            skill_use: rollshot_agent::skills::bundled_action_guide_captions_use()
                .unwrap()
                .receipt(),
            bound_at_unix_ms: now,
        };
        let bound = running.bind_run_contract(run_contract, now).unwrap();

        let payload_bytes = caption_artifact_payload(proposal);
        let meta = ProductArtifactMetadata::new_v3(
            ArtifactId::parse("artifact-00000000-0000-4000-8000-000000000001").unwrap(),
            ArtifactRevision::new(1),
            ArtifactKind::ActionGuideCaptions,
            1,
            format!("{:x}", Sha256::digest(&payload_bytes)),
            binding.clone(),
            task_id,
            TaskAttemptId::new(1),
            run_id,
            proposal.id.0.to_string(),
            "test-provider".to_string(),
            "test-model".to_string(),
            "run-config-digest".to_string(),
            ArtifactSummary::ActionGuideCaptions {
                suggestion_count: proposal.suggestions.len() as u32,
            },
            now,
        );

        bound
            .record_ready_for_review(meta, payload_bytes, None, now)
            .unwrap()
    }

    /// Seed a ready-for-review caption task in the store with the given binding.
    /// Returns the task id.
    pub fn seed_ready_for_review_caption_task(
        store: &crate::agent_store::TaskStore,
        binding: &rollshot_agent::product_task::SourceBinding,
    ) -> rollshot_agent::product_task::ProductTaskId {
        let proposal = caption_proposal_fixture();
        seed_ready_for_review_caption_task_with_payload(
            store,
            binding,
            serde_json::to_vec(&proposal).unwrap(),
        )
    }

    /// Seed a ready-for-review caption task with a custom proposal payload.
    pub fn seed_ready_for_review_caption_task_with_payload(
        store: &crate::agent_store::TaskStore,
        binding: &rollshot_agent::product_task::SourceBinding,
        proposal_payload: Vec<u8>,
    ) -> rollshot_agent::product_task::ProductTaskId {
        use rollshot_agent::product_task::{
            ArtifactId, ArtifactKind, ArtifactRevision, ArtifactSummary, ProductArtifactMetadata,
            ProductTaskSnapshot, RunContractReceiptV1, TaskAttempt, TaskAttemptId, TaskKind,
        };
        use sha2::{Digest, Sha256};

        let proposal = caption_proposal_fixture();
        let task_id = test_task_id();
        let run_id = test_run_id();
        let now: i64 = 5_000;
        let created = ProductTaskSnapshot::new_v3(
            task_id.clone(),
            TaskKind::ActionGuideCaptions,
            binding.clone(),
            now,
        )
        .unwrap();
        let attempt = TaskAttempt::new(TaskAttemptId::new(1), run_id.clone(), now);
        let running = created.start_attempt(attempt, now).unwrap();

        let subject = match binding {
            rollshot_agent::product_task::SourceBinding::ActionGuideProject {
                project_root_sha256,
                revision,
                projection_digest,
            } => rollshot_agent::authority::AuthoritySubject::ActionGuideProject {
                project_root_sha256: *project_root_sha256,
                revision: *revision,
                projection_digest: projection_digest.clone(),
            },
            rollshot_agent::product_task::SourceBinding::ActionGuideEphemeralGuide {
                guide_digest,
            } => rollshot_agent::authority::AuthoritySubject::ActionGuideEphemeralGuide {
                guide_digest: guide_digest.clone(),
            },
            _ => panic!("unexpected binding domain"),
        };
        let authority = caption_authority(task_id.clone(), run_id.clone(), subject).unwrap();
        let run_contract = RunContractReceiptV1 {
            authority: authority.receipt(now),
            skill_use: rollshot_agent::skills::bundled_action_guide_captions_use()
                .unwrap()
                .receipt(),
            bound_at_unix_ms: now,
        };
        let bound = running.bind_run_contract(run_contract, now).unwrap();

        let payload_bytes = caption_artifact_payload(&proposal);
        let meta = ProductArtifactMetadata::new_v3(
            ArtifactId::parse("artifact-00000000-0000-4000-8000-000000000001").unwrap(),
            ArtifactRevision::new(1),
            ArtifactKind::ActionGuideCaptions,
            1,
            format!("{:x}", Sha256::digest(&payload_bytes)),
            binding.clone(),
            task_id.clone(),
            TaskAttemptId::new(1),
            run_id,
            proposal.id.0.to_string(),
            "test-provider".to_string(),
            "test-model".to_string(),
            "run-config-digest".to_string(),
            ArtifactSummary::ActionGuideCaptions {
                suggestion_count: proposal.suggestions.len() as u32,
            },
            now,
        );

        let ready_with_payload = bound
            .record_ready_for_review(meta, payload_bytes, Some(proposal_payload), now)
            .unwrap();
        store.create(&ready_with_payload).unwrap();
        ready_with_payload.task_id().clone()
    }

    /// Return the same binding with a bumped revision (freshness mismatch).
    pub fn bump_revision(
        binding: &rollshot_agent::product_task::SourceBinding,
    ) -> rollshot_agent::product_task::SourceBinding {
        match binding {
            rollshot_agent::product_task::SourceBinding::ActionGuideProject {
                project_root_sha256,
                revision,
                projection_digest,
            } => rollshot_agent::product_task::SourceBinding::ActionGuideProject {
                project_root_sha256: *project_root_sha256,
                revision: revision + 1,
                projection_digest: projection_digest.clone(),
            },
            _ => panic!("bump_revision only supports ActionGuideProject"),
        }
    }

    /// Return the same kind of binding but with a different project root (identity mismatch).
    pub fn with_different_project_root(
        binding: &rollshot_agent::product_task::SourceBinding,
    ) -> rollshot_agent::product_task::SourceBinding {
        match binding {
            rollshot_agent::product_task::SourceBinding::ActionGuideProject {
                revision,
                projection_digest,
                ..
            } => rollshot_agent::product_task::SourceBinding::ActionGuideProject {
                project_root_sha256: [0xBB; 32],
                revision: *revision,
                projection_digest: projection_digest.clone(),
            },
            _ => panic!("with_different_project_root only supports ActionGuideProject"),
        }
    }

    /// Provider adapter that panics if `stream` is ever called. Used to prove
    /// that `restore_caption_proposal` makes no provider calls.
    pub struct PanicProvider;

    impl rollshot_agent::ProviderAdapter for PanicProvider {
        fn stream(
            &self,
            _request: rollshot_agent::model::ModelRequest,
            _bounds: rollshot_agent::StreamBounds,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            std::pin::Pin<
                                Box<
                                    dyn iced::futures::Stream<
                                            Item = Result<
                                                rollshot_agent::model::ModelStreamEvent,
                                                rollshot_agent::model::ModelError,
                                            >,
                                        > + Send,
                                >,
                            >,
                            rollshot_agent::model::ModelError,
                        >,
                    > + Send,
            >,
        > {
            panic!("PanicProvider::stream must not be called during restore")
        }
    }

    /// `restore_caption_proposal` with an explicit provider argument (used to
    /// prove no provider call is made). The provider is unused except to hold
    /// the panicking mock.
    pub fn restore_caption_proposal_with_provider(
        store: &crate::agent_store::TaskStore,
        binding: &rollshot_agent::product_task::SourceBinding,
        now: i64,
        _provider: &dyn rollshot_agent::ProviderAdapter,
    ) -> Option<(
        rollshot_agent::product_task::ProductTaskSnapshot,
        rollshot_action::CaptionProposal,
    )> {
        restore_caption_proposal(store, binding, now)
    }
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
pub(crate) mod provider_tests {
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
    pub(crate) fn durable_context(root: &std::path::Path) -> (PreparedCaptionContext, u64, String) {
        use rollshot_action::project::{
            create_project, load_project, ActionGuideContextProjectionV1, EnabledOutputs,
            ProjectSnapshot, ProjectStep, ProjectStepId, SnapshotFrame, SnapshotFramePayload,
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
            motion: None,
        };
        create_project(&snapshot, &project_dir).unwrap();
        let loaded = load_project(&project_dir, None).unwrap();
        let projection = ActionGuideContextProjectionV1::from_loaded_project(&loaded).unwrap();
        let revision = projection.revision();
        let digest = projection.digest().to_owned();
        let guide = projection.to_guide().unwrap();
        (
            PreparedCaptionContext::Durable { guide, projection },
            revision,
            digest,
        )
    }

    #[allow(dead_code)]
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
                authority
                    .authorize_tool(&run_id, &subject, forbidden)
                    .is_err(),
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

    // ---- Task 17: Artifact promotion and review receipt ----

    /// One-suggestion caption proposal fixture. `from_agent_drafts` assigns
    /// `CaptionSuggestionId(index + 1)` — so id is `1`.
    pub(crate) fn caption_proposal_fixture() -> rollshot_action::CaptionProposal {
        let guide = guide();
        let drafts = vec![rollshot_action::CaptionSuggestionDraft {
            step_source: 10,
            title: Some("Open Settings".into()),
            caption: "The user opens the settings panel.".into(),
            confidence: 0.85,
            rationale: Some("Click begins the flow.".into()),
        }];
        rollshot_action::CaptionProposal::from_agent_drafts(
            rollshot_action::CaptionProposalId(42),
            42,
            rollshot_action::CaptionProposalOrigin::EphemeralGuide {
                guide_digest: "0".repeat(64),
            },
            &guide,
            drafts,
        )
    }

    /// Metadata fixture whose proposal_id matches `caption_proposal_fixture`.
    pub(crate) fn caption_artifact_metadata_fixture(
    ) -> rollshot_agent::product_task::ProductArtifactMetadata {
        use rollshot_agent::product_task::{
            ArtifactId, ArtifactKind, ArtifactRevision, ArtifactSummary, ProductArtifactMetadata,
            TaskAttemptId,
        };
        ProductArtifactMetadata::new_v3(
            ArtifactId::parse("artifact-00000000-0000-4000-8000-000000000001").unwrap(),
            ArtifactRevision::new(1),
            ArtifactKind::ActionGuideCaptions,
            1,
            "".to_string(),
            caption_source_binding(&ephemeral_context(), None),
            test_task_id(),
            TaskAttemptId::new(1),
            test_run_id(),
            "42".to_string(),
            "test-provider".to_string(),
            "test-model".to_string(),
            "run-config-digest".to_string(),
            ArtifactSummary::ActionGuideCaptions {
                suggestion_count: 1,
            },
            5_000,
        )
    }

    /// Promote a caption proposal through the full Created → Running →
    /// ReadyForReview lifecycle and return the `ReadyForReview` snapshot.
    pub(crate) fn promote_caption_task_for_tests(
        binding: &rollshot_agent::product_task::SourceBinding,
        proposal: &rollshot_action::CaptionProposal,
    ) -> rollshot_agent::product_task::ProductTaskSnapshot {
        use rollshot_agent::product_task::{
            ArtifactId, ArtifactKind, ArtifactRevision, ArtifactSummary, ProductArtifactMetadata,
            ProductTaskSnapshot, RunContractReceiptV1, TaskAttempt, TaskAttemptId, TaskKind,
        };
        use sha2::{Digest, Sha256};

        let task_id = test_task_id();
        let run_id = test_run_id();
        let now: i64 = 5_000;

        let created = ProductTaskSnapshot::new_v3(
            task_id.clone(),
            TaskKind::ActionGuideCaptions,
            binding.clone(),
            now,
        )
        .unwrap();
        let attempt = TaskAttempt::new(TaskAttemptId::new(1), run_id.clone(), now);
        let running = created.start_attempt(attempt, now).unwrap();

        // Minimal run-contract binding — the test does not exercise provenance.
        let subject = match binding {
            rollshot_agent::product_task::SourceBinding::ActionGuideProject {
                project_root_sha256,
                revision,
                projection_digest,
            } => rollshot_agent::authority::AuthoritySubject::ActionGuideProject {
                project_root_sha256: *project_root_sha256,
                revision: *revision,
                projection_digest: projection_digest.clone(),
            },
            rollshot_agent::product_task::SourceBinding::ActionGuideEphemeralGuide {
                guide_digest,
            } => rollshot_agent::authority::AuthoritySubject::ActionGuideEphemeralGuide {
                guide_digest: guide_digest.clone(),
            },
            _ => panic!("unexpected binding domain"),
        };
        let authority = caption_authority(task_id.clone(), run_id.clone(), subject).unwrap();
        let run_contract = RunContractReceiptV1 {
            authority: authority.receipt(now),
            skill_use: rollshot_agent::skills::bundled_action_guide_captions_use()
                .unwrap()
                .receipt(),
            bound_at_unix_ms: now,
        };
        let bound = running.bind_run_contract(run_contract, now).unwrap();

        let payload_bytes = caption_artifact_payload(proposal);
        let meta = ProductArtifactMetadata::new_v3(
            ArtifactId::parse("artifact-00000000-0000-4000-8000-000000000001").unwrap(),
            ArtifactRevision::new(1),
            ArtifactKind::ActionGuideCaptions,
            1,
            format!("{:x}", Sha256::digest(&payload_bytes)),
            binding.clone(),
            task_id,
            TaskAttemptId::new(1),
            run_id,
            proposal.id.0.to_string(),
            "test-provider".to_string(),
            "test-model".to_string(),
            "run-config-digest".to_string(),
            ArtifactSummary::ActionGuideCaptions {
                suggestion_count: proposal.suggestions.len() as u32,
            },
            now,
        );

        bound
            .record_ready_for_review(meta, payload_bytes, None, now)
            .unwrap()
    }

    #[test]
    fn artifact_payload_carries_suggestions_and_nothing_else() {
        let proposal = caption_proposal_fixture();

        let bytes = caption_artifact_payload(&proposal);
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(json["suggestions"].as_array().unwrap().len(), 1);
        assert!(json.get("guide").is_none(), "no whole-guide copy: {json}");
    }

    #[test]
    fn review_receipt_partitions_decisions_and_binds_the_artifact_revision() {
        let mut proposal = caption_proposal_fixture();
        proposal.reject(rollshot_action::CaptionSuggestionId(1));
        let metadata = caption_artifact_metadata_fixture();

        let receipt = caption_review_receipt(&proposal, &metadata, 5_000).unwrap();

        assert_eq!(receipt.artifact_revision, metadata.artifact_revision());
        assert_eq!(receipt.rejected_candidates, vec![1]);
        assert!(receipt.applied_candidates.is_empty());
        assert!(receipt.local_delta.moved_candidates.is_empty());
        assert!(receipt.local_delta.manual_additions.is_empty());
        assert_eq!(receipt.resulting_document_state_id, None);
    }

    #[test]
    fn suggestion_ids_above_u32_are_rejected_not_truncated() {
        let mut proposal = caption_proposal_fixture();
        let oversized = rollshot_action::CaptionSuggestionId(u64::from(u32::MAX) + 1);
        proposal.suggestions[0].id = oversized;
        // The narrowing guard only runs for decided suggestions
        // (Accepted / Rejected / Stale). A Pending suggestion is skipped
        // entirely, so without this line the test would pass for the wrong
        // reason — `caption_review_receipt` would return Ok and `is_err()`
        // would fail.
        assert!(
            proposal.reject(oversized),
            "reject must find the mutated id"
        );
        let metadata = caption_artifact_metadata_fixture();

        let err = caption_review_receipt(&proposal, &metadata, 5_000)
            .expect_err("an out-of-range suggestion id must be rejected");
        assert!(err.contains("exceeds u32"), "unexpected error: {err}");
    }

    #[test]
    fn accepted_suggestions_land_in_applied_not_rejected() {
        // The mirror of the reject case: without this, the Accepted arm of the
        // partition is never exercised and could be swapped with Rejected
        // without any test noticing.
        let mut proposal = caption_proposal_fixture();
        proposal.suggestions[0].status = rollshot_action::CaptionSuggestionStatus::Accepted;
        let metadata = caption_artifact_metadata_fixture();

        let receipt = caption_review_receipt(&proposal, &metadata, 5_000).unwrap();

        assert_eq!(receipt.applied_candidates, vec![1]);
        assert!(receipt.rejected_candidates.is_empty());
    }

    #[test]
    fn stale_suggestions_are_recorded_as_rejected() {
        let mut proposal = caption_proposal_fixture();
        proposal.suggestions[0].status = rollshot_action::CaptionSuggestionStatus::Stale;
        let metadata = caption_artifact_metadata_fixture();

        let receipt = caption_review_receipt(&proposal, &metadata, 5_000).unwrap();

        assert_eq!(receipt.rejected_candidates, vec![1]);
    }

    #[test]
    fn promotion_binds_the_kind_the_origin_and_the_payload_digest() {
        // Gate A1 item 2 and spec §8 item 2: both origins, with the recorded
        // binding and canonical_payload_sha256 asserted, not just the payload
        // shape.
        use sha2::{Digest, Sha256};
        let root = tempfile::tempdir().unwrap();

        for (label, binding) in [
            (
                "durable",
                caption_source_binding(&durable_context(root.path()).0, Some(root.path())),
            ),
            (
                "ephemeral",
                caption_source_binding(&ephemeral_context(), None),
            ),
        ] {
            let proposal = caption_proposal_fixture();
            let bytes = caption_artifact_payload(&proposal);
            let ready = promote_caption_task_for_tests(&binding, &proposal);
            let meta = ready.artifact_metadata().expect(label);

            assert_eq!(
                meta.kind(),
                rollshot_agent::product_task::ArtifactKind::ActionGuideCaptions
            );
            assert_eq!(meta.source_binding(), &binding, "{label}");
            assert_eq!(
                meta.summary(),
                &rollshot_agent::product_task::ArtifactSummary::ActionGuideCaptions {
                    suggestion_count: proposal.suggestions.len() as u32,
                },
                "{label}"
            );
            assert_eq!(
                meta.canonical_payload_sha256(),
                format!("{:x}", Sha256::digest(&bytes)),
                "{label}: digest must cover exactly the promoted bytes"
            );
            assert_eq!(
                ready.pending_artifact_payload(),
                Some(bytes.as_slice()),
                "{label}"
            );
        }
    }
    struct ScriptedCaptionProvider {
        events: Vec<rollshot_agent::model::ModelStreamEvent>,
    }

    impl rollshot_agent::ProviderAdapter for ScriptedCaptionProvider {
        fn stream(
            &self,
            _request: rollshot_agent::model::ModelRequest,
            _bounds: rollshot_agent::StreamBounds,
        ) -> std::pin::Pin<
            Box<
                dyn Future<
                        Output = Result<
                            std::pin::Pin<
                                Box<
                                    dyn iced::futures::Stream<
                                            Item = Result<
                                                rollshot_agent::model::ModelStreamEvent,
                                                rollshot_agent::model::ModelError,
                                            >,
                                        > + Send,
                                >,
                            >,
                            rollshot_agent::model::ModelError,
                        >,
                    > + Send
                    + '_,
            >,
        > {
            let events = self.events.clone();
            Box::pin(async move {
                Ok(
                    Box::pin(iced::futures::stream::iter(events.into_iter().map(Ok)))
                        as std::pin::Pin<
                            Box<
                                dyn iced::futures::Stream<
                                        Item = Result<
                                            rollshot_agent::model::ModelStreamEvent,
                                            rollshot_agent::model::ModelError,
                                        >,
                                    > + Send,
                            >,
                        >,
                )
            })
        }
    }

    pub(crate) fn caption_tool_provider(source: u64) -> Box<dyn rollshot_agent::ProviderAdapter> {
        use rollshot_agent::model::{ModelCompletion, ModelStreamEvent, ModelUsage, StopReason};

        let arguments = serde_json::json!({
            "suggestions": [{
                "source": source,
                "title": "Open Settings",
                "caption": "The user opens the settings panel.",
                "confidence": 0.85,
                "rationale": "The click begins the flow."
            }]
        })
        .to_string();
        Box::new(ScriptedCaptionProvider {
            events: vec![
                ModelStreamEvent::ToolCallStart {
                    id: "tc_1".to_owned(),
                    name: "submit_caption_suggestions".to_owned(),
                },
                ModelStreamEvent::ToolCallArgumentDelta {
                    id: "tc_1".to_owned(),
                    delta: arguments,
                },
                ModelStreamEvent::Completed(ModelCompletion {
                    usage: ModelUsage {
                        input_tokens: 5,
                        output_tokens: 3,
                        total_tokens: 8,
                    },
                    stop_reason: StopReason::ToolUse,
                }),
            ],
        })
    }

    #[test]
    fn real_worker_promotes_both_caption_payloads_before_success() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();

        let success = run(suggest_captions_task(
            42,
            store.clone(),
            rollshot_agent::runtime::RunCancellation::new(),
            "test-model".to_owned(),
            "test-provider".to_owned(),
            caption_tool_provider(10),
            ephemeral_context(),
            None,
        ))
        .unwrap();

        let snapshot = store.load(&success.task_id).unwrap();
        assert_eq!(
            snapshot.status(),
            rollshot_agent::product_task::TaskStatus::ReadyForReview
        );
        assert!(snapshot.pending_artifact_payload().is_some());
        let proposal_payload = snapshot
            .pending_proposal_payload()
            .expect("serialized proposal is retained for restore");
        let restored: rollshot_action::CaptionProposal =
            serde_json::from_slice(proposal_payload).unwrap();

        assert_eq!(restored, success.proposal);
        assert_eq!(snapshot, success.snapshot);
    }

    #[test]
    fn real_worker_terminalizes_attempt_audit_commit_failure() {
        let dir = tempfile::tempdir().unwrap();
        let store = std::sync::Arc::new(
            crate::agent_store::TaskStore::open_with_failpoint(
                dir.path(),
                crate::agent_store::Failpoint::AuditCommit,
            )
            .unwrap(),
        );

        let result = run(suggest_captions_task(
            42,
            store.clone(),
            rollshot_agent::runtime::RunCancellation::new(),
            "test-model".to_owned(),
            "test-provider".to_owned(),
            caption_tool_provider(10),
            ephemeral_context(),
            None,
        ));

        assert!(result.is_err());
        let snapshot = only_stored_task(&store);
        assert!(matches!(
            snapshot.status(),
            rollshot_agent::product_task::TaskStatus::Failed {
                terminal: rollshot_agent::product_task::TaskTerminal::AuditFailure { .. }
            }
        ));
    }

    fn only_stored_task(
        store: &crate::agent_store::TaskStore,
    ) -> rollshot_agent::product_task::ProductTaskSnapshot {
        let task_ids: Vec<_> = std::fs::read_dir(store.tasks_dir())
            .unwrap()
            .map(|entry| {
                let filename = entry.unwrap().file_name().into_string().unwrap();
                let id = filename.strip_suffix(".json").unwrap();
                rollshot_agent::product_task::ProductTaskId::parse(id).unwrap()
            })
            .collect();
        assert_eq!(task_ids.len(), 1);
        store.load(&task_ids[0]).unwrap()
    }

    #[test]
    fn real_worker_persists_cancellation_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();
        let cancellation = rollshot_agent::runtime::RunCancellation::new();
        cancellation.cancel();

        let result = run(suggest_captions_task(
            42,
            store.clone(),
            cancellation,
            "test-model".to_owned(),
            "test-provider".to_owned(),
            caption_tool_provider(10),
            ephemeral_context(),
            None,
        ));

        assert!(result.is_err());
        let snapshot = only_stored_task(&store);
        assert_eq!(
            snapshot.status(),
            rollshot_agent::product_task::TaskStatus::Cancelled
        );
        let kinds: Vec<_> = store
            .committed_audit_events(snapshot.task_id())
            .unwrap()
            .into_iter()
            .map(|event| event.event().kind())
            .collect();
        assert!(kinds.contains(&rollshot_agent::audit::AuditEventKindV1::TaskTerminated));
    }

    fn provider_with_events(
        events: Vec<rollshot_agent::model::ModelStreamEvent>,
    ) -> Box<dyn rollshot_agent::ProviderAdapter> {
        Box::new(ScriptedCaptionProvider { events })
    }

    fn assert_worker_terminal(
        adapter: Box<dyn rollshot_agent::ProviderAdapter>,
        expected: rollshot_agent::product_task::TaskTerminal,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();

        let result = run(suggest_captions_task(
            42,
            store.clone(),
            rollshot_agent::runtime::RunCancellation::new(),
            "test-model".to_owned(),
            "test-provider".to_owned(),
            adapter,
            ephemeral_context(),
            None,
        ));

        assert!(result.is_err());
        let snapshot = only_stored_task(&store);
        assert_eq!(
            snapshot.status(),
            rollshot_agent::product_task::TaskStatus::Failed { terminal: expected }
        );
        let kinds: Vec<_> = store
            .committed_audit_events(snapshot.task_id())
            .unwrap()
            .into_iter()
            .map(|event| event.event().kind())
            .collect();
        assert!(kinds.contains(&rollshot_agent::audit::AuditEventKindV1::TaskTerminated));
    }

    #[test]
    fn real_worker_persists_provider_and_decode_failures() {
        use rollshot_agent::model::{
            ModelCompletion, ModelError, ModelStreamEvent, ModelUsage, StopReason,
        };

        assert_worker_terminal(
            provider_with_events(vec![ModelStreamEvent::Error(ModelError::ProviderFailure(
                "rate limited".to_owned(),
            ))]),
            rollshot_agent::product_task::TaskTerminal::ProviderFailure,
        );
        assert_worker_terminal(
            provider_with_events(vec![
                ModelStreamEvent::TextDelta("not caption json".to_owned()),
                ModelStreamEvent::Completed(ModelCompletion {
                    usage: ModelUsage {
                        input_tokens: 5,
                        output_tokens: 3,
                        total_tokens: 8,
                    },
                    stop_reason: StopReason::EndTurn,
                }),
            ]),
            rollshot_agent::product_task::TaskTerminal::AgentProtocolFailure,
        );
    }

    #[test]
    fn real_worker_persists_wall_time_and_authority_failures() {
        let timeout_dir = tempfile::tempdir().unwrap();
        let timeout_store = crate::agent_store::open_process_store(timeout_dir.path()).unwrap();
        let timeout_budget = rollshot_agent::runtime::RunBudget {
            wall_time: std::time::Duration::ZERO,
            ..rollshot_agent::captions::caption_run_budget()
        };
        let timeout_result = run(suggest_captions_with_store(
            42,
            timeout_store.clone(),
            rollshot_agent::runtime::RunCancellation::new(),
            "test-model".to_owned(),
            "test-provider".to_owned(),
            caption_tool_provider(10),
            ephemeral_context(),
            None,
            timeout_budget,
            true,
        ));
        assert!(timeout_result.is_err());
        assert!(matches!(
            only_stored_task(&timeout_store).status(),
            rollshot_agent::product_task::TaskStatus::Failed {
                terminal: rollshot_agent::product_task::TaskTerminal::BudgetExhausted { .. }
            }
        ));

        let denied_dir = tempfile::tempdir().unwrap();
        let denied_store = crate::agent_store::open_process_store(denied_dir.path()).unwrap();
        let denied_result = run(suggest_captions_with_store(
            42,
            denied_store.clone(),
            rollshot_agent::runtime::RunCancellation::new(),
            "test-model".to_owned(),
            "test-provider".to_owned(),
            caption_tool_provider(10),
            ephemeral_context(),
            None,
            rollshot_agent::captions::caption_run_budget(),
            false,
        ));
        assert!(denied_result.is_err());
        let denied = only_stored_task(&denied_store);
        assert!(matches!(
            denied.status(),
            rollshot_agent::product_task::TaskStatus::Failed {
                terminal: rollshot_agent::product_task::TaskTerminal::AgentProtocolFailure
            }
        ));
        let kinds: Vec<_> = denied_store
            .committed_audit_events(denied.task_id())
            .unwrap()
            .into_iter()
            .map(|event| event.event().kind())
            .collect();
        assert!(kinds.contains(&rollshot_agent::audit::AuditEventKindV1::AuthorityDenied));
        assert!(kinds.contains(&rollshot_agent::audit::AuditEventKindV1::TaskTerminated));
    }
}

// ========================================================================
// Audit coverage and privacy tests (Task 20)
// ========================================================================

#[cfg(test)]
mod audit_tests {
    use super::*;
    use rollshot_agent::audit::{AuditEventId, AuditEventKindV1};
    use rollshot_agent::authority::{
        AuthorityBinding, AuthoritySnapshot, AuthoritySubject, DisclosureCeiling,
    };
    use rollshot_agent::product_task::{
        ArtifactId, ArtifactKind, ArtifactRevision, ArtifactSummary, ProductArtifactMetadata,
        ProductTaskId, ProductTaskSnapshot, RunContractReceiptV1, TaskAttempt, TaskAttemptId,
        TaskKind, TaskTerminal,
    };
    use sha2::{Digest, Sha256};

    fn test_task_id() -> ProductTaskId {
        ProductTaskId::parse("task-00000000-0000-4000-8000-000000000001").unwrap()
    }

    fn test_run_id() -> rollshot_agent::domain::RunId {
        rollshot_agent::domain::RunId::parse("run-00000000-0000-4000-8000-000000000001").unwrap()
    }

    fn ephemeral_binding() -> rollshot_agent::product_task::SourceBinding {
        rollshot_agent::product_task::SourceBinding::ActionGuideEphemeralGuide {
            guide_digest: "0".repeat(64),
        }
    }

    fn caption_proposal_fixture() -> rollshot_action::CaptionProposal {
        let guide = rollshot_action::Guide::from_candidates(vec![rollshot_action::CandidateStep {
            id: 10,
            kind: rollshot_action::CandidateKind::Click,
            reason: rollshot_action::DetectReason::ClickConfirmed,
            at_ms: 120,
            keyframe: 1,
            nearby: vec![1],
        }]);
        let drafts = vec![rollshot_action::CaptionSuggestionDraft {
            step_source: 10,
            title: Some("Open Settings".into()),
            caption: "The settings panel appears.".into(),
            confidence: 0.85,
            rationale: Some("Click begins the flow.".into()),
        }];
        rollshot_action::CaptionProposal::from_agent_drafts(
            rollshot_action::CaptionProposalId(42),
            42,
            rollshot_action::CaptionProposalOrigin::EphemeralGuide {
                guide_digest: "0".repeat(64),
            },
            &guide,
            drafts,
        )
    }

    fn build_run_contract(
        task_id: &ProductTaskId,
        run_id: &rollshot_agent::domain::RunId,
    ) -> RunContractReceiptV1 {
        let subject = AuthoritySubject::ActionGuideEphemeralGuide {
            guide_digest: "0".repeat(64),
        };
        let authority = caption_authority(task_id.clone(), run_id.clone(), subject).unwrap();
        RunContractReceiptV1 {
            authority: authority.receipt(20),
            skill_use: rollshot_agent::skills::bundled_action_guide_captions_use()
                .unwrap()
                .receipt(),
            bound_at_unix_ms: 20,
        }
    }

    fn build_meta(
        task_id: &ProductTaskId,
        run_id: &rollshot_agent::domain::RunId,
        binding: &rollshot_agent::product_task::SourceBinding,
        proposal: &rollshot_action::CaptionProposal,
        payload_bytes: &[u8],
    ) -> ProductArtifactMetadata {
        ProductArtifactMetadata::new_v3(
            ArtifactId::parse("artifact-00000000-0000-4000-8000-000000000001").unwrap(),
            ArtifactRevision::new(1),
            ArtifactKind::ActionGuideCaptions,
            1,
            format!("{:x}", Sha256::digest(payload_bytes)),
            binding.clone(),
            task_id.clone(),
            TaskAttemptId::new(1),
            run_id.clone(),
            proposal.id.0.to_string(),
            "test-provider".to_string(),
            "test-model".to_string(),
            "run-config-digest".to_string(),
            ArtifactSummary::ActionGuideCaptions {
                suggestion_count: proposal.suggestions.len() as u32,
            },
            30,
        )
    }

    /// Drive a caption task through the full happy-path lifecycle using
    /// audited store methods. Returns the task id.
    fn drive_full_caption_lifecycle(store: &crate::agent_store::TaskStore) -> ProductTaskId {
        let task_id = test_task_id();
        let run_id = test_run_id();
        let binding = ephemeral_binding();
        let proposal = caption_proposal_fixture();
        let payload_bytes = caption_artifact_payload(&proposal);

        // 1. TaskCreated.
        let created = ProductTaskSnapshot::new_v3(
            task_id.clone(),
            TaskKind::ActionGuideCaptions,
            binding.clone(),
            10,
        )
        .unwrap();
        store
            .create_audited(&created, AuditEventId::new_v4(), 10)
            .unwrap();

        // 2. AttemptStarted.
        let attempt = TaskAttempt::new(TaskAttemptId::new(1), run_id.clone(), 15);
        let running = created.start_attempt(attempt, 15).unwrap();
        store
            .transition_audited(&created, &running, AuditEventId::new_v4(), 15)
            .unwrap();

        // 3. RunContractBound.
        let contract = build_run_contract(&task_id, &run_id);
        let bound = running.bind_run_contract(contract, 20).unwrap();
        store
            .transition_audited(&running, &bound, AuditEventId::new_v4(), 20)
            .unwrap();

        // 4. ArtifactPromoted (Running → ReadyForReview).
        let meta = build_meta(&task_id, &run_id, &binding, &proposal, &payload_bytes);
        let ready = bound
            .record_ready_for_review(meta, payload_bytes, None, 30)
            .unwrap();
        store
            .transition_audited(&bound, &ready, AuditEventId::new_v4(), 30)
            .unwrap();

        // 5. ReviewApplyStarted (ReadyForReview → Applying).
        let applying = ready.begin_apply(35).unwrap();
        store
            .transition_audited(&ready, &applying, AuditEventId::new_v4(), 35)
            .unwrap();

        // 6. ReviewDecisionCommitted (Applying → Completed).
        let metadata = applying.artifact_metadata().unwrap();
        let receipt = caption_review_receipt(&proposal, metadata, 40).unwrap();
        let completed = applying.complete_apply(receipt, 40).unwrap();
        store
            .transition_audited(&applying, &completed, AuditEventId::new_v4(), 40)
            .unwrap();

        task_id
    }

    /// Drive a caption task to Running, then record a terminal condition.
    fn drive_caption_lifecycle_to_terminal(
        store: &crate::agent_store::TaskStore,
        terminal: TaskTerminal,
    ) -> ProductTaskId {
        let task_id = test_task_id();
        let run_id = test_run_id();
        let binding = ephemeral_binding();

        // 1. TaskCreated.
        let created = ProductTaskSnapshot::new_v3(
            task_id.clone(),
            TaskKind::ActionGuideCaptions,
            binding,
            10,
        )
        .unwrap();
        store
            .create_audited(&created, AuditEventId::new_v4(), 10)
            .unwrap();

        // 2. AttemptStarted.
        let attempt = TaskAttempt::new(TaskAttemptId::new(1), run_id, 15);
        let running = created.start_attempt(attempt, 15).unwrap();
        store
            .transition_audited(&created, &running, AuditEventId::new_v4(), 15)
            .unwrap();

        // 3. TaskTerminated (Running → Failed).
        let failed = running.record_terminal(terminal, 20).unwrap();
        store
            .transition_audited(&running, &failed, AuditEventId::new_v4(), 20)
            .unwrap();

        task_id
    }

    /// Drive a caption task to Running with a no-grants authority bound,
    /// then append an AuthorityDenied standalone event. No promotion occurs
    /// because the submit is denied before the provider can produce results.
    fn drive_caption_lifecycle_with_no_grants(
        store: &crate::agent_store::TaskStore,
    ) -> ProductTaskId {
        let task_id = test_task_id();
        let run_id = test_run_id();
        let binding = ephemeral_binding();

        // 1. TaskCreated.
        let created = ProductTaskSnapshot::new_v3(
            task_id.clone(),
            TaskKind::ActionGuideCaptions,
            binding,
            10,
        )
        .unwrap();
        store
            .create_audited(&created, AuditEventId::new_v4(), 10)
            .unwrap();

        // 2. AttemptStarted.
        let attempt = TaskAttempt::new(TaskAttemptId::new(1), run_id.clone(), 15);
        let running = created.start_attempt(attempt, 15).unwrap();
        store
            .transition_audited(&created, &running, AuditEventId::new_v4(), 15)
            .unwrap();

        // 3. RunContractBound with a NO-GRANTS authority.
        let subject = AuthoritySubject::ActionGuideEphemeralGuide {
            guide_digest: "0".repeat(64),
        };
        let authority = AuthoritySnapshot::new(
            AuthorityBinding::new(
                task_id.clone(),
                TaskAttemptId::new(1),
                run_id.clone(),
                subject,
            ),
            "no-grants".into(),
            DisclosureCeiling::TextMetadataOnly,
            false,
            std::collections::BTreeSet::new(),
            std::collections::BTreeSet::new(),
        )
        .unwrap();
        let contract = RunContractReceiptV1 {
            authority: authority.receipt(20),
            skill_use: rollshot_agent::skills::bundled_action_guide_captions_use()
                .unwrap()
                .receipt(),
            bound_at_unix_ms: 20,
        };
        let bound = running.bind_run_contract(contract, 20).unwrap();
        store
            .transition_audited(&running, &bound, AuditEventId::new_v4(), 20)
            .unwrap();

        // 4. AuthorityDenied — standalone audit event. No promotion because
        // the submit was denied before any artifact could be produced.
        let denied_envelope = rollshot_agent::audit::authority_denied_envelope(
            &authority,
            "submit_caption_suggestions",
            "SubmitReviewCandidate",
            AuditEventId::new_v4(),
            25,
        )
        .unwrap();
        store.append_standalone_audit(denied_envelope).unwrap();

        task_id
    }

    /// Read the raw audit journal file for a task.
    fn read_journal_to_string(config_dir: &std::path::Path, task_id: &ProductTaskId) -> String {
        let path = config_dir
            .join("agent-tasks/audit")
            .join(format!("{}.jsonl", task_id.as_str()));
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("journal file not found at {}: {e}", path.display()))
    }

    // ------------------------------------------------------------------
    // Coverage tests
    // ------------------------------------------------------------------

    #[test]
    fn caption_task_lifecycle_appends_every_material_event() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();
        let task_id = drive_full_caption_lifecycle(&store);

        let kinds: Vec<_> = store
            .committed_audit_events(&task_id)
            .unwrap()
            .into_iter()
            .map(|e| e.event().kind())
            .collect();

        for expected in [
            AuditEventKindV1::TaskCreated,
            AuditEventKindV1::AttemptStarted,
            AuditEventKindV1::RunContractBound,
            AuditEventKindV1::ArtifactPromoted,
            AuditEventKindV1::ReviewApplyStarted,
            AuditEventKindV1::ReviewDecisionCommitted,
        ] {
            assert!(
                kinds.contains(&expected),
                "missing {expected:?} in {kinds:?}"
            );
        }

        // Order is part of the contract: a promotion cannot precede its
        // contract bind, and a review decision cannot precede its apply.
        let position = |k: AuditEventKindV1| kinds.iter().position(|got| *got == k).unwrap();
        assert!(
            position(AuditEventKindV1::TaskCreated) < position(AuditEventKindV1::AttemptStarted)
        );
        assert!(
            position(AuditEventKindV1::RunContractBound)
                < position(AuditEventKindV1::ArtifactPromoted)
        );
        assert!(
            position(AuditEventKindV1::ReviewApplyStarted)
                < position(AuditEventKindV1::ReviewDecisionCommitted)
        );
    }

    #[test]
    fn a_failed_caption_run_appends_task_terminated() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();
        let task_id = drive_caption_lifecycle_to_terminal(
            &store,
            TaskTerminal::BudgetExhausted {
                dimension: "wall_time".to_owned(),
            },
        );

        let kinds: Vec<_> = store
            .committed_audit_events(&task_id)
            .unwrap()
            .into_iter()
            .map(|e| e.event().kind())
            .collect();

        assert!(
            kinds.contains(&AuditEventKindV1::TaskTerminated),
            "{kinds:?}"
        );
        assert!(
            !kinds.contains(&AuditEventKindV1::ArtifactPromoted),
            "a budget-exhausted run must never promote an artifact: {kinds:?}"
        );
    }

    #[test]
    fn an_authority_denied_submit_appends_authority_denied_and_does_not_promote() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();
        let task_id = drive_caption_lifecycle_with_no_grants(&store);

        let kinds: Vec<_> = store
            .committed_audit_events(&task_id)
            .unwrap()
            .into_iter()
            .map(|e| e.event().kind())
            .collect();

        assert!(
            kinds.contains(&AuditEventKindV1::AuthorityDenied),
            "{kinds:?}"
        );
        assert!(
            !kinds.contains(&AuditEventKindV1::ArtifactPromoted),
            "{kinds:?}"
        );
        assert_eq!(
            store.load(&task_id).unwrap().artifact_metadata(),
            None,
            "a denied submit must leave no artifact metadata"
        );
    }

    // ------------------------------------------------------------------
    // Privacy tests
    // ------------------------------------------------------------------

    #[test]
    fn caption_audit_journal_holds_no_caption_or_step_text() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();
        let task_id = drive_full_caption_lifecycle(&store);

        let journal = read_journal_to_string(dir.path(), &task_id);

        for secret in [
            "The settings panel appears.",
            "Open Settings",
            "Suggest concise Action Guide titles",
        ] {
            assert!(
                !journal.contains(secret),
                "audit journal leaked {secret:?}: {journal}"
            );
        }
    }

    #[test]
    fn caption_task_file_holds_no_image_bytes_and_no_skill_body() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::agent_store::open_process_store(dir.path()).unwrap();
        let task_id = drive_full_caption_lifecycle(&store);

        // `ProductTaskId::as_str()` already begins with "task-"
        // (product_task.rs:29), and `task_path` writes
        // `<tasks_dir>/{id}.json` (task_store.rs:398). Re-adding the prefix
        // here would look for `task-task-<uuid>.json` and panic on unwrap.
        let raw = std::fs::read_to_string(
            dir.path()
                .join("agent-tasks/tasks")
                .join(format!("{}.json", task_id.as_str())),
        )
        .unwrap();

        assert!(
            !raw.contains("Suggest concise Action Guide titles"),
            "the skill body must not be persisted; only its digest"
        );
        assert!(
            !raw.contains("base_image_sha256"),
            "a caption binding must not carry image fields"
        );
        assert!(
            !raw.contains("iVBORw0KGgo"),
            "no PNG payload may reach the task store"
        );

        // Positive counterpart, so the three negatives above cannot all pass
        // simply because nothing was written: the digest IS present, and it is
        // the caption package that was bound.
        let snapshot = store.load(&task_id).unwrap();
        let contract = snapshot
            .attempts()
            .last()
            .unwrap()
            .run_contract()
            .expect("a caption task always binds a run contract");
        assert_eq!(contract.skill_use.package_id, "action-guide-captions");
        assert_eq!(contract.skill_use.package_digest.len(), 64);
        assert!(raw.contains(&contract.skill_use.package_digest));
        assert_eq!(
            contract.authority.disclosure_ceiling,
            rollshot_agent::authority::DisclosureCeiling::TextMetadataOnly
        );
        assert_eq!(
            contract.authority.granted_operations,
            vec![rollshot_agent::authority::RunOperation::SubmitReviewCandidate]
        );
    }
}
