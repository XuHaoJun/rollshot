# Agent Foundation Slice 6: Durable Audit Observability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add durable, correlated, privacy-safe evidence for every material Smart Redaction Product Task transition while keeping task/document snapshots authoritative and transient UI events lossy.

**Architecture:** `rollshot-agent` defines a closed V1 audit vocabulary, exact Product Task transition derivation, bounded failures, and an async append-sink trait. `rollshot-app` adds a per-task hash-chained JSONL journal and coordinates each task mutation with a prepare → existing snapshot create/CAS → commit protocol under the existing TaskStore lock; startup resolves incomplete transactions from authoritative snapshots before admitting writes. The UI continues to restore from Product Task/document state and never replays audit records.

**Tech Stack:** Rust 2021 workspace; `serde`/`serde_json`; `sha2`; `uuid`; `fs4`; `tokio::task::spawn_blocking`; existing `ProductTaskSnapshot`, `TaskStore`, `AuthoritySnapshot`, `SkillUseReceiptV1`, `RunEventSink`, and `tracing` contracts.

## Global Constraints

- Governing spec: `docs/superpowers/specs/2026-07-28-agent-foundation-audit-observability-design.md`.
- `ProductTaskSnapshot` and the image document remain authoritative; NEVER reconstruct product state from the audit journal.
- V1 scope is the existing Smart Redaction author/improve Product Task path only.
- `complete_apply` is the V1 product publication/commit boundary; external Save/Export and Action Guide publishing remain out of scope.
- `RunEvent` remains transient and lossy; the result workspace MUST NOT read or replay audit journals.
- Durable fields are IDs, revisions, digests, bounded enums/categories, timestamps, registered tool names, and bounded review receipts only.
- NEVER persist pixels, image/proposal bytes, prompt/source/response prose, transcripts, raw semantic input, provider internals/credentials, full skill bodies, authority grants, or tool arguments/results.
- Every runtime diagnostic uses structured `tracing` with stable `rollshot::agent::audit` or `rollshot::app::agent_audit_store` targets; per-record details use `trace`.
- No new dependencies are required: `sha2`, `uuid`, `serde_json`, `fs4`, and `tokio` already exist in the affected crates.
- No user-visible iced UI change is authorized. Stop and revise the spec before any layout, copy, interaction, or visual-baseline change.
- Do not close Gate G3 until Slice 5's pending user approval and missing formal independent review are separately resolved.
- All shell commands MUST be prefixed with `rtk`.
- The active Smart Redaction path requires an opened/reconciled `TaskStore`;
  absence or startup corruption fails before provider dispatch.
- Journal scans stream bounded records with `BufReader`; production scanning
  MUST NOT allocate the complete 16 MiB journal.
- Each physical record is opened, appended, synchronized, and closed within one
  `spawn_blocking` operation. V1 has no group commit or cached file handles.

## File map

### New files

- `crates/rollshot-agent/src/audit.rs` — storage-neutral V1 audit IDs, references, events, envelope validation/canonicalization, transition derivation, append receipts/errors, and `AuditAppendSink`.
- `crates/rollshot-app/src/result_workspace/workbench/audit_store/mod.rs` — per-task journal open/append/scan APIs, file safety, physical acknowledgements, failpoints, and async `TaskAuditSink` adapter.
- `crates/rollshot-app/src/result_workspace/workbench/audit_store/record.rs` — private physical JSONL record schema, canonical record hashing, sequence/hash-chain validation, and verified scan state.
- `crates/rollshot-app/src/result_workspace/workbench/audit_store/reconcile.rs` — pure unresolved-transaction classification from verified journal state plus authoritative task presence/revision/transition receipt.
- `docs/superpowers/spikes/2026-07-28-audit-observability-decision.md` — implementation-time Gate G3 evidence, migrations, residual risks, deferred scope, and the separate Slice 5 blocker.

### Modified files

- `crates/rollshot-agent/src/lib.rs` — export `audit`.
- `crates/rollshot-agent/src/runtime.rs` — remove dormant `AuditEvent` and its two tests; retain `RunEvent` unchanged.
- `crates/rollshot-agent/src/authority.rs` — add a bounded audit reference constructor that excludes grants/capability lists.
- `crates/rollshot-agent/src/product_task.rs` — add read-only receipts/accessors required for deterministic transition derivation and reconcile abandoned `Created` tasks to `Interrupted`; do not add audit storage state.
- `crates/rollshot-agent/src/driver.rs` — add `RunTerminalState::AuditFailure`, accept an audit sink on the authorized Smart Redaction path, and durably append authority denials before terminal return.
- `crates/rollshot-app/src/result_workspace/workbench/mod.rs` — declare `audit_store`.
- `crates/rollshot-app/src/result_workspace/workbench/task_store.rs` — compose the journal with existing snapshot persistence/lock, expose audited create/transition/standalone append, bootstrap old tasks, reconcile protocol records, and prune whole task/journal pairs.
- `crates/rollshot-app/src/result_workspace/workbench/run.rs` — split task create/start persistence, route run-contract/artifact/terminal transitions through audited methods, inject `TaskAuditSink`, and map audit failure terminals.
- `crates/rollshot-app/src/result_workspace/update.rs` — route apply/reject/compensation transitions through audited methods and preserve authoritative-state UI repair.
- `crates/rollshot-app/src/result_workspace/workbench/state.rs` — add bounded workbench error display mapping only if exhaustive `RunTerminalState` matching requires it; no new UI copy beyond the existing generic persistence failure path.
- `crates/rollshot-agent/src/continuity.rs` — update exhaustive terminal/category fixtures only when compilation identifies a `RunTerminalState` match; continuity data must not gain audit payloads.

## Mandatory red/green checkpoints

Every implementation step below stops at its focused green check before the
next layer begins. These checkpoints supplement, not replace, each task's final
regression command.

| Task / after step | Run | Expected |
|---|---|---|
| Task 1 / Step 3 | `rtk cargo test -p rollshot-agent audit::tests::envelope` | PASS for identity, correlation, canonical bytes |
| Task 1 / Step 4 | `rtk cargo test -p rollshot-agent audit_ref && rtk cargo test -p rollshot-agent audit_transition_receipt` | PASS; no grant/payload leakage |
| Task 1 / Step 5 | `rtk cargo test -p rollshot-agent audit::tests::derive` | PASS for every legal/illegal edge |
| Task 1 / Step 6 | `rtk cargo test -p rollshot-agent audit::tests::append_contract` | PASS for bounded sink receipts/errors |
| Task 2 / Step 3 | `rtk cargo test -p rollshot-app audit_store::record --lib` | PASS for golden/hash-chain tests |
| Task 2 / Step 5 | `rtk cargo test -p rollshot-app audit_store::tests::scan --lib` | PASS for streaming scan/tail/corruption tests |
| Task 2 / Step 6 | `rtk cargo test -p rollshot-app audit_store::tests::append --lib` | PASS for sync/visibility failpoints |
| Task 3 / Step 3 | `rtk cargo test -p rollshot-app audit_store::reconcile --lib` | PASS for pure decision matrix |
| Task 3 / Step 5 | `rtk cargo test -p rollshot-app task_store::tests::cas --lib` | PASS for unchanged raw CAS semantics |
| Task 3 / Step 6 | `rtk cargo test -p rollshot-app audited_ --lib` | PASS for prepare/snapshot/outcome protocol |
| Task 3 / Step 7 | `rtk cargo test -p rollshot-app audit_reopen --lib && rtk cargo test -p rollshot-app audit_same_process_reconcile --lib` | PASS with no unresolved transaction |
| Task 4 / Step 4 | `rtk cargo test -p rollshot-app task_store_required --lib && rtk cargo test -p rollshot-app created_attempt_audit --lib` | PASS; no dispatch without store |
| Task 4 / Step 5 | `rtk cargo test -p rollshot-app run_contract_audit --lib` | PASS with exact authority/skill receipts |
| Task 4 / Step 6 | `rtk cargo test -p rollshot-app artifact_terminal_audit --lib` | PASS; no partial promotion |
| Task 4 / Step 7 | `rtk cargo test -p rollshot-app abandoned_created_task_is_interrupted --lib` | PASS after fresh reopen |
| Task 5 / Step 4 | `rtk cargo test -p rollshot-app review_audit --lib` | PASS for begin/apply/reject/compensation |
| Task 5 / Step 5 | `rtk cargo test -p rollshot-app applied_review_receipt_audit --lib` | PASS with exact available document receipt |
| Task 5 / Step 7 | `rtk cargo test -p rollshot-app audit_retention --lib` | PASS including half-delete recovery |
| Task 6 / Step 4 | `rtk cargo test -p rollshot-agent authority_denial_is_acknowledged -- --nocapture` | PASS before tool terminal |
| Task 6 / Step 5 | `rtk cargo test -p rollshot-agent authority_denial_audit_failure -- --nocapture` | PASS with `AuditFailure` |
| Task 6 / Step 6 | `rtk cargo test -p rollshot-app task_audit_sink --lib` | PASS without blocking runtime worker |
| Task 6 / Step 7 | `rtk cargo test -p rollshot-app authority_denial --lib` | PASS after fresh reopen |
| Task 7 / Step 6 | `rtk cargo test -p rollshot-agent audit && rtk cargo test -p rollshot-app dropped_display_events --lib && rtk cargo test -p rollshot-app audit_privacy --lib && rtk cargo test -p rollshot-app corrupt_journal --lib` | PASS after minimal repair/privacy fixes |

---

### Task 1: Define the storage-neutral audit domain and exact transition derivation

**Files:**
- Create: `crates/rollshot-agent/src/audit.rs`
- Modify: `crates/rollshot-agent/src/lib.rs:1-10`
- Modify: `crates/rollshot-agent/src/authority.rs:119-250`
- Modify: `crates/rollshot-agent/src/product_task.rs:235-441,581-641,704-1186`
- Modify: `crates/rollshot-agent/src/runtime.rs:604-613,1149-1186`
- Test: inline `#[cfg(test)]` modules in `audit.rs`, `authority.rs`, and `product_task.rs`

