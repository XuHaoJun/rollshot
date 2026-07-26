//! Bounded agent driver — owns one complete authoring run lifecycle.
//!
//! ```text
//! AuthorizedModelInput
//!         |
//!         v
//!   AgentSession + DraftState + RunBudget
//!         |
//!         v
//!  Rig AgentRun::next_step()
//!    | CallModel { prompt, history, turn }
//!    |        |
//!    |        v
//!    |   RollshotModel facade
//!    |        |
//!    |        +--> Anthropic Rig provider
//!    |        `--> OpenAI Chat Completions Rig provider
//!    |                 |
//!    |                 v
//!    |       StreamedAssistantContent
//!    |                 |
//!    |                 v
//!    |       StreamedTurnAssembler
//!    |                 |
//!    |                 v
//!    |       AgentRun::streamed_turn()
//!    |
//!    | CallTools { calls } -- serial --> ToolRegistry
//!    |                                  |
//!    |                                  +--> DraftState generation
//!    |                                  +--> validation/proposal/QuickJS
//!    |                                  `--> InspectionProvider
//!    |
//!    ` Done --> ReadyForReview | typed terminal state
//! ```

use std::collections::{BTreeSet, HashSet};

use rig_core::agent::run::StreamedTurnAssembler;
use rig_core::completion::Usage;
use rig_core::message::{AssistantContent, ToolCall as RigToolCall, ToolFunction};
use rig_core::OneOrMany;
// Rig stream types are used only by the crate-internal scripted `run` harness.
#[cfg(test)]
use rig_core::streaming::StreamedAssistantContent;
#[cfg(test)]
use rig_core::test_utils::MockResponse;

use crate::domain::{AgentSession, AuthorizedModelInput, SessionId};
#[cfg(test)]
use crate::model::drive_streamed_turn;
use crate::model::{emit_tool_call_completions, ModelStreamEvent};
use crate::provider::{ProviderAdapter, StreamBounds};
use crate::runtime::{
    BudgetDimension, BudgetError, BudgetTracker, NullEventSink, RunBudget, RunCancellation,
    RunEvent, RunEventSink, UsageSnapshot,
};
use crate::tools::{ToolCall, ToolContext, ToolOutcome, ToolRegistry};

// ---------- Configuration ----------

const SMART_REDACTION_SYSTEM_PROMPT: &str = r#"You are Rollshot Smart Redaction Agent.
Your only job is to create editable redaction candidates for the current screenshot.
Rollshot has already captured the current screenshot for this run. Use the provided screenshot attachment, local context, and available tools; do not ask the user to upload, attach, or take another screenshot.

Interpret user requests like "hide the URL bar", "hide emails", or "redact names" as redaction targets.
For common screenshot regions such as a browser URL/address bar, infer the visible target from the current screenshot instead of asking what device or app environment the user is using.
If the request is not about hiding or redacting visible content, refuse briefly and ask for a redaction target.
If the redaction target is ambiguous after inspecting the available screenshot/context, ask one brief clarifying question about what visible content should be redacted.
Do not provide general advice, product support, or workflow guidance.

Rollshot JavaScript authoring guide:
- Write exactly one synchronous function main(input). Do not use async, imports, exports, timers, eval, Function, DOM, filesystem, network, process APIs, dynamic property access, or loops that can run forever.
- Available input fields use camelCase: input.imageWidth, input.imageHeight, input.region, input.annotations, input.capabilityHandles.
- Return an object shaped like { candidates: [...] }.
- Each candidate must be { kind: "addRedaction", bounds, confidence, label } with optional rationale.
- bounds is { x, y, width, height } in image pixels. width and height must be positive.
- confidence must be between 0 and 1. label must be short and non-empty.
- Supported capability calls are rollshot.ocr(query), rollshot.layout(query) when available, rollshot.regionFeatures(query), and rollshot.templateMatch(query) only when a matching input.capabilityHandles entry exists.
- Use only template handles listed by inspect_image_context capability_handles before calling rollshot.templateMatch. Do not invent template handles when that list is empty.
- Refer to template handles through input.capabilityHandles.<alias>; do not hard-code raw handle strings.
- In OCR-enabled runs, call inspect_ocr for text-driven redaction requests before writing source. inspect_ocr returns full recognized text, bounds, and confidence for canonical regions. Use OCR bounds as evidence for candidate rectangles.
- If OCR is unavailable, treat that as a harness limitation and do not invent text evidence.
- Prefer deterministic regionFeatures strip regions for simple screenshot chrome targets, for example:
  const bounds = { x: 0, y: 0, width: input.imageWidth, height: Math.min(96, input.imageHeight) };
  const features = rollshot.regionFeatures({ region: { kind: "rect", bounds: bounds }, limit: 1 });
- Example empty result: function main(input) { return { candidates: [] }; }
- Example redaction from a strip:
  function main(input) {
    const bounds = { x: 0, y: 0, width: input.imageWidth, height: Math.min(96, input.imageHeight) };
    const features = rollshot.regionFeatures({ region: { kind: "rect", bounds: bounds }, limit: 1 });
    const hasFeatures = features.length > 0;
    return { candidates: hasFeatures ? [{ kind: "addRedaction", bounds: bounds, confidence: 0.6, label: "top-strip" }] : [] };
  }
- Example OCR redaction when OCR is available:
  function expand(rect, padding) {
    return { x: Math.max(0, rect.x - padding), y: Math.max(0, rect.y - padding), width: rect.width + padding * 2, height: rect.height + padding * 2 };
  }
  function main(input) {
    const matches = rollshot.ocr({ region: input.region, limit: 20 });
    return { candidates: matches.map((match) => ({ kind: "addRedaction", bounds: expand(match.bounds, 6), confidence: match.confidence, label: "ocr-match" })) };
  }

Inspection loop:
1. Call inspect_image_context before writing or replacing source.
2. Check capability_handles before writing source that calls rollshot.templateMatch.
3. Call inspect_ocr for text-driven redaction requests such as visible words, names, emails, ids, labels, form fields, or account-like strings.
4. Use inspect_region_features with canonical regions when coarse visual evidence is needed.
5. Valid canonical regions are full, top_strip, left_strip, right_strip, bottom_strip.
6. Do not ask for raw pixels or custom crop inspection; use dry_run to verify source behavior.

Authoring loop:
1. Use read_current_source to inspect the current source, generation, validation summary, and recent evidence before editing.
2. Prefer edit_source with unique exact old/new text for small changes; use replace_source only when a full rewrite is clearer.
3. Use validate_source on the current generation.
4. Use dry_run on the current generation.
5. If validation or dry_run fails, read_current_source, edit_source, and retry validation/dry-run on the new generation.
6. Use submit_for_review only after the current generation has successful validate_source and dry_run evidence.
7. A successful dry_run means "ready for user review", not "safe to export".

Improve runs:
1. The user message may contain reviewed correction evidence from a previous detector run.
2. Treat rejected candidates as false positives to remove or narrow.
3. Treat resized candidates as geometry corrections for the intended target.
4. Treat manually added candidates as missed targets the detector should learn to include.
5. Preserve unrelated useful detections from the current source.
6. Explain what changed in the detector before submit_for_review."#;

const VISUAL_ANNOTATION_SYSTEM_PROMPT: &str = r#"You are Rollshot Visual Annotation Agent.
Your only job is to suggest visual annotations for the single most important UI
element(s) in the screenshot the user is reviewing. Rollshot has already
authorized the screenshot for this run as an image attachment; do not ask the
user to upload, attach, or take another screenshot.

You have exactly one terminal tool: `submit_visual_annotation_suggestions`. The
tool accepts one of two payloads:

  1. A batch of annotation suggestions:
     {
       "suggestions": [
         {
           "kind": "number_callout",
           "id": <unique integer>,
           "tip": { "x": <0.0..=1.0>, "y": <0.0..=1.0> },
           "bubble": { "x": <0.0..=1.0>, "y": <0.0..=1.0> },
           "confidence": <0.0..=1.0>,
           "rationale": <string <= 500 chars, optional>
         },
         {
           "kind": "text_note",
           "id": <unique integer>,
           "position": { "x": <0.0..=1.0>, "y": <0.0..=1.0> },
           "text": <non-empty string <= 500 chars>,
           "confidence": <0.0..=1.0>,
           "rationale": <string <= 500 chars, optional>
         },
         {
           "kind": "opaque_redaction",
           "id": <unique integer>,
           "bounds": { "x": <0.0..=1.0>, "y": <0.0..=1.0>, "width": <0.0..=1.0>, "height": <0.0..=1.0> },
           "confidence": <0.0..=1.0>,
           "rationale": <string <= 500 chars, optional>
         }
       ]
     }
     Coordinates are normalized image-fraction values. The batch may contain
     any combination of the three kinds. Each suggestion must have a unique id.

  2. A no-suggestion report when no annotation is appropriate:
     {
       "result": "no_suggestion",
       "reason": <string <= 500 chars, optional>
     }

Rules you must follow:
- Choose at most a few high-confidence annotations. Rollshot owns bubble
  placement and numbering for callouts.
- Do not output any prose, reasoning, JSON, or commentary outside the
  single `submit_visual_annotation_suggestions` tool call.
- Coordinates and confidence must be finite numbers in 0..=1. Keep
  `rationale` and `reason` at or under 500 characters. Do not include
  URLs, raw bytes, or PII.
- Do not reference, transcribe, or speculate about PII (names, emails,
  account numbers, addresses).
- Only call tools advertised in this run. There is exactly one:
  `submit_visual_annotation_suggestions`. Do not invent tool handles,
  function names, or capability identifiers.
- If the screenshot is too small, too low-contrast, or shows no
  meaningful UI, return `no_suggestion` with a short reason. Do not guess."#;

pub(crate) enum AgentTaskProfile {
    SmartRedaction,
    #[allow(dead_code)]
    VisualAnnotation,
}

impl AgentTaskProfile {
    pub(crate) fn system_prompt(&self) -> &'static str {
        match self {
            Self::SmartRedaction => SMART_REDACTION_SYSTEM_PROMPT,
            Self::VisualAnnotation => VISUAL_ANNOTATION_SYSTEM_PROMPT,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn terminal_tools(&self) -> &'static [&'static str] {
        match self {
            Self::SmartRedaction => &["submit_for_review", "request_user_input"],
            Self::VisualAnnotation => &["submit_visual_annotation_suggestions"],
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub max_turns: usize,
    pub max_assistant_bytes: usize,
    pub max_argument_bytes: usize,
    pub max_result_bytes: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_turns: 10,
            max_assistant_bytes: 4 * 1024 * 1024,
            max_argument_bytes: 256 * 1024,
            max_result_bytes: 256 * 1024,
        }
    }
}

// ---------- Result types ----------

/// Evidence from a successful dry run, stored for the review handoff.
#[derive(Debug, Clone, PartialEq)]
pub struct DryRunEvidence {
    pub candidate_count: u32,
    pub affected_area: f32,
}

