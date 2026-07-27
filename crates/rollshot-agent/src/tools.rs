use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{RunId, SessionId};
use crate::product_task::DocumentContentBinding;
use crate::runtime::{
    DraftState, EvidenceKind, RunCancellation, SourceDiffLine, SourceDiffLineKind,
    SourceDiffSummary,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    #[error("tool call incomplete: missing name or arguments")]
    IncompleteCall,
    #[error("duplicate tool name: {0}")]
    DuplicateName(String),
    #[error("argument decode error: {0}")]
    ArgumentDecode(String),
    #[error("argument byte limit exceeded: {bytes} bytes exceeds {max}")]
    ArgumentBytesExceeded { bytes: usize, max: usize },
    #[error("result byte limit exceeded: {bytes} bytes exceeds {max}")]
    ResultBytesExceeded { bytes: usize, max: usize },
    #[error("per-tool call limit exceeded for {name}: {count} exceeds {max}")]
    PerToolCallLimitExceeded { name: String, count: u32, max: u32 },
    #[error("cancelled before tool call")]
    Cancelled,
    #[error("authority denied for tool `{tool}`: missing operation {operation:?}")]
    AuthorityDenied {
        tool: String,
        operation: crate::authority::RunOperation,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolOutcome {
    Success { result_json: Value },
    Recoverable { error: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub name: String,
    pub arguments_json: Value,
}

pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<ToolOutcome, ToolError>> + Send + 'a>>;

pub trait Tool: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn json_schema(&self) -> Value;
    fn call<'a>(&'a self, arguments: &'a Value) -> ToolFuture<'a>;

    /// The authority operations required to invoke this tool.
    ///
    /// Returns a static slice so tools never allocate per call.
    /// The default is empty — test-only tools and tools that predate
    /// the authority system declare no required operations.
    fn required_operations(&self) -> &'static [crate::authority::RunOperation] {
        &[]
    }
}

#[derive(Debug, Clone)]
pub struct ToolRegistryLimits {
    pub max_argument_bytes: usize,
    pub max_result_bytes: usize,
    pub per_tool_call_limit: u32,
}

impl ToolRegistryLimits {
    pub fn permissive() -> Self {
        Self {
            max_argument_bytes: 256 * 1024,
            max_result_bytes: 256 * 1024,
            per_tool_call_limit: u32::MAX,
        }
    }
}

#[derive(Debug, Default)]
struct CallCounter {
    total: AtomicU32,
    per_tool: Mutex<HashMap<String, u32>>,
}

pub struct ToolRegistry {
    tools: Vec<Arc<dyn Tool>>,
    limits: ToolRegistryLimits,
    counters: CallCounter,
}

impl ToolRegistry {
    pub fn new(limits: ToolRegistryLimits) -> Self {
        Self {
            tools: Vec::new(),
            limits,
            counters: CallCounter::default(),
        }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) -> Result<(), ToolError> {
        if self.tools.iter().any(|t| t.name() == tool.name()) {
            return Err(ToolError::DuplicateName(tool.name().to_string()));
        }
        self.tools.push(tool);
        Ok(())
    }

    pub fn tool_names(&self) -> Vec<&str> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    /// Provider-neutral tool definitions (name + JSON input schema) for the
    /// model request. Descriptions are left to the caller/product layer.
    pub fn tool_definitions(&self) -> Vec<crate::model::ToolDefinition> {
        self.tools
            .iter()
            .map(|t| crate::model::ToolDefinition {
                name: t.name().to_string(),
                description: String::new(),
                parameters: t.json_schema(),
            })
            .collect()
    }

    async fn execute_single(
        &self,
        index: usize,
        call: &ToolCall,
        cancellation: &RunCancellation,
        authority: Option<&crate::authority::AuthoritySnapshot>,
        tool_ctx: Option<&ToolContext>,
    ) -> Result<ToolOutcome, ToolError> {
        if cancellation.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        // Authority check — immediately after cancellation, before counters
        // and before the tool body is entered.
        if let (Some(snapshot), Some(ctx)) = (authority, tool_ctx) {
            let tool = &self.tools[index];
            for &op in tool.required_operations() {
                if let Err(_auth_err) =
                    snapshot.authorize_tool(&ctx.run_id, &ctx.content_binding, op)
                {
                    return Err(ToolError::AuthorityDenied {
                        tool: tool.name().to_string(),
                        operation: op,
                    });
                }
            }
        }

        let tool = &self.tools[index];
        let name = tool.name().to_string();

        {
            let mut per = self.counters.per_tool.lock().unwrap();
            let count = per.entry(name.clone()).or_insert(0);
            *count += 1;
            if *count > self.limits.per_tool_call_limit {
                return Err(ToolError::PerToolCallLimitExceeded {
                    name: name.clone(),
                    count: *count,
                    max: self.limits.per_tool_call_limit,
                });
            }
        }

        let args_bytes = serde_json::to_vec(&call.arguments_json).unwrap_or_default();
        if args_bytes.len() > self.limits.max_argument_bytes {
            return Err(ToolError::ArgumentBytesExceeded {
                bytes: args_bytes.len(),
                max: self.limits.max_argument_bytes,
            });
        }

        self.counters.total.fetch_add(1, Ordering::SeqCst);

        let outcome = match tool.call(&call.arguments_json).await {
            Ok(outcome) => outcome,
            Err(ToolError::ArgumentDecode(msg)) => {
                return Ok(ToolOutcome::Recoverable { error: msg });
            }
            Err(e) => return Err(e),
        };

        let result_bytes = match &outcome {
            ToolOutcome::Success { result_json } => {
                serde_json::to_vec(result_json).unwrap_or_default()
            }
            ToolOutcome::Recoverable { error } => error.as_bytes().to_vec(),
        };
        if result_bytes.len() > self.limits.max_result_bytes {
            return Err(ToolError::ResultBytesExceeded {
                bytes: result_bytes.len(),
                max: self.limits.max_result_bytes,
            });
        }

        Ok(outcome)
    }

    /// Shared serial loop used by both [`execute_calls`] and
    /// [`execute_authorized_calls`]. The `authority` and `tool_ctx` params
    /// are `None` for unauthenticated batches and `Some` for authorized ones.
    async fn execute_serial_loop(
        &self,
        calls: &[ToolCall],
        cancellation: &RunCancellation,
        stop_after_success: &std::collections::BTreeSet<String>,
        authority: Option<&crate::authority::AuthoritySnapshot>,
        tool_ctx: Option<&ToolContext>,
    ) -> Vec<Result<ToolOutcome, ToolError>> {
        let mut results = Vec::with_capacity(calls.len());
        for call in calls {
            let index = match self.tools.iter().position(|t| t.name() == call.name) {
                Some(i) => i,
                None => {
                    results.push(Err(ToolError::UnknownTool(call.name.clone())));
                    break;
                }
            };

            let result = self
                .execute_single(index, call, cancellation, authority, tool_ctx)
                .await;
            let stop = match &result {
                Err(_) => true,
                Ok(ToolOutcome::Success { .. }) => stop_after_success.contains(&call.name),
                Ok(ToolOutcome::Recoverable { .. }) => false,
            };
            results.push(result);

            if stop {
                break;
            }
        }
        results
    }

    /// Execute the batch serially in response order. Stops after a hard error
    /// or after the first successful call whose name is in `stop_after_success`
    /// — used to halt remaining calls once a terminal tool succeeds (§8.3), so
    /// later calls in the same batch never run.
    pub async fn execute_calls(
        &self,
        calls: &[ToolCall],
        cancellation: &RunCancellation,
        stop_after_success: &std::collections::BTreeSet<String>,
    ) -> Vec<Result<ToolOutcome, ToolError>> {
        self.execute_serial_loop(calls, cancellation, stop_after_success, None, None)
            .await
    }

    /// Execute the batch with authority enforcement.
    ///
    /// This is the only authority-bearing entry point. It checks each tool's
    /// [`Tool::required_operations()`] against the provided authority snapshot
    /// before the tool body is entered, before per-tool counters increment.
    ///
    /// Stops after a hard error or after the first successful call whose name
    /// is in `stop_after_success`.
    pub async fn execute_authorized_calls(
        &self,
        calls: &[ToolCall],
        cancellation: &RunCancellation,
        stop_after_success: &std::collections::BTreeSet<String>,
        authority: &crate::authority::AuthoritySnapshot,
        tool_ctx: &ToolContext,
    ) -> Vec<Result<ToolOutcome, ToolError>> {
        self.execute_serial_loop(
            calls,
            cancellation,
            stop_after_success,
            Some(authority),
            Some(tool_ctx),
        )
        .await
    }
}

pub fn tool_schema<T: schemars::JsonSchema>() -> Value {
    let schema = schemars::schema_for!(T);
    serde_json::to_value(schema).unwrap_or_default()
}

// ---------- Authoring tool argument/result types ----------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaceSourceArgs {
    pub source: String,
    pub generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaceSourceResult {
    pub new_generation: u64,
    pub diff: SourceDiffSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEvidenceSummary {
    pub kind: String,
    pub generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadCurrentSourceResult {
    pub generation: u64,
    pub source: String,
    pub source_bytes: usize,
    pub evidence: Vec<SourceEvidenceSummary>,
    pub validation_summary: Option<rollshot_automation::ValidationSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditSourceArgs {
    pub generation: u64,
    pub old: String,
    pub new: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditSourceResult {
    pub new_generation: u64,
    pub diff: SourceDiffSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidateSourceArgs {
    pub source: String,
    pub generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidateSourceResult {
    pub source_bytes: usize,
    pub ast_nodes: u32,
    pub helper_count: u32,
    pub capability_calls: u32,
    pub max_output_candidates: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DryRunArgs {
    pub source: String,
    pub generation: u64,
}

const DRY_RUN_CANDIDATE_PREVIEW_LIMIT: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DryRunCandidatePreview {
    pub kind: String,
    pub bounds: rollshot_image_document::ImageRect,
    pub confidence: f32,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DryRunResult {
    pub candidate_count: u32,
    pub affected_area: f32,
    pub capability_calls: u32,
    pub candidate_preview: Vec<DryRunCandidatePreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubmitForReviewArgs {
    pub generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitForReviewResult {
    pub submitted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestUserInputArgs {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyArgs {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestUserInputResult {
    pub needs_input: bool,
    pub current_generation: u64,
}

const SOURCE_DIFF_CONTEXT_LINES: usize = 2;
const SOURCE_DIFF_MAX_CHANGE_LINES: usize = 40;
const SOURCE_DIFF_MAX_LINE_CHARS: usize = 160;

fn evidence_kind_label(kind: &EvidenceKind) -> &'static str {
    match kind {
        EvidenceKind::Validation => "validation",
        EvidenceKind::Policy => "policy",
        EvidenceKind::DryRun => "dry_run",
    }
}

fn truncate_diff_line(line: &str) -> String {
    let mut out = String::new();
    for (idx, ch) in line.chars().enumerate() {
        if idx >= SOURCE_DIFF_MAX_LINE_CHARS {
            out.push_str(" [truncated]");
            return out;
        }
        out.push(ch);
    }
    out
}

fn push_diff_line(lines: &mut Vec<SourceDiffLine>, kind: SourceDiffLineKind, text: &str) {
    lines.push(SourceDiffLine {
        kind,
        text: truncate_diff_line(text),
    });
}

fn build_source_diff(
    old_source: &str,
    new_source: &str,
    old_generation: u64,
    new_generation: u64,
) -> SourceDiffSummary {
    let old_lines: Vec<&str> = old_source.lines().collect();
    let new_lines: Vec<&str> = new_source.lines().collect();

    let mut prefix = 0;
    while prefix < old_lines.len()
        && prefix < new_lines.len()
        && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }

    let mut suffix = 0;
    while suffix < old_lines.len().saturating_sub(prefix)
        && suffix < new_lines.len().saturating_sub(prefix)
        && old_lines[old_lines.len() - 1 - suffix] == new_lines[new_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let old_change_end = old_lines.len().saturating_sub(suffix);
    let new_change_end = new_lines.len().saturating_sub(suffix);
    let context_start = prefix.saturating_sub(SOURCE_DIFF_CONTEXT_LINES);
    let context_end = old_change_end
        .saturating_add(SOURCE_DIFF_CONTEXT_LINES)
        .min(old_lines.len());

    let mut lines = Vec::new();
    for line in &old_lines[context_start..prefix] {
        push_diff_line(&mut lines, SourceDiffLineKind::Context, line);
    }

    let removed = &old_lines[prefix..old_change_end];
    let added = &new_lines[prefix..new_change_end];
    let changed_total = removed.len().saturating_add(added.len());
    let mut emitted_changes = 0;
    for line in removed {
        if emitted_changes >= SOURCE_DIFF_MAX_CHANGE_LINES {
            break;
        }
        push_diff_line(&mut lines, SourceDiffLineKind::Removed, line);
        emitted_changes += 1;
    }
    for line in added {
        if emitted_changes >= SOURCE_DIFF_MAX_CHANGE_LINES {
            break;
        }
        push_diff_line(&mut lines, SourceDiffLineKind::Added, line);
        emitted_changes += 1;
    }

    let omitted_lines = changed_total.saturating_sub(emitted_changes);
    if omitted_lines > 0 {
        push_diff_line(
            &mut lines,
            SourceDiffLineKind::Omitted,
            &format!("{omitted_lines} changed line(s) omitted"),
        );
    }

    for line in &old_lines[old_change_end..context_end] {
        push_diff_line(&mut lines, SourceDiffLineKind::Context, line);
    }

    SourceDiffSummary {
        old_generation,
        new_generation,
        old_source_bytes: old_source.len(),
        new_source_bytes: new_source.len(),
        omitted_lines,
        lines,
    }
}

fn clear_generation_caches(ctx: &ToolContext) {
    *ctx.last_validated.lock().unwrap() = None;
    *ctx.last_dry_run_proposal.lock().unwrap() = None;
    *ctx.last_dry_run_metrics.lock().unwrap() = None;
    *ctx.last_dry_run_source.lock().unwrap() = None;
    *ctx.pending_ready_for_review.lock().unwrap() = None;
}

// ---------- Inspection types ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSummary {
    pub source_bytes: usize,
    pub generation: u64,
    pub evidence_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityUnavailable {
    pub capability: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AuthoringInspectionContext {
    pub payload_mode: String,
    pub regions: Vec<CanonicalRegionInspection>,
    pub ocr_regions: Vec<CanonicalOcrInspection>,
    pub ocr_status: CapabilityStatus,
    pub layout_status: CapabilityStatus,
    pub template_match_status: CapabilityStatus,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CanonicalRegionInspection {
    pub name: String,
    pub bounds: Option<rollshot_image_document::ImageRect>,
    #[serde(skip_serializing)]
    pub query: Option<rollshot_automation::RegionFeaturesQuery>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CanonicalOcrInspection {
    pub name: String,
    pub bounds: Option<rollshot_image_document::ImageRect>,
    #[serde(skip_serializing)]
    pub query: Option<rollshot_automation::OcrQuery>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CapabilityStatus {
    pub status: String,
    pub reason: Option<String>,
}

impl CapabilityStatus {
    pub fn available() -> Self {
        Self {
            status: "available".into(),
            reason: None,
        }
    }

    pub fn partial(reason: impl Into<String>) -> Self {
        Self {
            status: "partial".into(),
            reason: Some(reason.into()),
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            status: "unavailable".into(),
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageContextImage {
    pub width: u32,
    pub height: u32,
    pub payload_mode: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageContextSource {
    pub generation: u64,
    pub source_bytes: usize,
    pub evidence_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageContextCapabilities {
    pub region_features: CapabilityStatus,
    pub ocr: CapabilityStatus,
    pub layout: CapabilityStatus,
    pub template_match: CapabilityStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct CapabilityHandleSummary {
    pub name: String,
    pub handle: String,
    pub capability: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageContextResult {
    pub image: ImageContextImage,
    pub source: ImageContextSource,
    pub regions: Vec<CanonicalRegionInspection>,
    pub ocr_regions: Vec<CanonicalOcrInspection>,
    pub capability_handles: Vec<CapabilityHandleSummary>,
    pub capabilities: ImageContextCapabilities,
}

// ---------- Tool context ----------

pub struct ToolContext {
    pub draft: Mutex<DraftState>,
    pub source: Mutex<String>,
    pub validation_limits: rollshot_automation::ValidationLimits,
    pub execution_policy: rollshot_automation::ExecutionPolicy,
    pub automation_cancellation: rollshot_automation::CancellationFlag,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub proposal_id: rollshot_edit_proposal::ProposalId,
    pub content_binding: DocumentContentBinding,
    pub image_dims: (u32, u32),
    pub capability_handles: BTreeMap<String, String>,
    pub pending_ready_for_review: Mutex<Option<crate::driver::ReadyForReview>>,
    pub last_validated: Mutex<Option<rollshot_automation::ValidatedAutomation>>,
    pub last_dry_run_proposal: Mutex<Option<rollshot_edit_proposal::EditProposal>>,
    pub last_dry_run_metrics: Mutex<Option<rollshot_automation::ExecutionMetrics>>,
    pub last_dry_run_source: Mutex<Option<String>>,
}

impl ToolContext {
    /// Construct a tool context bound to the run's single cancellation source.
    ///
    /// The dry-run executor observes `cancellation`'s automation flag, so the
    /// same `RunCancellation` passed to [`AgentRunner::run`](crate::driver::AgentRunner)
    /// must be passed here. There is no second, independent cancellation
    /// primitive (§10 / D2).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        proposal_id: rollshot_edit_proposal::ProposalId,
        content_binding: DocumentContentBinding,
        initial_source: String,
        validation_limits: rollshot_automation::ValidationLimits,
        execution_policy: rollshot_automation::ExecutionPolicy,
        image_dims: (u32, u32),
        cancellation: &RunCancellation,
    ) -> Self {
        Self::new_with_capability_handles(
            session_id,
            run_id,
            proposal_id,
            content_binding,
            initial_source,
            validation_limits,
            execution_policy,
            image_dims,
            BTreeMap::new(),
            cancellation,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_capability_handles(
        session_id: SessionId,
        run_id: RunId,
        proposal_id: rollshot_edit_proposal::ProposalId,
        content_binding: DocumentContentBinding,
        initial_source: String,
        validation_limits: rollshot_automation::ValidationLimits,
        execution_policy: rollshot_automation::ExecutionPolicy,
        image_dims: (u32, u32),
        capability_handles: BTreeMap<String, String>,
        cancellation: &RunCancellation,
    ) -> Self {
        Self {
            draft: Mutex::new(DraftState::new(session_id)),
            source: Mutex::new(initial_source),
            validation_limits,
            execution_policy,
            automation_cancellation: cancellation.automation_flag().clone(),
            session_id,
            run_id,
            proposal_id,
            content_binding,
            image_dims,
            capability_handles,
            pending_ready_for_review: Mutex::new(None),
            last_validated: Mutex::new(None),
            last_dry_run_proposal: Mutex::new(None),
            last_dry_run_metrics: Mutex::new(None),
            last_dry_run_source: Mutex::new(None),
        }
    }
}

// ---------- Concrete authoring tools ----------

pub struct ReplaceSourceTool {
    ctx: Arc<ToolContext>,
}

impl ReplaceSourceTool {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

impl Tool for ReplaceSourceTool {
    fn name(&self) -> &str {
        "replace_source"
    }

    fn required_operations(&self) -> &'static [crate::authority::RunOperation] {
        &[crate::authority::RunOperation::WriteDraft]
    }

    fn json_schema(&self) -> Value {
        tool_schema::<ReplaceSourceArgs>()
    }

    fn call<'a>(&'a self, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: ReplaceSourceArgs = serde_json::from_value(arguments.clone())
                .map_err(|e| ToolError::ArgumentDecode(e.to_string()))?;

            let mut draft = self.ctx.draft.lock().unwrap();
            if draft.generation() != args.generation {
                return Err(ToolError::ArgumentDecode(format!(
                    "stale generation: expected {}, got {}",
                    draft.generation(),
                    args.generation
                )));
            }

            draft.invalidate_evidence_after(args.generation);
            let new_gen = draft
                .next_generation()
                .map_err(|e| ToolError::ArgumentDecode(e.to_string()))?;
            drop(draft);

            let old_source = self.ctx.source.lock().unwrap().clone();
            let diff = build_source_diff(&old_source, &args.source, args.generation, new_gen);
            *self.ctx.source.lock().unwrap() = args.source;

            // §4.3: replacement invalidates every validation, dry-run, proposal,
            // and submission result from older generations.
            clear_generation_caches(&self.ctx);

            Ok(ToolOutcome::Success {
                result_json: serde_json::to_value(ReplaceSourceResult {
                    new_generation: new_gen,
                    diff,
                })
                .unwrap_or_default(),
            })
        })
    }
}

pub struct ReadCurrentSourceTool {
    ctx: Arc<ToolContext>,
}

impl ReadCurrentSourceTool {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

impl Tool for ReadCurrentSourceTool {
    fn name(&self) -> &str {
        "read_current_source"
    }

    fn required_operations(&self) -> &'static [crate::authority::RunOperation] {
        &[crate::authority::RunOperation::ReadDraft]
    }

    fn json_schema(&self) -> Value {
        tool_schema::<EmptyArgs>()
    }

    fn call<'a>(&'a self, _arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let draft = self.ctx.draft.lock().unwrap();
            let generation = draft.generation();
            let evidence = draft
                .evidence()
                .iter()
                .rev()
                .take(8)
                .rev()
                .map(|record| SourceEvidenceSummary {
                    kind: evidence_kind_label(&record.kind).into(),
                    generation: record.source_generation,
                })
                .collect();
            drop(draft);

            let source = self.ctx.source.lock().unwrap().clone();
            let validation_summary = self
                .ctx
                .last_validated
                .lock()
                .unwrap()
                .as_ref()
                .filter(|validated| validated.source == source)
                .map(|validated| validated.validation_summary.clone());

            Ok(ToolOutcome::Success {
                result_json: serde_json::to_value(ReadCurrentSourceResult {
                    generation,
                    source_bytes: source.len(),
                    source,
                    evidence,
                    validation_summary,
                })
                .unwrap_or_default(),
            })
        })
    }
}

pub struct EditSourceTool {
    ctx: Arc<ToolContext>,
}

impl EditSourceTool {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

impl Tool for EditSourceTool {
    fn name(&self) -> &str {
        "edit_source"
    }

    fn required_operations(&self) -> &'static [crate::authority::RunOperation] {
        &[
            crate::authority::RunOperation::ReadDraft,
            crate::authority::RunOperation::WriteDraft,
        ]
    }

    fn json_schema(&self) -> Value {
        tool_schema::<EditSourceArgs>()
    }

    fn call<'a>(&'a self, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: EditSourceArgs = serde_json::from_value(arguments.clone())
                .map_err(|e| ToolError::ArgumentDecode(e.to_string()))?;

            let mut draft = self.ctx.draft.lock().unwrap();
            if draft.generation() != args.generation {
                return Err(ToolError::ArgumentDecode(format!(
                    "stale generation: expected {}, got {}",
                    draft.generation(),
                    args.generation
                )));
            }
            if args.old.is_empty() {
                return Ok(ToolOutcome::Recoverable {
                    error: "old text must be non-empty".into(),
                });
            }

            let old_source = self.ctx.source.lock().unwrap().clone();
            let matches = old_source.matches(&args.old).count();
            if matches == 0 {
                return Ok(ToolOutcome::Recoverable {
                    error: "old text not found in current source".into(),
                });
            }
            if matches > 1 {
                return Ok(ToolOutcome::Recoverable {
                    error: format!(
                        "old text matched {matches} ranges; provide a unique exact text"
                    ),
                });
            }

            let new_source = old_source.replacen(&args.old, &args.new, 1);
            draft.invalidate_evidence_after(args.generation);
            let new_gen = draft
                .next_generation()
                .map_err(|e| ToolError::ArgumentDecode(e.to_string()))?;
            drop(draft);

            let diff = build_source_diff(&old_source, &new_source, args.generation, new_gen);
            *self.ctx.source.lock().unwrap() = new_source;
            clear_generation_caches(&self.ctx);

            Ok(ToolOutcome::Success {
                result_json: serde_json::to_value(EditSourceResult {
                    new_generation: new_gen,
                    diff,
                })
                .unwrap_or_default(),
            })
        })
    }
}

pub struct ValidateSourceTool {
    ctx: Arc<ToolContext>,
}

impl ValidateSourceTool {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

impl Tool for ValidateSourceTool {
    fn name(&self) -> &str {
        "validate_source"
    }

    fn required_operations(&self) -> &'static [crate::authority::RunOperation] {
        &[crate::authority::RunOperation::ReadDraft]
    }

    fn json_schema(&self) -> Value {
        tool_schema::<ValidateSourceArgs>()
    }

    fn call<'a>(&'a self, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: ValidateSourceArgs = serde_json::from_value(arguments.clone())
                .map_err(|e| ToolError::ArgumentDecode(e.to_string()))?;

            {
                let draft = self.ctx.draft.lock().unwrap();
                if draft.generation() != args.generation {
                    return Err(ToolError::ArgumentDecode(format!(
                        "stale generation: expected {}, got {}",
                        draft.generation(),
                        args.generation
                    )));
                }
            }

            let validated =
                rollshot_automation::validate_source(&args.source, &self.ctx.validation_limits)
                    .map_err(|diags| {
                        ToolError::ArgumentDecode(serde_json::to_string(&diags).unwrap_or_default())
                    })?;

            let mut draft = self.ctx.draft.lock().unwrap();
            draft
                .record_evidence(
                    EvidenceKind::Validation,
                    args.generation,
                    tokio::time::Instant::now(),
                )
                .map_err(|e| ToolError::ArgumentDecode(e.to_string()))?;
            drop(draft);

            *self.ctx.last_validated.lock().unwrap() = Some(validated.clone());

            Ok(ToolOutcome::Success {
                result_json: serde_json::to_value(ValidateSourceResult {
                    source_bytes: validated.validation_summary.source_bytes,
                    ast_nodes: validated.validation_summary.ast_nodes,
                    helper_count: validated.validation_summary.helper_count,
                    capability_calls: validated.validation_summary.capability_calls,
                    max_output_candidates: validated.validation_summary.max_output_candidates,
                })
                .unwrap_or_default(),
            })
        })
    }
}

pub struct DryRunTool {
    ctx: Arc<ToolContext>,
    executor: Arc<dyn rollshot_automation::AutomationExecutor>,
    host: Arc<Mutex<dyn rollshot_automation::AutomationHost>>,
}

impl DryRunTool {
    pub fn new(
        ctx: Arc<ToolContext>,
        executor: Arc<dyn rollshot_automation::AutomationExecutor>,
        host: Arc<Mutex<dyn rollshot_automation::AutomationHost>>,
    ) -> Self {
        Self {
            ctx,
            executor,
            host,
        }
    }
}

impl Tool for DryRunTool {
    fn name(&self) -> &str {
        "dry_run"
    }

    fn required_operations(&self) -> &'static [crate::authority::RunOperation] {
        &[
            crate::authority::RunOperation::ReadDraft,
            crate::authority::RunOperation::InspectPreparedImage,
            crate::authority::RunOperation::ExecuteRestrictedAutomation,
        ]
    }

    fn json_schema(&self) -> Value {
        tool_schema::<DryRunArgs>()
    }

    fn call<'a>(&'a self, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: DryRunArgs = serde_json::from_value(arguments.clone())
                .map_err(|e| ToolError::ArgumentDecode(e.to_string()))?;

            let generation = {
                let draft = self.ctx.draft.lock().unwrap();
                if draft.generation() != args.generation {
                    return Err(ToolError::ArgumentDecode(format!(
                        "stale generation: expected {}, got {}",
                        draft.generation(),
                        args.generation
                    )));
                }
                args.generation
            };

            let validated =
                rollshot_automation::validate_source(&args.source, &self.ctx.validation_limits)
                    .map_err(|diags| {
                        ToolError::ArgumentDecode(serde_json::to_string(&diags).unwrap_or_default())
                    })?;

            let proposal_ctx = rollshot_automation::ProposalContext {
                proposal_id: self.ctx.proposal_id.clone(),
                base_document_state_id: self.ctx.content_binding.state_id() as u64,
                provenance: rollshot_edit_proposal::Provenance {
                    source: rollshot_edit_proposal::ProvenanceSource::Agent {
                        run_id: self.ctx.run_id.as_str().to_owned(),
                    },
                },
            };

            let input = rollshot_automation::AutomationInput {
                image_width: self.ctx.image_dims.0,
                image_height: self.ctx.image_dims.1,
                region: None,
                annotations: Vec::new(),
                capability_handles: self.ctx.capability_handles.clone(),
            };

            let cancellation = self.ctx.automation_cancellation.clone();

            let mut host_guard = self.host.lock().unwrap();
            let (proposal, metrics) = rollshot_automation::execute_to_proposal(
                self.executor.as_ref(),
                &validated,
                &input,
                &proposal_ctx,
                &mut *host_guard,
                &self.ctx.execution_policy,
                &cancellation,
            )
            .map_err(|e| ToolError::ArgumentDecode(e.to_string()))?;
            drop(host_guard);

            rollshot_edit_proposal::validate_policy(
                &proposal.candidates,
                &self.ctx.execution_policy.proposal_limits,
                self.ctx.image_dims,
            )
            .map_err(|e| ToolError::ArgumentDecode(e.to_string()))?;

            let affected_area: f32 = proposal
                .candidates
                .iter()
                .filter_map(|c| match &c.edit {
                    rollshot_edit_proposal::ProposedEdit::AddRedaction { bounds } => {
                        Some(bounds.width.max(0.0) * bounds.height.max(0.0))
                    }
                    _ => None,
                })
                .sum();

            let candidate_preview: Vec<DryRunCandidatePreview> = proposal
                .candidates
                .iter()
                .filter_map(|candidate| match &candidate.edit {
                    rollshot_edit_proposal::ProposedEdit::AddRedaction { bounds } => {
                        Some(DryRunCandidatePreview {
                            kind: "addRedaction".into(),
                            bounds: *bounds,
                            confidence: candidate.confidence,
                            label: candidate.label.clone(),
                        })
                    }
                    _ => None,
                })
                .take(DRY_RUN_CANDIDATE_PREVIEW_LIMIT)
                .collect();

            let mut draft = self.ctx.draft.lock().unwrap();
            draft
                .record_evidence(
                    EvidenceKind::DryRun,
                    generation,
                    tokio::time::Instant::now(),
                )
                .map_err(|e| ToolError::ArgumentDecode(e.to_string()))?;
            drop(draft);

            let capability_calls = metrics.capability_calls;
            *self.ctx.last_dry_run_proposal.lock().unwrap() = Some(proposal.clone());
            *self.ctx.last_dry_run_metrics.lock().unwrap() = Some(metrics);
            *self.ctx.last_dry_run_source.lock().unwrap() = Some(args.source.clone());

            Ok(ToolOutcome::Success {
                result_json: serde_json::to_value(DryRunResult {
                    candidate_count: proposal.candidates.len() as u32,
                    affected_area,
                    capability_calls,
                    candidate_preview,
                })
                .unwrap_or_default(),
            })
        })
    }
}

pub struct SubmitForReviewTool {
    ctx: Arc<ToolContext>,
}

impl SubmitForReviewTool {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

impl Tool for SubmitForReviewTool {
    fn name(&self) -> &str {
        "submit_for_review"
    }

    fn required_operations(&self) -> &'static [crate::authority::RunOperation] {
        &[
            crate::authority::RunOperation::ReadDraft,
            crate::authority::RunOperation::SubmitReviewCandidate,
        ]
    }

    fn json_schema(&self) -> Value {
        tool_schema::<SubmitForReviewArgs>()
    }

    fn call<'a>(&'a self, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: SubmitForReviewArgs = serde_json::from_value(arguments.clone())
                .map_err(|e| ToolError::ArgumentDecode(e.to_string()))?;

            let draft = self.ctx.draft.lock().unwrap();
            if draft.generation() != args.generation {
                return Err(ToolError::ArgumentDecode(format!(
                    "stale generation: expected {}, got {}",
                    draft.generation(),
                    args.generation
                )));
            }

            // Require BOTH successful validation AND dry-run evidence at this
            // generation (§4.5/§8.4 — submit requires complete current-generation
            // evidence). Either alone is a recoverable error so the model can run
            // the missing step and resubmit.
            let current_gen = draft.generation();
            let has_validation = draft.evidence().iter().any(|e| {
                e.source_generation == current_gen && matches!(e.kind, EvidenceKind::Validation)
            });
            let has_dry_run = draft.evidence().iter().any(|e| {
                e.source_generation == current_gen && matches!(e.kind, EvidenceKind::DryRun)
            });
            if !has_validation || !has_dry_run {
                return Err(ToolError::ArgumentDecode(format!(
                    "incomplete evidence at generation {current_gen}: validation={has_validation}, dry_run={has_dry_run}"
                )));
            }
            drop(draft);

            // Construct the full ReadyForReview from stored intermediate results.
            let source = self.ctx.source.lock().unwrap().clone();
            let validated = self.ctx.last_validated.lock().unwrap().clone();
            let proposal = self.ctx.last_dry_run_proposal.lock().unwrap().clone();
            let dry_run_source = self.ctx.last_dry_run_source.lock().unwrap().clone();

            let source_matches = validated
                .as_ref()
                .map(|validated| validated.source == source)
                .unwrap_or(false)
                && dry_run_source
                    .as_ref()
                    .map(|dry_run_source| dry_run_source == &source)
                    .unwrap_or(false);
            if !source_matches {
                return Err(ToolError::ArgumentDecode(
                    "current source does not match validation and dry-run evidence".into(),
                ));
            }

            if let (Some(validated), Some(proposal)) = (validated, proposal) {
                let dry_run_evidence = crate::driver::DryRunEvidence {
                    candidate_count: proposal.candidates.len() as u32,
                    affected_area: proposal
                        .candidates
                        .iter()
                        .filter_map(|c| match &c.edit {
                            rollshot_edit_proposal::ProposedEdit::AddRedaction { bounds } => {
                                Some(bounds.width.max(0.0) * bounds.height.max(0.0))
                            }
                            _ => None,
                        })
                        .sum(),
                };

                let draft_automation = crate::driver::DraftAutomation {
                    source,
                    validation_summary: validated.validation_summary.clone(),
                    validated,
                    dry_run: dry_run_evidence,
                };

                let ready = crate::driver::ReadyForReview {
                    automation: draft_automation,
                    proposal,
                    budget_usage: crate::runtime::UsageSnapshot::default(),
                    session_id: self.ctx.session_id,
                    assistant_text: String::new(),
                    generation: args.generation,
                    usage: crate::runtime::UsageSnapshot::default(),
                };

                *self.ctx.pending_ready_for_review.lock().unwrap() = Some(ready);
            }

            Ok(ToolOutcome::Success {
                result_json: serde_json::to_value(SubmitForReviewResult { submitted: true })
                    .unwrap_or_default(),
            })
        })
    }
}

pub struct RequestUserInputTool {
    ctx: Arc<ToolContext>,
}

impl RequestUserInputTool {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

impl Tool for RequestUserInputTool {
    fn name(&self) -> &str {
        "request_user_input"
    }

    fn required_operations(&self) -> &'static [crate::authority::RunOperation] {
        &[crate::authority::RunOperation::RequestUserInput]
    }

    fn json_schema(&self) -> Value {
        tool_schema::<RequestUserInputArgs>()
    }

    fn call<'a>(&'a self, _arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let draft = self.ctx.draft.lock().unwrap();
            Ok(ToolOutcome::Success {
                result_json: serde_json::to_value(RequestUserInputResult {
                    needs_input: true,
                    current_generation: draft.generation(),
                })
                .unwrap_or_default(),
            })
        })
    }
}

// ---------- Inspection tools ----------

pub struct InspectImageContextTool {
    ctx: Arc<ToolContext>,
    inspection: AuthoringInspectionContext,
}

impl InspectImageContextTool {
    pub fn new(ctx: Arc<ToolContext>, inspection: AuthoringInspectionContext) -> Self {
        Self { ctx, inspection }
    }
}

impl Tool for InspectImageContextTool {
    fn name(&self) -> &str {
        "inspect_image_context"
    }

    fn required_operations(&self) -> &'static [crate::authority::RunOperation] {
        &[crate::authority::RunOperation::InspectPreparedImage]
    }

    fn json_schema(&self) -> Value {
        tool_schema::<EmptyArgs>()
    }

    fn call<'a>(&'a self, _arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let draft = self.ctx.draft.lock().unwrap();
            let generation = draft.generation();
            let evidence_count = draft.evidence().len();
            drop(draft);

            let source_bytes = self.ctx.source.lock().unwrap().len();
            let prepared = self
                .inspection
                .regions
                .iter()
                .filter(|region| region.query.is_some())
                .count();
            let skipped = self.inspection.regions.len().saturating_sub(prepared);
            let region_features = if prepared == 0 {
                CapabilityStatus::unavailable("no_prepared_regions")
            } else if skipped > 0 {
                CapabilityStatus::partial("some_regions_unavailable")
            } else {
                CapabilityStatus::available()
            };

            let ocr_prepared = self
                .inspection
                .ocr_regions
                .iter()
                .filter(|region| region.query.is_some())
                .count();
            let ocr_skipped = self
                .inspection
                .ocr_regions
                .len()
                .saturating_sub(ocr_prepared);
            let ocr = if self.inspection.ocr_regions.is_empty() {
                self.inspection.ocr_status.clone()
            } else if ocr_prepared == 0 {
                CapabilityStatus::unavailable("no_prepared_ocr_regions")
            } else if ocr_skipped > 0 {
                CapabilityStatus::partial("some_ocr_regions_unavailable")
            } else {
                CapabilityStatus::available()
            };

            let capability_handles: Vec<CapabilityHandleSummary> = self
                .ctx
                .capability_handles
                .iter()
                .take(16)
                .map(|(name, handle)| CapabilityHandleSummary {
                    name: name.clone(),
                    handle: handle.clone(),
                    capability: "template_match".into(),
                })
                .collect();
            let template_match = if self.ctx.capability_handles.is_empty() {
                CapabilityStatus::unavailable("no_capability_handles")
            } else {
                CapabilityStatus::available()
            };

            Ok(ToolOutcome::Success {
                result_json: serde_json::to_value(ImageContextResult {
                    image: ImageContextImage {
                        width: self.ctx.image_dims.0,
                        height: self.ctx.image_dims.1,
                        payload_mode: self.inspection.payload_mode.clone(),
                    },
                    source: ImageContextSource {
                        generation,
                        source_bytes,
                        evidence_count,
                    },
                    regions: self.inspection.regions.clone(),
                    ocr_regions: self.inspection.ocr_regions.clone(),
                    capability_handles,
                    capabilities: ImageContextCapabilities {
                        region_features,
                        ocr,
                        layout: self.inspection.layout_status.clone(),
                        template_match,
                    },
                })
                .unwrap_or_default(),
            })
        })
    }
}