**Interfaces:**
- Consumes: `ProductTaskSnapshot`, `TaskStatus`, `TaskTerminal`, `TaskAttemptId`, `RunId`, `ProductArtifactMetadata`, `ReviewReceipt`, `AuthoritySnapshot`, `RunOperation`, and `SkillUseReceiptV1`.
- Produces:
  - `AuditEventId::parse(String) -> Result<AuditEventId, AuditContractError>` and `AuditEventId::new_v4() -> AuditEventId`.
  - `AuditEnvelopeV1::new(AuditEventId, i64, AuditEventV1, AuditCorrelationV1) -> Result<Self, AuditContractError>`.
  - `derive_material_transition(Option<&ProductTaskSnapshot>, &ProductTaskSnapshot, AuditEventId, i64) -> Result<AuditEnvelopeV1, AuditContractError>`.
  - `AuditAppendSink::append(AuditEnvelopeV1) -> Pin<Box<dyn Future<Output = Result<AuditAppendReceiptV1, AuditAppendError>> + Send + '_>>`.
  - `AuthoritySnapshot::audit_ref() -> AuthorityAuditRefV1` with no grant/capability collections.
  - `ProductTaskSnapshot::audit_transition_receipt() -> AuditTaskStateReceiptV1` with no artifact/proposal payload bytes.

- [ ] **Step 1: Add failing identity, correlation, privacy, and transition-table tests**

Add tests that construct existing Product Task fixtures and assert exact variants:

```rust
#[test]
fn derives_task_created_only_from_absent_to_created() {
    let created = created_task_fixture();
    let envelope = derive_material_transition(
        None,
        &created,
        AuditEventId::parse("audit-00000000-0000-4000-8000-000000000001").unwrap(),
        10,
    )
    .unwrap();
    assert_eq!(envelope.event(), &AuditEventV1::TaskCreated);
    assert_eq!(envelope.correlation().task_id(), created.task_id());
}

#[test]
fn derives_attempt_started_from_created_to_running() {
    let created = created_task_fixture();
    let running = created
        .start_attempt(TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20), 20)
        .unwrap();
    let envelope = derive_material_transition(
        Some(&created),
        &running,
        audit_id(2),
        20,
    )
    .unwrap();
    assert_eq!(envelope.event(), &AuditEventV1::AttemptStarted);
    assert_eq!(envelope.correlation().attempt_id(), Some(TaskAttemptId::new(1)));
    assert_eq!(envelope.correlation().run_id(), Some(&run_id()));
}

#[test]
fn rejects_collapsed_absent_to_running_transition() {
    let created = created_task_fixture();
    let running = created
        .start_attempt(TaskAttempt::new(TaskAttemptId::new(1), run_id(), 20), 20)
        .unwrap();
    assert!(matches!(
        derive_material_transition(None, &running, audit_id(3), 20),
        Err(AuditContractError::TransitionMismatch { .. })
    ));
}

#[test]
fn serialized_event_excludes_adjacent_sensitive_objects() {
    let (running, authority, skill_use, source_sentinel, skill_sentinel) =
        sensitive_run_fixture();
    let bound = running
        .bind_run_contract(run_contract_fixture(&authority, &skill_use), 30)
        .unwrap();
    let envelope = derive_material_transition(Some(&running), &bound, audit_id(4), 30).unwrap();
    let json = serde_json::to_string(&envelope).unwrap();
    for forbidden in [source_sentinel, skill_sentinel, "granted_operations", "prepared_capabilities"] {
        assert!(!json.contains(forbidden), "audit leak: {forbidden}");
    }
}
```

Add one test per legal state edge and one rejection test for each mismatch: task ID, revision, timestamp regression, attempt, run, artifact, proposal, run contract, skill digest, authority digest, review receipt, and document state receipt.

- [ ] **Step 2: Run the new tests and confirm the module is absent**

Run:

```bash
rtk cargo test -p rollshot-agent audit -- --nocapture
```

Expected: FAIL because `rollshot_agent::audit` and the new types do not exist.

- [ ] **Step 3: Implement opaque IDs, bounded references, event vocabulary, and envelope validation**

Create these closed public shapes in `audit.rs`:

```rust
pub const AUDIT_SCHEMA_VERSION_V1: u32 = 1;
pub const MAX_AUDIT_STRING_BYTES: usize = 256;
pub const MAX_AUDIT_CANDIDATE_IDS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AuditEventId(String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityAuditRefV1 {
    pub snapshot_digest: String,
    pub policy_revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillUseAuditRefV1 {
    pub source_authority: String,
    pub package_id: String,
    pub main_resource_id: String,
    pub package_digest: String,
    pub invocation_kind: String,
    pub catalog_revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactAuditRefV1 {
    pub artifact_id: ArtifactId,
    pub artifact_revision: ArtifactRevision,
    pub kind: ArtifactKind,
    pub schema_version: u32,
    pub canonical_payload_sha256: String,
    pub proposal_id: String,
    pub source_binding_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewDecisionAuditV1 {
    Applied {
        applied_candidate_ids: Vec<u32>,
        rejected_candidate_ids: Vec<u32>,
        resulting_document_state_id: u32,
        resulting_document_sha256: Option<String>,
    },
    Rejected,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditCorrelationV1 {
    task_id: ProductTaskId,
    attempt_id: Option<TaskAttemptId>,
    run_id: Option<RunId>,
    authority: Option<AuthorityAuditRefV1>,
    skill_use: Option<SkillUseAuditRefV1>,
    artifact: Option<ArtifactAuditRefV1>,
    review: Option<ReviewDecisionAuditV1>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventKindV1 {
    TaskCreated,
    AttemptStarted,
    RunContractBound,
    AuthorityDenied,
    ArtifactPromoted,
    ReviewApplyStarted,
    ReviewDecisionCommitted,
    TaskTerminated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditBudgetDimensionV1 {
    WallTime,
    ModelCalls,
    InputTokens,
    OutputTokens,
    Cost,
    ToolCalls,
    PerToolCalls,
    ArgumentBytes,
    ResultBytes,
    SourceBytes,
    Attachments,
    ValidationAttempts,
    DryRunAttempts,
    CapabilityCalls,
    CandidateCount,
    AffectedArea,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditContextRecoveryFailureCategoryV1 {
    StaleReference,
    ManifestBuildFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditTaskTerminalV1 {
    NeedsUserInput,
    Cancelled,
    BudgetExhausted { dimension: AuditBudgetDimensionV1 },
    SourceValidationFailure,
    RuntimeFailure,
    AgentProtocolFailure,
    ProviderFailure,
    Interrupted,
    Stale,
    ContextOverflow,
    ContextRecoveryFailure {
        category: AuditContextRecoveryFailureCategoryV1,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditTaskStatusV1 {
    Created,
    Running,
    ReadyForReview,
    Applying,
    Completed,
    Rejected,
    Stale,
    Failed { terminal: AuditTaskTerminalV1 },
    NeedsUserInput,
    Cancelled,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventV1 {
    TaskCreated,
    AttemptStarted,
    RunContractBound,
    AuthorityDenied { tool: String, operation: RunOperation },
    ArtifactPromoted,
    ReviewApplyStarted,
    ReviewDecisionCommitted,
    TaskTerminated { terminal: AuditTaskTerminalV1 },
}

impl AuditEventV1 {
    pub fn kind(&self) -> AuditEventKindV1 {
        match self {
            Self::TaskCreated => AuditEventKindV1::TaskCreated,
            Self::AttemptStarted => AuditEventKindV1::AttemptStarted,
            Self::RunContractBound => AuditEventKindV1::RunContractBound,
            Self::AuthorityDenied { .. } => AuditEventKindV1::AuthorityDenied,
            Self::ArtifactPromoted => AuditEventKindV1::ArtifactPromoted,
            Self::ReviewApplyStarted => AuditEventKindV1::ReviewApplyStarted,
            Self::ReviewDecisionCommitted => AuditEventKindV1::ReviewDecisionCommitted,
            Self::TaskTerminated { .. } => AuditEventKindV1::TaskTerminated,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEnvelopeV1 {
    schema_version: u32,
    event_id: AuditEventId,
    occurred_at_unix_ms: i64,
    event: AuditEventV1,
    correlation: AuditCorrelationV1,
    canonical_payload_sha256: String,
}
```

Use explicit checked constructors and read-only accessors. Canonical bytes use fixed-field DTOs and the domain separator `b"rollshot-audit-envelope-v1\0"`; do not serialize maps or native error strings.

- [ ] **Step 4: Add bounded authority and Product Task receipts**

Implement:

```rust
impl AuthoritySnapshot {
    pub fn audit_ref(&self) -> AuthorityAuditRefV1 {
        AuthorityAuditRefV1 {
            snapshot_digest: self.digest().to_owned(),
            policy_revision: self.policy_revision().to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditTaskStateReceiptV1 {
    pub task_id: ProductTaskId,
    pub snapshot_revision: u32,
    pub status: AuditTaskStatusV1,
    pub attempt_id: Option<TaskAttemptId>,
    pub run_id: Option<RunId>,
    pub artifact: Option<ArtifactAuditRefV1>,
    pub review_decided_at_unix_ms: Option<i64>,
    pub authority_snapshot_digest: Option<String>,
    pub skill_package_digest: Option<String>,
}
```

Add only the minimal `AuthoritySnapshot::policy_revision()` and Product Task metadata accessors needed to construct these receipts. Do not expose authority grants, pending artifact bytes, or proposal bytes.

- [ ] **Step 5: Implement the legal transition derivation table**

Implement one exhaustive match over `(expected status, replacement status)`:

```rust
match (expected.map(ProductTaskSnapshot::status), replacement.status()) {
    (None, TaskStatus::Created) => AuditEventV1::TaskCreated,
    (Some(TaskStatus::Created), TaskStatus::Running) => AuditEventV1::AttemptStarted,
    (Some(TaskStatus::Running), TaskStatus::Running)
        if run_contract_was_bound(expected.unwrap(), replacement) =>
            AuditEventV1::RunContractBound,
    (Some(TaskStatus::Running), TaskStatus::ReadyForReview) =>
        AuditEventV1::ArtifactPromoted,
    (Some(TaskStatus::ReadyForReview), TaskStatus::Applying) =>
        AuditEventV1::ReviewApplyStarted,
    (Some(TaskStatus::Applying), TaskStatus::Completed) =>
        AuditEventV1::ReviewDecisionCommitted,
    (Some(TaskStatus::ReadyForReview), TaskStatus::Rejected) =>
        AuditEventV1::ReviewDecisionCommitted,
    (Some(_), status) if is_supported_non_review_terminal(status) =>
        AuditEventV1::TaskTerminated { terminal: audit_terminal(status)? },
    (from, to) => return Err(AuditContractError::TransitionMismatch {
        from: from.cloned(),
        to: to.clone(),
    }),
}
```