/// Complete validated automation draft ready for review.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftAutomation {
    pub source: String,
    pub validated: rollshot_automation::ValidatedAutomation,
    pub validation_summary: rollshot_automation::ValidationSummary,
    pub dry_run: DryRunEvidence,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReadyForReview {
    pub automation: DraftAutomation,
    pub proposal: rollshot_edit_proposal::EditProposal,
    pub budget_usage: UsageSnapshot,
    pub session_id: SessionId,
    pub assistant_text: String,
    pub generation: u64,
    pub usage: UsageSnapshot,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NeedsUserInput {
    pub session_id: SessionId,
    pub generation: u64,
    pub assistant_text: String,
}

// ---------- Terminal state ----------

#[derive(Debug, Clone, PartialEq)]
pub enum RunTerminalState {
    ReadyForReview(Box<ReadyForReview>),
    NeedsUserInput(NeedsUserInput),
    Cancelled,
    BudgetExhausted { dimension: BudgetDimension },
    SourceValidationFailure,
    RuntimeFailure,
    AgentProtocolFailure { message: String },
    ProviderFailure { message: String },
}

// ---------- Errors ----------

#[derive(Debug, Clone, PartialEq)]
pub enum DriverError {
    BudgetExhausted(BudgetDimension),
    Cancelled,
    ProviderFailure(String),
    AgentProtocolFailure(String),
}

impl From<BudgetError> for DriverError {
    fn from(e: BudgetError) -> Self {
        match e {
            BudgetError::Exceeded(dim) => DriverError::BudgetExhausted(dim),
            BudgetError::Overflow => DriverError::AgentProtocolFailure("budget overflow".into()),
        }
    }
}

async fn await_provider_progress<F, T>(
    cancellation: &RunCancellation,
    deadline: tokio::time::Instant,
    future: F,
) -> Result<T, DriverError>
where
    F: std::future::Future<Output = T>,
{
    if cancellation.is_cancelled() {
        return Err(DriverError::Cancelled);
    }
    if tokio::time::Instant::now() >= deadline {
        return Err(DriverError::BudgetExhausted(BudgetDimension::WallTime));
    }
    tokio::pin!(future);
    tokio::select! {
        biased;
        _ = cancellation.wait() => Err(DriverError::Cancelled),
        _ = tokio::time::sleep_until(deadline) => {
            Err(DriverError::BudgetExhausted(BudgetDimension::WallTime))
        }
        output = &mut future => Ok(output),
    }
}

// ---------- Tool failure tracking ----------

#[derive(Debug, Clone, Copy)]
enum ToolFailureKind {
    SourceValidation,
    Runtime,
}

// ---------- Terminal construction ----------

/// Build the `ReadyForReview` terminal from the handoff a successful
/// `submit_for_review` left in the tool context. A missing handoff means the
/// evidence was incomplete — never fabricate one (§4.5); fail as `RuntimeFailure`.
fn finalize_ready_for_review(
    tool_ctx: &ToolContext,
    usage: &UsageSnapshot,
    assistant_text: &str,
) -> RunTerminalState {
    match tool_ctx.pending_ready_for_review.lock().unwrap().take() {
        Some(mut ready) => {
            ready.usage = usage.clone();
            ready.budget_usage = usage.clone();
            ready.assistant_text = assistant_text.to_string();
            ready.session_id = tool_ctx.session_id;
            ready.generation = tool_ctx.draft.lock().unwrap().generation();
            RunTerminalState::ReadyForReview(Box::new(ready))
        }
        None => {
            tracing::debug!(
                target: "rollshot::agent::driver",
                session_id = tool_ctx.session_id.get(),
                "submit reported success without a complete review handoff"
            );
            RunTerminalState::RuntimeFailure
        }
    }
}

// ---------- Runner ----------

pub struct AgentRunner {
    pub config: AgentConfig,
}

impl AgentRunner {
    pub fn new(config: AgentConfig) -> Self {
        Self { config }
    }

    /// Run one complete authoring lifecycle.
    ///
    /// `model_turn_fn` is called with the turn index (1-based) and must return
    /// `Some(items)` for that turn, or `None` to end the model's contribution.
    ///
    /// This scripted entry point is crate-internal: its `model_turn_fn` uses Rig
    /// stream types (`StreamedAssistantContent`/`MockResponse`), which must not
    /// appear in the public API (§2.1, success criterion #10). Production callers
    /// use [`AgentRunner::run_with_provider`]. It drives the acceptance/unit
    /// tests for the bounded author loop.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn run(
        &self,
        input: AuthorizedModelInput,
        session: &mut AgentSession,
        tool_registry: &ToolRegistry,
        budget: RunBudget,
        cancellation: &RunCancellation,
        event_sink: &dyn RunEventSink,
        tool_ctx: &ToolContext,
        mut model_turn_fn: impl FnMut(usize) -> Option<Vec<StreamedAssistantContent<MockResponse>>>,
    ) -> RunTerminalState {
        session.push_user(input.user_message.clone());

        let mut rig_run = rig_core::agent::run::AgentRun::new(rig_core::message::Message::user(
            &input.user_message,
        ))
        .max_turns(self.config.max_turns);

        let start = tokio::time::Instant::now();
        let mut tracker = BudgetTracker::new(budget, start);
        let mut total_assistant_bytes: usize = 0;
        let mut last_assistant_text = String::new();
        let mut last_failure_kind: Option<ToolFailureKind> = None;

        let tool_names: BTreeSet<String> = tool_registry
            .tool_names()
            .into_iter()
            .map(String::from)
            .collect();

        tracing::debug!(
            target: "rollshot::agent::driver",
            session_id = tool_ctx.session_id.get(),
            provider = %input.manifest.provider,
            model = %input.manifest.model,
            "run started"
        );

        loop {
            if cancellation.is_cancelled() {
                tracing::debug!(
                    target: "rollshot::agent::driver",
                    session_id = tool_ctx.session_id.get(),
                    "run cancelled"
                );
                return RunTerminalState::Cancelled;
            }

            if let Err(BudgetError::Exceeded(dim)) =
                tracker.check_wall_time(tokio::time::Instant::now())
            {
                tracing::debug!(
                    target: "rollshot::agent::driver",
                    session_id = tool_ctx.session_id.get(),
                    dimension = ?dim,
                    "run budget exhausted"
                );
                return RunTerminalState::BudgetExhausted { dimension: dim };
            }

            let step = match rig_run.next_step() {
                Ok(s) => s,
                Err(e) => {
                    return RunTerminalState::AgentProtocolFailure {
                        message: e.to_string(),
                    };
                }
            };

            match step {
                rig_core::agent::run::AgentRunStep::CallModel { .. } => {
                    match self.run_model_turn(
                        &mut rig_run,
                        &tool_names,
                        &mut model_turn_fn,
                        event_sink,
                        &mut tracker,
                        &mut total_assistant_bytes,
                        self.config.max_assistant_bytes,
                        cancellation,
                        &mut last_assistant_text,
                    ) {
                        Ok(()) => {}
                        Err(DriverError::BudgetExhausted(dim)) => {
                            return RunTerminalState::BudgetExhausted { dimension: dim };
                        }
                        Err(DriverError::Cancelled) => return RunTerminalState::Cancelled,
                        Err(DriverError::ProviderFailure(msg)) => {
                            return RunTerminalState::ProviderFailure { message: msg };
                        }
                        Err(DriverError::AgentProtocolFailure(msg)) => {
                            return RunTerminalState::AgentProtocolFailure { message: msg };
                        }
                    }
                }
                rig_core::agent::run::AgentRunStep::CallTools { calls } => {
                    match self
                        .run_tool_turn(
                            &mut rig_run,
                            &calls,
                            tool_registry,
                            event_sink,
                            &mut tracker,
                            cancellation,
                            tool_ctx,
                            &last_assistant_text,
                            &mut last_failure_kind,
                        )
                        .await
                    {
                        Ok(terminal) => {
                            if let Some(state) = terminal {
                                // The model turn completed before this terminal;
                                // commit its prose as the session's assistant msg.
                                let _ = session.push_assistant(last_assistant_text.clone());
                                return state;
                            }
                        }
                        Err(DriverError::BudgetExhausted(dim)) => {
                            return RunTerminalState::BudgetExhausted { dimension: dim };
                        }
                        Err(DriverError::Cancelled) => return RunTerminalState::Cancelled,
                        Err(DriverError::ProviderFailure(msg)) => {
                            return RunTerminalState::ProviderFailure { message: msg };
                        }
                        Err(DriverError::AgentProtocolFailure(msg)) => {
                            return RunTerminalState::AgentProtocolFailure { message: msg };
                        }
                    }
                }
                rig_core::agent::run::AgentRunStep::Done(_) => {
                    tracker.apply_turn();

                    if let Err(BudgetError::Exceeded(dim)) = tracker.check_accumulated() {
                        return RunTerminalState::BudgetExhausted { dimension: dim };
                    }

                    let _ = session.push_assistant(last_assistant_text.clone());

                    if let Some(failure) = last_failure_kind {
                        return match failure {
                            ToolFailureKind::SourceValidation => {
                                RunTerminalState::SourceValidationFailure
                            }
                            ToolFailureKind::Runtime => RunTerminalState::RuntimeFailure,
                        };
                    }

                    // A successful submit_for_review terminates the run in
                    // run_tool_turn (§4.5), so reaching Done means the model never
                    // submitted.
                    return RunTerminalState::AgentProtocolFailure {
                        message: "model completed without submission".into(),
                    };
                }
            }

            tracker.apply_turn();
        }
    }

    /// Run one complete authoring lifecycle using a real provider adapter.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_with_provider(
        &self,
        input: AuthorizedModelInput,
        session: &mut AgentSession,
        tool_registry: &ToolRegistry,
        budget: RunBudget,
        cancellation: &RunCancellation,
        event_sink: &dyn RunEventSink,
        tool_ctx: &ToolContext,
        provider: &dyn ProviderAdapter,
    ) -> RunTerminalState {
        session.push_user(input.user_message.clone());

        let mut rig_run = rig_core::agent::run::AgentRun::new(rig_core::message::Message::user(
            &input.user_message,
        ))
        .max_turns(self.config.max_turns);

        let start = tokio::time::Instant::now();
        let mut tracker = BudgetTracker::new(budget, start);
        let mut total_assistant_bytes: usize = 0;
        let mut last_assistant_text = String::new();
        let mut last_failure_kind: Option<ToolFailureKind> = None;

        let tool_names: BTreeSet<String> = tool_registry
            .tool_names()
            .into_iter()
            .map(String::from)
            .collect();
        let tool_definitions = tool_registry.tool_definitions();

        tracing::debug!(
            target: "rollshot::agent::driver",
            session_id = tool_ctx.session_id.get(),
            provider = %input.manifest.provider,
            model = %input.manifest.model,
            "run_with_provider started"
        );

        loop {
            if cancellation.is_cancelled() {
                return RunTerminalState::Cancelled;
            }

            if let Err(BudgetError::Exceeded(dim)) =
                tracker.check_wall_time(tokio::time::Instant::now())
            {
                return RunTerminalState::BudgetExhausted { dimension: dim };
            }

            let step = match rig_run.next_step() {
                Ok(s) => s,
                Err(e) => {
                    return RunTerminalState::AgentProtocolFailure {
                        message: e.to_string(),
                    };
                }
            };

            match step {
                rig_core::agent::run::AgentRunStep::CallModel {
                    prompt, history, ..
                } => {
                    match self
                        .run_model_turn_with_provider(
                            &mut rig_run,
                            &tool_names,
                            &tool_definitions,
                            prompt,
                            history,
                            provider,
                            event_sink,
                            &mut tracker,
                            &mut total_assistant_bytes,
                            self.config.max_assistant_bytes,
                            cancellation,
                            &input,
                            &mut last_assistant_text,
                        )
                        .await
                    {
                        Ok(()) => {}
                        Err(DriverError::BudgetExhausted(dim)) => {
                            return RunTerminalState::BudgetExhausted { dimension: dim };
                        }
                        Err(DriverError::Cancelled) => return RunTerminalState::Cancelled,
                        Err(DriverError::ProviderFailure(msg)) => {
                            return RunTerminalState::ProviderFailure { message: msg };
                        }
                        Err(DriverError::AgentProtocolFailure(msg)) => {
                            return RunTerminalState::AgentProtocolFailure { message: msg };
                        }
                    }
                }
                rig_core::agent::run::AgentRunStep::CallTools { calls } => {
                    match self
                        .run_tool_turn(
                            &mut rig_run,
                            &calls,
                            tool_registry,
                            event_sink,
                            &mut tracker,
                            cancellation,
                            tool_ctx,
                            &last_assistant_text,
                            &mut last_failure_kind,
                        )
                        .await
                    {
                        Ok(terminal) => {
                            if let Some(state) = terminal {
                                // The model turn completed before this terminal;
                                // commit its prose as the session's assistant msg.
                                let _ = session.push_assistant(last_assistant_text.clone());
                                return state;
                            }
                        }
                        Err(DriverError::BudgetExhausted(dim)) => {
                            return RunTerminalState::BudgetExhausted { dimension: dim };
                        }
                        Err(DriverError::Cancelled) => return RunTerminalState::Cancelled,
                        Err(DriverError::ProviderFailure(msg)) => {
                            return RunTerminalState::ProviderFailure { message: msg };
                        }
                        Err(DriverError::AgentProtocolFailure(msg)) => {
                            return RunTerminalState::AgentProtocolFailure { message: msg };
                        }
                    }
                }
                rig_core::agent::run::AgentRunStep::Done(_) => {
                    tracker.apply_turn();

                    if let Err(BudgetError::Exceeded(dim)) = tracker.check_accumulated() {
                        return RunTerminalState::BudgetExhausted { dimension: dim };
                    }

                    let _ = session.push_assistant(last_assistant_text.clone());

                    if let Some(failure) = last_failure_kind {
                        return match failure {
                            ToolFailureKind::SourceValidation => {
                                RunTerminalState::SourceValidationFailure
                            }
                            ToolFailureKind::Runtime => RunTerminalState::RuntimeFailure,
                        };
                    }

                    // A successful submit_for_review terminates the run in
                    // run_tool_turn (§4.5), so reaching Done means the model never
                    // submitted.
                    return RunTerminalState::AgentProtocolFailure {
                        message: "model completed without submission".into(),
                    };
                }
            }

            tracker.apply_turn();
        }
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    fn run_model_turn(
        &self,
        rig_run: &mut rig_core::agent::run::AgentRun,
        tool_names: &BTreeSet<String>,
        model_turn_fn: &mut impl FnMut(usize) -> Option<Vec<StreamedAssistantContent<MockResponse>>>,
        event_sink: &dyn RunEventSink,
        tracker: &mut BudgetTracker,
        total_assistant_bytes: &mut usize,
        max_assistant_bytes: usize,
        cancellation: &RunCancellation,
        last_assistant_text: &mut String,
    ) -> Result<(), DriverError> {
        if cancellation.is_cancelled() {
            return Err(DriverError::Cancelled);
        }

        tracker.check_wall_time(tokio::time::Instant::now())?;

        let turn_index = rig_run.turn();
        let items = match model_turn_fn(turn_index) {
            Some(items) => items,
            None => {
                return Err(DriverError::ProviderFailure(
                    "model returned no items for turn".into(),
                ));
            }
        };

        let mut asm = StreamedTurnAssembler::new(tool_names.clone(), tool_names.clone());
        let mut turn_input_tokens: u64 = 0;
        let mut turn_output_tokens: u64 = 0;
        // The model's streamed prose for this turn (committed to the session and
        // surfaced in terminal reports — distinct from the automation draft).
        let mut turn_text = String::new();

        // Track tool calls: id → (name, accumulated_arg_deltas)
        let mut tool_call_ids: Vec<String> = Vec::new();
        let mut tool_call_names: Vec<String> = Vec::new();
        let mut tool_call_arg_deltas: Vec<String> = Vec::new();
        let mut total_argument_bytes: usize = 0;
        let mut tool_calls_with_deltas: HashSet<String> = HashSet::new();

        for item in &items {
            if cancellation.is_cancelled() {
                tracing::debug!(
                    target: "rollshot::agent::driver",
                    turn = turn_index,
                    "model turn cancelled between items"
                );
                return Err(DriverError::Cancelled);
            }

            if let Err(BudgetError::Exceeded(dim)) =
                tracker.check_wall_time(tokio::time::Instant::now())
            {
                tracing::debug!(
                    target: "rollshot::agent::driver",
                    turn = turn_index,
                    dimension = ?dim,
                    "model turn deadline exceeded between items"
                );
                return Err(DriverError::BudgetExhausted(dim));
            }

            let events = drive_streamed_turn(&mut asm, item)
                .map_err(|e| DriverError::ProviderFailure(e.to_string()))?;

            for event in events {
                match event {
                    ModelStreamEvent::TextDelta(text) => {
                        *total_assistant_bytes += text.len();
                        if *total_assistant_bytes > max_assistant_bytes {
                            tracing::debug!(
                                target: "rollshot::agent::driver",
                                turn = turn_index,
                                limit = max_assistant_bytes,
                                "assistant bytes limit exceeded"
                            );
                            return Err(DriverError::BudgetExhausted(BudgetDimension::SourceBytes));
                        }
                        turn_text.push_str(&text);
                        event_sink.emit(RunEvent::TextChunk { text });
                    }
                    ModelStreamEvent::ToolCallStart { id, name } => {
                        tool_call_ids.push(id);
                        tool_call_names.push(name);
                        tool_call_arg_deltas.push(String::new());
                    }
                    ModelStreamEvent::ToolCallArgumentDelta { id, delta } => {
                        tool_calls_with_deltas.insert(id.clone());
                        total_argument_bytes += delta.len();
                        if total_argument_bytes > self.config.max_argument_bytes {
                            tracing::debug!(
                                target: "rollshot::agent::driver",
                                turn = turn_index,
                                limit = self.config.max_argument_bytes,
                                "argument bytes limit exceeded"
                            );
                            return Err(DriverError::BudgetExhausted(
                                BudgetDimension::ArgumentBytes,
                            ));
                        }
                        if let Some(pos) = tool_call_ids.iter().position(|tc_id| *tc_id == id) {
                            tool_call_arg_deltas[pos].push_str(&delta);
                        }
                    }
                    ModelStreamEvent::ToolCallComplete {
                        id,
                        name,
                        arguments,
                    } => {
                        // Complete tool call from assembler — use directly
                        let serialized = serde_json::to_string(&arguments).unwrap_or_default();
                        // Only count argument bytes if deltas weren't already received
                        // for this tool call, to avoid double-counting.
                        if !tool_calls_with_deltas.contains(&id) {
                            total_argument_bytes += serialized.len();
                            if total_argument_bytes > self.config.max_argument_bytes {
                                tracing::debug!(
                                    target: "rollshot::agent::driver",
                                    turn = turn_index,
                                    limit = self.config.max_argument_bytes,
                                    "argument bytes limit exceeded"
                                );
                                return Err(DriverError::BudgetExhausted(
                                    BudgetDimension::ArgumentBytes,
                                ));
                            }
                        }
                        if let Some(pos) = tool_call_ids.iter().position(|tc_id| *tc_id == id) {
                            tool_call_names[pos] = name;
                            tool_call_arg_deltas[pos] = serialized;
                        } else {
                            tool_call_ids.push(id);
                            tool_call_names.push(name);
                            tool_call_arg_deltas.push(serialized);
                        }
                    }
                    ModelStreamEvent::UsageDelta(u) => {
                        turn_input_tokens = u.input_tokens;
                        turn_output_tokens = u.output_tokens;
                    }
                    ModelStreamEvent::Completed(c) => {
                        turn_input_tokens = c.usage.input_tokens;
                        turn_output_tokens = c.usage.output_tokens;
                    }
                    ModelStreamEvent::Error(e) => {
                        return Err(DriverError::ProviderFailure(e.to_string()));
                    }
                }
            }
        }

        // Commit this turn's prose as the run's latest assistant message.
        *last_assistant_text = turn_text;

        // Build final choice from tracked tool calls. Non-empty arguments that
        // are not valid JSON are an unrecoverable protocol failure (§6.2); empty
        // arguments are treated as an empty object (no-argument tools).
        let mut final_items: Vec<AssistantContent> = Vec::new();
        for i in 0..tool_call_ids.len() {
            let raw = &tool_call_arg_deltas[i];
            let args: serde_json::Value = if raw.trim().is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(raw).map_err(|e| {
                    DriverError::AgentProtocolFailure(format!(
                        "tool call arguments are not valid JSON: {e}"
                    ))
                })?
            };
            final_items.push(AssistantContent::ToolCall(RigToolCall::new(
                tool_call_ids[i].clone(),
                ToolFunction::new(tool_call_names[i].clone(), args),
            )));
        }
        if final_items.is_empty() {
            final_items.push(AssistantContent::text(""));
        }
        let final_choice = OneOrMany::many(final_items)
            .unwrap_or_else(|_| OneOrMany::one(AssistantContent::text("")));

        // Finish the assembler and advance the state machine
        let stream_turn = asm.finish(None, &final_choice);

        // Emit completion events for fully assembled tool calls
        let completions = emit_tool_call_completions(&stream_turn);
        for event in completions {
            if let ModelStreamEvent::ToolCallComplete { .. } = event {
                // Tool calls are already in assembled_tool_calls; events are informational
            }
        }

        // Charge usage
        let turn_usage = UsageSnapshot {
            model_calls: 1,
            input_tokens: turn_input_tokens,
            output_tokens: turn_output_tokens,
            ..Default::default()
        };
        tracker.charge(turn_usage)?;

        // Record usage and advance the state machine
        let usage = Usage {
            input_tokens: turn_input_tokens,
            output_tokens: turn_output_tokens,
            total_tokens: turn_input_tokens + turn_output_tokens,
            ..Usage::new()
        };
        rig_run
            .record_streamed_completion_call(usage)
            .map_err(|e| DriverError::AgentProtocolFailure(e.to_string()))?;

        rig_run
            .streamed_turn(stream_turn)
            .map_err(|e| DriverError::AgentProtocolFailure(e.to_string()))?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_model_turn_with_provider(
        &self,
        rig_run: &mut rig_core::agent::run::AgentRun,
        tool_names: &BTreeSet<String>,
        tool_definitions: &[crate::model::ToolDefinition],
        prompt: rig_core::completion::Message,
        history: Vec<rig_core::completion::Message>,
        provider: &dyn ProviderAdapter,
        event_sink: &dyn RunEventSink,
        tracker: &mut BudgetTracker,
        total_assistant_bytes: &mut usize,
        max_assistant_bytes: usize,
        cancellation: &RunCancellation,
        input: &AuthorizedModelInput,
        last_assistant_text: &mut String,
    ) -> Result<(), DriverError> {
        let turn_index = rig_run.turn();

        // Faithfully reconstruct the conversation Rig has accumulated (prior
        // user/assistant turns, the assistant's tool calls, and the latest tool
        // result) so the model sees its own tool results across turns. The whole
        // conversation is carried in `history`; `prompt` is left empty.
        let mut history_msgs: Vec<crate::model::ModelMessage> = Vec::new();
        for m in &history {
            crate::model::push_model_messages(m, &mut history_msgs);
        }
        crate::model::push_model_messages(&prompt, &mut history_msgs);

        let request = crate::model::ModelRequest {
            model: input.manifest.model.clone(),
            prompt: String::new(),
            history: history_msgs,
            turn: turn_index,
            tool_definitions: tool_definitions.to_vec(),
            system_prompt: Some(AgentTaskProfile::SmartRedaction.system_prompt().to_string()),
            max_tokens: None,
            attachments: vec![],
        };

        self.drive_streamed_turn(
            rig_run,
            tool_names,
            provider,
            event_sink,
            tracker,
            total_assistant_bytes,
            max_assistant_bytes,
            cancellation,
            request,
            last_assistant_text,
        )
        .await
    }

    /// Stream one provider turn into the rig state machine. Shared by the
    /// Smart Redaction driver (`run_with_provider`) and the visual annotation
    /// runner so both paths reuse the same budget charging, cancellation,
    /// and Rig tool-result threading.
    #[allow(clippy::too_many_arguments)]
    async fn drive_streamed_turn(
        &self,
        rig_run: &mut rig_core::agent::run::AgentRun,
        tool_names: &BTreeSet<String>,
        provider: &dyn ProviderAdapter,
        event_sink: &dyn RunEventSink,
        tracker: &mut BudgetTracker,
        total_assistant_bytes: &mut usize,
        max_assistant_bytes: usize,
        cancellation: &RunCancellation,
        request: crate::model::ModelRequest,
        last_assistant_text: &mut String,
    ) -> Result<(), DriverError> {
        if cancellation.is_cancelled() {
            return Err(DriverError::Cancelled);
        }

        tracker.check_wall_time(tokio::time::Instant::now())?;

        // Cap the per-stream deadline so an unbounded wall-time budget
        // (Duration::MAX) cannot overflow the instant arithmetic. The tracker
        // still enforces the real wall-time budget between stream items.
        let now = tokio::time::Instant::now();
        let remaining = tracker
            .remaining_wall_time(now)
            .min(std::time::Duration::from_secs(3600));
        let deadline = now + remaining;
        let bounds = StreamBounds::new(cancellation.clone(), deadline);

        let mut stream =
            await_provider_progress(cancellation, deadline, provider.stream(request, bounds))
                .await?
                .map_err(|error| DriverError::ProviderFailure(error.to_string()))?;

        let asm = StreamedTurnAssembler::new(tool_names.clone(), tool_names.clone());
        // The model's streamed prose for this turn.
        let mut turn_text = String::new();

        let mut tool_call_ids: Vec<String> = Vec::new();
        let mut tool_call_names: Vec<String> = Vec::new();
        let mut tool_call_arg_deltas: Vec<String> = Vec::new();
        let mut total_argument_bytes: usize = 0;
        let mut tool_calls_with_deltas: HashSet<String> = HashSet::new();

        use futures_util::StreamExt;
        let completion_usage = loop {
            let next = await_provider_progress(cancellation, deadline, stream.next()).await?;
            let Some(event_result) = next else {
                // Bare EOF without a proven Completed event is an error.
                return Err(DriverError::ProviderFailure(
                    "provider stream ended before completion".to_string(),
                ));
            };

            if let Err(BudgetError::Exceeded(dim)) =
                tracker.check_wall_time(tokio::time::Instant::now())
            {
                return Err(DriverError::BudgetExhausted(dim));
            }

            let event = event_result.map_err(|e| DriverError::ProviderFailure(e.to_string()))?;

            match event {
                ModelStreamEvent::TextDelta(text) => {
                    *total_assistant_bytes += text.len();
                    if *total_assistant_bytes > max_assistant_bytes {
                        return Err(DriverError::BudgetExhausted(BudgetDimension::SourceBytes));
                    }
                    turn_text.push_str(&text);
                    event_sink.emit(RunEvent::TextChunk { text });
                }
                ModelStreamEvent::ToolCallStart { id, name } => {
                    tool_call_ids.push(id);
                    tool_call_names.push(name);
                    tool_call_arg_deltas.push(String::new());
                }
                ModelStreamEvent::ToolCallArgumentDelta { id, delta } => {
                    tool_calls_with_deltas.insert(id.clone());
                    total_argument_bytes += delta.len();
                    if total_argument_bytes > self.config.max_argument_bytes {
                        return Err(DriverError::BudgetExhausted(BudgetDimension::ArgumentBytes));
                    }
                    if let Some(pos) = tool_call_ids.iter().position(|tc_id| *tc_id == id) {
                        tool_call_arg_deltas[pos].push_str(&delta);
                    }
                }
                ModelStreamEvent::ToolCallComplete {
                    id,
                    name,
                    arguments,
                } => {
                    let serialized = serde_json::to_string(&arguments).unwrap_or_default();
                    if !tool_calls_with_deltas.contains(&id) {
                        total_argument_bytes += serialized.len();
                        if total_argument_bytes > self.config.max_argument_bytes {
                            return Err(DriverError::BudgetExhausted(
                                BudgetDimension::ArgumentBytes,
                            ));
                        }
                    }
                    if let Some(pos) = tool_call_ids.iter().position(|tc_id| *tc_id == id) {
                        tool_call_names[pos] = name;
                        tool_call_arg_deltas[pos] = serialized;
                    } else {
                        tool_call_ids.push(id);
                        tool_call_names.push(name);
                        tool_call_arg_deltas.push(serialized);
                    }
                }
                ModelStreamEvent::UsageDelta(_) => {
                    // Final usage is authoritative from Completed; skip deltas.
                }
                ModelStreamEvent::Completed(c) => {
                    break c.usage;
                }
                ModelStreamEvent::Error(e) => {
                    return Err(DriverError::ProviderFailure(e.to_string()));
                }
            }
        };

        // Commit this turn's prose as the run's latest assistant message.
        *last_assistant_text = turn_text;

        // Build final choice from tracked tool calls. Non-empty arguments that
        // are not valid JSON are an unrecoverable protocol failure (§6.2); empty
        // arguments are treated as an empty object (no-argument tools).
        let mut final_items: Vec<AssistantContent> = Vec::new();
        for i in 0..tool_call_ids.len() {
            let raw = &tool_call_arg_deltas[i];
            let args: serde_json::Value = if raw.trim().is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(raw).map_err(|e| {
                    DriverError::AgentProtocolFailure(format!(
                        "tool call arguments are not valid JSON: {e}"
                    ))
                })?
            };
            final_items.push(AssistantContent::ToolCall(RigToolCall::new(
                tool_call_ids[i].clone(),
                ToolFunction::new(tool_call_names[i].clone(), args),
            )));
        }
        if final_items.is_empty() {
            final_items.push(AssistantContent::text(""));
        }
        let final_choice = OneOrMany::many(final_items)
            .unwrap_or_else(|_| OneOrMany::one(AssistantContent::text("")));

        let stream_turn = asm.finish(None, &final_choice);

        let completions = emit_tool_call_completions(&stream_turn);
        for event in completions {
            if let ModelStreamEvent::ToolCallComplete { .. } = event {
                // informational
            }
        }

        let turn_usage = UsageSnapshot {
            model_calls: 1,
            input_tokens: completion_usage.input_tokens,
            output_tokens: completion_usage.output_tokens,
            ..Default::default()
        };
        tracker.charge(turn_usage)?;

        let usage = Usage {
            input_tokens: completion_usage.input_tokens,
            output_tokens: completion_usage.output_tokens,
            total_tokens: completion_usage.total_tokens,
            ..Usage::new()
        };
        rig_run
            .record_streamed_completion_call(usage)
            .map_err(|e| DriverError::AgentProtocolFailure(e.to_string()))?;

        rig_run
            .streamed_turn(stream_turn)
            .map_err(|e| DriverError::AgentProtocolFailure(e.to_string()))?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_tool_turn(
        &self,
        rig_run: &mut rig_core::agent::run::AgentRun,
        pending_calls: &[rig_core::agent::run::PendingToolCall],
        tool_registry: &ToolRegistry,
        event_sink: &dyn RunEventSink,
        tracker: &mut BudgetTracker,
        cancellation: &RunCancellation,
        tool_ctx: &ToolContext,
        assistant_text: &str,
        last_failure_kind: &mut Option<ToolFailureKind>,
    ) -> Result<Option<RunTerminalState>, DriverError> {
        if cancellation.is_cancelled() {
            return Err(DriverError::Cancelled);
        }

        tracker.check_wall_time(tokio::time::Instant::now())?;

        let tool_calls: Vec<ToolCall> = pending_calls
            .iter()
            .map(|pc| ToolCall {
                name: pc.tool_call.function.name.clone(),
                arguments_json: pc.tool_call.function.arguments.clone(),
            })
            .collect();

        for tc in &tool_calls {
            event_sink.emit(RunEvent::ToolCallStart {
                name: tc.name.clone(),
            });
        }

        let tool_usage = UsageSnapshot {
            tool_calls: tool_calls.len() as u32,
            validation_attempts: tool_calls
                .iter()
                .filter(|tc| tc.name == "validate_source")
                .count() as u32,
            dry_run_attempts: tool_calls.iter().filter(|tc| tc.name == "dry_run").count() as u32,
            ..Default::default()
        };
        tracker.charge(tool_usage)?;

        // A successful terminal tool stops the rest of the batch (§8.3).
        let terminal_tools: BTreeSet<String> = ["submit_for_review", "request_user_input"]
            .into_iter()
            .map(String::from)
            .collect();
        let results = tool_registry
            .execute_calls(&tool_calls, cancellation, &terminal_tools)
            .await;

        let mut rig_results = Vec::new();
        let mut terminal_error: Option<String> = None;
        let mut total_result_bytes: usize = 0;
        // Usage that only becomes known after a tool runs (dry-run candidate
        // count, affected area, and capability calls). Charged after the batch.
        let mut post_usage = UsageSnapshot::default();
        let mut submit_succeeded = false;
        let mut user_input_requested = false;

        for (i, result) in results.into_iter().enumerate() {
            let call_id = pending_calls[i].tool_call.id.clone();
            let tool_name = &pending_calls[i].tool_call.function.name;
            match result {
                Ok(ToolOutcome::Success { result_json }) => {
                    let result_str = serde_json::to_string(&result_json).unwrap_or_default();
                    total_result_bytes += result_str.len();
                    if total_result_bytes > self.config.max_result_bytes {
                        tracing::debug!(
                            target: "rollshot::agent::driver",
                            tool = tool_name.as_str(),
                            limit = self.config.max_result_bytes,
                            "result bytes limit exceeded"
                        );
                        return Err(DriverError::BudgetExhausted(BudgetDimension::ResultBytes));
                    }
                    if tool_name == "submit_for_review"
                        && result_json.get("submitted").and_then(|v| v.as_bool()) == Some(true)
                    {
                        submit_succeeded = true;
                    }
                    if tool_name == "request_user_input" {
                        user_input_requested = true;
                    }
                    if matches!(tool_name.as_str(), "replace_source" | "edit_source") {
                        if let Some(diff) = result_json
                            .get("diff")
                            .cloned()
                            .and_then(|value| serde_json::from_value(value).ok())
                        {
                            event_sink.emit(RunEvent::SourceChanged {
                                tool: tool_name.clone(),
                                diff,
                            });
                        }
                    }
                    if tool_name == "dry_run" {
                        if let Some(cc) =
                            result_json.get("candidate_count").and_then(|v| v.as_u64())
                        {
                            post_usage.candidate_count =
                                post_usage.candidate_count.saturating_add(cc as u32);
                        }
                        if let Some(area) =
                            result_json.get("affected_area").and_then(|v| v.as_f64())
                        {
                            post_usage.affected_area = post_usage
                                .affected_area
                                .saturating_add(area.max(0.0).ceil() as u64);
                        }
                        if let Some(caps) =
                            result_json.get("capability_calls").and_then(|v| v.as_u64())
                        {
                            post_usage.capability_calls =
                                post_usage.capability_calls.saturating_add(caps as u32);
                        }
                    }
                    rig_results.push(rig_core::message::UserContent::tool_result(
                        call_id,
                        rig_core::message::ToolResultContent::from_tool_output(result_str),
                    ));
                }
                Ok(ToolOutcome::Recoverable { error }) => {
                    if tool_name == "validate_source" {
                        *last_failure_kind = Some(ToolFailureKind::SourceValidation);
                    } else if tool_name == "dry_run" {
                        *last_failure_kind = Some(ToolFailureKind::Runtime);
                    }
                    rig_results.push(rig_core::message::UserContent::tool_result(
                        call_id,
                        rig_core::message::ToolResultContent::from_tool_output(error),
                    ));
                }
                Err(e) => {
                    tracing::debug!(
                        target: "rollshot::agent::driver",
                        tool = tool_name.as_str(),
                        error = %e,
                        "tool call failed"
                    );
                    event_sink.emit(RunEvent::ToolCallEnd {
                        name: tool_name.clone(),
                        success: false,
                    });
                    terminal_error = Some(e.to_string());
                    break;
                }
            }
        }

        for tc in &tool_calls {
            event_sink.emit(RunEvent::ToolCallEnd {
                name: tc.name.clone(),
                success: terminal_error.is_none(),
            });
        }

        if let Some(msg) = terminal_error {
            return Err(DriverError::AgentProtocolFailure(msg));
        }

        // Charge usage that only became known once the tools ran (dry-run
        // candidate count, affected area, capability calls).
        tracker.charge(post_usage)?;

        // A successful terminal tool ends the run now; no further model work
        // runs (§4.5). The first terminal tool in response order wins because
        // execute_calls already stopped the batch after it.
        if user_input_requested {
            tracker.apply_turn();
            return Ok(Some(RunTerminalState::NeedsUserInput(NeedsUserInput {
                session_id: tool_ctx.session_id,
                generation: tool_ctx.draft.lock().unwrap().generation(),
                assistant_text: assistant_text.to_string(),
            })));
        }
        if submit_succeeded {
            tracker.apply_turn();
            if let Err(BudgetError::Exceeded(dim)) = tracker.check_accumulated() {
                return Ok(Some(RunTerminalState::BudgetExhausted { dimension: dim }));
            }
            return Ok(Some(finalize_ready_for_review(
                tool_ctx,
                tracker.used(),
                assistant_text,
            )));
        }

        rig_run
            .tool_results(rig_results)
            .map_err(|e| DriverError::AgentProtocolFailure(e.to_string()))?;

        Ok(None)
    }

    /// Bounded visual annotation runner. Returns a
    /// `VisualAnnotationRunTerminal` that never carries provider payload,
    /// prompt text, attachment bytes, or image coordinates. Reuses
    /// `drive_streamed_turn` and the rig state machine for streamed-turn
    /// assembly, budget charging, cancellation, and tool-result threading.
    #[allow(clippy::too_many_lines)]
    pub async fn run_visual_annotation_with_provider(
        &self,
        mut input: crate::domain::AuthorizedModelInput,
        provider: &dyn ProviderAdapter,
        budget: RunBudget,
        cancellation: &RunCancellation,
    ) -> crate::visual_annotation::VisualAnnotationRunTerminal {
        use crate::tools::ToolRegistryLimits;
        use crate::visual_annotation::{
            decode_visual_annotation_terminal, submit_visual_annotation_suggestions_definition,
            submit_visual_annotation_suggestions_tool_arc, VisualAnnotationRunTerminal,
            SUBMIT_VISUAL_ANNOTATION_SUGGESTIONS,
        };

        // ---- Pre-flight ----

        if cancellation.is_cancelled() {
            return VisualAnnotationRunTerminal::Cancelled;
        }

        let attachments = input.take_model_attachments();
        let attachment_count = attachments.len() as u32;

        let start = tokio::time::Instant::now();
        let mut tracker = BudgetTracker::new(budget, start);

        if let Err(err) = tracker.charge(UsageSnapshot {
            attachments: attachment_count,
            ..UsageSnapshot::default()
        }) {
            return map_budget_error_to_visual_annotation(err);
        }

        let tool_definitions = vec![submit_visual_annotation_suggestions_definition()];
        let tool_names: BTreeSet<String> =
            tool_definitions.iter().map(|t| t.name.clone()).collect();

        let mut registry = ToolRegistry::new(ToolRegistryLimits::permissive());
        if let Err(e) = registry.register(submit_visual_annotation_suggestions_tool_arc()) {
            tracing::error!(
                target: "rollshot::agent::visual_annotation",
                error = %e,
                "failed to register visual annotation stub tool"
            );
            return VisualAnnotationRunTerminal::ProtocolFailure;
        }

        let mut rig_run = rig_core::agent::run::AgentRun::new(rig_core::message::Message::user(
            &input.user_message,
        ))
        .max_turns(self.config.max_turns);

        let mut total_assistant_bytes: usize = 0;
        let max_assistant_bytes = self.config.max_assistant_bytes;
        let mut last_assistant_text = String::new();
        let mut first_model_call = true;

        // ---- Loop ----

        loop {
            if cancellation.is_cancelled() {
                return VisualAnnotationRunTerminal::Cancelled;
            }

            if let Err(err) = tracker.check_wall_time(tokio::time::Instant::now()) {
                return map_budget_error_to_visual_annotation(err);
            }

            let step = match rig_run.next_step() {
                Ok(s) => s,
                Err(e) => {
                    if matches!(e, rig_core::completion::PromptError::MaxTurnsError { .. }) {
                        tracing::debug!(
                            target: "rollshot::agent::visual_annotation",
                            max_turns = self.config.max_turns,
                            "visual annotation run exceeded model-call budget"
                        );
                        return VisualAnnotationRunTerminal::BudgetExhausted {
                            dimension: BudgetDimension::ModelCalls,
                        };
                    }
                    tracing::debug!(
                        target: "rollshot::agent::visual_annotation",
                        error = %e,
                        "visual annotation rig agent run returned a protocol error"
                    );
                    return VisualAnnotationRunTerminal::ProtocolFailure;
                }
            };

            match step {
                rig_core::agent::run::AgentRunStep::CallModel {
                    prompt, history, ..
                } => {
                    let turn_attachments = if first_model_call {
                        first_model_call = false;
                        attachments.clone()
                    } else {
                        Vec::new()
                    };

                    let mut history_msgs: Vec<crate::model::ModelMessage> = Vec::new();
                    for m in &history {
                        crate::model::push_model_messages(m, &mut history_msgs);
                    }
                    crate::model::push_model_messages(&prompt, &mut history_msgs);

                    let request = crate::model::ModelRequest {
                        model: input.manifest.model.clone(),
                        prompt: String::new(),
                        history: history_msgs,
                        turn: rig_run.turn(),
                        tool_definitions: tool_definitions.clone(),
                        system_prompt: Some(
                            AgentTaskProfile::VisualAnnotation
                                .system_prompt()
                                .to_string(),
                        ),
                        max_tokens: None,
                        attachments: turn_attachments,
                    };

                    let turn_result = self
                        .drive_streamed_turn(
                            &mut rig_run,
                            &tool_names,
                            provider,
                            &NullEventSink,
                            &mut tracker,
                            &mut total_assistant_bytes,
                            max_assistant_bytes,
                            cancellation,
                            request,
                            &mut last_assistant_text,
                        )
                        .await;

                    match turn_result {
                        Ok(()) => {}
                        Err(DriverError::BudgetExhausted(dim)) => {
                            return VisualAnnotationRunTerminal::BudgetExhausted { dimension: dim };
                        }
                        Err(DriverError::Cancelled) => {
                            return VisualAnnotationRunTerminal::Cancelled;
                        }
                        Err(DriverError::ProviderFailure(msg)) => {
                            tracing::debug!(
                                target: "rollshot::agent::visual_annotation",
                                error = %msg,
                                "visual annotation provider stream failed"
                            );
                            return VisualAnnotationRunTerminal::ProviderFailure;
                        }
                        Err(DriverError::AgentProtocolFailure(msg)) => {
                            tracing::debug!(
                                target: "rollshot::agent::visual_annotation",
                                error = %msg,
                                "visual annotation model turn produced a protocol error"
                            );
                            return VisualAnnotationRunTerminal::ProtocolFailure;
                        }
                    }

                    tracker.apply_turn();
                }
                rig_core::agent::run::AgentRunStep::CallTools { calls } => {
                    if calls.len() != 1
                        || calls[0].tool_call.function.name != SUBMIT_VISUAL_ANNOTATION_SUGGESTIONS
                    {
                        tracing::debug!(
                            target: "rollshot::agent::visual_annotation",
                            call_count = calls.len(),
                            tool_name = %calls.first().map(|c| c.tool_call.function.name.as_str()).unwrap_or(""),
                            "visual annotation runner rejecting tool call batch"
                        );
                        return VisualAnnotationRunTerminal::ProtocolFailure;
                    }

                    let pending = &calls[0];
                    let decoded = match decode_visual_annotation_terminal(
                        &pending.tool_call.function.arguments,
                    ) {
                        Ok(t) => t,
                        Err(err) => {
                            tracing::debug!(
                                target: "rollshot::agent::visual_annotation",
                                error = %err,
                                "visual annotation terminal payload failed validation"
                            );
                            return VisualAnnotationRunTerminal::ProtocolFailure;
                        }
                    };

                    if let Err(err) = tracker.charge(UsageSnapshot {
                        tool_calls: 1,
                        ..UsageSnapshot::default()
                    }) {
                        return map_budget_error_to_visual_annotation(err);
                    }

                    let tool_call = ToolCall {
                        name: pending.tool_call.function.name.clone(),
                        arguments_json: pending.tool_call.function.arguments.clone(),
                    };
                    let terminal_tools: BTreeSet<String> =
                        [SUBMIT_VISUAL_ANNOTATION_SUGGESTIONS.to_string()]
                            .into_iter()
                            .collect();
                    let results = registry
                        .execute_calls(&[tool_call], cancellation, &terminal_tools)
                        .await;

                    let mut rig_results: Vec<rig_core::message::UserContent> = Vec::new();
                    for (i, result) in results.into_iter().enumerate() {
                        let call_id = calls[i].tool_call.id.clone();
                        match result {
                            Ok(ToolOutcome::Success { result_json }) => {
                                let result_str =
                                    serde_json::to_string(&result_json).unwrap_or_default();
                                rig_results.push(rig_core::message::UserContent::tool_result(
                                    call_id,
                                    rig_core::message::ToolResultContent::from_tool_output(
                                        result_str,
                                    ),
                                ));
                            }
                            Ok(ToolOutcome::Recoverable { error }) => {
                                tracing::debug!(
                                    target: "rollshot::agent::visual_annotation",
                                    error = %error,
                                    "visual annotation stub tool rejected payload"
                                );
                                return VisualAnnotationRunTerminal::ProtocolFailure;
                            }
                            Err(err) => {
                                tracing::debug!(
                                    target: "rollshot::agent::visual_annotation",
                                    error = %err,
                                    "visual annotation stub tool returned an error"
                                );
                                return VisualAnnotationRunTerminal::ProtocolFailure;
                            }
                        }
                    }

                    if let Err(e) = rig_run.tool_results(rig_results) {
                        tracing::debug!(
                            target: "rollshot::agent::visual_annotation",
                            error = %e,
                            "rig agent run tool_results failed"
                        );
                        return VisualAnnotationRunTerminal::ProtocolFailure;
                    }

                    tracker.apply_turn();
                    return decoded;
                }
                rig_core::agent::run::AgentRunStep::Done(_) => {
                    tracing::debug!(
                        target: "rollshot::agent::visual_annotation",
                        "visual annotation model completed without a terminal tool call"
                    );
                    return VisualAnnotationRunTerminal::ProtocolFailure;
                }
            }
        }
    }
}