pub struct GetContextSummaryTool {
    ctx: Arc<ToolContext>,
}

impl GetContextSummaryTool {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

impl Tool for GetContextSummaryTool {
    fn name(&self) -> &str {
        "inspect_context_summary"
    }

    fn required_operations(&self) -> &'static [crate::authority::RunOperation] {
        &[crate::authority::RunOperation::ReadDraft]
    }

    fn json_schema(&self) -> Value {
        tool_schema::<EmptyArgs>()
    }

    fn call<'a>(&'a self, _arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let draft = self.ctx.draft.lock().unwrap();
            let source = self.ctx.source.lock().unwrap();
            Ok(ToolOutcome::Success {
                result_json: serde_json::to_value(ContextSummary {
                    source_bytes: source.len(),
                    generation: draft.generation(),
                    evidence_count: draft.evidence().len(),
                })
                .unwrap_or_default(),
            })
        })
    }
}

pub struct OcrTool {
    _ctx: Arc<ToolContext>,
    host: Arc<Mutex<dyn rollshot_automation::AutomationHost>>,
    regions: Vec<CanonicalOcrInspection>,
}

impl OcrTool {
    pub fn new(
        ctx: Arc<ToolContext>,
        host: Arc<Mutex<dyn rollshot_automation::AutomationHost>>,
        regions: Vec<CanonicalOcrInspection>,
    ) -> Self {
        Self {
            _ctx: ctx,
            host,
            regions,
        }
    }
}