Before matching, require same task, checked `old_revision + 1`, monotonic timestamps, and exact receipts. For `None`, require revision zero and `Created`; do not accept a pre-collapsed `Running` snapshot.

- [ ] **Step 6: Define the async append boundary and bounded errors**

Add:

```rust
pub trait AuditAppendSink: Send + Sync {
    fn append(
        &self,
        envelope: AuditEnvelopeV1,
    ) -> Pin<Box<dyn Future<Output = Result<AuditAppendReceiptV1, AuditAppendError>> + Send + '_>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditAppendReceiptV1 {
    pub event_id: AuditEventId,
    pub sequence: u64,
    pub record_sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditFailureCategory {
    Unavailable,
    LockContended,
    AppendPreCommitFailure,
    AppendVisibleDurabilityUncertain,
    UnsupportedSchema,
    CorruptJournal,
    SequenceOverflow,
    JournalTooLarge,
    CorrelationMismatch,
    TransitionMismatch,
    ReconciliationRequired,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("audit append failed: {category:?}")]
pub struct AuditAppendError {
    pub category: AuditFailureCategory,
}

impl AuditAppendError {
    pub fn from_category(category: AuditFailureCategory) -> Self {
        Self { category }
    }
}
```

The public error carries no path or native source. Concrete app errors may retain an `io::Error` privately.

- [ ] **Step 7: Remove the dormant runtime audit vocabulary and run focused tests**

Delete `runtime::AuditEvent` and only its two serialization tests. Do not modify `RunEvent`.

Run:

```bash
rtk cargo test -p rollshot-agent audit
rtk cargo test -p rollshot-agent authority
rtk cargo test -p rollshot-agent product_task
rtk cargo test -p rollshot-agent runtime
```

Expected: PASS; `AuditEventV1` is the sole audit vocabulary and all adjacent privacy tests remain green.

- [ ] **Step 8: Commit the domain contract**

```bash
rtk git add crates/rollshot-agent/src/audit.rs crates/rollshot-agent/src/lib.rs crates/rollshot-agent/src/authority.rs crates/rollshot-agent/src/product_task.rs crates/rollshot-agent/src/runtime.rs
rtk git commit -m "feat(agent): define durable audit contracts"
```

### Task 2: Build the hash-chained per-task JSONL journal

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/workbench/audit_store/mod.rs`
- Create: `crates/rollshot-app/src/result_workspace/workbench/audit_store/record.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/mod.rs:1-10`
- Test: inline `#[cfg(test)]` modules in both new files

**Interfaces:**
- Consumes: `AuditEnvelopeV1`, `AuditEventId`, `AuditAppendReceiptV1`, `AuditFailureCategory`, `ProductTaskId`, existing task-ID path rules, `sha2`, and `serde_json`.
- Produces:
  - `AuditJournal::open(PathBuf) -> Result<Self, AuditStoreError>`.
  - `AuditJournal::scan(&ProductTaskId) -> Result<VerifiedJournal, AuditStoreError>`.
  - `AuditJournal::append(&ProductTaskId, JournalPayloadV1) -> Result<PhysicalAppendReceipt, AuditStoreError>`; caller holds the TaskStore lock.
  - private `JournalRecordV1`, `JournalPayloadV1`, `PreparedTransactionV1`, `TransactionOutcomeV1`, and `VerifiedJournal`.

- [ ] **Step 1: Add failing physical-record golden and chain tests**

Add exact tests:

```rust
#[test]
fn first_record_has_sequence_zero_no_previous_hash_and_stable_digest() {
    let record = JournalRecordV1::build(
        task_id(),
        0,
        None,
        aborted_payload_fixture(),
    )
    .unwrap();
    assert_eq!(record.sequence, 0);
    assert_eq!(record.previous_record_sha256, None);
    assert_eq!(
        record.record_sha256,
        "8f792de3e8acb943a8e96cd21800712647ce5dd102d51d9b61736e04679f165f"
    );
}

#[test]
fn second_record_binds_previous_hash() {
    let first =
        JournalRecordV1::build(task_id(), 0, None, aborted_payload_fixture()).unwrap();
    let second = JournalRecordV1::build(
        task_id(),
        1,
        Some(first.record_sha256.clone()),
        committed_payload_fixture(),
    )
    .unwrap();
    assert_eq!(second.previous_record_sha256.as_deref(), Some(first.record_sha256.as_str()));
    assert!(second.verify(Some(&first)).is_ok());
}

#[test]
fn changed_interior_payload_breaks_hash_validation() {
    let mut bytes = two_record_journal_bytes();
    replace_same_length_ascii(&mut bytes, b"task_created", b"task_changed");
    assert!(matches!(
        scan_bytes(&bytes),
        Err(AuditStoreError::CorruptJournal { .. })
    ));
}
```

The fixed DTO and domain separator produce the digest above; any later change
requires an intentional schema/golden update.

- [ ] **Step 2: Run the record tests and confirm they fail**

```bash
rtk cargo test -p rollshot-app audit_store::record --lib
```

Expected: FAIL because `audit_store` and physical record types do not exist.

- [ ] **Step 3: Implement fixed physical record and canonical hashing**

Use these private shapes:

```rust
const JOURNAL_SCHEMA_VERSION_V1: u32 = 1;
const MAX_RECORD_BYTES: usize = 64 * 1024;
const MAX_JOURNAL_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct JournalRecordV1 {
    schema_version: u32,
    task_id: ProductTaskId,
    sequence: u64,
    previous_record_sha256: Option<String>,
    payload: JournalPayloadV1,
    record_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "record_kind", rename_all = "snake_case")]
enum JournalPayloadV1 {
    Prepared(PreparedTransactionV1),
    Committed { transaction_id: AuditTransactionId, event_id: AuditEventId },
    Aborted {
        transaction_id: AuditTransactionId,
        event_id: AuditEventId,
        reason: AuditAbortCategory,
    },
    Standalone { envelope: AuditEnvelopeV1 },
    Bootstrap { receipt: AuditTaskStateReceiptV1, observed_at_unix_ms: i64 },
}
```

Hash fixed canonical body fields with `b"rollshot-audit-journal-record-v1\0"`. Require lowercase 64-byte SHA-256 hex and checked sequence increment.

- [ ] **Step 4: Add failing file-safety, append, reopen, and partial-tail tests**

```rust
#[test]
fn acknowledged_append_survives_fresh_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let journal = AuditJournal::open(dir.path().join("audit")).unwrap();
    let receipt = journal.append(&task_id(), standalone_payload_fixture()).unwrap();
    drop(journal);
    let reopened = AuditJournal::open(dir.path().join("audit")).unwrap();
    let verified = reopened.scan(&task_id()).unwrap();
    assert_eq!(verified.last_sequence(), Some(receipt.sequence));
    assert_eq!(verified.last_record_sha256(), Some(receipt.record_sha256.as_str()));
}

#[test]
fn only_unterminated_final_fragment_is_repairable() {
    let dir = journal_with_two_complete_records_and_tail(b"{\"schema_version\":1");
    let verified = AuditJournal::open(dir.path().join("audit"))
        .unwrap()
        .scan_and_repair_tail(&task_id())
        .unwrap();
    assert_eq!(verified.records().len(), 2);
    assert!(journal_bytes(&dir, &task_id()).ends_with(b"\n"));
}

#[test]
fn malformed_complete_interior_line_fails_closed() {
    let dir = journal_with_lines(&[valid_line(0), b"{}\n".to_vec(), valid_line(2)]);
    assert!(matches!(
        AuditJournal::open(dir.path().join("audit")).unwrap().scan(&task_id()),
        Err(AuditStoreError::CorruptJournal { .. })
    ));
}
```

Also test symlink, directory/FIFO special file on Unix, filename/task-ID mismatch, unsupported schema, record/journal oversize, duplicate sequence, sequence gap, and hash mismatch.

- [ ] **Step 5: Implement safe scan and tail repair**

Implementation order:

```rust
fn scan_reader<R: BufRead>(
    mut reader: R,
    mut visit: impl FnMut(&JournalRecordV1) -> Result<(), AuditStoreError>,
) -> Result<ScanResult, AuditStoreError> {
    let mut line = Vec::with_capacity(4 * 1024);
    let mut state = VerifiedJournal::default();
    let mut total_bytes = 0_u64;

    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .map_err(AuditStoreError::read)?;
        if read == 0 {
            break;
        }
        total_bytes = total_bytes
            .checked_add(read as u64)
            .ok_or(AuditStoreError::JournalTooLarge)?;
        if total_bytes > MAX_JOURNAL_BYTES {
            return Err(AuditStoreError::JournalTooLarge);
        }
        if line.last() != Some(&b'\n') {
            return Ok(ScanResult::with_repairable_tail(state, read));
        }
        if line.len() - 1 > MAX_RECORD_BYTES {
            return Err(AuditStoreError::RecordTooLarge);
        }
        let record: JournalRecordV1 = serde_json::from_slice(&line[..line.len() - 1])
            .map_err(|_| AuditStoreError::CorruptJournal {
                category: "complete_record_decode",
            })?;
        state.observe(&record)?;
        visit(&record)?;
    }

    Ok(ScanResult::complete(state))
}
```

Use `symlink_metadata`, require a regular file, bound metadata length before
opening, and call `scan_reader(BufReader<File>, |_| Ok(()))`. `VerifiedJournal`
stores only the last sequence/hash plus unresolved transaction state; it does
not retain every physical record. The test/support event reader supplies a
visitor that collects committed logical envelopes. Truncate only the reported
unterminated final fragment after every complete line has validated.

- [ ] **Step 6: Implement durable append and failpoints**

Open a fresh append/write handle for each record, write exactly one serialized
record plus newline, call `sync_all`, sync the audit directory after first file
creation, then drop the handle before returning. Execute the whole operation in
`spawn_blocking`; do not cache handles or batch acknowledgements. Add test-only
failpoints:

```rust
pub enum AuditFailpoint {
    RecordWrite,
    FileSync,
    DirectorySync,
    VisibleBeforeSync,
}
```

On `VisibleBeforeSync`, re-read and classify exact bytes as `AppendVisibleDurabilityUncertain`; never report `Committed`.

- [ ] **Step 7: Run journal contract tests**