fn map_budget_error_to_visual_annotation(
    err: BudgetError,
) -> crate::visual_annotation::VisualAnnotationRunTerminal {
    match err {
        BudgetError::Exceeded(dim) => {
            crate::visual_annotation::VisualAnnotationRunTerminal::BudgetExhausted {
                dimension: dim,
            }
        }
        BudgetError::Overflow => {
            crate::visual_annotation::VisualAnnotationRunTerminal::ProtocolFailure
        }
    }
}

// ---------- Tests ----------

#[cfg(test)]
#[allow(clippy::useless_vec)]
pub(crate) mod tests {
    use super::*;
    use crate::domain::SessionId;
    use crate::runtime::{EvidenceKind, NullEventSink, RunBudget};
    use crate::tools::{
        DryRunTool, EditSourceTool, GetContextSummaryTool, InspectImageContextTool, OcrTool,
        ReadCurrentSourceTool, RegionFeaturesTool, ReplaceSourceTool, SubmitForReviewTool,
        ToolRegistryLimits, ValidateSourceTool,
    };
    use rig_core::completion::Usage;
    use rig_core::streaming::StreamedAssistantContent;
    use std::sync::Arc;
    use std::sync::Mutex;

    // ---- Task profile parity ----

    #[test]
    fn smart_redaction_profile_matches_existing_prompt_and_tools() {
        assert_eq!(
            AgentTaskProfile::SmartRedaction.system_prompt(),
            SMART_REDACTION_SYSTEM_PROMPT,
        );
        assert_eq!(
            AgentTaskProfile::SmartRedaction.terminal_tools(),
            &["submit_for_review", "request_user_input"],
        );
    }