impl Tool for OcrTool {
    fn name(&self) -> &str {
        "inspect_ocr"
    }

    fn required_operations(&self) -> &'static [crate::authority::RunOperation] {
        &[crate::authority::RunOperation::InspectPreparedImage]
    }

    fn json_schema(&self) -> Value {
        tool_schema::<InspectRegionFeaturesArgs>()
    }

    fn call<'a>(&'a self, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: InspectRegionFeaturesArgs = serde_json::from_value(arguments.clone())
                .map_err(|e| ToolError::ArgumentDecode(e.to_string()))?;
            let region_name = args.region.as_str();
            let region = self
                .regions
                .iter()
                .find(|region| region.name == region_name)
                .ok_or_else(|| {
                    ToolError::ArgumentDecode(format!(
                        "unknown canonical OCR region: {region_name}"
                    ))
                })?;

            let Some(query) = region.query.clone() else {
                return Ok(ToolOutcome::Success {
                    result_json: serde_json::to_value(InspectOcrResult {
                        region: region.name.clone(),
                        status: "unavailable".into(),
                        bounds: region.bounds,
                        matches: Vec::new(),
                        unavailable_reason: region
                            .unavailable_reason
                            .clone()
                            .or_else(|| Some("ocr_region_unavailable".into())),
                    })
                    .unwrap_or_default(),
                });
            };

            let matches = {
                let mut host = self.host.lock().unwrap();
                host.ocr(query)
            };

            match matches {
                Ok(matches) => {
                    let summaries = matches
                        .into_iter()
                        .map(|m| OcrMatchSummary {
                            bounds: m.bounds,
                            quad: m.quad,
                            text: m.text,
                            confidence: m.confidence,
                        })
                        .collect();
                    Ok(ToolOutcome::Success {
                        result_json: serde_json::to_value(InspectOcrResult {
                            region: region.name.clone(),
                            status: "available".into(),
                            bounds: region.bounds,
                            matches: summaries,
                            unavailable_reason: None,
                        })
                        .unwrap_or_default(),
                    })
                }
                Err(error) => Ok(ToolOutcome::Success {
                    result_json: serde_json::to_value(InspectOcrResult {
                        region: region.name.clone(),
                        status: "unavailable".into(),
                        bounds: region.bounds,
                        matches: Vec::new(),
                        unavailable_reason: Some(capability_error_code(error)),
                    })
                    .unwrap_or_default(),
                }),
            }
        })
    }
}