```bash
rtk cargo test -p rollshot-app audit_store::record --lib
rtk cargo test -p rollshot-app audit_store::tests --lib
```

Expected: PASS, including fresh-reopen durability and every corruption/failpoint case.

- [ ] **Step 8: Commit the journal primitive**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/audit_store crates/rollshot-app/src/result_workspace/workbench/mod.rs
rtk git commit -m "feat(app): add append-only task audit journal"
```

### Task 3: Add prepare/commit reconciliation and audited TaskStore methods

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/workbench/audit_store/reconcile.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/audit_store/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/audit_store/record.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/task_store.rs:29-141,144-599,601-777,799-855`
- Test: inline task-store and reconcile tests

**Interfaces:**
- Consumes: Task 1 `derive_material_transition` and Task 2 journal append/scan.
- Produces:
  - `TaskStore::create_audited(&ProductTaskSnapshot, AuditEventId, i64) -> Result<AuditedCommitOutcome, TaskStoreError>`; accepts only `Created` revision zero.
  - `TaskStore::transition_audited(&ProductTaskSnapshot, &ProductTaskSnapshot, AuditEventId, i64) -> Result<AuditedCommitOutcome, TaskStoreError>`.
  - `TaskStore::append_standalone_audit(AuditEnvelopeV1) -> Result<AuditAppendReceiptV1, TaskStoreError>`.
  - `TaskStore::reconcile_task_audit(&ProductTaskId) -> Result<(), TaskStoreError>` resolves an existing physical transaction using its stored IDs/bytes; callers never rebuild an uncertain envelope.
  - `TaskStore::committed_audit_events(&ProductTaskId) -> Result<Vec<AuditEnvelopeV1>, TaskStoreError>` for tests/support evidence only, not UI reconstruction.
  - `AuditedCommitOutcome { store: StoreCommitOutcome, audit: AuditAppendReceiptV1 }`.
  - raw snapshot methods become private `create_snapshot_locked` and `compare_and_swap_snapshot_locked`.

- [ ] **Step 1: Add failing pure reconciliation matrix tests**

Cover every state:

```rust
#[test]
fn unresolved_prepare_commits_when_exact_replacement_is_authoritative() {
    let prepared = prepared_fixture(4, 5, transition_receipt(5));
    assert_eq!(
        classify_unresolved(&prepared, Some(&task_receipt(5))).unwrap(),
        ReconcileDecision::Commit
    );
}

#[test]
fn unresolved_prepare_aborts_when_expected_revision_remains_authoritative() {
    let prepared = prepared_fixture(4, 5, transition_receipt(5));
    assert_eq!(
        classify_unresolved(&prepared, Some(&task_receipt(4))).unwrap(),
        ReconcileDecision::Abort(AuditAbortCategory::StateNotCommitted)
    );
}

#[test]
fn unresolved_prepare_rejects_unrelated_revision() {
    let prepared = prepared_fixture(4, 5, transition_receipt(5));
    assert!(matches!(
        classify_unresolved(&prepared, Some(&task_receipt(6))),
        Err(AuditStoreError::ReconciliationRequired { .. })
    ));
}
```

Add create cases: absent → abort, exact revision-zero receipt → commit, mismatched task/receipt → corrupt.

- [ ] **Step 2: Run reconciliation tests and confirm failure**

```bash
rtk cargo test -p rollshot-app audit_store::reconcile --lib
```

Expected: FAIL because the reconcile module and decisions do not exist.

- [ ] **Step 3: Implement transaction IDs, prepared receipts, and pure classification**

Use an opaque `audit-tx-<uuid>` ID. `PreparedTransactionV1` contains:

```rust
struct PreparedTransactionV1 {
    transaction_id: AuditTransactionId,
    envelope: AuditEnvelopeV1,
    expected_revision: Option<u32>,
    replacement_revision: u32,
    replacement_receipt: AuditTaskStateReceiptV1,
}
```

`classify_unresolved` compares task ID, exact expected/replacement revision, and the complete privacy-safe replacement receipt. It never accepts `current_revision > replacement_revision` and never mutates state.

- [ ] **Step 4: Add failing audited create/transition crash-window tests**

```rust
#[test]
fn audited_create_persists_prepare_snapshot_and_commit() {
    let (store, _dir) = store();
    let created = created_task_fixture();
    store.create_audited(&created, audit_id(1), 10).unwrap();
    assert_eq!(event_kinds(&store, created.task_id()), vec![AuditEventV1::TaskCreated]);
    assert_eq!(store.load(created.task_id()).unwrap(), created);
}

#[test]
fn crash_after_prepare_before_snapshot_resolves_aborted() {
    let (store, dir) = store_with_audit_failpoint(AuditedFailpoint::AfterPrepare);
    let created = created_task_fixture();
    assert!(matches!(
        store.create_audited(&created, audit_id(1), 10),
        Err(TaskStoreError::InjectedCrash)
    ));
    drop(store);
    let reopened = TaskStore::open(dir.path()).unwrap();
    assert!(reopened.committed_audit_events(created.task_id()).unwrap().is_empty());
    assert!(matches!(reopened.load(created.task_id()), Err(TaskStoreError::NotFound { .. })));
}

#[test]
fn crash_after_snapshot_before_audit_commit_repairs_commit() {
    let (store, dir) = store_with_audit_failpoint(AuditedFailpoint::AfterSnapshotCommit);
    let created = created_task_fixture();
    assert!(store.create_audited(&created, audit_id(1), 10).is_err());
    drop(store);
    let reopened = TaskStore::open(dir.path()).unwrap();
    assert_eq!(event_kinds(&reopened, created.task_id()), vec![AuditEventV1::TaskCreated]);
}

#[test]
fn same_process_reconciliation_reuses_uncertain_transaction_identity() {
    let (store, _dir) =
        store_with_audit_failpoint(AuditedFailpoint::CommitVisibleBeforeSync);
    let created = created_task_fixture();
    let event_id = audit_id(7);
    assert!(matches!(
        store.create_audited(&created, event_id.clone(), 10),
        Err(TaskStoreError::AuditDurabilityUncertain { .. })
    ));
    store.clear_failpoint_for_test();
    store.reconcile_task_audit(created.task_id()).unwrap();
    let events = store.committed_audit_events(created.task_id()).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_id(), &event_id);
}
```

Also test failed snapshot CAS → aborted, commit-visible-durability-uncertain exact re-read → committed, mismatch → reconciliation-required, duplicate same transaction idempotency, and conflicting identity corruption.

- [ ] **Step 5: Refactor existing snapshot writes under one lock without changing raw semantics**

Extract private methods that require an already-held lock guard:

```rust
fn create_snapshot_locked(
    &self,
    snapshot: &ProductTaskSnapshot,
) -> Result<StoreCommitOutcome, TaskStoreError>;

fn compare_and_swap_snapshot_locked(
    &self,
    expected: &ProductTaskSnapshot,
    replacement: &ProductTaskSnapshot,
) -> Result<StoreCommitOutcome, TaskStoreError>;
```

Add this diagram as a module doc-comment beside the lock-owning composition:

```text
lock .lock
  ├─ append Prepared + sync
  ├─ create/CAS authoritative snapshot
  └─ append Committed|Aborted + sync
unlock

No AuditJournal method acquires .lock.
```

Acquire `.lock` once in each public audited method. Do not let `AuditJournal` acquire another lock. This fixed lock order is the stop condition from the spec.

- [ ] **Step 6: Implement prepare → snapshot → commit/abort**

The exact public flow is:

```rust
pub fn transition_audited(
    &self,
    expected: &ProductTaskSnapshot,
    replacement: &ProductTaskSnapshot,
    event_id: AuditEventId,
    occurred_at_unix_ms: i64,
) -> Result<AuditedCommitOutcome, TaskStoreError> {
    let envelope = derive_material_transition(
        Some(expected),
        replacement,
        event_id,
        occurred_at_unix_ms,
    )?;
    let _guard = self.lock_exclusive()?;
    self.ensure_task_reconciled_locked(expected.task_id())?;
    let prepared = self.audit.prepare_locked(expected, replacement, envelope)?;
    match self.compare_and_swap_snapshot_locked(expected, replacement) {
        Ok(store) => self.audit.commit_locked(prepared, store),
        Err(error) if error.proves_no_visible_commit() => {
            self.audit.abort_locked(prepared, AuditAbortCategory::StateNotCommitted)?;
            Err(error)
        }
        Err(error) => Err(self.classify_uncertain_locked(prepared, error)?),
    }
}
```

`create_audited` uses the same record ordering but has an explicit absent-state
branch:

```rust
pub fn create_audited(
    &self,
    created: &ProductTaskSnapshot,
    event_id: AuditEventId,
    occurred_at_unix_ms: i64,
) -> Result<AuditedCommitOutcome, TaskStoreError> {
    let envelope =
        derive_material_transition(None, created, event_id, occurred_at_unix_ms)?;
    let _guard = self.lock_exclusive()?;
    self.ensure_task_reconciled_locked(created.task_id())?;
    let prepared = self.audit.prepare_create_locked(created, envelope)?;
    match self.create_snapshot_locked(created) {
        Ok(store) => self.audit.commit_locked(prepared, store),
        Err(error) if error.proves_no_visible_commit() => {
            self.audit
                .abort_locked(prepared, AuditAbortCategory::StateNotCommitted)?;
            Err(error)
        }
        Err(error) => Err(self.classify_uncertain_locked(prepared, error)?),
    }
}
```

The transition derivation rejects any created snapshot that is not exactly
`Created` at revision zero.

- [ ] **Step 7: Reconcile protocol journals during open before returning TaskStore**

`TaskStore::open` must:

1. create/validate `tasks/` and `audit/`;
2. clean repairable final fragments;
3. scan every validated journal deterministically;
4. resolve every prepared transaction against the matching task snapshot;
5. bootstrap existing active/reviewable pre-Slice-6 tasks with a physical `Bootstrap` record; and
6. return only when no journal is unresolved/uncertain/corrupt.

A bootstrap is not returned by `committed_audit_events` and does not invent prior event timestamps.

`reconcile_task_audit(task_id)` runs the same scan/classification path under the
store lock and appends only the missing outcome record with the original
transaction/event identity. Call it after a same-process uncertain result or
from startup; never ask UI/driver code to retry with a new `AuditEventId`.

- [ ] **Step 8: Run TaskStore and crash-reopen tests**

