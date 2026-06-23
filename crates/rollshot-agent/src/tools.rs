use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::SessionId;
use crate::runtime::{DraftState, EvidenceKind, RunCancellation};

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

    async fn execute_single(
        &self,
        index: usize,
        call: &ToolCall,
        cancellation: &RunCancellation,
    ) -> Result<ToolOutcome, ToolError> {
        if cancellation.is_cancelled() {
            return Err(ToolError::Cancelled);
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

        let outcome = tool.call(&call.arguments_json).await?;

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

    pub async fn execute_calls(
        &self,
        calls: &[ToolCall],
        cancellation: &RunCancellation,
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

            let result = self.execute_single(index, call, cancellation).await;
            let is_terminal = result.is_err();
            results.push(result);

            if is_terminal {
                break;
            }
        }
        results
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DryRunResult {
    pub candidate_count: u32,
    pub affected_area: f32,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestUserInputResult {
    pub needs_input: bool,
    pub current_generation: u64,
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

// ---------- Tool context ----------

pub struct ToolContext {
    pub draft: Mutex<DraftState>,
    pub source: Mutex<String>,
    pub validation_limits: rollshot_automation::ValidationLimits,
    pub execution_policy: rollshot_automation::ExecutionPolicy,
    pub session_id: SessionId,
    pub image_dims: (u32, u32),
}

impl ToolContext {
    pub fn new(
        session_id: SessionId,
        initial_source: String,
        validation_limits: rollshot_automation::ValidationLimits,
        execution_policy: rollshot_automation::ExecutionPolicy,
        image_dims: (u32, u32),
    ) -> Self {
        Self {
            draft: Mutex::new(DraftState::new(session_id)),
            source: Mutex::new(initial_source),
            validation_limits,
            execution_policy,
            session_id,
            image_dims,
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

            *self.ctx.source.lock().unwrap() = args.source;

            Ok(ToolOutcome::Success {
                result_json: serde_json::to_value(ReplaceSourceResult {
                    new_generation: new_gen,
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
                proposal_id: rollshot_edit_proposal::ProposalId(1),
                base_document_state_id: 0,
                provenance: rollshot_edit_proposal::Provenance {
                    source: rollshot_edit_proposal::ProvenanceSource::Agent {
                        run_id: self.ctx.session_id.get(),
                    },
                },
            };

            let input = rollshot_automation::AutomationInput {
                image_width: self.ctx.image_dims.0,
                image_height: self.ctx.image_dims.1,
                region: None,
                annotations: Vec::new(),
                capability_handles: std::collections::BTreeMap::new(),
            };

            let cancellation = rollshot_automation::CancellationFlag::new();

            let mut host_guard = self.host.lock().unwrap();
            let (proposal, _metrics) = rollshot_automation::execute_to_proposal(
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

            let mut draft = self.ctx.draft.lock().unwrap();
            draft
                .record_evidence(
                    EvidenceKind::DryRun,
                    generation,
                    tokio::time::Instant::now(),
                )
                .map_err(|e| ToolError::ArgumentDecode(e.to_string()))?;

            Ok(ToolOutcome::Success {
                result_json: serde_json::to_value(DryRunResult {
                    candidate_count: proposal.candidates.len() as u32,
                    affected_area,
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

            // Require prior validation or dry_run evidence at this generation.
            let has_evidence = draft.evidence().iter().any(|e| {
                e.source_generation == args.generation
                    && matches!(e.kind, EvidenceKind::Validation | EvidenceKind::DryRun)
            });
            if !has_evidence {
                return Err(ToolError::ArgumentDecode(
                    "no validation or dry_run evidence at this generation".into(),
                ));
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
        "get_context_summary"
    }

    fn json_schema(&self) -> Value {
        tool_schema::<RequestUserInputArgs>()
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

#[derive(Default)]
pub struct OcrTool;

impl OcrTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for OcrTool {
    fn name(&self) -> &str {
        "ocr"
    }

    fn json_schema(&self) -> Value {
        tool_schema::<RequestUserInputArgs>()
    }

    fn call<'a>(&'a self, _arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            Ok(ToolOutcome::Success {
                result_json: serde_json::to_value(CapabilityUnavailable {
                    capability: "ocr".into(),
                    reason: "OCR not available in this context".into(),
                })
                .unwrap_or_default(),
            })
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
        "layout"
    }

    fn json_schema(&self) -> Value {
        tool_schema::<RequestUserInputArgs>()
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

#[derive(Default)]
pub struct RegionFeaturesTool;

impl RegionFeaturesTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for RegionFeaturesTool {
    fn name(&self) -> &str {
        "region_features"
    }

    fn json_schema(&self) -> Value {
        tool_schema::<RequestUserInputArgs>()
    }

    fn call<'a>(&'a self, _arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            Ok(ToolOutcome::Success {
                result_json: serde_json::to_value(CapabilityUnavailable {
                    capability: "region_features".into(),
                    reason: "Region features not available in this context".into(),
                })
                .unwrap_or_default(),
            })
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
        let results = reg.execute_calls(&calls, &cancel).await;
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
        let results = reg.execute_calls(&calls, &cancel).await;
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], Err(ToolError::ArgumentDecode(_))));
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
        let results = reg.execute_calls(&calls, &cancel).await;
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], Err(ToolError::ArgumentDecode(_))));
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
        let results = reg.execute_calls(&calls, &cancel).await;
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
        let results = reg.execute_calls(&calls, &cancel).await;
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
        let results = reg.execute_calls(&calls, &cancel).await;
        assert!(results[0].is_ok());

        let calls = vec![ToolCall {
            name: "echo".into(),
            arguments_json: serde_json::json!({"text": "second"}),
        }];
        let results = reg.execute_calls(&calls, &cancel).await;
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
        let results = reg.execute_calls(&calls, &cancel).await;
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
        let results = reg.execute_calls(&calls, &cancel).await;
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
        let results = reg.execute_calls(&calls, &cancel).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], Err(ToolError::Cancelled));
    }

    // ---- Authoring test helpers ----

    fn test_context(source: &str) -> Arc<ToolContext> {
        let mut policy = rollshot_automation::ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(5),
            4 * 1024 * 1024,
            1024 * 1024,
        );
        policy.proposal_limits.max_total_area_fraction = 0.5;
        Arc::new(ToolContext::new(
            SessionId::new(1),
            source.into(),
            rollshot_automation::ValidationLimits::default(),
            policy,
            (100, 100),
        ))
    }

    fn valid_js_source() -> &'static str {
        "function main(input) { return [{kind: 'addRedaction', bounds: {x: 0, y: 0, width: 10, height: 10}, confidence: 0.9, label: 'test'}]; }"
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
            }
            other => panic!("expected success, got {other:?}"),
        }

        assert_eq!(*ctx.source.lock().unwrap(), "new source");
        assert_eq!(ctx.draft.lock().unwrap().generation(), 1);
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
            }
            other => panic!("expected success, got {other:?}"),
        }

        // Evidence should be recorded.
        assert_eq!(ctx.draft.lock().unwrap().evidence().len(), 1);
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
    async fn submit_for_review_succeeds_with_prior_evidence() {
        let ctx = test_context(valid_js_source());
        // Record validation evidence at generation 0.
        ctx.draft
            .lock()
            .unwrap()
            .record_evidence(EvidenceKind::Validation, 0, tokio::time::Instant::now())
            .unwrap();

        let tool = SubmitForReviewTool::new(ctx);
        let args = serde_json::json!({"generation": 0});
        let result = tool.call(&args).await.unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["submitted"].as_bool(), Some(true));
            }
            other => panic!("expected success, got {other:?}"),
        }
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

    // ---- Inspection: unavailable ----

    #[tokio::test]
    async fn ocr_returns_unavailable() {
        let tool = OcrTool::new();
        let args = serde_json::json!({});
        let result = tool.call(&args).await.unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["capability"].as_str(), Some("ocr"));
                assert!(result_json["reason"]
                    .as_str()
                    .unwrap()
                    .contains("not available"));
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

    #[tokio::test]
    async fn region_features_returns_unavailable() {
        let tool = RegionFeaturesTool::new();
        let args = serde_json::json!({});
        let result = tool.call(&args).await.unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["capability"].as_str(), Some("region_features"));
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
}