#[derive(Default)]
pub struct LayoutTool;

impl LayoutTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for LayoutTool {
    fn name(&self) -> &str {
        "inspect_layout"
    }

    fn required_operations(&self) -> &'static [crate::authority::RunOperation] {
        &[crate::authority::RunOperation::InspectPreparedImage]
    }

    fn json_schema(&self) -> Value {
        tool_schema::<EmptyArgs>()
    }

    fn call<'a>(&'a self, _arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            Ok(ToolOutcome::Success {
                result_json: serde_json::to_value(CapabilityUnavailable {
                    capability: "layout".into(),
                    reason: "Layout analysis not available in this context".into(),
                })
                .unwrap_or_default(),
            })
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalRegion {
    Full,
    TopStrip,
    LeftStrip,
    RightStrip,
    BottomStrip,
}

impl CanonicalRegion {
    pub fn as_str(self) -> &'static str {
        match self {
            CanonicalRegion::Full => "full",
            CanonicalRegion::TopStrip => "top_strip",
            CanonicalRegion::LeftStrip => "left_strip",
            CanonicalRegion::RightStrip => "right_strip",
            CanonicalRegion::BottomStrip => "bottom_strip",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectRegionFeaturesArgs {
    pub region: CanonicalRegion,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegionFeatureSummary {
    pub bounds: rollshot_image_document::ImageRect,
    pub dominant_rgba: [u8; 4],
    pub edge_density: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectRegionFeaturesResult {
    pub region: String,
    pub status: String,
    pub bounds: Option<rollshot_image_document::ImageRect>,
    pub features: Vec<RegionFeatureSummary>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OcrMatchSummary {
    pub bounds: rollshot_image_document::ImageRect,
    pub quad: [rollshot_image_document::ImagePoint; 4],
    pub text: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectOcrResult {
    pub region: String,
    pub status: String,
    pub bounds: Option<rollshot_image_document::ImageRect>,
    pub matches: Vec<OcrMatchSummary>,
    pub unavailable_reason: Option<String>,
}

fn capability_error_code(error: rollshot_automation::CapabilityError) -> String {
    match error {
        rollshot_automation::CapabilityError::InvalidInput { code } => code.into(),
        rollshot_automation::CapabilityError::LimitExceeded => "limit_exceeded".into(),
        rollshot_automation::CapabilityError::Failed { code } => code.into(),
    }
}

pub struct RegionFeaturesTool {
    _ctx: Arc<ToolContext>,
    host: Arc<Mutex<dyn rollshot_automation::AutomationHost>>,
    regions: Vec<CanonicalRegionInspection>,
}

impl RegionFeaturesTool {
    pub fn new(
        ctx: Arc<ToolContext>,
        host: Arc<Mutex<dyn rollshot_automation::AutomationHost>>,
        regions: Vec<CanonicalRegionInspection>,
    ) -> Self {
        Self {
            _ctx: ctx,
            host,
            regions,
        }
    }
}

impl Tool for RegionFeaturesTool {
    fn name(&self) -> &str {
        "inspect_region_features"
    }

    fn required_operations(&self) -> &'static [crate::authority::RunOperation] {
        &[crate::authority::RunOperation::InspectPreparedImage]
    }

    fn json_schema(&self) -> Value {
        tool_schema::<InspectRegionFeaturesArgs>()
    }

    fn call<'a>(&'a self, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: InspectRegionFeaturesArgs = serde_json::from_value(arguments.clone())
                .map_err(|e| ToolError::ArgumentDecode(e.to_string()))?;
            let region_name = args.region.as_str();
            let region = self
                .regions
                .iter()
                .find(|region| region.name == region_name)
                .ok_or_else(|| {
                    ToolError::ArgumentDecode(format!("unknown canonical region: {region_name}"))
                })?;

            let Some(query) = region.query.clone() else {
                return Ok(ToolOutcome::Success {
                    result_json: serde_json::to_value(InspectRegionFeaturesResult {
                        region: region.name.clone(),
                        status: "unavailable".into(),
                        bounds: region.bounds,
                        features: Vec::new(),
                        unavailable_reason: region
                            .unavailable_reason
                            .clone()
                            .or_else(|| Some("region_unavailable".into())),
                    })
                    .unwrap_or_default(),
                });
            };

            let features = {
                let mut host = self.host.lock().unwrap();
                host.region_features(query)
            };

            match features {
                Ok(features) => {
                    let summaries = features
                        .into_iter()
                        .take(1)
                        .map(|feature| RegionFeatureSummary {
                            bounds: feature.bounds,
                            dominant_rgba: feature.dominant_rgba,
                            edge_density: feature.edge_density,
                        })
                        .collect();
                    Ok(ToolOutcome::Success {
                        result_json: serde_json::to_value(InspectRegionFeaturesResult {
                            region: region.name.clone(),
                            status: "available".into(),
                            bounds: region.bounds,
                            features: summaries,
                            unavailable_reason: None,
                        })
                        .unwrap_or_default(),
                    })
                }
                Err(error) => Ok(ToolOutcome::Success {
                    result_json: serde_json::to_value(InspectRegionFeaturesResult {
                        region: region.name.clone(),
                        status: "unavailable".into(),
                        bounds: region.bounds,
                        features: Vec::new(),
                        unavailable_reason: Some(capability_error_code(error)),
                    })
                    .unwrap_or_default(),
                }),
            }
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct EchoArgs {
        text: String,
    }

    struct EchoTool;

    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn json_schema(&self) -> Value {
            tool_schema::<EchoArgs>()
        }

        fn call<'a>(&'a self, arguments: &'a Value) -> ToolFuture<'a> {
            Box::pin(async move {
                let _args: EchoArgs = serde_json::from_value(arguments.clone())
                    .map_err(|e| ToolError::ArgumentDecode(e.to_string()))?;
                Ok(ToolOutcome::Success {
                    result_json: arguments.clone(),
                })
            })
        }
    }

    struct FailingTool;

    impl Tool for FailingTool {
        fn name(&self) -> &str {
            "failing"
        }

        fn json_schema(&self) -> Value {
            tool_schema::<EchoArgs>()
        }

        fn call<'a>(&'a self, _arguments: &'a Value) -> ToolFuture<'a> {
            Box::pin(async move { Err(ToolError::UnknownTool("failing".into())) })
        }
    }

    struct OrderTrackerTool {
        name: String,
        counter: Arc<AtomicUsize>,
    }

    impl Tool for OrderTrackerTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn json_schema(&self) -> Value {
            tool_schema::<EchoArgs>()
        }

        fn call<'a>(&'a self, _arguments: &'a Value) -> ToolFuture<'a> {
            let seq = self.counter.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(ToolOutcome::Success {
                    result_json: serde_json::json!({ "sequence": seq }),
                })
            })
        }
    }

    #[test]
    fn duplicate_tool_name_registration_fails() {
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        reg.register(Arc::new(EchoTool)).unwrap();
        let err = reg.register(Arc::new(EchoTool)).unwrap_err();
        assert_eq!(err, ToolError::DuplicateName("echo".into()));
    }

    #[tokio::test]
    async fn unknown_tool_returns_terminal_error() {
        let reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        let cancel = RunCancellation::new();
        let calls = vec![ToolCall {
            name: "nonexistent".into(),
            arguments_json: serde_json::json!({}),
        }];
        let results = reg
            .execute_calls(&calls, &cancel, &std::collections::BTreeSet::new())
            .await;
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0],
            Err(ToolError::UnknownTool("nonexistent".into()))
        );
    }

    #[tokio::test]
    async fn known_tool_malformed_json_returns_recoverable() {
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        reg.register(Arc::new(EchoTool)).unwrap();
        let cancel = RunCancellation::new();
        let calls = vec![ToolCall {
            name: "echo".into(),
            arguments_json: serde_json::json!({"wrong_field": 42}),
        }];
        let results = reg
            .execute_calls(&calls, &cancel, &std::collections::BTreeSet::new())
            .await;
        assert_eq!(results.len(), 1);
        assert!(matches!(&results[0], Ok(ToolOutcome::Recoverable { .. })));
    }

    #[tokio::test]
    async fn unknown_fields_rejected_by_deny_unknown_fields() {
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        reg.register(Arc::new(EchoTool)).unwrap();
        let cancel = RunCancellation::new();
        let calls = vec![ToolCall {
            name: "echo".into(),
            arguments_json: serde_json::json!({"text": "hello", "extra": true}),
        }];
        let results = reg
            .execute_calls(&calls, &cancel, &std::collections::BTreeSet::new())
            .await;
        assert_eq!(results.len(), 1);
        assert!(matches!(&results[0], Ok(ToolOutcome::Recoverable { .. })));
    }

    #[tokio::test]
    async fn argument_byte_limit_exceeded() {
        let limits = ToolRegistryLimits {
            max_argument_bytes: 10,
            max_result_bytes: 256 * 1024,
            per_tool_call_limit: u32::MAX,
        };
        let mut reg = ToolRegistry::new(limits);
        reg.register(Arc::new(EchoTool)).unwrap();
        let cancel = RunCancellation::new();
        let long_text = "x".repeat(100);
        let calls = vec![ToolCall {
            name: "echo".into(),
            arguments_json: serde_json::json!({"text": long_text}),
        }];
        let results = reg
            .execute_calls(&calls, &cancel, &std::collections::BTreeSet::new())
            .await;
        assert_eq!(results.len(), 1);
        assert!(matches!(
            results[0],
            Err(ToolError::ArgumentBytesExceeded { .. })
        ));
    }

    #[tokio::test]
    async fn result_byte_limit_exceeded() {
        struct BigResultTool;

        impl Tool for BigResultTool {
            fn name(&self) -> &str {
                "big_result"
            }

            fn json_schema(&self) -> Value {
                tool_schema::<EchoArgs>()
            }

            fn call<'a>(&'a self, _arguments: &'a Value) -> ToolFuture<'a> {
                Box::pin(async move {
                    let big = "y".repeat(1000);
                    Ok(ToolOutcome::Success {
                        result_json: serde_json::json!({"data": big}),
                    })
                })
            }
        }

        let limits = ToolRegistryLimits {
            max_argument_bytes: 256 * 1024,
            max_result_bytes: 50,
            per_tool_call_limit: u32::MAX,
        };
        let mut reg = ToolRegistry::new(limits);
        reg.register(Arc::new(BigResultTool)).unwrap();
        let cancel = RunCancellation::new();
        let calls = vec![ToolCall {
            name: "big_result".into(),
            arguments_json: serde_json::json!({"text": "ok"}),
        }];
        let results = reg
            .execute_calls(&calls, &cancel, &std::collections::BTreeSet::new())
            .await;
        assert_eq!(results.len(), 1);
        assert!(matches!(
            results[0],
            Err(ToolError::ResultBytesExceeded { .. })
        ));
    }

    #[tokio::test]
    async fn per_tool_call_limit_enforced() {
        let limits = ToolRegistryLimits {
            max_argument_bytes: 256 * 1024,
            max_result_bytes: 256 * 1024,
            per_tool_call_limit: 1,
        };
        let mut reg = ToolRegistry::new(limits);
        reg.register(Arc::new(EchoTool)).unwrap();
        let cancel = RunCancellation::new();

        let calls = vec![ToolCall {
            name: "echo".into(),
            arguments_json: serde_json::json!({"text": "first"}),
        }];
        let results = reg
            .execute_calls(&calls, &cancel, &std::collections::BTreeSet::new())
            .await;
        assert!(results[0].is_ok());

        let calls = vec![ToolCall {
            name: "echo".into(),
            arguments_json: serde_json::json!({"text": "second"}),
        }];
        let results = reg
            .execute_calls(&calls, &cancel, &std::collections::BTreeSet::new())
            .await;
        assert!(matches!(
            results[0],
            Err(ToolError::PerToolCallLimitExceeded { .. })
        ));
    }

    #[tokio::test]
    async fn serial_order_for_multiple_calls() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        reg.register(Arc::new(OrderTrackerTool {
            name: "a".into(),
            counter: counter.clone(),
        }))
        .unwrap();
        reg.register(Arc::new(OrderTrackerTool {
            name: "b".into(),
            counter: counter.clone(),
        }))
        .unwrap();
        let cancel = RunCancellation::new();

        let calls = vec![
            ToolCall {
                name: "a".into(),
                arguments_json: serde_json::json!({"text": "1"}),
            },
            ToolCall {
                name: "b".into(),
                arguments_json: serde_json::json!({"text": "2"}),
            },
            ToolCall {
                name: "a".into(),
                arguments_json: serde_json::json!({"text": "3"}),
            },
        ];
        let results = reg
            .execute_calls(&calls, &cancel, &std::collections::BTreeSet::new())
            .await;
        assert_eq!(results.len(), 3);

        fn seq(outcome: &ToolOutcome) -> u64 {
            match outcome {
                ToolOutcome::Success { result_json } => result_json["sequence"].as_u64().unwrap(),
                _ => panic!("expected success"),
            }
        }

        assert_eq!(seq(results[0].as_ref().unwrap()), 0);
        assert_eq!(seq(results[1].as_ref().unwrap()), 1);
        assert_eq!(seq(results[2].as_ref().unwrap()), 2);
    }

    #[tokio::test]
    async fn terminal_tool_error_stops_later_calls() {
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        reg.register(Arc::new(FailingTool)).unwrap();
        reg.register(Arc::new(EchoTool)).unwrap();
        let cancel = RunCancellation::new();

        let calls = vec![
            ToolCall {
                name: "failing".into(),
                arguments_json: serde_json::json!({"text": "x"}),
            },
            ToolCall {
                name: "echo".into(),
                arguments_json: serde_json::json!({"text": "y"}),
            },
        ];
        let results = reg
            .execute_calls(&calls, &cancel, &std::collections::BTreeSet::new())
            .await;
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[tokio::test]
    async fn cancellation_before_tool_call_stops_execution() {
        let mut reg = ToolRegistry::new(ToolRegistryLimits::permissive());
        reg.register(Arc::new(EchoTool)).unwrap();
        let cancel = RunCancellation::new();
        cancel.cancel();

        let calls = vec![ToolCall {
            name: "echo".into(),
            arguments_json: serde_json::json!({"text": "hello"}),
        }];
        let results = reg
            .execute_calls(&calls, &cancel, &std::collections::BTreeSet::new())
            .await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], Err(ToolError::Cancelled));
    }

    // ---- Authoring test helpers ----

    fn test_context(source: &str) -> Arc<ToolContext> {
        test_context_with_handles(source, std::collections::BTreeMap::new())
    }

    fn test_context_with_handles(
        source: &str,
        capability_handles: std::collections::BTreeMap<String, String>,
    ) -> Arc<ToolContext> {
        use crate::product_task::DocumentContentBinding;
        let mut policy = rollshot_automation::ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(5),
            4 * 1024 * 1024,
            1024 * 1024,
        );
        policy.proposal_limits.max_total_area_fraction = 0.5;
        let binding = DocumentContentBinding::new(
            [1u8; 32],
            &crate::product_task::AnnotationStateV1 {
                width: 100,
                height: 100,
                state_id: 0,
                annotations: vec![],
            },
            0,
        )
        .unwrap();
        Arc::new(ToolContext::new_with_capability_handles(
            SessionId::new(1),
            RunId::parse("run-00000000-0000-4000-8000-000000000001").unwrap(),
            rollshot_edit_proposal::ProposalId::parse(
                "proposal-00000000-0000-4000-8000-000000000001",
            )
            .unwrap(),
            binding,
            source.into(),
            rollshot_automation::ValidationLimits::default(),
            policy,
            (100, 100),
            capability_handles,
            &RunCancellation::new(),
        ))
    }

    fn template_handle_map() -> std::collections::BTreeMap<String, String> {
        std::collections::BTreeMap::from([("logo".into(), "tpl-logo-v1".into())])
    }

    fn valid_js_source() -> &'static str {
        "function main(input) { return [{kind: 'addRedaction', bounds: {x: 0, y: 0, width: 10, height: 10}, confidence: 0.9, label: 'test'}]; }"
    }

    fn alternate_valid_js_source() -> &'static str {
        "function main(input) { return { candidates: [] }; }"
    }

    struct FakeExecutor {
        output_json: String,
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
            }
        }

        fn with_policy_violating_proposal() -> Self {
            // 90x90 = 8100 over 100x100 = 10000 -> 0.81 > 0.5 default area limit
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
            }
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
                    capability_calls: 0,
                    output_bytes: self.output_json.len(),
                    interrupted: false,
                },
            })
        }
    }

    // ---- Cancellation wiring (D2/§10) ----

    #[test]
    fn tool_context_shares_run_cancellation_flag() {
        // The dry-run executor must observe the SAME flag that the run's
        // RunCancellation::cancel() sets — not a second, independent primitive.
        let cancel = RunCancellation::new();
        let policy = rollshot_automation::ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(5),
            4 * 1024 * 1024,
            1024 * 1024,
        );
        let binding = DocumentContentBinding::new(
            [1u8; 32],
            &crate::product_task::AnnotationStateV1 {
                width: 100,
                height: 100,
                state_id: 0,
                annotations: vec![],
            },
            0,
        )
        .unwrap();
        let ctx = ToolContext::new(
            SessionId::new(1),
            RunId::parse("run-00000000-0000-4000-8000-000000000001").unwrap(),
            rollshot_edit_proposal::ProposalId::parse(
                "proposal-00000000-0000-4000-8000-000000000001",
            )
            .unwrap(),
            binding,
            "src".into(),
            rollshot_automation::ValidationLimits::default(),
            policy,
            (100, 100),
            &cancel,
        );
        assert!(!ctx.automation_cancellation.is_cancelled());
        cancel.cancel();
        assert!(
            ctx.automation_cancellation.is_cancelled(),
            "dry-run must observe the run's cancellation flag (single source, D2/§10)"
        );
    }

    // ---- Authoring: replace_source ----

    #[tokio::test]
    async fn replace_source_succeeds_with_matching_generation() {
        let ctx = test_context("old source");
        let tool = ReplaceSourceTool::new(ctx.clone());

        let args = serde_json::json!({"source": "new source", "generation": 0});
        let result = tool.call(&args).await.unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["new_generation"].as_u64(), Some(1));
                assert_eq!(result_json["diff"]["old_generation"].as_u64(), Some(0));
                assert_eq!(result_json["diff"]["new_generation"].as_u64(), Some(1));
            }
            other => panic!("expected success, got {other:?}"),
        }

        assert_eq!(*ctx.source.lock().unwrap(), "new source");
        assert_eq!(ctx.draft.lock().unwrap().generation(), 1);
    }

    #[tokio::test]
    async fn replace_source_invalidates_prior_validation_and_dry_run_caches() {
        // §4.3: replacing the source invalidates every prior validation, dry-run,
        // and proposal result — including the cached payloads the submit handoff
        // reads — not just the evidence records.
        let ctx = test_context(valid_js_source());
        let executor = Arc::new(FakeExecutor::with_valid_proposal());
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));

        // Validate + dry-run at generation 0 to populate the caches.
        ValidateSourceTool::new(ctx.clone())
            .call(&serde_json::json!({"source": valid_js_source(), "generation": 0}))
            .await
            .unwrap();
        DryRunTool::new(ctx.clone(), executor, host)
            .call(&serde_json::json!({"source": valid_js_source(), "generation": 0}))
            .await
            .unwrap();
        assert!(ctx.last_validated.lock().unwrap().is_some());
        assert!(ctx.last_dry_run_proposal.lock().unwrap().is_some());

        // Replace the source — caches from the old generation must be cleared.
        ReplaceSourceTool::new(ctx.clone())
            .call(&serde_json::json!({"source": "new source", "generation": 0}))
            .await
            .unwrap();

        assert!(
            ctx.last_validated.lock().unwrap().is_none(),
            "replace must clear cached validation"
        );
        assert!(
            ctx.last_dry_run_proposal.lock().unwrap().is_none(),
            "replace must clear cached dry-run proposal"
        );
        assert!(
            ctx.last_dry_run_metrics.lock().unwrap().is_none(),
            "replace must clear cached dry-run metrics"
        );
    }

    #[tokio::test]
    async fn replace_source_rejects_stale_generation() {
        let ctx = test_context("source");
        // Bump generation to 1.
        ctx.draft.lock().unwrap().next_generation().unwrap();

        let tool = ReplaceSourceTool::new(ctx);
        let args = serde_json::json!({"source": "new", "generation": 0});
        let err = tool.call(&args).await.unwrap_err();
        assert!(matches!(err, ToolError::ArgumentDecode(_)));
    }

    // ---- Authoring: read_current_source / edit_source ----

    #[tokio::test]
    async fn read_current_source_returns_source_generation_and_evidence() {
        let ctx = test_context(valid_js_source());
        ValidateSourceTool::new(ctx.clone())
            .call(&serde_json::json!({"source": valid_js_source(), "generation": 0}))
            .await
            .unwrap();
        let tool = ReadCurrentSourceTool::new(ctx);

        let result = tool.call(&serde_json::json!({})).await.unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["generation"].as_u64(), Some(0));
                assert_eq!(result_json["source"].as_str(), Some(valid_js_source()));
                assert_eq!(
                    result_json["validation_summary"]["source_bytes"].as_u64(),
                    Some(valid_js_source().len() as u64)
                );
                assert_eq!(
                    result_json["evidence"][0]["kind"].as_str(),
                    Some("validation")
                );
                assert_eq!(result_json["evidence"][0]["generation"].as_u64(), Some(0));
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn edit_source_exact_replace_advances_generation_and_returns_diff() {
        let ctx = test_context("function main(input) {\n  return { candidates: [] };\n}");
        let tool = EditSourceTool::new(ctx.clone());

        let result = tool
            .call(&serde_json::json!({
                "generation": 0,
                "old": "candidates: []",
                "new": "candidates: [{ kind: 'addRedaction', bounds: { x: 0, y: 0, width: 10, height: 10 }, confidence: 0.8, label: 'top' }]"
            }))
            .await
            .unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["new_generation"].as_u64(), Some(1));
                assert_eq!(result_json["diff"]["old_generation"].as_u64(), Some(0));
                assert_eq!(result_json["diff"]["new_generation"].as_u64(), Some(1));
                let lines = result_json["diff"]["lines"].as_array().unwrap();
                assert!(lines.iter().any(|line| line["kind"] == "removed"));
                assert!(lines.iter().any(|line| line["kind"] == "added"));
            }
            other => panic!("expected success, got {other:?}"),
        }

        assert_eq!(ctx.draft.lock().unwrap().generation(), 1);
        assert!(ctx.source.lock().unwrap().contains("kind: 'addRedaction'"));
    }

    #[tokio::test]
    async fn edit_source_rejects_stale_generation_without_mutating_source() {
        let ctx = test_context("source text");
        ctx.draft.lock().unwrap().next_generation().unwrap();
        let tool = EditSourceTool::new(ctx.clone());

        let err = tool
            .call(&serde_json::json!({
                "generation": 0,
                "old": "source",
                "new": "changed"
            }))
            .await
            .unwrap_err();

        assert!(matches!(err, ToolError::ArgumentDecode(_)));
        assert_eq!(*ctx.source.lock().unwrap(), "source text");
        assert_eq!(ctx.draft.lock().unwrap().generation(), 1);
    }

    #[tokio::test]
    async fn edit_source_recovers_when_exact_text_is_missing() {
        let ctx = test_context("source text");
        let tool = EditSourceTool::new(ctx.clone());

        let result = tool
            .call(&serde_json::json!({
                "generation": 0,
                "old": "not present",
                "new": "changed"
            }))
            .await
            .unwrap();

        match result {
            ToolOutcome::Recoverable { error } => {
                assert!(error.contains("not found"));
            }
            other => panic!("expected recoverable mismatch, got {other:?}"),
        }
        assert_eq!(*ctx.source.lock().unwrap(), "source text");
        assert_eq!(ctx.draft.lock().unwrap().generation(), 0);
    }

    // ---- Authoring: validate_source ----

    #[tokio::test]
    async fn validate_source_succeeds_with_valid_js() {
        let ctx = test_context(valid_js_source());
        let tool = ValidateSourceTool::new(ctx.clone());

        let args = serde_json::json!({"source": valid_js_source(), "generation": 0});
        let result = tool.call(&args).await.unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert!(result_json["source_bytes"].as_u64().unwrap() > 0);
                assert!(result_json["ast_nodes"].as_u64().unwrap() > 0);
            }
            other => panic!("expected success, got {other:?}"),
        }

        // Evidence should be recorded.
        assert_eq!(ctx.draft.lock().unwrap().evidence().len(), 1);
    }

    #[tokio::test]
    async fn validate_source_fails_with_invalid_source() {
        let ctx = test_context("invalid {{{");
        let tool = ValidateSourceTool::new(ctx);

        let args = serde_json::json!({"source": "invalid {{{", "generation": 0});
        let err = tool.call(&args).await.unwrap_err();
        assert!(matches!(err, ToolError::ArgumentDecode(_)));
    }

    // ---- Authoring: dry_run ----

    #[tokio::test]
    async fn dry_run_succeeds_with_valid_proposal() {
        let ctx = test_context(valid_js_source());
        let executor = Arc::new(FakeExecutor::with_valid_proposal());
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let tool = DryRunTool::new(ctx.clone(), executor, host);

        let args = serde_json::json!({"source": valid_js_source(), "generation": 0});
        let result = tool.call(&args).await.unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["candidate_count"].as_u64(), Some(1));
                assert!(result_json["affected_area"].as_f64().unwrap() > 0.0);
                let preview = result_json["candidate_preview"].as_array().unwrap();
                assert_eq!(preview.len(), 1);
                assert_eq!(preview[0]["kind"].as_str(), Some("addRedaction"));
                assert_eq!(preview[0]["label"].as_str(), Some("email"));
                assert!((preview[0]["confidence"].as_f64().unwrap() - 0.85).abs() < 1e-6);
                assert_eq!(preview[0]["bounds"]["x"].as_f64(), Some(5.0));
                assert_eq!(preview[0]["bounds"]["y"].as_f64(), Some(5.0));
                assert_eq!(preview[0]["bounds"]["width"].as_f64(), Some(20.0));
                assert_eq!(preview[0]["bounds"]["height"].as_f64(), Some(20.0));
            }
            other => panic!("expected success, got {other:?}"),
        }

        // Evidence should be recorded.
        assert_eq!(ctx.draft.lock().unwrap().evidence().len(), 1);
    }

    #[tokio::test]
    async fn dry_run_uses_run_proposal_and_content_binding() {
        use crate::product_task::DocumentContentBinding;

        let proposal_id = rollshot_edit_proposal::ProposalId::parse(
            "proposal-00000000-0000-4000-8000-000000000042",
        )
        .unwrap();
        let run_id = RunId::parse("run-00000000-0000-4000-8000-000000000042").unwrap();
        let binding = DocumentContentBinding::new(
            [42u8; 32],
            &crate::product_task::AnnotationStateV1 {
                width: 100,
                height: 100,
                state_id: 42,
                annotations: vec![],
            },
            42,
        )
        .unwrap();

        let mut policy = rollshot_automation::ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(5),
            4 * 1024 * 1024,
            1024 * 1024,
        );
        policy.proposal_limits.max_total_area_fraction = 0.5;
        let ctx = Arc::new(ToolContext::new_with_capability_handles(
            SessionId::new(1),
            run_id.clone(),
            proposal_id.clone(),
            binding,
            valid_js_source().into(),
            rollshot_automation::ValidationLimits::default(),
            policy,
            (100, 100),
            BTreeMap::new(),
            &RunCancellation::new(),
        ));

        let executor = Arc::new(FakeExecutor::with_valid_proposal());
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let tool = DryRunTool::new(ctx.clone(), executor, host);

        let args = serde_json::json!({"source": valid_js_source(), "generation": 0});
        let _ = tool.call(&args).await.unwrap();

        let cached = ctx.last_dry_run_proposal.lock().unwrap();
        let proposal = cached.as_ref().expect("proposal cached");
        assert_eq!(proposal.id, proposal_id);
        assert_eq!(proposal.base_document_state_id, 42);
        assert_eq!(
            proposal.provenance.source,
            rollshot_edit_proposal::ProvenanceSource::Agent {
                run_id: run_id.as_str().to_owned(),
            }
        );
    }

    #[tokio::test]
    async fn dry_run_fails_on_policy_violation() {
        let ctx = test_context(valid_js_source());
        let executor = Arc::new(FakeExecutor::with_policy_violating_proposal());
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let tool = DryRunTool::new(ctx, executor, host);

        let args = serde_json::json!({"source": valid_js_source(), "generation": 0});
        let err = tool.call(&args).await.unwrap_err();
        assert!(matches!(err, ToolError::ArgumentDecode(_)));
    }

    #[tokio::test]
    async fn dry_run_rejects_stale_generation() {
        let ctx = test_context(valid_js_source());
        ctx.draft.lock().unwrap().next_generation().unwrap();

        let executor = Arc::new(FakeExecutor::with_valid_proposal());
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let tool = DryRunTool::new(ctx, executor, host);

        let args = serde_json::json!({"source": valid_js_source(), "generation": 0});
        let err = tool.call(&args).await.unwrap_err();
        assert!(matches!(err, ToolError::ArgumentDecode(_)));
    }

    // ---- Authoring: submit_for_review ----

    #[tokio::test]
    async fn submit_for_review_succeeds_with_validation_and_dry_run_evidence() {
        let ctx = test_context(valid_js_source());
        let executor = Arc::new(FakeExecutor::with_valid_proposal());
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));

        ValidateSourceTool::new(ctx.clone())
            .call(&serde_json::json!({"source": valid_js_source(), "generation": 0}))
            .await
            .unwrap();
        DryRunTool::new(ctx.clone(), executor, host)
            .call(&serde_json::json!({"source": valid_js_source(), "generation": 0}))
            .await
            .unwrap();

        let tool = SubmitForReviewTool::new(ctx.clone());
        let args = serde_json::json!({"generation": 0});
        let result = tool.call(&args).await.unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["submitted"].as_bool(), Some(true));
            }
            other => panic!("expected success, got {other:?}"),
        }
        assert!(
            ctx.pending_ready_for_review.lock().unwrap().is_some(),
            "submit should create the review handoff from current-source evidence"
        );
    }

    #[tokio::test]
    async fn submit_for_review_rejects_evidence_for_non_current_source() {
        let ctx = test_context(valid_js_source());
        let executor = Arc::new(FakeExecutor::with_valid_proposal());
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));

        ValidateSourceTool::new(ctx.clone())
            .call(&serde_json::json!({
                "source": alternate_valid_js_source(),
                "generation": 0
            }))
            .await
            .unwrap();
        DryRunTool::new(ctx.clone(), executor, host)
            .call(&serde_json::json!({
                "source": alternate_valid_js_source(),
                "generation": 0
            }))
            .await
            .unwrap();

        let err = SubmitForReviewTool::new(ctx.clone())
            .call(&serde_json::json!({"generation": 0}))
            .await
            .unwrap_err();

        assert!(matches!(err, ToolError::ArgumentDecode(_)));
        assert!(
            ctx.pending_ready_for_review.lock().unwrap().is_none(),
            "submit must not create a review handoff from stale source evidence"
        );
    }

    #[tokio::test]
    async fn submit_for_review_fails_with_only_validation_evidence() {
        // Validation alone is not enough — a successful dry-run is also required
        // (§4.5/§8.4). Without it, submit must return a recoverable error so the
        // model can run dry_run and resubmit, rather than producing a bogus
        // ReadyForReview.
        let ctx = test_context(valid_js_source());
        ctx.draft
            .lock()
            .unwrap()
            .record_evidence(EvidenceKind::Validation, 0, tokio::time::Instant::now())
            .unwrap();

        let tool = SubmitForReviewTool::new(ctx);
        let args = serde_json::json!({"generation": 0});
        let err = tool.call(&args).await.unwrap_err();
        assert!(matches!(err, ToolError::ArgumentDecode(_)));
    }

    #[tokio::test]
    async fn submit_for_review_fails_with_only_dry_run_evidence() {
        let ctx = test_context(valid_js_source());
        ctx.draft
            .lock()
            .unwrap()
            .record_evidence(EvidenceKind::DryRun, 0, tokio::time::Instant::now())
            .unwrap();

        let tool = SubmitForReviewTool::new(ctx);
        let args = serde_json::json!({"generation": 0});
        let err = tool.call(&args).await.unwrap_err();
        assert!(matches!(err, ToolError::ArgumentDecode(_)));
    }

    #[tokio::test]
    async fn submit_for_review_fails_without_prior_evidence() {
        let ctx = test_context(valid_js_source());
        let tool = SubmitForReviewTool::new(ctx);

        let args = serde_json::json!({"generation": 0});
        let err = tool.call(&args).await.unwrap_err();
        assert!(matches!(err, ToolError::ArgumentDecode(_)));
    }

    // ---- Authoring: request_user_input ----

    #[tokio::test]
    async fn request_user_input_returns_needs_input() {
        let ctx = test_context("source");
        let tool = RequestUserInputTool::new(ctx);

        let args = serde_json::json!({});
        let result = tool.call(&args).await.unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["needs_input"].as_bool(), Some(true));
                assert_eq!(result_json["current_generation"].as_u64(), Some(0));
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    // ---- Inspection: available ----

    #[tokio::test]
    async fn get_context_summary_returns_draft_info() {
        let ctx = test_context("hello world");
        let tool = GetContextSummaryTool::new(ctx);

        let args = serde_json::json!({});
        let result = tool.call(&args).await.unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["source_bytes"].as_u64(), Some(11));
                assert_eq!(result_json["generation"].as_u64(), Some(0));
                assert_eq!(result_json["evidence_count"].as_u64(), Some(0));
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    // ---- Inspection: OCR ----

    #[test]
    fn inspect_ocr_schema_is_object() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let tool = OcrTool::new(ctx, host, inspection_context_for_tests().ocr_regions);
        let schema = tool.json_schema();
        assert_eq!(schema["type"].as_str(), Some("object"));
    }

    #[test]
    fn inspect_ocr_schema_advertises_canonical_regions() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let tool = OcrTool::new(ctx, host, inspection_context_for_tests().ocr_regions);
        let schema = tool.json_schema().to_string();
        for name in [
            "full",
            "top_strip",
            "left_strip",
            "right_strip",
            "bottom_strip",
        ] {
            assert!(
                schema.contains(name),
                "schema should advertise canonical OCR region {name}, got: {schema}"
            );
        }
    }

    #[tokio::test]
    async fn inspect_ocr_rejects_unknown_region() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let tool = OcrTool::new(ctx, host, inspection_context_for_tests().ocr_regions);

        let err = tool
            .call(&serde_json::json!({"region": "custom_rect"}))
            .await
            .unwrap_err();

        assert!(matches!(err, ToolError::ArgumentDecode(_)));
    }

    #[tokio::test]
    async fn inspect_ocr_returns_full_text_bounds_and_confidence() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(rollshot_automation::FakeAutomationHost {
            ocr_results: vec![rollshot_automation::OcrMatch {
                bounds: rollshot_image_document::ImageRect {
                    x: 10.0,
                    y: 20.0,
                    width: 120.0,
                    height: 24.0,
                },
                quad: [
                    rollshot_image_document::ImagePoint { x: 10.0, y: 20.0 },
                    rollshot_image_document::ImagePoint { x: 130.0, y: 20.0 },
                    rollshot_image_document::ImagePoint { x: 130.0, y: 44.0 },
                    rollshot_image_document::ImagePoint { x: 10.0, y: 44.0 },
                ],
                text: "alice@example.com".into(),
                confidence: 0.92,
            }],
            ..Default::default()
        }));
        let tool = OcrTool::new(ctx, host, inspection_context_for_tests().ocr_regions);

        let result = tool
            .call(&serde_json::json!({"region": "full"}))
            .await
            .unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["region"].as_str(), Some("full"));
                assert_eq!(result_json["status"].as_str(), Some("available"));
                assert_eq!(result_json["matches"].as_array().unwrap().len(), 1);
                assert_eq!(
                    result_json["matches"][0]["text"].as_str(),
                    Some("alice@example.com")
                );
                assert_eq!(
                    result_json["matches"][0]["bounds"]["x"].as_f64(),
                    Some(10.0)
                );
                assert_eq!(
                    result_json["matches"][0]["confidence"].as_f64(),
                    Some(0.9200000166893005)
                );
                assert!(result_json["unavailable_reason"].is_null());
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn inspect_ocr_returns_unavailable_for_skipped_region() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let regions = vec![CanonicalOcrInspection {
            name: "full".into(),
            bounds: Some(rollshot_image_document::ImageRect {
                x: 0.0,
                y: 0.0,
                width: 100_000.0,
                height: 100_000.0,
            }),
            query: None,
            unavailable_reason: Some("area_limit_exceeded".into()),
        }];
        let tool = OcrTool::new(ctx, host, regions);

        let result = tool
            .call(&serde_json::json!({"region": "full"}))
            .await
            .unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["status"].as_str(), Some("unavailable"));
                assert_eq!(
                    result_json["unavailable_reason"].as_str(),
                    Some("area_limit_exceeded")
                );
                assert!(result_json["matches"].as_array().unwrap().is_empty());
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn inspect_ocr_converts_host_error_to_unavailable() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(rollshot_automation::FakeAutomationHost {
            failure: Some(rollshot_automation::CapabilityError::Failed {
                code: "vision_index_unavailable",
            }),
            ..Default::default()
        }));
        let tool = OcrTool::new(ctx, host, inspection_context_for_tests().ocr_regions);

        let result = tool
            .call(&serde_json::json!({"region": "full"}))
            .await
            .unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["status"].as_str(), Some("unavailable"));
                assert_eq!(
                    result_json["unavailable_reason"].as_str(),
                    Some("vision_index_unavailable")
                );
                assert!(result_json["matches"].as_array().unwrap().is_empty());
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn layout_returns_unavailable() {
        let tool = LayoutTool::new();
        let args = serde_json::json!({});
        let result = tool.call(&args).await.unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["capability"].as_str(), Some("layout"));
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[test]
    fn inspect_region_features_schema_is_object() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let tool = RegionFeaturesTool::new(ctx, host, inspection_context_for_tests().regions);
        let schema = tool.json_schema();
        assert_eq!(schema["type"].as_str(), Some("object"));
    }

    #[test]
    fn inspect_region_features_schema_advertises_canonical_regions() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let tool = RegionFeaturesTool::new(ctx, host, inspection_context_for_tests().regions);
        let schema = tool.json_schema().to_string();
        for name in [
            "full",
            "top_strip",
            "left_strip",
            "right_strip",
            "bottom_strip",
        ] {
            assert!(
                schema.contains(name),
                "schema should advertise canonical region {name}, got: {schema}"
            );
        }
    }

    #[tokio::test]
    async fn inspect_region_features_rejects_unknown_region() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let tool = RegionFeaturesTool::new(ctx, host, inspection_context_for_tests().regions);

        let err = tool
            .call(&serde_json::json!({"region": "custom_rect"}))
            .await
            .unwrap_err();

        assert!(matches!(err, ToolError::ArgumentDecode(_)));
    }

    #[tokio::test]
    async fn inspect_region_features_returns_prepared_feature_summary() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(rollshot_automation::FakeAutomationHost {
            region_feature_results: vec![rollshot_automation::RegionFeatures {
                bounds: rollshot_image_document::ImageRect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 96.0,
                },
                dominant_rgba: [10, 20, 30, 255],
                edge_density: 0.25,
            }],
            ..Default::default()
        }));
        let tool = RegionFeaturesTool::new(ctx, host, inspection_context_for_tests().regions);

        let result = tool
            .call(&serde_json::json!({"region": "top_strip"}))
            .await
            .unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["region"].as_str(), Some("top_strip"));
                assert_eq!(result_json["status"].as_str(), Some("available"));
                assert_eq!(result_json["features"].as_array().unwrap().len(), 1);
                assert_eq!(
                    result_json["features"][0]["dominant_rgba"][0].as_u64(),
                    Some(10)
                );
                assert_eq!(
                    result_json["features"][0]["edge_density"].as_f64(),
                    Some(0.25)
                );
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn inspect_region_features_returns_unavailable_for_skipped_region() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let regions = vec![CanonicalRegionInspection {
            name: "full".into(),
            bounds: Some(rollshot_image_document::ImageRect {
                x: 0.0,
                y: 0.0,
                width: 100_000.0,
                height: 100_000.0,
            }),
            query: None,
            unavailable_reason: Some("area_limit_exceeded".into()),
        }];
        let tool = RegionFeaturesTool::new(ctx, host, regions);

        let result = tool
            .call(&serde_json::json!({"region": "full"}))
            .await
            .unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["status"].as_str(), Some("unavailable"));
                assert_eq!(
                    result_json["unavailable_reason"].as_str(),
                    Some("area_limit_exceeded")
                );
                assert!(result_json["features"].as_array().unwrap().is_empty());
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn inspect_region_features_converts_host_error_to_unavailable() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(rollshot_automation::FakeAutomationHost {
            failure: Some(rollshot_automation::CapabilityError::Failed {
                code: "vision_index_unavailable",
            }),
            ..Default::default()
        }));
        let tool = RegionFeaturesTool::new(ctx, host, inspection_context_for_tests().regions);

        let result = tool
            .call(&serde_json::json!({"region": "top_strip"}))
            .await
            .unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["status"].as_str(), Some("unavailable"));
                assert_eq!(
                    result_json["unavailable_reason"].as_str(),
                    Some("vision_index_unavailable")
                );
                assert!(result_json["features"].as_array().unwrap().is_empty());
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    // ---- Inspection: image context ----

    fn inspection_context_for_tests() -> AuthoringInspectionContext {
        AuthoringInspectionContext {
            payload_mode: "full_screenshot".into(),
            regions: vec![CanonicalRegionInspection {
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
            ocr_regions: vec![CanonicalOcrInspection {
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
            ocr_status: CapabilityStatus::available(),
            layout_status: CapabilityStatus::unavailable("capability_unavailable"),
            template_match_status: CapabilityStatus::unavailable("no_capability_handles"),
        }
    }

    #[test]
    fn inspect_image_context_schema_is_object() {
        let tool =
            InspectImageContextTool::new(test_context("source"), inspection_context_for_tests());
        let schema = tool.json_schema();
        assert_eq!(schema["type"].as_str(), Some("object"));
    }

    #[tokio::test]
    async fn inspect_image_context_returns_authoring_and_region_context() {
        let ctx = test_context("hello world");
        let tool = InspectImageContextTool::new(ctx, inspection_context_for_tests());

        let result = tool.call(&serde_json::json!({})).await.unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["image"]["width"].as_u64(), Some(100));
                assert_eq!(result_json["image"]["height"].as_u64(), Some(100));
                assert_eq!(
                    result_json["image"]["payload_mode"].as_str(),
                    Some("full_screenshot")
                );
                assert_eq!(result_json["source"]["generation"].as_u64(), Some(0));
                assert_eq!(result_json["source"]["source_bytes"].as_u64(), Some(11));
                assert_eq!(
                    result_json["regions"][0]["name"].as_str(),
                    Some("top_strip")
                );
                assert!(result_json["regions"][0].get("query").is_none());
                assert_eq!(
                    result_json["capabilities"]["region_features"]["status"].as_str(),
                    Some("available")
                );
                assert_eq!(
                    result_json["capabilities"]["ocr"]["status"].as_str(),
                    Some("available")
                );
                assert_eq!(
                    result_json["capabilities"]["layout"]["status"].as_str(),
                    Some("unavailable")
                );
                assert_eq!(
                    result_json["capabilities"]["template_match"]["status"].as_str(),
                    Some("unavailable")
                );
                assert_eq!(result_json["ocr_regions"][0]["name"].as_str(), Some("full"));
                assert!(result_json["ocr_regions"][0].get("query").is_none());
                assert_eq!(
                    result_json["capabilities"]["ocr"]["status"].as_str(),
                    Some("available")
                );
                assert!(result_json["capability_handles"]
                    .as_array()
                    .unwrap()
                    .is_empty());
                assert_eq!(
                    result_json["capabilities"]["template_match"]["reason"].as_str(),
                    Some("no_capability_handles")
                );
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn inspect_image_context_exposes_existing_capability_handles() {
        let ctx = test_context_with_handles("source", template_handle_map());
        let tool = InspectImageContextTool::new(ctx, inspection_context_for_tests());

        let result = tool.call(&serde_json::json!({})).await.unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(
                    result_json["capability_handles"][0]["name"].as_str(),
                    Some("logo")
                );
                assert_eq!(
                    result_json["capability_handles"][0]["handle"].as_str(),
                    Some("tpl-logo-v1")
                );
                assert_eq!(
                    result_json["capability_handles"][0]["capability"].as_str(),
                    Some("template_match")
                );
                assert_eq!(
                    result_json["capabilities"]["template_match"]["status"].as_str(),
                    Some("available")
                );
                assert!(result_json["capabilities"]["template_match"]["reason"].is_null());
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn inspect_image_context_bounds_capability_handle_summaries() {
        let handles = (0..20)
            .map(|i| (format!("handle-{i:02}"), format!("tpl-{i:02}")))
            .collect();
        let ctx = test_context_with_handles("source", handles);
        let tool = InspectImageContextTool::new(ctx, inspection_context_for_tests());

        let result = tool.call(&serde_json::json!({})).await.unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                let handles = result_json["capability_handles"].as_array().unwrap();
                assert_eq!(handles.len(), 16);
                assert_eq!(handles[0]["name"].as_str(), Some("handle-00"));
                assert_eq!(handles[15]["name"].as_str(), Some("handle-15"));
                assert_eq!(
                    result_json["capabilities"]["template_match"]["status"].as_str(),
                    Some("available")
                );
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    // ---- Affected area budget ----

    #[tokio::test]
    async fn dry_run_reports_affected_area_from_redactions() {
        let ctx = test_context(valid_js_source());
        // Proposal with 20x20 redaction -> area = 400
        let executor = Arc::new(FakeExecutor::with_valid_proposal());
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let tool = DryRunTool::new(ctx, executor, host);

        let args = serde_json::json!({"source": valid_js_source(), "generation": 0});
        let result = tool.call(&args).await.unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                let area = result_json["affected_area"].as_f64().unwrap() as f32;
                assert!(area > 0.0, "affected area should be positive");
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dry_run_exposes_capability_handles_to_javascript_input() {
        let ctx = test_context_with_handles("source", template_handle_map());
        let tool = DryRunTool::new(
            ctx,
            Arc::new(rollshot_automation_rquickjs::QuickJsExecutor),
            Arc::new(Mutex::new(
                rollshot_automation::FakeAutomationHost::default(),
            )),
        );

        let source = r#"
function main(input) {
  return input.capabilityHandles.logo === "tpl-logo-v1"
    ? { candidates: [{ kind: "addRedaction", bounds: { x: 0, y: 0, width: 10, height: 10 }, confidence: 0.9, label: "handle-visible" }] }
    : { candidates: [] };
}
"#;
        let result = tool
            .call(&serde_json::json!({"source": source, "generation": 0}))
            .await
            .unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(
                    result_json["candidate_count"].as_u64(),
                    Some(1),
                    "expected JavaScript to see input.capabilityHandles.logo"
                );
            }
            other => panic!("expected dry-run success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dry_run_candidate_preview_is_capped() {
        let ctx = test_context(valid_js_source());
        let candidates: Vec<_> = (0..8)
            .map(|i| {
                serde_json::json!({
                    "kind": "addRedaction",
                    "bounds": {"x": i * 2, "y": 0, "width": 1, "height": 1},
                    "confidence": 0.8,
                    "label": format!("candidate-{i}")
                })
            })
            .collect();
        let output = serde_json::json!({ "candidates": candidates });
        let executor = Arc::new(FakeExecutor {
            output_json: serde_json::to_string(&output).unwrap(),
        });
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let tool = DryRunTool::new(ctx, executor, host);

        let result = tool
            .call(&serde_json::json!({"source": valid_js_source(), "generation": 0}))
            .await
            .unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["candidate_count"].as_u64(), Some(8));
                assert_eq!(
                    result_json["candidate_preview"].as_array().unwrap().len(),
                    5
                );
            }
            other => panic!("expected dry-run success, got {other:?}"),
        }
    }

    // ---- Authority enforcement ----

    use crate::authority::{
        AuthorityBinding, AuthoritySnapshot, DisclosureCeiling, RunOperation,
    };
    use std::collections::BTreeSet;
    use crate::product_task::{AnnotationStateV1, TaskAttemptId};

    /// Counting tool that tracks call count via an atomic counter.
    struct CountingTool {
        call_count: Arc<AtomicUsize>,
        ops: &'static [RunOperation],
    }

    impl CountingTool {
        fn new(call_count: Arc<AtomicUsize>, ops: &'static [RunOperation]) -> Self {
            Self { call_count, ops }
        }
    }

    impl Tool for CountingTool {
        fn name(&self) -> &str {
            "counting"
        }

        fn json_schema(&self) -> Value {
            tool_schema::<EmptyArgs>()
        }

        fn required_operations(&self) -> &'static [RunOperation] {
            self.ops
        }

        fn call<'a>(&'a self, _arguments: &'a Value) -> ToolFuture<'a> {
            let count = self.call_count.clone();
            Box::pin(async move {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(ToolOutcome::Success {
                    result_json: serde_json::json!({}),
                })
            })
        }
    }

    /// Create a snapshot granting the given operations, bound to the same
    /// run_id and content_binding as the standard test_context.
    fn snapshot_granting(
        ops: impl IntoIterator<Item = RunOperation>,
    ) -> AuthoritySnapshot {
        let annotation_state = AnnotationStateV1 {
            width: 100,
            height: 100,
            state_id: 0,
            annotations: vec![],
        };
        let binding = DocumentContentBinding::new([1u8; 32], &annotation_state, 0).unwrap();
        let auth_binding = AuthorityBinding::new(
            crate::product_task::ProductTaskId::parse(
                "task-00000000-0000-4000-8000-000000000001",
            )
            .unwrap(),
            TaskAttemptId::new(1),
            RunId::parse("run-00000000-0000-4000-8000-000000000001").unwrap(),
            binding,
        );
        // InspectPreparedImage requires existing_product_capture = true.
        let ops_vec: Vec<_> = ops.into_iter().collect();
        let needs_capture = ops_vec.contains(&RunOperation::InspectPreparedImage);
        AuthoritySnapshot::new(
            auth_binding,
            "test-rev".into(),
            DisclosureCeiling::FullScreenshot,
            needs_capture,
            BTreeSet::new(),
            ops_vec.into_iter().collect(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn advertised_registered_tool_without_grant_never_enters_tool_body() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new(ToolRegistryLimits::permissive());
        registry
            .register(Arc::new(CountingTool::new(
                calls.clone(),
                &[RunOperation::WriteDraft],
            )))
            .unwrap();
        assert!(registry.tool_names().contains(&"counting"));

        let result = registry
            .execute_authorized_calls(
                &[ToolCall {
                    name: "counting".into(),
                    arguments_json: serde_json::json!({}),
                }],
                &RunCancellation::new(),
                &BTreeSet::new(),
                &snapshot_granting([RunOperation::ReadDraft]),
                &test_context("source"),
            )
            .await;

        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            Err(ToolError::AuthorityDenied {
                operation: RunOperation::WriteDraft,
                ..
            })
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0, "tool body must not execute");
    }

    #[tokio::test]
    async fn tool_with_grant_succeeds() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new(ToolRegistryLimits::permissive());
        registry
            .register(Arc::new(CountingTool::new(
                calls.clone(),
                &[RunOperation::WriteDraft],
            )))
            .unwrap();

        let result = registry
            .execute_authorized_calls(
                &[ToolCall {
                    name: "counting".into(),
                    arguments_json: serde_json::json!({}),
                }],
                &RunCancellation::new(),
                &BTreeSet::new(),
                &snapshot_granting([RunOperation::WriteDraft]),
                &test_context("source"),
            )
            .await;

        assert_eq!(result.len(), 1);
        assert!(result[0].is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn multi_operation_requirement_denied_when_one_missing() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new(ToolRegistryLimits::permissive());
        registry
            .register(Arc::new(CountingTool::new(
                calls.clone(),
                &[RunOperation::ReadDraft, RunOperation::WriteDraft],
            )))
            .unwrap();

        // Grant only ReadDraft — WriteDraft is missing.
        let result = registry
            .execute_authorized_calls(
                &[ToolCall {
                    name: "counting".into(),
                    arguments_json: serde_json::json!({}),
                }],
                &RunCancellation::new(),
                &BTreeSet::new(),
                &snapshot_granting([RunOperation::ReadDraft]),
                &test_context("source"),
            )
            .await;

        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            Err(ToolError::AuthorityDenied {
                operation: RunOperation::WriteDraft,
                ..
            })
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn mismatched_run_id_denied() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new(ToolRegistryLimits::permissive());
        registry
            .register(Arc::new(CountingTool::new(
                calls.clone(),
                &[RunOperation::ReadDraft],
            )))
            .unwrap();

        let annotation_state = AnnotationStateV1 {
            width: 100,
            height: 100,
            state_id: 0,
            annotations: vec![],
        };
        let binding = DocumentContentBinding::new([1u8; 32], &annotation_state, 0).unwrap();
        let auth_binding = AuthorityBinding::new(
            crate::product_task::ProductTaskId::parse(
                "task-00000000-0000-4000-8000-000000000001",
            )
            .unwrap(),
            TaskAttemptId::new(1),
            // Wrong run ID
            RunId::parse("run-99999999-9999-4999-8999-999999999999").unwrap(),
            binding,
        );
        let stale_snapshot = AuthoritySnapshot::new(
            auth_binding,
            "test-rev".into(),
            DisclosureCeiling::FullScreenshot,
            false,
            BTreeSet::new(),
            [RunOperation::ReadDraft].into_iter().collect(),
        )
        .unwrap();

        let result = registry
            .execute_authorized_calls(
                &[ToolCall {
                    name: "counting".into(),
                    arguments_json: serde_json::json!({}),
                }],
                &RunCancellation::new(),
                &BTreeSet::new(),
                &stale_snapshot,
                &test_context("source"),
            )
            .await;

        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            Err(ToolError::AuthorityDenied { .. })
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn mismatched_document_binding_denied() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new(ToolRegistryLimits::permissive());
        registry
            .register(Arc::new(CountingTool::new(
                calls.clone(),
                &[RunOperation::ReadDraft],
            )))
            .unwrap();

        // Different state_id (1 vs 0) — binding mismatch.
        let annotation_state = AnnotationStateV1 {
            width: 100,
            height: 100,
            state_id: 1,
            annotations: vec![],
        };
        let binding = DocumentContentBinding::new([1u8; 32], &annotation_state, 1).unwrap();
        let auth_binding = AuthorityBinding::new(
            crate::product_task::ProductTaskId::parse(
                "task-00000000-0000-4000-8000-000000000001",
            )
            .unwrap(),
            TaskAttemptId::new(1),
            RunId::parse("run-00000000-0000-4000-8000-000000000001").unwrap(),
            binding,
        );
        let stale_snapshot = AuthoritySnapshot::new(
            auth_binding,
            "test-rev".into(),
            DisclosureCeiling::FullScreenshot,
            false,
            BTreeSet::new(),
            [RunOperation::ReadDraft].into_iter().collect(),
        )
        .unwrap();

        let result = registry
            .execute_authorized_calls(
                &[ToolCall {
                    name: "counting".into(),
                    arguments_json: serde_json::json!({}),
                }],
                &RunCancellation::new(),
                &BTreeSet::new(),
                &stale_snapshot,
                &test_context("source"),
            )
            .await;

        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            Err(ToolError::AuthorityDenied { .. })
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn authority_denial_stops_later_calls_in_batch() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new(ToolRegistryLimits::permissive());
        // First tool requires WriteDraft — not granted.
        registry
            .register(Arc::new(CountingTool::new(
                counter.clone(),
                &[RunOperation::WriteDraft],
            )))
            .unwrap();
        // Second tool requires ReadDraft — granted.
        struct SecondTool(Arc<AtomicUsize>);
        impl Tool for SecondTool {
            fn name(&self) -> &str { "second" }
            fn json_schema(&self) -> Value { tool_schema::<EmptyArgs>() }
            fn required_operations(&self) -> &'static [RunOperation] { &[RunOperation::ReadDraft] }
            fn call<'a>(&'a self, _arguments: &'a Value) -> ToolFuture<'a> {
                let c = self.0.clone();
                Box::pin(async move {
                    c.fetch_add(100, Ordering::SeqCst);
                    Ok(ToolOutcome::Success { result_json: serde_json::json!({}) })
                })
            }
        }
        registry.register(Arc::new(SecondTool(counter.clone()))).unwrap();

        let result = registry
            .execute_authorized_calls(
                &[
                    ToolCall {
                        name: "counting".into(),
                        arguments_json: serde_json::json!({}),
                    },
                    ToolCall {
                        name: "second".into(),
                        arguments_json: serde_json::json!({}),
                    },
                ],
                &RunCancellation::new(),
                &BTreeSet::new(),
                &snapshot_granting([RunOperation::ReadDraft]),
                &test_context("source"),
            )
            .await;

        // Authority denial is a hard error → stops after first call.
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            Err(ToolError::AuthorityDenied { .. })
        ));
        assert_eq!(counter.load(Ordering::SeqCst), 0, "neither tool body must execute");
    }

    #[tokio::test]
    async fn cancellation_wins_over_authority_check() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new(ToolRegistryLimits::permissive());
        registry
            .register(Arc::new(CountingTool::new(
                calls.clone(),
                &[RunOperation::WriteDraft],
            )))
            .unwrap();

        let cancel = RunCancellation::new();
        cancel.cancel();

        // Snapshot has the right grant — but cancellation fires first.
        let result = registry
            .execute_authorized_calls(
                &[ToolCall {
                    name: "counting".into(),
                    arguments_json: serde_json::json!({}),
                }],
                &cancel,
                &BTreeSet::new(),
                &snapshot_granting([RunOperation::WriteDraft]),
                &test_context("source"),
            )
            .await;

        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0],
            Err(ToolError::Cancelled),
            "cancellation must fire before authority check"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn existing_execute_calls_path_unchanged() {
        // The existing execute_calls (no authority) must continue to work
        // as before — no authority enforcement.
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new(ToolRegistryLimits::permissive());
        registry
            .register(Arc::new(CountingTool::new(
                calls.clone(),
                &[RunOperation::WriteDraft],
            )))
            .unwrap();

        let result = registry
            .execute_calls(
                &[ToolCall {
                    name: "counting".into(),
                    arguments_json: serde_json::json!({}),
                }],
                &RunCancellation::new(),
                &BTreeSet::new(),
            )
            .await;

        assert_eq!(result.len(), 1);
        assert!(result[0].is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "tool body must execute without authority");
    }
}