```bash
rtk cargo test -p rollshot-app audit_store::reconcile --lib
rtk cargo test -p rollshot-app task_store --lib
```

Expected: PASS. Existing exact CAS, visibility, path, reconciliation, and pruning tests remain green.

- [ ] **Step 9: Commit the audited persistence boundary**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/audit_store crates/rollshot-app/src/result_workspace/workbench/task_store.rs
rtk git commit -m "feat(app): coordinate task state with audit evidence"
```

### Task 4: Migrate task creation, attempt, run-contract, artifact, and terminal transitions

**Files:**
- Modify: `crates/rollshot-agent/src/product_task.rs:1136-1157` to reconcile abandoned `Created` state.
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs:755-935,995-1082,1180-1220,1420-1480`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/task_store.rs:601-738`
- Test: inline tests in `run.rs` and `task_store.rs`

**Interfaces:**
- Consumes: Task 3 `create_audited`, `transition_audited`, and committed event reader.
- Produces: production Smart Redaction persistence order `TaskCreated → AttemptStarted → RunContractBound → ArtifactPromoted` or `TaskTerminated`; every event uses a fresh stable ID generated once per attempted material transition.

- [ ] **Step 1: Add a failing full pre-review event-order test**

```rust
#[tokio::test]
async fn smart_redaction_persists_ordered_pre_review_audit_events() {
    let fixture = successful_author_run_fixture();
    let terminal = fixture.run_to_ready_for_review().await;
    assert!(matches!(terminal, RunTerminalState::ReadyForReview(_)));
    assert_eq!(
        fixture.committed_event_kinds(),
        vec![
            AuditEventV1::TaskCreated,
            AuditEventV1::AttemptStarted,
            AuditEventV1::RunContractBound,
            AuditEventV1::ArtifactPromoted,
        ]
    );
}
```

Assert the `RunContractBound` authority/skill references exactly equal the receipts copied into promoted artifact metadata.

- [ ] **Step 2: Add failure tests before changing production callsites**

Add tests proving:

```rust
#[tokio::test]
async fn audit_prepare_failure_prevents_model_dispatch() {
    let fixture = SmartRedactionAuditFixture::new()
        .fail_event(AuditEventKindV1::TaskCreated, AuditFailpoint::RecordWrite);
    let outcome = fixture.run().await;
    assert!(matches!(outcome, Err(WorkbenchError::StorePersist { .. })));
    assert_eq!(fixture.provider_call_count(), 0);
}

#[tokio::test]
async fn artifact_audit_failure_prevents_ready_for_review_delivery() {
    let fixture = SmartRedactionAuditFixture::new()
        .fail_event(AuditEventKindV1::ArtifactPromoted, AuditFailpoint::FileSync);
    let terminal = fixture.run().await;
    assert!(!matches!(terminal, Ok(RunTerminalState::ReadyForReview(_))));
    assert!(!fixture.committed_event_kinds().contains(&AuditEventV1::ArtifactPromoted));
}

#[tokio::test]
async fn partial_provider_failure_never_emits_artifact_promoted() {
    let fixture = SmartRedactionAuditFixture::new().with_partial_provider_failure();
    let terminal = fixture.run().await.unwrap();
    assert!(matches!(terminal, RunTerminalState::ProviderFailure { .. }));
    assert!(!fixture.committed_event_kinds().contains(&AuditEventV1::ArtifactPromoted));
}

#[tokio::test]
async fn stale_run_contract_never_emits_run_contract_or_artifact_event() {
    let fixture = SmartRedactionAuditFixture::new().with_stale_skill_digest();
    assert!(fixture.run().await.is_err());
    let kinds = fixture.committed_event_kinds();
    assert!(!kinds.contains(&AuditEventV1::RunContractBound));
    assert!(!kinds.contains(&AuditEventV1::ArtifactPromoted));
}

#[tokio::test]
async fn missing_task_store_fails_before_provider_dispatch() {
    let fixture = SmartRedactionAuditFixture::new().without_task_store();
    let outcome = fixture.run().await;
    assert!(matches!(outcome, Err(WorkbenchError::StorePersist { .. })));
    assert_eq!(fixture.provider_call_count(), 0);
}

#[test]
fn crash_between_created_and_attempt_reconciles_to_interrupted() {
    let fixture = SmartRedactionAuditFixture::new()
        .fail_event(AuditEventKindV1::AttemptStarted, AuditFailpoint::RecordWrite);
    let task_id = fixture.run_until_failure();
    let reopened = fixture.reopen_store();
    reopened
        .reconcile_for_source(fixture.source_binding(), fixture.now())
        .unwrap();
    assert_eq!(
        reopened.load(&task_id).unwrap().status(),
        &TaskStatus::Interrupted
    );
    assert_eq!(
        fixture.committed_events_after_reopen(),
        vec![AuditEventV1::TaskCreated, AuditEventV1::TaskTerminated {
            terminal: AuditTaskTerminalV1::Interrupted,
        }]
    );
}
```

Use deterministic fake provider and audit failpoints; do not assert source text.

- [ ] **Step 3: Run the new integration tests and verify the collapsed create fails**

```bash
rtk cargo test -p rollshot-app smart_redaction_persists_ordered_pre_review_audit_events --lib -- --nocapture
rtk cargo test -p rollshot-app audit_failure --lib -- --nocapture
```

Expected: FAIL because production persists one collapsed `Running` snapshot and uses raw CAS.

- [ ] **Step 4: Split initial persistence into two audited commits**

Replace the current in-memory `Created → Running` collapse with:

```rust
let Some(store) = task_store.as_ref() else {
    yield crate::result_workspace::Message::Workbench(
        super::WorkbenchMessage::RunFailed {
            task_id: task_id.clone(),
            run_id: run_id.clone(),
            error: WorkbenchError::StorePersist {
                message: "audit store unavailable".to_owned(),
            },
        },
    );
    return;
};
let created =
    ProductTaskSnapshot::new_v2(task_id.clone(), task_kind, source_binding, now)?;
store.create_audited(&created, AuditEventId::new_v4(), now)?;
let running = created.start_attempt(attempt, now)?;
store.transition_audited(&created, &running, AuditEventId::new_v4(), now)?;
```

Only after both acknowledgements may capability loading or provider dispatch start. Cache the persisted `running` snapshot for the run-contract CAS.

- [ ] **Step 5: Route run-contract binding through audited transition**

At the current `bind_run_contract` persistence block, call:

```rust
let bound = current.bind_run_contract(run_contract_receipt, bound_at)?;
store.transition_audited(&current, &bound, AuditEventId::new_v4(), bound_at)?;
```

Do not emit a separate skill event; `RunContractBound` is the one durable skill-use transition.

- [ ] **Step 6: Route artifact promotion and all run terminals through audited transition**

Update `persist_terminal_outcome` and `persist_terminal_if_possible`:

```rust
let replacement = match terminal {
    RunTerminalState::ReadyForReview(_) => current.record_ready_for_review(
        metadata,
        payload,
        proposal_payload,
        now,
    )?,
    terminal => current.record_terminal(map_task_terminal(terminal)?, now)?,
};
store.transition_audited(&current, &replacement, AuditEventId::new_v4(), now)?;
```

`RunTerminalState::AuditFailure` is not mapped to `TaskTerminal`; leave the last authoritative snapshot unchanged so later audited startup reconciliation can mark it `Interrupted`.

- [ ] **Step 7: Audit startup interruption transitions**

Extend `ProductTaskSnapshot::reconcile_interrupted` to accept
`Created | Running | Applying`. `Created → Interrupted` has no attempt/run
correlation and must not fabricate one; `Running|Applying` retain the existing
attempt terminal update. In `reconcile_for_source`, replace direct CAS with
`transition_audited`. Generate the event ID/timestamp once, collect
reconciliation work without holding `.lock`, then perform one audited transition
at a time so no second lock is acquired.

- [ ] **Step 8: Prove no production raw create/CAS caller remains**

Use LSP references for `TaskStore::create_snapshot_locked` and `compare_and_swap_snapshot_locked`. Expected: references only inside `task_store.rs` implementation/tests. Then run:

```bash
rtk cargo test -p rollshot-app result_workspace::workbench::run --lib
rtk cargo test -p rollshot-app task_store --lib
rtk cargo test -p rollshot-agent product_task
```

Expected: PASS.

- [ ] **Step 9: Commit the pre-review migration**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/run.rs crates/rollshot-app/src/result_workspace/workbench/task_store.rs
rtk git commit -m "feat(app): audit Smart Redaction task transitions"
```

### Task 5: Audit review apply, rejection, compensation, and retention

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/update.rs:1989-2205,2600-2660`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/task_store.rs:601-777`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs` test fixtures only
- Test: inline `update.rs`, `task_store.rs`, and `run.rs` tests

**Interfaces:**
- Consumes: audited TaskStore methods and exact `ReviewReceipt`/artifact metadata.
- Produces: `ReviewApplyStarted`, `ReviewDecisionCommitted::Applied`, `ReviewDecisionCommitted::Rejected`, audited compensation to `Interrupted`, and whole expired task/journal retention.

- [ ] **Step 1: Add failing applied/rejected review-order tests**

```rust
#[test]
fn accepted_review_records_apply_start_then_product_commit() {
    let mut fixture = ready_review_fixture();
    fixture.dispatch_apply_candidates();
    fixture.complete_all_tasks();
    assert_eq!(
        fixture.committed_event_kinds(),
        vec![
            AuditEventV1::ReviewApplyStarted,
            AuditEventV1::ReviewDecisionCommitted,
        ]
    );
    let applied = fixture.last_event();
    assert!(matches!(
        applied.correlation().review(),
        Some(ReviewDecisionAuditV1::Applied { resulting_document_state_id, .. })
            if *resulting_document_state_id == fixture.document_state_id()
    ));
}