    #[test]
    fn visual_annotation_profile_advertises_only_submit_visual_annotation_suggestions() {
        assert_eq!(
            AgentTaskProfile::VisualAnnotation.system_prompt(),
            VISUAL_ANNOTATION_SYSTEM_PROMPT,
        );
        assert_eq!(
            AgentTaskProfile::VisualAnnotation.terminal_tools(),
            &["submit_visual_annotation_suggestions"],
        );
    }

    // ---- Stream item builders ----

    fn text_item(text: &str) -> StreamedAssistantContent<MockResponse> {
        StreamedAssistantContent::text(text)
    }

    fn tool_call_delta_name(id: &str, name: &str) -> StreamedAssistantContent<MockResponse> {
        use rig_core::streaming::ToolCallDeltaContent;
        StreamedAssistantContent::ToolCallDelta {
            id: id.to_string(),
            internal_call_id: format!("internal_{id}"),
            content: ToolCallDeltaContent::Name(name.to_string()),
        }
    }

    fn tool_call_delta_args(id: &str, args: &str) -> StreamedAssistantContent<MockResponse> {
        use rig_core::streaming::ToolCallDeltaContent;
        StreamedAssistantContent::ToolCallDelta {
            id: id.to_string(),
            internal_call_id: format!("internal_{id}"),
            content: ToolCallDeltaContent::Delta(args.to_string()),
        }
    }

    fn final_item(usage: Usage) -> StreamedAssistantContent<MockResponse> {
        StreamedAssistantContent::Final(MockResponse::with_usage(usage))
    }

    fn usage(input: u64, output: u64) -> Usage {
        Usage {
            input_tokens: input,
            output_tokens: output,
            total_tokens: input + output,
            ..Usage::new()
        }
    }

    // ---- Test context builder ----