#[test]
fn rejected_review_records_one_rejected_decision() {
    let mut fixture = ready_review_fixture();
    fixture.dispatch_reject_artifact();
    fixture.complete_all_tasks();
    assert_eq!(fixture.last_event().correlation().review(), Some(&ReviewDecisionAuditV1::Rejected));
}
```

- [ ] **Step 2: Add failing side-effect and compensation tests**

Prove:

- `ReviewApplyStarted` is acknowledged before document mutation.
- apply failure uses audited `Applying → Interrupted` compensation.
- failure before apply-start audit leaves document and task `ReadyForReview`.
- failure after document mutation but before completion audit does not claim `ReviewDecisionCommitted`; authoritative task remains `Applying` until reconciliation.
- stale operation tokens do not append review events.

- [ ] **Step 3: Run review tests and verify raw CAS usage fails expectations**

```bash
rtk cargo test -p rollshot-app review_audit --lib -- --nocapture
rtk cargo test -p rollshot-app accepted_review_records_apply_start_then_product_commit --lib
```

Expected: FAIL because `update.rs` still calls raw `compare_and_swap`.

- [ ] **Step 4: Replace review CAS calls with audited transitions**

For begin/apply/reject/compensation, derive the replacement through the existing reducer first, then call:

```rust
store.transition_audited(
    &snapshot,
    &replacement,
    AuditEventId::new_v4(),
    occurred_at_unix_ms,
)
```

Do not clear pending review UI state until the audited acknowledgement returns. Reuse the existing generic `StorePersist` error path; do not add new user-facing copy.

- [ ] **Step 5: Bind the applied event to the exact available document receipt**

The event derivation must copy:

```rust
ReviewDecisionAuditV1::Applied {
    applied_candidate_ids: receipt.applied_candidates.clone(),
    rejected_candidate_ids: receipt.rejected_candidates.clone(),
    resulting_document_state_id: receipt.resulting_document_state_id
        .ok_or(AuditContractError::MissingDocumentStateReceipt)?,
    resulting_document_sha256: receipt.resulting_document_digest.map(hex_encode),
}
```

Do not invent a document digest when the current receipt has `None`. Add a residual-risk entry to the gate decision later.

- [ ] **Step 6: Add failing whole-pair retention tests**

```rust
#[test]
fn expired_terminal_task_prunes_matching_journal_as_a_whole() {
    let (store, _dir) = store();
    let terminal = persist_completed_task_with_audit(&store, THIRTY_ONE_DAYS_AGO);
    store.reconcile_for_source(terminal.source_binding(), NOW).unwrap();
    assert!(!store.task_file_exists(terminal.task_id()));
    assert!(!store.audit_file_exists(terminal.task_id()));
}

#[test]
fn active_uncertain_or_corrupt_task_journal_is_never_pruned() {
    let (store, _dir) = store();
    let active = persist_running_task_with_audit(&store, THIRTY_ONE_DAYS_AGO);
    let uncertain = persist_uncertain_task_with_audit(&store, THIRTY_ONE_DAYS_AGO);
    let corrupt = persist_corrupt_task_with_audit(&store, THIRTY_ONE_DAYS_AGO);
    let _ = store.reconcile_for_source(active.source_binding(), NOW);
    for task_id in [active.task_id(), uncertain.task_id(), corrupt.task_id()] {
        assert!(store.task_file_exists(task_id));
        assert!(store.audit_file_exists(task_id));
    }
}

#[test]
fn retention_half_delete_finishes_without_bootstrapping_false_history() {
    let (store, dir) =
        store_with_retention_failpoint(RetentionFailpoint::AfterTaskDelete);
    let terminal = persist_completed_task_with_audit(&store, THIRTY_ONE_DAYS_AGO);
    assert!(store
        .reconcile_for_source(terminal.source_binding(), NOW)
        .is_err());
    assert!(!store.task_file_exists(terminal.task_id()));
    assert!(store.audit_file_exists(terminal.task_id()));
    drop(store);
    let reopened = TaskStore::open(dir.path()).unwrap();
    assert!(!reopened.task_file_exists(terminal.task_id()));
    assert!(!reopened.audit_file_exists(terminal.task_id()));
}
```

- [ ] **Step 7: Implement retention under the existing lock**

When existing 30-day terminal pruning selects a task, validate that its journal
has no unresolved/uncertain transaction. Under the store lock, delete the task
file first and the matching journal second. If task deletion fails, retain both;
if journal deletion fails after task deletion, leave the orphan journal and
return a bounded error. Startup recognizes an absent task plus a retention-
eligible terminal receipt in that journal and completes journal deletion without
bootstrapping or rewriting history. Never delete the journal first and never
rewrite a live journal prefix.

- [ ] **Step 8: Run review and retention suites**

```bash
rtk cargo test -p rollshot-app result_workspace::update --lib
rtk cargo test -p rollshot-app task_store --lib
rtk cargo test -p rollshot-app result_workspace::workbench::run --lib
```

Expected: PASS; all review event ordering and whole-pair retention tests pass.

- [ ] **Step 9: Commit review and retention integration**

```bash
rtk git add crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/result_workspace/workbench/task_store.rs crates/rollshot-app/src/result_workspace/workbench/run.rs
rtk git commit -m "feat(app): audit review and product commit decisions"
```

### Task 6: Persist authority denial before returning the run terminal

**Files:**
- Modify: `crates/rollshot-agent/src/audit.rs`
- Modify: `crates/rollshot-agent/src/driver.rs:254-288,594-607,1379-1555`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/audit_store/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/task_store.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs:657-665,944-1479`
- Modify when exhaustive matching requires: `crates/rollshot-app/src/result_workspace/workbench/state.rs`, `crates/rollshot-app/src/result_workspace/update.rs`, `crates/rollshot-agent/src/continuity.rs`
- Test: driver and app sink integration tests

**Interfaces:**
- Consumes: `AuditAppendSink`, `AuthoritySnapshot::audit_ref`, `ToolError::AuthorityDenied`, and `TaskStore::append_standalone_audit`.
- Produces:
  - `RunTerminalState::AuditFailure { category: AuditFailureCategory }`.
  - `DriverError::AuditFailure(AuditFailureCategory)`.
  - `TaskAuditSink::new(Arc<TaskStore>) -> Self`, implementing `AuditAppendSink` with `spawn_blocking`.
  - `AgentRunner::run_with_provider` gains required parameter
    `audit_sink: &dyn AuditAppendSink` immediately after `event_sink`.

- [ ] **Step 1: Add a failing driver denial ordering test**

Use a recording sink and tool body counter:

```rust
#[tokio::test]
async fn authority_denial_is_acknowledged_before_terminal_and_tool_never_runs() {
    let sink = RecordingAuditSink::default();
    let body_calls = Arc::new(AtomicUsize::new(0));
    let terminal = run_denied_tool(&sink, body_calls.clone()).await;
    assert_eq!(body_calls.load(Ordering::SeqCst), 0);
    assert!(matches!(terminal, RunTerminalState::AgentProtocolFailure { .. }));
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].event(), AuditEventV1::AuthorityDenied { .. }));
}
```

The current terminal remains protocol failure for a successfully audited denial; only audit append failure returns `AuditFailure`.

- [ ] **Step 2: Add a failing audit-sink failure test**

```rust
#[tokio::test]
async fn authority_denial_audit_failure_returns_audit_terminal_without_tool_execution() {
    let sink = FailingAuditSink::new(AuditFailureCategory::AppendPreCommitFailure);
    let body_calls = Arc::new(AtomicUsize::new(0));
    let terminal = run_denied_tool(&sink, body_calls.clone()).await;
    assert_eq!(body_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        terminal,
        RunTerminalState::AuditFailure {
            category: AuditFailureCategory::AppendPreCommitFailure,
        }
    );
}
```

Also prove no automatic tool or provider retry occurs.

- [ ] **Step 3: Run focused driver tests and confirm failure**

```bash
rtk cargo test -p rollshot-agent authority_denial_is_acknowledged -- --nocapture
rtk cargo test -p rollshot-agent authority_denial_audit_failure -- --nocapture
```

Expected: FAIL because the runner has no audit sink or audit terminal.

- [ ] **Step 4: Match the typed denial before generic tool-error conversion**

In `run_tool_turn`, replace the generic `Err(e)` handling with a typed first arm:

```rust
Err(ToolError::AuthorityDenied { tool, operation }) => {
    let authority = authority.ok_or_else(|| {
        DriverError::AgentProtocolFailure("authority denial without snapshot".into())
    })?;
    let envelope = AuditEnvelopeV1::authority_denied(
        AuditEventId::new_v4(),
        unix_time_ms()?,
        authority.audit_ref(),
        tool,
        operation,
    )?;
    audit_sink
        .ok_or(DriverError::AuditFailure(AuditFailureCategory::Unavailable))?
        .append(envelope)
        .await
        .map_err(|error| DriverError::AuditFailure(error.category))?;
    terminal_error = Some("authority denied".to_owned());
    break;
}
```

Do not include the tool arguments, authority error string, grants, or model output in the envelope or tracing fields.

- [ ] **Step 5: Add the audit terminal through exhaustive matches**

Add:

```rust
RunTerminalState::AuditFailure { category: AuditFailureCategory }
DriverError::AuditFailure(AuditFailureCategory)
```

Map `DriverError::AuditFailure` at every authorized run loop boundary. Keep it out of `TaskTerminal`; app terminal persistence leaves the snapshot unchanged and displays the existing bounded persistence failure category.

Use compiler errors and LSP references for `RunTerminalState` to update only exhaustive matches. `continuity.rs` may carry the category in an in-memory manifest test but must not serialize journal data.

- [ ] **Step 6: Implement the app async sink without blocking Tokio workers**

```rust
pub struct TaskAuditSink {
    store: Arc<TaskStore>,
}

impl AuditAppendSink for TaskAuditSink {
    fn append(
        &self,
        envelope: AuditEnvelopeV1,
    ) -> Pin<Box<dyn Future<Output = Result<AuditAppendReceiptV1, AuditAppendError>> + Send + '_>> {
        let store = self.store.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || store.append_standalone_audit(envelope))
                .await
                .map_err(|_| AuditAppendError::from_category(AuditFailureCategory::Unavailable))?
                .map_err(TaskStoreError::into_public_audit_error)
        })
    }
}
```

`append_standalone_audit` acquires the existing store lock, verifies reconciliation, appends/syncs one `Standalone` record, and returns only after acknowledgement.

- [ ] **Step 7: Inject the sink into the active Smart Redaction runner**

Construct one `TaskAuditSink` from the same `Arc<TaskStore>` used for Product Task persistence and pass it to `run_with_provider`. Do not pass it to unrelated visual-annotation paths that have no Product Task/authority binding.

- [ ] **Step 8: Add a fresh-reopen app integration test**

Run a denied tool through `TaskAuditSink`, drop/reopen the TaskStore, and assert exactly one committed `AuthorityDenied` event with the exact task/attempt/run/authority digest, registered tool name, and `RunOperation`; assert serialized bytes omit args, grants, source, and skill body.

- [ ] **Step 9: Run provider, authority, driver, and app tests**

```bash
rtk cargo test -p rollshot-agent authority
rtk cargo test -p rollshot-agent driver
rtk cargo test -p rollshot-agent --test provider_contract
rtk cargo test -p rollshot-app authority_denial --lib
rtk cargo test -p rollshot-app result_workspace::workbench::run --lib
```

Expected: PASS; denial is durable-before-terminal and denied tool body count stays zero.

- [ ] **Step 10: Commit authority-denial observability**

```bash
rtk git add crates/rollshot-agent/src/audit.rs crates/rollshot-agent/src/driver.rs crates/rollshot-agent/src/continuity.rs crates/rollshot-app/src/result_workspace/workbench/audit_store/mod.rs crates/rollshot-app/src/result_workspace/workbench/task_store.rs crates/rollshot-app/src/result_workspace/workbench/run.rs crates/rollshot-app/src/result_workspace/workbench/state.rs crates/rollshot-app/src/result_workspace/update.rs
rtk git commit -m "feat(agent): persist authority denial evidence"
```

Stage only files actually changed; omit unchanged optional exhaustive-match files from the command.

### Task 7: Prove transient-event loss repair, privacy, corruption blocking, and callsite completeness

**Files:**
- Modify: `crates/rollshot-agent/src/audit.rs` tests
- Modify: `crates/rollshot-agent/src/driver.rs` tests
- Modify: `crates/rollshot-app/src/result_workspace/workbench/audit_store/mod.rs` tests
- Modify: `crates/rollshot-app/src/result_workspace/workbench/task_store.rs` tests
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs` tests
- Modify: `crates/rollshot-app/src/result_workspace/update.rs` tests

**Interfaces:**
- Consumes: completed production audit path from Tasks 1–6.
- Produces: acceptance evidence that audit is complete, private, crash-consistent, non-authoritative, and independent from transient UI delivery.

- [ ] **Step 1: Add a drop-all transient sink fixture**

```rust
#[derive(Debug, Default)]
struct DropAllRunEvents;

impl RunEventSink for DropAllRunEvents {
    fn emit(&self, _event: RunEvent) {}
}
```

Use it in real result-workspace run fixtures rather than directly mutating UI state.

- [ ] **Step 2: Add failing authoritative repair tests**

Prove with no delivered `RunEvent` values:

```rust
#[tokio::test]
async fn dropped_display_events_restore_ready_for_review_from_task_snapshot() {
    let fixture = ResultWorkspaceAuditFixture::new().drop_all_run_events();
    let task_id = fixture.run_to_ready_for_review().await;
    let restored = fixture.reopen_workspace();
    assert_eq!(restored.review_task_id(), Some(&task_id));
    assert!(restored.has_pending_proposal());
}

#[tokio::test]
async fn dropped_display_events_repair_terminal_from_correlated_terminal_and_task() {
    let fixture = ResultWorkspaceAuditFixture::new().drop_all_run_events();
    let task_id = fixture.run_to_terminal(TaskTerminal::Cancelled).await;
    let restored = fixture.reopen_workspace();
    assert_eq!(restored.task_status(&task_id), Some(TaskStatus::Cancelled));
    assert!(!restored.run_is_active());
}

#[test]
fn completed_rejected_stale_and_interrupted_restore_without_audit_replay() {
    let fixture = ResultWorkspaceAuditFixture::with_restorable_terminal_tasks([
        TaskStatus::Completed,
        TaskStatus::Rejected,
        TaskStatus::Stale,
        TaskStatus::Interrupted,
    ])
    .panic_on_audit_read();
    let restored = fixture.reopen_workspace();
    assert_eq!(
        restored.restored_statuses(),
        vec![
            TaskStatus::Completed,
            TaskStatus::Rejected,
            TaskStatus::Stale,
            TaskStatus::Interrupted,
        ]
    );
}
```

Instrument `committed_audit_events` with a test failpoint that panics if UI restoration calls it; restoration tests must still pass.

- [ ] **Step 3: Add failing corruption non-authority tests**

Corrupt an interior audit record, preserve a valid Product Task snapshot, reopen, and assert:

- task admission/open reports `CorruptJournal`;
- snapshot bytes are unchanged;
- no state transition is derived from the journal;
- UI does not display a fabricated terminal/review state; and
- the journal is not truncated or replaced.

- [ ] **Step 4: Add full event-variant privacy sentinels**

For every `AuditEventV1` variant, place unique sentinels in adjacent image bytes, proposal payload, prompt, provider error, tool args/result, skill body, credentials, authority grants, semantic input, path, and `RunEvent`. Serialize the domain envelope and physical journal, format `Debug`/`Display`, and capture tracing. Assert none of the sentinels appear.

- [ ] **Step 5: Run tests to expose any missing repair/privacy wiring**

```bash
rtk cargo test -p rollshot-agent audit -- --nocapture
rtk cargo test -p rollshot-app dropped_display_events --lib -- --nocapture
rtk cargo test -p rollshot-app audit_privacy --lib -- --nocapture
rtk cargo test -p rollshot-app corrupt_journal --lib -- --nocapture
```

Expected before fixes: at least one new test FAIL. If all pass immediately, mutate the test fixture once (for example, include a sentinel in a candidate field) to prove the assertion detects a plausible leak, then restore the correct fixture.

- [ ] **Step 6: Make only the minimal repair/privacy fixes**

Allowed fixes:

- remove sensitive fields from audit DTOs;
- replace native/path strings with bounded categories;
- ensure UI restore uses `TaskStore::load/reconcile_for_source` only;
- ensure corruption blocks admission without touching snapshots; and
- add stable tracing targets/structured safe fields.

Do not add audit replay, event UI, remote reporting, or a second persistence store.

- [ ] **Step 7: Prove raw persistence and dormant vocabulary are unreachable**

Use LSP references:

- `runtime::AuditEvent`: definition absent.
- `TaskStore::create_snapshot_locked`: only TaskStore internals/tests.
- `TaskStore::compare_and_swap_snapshot_locked`: only TaskStore internals/tests.
- `TaskStore::committed_audit_events`: tests/support evidence only; no view/update restoration caller.
- `RunEvent`: unchanged transient producer/consumer paths.

Treat any production bypass as a blocker; migrate it before continuing.

- [ ] **Step 8: Run affected crate regression suites**

```bash
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-app
```

If shared result-workspace compilation or tests touch feature-gated Action Guide code, also run:

```bash
rtk cargo test -p rollshot-app --features action-guide
```

Expected: PASS with no regression to continuity, authority, skills, provider contracts, review restore, or Action Guide.

- [ ] **Step 9: Commit acceptance-test hardening**

```bash
rtk git add crates/rollshot-agent/src/audit.rs crates/rollshot-agent/src/driver.rs crates/rollshot-app/src/result_workspace/workbench/audit_store crates/rollshot-app/src/result_workspace/workbench/task_store.rs crates/rollshot-app/src/result_workspace/workbench/run.rs crates/rollshot-app/src/result_workspace/update.rs
rtk git commit -m "test(agent): prove durable audit invariants"
```

### Task 8: Verify, independently review, and record the Slice 6/G3 decision

**Files:**
- Create: `docs/superpowers/spikes/2026-07-28-audit-observability-decision.md`
- Modify only if verification finds a real defect: files from Tasks 1–7

**Interfaces:**
- Consumes: all Slice 6 implementation and tests.
- Produces: reproducible gate evidence, independent review verdict, migrations/residual risks/deferred scope, and an explicit statement that umbrella G3 remains blocked until Slice 5's separate gate evidence is closed.

- [ ] **Step 1: Run focused contract and failure-injection suites**

```bash
rtk cargo test -p rollshot-agent audit
rtk cargo test -p rollshot-agent product_task
rtk cargo test -p rollshot-agent authority
rtk cargo test -p rollshot-agent --test provider_contract
rtk cargo test -p rollshot-app audit_store --lib
rtk cargo test -p rollshot-app result_workspace::workbench::run --lib
rtk cargo test -p rollshot-app result_workspace::update --lib
```

Record exact passed/failed/ignored counts in the gate decision. Any failure blocks the gate.

- [ ] **Step 2: Run affected full regression suites**

```bash
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-app
rtk cargo test -p rollshot-app --features action-guide
```

Record exact counts and confirm no stalled provider/decoder result. Do not claim platform-native runtime coverage that was not executed.

- [ ] **Step 3: Run formatting, lint, and whitespace checks**

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings
rtk proxy git diff --check
```

All must pass. Fix only Slice 6 defects; do not refactor unrelated code.

- [ ] **Step 4: Run a fresh-reopen durability measurement fixture**

Run the ignored/measurement test with output:

```bash
rtk cargo test -p rollshot-app audit_acknowledgement_survives_reopen --lib -- --ignored --nocapture
```

The test prints only safe numeric evidence:

```text
audit_records=<n> acknowledged=<n> reopened=<n> corruptions=0 unresolved=0
```

Require `acknowledged == reopened`, `corruptions == 0`, and `unresolved == 0` over deterministic prepare/CAS/commit failpoint cases. Record values in the decision document.

- [ ] **Step 5: Perform the required independent code review**

Dispatch a fresh reviewer with no implementation context. Require explicit answers to:

1. Can a completed material Product Task transition lack committed audit evidence after restart?
2. Can audit claim a transition that authoritative task state never committed?
3. Can an acknowledged interior record be silently lost, reordered, duplicated, or altered?
4. Can malformed interior data be mistaken for a repairable tail?
5. Can lock ordering deadlock TaskStore and journal operations?
6. Can any production material transition bypass audited methods?
7. Can authority denial return before durable append or enter the tool body?
8. Can audit failure promote partial provider/tool output or retry a side effect?
9. Can UI state be reconstructed from audit replay rather than authoritative state?
10. Can pixels, prose, semantic input, credentials, provider internals, full skill bodies, grants, paths, or tool args/results leak via serialization, errors, debug, or tracing?
11. Did the slice introduce event sourcing, replay, audit UI, database, global scheduling, or external publication semantics?
12. Are pre-Slice-6 bootstrap records honest about unknown history?

Any correctness/security finding must be fixed and re-reviewed. A self-review does not satisfy this step.

- [ ] **Step 6: Write the gate decision record**