    fn test_ctx(source: &str) -> Arc<ToolContext> {
        let mut policy = rollshot_automation::ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(5),
            4 * 1024 * 1024,
            1024 * 1024,
        );
        policy.proposal_limits.max_total_area_fraction = 0.5;
        Arc::new(ToolContext::new(
            SessionId::new(42),
            source.into(),
            rollshot_automation::ValidationLimits::default(),
            policy,
            (100, 100),
            &RunCancellation::new(),
        ))
    }

    fn valid_js() -> &'static str {
        "function main(input) { return [{kind: 'addRedaction', bounds: {x: 0, y: 0, width: 10, height: 10}, confidence: 0.9, label: 'test'}]; }"
    }

    fn register_all_tools(reg: &mut ToolRegistry, ctx: &Arc<ToolContext>) {
        let executor = Arc::new(FakeExecutor::with_valid_proposal());
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        reg.register(Arc::new(GetContextSummaryTool::new(ctx.clone())))
            .unwrap();
        reg.register(Arc::new(ReadCurrentSourceTool::new(ctx.clone())))
            .unwrap();
        reg.register(Arc::new(ReplaceSourceTool::new(ctx.clone())))
            .unwrap();
        reg.register(Arc::new(EditSourceTool::new(ctx.clone())))
            .unwrap();
        reg.register(Arc::new(ValidateSourceTool::new(ctx.clone())))
            .unwrap();
        reg.register(Arc::new(DryRunTool::new(ctx.clone(), executor, host)))
            .unwrap();
        reg.register(Arc::new(SubmitForReviewTool::new(ctx.clone())))
            .unwrap();
    }

    struct FakeExecutor {
        output_json: String,
        capability_calls: u32,
    }

    impl FakeExecutor {
        fn with_valid_proposal() -> Self {
            let output = serde_json::json!({
                "candidates": [{
                    "kind": "addRedaction",
                    "bounds": {"x": 5, "y": 5, "width": 20, "height": 20},
                    "confidence": 0.85,
                    "label": "email"
                }]
            });
            Self {
                output_json: serde_json::to_string(&output).unwrap(),
                capability_calls: 0,
            }
        }

        fn with_policy_violating_proposal() -> Self {
            let output = serde_json::json!({
                "candidates": [{
                    "kind": "addRedaction",
                    "bounds": {"x": 0, "y": 0, "width": 90, "height": 90},
                    "confidence": 0.9,
                    "label": "huge"
                }]
            });
            Self {
                output_json: serde_json::to_string(&output).unwrap(),
                capability_calls: 0,
            }
        }

        fn with_capability_calls(capability_calls: u32) -> Self {
            let mut e = Self::with_valid_proposal();
            e.capability_calls = capability_calls;
            e
        }
    }

    impl rollshot_automation::AutomationExecutor for FakeExecutor {
        fn execute(
            &self,
            _automation: &rollshot_automation::ValidatedAutomation,
            _input: &rollshot_automation::AutomationInput,
            _proposal: &rollshot_automation::ProposalContext,
            _host: &mut dyn rollshot_automation::AutomationHost,
            _policy: &rollshot_automation::ExecutionPolicy,
            _cancellation: &rollshot_automation::CancellationFlag,
        ) -> Result<rollshot_automation::AutomationExecution, rollshot_automation::ExecutionError>
        {
            Ok(rollshot_automation::AutomationExecution {
                output_json: self.output_json.clone(),
                metrics: rollshot_automation::ExecutionMetrics {
                    duration: std::time::Duration::from_millis(10),
                    capability_calls: self.capability_calls,
                    output_bytes: self.output_json.len(),
                    interrupted: false,
                },
            })
        }
    }

    // ---- Event collector ----

    struct CollectingSink {
        events: Mutex<Vec<RunEvent>>,
    }

    impl CollectingSink {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }

        fn drain(&self) -> Vec<RunEvent> {
            std::mem::take(&mut *self.events.lock().unwrap())
        }
    }

    impl RunEventSink for CollectingSink {
        fn emit(&self, event: RunEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    // ---- Full author loop test ----

    #[tokio::test]
    async fn full_author_loop() {
        let ctx = test_ctx("function main(input) { return { candidates: [] }; }");
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        let js = valid_js();
        let replace_args = serde_json::json!({"source": js, "generation": 0}).to_string();
        let validate_args = serde_json::json!({"source": js, "generation": 1}).to_string();
        let dry_run_args = serde_json::json!({"source": js, "generation": 1}).to_string();
        let submit_args = serde_json::json!({"generation": 1}).to_string();

        // Turn 1: inspect + replace
        let turn1 = vec![
            tool_call_delta_name("tc_1", "inspect_context_summary"),
            tool_call_delta_name("tc_2", "replace_source"),
            tool_call_delta_args("tc_2", &replace_args),
            final_item(usage(50, 30)),
        ];

        // Turn 2: validate + dry_run + submit. submit_for_review terminates the
        // run here (§4.5) — there is no third model turn.
        let turn2 = vec![
            tool_call_delta_name("tc_3", "validate_source"),
            tool_call_delta_args("tc_3", &validate_args),
            tool_call_delta_name("tc_4", "dry_run"),
            tool_call_delta_args("tc_4", &dry_run_args),
            tool_call_delta_name("tc_5", "submit_for_review"),
            tool_call_delta_args("tc_5", &submit_args),
            final_item(usage(40, 25)),
        ];

        let turns = vec![turn1, turn2];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(42));
        let cancel = RunCancellation::new();
        let sink = CollectingSink::new();

        let result = runner
            .run(
                AuthorizedModelInput::new(
                    "test".into(),
                    "test-model".into(),
                    "author a redaction".into(),
                    vec![],
                    vec![],
                )
                .unwrap(),
                &mut session,
                &reg,
                RunBudget::unlimited(),
                &cancel,
                &sink,
                &ctx,
                model_fn,
            )
            .await;

        // Assert ReadyForReview
        match &result {
            RunTerminalState::ReadyForReview(r) => {
                assert_eq!(r.session_id, SessionId::new(42));
                assert_eq!(r.generation, 1);
                // turns 1 + 2 only (submit terminates the run): 50+40 / 30+25.
                assert_eq!(r.usage.input_tokens, 90);
                assert_eq!(r.usage.output_tokens, 55);
            }
            other => panic!("expected ReadyForReview, got {other:?}"),
        }

        // Assert tool order via events
        let events = sink.drain();
        let tool_starts: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                RunEvent::ToolCallStart { name } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            tool_starts,
            vec![
                "inspect_context_summary",
                "replace_source",
                "validate_source",
                "dry_run",
                "submit_for_review"
            ]
        );
        assert!(events.iter().any(|e| {
            matches!(
                e,
                RunEvent::SourceChanged {
                    tool,
                    diff
                } if tool == "replace_source"
                    && diff.old_generation == 0
                    && diff.new_generation == 1
                    && diff.lines.iter().any(|line| {
                        line.kind == crate::runtime::SourceDiffLineKind::Removed
                    })
                    && diff.lines.iter().any(|line| {
                        line.kind == crate::runtime::SourceDiffLineKind::Added
                    })
            )
        }));

        // Assert generation evidence
        let draft = ctx.draft.lock().unwrap();
        assert_eq!(draft.generation(), 1);
        assert!(draft
            .evidence()
            .iter()
            .any(|e| e.kind == EvidenceKind::Validation && e.source_generation == 1));
        assert!(draft
            .evidence()
            .iter()
            .any(|e| e.kind == EvidenceKind::DryRun && e.source_generation == 1));

        // Assert source was replaced
        assert_eq!(*ctx.source.lock().unwrap(), valid_js());

        // Assert session has completed exchange
        assert_eq!(session.exchanges().len(), 1);
        assert_eq!(session.exchanges()[0].user.text, "author a redaction");
    }

    // ---- Terminal: NeedsUserInput ----

    #[tokio::test]
    async fn terminal_needs_user_input() {
        use crate::tools::RequestUserInputTool;

        let ctx = test_ctx("source");
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        reg.register(Arc::new(RequestUserInputTool::new(ctx.clone())))
            .unwrap();

        let turn1 = vec![
            tool_call_delta_name("tc_1", "request_user_input"),
            final_item(usage(10, 5)),
        ];

        let turns = vec![turn1];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                RunBudget::unlimited(),
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(matches!(result, RunTerminalState::NeedsUserInput(_)));
    }

    // ---- Terminal: cancellation before model ----

    #[tokio::test]
    async fn terminal_cancel_before_model() {
        let ctx = test_ctx("src");
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        let model_fn = |_turn: usize| -> Option<Vec<StreamedAssistantContent<MockResponse>>> {
            Some(vec![text_item("x"), final_item(usage(1, 1))])
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();
        cancel.cancel();

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                RunBudget::unlimited(),
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert_eq!(result, RunTerminalState::Cancelled);
    }

    // ---- Terminal: input token budget ----

    #[tokio::test]
    async fn terminal_input_token_budget() {
        let ctx = test_ctx("src");
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        let turn1 = vec![text_item("text"), final_item(usage(100, 5))];
        let turn2 = vec![text_item("more"), final_item(usage(5, 3))];
        let turns = vec![turn1, turn2];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();
        let budget = RunBudget {
            input_tokens: 50,
            ..RunBudget::unlimited()
        };

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                budget,
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(matches!(
            result,
            RunTerminalState::BudgetExhausted {
                dimension: BudgetDimension::InputTokens
            }
        ));
    }

    // ---- Terminal: output token budget ----

    #[tokio::test]
    async fn terminal_output_token_budget() {
        let ctx = test_ctx("src");
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        let turn1 = vec![text_item("text"), final_item(usage(5, 100))];
        let turns = vec![turn1];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();
        let budget = RunBudget {
            output_tokens: 10,
            ..RunBudget::unlimited()
        };

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                budget,
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(matches!(
            result,
            RunTerminalState::BudgetExhausted {
                dimension: BudgetDimension::OutputTokens
            }
        ));
    }

    // ---- Terminal: tool call budget ----

    #[tokio::test]
    async fn terminal_tool_call_budget() {
        let ctx = test_ctx("src");
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        // Turn with 2 tool calls, budget allows only 1
        let turn1 = vec![
            tool_call_delta_name("tc_1", "inspect_context_summary"),
            tool_call_delta_name("tc_2", "replace_source"),
            tool_call_delta_args("tc_2", r#"{"source":"new","generation":0}"#),
            final_item(usage(10, 5)),
        ];

        let turns = vec![turn1];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();
        let budget = RunBudget {
            tool_calls: 1,
            ..RunBudget::unlimited()
        };

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                budget,
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(matches!(
            result,
            RunTerminalState::BudgetExhausted {
                dimension: BudgetDimension::ToolCalls
            }
        ));
    }

    // ---- Terminal: source bytes budget ----

    #[tokio::test]
    async fn terminal_source_bytes_budget() {
        let ctx = test_ctx("src");
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        let long_text = "x".repeat(200);
        let turn1 = vec![text_item(&long_text), final_item(usage(5, 3))];

        let turns = vec![turn1];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig {
            max_assistant_bytes: 100,
            ..AgentConfig::default()
        });
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                RunBudget::unlimited(),
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(matches!(
            result,
            RunTerminalState::BudgetExhausted {
                dimension: BudgetDimension::SourceBytes
            }
        ));
    }

    // ---- Terminal: model-call budget ----

    #[tokio::test]
    async fn terminal_model_calls_budget() {
        let ctx = test_ctx("src");
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        // Turn 1 makes a tool call (forcing a second model turn); the second
        // model turn must exceed a model_calls budget of 1.
        let turn1 = vec![
            tool_call_delta_name("tc_1", "inspect_context_summary"),
            final_item(usage(5, 3)),
        ];
        let turn2 = vec![text_item("more"), final_item(usage(5, 3))];
        let turns = vec![turn1, turn2];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();
        let budget = RunBudget {
            model_calls: 1,
            ..RunBudget::unlimited()
        };

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                budget,
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(matches!(
            result,
            RunTerminalState::BudgetExhausted {
                dimension: BudgetDimension::ModelCalls
            }
        ));
    }

    // ---- Terminal: candidate-count budget ----

    #[tokio::test]
    async fn terminal_candidate_count_budget() {
        let ctx = test_ctx(valid_js());
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx); // dry-run yields 1 candidate

        let js = valid_js();
        let replace_args = serde_json::json!({"source": js, "generation": 0}).to_string();
        let validate_args = serde_json::json!({"source": js, "generation": 1}).to_string();
        let dry_run_args = serde_json::json!({"source": js, "generation": 1}).to_string();

        let turn1 = vec![
            tool_call_delta_name("tc_1", "replace_source"),
            tool_call_delta_args("tc_1", &replace_args),
            tool_call_delta_name("tc_2", "validate_source"),
            tool_call_delta_args("tc_2", &validate_args),
            tool_call_delta_name("tc_3", "dry_run"),
            tool_call_delta_args("tc_3", &dry_run_args),
            final_item(usage(10, 5)),
        ];
        let turns = vec![turn1];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();
        let budget = RunBudget {
            candidate_count: 0,
            ..RunBudget::unlimited()
        };

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                budget,
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(matches!(
            result,
            RunTerminalState::BudgetExhausted {
                dimension: BudgetDimension::CandidateCount
            }
        ));
    }

    // ---- Terminal: affected-area budget ----

    #[tokio::test]
    async fn terminal_affected_area_budget() {
        let ctx = test_ctx(valid_js());
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx); // dry-run yields a 20x20 = 400 area redaction

        let js = valid_js();
        let replace_args = serde_json::json!({"source": js, "generation": 0}).to_string();
        let validate_args = serde_json::json!({"source": js, "generation": 1}).to_string();
        let dry_run_args = serde_json::json!({"source": js, "generation": 1}).to_string();

        let turn1 = vec![
            tool_call_delta_name("tc_1", "replace_source"),
            tool_call_delta_args("tc_1", &replace_args),
            tool_call_delta_name("tc_2", "validate_source"),
            tool_call_delta_args("tc_2", &validate_args),
            tool_call_delta_name("tc_3", "dry_run"),
            tool_call_delta_args("tc_3", &dry_run_args),
            final_item(usage(10, 5)),
        ];
        let turns = vec![turn1];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();
        let budget = RunBudget {
            affected_area: 100, // 400 > 100
            ..RunBudget::unlimited()
        };

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                budget,
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(matches!(
            result,
            RunTerminalState::BudgetExhausted {
                dimension: BudgetDimension::AffectedArea
            }
        ));
    }

    // ---- Terminal: capability-calls budget ----

    #[tokio::test]
    async fn terminal_capability_calls_budget() {
        let ctx = test_ctx(valid_js());
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        // dry-run executor reports 5 capability calls.
        let executor = Arc::new(FakeExecutor::with_capability_calls(5));
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        reg.register(Arc::new(ReplaceSourceTool::new(ctx.clone())))
            .unwrap();
        reg.register(Arc::new(ValidateSourceTool::new(ctx.clone())))
            .unwrap();
        reg.register(Arc::new(DryRunTool::new(ctx.clone(), executor, host)))
            .unwrap();

        let js = valid_js();
        let replace_args = serde_json::json!({"source": js, "generation": 0}).to_string();
        let validate_args = serde_json::json!({"source": js, "generation": 1}).to_string();
        let dry_run_args = serde_json::json!({"source": js, "generation": 1}).to_string();

        let turn1 = vec![
            tool_call_delta_name("tc_1", "replace_source"),
            tool_call_delta_args("tc_1", &replace_args),
            tool_call_delta_name("tc_2", "validate_source"),
            tool_call_delta_args("tc_2", &validate_args),
            tool_call_delta_name("tc_3", "dry_run"),
            tool_call_delta_args("tc_3", &dry_run_args),
            final_item(usage(10, 5)),
        ];
        let turns = vec![turn1];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();
        let budget = RunBudget {
            capability_calls: 2, // 5 > 2
            ..RunBudget::unlimited()
        };

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                budget,
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(matches!(
            result,
            RunTerminalState::BudgetExhausted {
                dimension: BudgetDimension::CapabilityCalls
            }
        ));
    }

    // ---- Terminal: malformed tool-call arguments ----

    #[tokio::test]
    async fn malformed_tool_arguments_are_protocol_failure() {
        // Non-empty tool-call arguments that do not reassemble into valid JSON
        // are an unrecoverable protocol failure (§6.2), not silently coerced to
        // an empty object.
        let ctx = test_ctx("src");
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        let turn1 = vec![
            tool_call_delta_name("tc_1", "replace_source"),
            // Truncated / invalid JSON.
            tool_call_delta_args("tc_1", r#"{"source": "x", "generation":"#),
            final_item(usage(5, 3)),
        ];
        let turns = vec![turn1];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                RunBudget::unlimited(),
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(
            matches!(result, RunTerminalState::AgentProtocolFailure { .. }),
            "malformed tool args must be AgentProtocolFailure, got {result:?}"
        );
    }

    // ---- Terminal: unknown tool ----

    #[tokio::test]
    async fn terminal_unknown_tool() {
        let ctx = test_ctx("src");
        let reg = ToolRegistry::new(ToolRegistryLimits::permissive());

        let turn1 = vec![
            tool_call_delta_name("tc_1", "nonexistent_tool"),
            final_item(usage(5, 3)),
        ];

        let turns = vec![turn1];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                RunBudget::unlimited(),
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(matches!(result, RunTerminalState::ProviderFailure { .. }));
    }

    // ---- Terminal: source validation failure ----

    #[tokio::test]
    async fn terminal_source_validation_failure() {
        let ctx = test_ctx(valid_js());
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        // Turn 1: replace with valid source, then validate with INVALID source
        let turn1 = vec![
            tool_call_delta_name("tc_1", "replace_source"),
            tool_call_delta_args("tc_1", r#"{"source":"valid JS source","generation":0}"#),
            tool_call_delta_name("tc_2", "validate_source"),
            tool_call_delta_args("tc_2", r#"{"source":"invalid {{{","generation":1}"#),
            final_item(usage(10, 5)),
        ];

        // Turn 2: model gives up
        let turn2 = vec![text_item("can't fix it"), final_item(usage(5, 3))];

        let turns = vec![turn1, turn2];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                RunBudget::unlimited(),
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert_eq!(result, RunTerminalState::SourceValidationFailure);
    }

    // ---- Terminal: runtime failure ----

    #[tokio::test]
    async fn terminal_runtime_failure() {
        let ctx = test_ctx(valid_js());
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());

        // Register tools with a policy-violating executor
        let bad_executor = Arc::new(FakeExecutor::with_policy_violating_proposal());
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        reg.register(Arc::new(ReplaceSourceTool::new(ctx.clone())))
            .unwrap();
        reg.register(Arc::new(ValidateSourceTool::new(ctx.clone())))
            .unwrap();
        reg.register(Arc::new(DryRunTool::new(ctx.clone(), bad_executor, host)))
            .unwrap();

        // Turn 1: replace + validate (succeeds) + dry_run (fails policy)
        let turn1 = vec![
            tool_call_delta_name("tc_1", "replace_source"),
            tool_call_delta_args("tc_1", r#"{"source":"valid JS source","generation":0}"#),
            tool_call_delta_name("tc_2", "validate_source"),
            tool_call_delta_args("tc_2", r#"{"source":"valid JS source","generation":1}"#),
            tool_call_delta_name("tc_3", "dry_run"),
            tool_call_delta_args("tc_3", r#"{"source":"valid JS source","generation":1}"#),
            final_item(usage(20, 10)),
        ];

        // Turn 2: model gives up
        let turn2 = vec![text_item("runtime error"), final_item(usage(5, 3))];

        let turns = vec![turn1, turn2];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                RunBudget::unlimited(),
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert_eq!(result, RunTerminalState::RuntimeFailure);
    }

    // ---- Terminal: submit without dry-run must not fabricate ReadyForReview ----

    #[tokio::test]
    async fn submit_without_dry_run_does_not_become_ready_for_review() {
        // The model validates then submits, skipping dry_run. Per §4.5/§8.4 the
        // submission has incomplete evidence, so the run must NOT terminate as
        // ReadyForReview (the driver previously fabricated an empty automation
        // and proposal here).
        let ctx = test_ctx(valid_js());
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        let js = valid_js();
        let replace_args = serde_json::json!({"source": js, "generation": 0}).to_string();
        let validate_args = serde_json::json!({"source": js, "generation": 1}).to_string();
        let submit_args = serde_json::json!({"generation": 1}).to_string();

        // Turn 1: replace + validate + submit (no dry_run).
        let turn1 = vec![
            tool_call_delta_name("tc_1", "replace_source"),
            tool_call_delta_args("tc_1", &replace_args),
            tool_call_delta_name("tc_2", "validate_source"),
            tool_call_delta_args("tc_2", &validate_args),
            tool_call_delta_name("tc_3", "submit_for_review"),
            tool_call_delta_args("tc_3", &submit_args),
            final_item(usage(20, 10)),
        ];
        // Turn 2: model gives up.
        let turn2 = vec![text_item("giving up"), final_item(usage(5, 3))];

        let turns = vec![turn1, turn2];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                RunBudget::unlimited(),
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(
            !matches!(result, RunTerminalState::ReadyForReview(_)),
            "submit without dry-run must not become ReadyForReview, got {result:?}"
        );
        assert!(
            matches!(result, RunTerminalState::AgentProtocolFailure { .. }),
            "expected AgentProtocolFailure, got {result:?}"
        );
    }

    // ---- Terminal: model completion without submission ----

    #[tokio::test]
    async fn terminal_model_completion_without_submission() {
        let ctx = test_ctx("src");
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        let turns = vec![vec![text_item("I'm done"), final_item(usage(5, 3))]];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                RunBudget::unlimited(),
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(matches!(
            result,
            RunTerminalState::AgentProtocolFailure { .. }
        ));
    }

    // ---- Terminal: provider failure (model returns None) ----

    #[tokio::test]
    async fn terminal_provider_failure_no_items() {
        let ctx = test_ctx("src");
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        let model_fn =
            |_turn: usize| -> Option<Vec<StreamedAssistantContent<MockResponse>>> { None };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                RunBudget::unlimited(),
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(matches!(result, RunTerminalState::ProviderFailure { .. }));
    }

    // ---- Terminal: validation attempts budget (per-tool budgets tracked by BudgetTracker) ----

    #[tokio::test]
    async fn terminal_validation_attempts_budget() {
        // Per-tool budget dimensions (validation_attempts, dry_run_attempts, etc.)
        // are enforced by BudgetTracker. The driver charges tool_calls for the turn;
        // per-tool dimensions are validated by the tracker's charge() method.
        // This test verifies the budget is charged and the turn succeeds when
        // validation_attempts is within budget.
        let ctx = test_ctx(valid_js());
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        let js = valid_js();
        let replace_args = serde_json::json!({"source": js, "generation": 0}).to_string();
        let validate_args = serde_json::json!({"source": js, "generation": 1}).to_string();
        let dry_run_args = serde_json::json!({"source": js, "generation": 1}).to_string();
        let submit_args = serde_json::json!({"generation": 1}).to_string();

        let turn1 = vec![
            tool_call_delta_name("tc_1", "replace_source"),
            tool_call_delta_args("tc_1", &replace_args),
            tool_call_delta_name("tc_2", "validate_source"),
            tool_call_delta_args("tc_2", &validate_args),
            tool_call_delta_name("tc_2b", "dry_run"),
            tool_call_delta_args("tc_2b", &dry_run_args),
            final_item(usage(10, 5)),
        ];

        let turn2 = vec![
            tool_call_delta_name("tc_3", "submit_for_review"),
            tool_call_delta_args("tc_3", &submit_args),
            final_item(usage(5, 3)),
        ];

        // Turn 3: final text
        let turn3 = vec![text_item("done"), final_item(usage(5, 3))];

        let turns = vec![turn1, turn2, turn3];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();
        // validation_attempts budget of 1 should allow one validation
        let budget = RunBudget {
            validation_attempts: 1,
            ..RunBudget::unlimited()
        };

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                budget,
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        // With validation_attempts=1, the validate_source call should succeed
        // and the run should complete with ReadyForReview
        assert!(matches!(result, RunTerminalState::ReadyForReview(_)));
    }

    // ---- Terminal: per-tool validation_attempts budget exceeded (I4) ----

    #[tokio::test]
    async fn terminal_validation_attempts_budget_exceeded() {
        let ctx = test_ctx(valid_js());
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        let js = valid_js();
        let replace_args = serde_json::json!({"source": js, "generation": 0}).to_string();
        let validate_args = serde_json::json!({"source": js, "generation": 1}).to_string();

        let turn1 = vec![
            tool_call_delta_name("tc_1", "replace_source"),
            tool_call_delta_args("tc_1", &replace_args),
            tool_call_delta_name("tc_2", "validate_source"),
            tool_call_delta_args("tc_2", &validate_args),
            final_item(usage(10, 5)),
        ];

        let turns = vec![turn1];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();
        // Budget allows 0 validation attempts — the validate_source call
        // in the turn should cause BudgetExhausted.
        let budget = RunBudget {
            validation_attempts: 0,
            ..RunBudget::unlimited()
        };

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                budget,
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(matches!(
            result,
            RunTerminalState::BudgetExhausted {
                dimension: BudgetDimension::ValidationAttempts
            }
        ));
    }

    // ---- Terminal: cancellation during stream (C2) ----

    #[tokio::test]
    async fn terminal_cancel_during_stream() {
        let ctx = test_ctx("src");
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        let cancel = RunCancellation::new();
        let cancel_clone = cancel.clone();

        let mut call_count = 0;
        let model_fn = move |_turn: usize| -> Option<Vec<StreamedAssistantContent<MockResponse>>> {
            call_count += 1;
            if call_count == 1 {
                // First call: return items, cancel between first and second item
                cancel_clone.cancel();
                Some(vec![
                    text_item("before cancel"),
                    text_item("after cancel"),
                    final_item(usage(5, 3)),
                ])
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                RunBudget::unlimited(),
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert_eq!(result, RunTerminalState::Cancelled);
    }

    // ---- Terminal: provider failure via ModelStreamEvent::Error (I1) ----

    #[tokio::test]
    async fn terminal_provider_failure_stream_error() {
        let ctx = test_ctx("src");
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        // Emit a tool call with an unknown name via streaming delta.
        // The assembler detects the unknown tool during ingest() and emits
        // StreamedTurnEvent::InvalidToolCall, which drive_streamed_turn maps
        // to ModelStreamEvent::Error → DriverError::ProviderFailure.
        let turn1 = vec![
            tool_call_delta_name("tc_1", "nonexistent_tool"),
            final_item(usage(5, 3)),
        ];

        let turns = vec![turn1];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                RunBudget::unlimited(),
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(matches!(result, RunTerminalState::ProviderFailure { .. }));
    }

    // ---- Terminal: wall-time budget (I2) ----

    #[tokio::test]
    async fn terminal_wall_time_budget() {
        let ctx = test_ctx("src");
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        let turn1 = vec![text_item("text"), final_item(usage(5, 3))];
        let turns = vec![turn1];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();
        let budget = RunBudget {
            wall_time: std::time::Duration::from_micros(1),
            ..RunBudget::unlimited()
        };

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                budget,
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(matches!(
            result,
            RunTerminalState::BudgetExhausted {
                dimension: BudgetDimension::WallTime
            }
        ));
    }

    // ---- Terminal: needs_user_input per-call ordering (I5) ----

    #[tokio::test]
    async fn terminal_needs_user_input_with_submit_in_same_batch() {
        use crate::tools::RequestUserInputTool;

        let ctx = test_ctx("src");
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);
        reg.register(Arc::new(RequestUserInputTool::new(ctx.clone())))
            .unwrap();

        // Both request_user_input and submit_for_review in the same batch.
        // request_user_input is processed first (lower tc id), so needs_user_input
        // is set before submit_for_review sets has_submit_evidence.
        // The Done handler checks needs_user_input first, so NeedsUserInput wins.
        let turn1 = vec![
            tool_call_delta_name("tc_1", "request_user_input"),
            tool_call_delta_name("tc_2", "submit_for_review"),
            tool_call_delta_args("tc_2", r#"{"generation":0}"#),
            final_item(usage(10, 5)),
        ];

        // Turn 2: model gives up (no submission after NeedsUserInput early return
        // wouldn't happen in practice, but this tests the ordering)
        let turn2 = vec![text_item("done"), final_item(usage(5, 3))];

        let turns = vec![turn1, turn2];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                RunBudget::unlimited(),
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        // NeedsUserInput should take precedence since it's checked before
        // has_submit_evidence in the Done handler.
        assert!(matches!(result, RunTerminalState::NeedsUserInput(_)));
    }

    // ---- C1: token budget bypassed on same-turn Done ----

    #[tokio::test]
    async fn terminal_budget_exhausted_on_same_turn_done() {
        let ctx = test_ctx("src");
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        register_all_tools(&mut reg, &ctx);

        // Single turn with usage exceeding budget, followed by Done.
        // Before the C1 fix, this would return ReadyForReview instead of
        // BudgetExhausted because apply_turn() was called without a
        // subsequent budget check.
        let turn1 = vec![text_item("x"), final_item(usage(200, 5))];

        let turns = vec![turn1];
        let mut turn_idx = 0;
        let model_fn = move |_turn: usize| {
            if turn_idx < turns.len() {
                let items = turns[turn_idx].clone();
                turn_idx += 1;
                Some(items)
            } else {
                None
            }
        };

        let runner = AgentRunner::new(AgentConfig::default());
        let mut session = AgentSession::new(SessionId::new(1));
        let cancel = RunCancellation::new();
        let budget = RunBudget {
            input_tokens: 100,
            ..RunBudget::unlimited()
        };

        let result = runner
            .run(
                AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                    .unwrap(),
                &mut session,
                &reg,
                budget,
                &cancel,
                &NullEventSink,
                &ctx,
                model_fn,
            )
            .await;

        assert!(matches!(
            result,
            RunTerminalState::BudgetExhausted {
                dimension: BudgetDimension::InputTokens
            }
        ));
    }

    // ---- Terminal-tool semantics (§8.3/§4.5) and assistant prose (§5) ----

    mod terminal_semantics {
        use super::*;
        use std::sync::atomic::{AtomicUsize, Ordering};

        fn complete_evidence_turn1() -> Vec<StreamedAssistantContent<MockResponse>> {
            let js = valid_js();
            vec![
                tool_call_delta_name("tc_1", "replace_source"),
                tool_call_delta_args(
                    "tc_1",
                    &serde_json::json!({"source": js, "generation": 0}).to_string(),
                ),
                tool_call_delta_name("tc_2", "validate_source"),
                tool_call_delta_args(
                    "tc_2",
                    &serde_json::json!({"source": js, "generation": 1}).to_string(),
                ),
                tool_call_delta_name("tc_3", "dry_run"),
                tool_call_delta_args(
                    "tc_3",
                    &serde_json::json!({"source": js, "generation": 1}).to_string(),
                ),
                final_item(usage(10, 5)),
            ]
        }

        #[tokio::test]
        async fn submit_terminates_run_without_further_model_turn() {
            // After a successful submit, no further model work may run (§4.5).
            let ctx = test_ctx(valid_js());
            let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
            register_all_tools(&mut reg, &ctx);

            let submit_args = serde_json::json!({"generation": 1}).to_string();
            // A third turn that, if it ran, would replace the source and
            // invalidate the submission.
            let replace2 = serde_json::json!({"source": "different", "generation": 1}).to_string();

            let turns = vec![
                complete_evidence_turn1(),
                vec![
                    tool_call_delta_name("tc_4", "submit_for_review"),
                    tool_call_delta_args("tc_4", &submit_args),
                    final_item(usage(5, 3)),
                ],
                vec![
                    tool_call_delta_name("tc_5", "replace_source"),
                    tool_call_delta_args("tc_5", &replace2),
                    final_item(usage(99, 99)),
                ],
            ];

            let calls = Arc::new(AtomicUsize::new(0));
            let calls_seen = calls.clone();
            let mut turn_idx = 0;
            let model_fn = move |_t: usize| {
                calls_seen.fetch_add(1, Ordering::SeqCst);
                if turn_idx < turns.len() {
                    let it = turns[turn_idx].clone();
                    turn_idx += 1;
                    Some(it)
                } else {
                    None
                }
            };

            let runner = AgentRunner::new(AgentConfig::default());
            let mut session = AgentSession::new(SessionId::new(1));
            let cancel = RunCancellation::new();

            let result = runner
                .run(
                    AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                        .unwrap(),
                    &mut session,
                    &reg,
                    RunBudget::unlimited(),
                    &cancel,
                    &NullEventSink,
                    &ctx,
                    model_fn,
                )
                .await;

            assert!(
                matches!(result, RunTerminalState::ReadyForReview(_)),
                "expected ReadyForReview, got {result:?}"
            );
            assert_eq!(
                calls.load(Ordering::SeqCst),
                2,
                "the model must not be invoked after a successful submit"
            );
        }

        #[tokio::test]
        async fn first_terminal_tool_in_response_order_wins() {
            // submit_for_review before request_user_input in the same batch: the
            // first terminal tool (submit) wins and the rest do not execute.
            let ctx = test_ctx(valid_js());
            let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
            register_all_tools(&mut reg, &ctx);
            reg.register(Arc::new(crate::tools::RequestUserInputTool::new(
                ctx.clone(),
            )))
            .unwrap();

            let submit_args = serde_json::json!({"generation": 1}).to_string();
            let turns = vec![
                complete_evidence_turn1(),
                vec![
                    tool_call_delta_name("tc_a", "submit_for_review"),
                    tool_call_delta_args("tc_a", &submit_args),
                    tool_call_delta_name("tc_b", "request_user_input"),
                    final_item(usage(5, 3)),
                ],
            ];
            let mut turn_idx = 0;
            let model_fn = move |_t: usize| {
                if turn_idx < turns.len() {
                    let it = turns[turn_idx].clone();
                    turn_idx += 1;
                    Some(it)
                } else {
                    None
                }
            };

            let runner = AgentRunner::new(AgentConfig::default());
            let mut session = AgentSession::new(SessionId::new(1));
            let cancel = RunCancellation::new();

            let result = runner
                .run(
                    AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                        .unwrap(),
                    &mut session,
                    &reg,
                    RunBudget::unlimited(),
                    &cancel,
                    &NullEventSink,
                    &ctx,
                    model_fn,
                )
                .await;

            assert!(
                matches!(result, RunTerminalState::ReadyForReview(_)),
                "submit came first, so ReadyForReview must win, got {result:?}"
            );
        }

        #[tokio::test]
        async fn ready_for_review_assistant_text_is_model_prose() {
            // assistant_text must be the model's prose, not the automation source.
            let ctx = test_ctx(valid_js());
            let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
            register_all_tools(&mut reg, &ctx);

            let submit_args = serde_json::json!({"generation": 1}).to_string();
            let turns = vec![
                complete_evidence_turn1(),
                vec![
                    text_item("submitting the redaction now"),
                    tool_call_delta_name("tc_4", "submit_for_review"),
                    tool_call_delta_args("tc_4", &submit_args),
                    final_item(usage(5, 3)),
                ],
            ];
            let mut turn_idx = 0;
            let model_fn = move |_t: usize| {
                if turn_idx < turns.len() {
                    let it = turns[turn_idx].clone();
                    turn_idx += 1;
                    Some(it)
                } else {
                    None
                }
            };

            let runner = AgentRunner::new(AgentConfig::default());
            let mut session = AgentSession::new(SessionId::new(1));
            let cancel = RunCancellation::new();

            let result = runner
                .run(
                    AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                        .unwrap(),
                    &mut session,
                    &reg,
                    RunBudget::unlimited(),
                    &cancel,
                    &NullEventSink,
                    &ctx,
                    model_fn,
                )
                .await;

            match result {
                RunTerminalState::ReadyForReview(r) => {
                    assert_eq!(r.assistant_text, "submitting the redaction now");
                    // the draft source must NOT be used as the assistant message
                    assert_ne!(r.assistant_text, valid_js());
                }
                other => panic!("expected ReadyForReview, got {other:?}"),
            }
            assert_eq!(
                session.exchanges()[0].assistant.text,
                "submitting the redaction now"
            );
        }

        #[tokio::test]
        async fn needs_user_input_assistant_text_is_model_prose() {
            let ctx = test_ctx("automation source here");
            let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
            reg.register(Arc::new(crate::tools::RequestUserInputTool::new(
                ctx.clone(),
            )))
            .unwrap();

            let turns = vec![vec![
                text_item("which region should I redact?"),
                tool_call_delta_name("tc_1", "request_user_input"),
                final_item(usage(5, 3)),
            ]];
            let mut turn_idx = 0;
            let model_fn = move |_t: usize| {
                if turn_idx < turns.len() {
                    let it = turns[turn_idx].clone();
                    turn_idx += 1;
                    Some(it)
                } else {
                    None
                }
            };

            let runner = AgentRunner::new(AgentConfig::default());
            let mut session = AgentSession::new(SessionId::new(1));
            let cancel = RunCancellation::new();

            let result = runner
                .run(
                    AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                        .unwrap(),
                    &mut session,
                    &reg,
                    RunBudget::unlimited(),
                    &cancel,
                    &NullEventSink,
                    &ctx,
                    model_fn,
                )
                .await;

            match result {
                RunTerminalState::NeedsUserInput(n) => {
                    assert_eq!(n.assistant_text, "which region should I redact?");
                    assert_ne!(n.assistant_text, "automation source here");
                }
                other => panic!("expected NeedsUserInput, got {other:?}"),
            }
        }
    }

    // ---- Real-provider path: history + tool schema threading ----

    mod provider_path {
        use super::*;
        use crate::model::{
            ModelCompletion, ModelMessage, ModelRequest, ModelStreamEvent, StopReason,
        };
        use crate::provider::{ProviderAdapter, StreamBounds};
        use std::collections::VecDeque;
        use std::pin::Pin;

        /// Records every `ModelRequest` it receives and replays a scripted set of
        /// stream events per call.
        struct RecordingProvider {
            requests: Mutex<Vec<ModelRequest>>,
            scripts: Mutex<VecDeque<Vec<ModelStreamEvent>>>,
        }

        impl ProviderAdapter for RecordingProvider {
            fn stream(
                &self,
                request: ModelRequest,
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

        fn completion(stop: StopReason) -> ModelStreamEvent {
            ModelStreamEvent::Completed(ModelCompletion {
                usage: crate::model::ModelUsage {
                    input_tokens: 5,
                    output_tokens: 3,
                    total_tokens: 8,
                },
                stop_reason: stop,
            })
        }

        #[tokio::test]
        async fn second_turn_request_carries_history_and_tool_schemas() {
            let ctx = test_ctx("src");
            let inspection = crate::tools::AuthoringInspectionContext {
                payload_mode: "full_screenshot".into(),
                regions: vec![crate::tools::CanonicalRegionInspection {
                    name: "top_strip".into(),
                    bounds: Some(rollshot_image_document::ImageRect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 96.0,
                    }),
                    query: Some(rollshot_automation::RegionFeaturesQuery {
                        region: rollshot_automation::Region::Rect {
                            bounds: rollshot_image_document::ImageRect {
                                x: 0.0,
                                y: 0.0,
                                width: 100.0,
                                height: 96.0,
                            },
                        },
                        limit: 1,
                    }),
                    unavailable_reason: None,
                }],
                ocr_regions: vec![crate::tools::CanonicalOcrInspection {
                    name: "full".into(),
                    bounds: Some(rollshot_image_document::ImageRect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 100.0,
                    }),
                    query: Some(rollshot_automation::OcrQuery {
                        region: rollshot_automation::Region::Full,
                        limit: 50,
                    }),
                    unavailable_reason: None,
                }],
                ocr_status: crate::tools::CapabilityStatus::available(),
                layout_status: crate::tools::CapabilityStatus::unavailable(
                    "capability_unavailable",
                ),
                template_match_status: crate::tools::CapabilityStatus::unavailable(
                    "no_capability_handles",
                ),
            };
            let host = Arc::new(Mutex::new(
                rollshot_automation::FakeAutomationHost::default(),
            ));
            let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
            reg.register(Arc::new(GetContextSummaryTool::new(ctx.clone())))
                .unwrap();
            reg.register(Arc::new(InspectImageContextTool::new(
                ctx.clone(),
                inspection.clone(),
            )))
            .unwrap();
            reg.register(Arc::new(RegionFeaturesTool::new(
                ctx.clone(),
                host.clone(),
                inspection.regions.clone(),
            )))
            .unwrap();
            reg.register(Arc::new(OcrTool::new(
                ctx.clone(),
                host.clone(),
                inspection.ocr_regions.clone(),
            )))
            .unwrap();
            reg.register(Arc::new(ReplaceSourceTool::new(ctx.clone())))
                .unwrap();

            // Turn 1: a tool call. Turn 2: plain text.
            let provider = RecordingProvider {
                requests: Mutex::new(Vec::new()),
                scripts: Mutex::new(VecDeque::from(vec![
                    vec![
                        ModelStreamEvent::ToolCallStart {
                            id: "tc_1".into(),
                            name: "inspect_context_summary".into(),
                        },
                        completion(StopReason::ToolUse),
                    ],
                    vec![
                        ModelStreamEvent::TextDelta("done".into()),
                        completion(StopReason::EndTurn),
                    ],
                ])),
            };

            let runner = AgentRunner::new(AgentConfig::default());
            let mut session = AgentSession::new(SessionId::new(7));
            let cancel = RunCancellation::new();

            let _result = runner
                .run_with_provider(
                    AuthorizedModelInput::new(
                        "anthropic".into(),
                        "claude-sonnet-4-6".into(),
                        "please inspect".into(),
                        vec![],
                        vec![],
                    )
                    .unwrap(),
                    &mut session,
                    &reg,
                    RunBudget::unlimited(),
                    &cancel,
                    &NullEventSink,
                    &ctx,
                    &provider,
                )
                .await;

            let requests = provider.requests.lock().unwrap();
            assert!(
                requests.len() >= 2,
                "expected at least 2 model requests, got {}",
                requests.len()
            );
            assert_eq!(
                requests[0].system_prompt.as_deref(),
                Some(SMART_REDACTION_SYSTEM_PROMPT)
            );
            assert_eq!(
                requests[1].system_prompt.as_deref(),
                Some(SMART_REDACTION_SYSTEM_PROMPT)
            );
            assert!(
                requests[0]
                    .system_prompt
                    .as_deref()
                    .unwrap_or_default()
                    .contains("hide the URL bar"),
                "system prompt should teach common redaction phrasing, got: {:?}",
                requests[0].system_prompt
            );
            assert!(
                requests[0]
                    .system_prompt
                    .as_deref()
                    .unwrap_or_default()
                    .contains("already captured the current screenshot"),
                "system prompt should tell the model the screenshot already exists, got: {:?}",
                requests[0].system_prompt
            );
            assert!(
                requests[0]
                    .system_prompt
                    .as_deref()
                    .unwrap_or_default()
                    .contains("do not ask the user to upload"),
                "system prompt should prevent upload requests, got: {:?}",
                requests[0].system_prompt
            );
            let system_prompt = requests[0].system_prompt.as_deref().unwrap_or_default();
            assert!(
                system_prompt.contains("Rollshot JavaScript authoring guide"),
                "system prompt should include authoring guide marker, got: {:?}",
                requests[0].system_prompt
            );
            assert!(
                system_prompt.contains("function main(input)"),
                "system prompt should document required source shape, got: {:?}",
                requests[0].system_prompt
            );
            assert!(
                system_prompt.contains("rollshot.regionFeatures"),
                "system prompt should document region features API, got: {:?}",
                requests[0].system_prompt
            );
            assert!(
                system_prompt.contains("{ candidates:"),
                "system prompt should document output envelope, got: {:?}",
                requests[0].system_prompt
            );
            assert!(
                system_prompt.contains("validate_source"),
                "system prompt should require validation before submit, got: {:?}",
                requests[0].system_prompt
            );
            assert!(
                system_prompt.contains("dry_run"),
                "system prompt should require dry run before submit, got: {:?}",
                requests[0].system_prompt
            );
            assert!(
                system_prompt.contains("submit_for_review"),
                "system prompt should require review submit, got: {:?}",
                requests[0].system_prompt
            );
            assert!(
                system_prompt
                    .contains("Call inspect_image_context before writing or replacing source"),
                "system prompt should guide inspection before source writing, got: {:?}",
                system_prompt
            );
            assert!(
                system_prompt.contains("Use inspect_region_features with canonical regions"),
                "system prompt should guide region feature inspection, got: {:?}",
                system_prompt
            );
            assert!(
                system_prompt.contains("full, top_strip, left_strip, right_strip, bottom_strip"),
                "system prompt should list canonical region names, got: {:?}",
                system_prompt
            );
            assert!(
                system_prompt.contains("Call inspect_ocr for text-driven redaction requests"),
                "system prompt should guide OCR inspection for text-driven intents, got: {:?}",
                system_prompt
            );
            assert!(
                system_prompt.contains("inspect_ocr returns full recognized text"),
                "system prompt should disclose full OCR text in tool results, got: {:?}",
                system_prompt
            );
            assert!(
                system_prompt.contains("Use only template handles listed by inspect_image_context"),
                "system prompt should require inspected template handles before templateMatch, got: {:?}",
                system_prompt
            );
            assert!(
                system_prompt.contains("Do not invent template handles"),
                "system prompt should forbid invented template handles, got: {:?}",
                system_prompt
            );
            assert!(
                system_prompt.contains("Refer to template handles through input.capabilityHandles"),
                "system prompt should teach alias access for template handles, got: {:?}",
                system_prompt
            );

            // The second request must carry the prior assistant tool call and the
            // tool result so the model can continue the loop.
            let second = &requests[1];
            assert!(
                second.history.iter().any(|m| matches!(
                    m,
                    ModelMessage::AssistantToolCall { name, .. } if name == "inspect_context_summary"
                )),
                "second request history must include the assistant tool call, got: {:?}",
                second.history
            );
            assert!(
                second
                    .history
                    .iter()
                    .any(|m| matches!(m, ModelMessage::ToolResult { .. })),
                "second request history must include the tool result, got: {:?}",
                second.history
            );

            // Tool definitions must carry real JSON schemas, not empty objects.
            let summary_def = second
                .tool_definitions
                .iter()
                .find(|d| d.name == "inspect_context_summary")
                .expect("inspect_context_summary tool definition present");
            assert!(
                summary_def.parameters.get("type").is_some()
                    || summary_def
                        .parameters
                        .as_object()
                        .map(|o| !o.is_empty())
                        .unwrap_or(false),
                "tool definition must carry a real schema, got: {}",
                summary_def.parameters
            );

            let image_context_def = second
                .tool_definitions
                .iter()
                .find(|d| d.name == "inspect_image_context")
                .expect("inspect_image_context tool definition present");
            assert_eq!(
                image_context_def.parameters["type"].as_str(),
                Some("object")
            );

            let region_features_def = second
                .tool_definitions
                .iter()
                .find(|d| d.name == "inspect_region_features")
                .expect("inspect_region_features tool definition present");
            assert_eq!(
                region_features_def.parameters["type"].as_str(),
                Some("object")
            );
            assert!(
                region_features_def
                    .parameters
                    .to_string()
                    .contains("region"),
                "inspect_region_features schema must require a canonical region argument, got: {}",
                region_features_def.parameters
            );

            let ocr_def = second
                .tool_definitions
                .iter()
                .find(|d| d.name == "inspect_ocr")
                .expect("inspect_ocr tool definition present");
            assert_eq!(ocr_def.parameters["type"].as_str(), Some("object"));
            assert!(
                ocr_def.parameters.to_string().contains("region"),
                "inspect_ocr schema must require a canonical region argument, got: {}",
                ocr_def.parameters
            );
        }
    }

    #[test]
    fn smart_redaction_prompt_examples_validate() {
        fn example_source(start_marker: &str, end_marker: &str) -> String {
            let after_start = SMART_REDACTION_SYSTEM_PROMPT
                .split_once(start_marker)
                .unwrap_or_else(|| panic!("missing prompt marker: {start_marker}"))
                .1;
            let example = after_start
                .split_once(end_marker)
                .unwrap_or_else(|| panic!("missing prompt marker: {end_marker}"))
                .0;
            example
                .lines()
                .filter_map(|line| line.strip_prefix("  "))
                .collect::<Vec<_>>()
                .join("\n")
        }

        let limits = rollshot_automation::ValidationLimits::default();
        for source in [
            example_source(
                "- Example redaction from a strip:",
                "- Example OCR redaction when OCR is available:",
            ),
            example_source(
                "- Example OCR redaction when OCR is available:",
                "Authoring loop:",
            ),
        ] {
            rollshot_automation::validate_source(&source, &limits).unwrap_or_else(|diags| {
                panic!("prompt example should validate:\n{source}\n{diags:#?}")
            });
        }
    }

    #[test]
    fn smart_redaction_system_prompt_documents_improve_runs() {
        let system_prompt = SMART_REDACTION_SYSTEM_PROMPT;
        assert!(
            system_prompt.contains("Improve runs"),
            "system prompt should document improve runs, got: {:?}",
            system_prompt
        );
        assert!(
            system_prompt.contains("Treat rejected candidates as false positives"),
            "system prompt should explain rejected correction semantics, got: {:?}",
            system_prompt
        );
        assert!(
            system_prompt.contains("Treat manually added candidates as missed targets"),
            "system prompt should explain manual correction semantics, got: {:?}",
            system_prompt
        );
        assert!(
            system_prompt.contains("Explain what changed in the detector before submit_for_review"),
            "system prompt should require detector-change explanation, got: {:?}",
            system_prompt
        );
    }

    // ---- Resource bounds: cancellation between items ----

    mod resource_bounds {
        use super::*;
        use std::sync::atomic::{AtomicUsize, Ordering};

        /// Event sink that cancels the run after N text chunk events.
        struct CancelAfterNTexts {
            inner: CollectingSink,
            trigger_after: usize,
            count: AtomicUsize,
            cancel: RunCancellation,
        }

        impl CancelAfterNTexts {
            fn new(trigger_after: usize, cancel: RunCancellation) -> Self {
                Self {
                    inner: CollectingSink::new(),
                    trigger_after,
                    count: AtomicUsize::new(0),
                    cancel,
                }
            }

            fn drain(&self) -> Vec<RunEvent> {
                self.inner.drain()
            }
        }

        impl RunEventSink for CancelAfterNTexts {
            fn emit(&self, event: RunEvent) {
                if matches!(&event, RunEvent::TextChunk { .. })
                    && self.count.fetch_add(1, Ordering::SeqCst) == self.trigger_after
                {
                    self.cancel.cancel();
                }
                self.inner.emit(event);
            }
        }

        #[tokio::test]
        async fn cancellation_between_items_stops_early() {
            let ctx = test_ctx("src");
            let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
            register_all_tools(&mut reg, &ctx);

            let cancel = RunCancellation::new();
            // Cancel after the first text chunk
            let sink = CancelAfterNTexts::new(0, cancel.clone());

            let model_fn = |_turn: usize| -> Option<Vec<StreamedAssistantContent<MockResponse>>> {
                Some(vec![
                    text_item("first"),
                    text_item("second"),
                    text_item("third"),
                    final_item(usage(5, 3)),
                ])
            };

            let runner = AgentRunner::new(AgentConfig::default());
            let mut session = AgentSession::new(SessionId::new(1));

            let result = runner
                .run(
                    AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                        .unwrap(),
                    &mut session,
                    &reg,
                    RunBudget::unlimited(),
                    &cancel,
                    &sink,
                    &ctx,
                    model_fn,
                )
                .await;

            assert_eq!(result, RunTerminalState::Cancelled);

            // Verify only 1 text event was emitted — the second and third
            // should have been skipped because cancellation was detected
            // between items.
            let events = sink.drain();
            let text_count = events
                .iter()
                .filter(|e| matches!(e, RunEvent::TextChunk { .. }))
                .count();
            assert_eq!(
                text_count, 1,
                "should stop after cancellation, got {text_count} text events"
            );
        }

        #[tokio::test]
        async fn max_argument_bytes_exceeded() {
            let ctx = test_ctx("src");
            let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
            register_all_tools(&mut reg, &ctx);

            // Create a very long argument string
            let long_arg = "x".repeat(500);
            let replace_args = serde_json::json!({"source": long_arg, "generation": 0}).to_string();

            let turn1 = vec![
                tool_call_delta_name("tc_1", "replace_source"),
                tool_call_delta_args("tc_1", &replace_args),
                final_item(usage(10, 5)),
            ];

            let turns = vec![turn1];
            let mut turn_idx = 0;
            let model_fn = move |_turn: usize| {
                if turn_idx < turns.len() {
                    let items = turns[turn_idx].clone();
                    turn_idx += 1;
                    Some(items)
                } else {
                    None
                }
            };

            // Set max_argument_bytes to 100 — the 500-byte arg should exceed it
            let runner = AgentRunner::new(AgentConfig {
                max_argument_bytes: 100,
                ..AgentConfig::default()
            });
            let mut session = AgentSession::new(SessionId::new(1));
            let cancel = RunCancellation::new();

            let result = runner
                .run(
                    AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                        .unwrap(),
                    &mut session,
                    &reg,
                    RunBudget::unlimited(),
                    &cancel,
                    &NullEventSink,
                    &ctx,
                    model_fn,
                )
                .await;

            assert!(matches!(
                result,
                RunTerminalState::BudgetExhausted {
                    dimension: BudgetDimension::ArgumentBytes
                }
            ));
        }

        #[tokio::test]
        async fn max_result_bytes_exceeded() {
            let ctx = test_ctx("src");

            // Create a tool registry with a tool that returns a large result
            struct BigResultTool;
            impl crate::tools::Tool for BigResultTool {
                fn name(&self) -> &str {
                    "big_result"
                }
                fn json_schema(&self) -> serde_json::Value {
                    crate::tools::tool_schema::<crate::tools::EmptyArgs>()
                }
                fn call<'a>(
                    &'a self,
                    _arguments: &'a serde_json::Value,
                ) -> crate::tools::ToolFuture<'a> {
                    Box::pin(async move {
                        let big = "y".repeat(1000);
                        Ok(crate::tools::ToolOutcome::Success {
                            result_json: serde_json::json!({"data": big}),
                        })
                    })
                }
            }

            let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
            reg.register(Arc::new(BigResultTool)).unwrap();

            let turn1 = vec![
                tool_call_delta_name("tc_1", "big_result"),
                final_item(usage(10, 5)),
            ];

            let turns = vec![turn1];
            let mut turn_idx = 0;
            let model_fn = move |_turn: usize| {
                if turn_idx < turns.len() {
                    let items = turns[turn_idx].clone();
                    turn_idx += 1;
                    Some(items)
                } else {
                    None
                }
            };

            // Set max_result_bytes to 50 — the 1000-byte result should exceed it
            let runner = AgentRunner::new(AgentConfig {
                max_result_bytes: 50,
                ..AgentConfig::default()
            });
            let mut session = AgentSession::new(SessionId::new(1));
            let cancel = RunCancellation::new();

            let result = runner
                .run(
                    AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                        .unwrap(),
                    &mut session,
                    &reg,
                    RunBudget::unlimited(),
                    &cancel,
                    &NullEventSink,
                    &ctx,
                    model_fn,
                )
                .await;

            assert!(matches!(
                result,
                RunTerminalState::BudgetExhausted {
                    dimension: BudgetDimension::ResultBytes
                }
            ));
        }

        // ---- I3: Deadline expiry between stream items ----

        #[tokio::test]
        async fn deadline_between_stream_items() {
            let ctx = test_ctx("src");
            let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
            register_all_tools(&mut reg, &ctx);

            let cancel = RunCancellation::new();

            // First turn: 3 text items — the deadline check between items should fire
            let turn1 = vec![
                text_item("first"),
                text_item("second"),
                text_item("third"),
                final_item(usage(5, 3)),
            ];

            let turns = vec![turn1];
            let mut turn_idx = 0;
            let model_fn = move |_turn: usize| {
                if turn_idx < turns.len() {
                    let items = turns[turn_idx].clone();
                    turn_idx += 1;
                    Some(items)
                } else {
                    None
                }
            };

            let runner = AgentRunner::new(AgentConfig::default());
            let mut session = AgentSession::new(SessionId::new(1));

            // Budget with 1 nanosecond wall time — expires immediately
            let budget = RunBudget {
                wall_time: std::time::Duration::from_nanos(1),
                ..RunBudget::unlimited()
            };

            let result = runner
                .run(
                    AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                        .unwrap(),
                    &mut session,
                    &reg,
                    budget,
                    &cancel,
                    &NullEventSink,
                    &ctx,
                    model_fn,
                )
                .await;

            assert!(matches!(
                result,
                RunTerminalState::BudgetExhausted {
                    dimension: BudgetDimension::WallTime
                }
            ));
        }

        // ---- I3: Provider stream returns None (stream dropped/cancelled) ----

        #[tokio::test]
        async fn provider_stream_returns_none() {
            let ctx = test_ctx("src");
            let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
            register_all_tools(&mut reg, &ctx);

            // Turn 1: tool call (no submit), so the state machine requests a second model turn.
            // Turn 2: model returns None (provider stream ended).
            let turn1 = vec![
                tool_call_delta_name("tc_1", "inspect_context_summary"),
                final_item(usage(5, 3)),
            ];
            let turns = vec![turn1];
            let mut turn_idx = 0;
            let model_fn = move |_turn: usize| {
                if turn_idx < turns.len() {
                    let items = turns[turn_idx].clone();
                    turn_idx += 1;
                    Some(items)
                } else {
                    // Simulates provider stream ending unexpectedly
                    None
                }
            };

            let runner = AgentRunner::new(AgentConfig::default());
            let mut session = AgentSession::new(SessionId::new(1));
            let cancel = RunCancellation::new();

            let result = runner
                .run(
                    AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                        .unwrap(),
                    &mut session,
                    &reg,
                    RunBudget::unlimited(),
                    &cancel,
                    &NullEventSink,
                    &ctx,
                    model_fn,
                )
                .await;

            // The model returning None for the second turn should produce ProviderFailure
            assert!(matches!(result, RunTerminalState::ProviderFailure { .. }));
        }
    }

    // ---- Privacy and QuickJS integration ----

    mod privacy_and_quickjs {
        use super::*;

        #[tokio::test]
        async fn sentinels_not_in_run_events_or_session() {
            let user_sentinel = "USER_SECRET_abc123";
            let source_sentinel = "SOURCE_SENTINEL_xyz789";
            let tool_arg_sentinel = "TOOL_ARG_SENTINEL_ghi789";

            let ctx = test_ctx("initial source");
            let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
            register_all_tools(&mut reg, &ctx);

            // Replace source with a DIFFERENT value than the sentinel
            // The tool_arg_sentinel is in the replace_source args but not as the source value
            let replace_args = serde_json::json!({
                "source": source_sentinel,
                "generation": 0
            })
            .to_string();
            let validate_args = serde_json::json!({
                "source": source_sentinel,
                "generation": 1
            })
            .to_string();
            let submit_args = serde_json::json!({"generation": 1}).to_string();

            let turn1 = vec![
                tool_call_delta_name("tc_1", "replace_source"),
                tool_call_delta_args("tc_1", &replace_args),
                tool_call_delta_name("tc_2", "validate_source"),
                tool_call_delta_args("tc_2", &validate_args),
                tool_call_delta_name("tc_3", "submit_for_review"),
                tool_call_delta_args("tc_3", &submit_args),
                final_item(usage(20, 10)),
            ];
            let turn2 = vec![text_item("done"), final_item(usage(5, 3))];

            let turns = vec![turn1, turn2];
            let mut turn_idx = 0;
            let model_fn = move |_turn: usize| {
                if turn_idx < turns.len() {
                    let items = turns[turn_idx].clone();
                    turn_idx += 1;
                    Some(items)
                } else {
                    None
                }
            };

            let runner = AgentRunner::new(AgentConfig::default());
            let mut session = AgentSession::new(SessionId::new(1));
            let cancel = RunCancellation::new();
            let sink = CollectingSink::new();

            let result = runner
                .run(
                    AuthorizedModelInput::new(
                        "t".into(),
                        "m".into(),
                        user_sentinel.into(),
                        vec![],
                        vec![],
                    )
                    .unwrap(),
                    &mut session,
                    &reg,
                    RunBudget::unlimited(),
                    &cancel,
                    &sink,
                    &ctx,
                    model_fn,
                )
                .await;

            // Check RunEvents: only ToolCallStart (name only), ToolCallEnd (name only),
            // and TextChunk (assistant text, which is allowed)
            let events = sink.drain();
            for event in &events {
                let dbg = format!("{event:?}");
                match event {
                    RunEvent::ToolCallStart { .. } => {
                        assert!(
                            !dbg.contains(tool_arg_sentinel),
                            "ToolCallStart debug must not contain tool arg sentinel: {dbg}"
                        );
                        assert!(
                            !dbg.contains(user_sentinel),
                            "ToolCallStart debug must not contain user sentinel: {dbg}"
                        );
                    }
                    RunEvent::ToolCallEnd { .. } => {
                        assert!(
                            !dbg.contains(tool_arg_sentinel),
                            "ToolCallEnd debug must not contain tool arg sentinel: {dbg}"
                        );
                    }
                    RunEvent::TextChunk { .. } => {
                        // Assistant text may contain source_sentinel by design
                        // (it IS the model's output)
                    }
                    _ => {}
                }
            }

            // Check session: assistant text may contain source_sentinel (it IS the output),
            // but must NOT contain the user_secret or tool_arg_sentinel
            for exchange in session.exchanges() {
                assert!(
                    !exchange.user.text.contains(tool_arg_sentinel),
                    "session user text must not contain tool arg sentinel"
                );
                assert!(
                    !exchange.user.text.contains(source_sentinel),
                    "session user text must not contain source sentinel"
                );
            }

            // Check terminal state Debug
            let result_dbg = format!("{result:?}");
            assert!(
                !result_dbg.contains(user_sentinel),
                "terminal state must not contain user sentinel: {result_dbg}"
            );
        }

        #[tokio::test]
        async fn real_quickjs_produces_valid_proposal() {
            use rollshot_automation_rquickjs::QuickJsExecutor;

            // Source that returns a valid proposal when executed by real QuickJS
            let valid_source = "function main(input) { return {candidates: [{kind: 'addRedaction', bounds: {x: 5, y: 5, width: 20, height: 20}, confidence: 0.85, label: 'email'}]}; }";

            let mut policy = rollshot_automation::ExecutionPolicy::smart_redaction_default(
                std::time::Duration::from_secs(5),
                4 * 1024 * 1024,
                1024 * 1024,
            );
            policy.proposal_limits.max_total_area_fraction = 0.5;

            let cancel = RunCancellation::new();
            let ctx = Arc::new(ToolContext::new(
                SessionId::new(42),
                valid_source.into(),
                rollshot_automation::ValidationLimits::default(),
                policy,
                (100, 100),
                &cancel,
            ));

            let executor = Arc::new(QuickJsExecutor);
            let host = Arc::new(Mutex::new(
                rollshot_automation::FakeAutomationHost::default(),
            ));

            let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
            reg.register(Arc::new(ReplaceSourceTool::new(ctx.clone())))
                .unwrap();
            reg.register(Arc::new(ValidateSourceTool::new(ctx.clone())))
                .unwrap();
            reg.register(Arc::new(DryRunTool::new(ctx.clone(), executor, host)))
                .unwrap();
            reg.register(Arc::new(SubmitForReviewTool::new(ctx.clone())))
                .unwrap();
            reg.register(Arc::new(GetContextSummaryTool::new(ctx.clone())))
                .unwrap();

            let js = valid_source;
            let replace_args = serde_json::json!({"source": js, "generation": 0}).to_string();
            let validate_args = serde_json::json!({"source": js, "generation": 1}).to_string();
            let dry_run_args = serde_json::json!({"source": js, "generation": 1}).to_string();
            let submit_args = serde_json::json!({"generation": 1}).to_string();

            let turn1 = vec![
                tool_call_delta_name("tc_1", "inspect_context_summary"),
                tool_call_delta_name("tc_2", "replace_source"),
                tool_call_delta_args("tc_2", &replace_args),
                final_item(usage(50, 30)),
            ];

            let turn2 = vec![
                tool_call_delta_name("tc_3", "validate_source"),
                tool_call_delta_args("tc_3", &validate_args),
                tool_call_delta_name("tc_4", "dry_run"),
                tool_call_delta_args("tc_4", &dry_run_args),
                tool_call_delta_name("tc_5", "submit_for_review"),
                tool_call_delta_args("tc_5", &submit_args),
                final_item(usage(40, 25)),
            ];

            let turn3 = vec![text_item("workflow ready"), final_item(usage(20, 10))];

            let turns = vec![turn1, turn2, turn3];
            let mut turn_idx = 0;
            let model_fn = move |_turn: usize| {
                if turn_idx < turns.len() {
                    let items = turns[turn_idx].clone();
                    turn_idx += 1;
                    Some(items)
                } else {
                    None
                }
            };

            let runner = AgentRunner::new(AgentConfig::default());
            let mut session = AgentSession::new(SessionId::new(42));

            let result = runner
                .run(
                    AuthorizedModelInput::new(
                        "anthropic".into(),
                        "claude-3".into(),
                        "author a redaction".into(),
                        vec![],
                        vec![],
                    )
                    .unwrap(),
                    &mut session,
                    &reg,
                    RunBudget::unlimited(),
                    &cancel,
                    &NullEventSink,
                    &ctx,
                    model_fn,
                )
                .await;

            // Verify ReadyForReview with correct generation
            match &result {
                RunTerminalState::ReadyForReview(r) => {
                    assert_eq!(r.session_id, SessionId::new(42));
                    assert_eq!(r.generation, 1);
                    assert!(r.usage.input_tokens > 0);
                    assert!(r.usage.output_tokens > 0);
                }
                other => panic!("expected ReadyForReview, got {other:?}"),
            }

            // Verify generation evidence
            let draft = ctx.draft.lock().unwrap();
            assert_eq!(draft.generation(), 1);
            assert!(draft
                .evidence()
                .iter()
                .any(|e| e.kind == EvidenceKind::Validation && e.source_generation == 1));
            assert!(draft
                .evidence()
                .iter()
                .any(|e| e.kind == EvidenceKind::DryRun && e.source_generation == 1));

            // Verify source was replaced
            assert_eq!(*ctx.source.lock().unwrap(), valid_source);

            // Verify session has completed exchange
            assert_eq!(session.exchanges().len(), 1);
            assert_eq!(session.exchanges()[0].user.text, "author a redaction");
        }

        #[tokio::test]
        async fn cancellation_flag_wired_to_executor() {
            use rollshot_automation_rquickjs::QuickJsExecutor;

            // Source with an infinite loop — should be interrupted by cancellation
            let infinite_source =
                "function main(input) { while (true) {} return {candidates: []}; }";

            let mut policy = rollshot_automation::ExecutionPolicy::smart_redaction_default(
                std::time::Duration::from_secs(5),
                4 * 1024 * 1024,
                1024 * 1024,
            );
            policy.proposal_limits.max_total_area_fraction = 1.0;

            let cancel = RunCancellation::new();

            // Pre-validate a valid source, then replace with infinite loop.
            // The tool context binds to the run's single cancellation source.
            let valid_source = "function main(input) { return {candidates: []}; }";
            let ctx = Arc::new(ToolContext::new(
                SessionId::new(1),
                valid_source.into(),
                rollshot_automation::ValidationLimits::default(),
                policy,
                (100, 100),
                &cancel,
            ));

            let executor = Arc::new(QuickJsExecutor);
            let host = Arc::new(Mutex::new(
                rollshot_automation::FakeAutomationHost::default(),
            ));

            let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
            reg.register(Arc::new(ReplaceSourceTool::new(ctx.clone())))
                .unwrap();
            reg.register(Arc::new(ValidateSourceTool::new(ctx.clone())))
                .unwrap();
            reg.register(Arc::new(DryRunTool::new(ctx.clone(), executor, host)))
                .unwrap();

            // Replace source with infinite loop, validate, then dry_run
            let replace_args = serde_json::json!({
                "source": infinite_source,
                "generation": 0
            })
            .to_string();
            let validate_args = serde_json::json!({
                "source": infinite_source,
                "generation": 1
            })
            .to_string();
            let dry_run_args = serde_json::json!({
                "source": infinite_source,
                "generation": 1
            })
            .to_string();

            let turn1 = vec![
                tool_call_delta_name("tc_1", "replace_source"),
                tool_call_delta_args("tc_1", &replace_args),
                tool_call_delta_name("tc_2", "validate_source"),
                tool_call_delta_args("tc_2", &validate_args),
                tool_call_delta_name("tc_3", "dry_run"),
                tool_call_delta_args("tc_3", &dry_run_args),
                final_item(usage(20, 10)),
            ];
            let turn2 = vec![text_item("failed"), final_item(usage(5, 3))];

            let turns = vec![turn1, turn2];
            let cancel_clone = cancel.clone();
            let mut turn_idx = 0;
            let model_fn = move |_turn: usize| {
                turn_idx += 1;
                if turn_idx == 1 {
                    // Cancel before dry_run executes — the executor should see
                    // the cancellation flag and bail out
                    cancel_clone.cancel();
                }
                if turn_idx <= turns.len() {
                    Some(turns[turn_idx - 1].clone())
                } else {
                    None
                }
            };

            let runner = AgentRunner::new(AgentConfig::default());
            let mut session = AgentSession::new(SessionId::new(1));

            let result = runner
                .run(
                    AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                        .unwrap(),
                    &mut session,
                    &reg,
                    RunBudget::unlimited(),
                    &cancel,
                    &NullEventSink,
                    &ctx,
                    model_fn,
                )
                .await;

            // The run should be cancelled because the cancellation flag was set
            // before the tool calls executed
            assert_eq!(result, RunTerminalState::Cancelled);
        }

        // ---- C2/I1: All 6 sentinel positions + tracing subscriber capture ----

        #[tokio::test]
        async fn all_sentinels_not_in_tracing_or_session_or_events() {
            // All 6 sentinel positions
            let user_sentinel = "USER_SECRET_all_positions";
            let source_sentinel = "SOURCE_SENTINEL_all_positions";
            let tool_arg_sentinel = "TOOL_ARG_SENTINEL_all_positions";
            let api_key_sentinel = "API_KEY_SENTINEL_all_positions";
            let attachment_sentinel = "ATTACHMENT_BYTES_SENTINEL_all_positions";
            let provider_meta_sentinel = "PROVIDER_RAW_META_SENTINEL_all_positions";

            // Capturing writer for tracing subscriber
            let log_buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
            let log_buffer_check = log_buffer.clone();

            let subscriber = tracing_subscriber::fmt()
                .with_writer(move || WriteAdaptor {
                    buf: log_buffer.clone(),
                })
                .with_ansi(false)
                .with_target(true)
                .with_max_level(tracing::Level::TRACE)
                .finish();

            let _guard = tracing::subscriber::set_default(subscriber);

            let ctx = test_ctx("initial source");
            let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
            register_all_tools(&mut reg, &ctx);

            let replace_args = serde_json::json!({
                "source": source_sentinel,
                "generation": 0
            })
            .to_string();
            let validate_args = serde_json::json!({
                "source": source_sentinel,
                "generation": 1
            })
            .to_string();
            let submit_args = serde_json::json!({"generation": 1}).to_string();

            let turn1 = vec![
                tool_call_delta_name("tc_1", "replace_source"),
                tool_call_delta_args("tc_1", &replace_args),
                tool_call_delta_name("tc_2", "validate_source"),
                tool_call_delta_args("tc_2", &validate_args),
                tool_call_delta_name("tc_3", "submit_for_review"),
                tool_call_delta_args("tc_3", &submit_args),
                final_item(usage(20, 10)),
            ];
            let turn2 = vec![text_item("done"), final_item(usage(5, 3))];

            let turns = vec![turn1, turn2];
            let mut turn_idx = 0;
            let model_fn = move |_turn: usize| {
                if turn_idx < turns.len() {
                    let items = turns[turn_idx].clone();
                    turn_idx += 1;
                    Some(items)
                } else {
                    None
                }
            };

            let runner = AgentRunner::new(AgentConfig::default());
            let mut session = AgentSession::new(SessionId::new(1));
            let cancel = RunCancellation::new();
            let sink = CollectingSink::new();

            let result = runner
                .run(
                    AuthorizedModelInput::new(
                        "t".into(),
                        "m".into(),
                        user_sentinel.into(),
                        vec![],
                        vec![],
                    )
                    .unwrap(),
                    &mut session,
                    &reg,
                    RunBudget::unlimited(),
                    &cancel,
                    &sink,
                    &ctx,
                    model_fn,
                )
                .await;

            // 1. RunEvent Debug: no tool arg, user, api key, attachment, or provider sentinels
            let events = sink.drain();
            for event in &events {
                let dbg = format!("{event:?}");
                assert!(
                    !dbg.contains(tool_arg_sentinel),
                    "RunEvent debug must not contain tool arg sentinel: {dbg}"
                );
                assert!(
                    !dbg.contains(user_sentinel),
                    "RunEvent debug must not contain user sentinel: {dbg}"
                );
                assert!(
                    !dbg.contains(api_key_sentinel),
                    "RunEvent debug must not contain API key sentinel: {dbg}"
                );
                assert!(
                    !dbg.contains(attachment_sentinel),
                    "RunEvent debug must not contain attachment sentinel: {dbg}"
                );
                assert!(
                    !dbg.contains(provider_meta_sentinel),
                    "RunEvent debug must not contain provider meta sentinel: {dbg}"
                );
            }

            // 2. Terminal state Debug: no user, api key, attachment, or provider sentinels
            let result_dbg = format!("{result:?}");
            for sentinel in &[
                user_sentinel,
                api_key_sentinel,
                attachment_sentinel,
                provider_meta_sentinel,
            ] {
                assert!(
                    !result_dbg.contains(sentinel),
                    "terminal state must not contain sentinel '{sentinel}': {result_dbg}"
                );
            }

            // 3. Session Debug: no tool arg, api key, attachment, or provider sentinels
            // (user message is stored in session by design — that's expected)
            let session_dbg = format!("{session:?}");
            for sentinel in &[
                tool_arg_sentinel,
                api_key_sentinel,
                attachment_sentinel,
                provider_meta_sentinel,
            ] {
                assert!(
                    !session_dbg.contains(sentinel),
                    "session debug must not contain sentinel '{sentinel}': {session_dbg}"
                );
            }

            // 4. Tracing output: no sentinels in captured log
            let log_guard = log_buffer_check.lock().unwrap();
            let logs = String::from_utf8_lossy(&log_guard);
            for sentinel in &[
                user_sentinel,
                source_sentinel,
                tool_arg_sentinel,
                api_key_sentinel,
                attachment_sentinel,
                provider_meta_sentinel,
            ] {
                assert!(
                    !logs.contains(sentinel),
                    "tracing output must not contain sentinel '{sentinel}': {logs}"
                );
            }
        }

        /// Helper: adapts `Arc<Mutex<Vec<u8>>>` to `std::io::Write` for tracing subscriber.
        struct WriteAdaptor {
            buf: Arc<Mutex<Vec<u8>>>,
        }

        impl std::io::Write for WriteAdaptor {
            fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
                self.buf.lock().unwrap().write(data)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        // ---- C1: No double-counting when deltas precede ToolCallComplete ----

        #[tokio::test]
        async fn no_double_counting_argument_bytes_with_deltas() {
            let ctx = test_ctx("src");
            let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
            register_all_tools(&mut reg, &ctx);

            // 50-byte argument via delta, then ToolCallComplete with same content.
            // Budget is 60 bytes — if double-counted, would exceed (50+50=100 > 60).
            let args_50 = "x".repeat(50);
            let turn1 = vec![
                tool_call_delta_name("tc_1", "replace_source"),
                tool_call_delta_args("tc_1", &args_50),
                final_item(usage(10, 5)),
            ];
            let turn2 = vec![text_item("done"), final_item(usage(5, 3))];

            let turns = vec![turn1, turn2];
            let mut turn_idx = 0;
            let model_fn = move |_turn: usize| {
                if turn_idx < turns.len() {
                    let items = turns[turn_idx].clone();
                    turn_idx += 1;
                    Some(items)
                } else {
                    None
                }
            };

            let runner = AgentRunner::new(AgentConfig {
                max_argument_bytes: 60,
                ..AgentConfig::default()
            });
            let mut session = AgentSession::new(SessionId::new(1));
            let cancel = RunCancellation::new();

            let result = runner
                .run(
                    AuthorizedModelInput::new("t".into(), "m".into(), "q".into(), vec![], vec![])
                        .unwrap(),
                    &mut session,
                    &reg,
                    RunBudget::unlimited(),
                    &cancel,
                    &NullEventSink,
                    &ctx,
                    model_fn,
                )
                .await;

            // Should NOT be BudgetExhausted — the 50-byte delta should be counted once.
            // Without the fix, the ToolCallComplete would add 50 more bytes (100 > 60).
            assert!(
                !matches!(
                    &result,
                    RunTerminalState::BudgetExhausted {
                        dimension: BudgetDimension::ArgumentBytes
                    }
                ),
                "should not double-count argument bytes, got: {result:?}"
            );
        }

        // ---- I4: await_provider_progress host-owned bounds ----

        #[tokio::test]
        async fn provider_progress_cancel_wakes_pending_future() {
            let cancellation = RunCancellation::new();
            cancellation.cancel();
            let result = await_provider_progress(
                &cancellation,
                tokio::time::Instant::now() + std::time::Duration::from_secs(30),
                std::future::pending::<()>(),
            )
            .await;
            assert_eq!(result, Err(DriverError::Cancelled));
        }

        #[tokio::test(start_paused = true)]
        async fn provider_progress_deadline_wakes_pending_future() {
            let cancellation = RunCancellation::new();
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
            let future =
                await_provider_progress(&cancellation, deadline, std::future::pending::<()>());
            tokio::pin!(future);
            tokio::time::advance(std::time::Duration::from_secs(10)).await;
            let result = future.await;
            assert_eq!(
                result,
                Err(DriverError::BudgetExhausted(BudgetDimension::WallTime))
            );
        }

        #[tokio::test(start_paused = true)]
        async fn provider_progress_same_poll_tie_prefers_cancel() {
            let cancellation = RunCancellation::new();
            cancellation.cancel();
            let result = await_provider_progress(
                &cancellation,
                tokio::time::Instant::now(),
                std::future::ready(()),
            )
            .await;
            assert_eq!(result, Err(DriverError::Cancelled));
        }
    }
}