Create the document with these sections and concrete evidence:

```markdown
# Gate Decision: Slice 6 Durable Audit Observability

**Status:** Verified, pending user approval
**Date:** 2026-07-28

## 1. Selected architecture
## 2. Material event and correlation matrix
## 3. Append acknowledgement and hash-chain evidence
## 4. Prepare/CAS/commit crash matrix
## 5. Startup reconciliation and retention evidence
## 6. Transient-loss repair evidence
## 7. Privacy and diagnostics inspection
## 8. Verification command results
## 9. Independent review
## 10. Migration and rollback
## 11. Residual risks
## 12. Deferred scope
## 13. Slice 5 outstanding gate evidence
## 14. Slice 6 decision and umbrella G3 status
```

Do not mark umbrella G3 complete unless all six gates, including Slice 5 approval/formal review, are directly evidenced and the user approves G3.

- [ ] **Step 7: Commit verification fixes and the decision record**

If verification required code fixes, commit each logical fix first with a `fix(agent):` or `fix(app):` message. Then commit the decision record:

```bash
rtk git add docs/superpowers/spikes/2026-07-28-audit-observability-decision.md
rtk git commit -m "docs(agent): record audit observability gate evidence"
```

- [ ] **Step 8: Stop for the user Gate G3 decision**

Present:

- Slice 6 review verdict and exact verification evidence;
- migrations, residual risks, and deferred scope;
- whether Slice 5 approval/formal independent review is now closed; and
- whether all six gates meet the umbrella completion policy.

Do not begin launch-video work, deferred capabilities, or another foundation iteration from this plan.

## Plan self-review checklist

- Slice 6 spec sections 1–17 map to Tasks 1–8.
- The material transition table maps to Task 1 derivation and Tasks 4–6 production callsites.
- Acknowledged append durability, interior-loss detection, crash reconciliation, and retention map to Tasks 2, 3, 5, and 8.
- Privacy and transient-loss requirements map to Tasks 1, 6, 7, and 8.
- No task makes audit authoritative or adds replay/UI/database/remote publication scope.
- Every new type referenced by a later task is introduced in Task 1, Task 2, or Task 3.
- Production task creation is deliberately split into `Created` then `Running`; no absent → running transition is accepted.
- `RunTerminalState::AuditFailure` remains in-memory and is not converted to a self-auditing `TaskTerminal`.
- Slice 5's unresolved approval/formal-review evidence is recorded but not absorbed.

## Engineering review record (auto mode)

### Step 0 — Scope challenge

- Goal alignment: all eight tasks directly implement or verify durable audit
  observability; no task is merely nice-to-have.
- Complexity: 5 created files, 10 potentially modified files, 8 tasks, and 2 new
  module boundaries. The review threshold is not triggered.
- Minimum viable plan: Tasks 1–7 are the minimum complete implementation; Task 8
  is mandatory umbrella gate evidence. Deferring any task breaks a stated gate.
- Built-in check: Rust provides append/write and `File::sync_all`, but no atomic
  transaction spanning the existing snapshot file and a separate journal.
  SQLite would still require cross-store coordination. Reusing TaskStore lock,
  CAS, sync, reconciliation, and retention is the smallest correct design.
- Distribution: no new binary/library/package artifact is introduced; existing
  workspace CI and application distribution remain authoritative.

### What already exists

| Existing contract | Reuse decision |
|---|---|
| `ProductTaskSnapshot` legal reducers and V2 run-contract receipts | Reuse as the only source for audit transition derivation |
| `TaskStore` exact CAS, `.lock`, sibling-temp sync/rename, visibility classification | Reuse; extract private lock-held primitives instead of adding a second lock/store |
| `TaskStore::reconcile_for_source` and 30-day terminal pruning | Extend for audited interruption and whole-pair retention |
| `AuthoritySnapshot` digest and typed `ToolError::AuthorityDenied` | Reuse; derive a bounded denial reference without grants |
| `SkillUseReceiptV1` copied into task/artifact provenance | Reuse; `RunContractBound` is the one skill-use event |
| lossy `RunEventSink` plus terminal/task UI repair | Preserve; add drop-all evidence rather than audit replay |
| dormant `runtime::AuditEvent` used only by tests | Replace cleanly; do not keep a second convention |

### NOT in scope

- Audit replay or event-sourced Product Tasks — snapshots remain authoritative.
- Audit UI, search, export, reconnect, or remote telemetry — no approved product
  requirement.
- External Save/Export or Action Guide publication events — no Product Task
  binding exists.
- SQLite/global segmented database — one bounded task family does not justify a
  second storage engine.
- Group commit, background append queue, cached journal handles — material
  events are low-frequency and require direct acknowledgement.
- Signatures, encryption/key management, remote attestation — no approved trust
  or compliance model.
- Whole-journal adversarial tamper resistance — V1 detects accidental
  loss/reorder/change, not attacker rewrite.
- Slice 5 gate repair or launch-video work — separate approvals remain required.

### Test coverage table

| Task / behavior | Unit | Integration | E2E / smoke | Manual only |
|---|:---:|:---:|:---:|:---:|
| 1 / IDs, envelope, bounded references, legal transition table | ✓ | — | — | no |
| 2 / canonical record, chain, streaming scan, durable append | ✓ | ✓ fresh reopen | — | no |
| 3 / prepare-CAS-outcome and startup/same-process recovery | ✓ | ✓ real temp files | — | no |
| 4 / task, attempt, run-contract, artifact, terminal order | ✓ | ✓ fake provider | ✓ active Smart Redaction fixture | no |
| 5 / review apply/reject/compensation and pair retention | ✓ | ✓ document/store | ✓ result-workspace fixture | no |
| 6 / authority denial before terminal and async app sink | ✓ | ✓ driver/store | ✓ denied-tool fixture | no |
| 7 / transient loss, corruption blocking, privacy sentinels | ✓ | ✓ reopen/restore | ✓ drop-all event fixture | no |
| 8 / affected regressions, lint, durability measurement | — | ✓ | ✓ full affected flow | destructive power-loss only |

All tests use fixed timestamps, temp directories, fake providers, deterministic
failpoints, and synthetic product state. None require network, native capture,
GUI interaction, or sleeps.

### Failure-mode matrix

| New codepath / realistic failure | Test | Handling | User-visible result |
|---|---|---|---|
| Envelope has stale/mismatched task/run/artifact receipt | Task 1 Steps 1–5 | `AuditContractError::{CorrelationMismatch,TransitionMismatch}` | bounded persistence failure; no state change |
| Audit directory/file permission or disk write failure | Task 2 Steps 4–7 | `AppendPreCommitFailure` | run/review does not claim success |
| Sync returns after bytes become visible but durability is uncertain | Task 2 Step 6; Task 3 Steps 4–7 | `AppendVisibleDurabilityUncertain`, blocked task, original-ID reconciliation | bounded persistence failure until repaired |
| Final unacknowledged record is torn | Task 2 Steps 4–5 | truncate only unterminated final fragment | automatic startup repair |
| Complete interior line/hash/sequence is corrupt | Task 2 Steps 4–5; Task 7 Step 3 | `CorruptJournal`, no truncation | task admission blocked; snapshot unchanged |
| Crash after prepare but before snapshot | Task 3 Step 4 | append `Aborted` from authoritative expected state | no product transition |
| Crash after snapshot but before audit commit | Task 3 Step 4 | append `Committed` from exact replacement receipt | transition visible after reconciliation |
| Crash between `TaskCreated` and `AttemptStarted` | Task 4 Steps 2, 7 | audited `Created → Interrupted` | recoverable interrupted task |
| `TaskStore` absent/corrupt at run start | Task 4 Steps 2, 4 | fail before provider dispatch | existing bounded store error |
| Authority denial append fails | Task 6 Steps 2, 4–7 | `RunTerminalState::AuditFailure`; tool body remains uncalled | bounded audit failure |
| Transient display channel drops every event | Task 7 Steps 1–2 | restore from terminal/task/document state | repaired terminal/review display |
| Retention stops after task deletion | Task 5 Steps 6–7 | startup removes orphan journal; never bootstrap | cleanup completes, no false history |

No listed production failure is untested, unhandled, and silent.

### Task dependencies and execution topology

| Task | Modules touched | Depends on |
|---|---|---|
| 1 | `crates/rollshot-agent/` | — |
| 2 | `crates/rollshot-app/result_workspace/workbench/audit_store/` | 1 |
| 3 | app audit store + TaskStore | 1, 2 |
| 4 | agent Product Task + app TaskStore/run | 1, 3 |
| 5 | app TaskStore/update/run | 3, 4 |
| 6 | agent audit/driver + app audit store/TaskStore/run | 1, 3, 4 |
| 7 | agent audit/driver + app TaskStore/run/update | 1–6 |
| 8 | all affected modules + gate document | 1–7 |

Sequential execution, no parallelization opportunity. Tasks share contract
modules and form a strict dependency chain. Task 8's independent reviewer is a
review gate, not a parallel implementation lane. No task modifies workspace
membership or root dependency declarations.

### Auto decisions applied

- D1: require reconciled TaskStore before provider dispatch.
- D2: reconcile abandoned `Created` tasks to audited `Interrupted`.
- D3: stream journal records with bounded per-record memory.
- D4: recover uncertain appends inside TaskStore with original identities.
- D5: add focused Run/Expected checkpoints after every implementation layer.
- D6: add deterministic unavailable-store, Created-crash, and retention
  half-delete tests.
- D7: retain sync-per-record semantics through `spawn_blocking`; no batching or
  cached handles.
- D8: execute all implementation tasks sequentially.

### Engineering review completion summary

```text
Plan reviewed:           docs/superpowers/plans/2026-07-28-agent-foundation-audit-observability.md
Tasks in plan:           8
Files Create/Modify:     5 create / 10 modify

- Step 0: Scope Challenge   — accepted as-is
- Architecture Review:       4 issues, all auto-resolved
- Plan Structure + Code Q:   1 issue, Run/Expected matrix added
- Test Review:               table produced, 3 negative gaps added
- Performance Review:        1 issue, streaming scan selected
- NOT in scope:              written
- What already exists:       written
- Failure modes:             0 critical gaps
- Parallelization:           1 sequential lane, 0 parallel lanes
- Unresolved decisions:      0
```

Plan is locked for execution with
`superpowers:subagent-driven-development` or `superpowers:executing-plans`.
