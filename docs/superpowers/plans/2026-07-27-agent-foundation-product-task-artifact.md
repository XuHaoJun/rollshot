# Product Task and Artifact Promotion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every Smart Redaction request a durable Product Task/attempt/run identity, atomically promote `ReadyForReview` into a content- and revision-bound artifact, restore only compatible pending review, and persist truthful stale/review outcomes.

**Architecture:** Framework-neutral private-field task/artifact reducers and canonical V1 digest contracts live in `rollshot-agent`; `rollshot-app` owns an exact-CAS, one-file-per-task store under `<config>/agent-tasks`. Every filesystem operation runs in `spawn_blocking`; all iced completions carry task/run/operation identity. Review apply is an explicit asynchronous reducer sequence around synchronous `ImageDocument::apply_batch`, with commit-point-aware compensation.

**Tech Stack:** Rust 2024, serde/serde_json, SHA-256, UUID v4, fs4, sibling-temp + fsync + rename persistence, Tokio, iced 0.14, `iced_test::Simulator`, ImageMagick AE diff, existing `rollshot-agent`, `rollshot-edit-proposal`, and `ImageDocument` contracts.

## Global Constraints

- One Product Task is one user-authorized Smart Redaction request with one attempt in this slice.
- Product Task, attempt, run, proposal, artifact, and artifact revision identities are distinct.
- Source binding is base-image SHA-256 + canonical annotation-state SHA-256 + document state ID; state ID alone is never sufficient.
- The app owns IDs, timestamps, task/artifact/review/document truth, and storage; `AgentRunner` owns only live execution.
- Persist `running` before vision/provider/tool work and persist terminal/artifact before correlated iced delivery.
- Store transitions consume an exact expected snapshot and increment `snapshot_revision`; no status-only update API.
- Distinguish pre-commit failure, committed success, and commit-visible directory-sync uncertainty.
- Every store call uses `spawn_blocking`; no filesystem work in iced `view()` or synchronous update paths.
- Every run event/failure/terminal and restore/review completion is correlated; stale messages are ignored.
- Reconcile `running`/`applying` to `interrupted`; never resume or infer partial success.
- Reject source/digest/provenance mismatch before document mutation; never restamp or silently rebase.
- Review receipts bind exact artifact revision, actual post-apply state, and typed local modifications/manual additions.
- Persist no pixels, user/assistant text, transcript, credentials, raw OCR, unrestricted tool payload, absolute path, cancellation handle, or Rig/provider-native state.
- Clear pending payload only after commit-visible completed/rejected/stale transition; prune terminal metadata after 30 days.
- Enforce 4 MiB task-file limit from metadata before allocation; do not introduce an arbitrary task-count hard stop.
- Preserve current provider-neutral boundary, serial tools, budgets, cancellation, validation, dry-run, and manual review behavior.
- Do not add retries, DAG/scheduler, child agents, jobs, expected-output contracts, publication, transcript persistence, or durable audit events.
- No Smart Redaction layout/copy/theme/token change and no capture-overlay change.
- Runtime diagnostics use privacy-safe stable `rollshot::agent_task::*` targets.
- Before implementation use `test-driven-development`; before iced work use `testing-iced-ui` and `iced-rs`; before completion use `verification-before-completion` and `requesting-code-review`.
- The product-changing agent never writes or approves golden baselines; clean-context semantic review owns raw visual evidence acceptance.
- Do not create a worktree; repository policy requires explicit user request.

---

## What already exists

- `RunTerminalState::ReadyForReview` carries validated automation, dry-run evidence, proposal, generation, usage, session, and assistant text.
- `EditProposal`, `ValidatedAutomation`, and `ReviewDecision` serialize; `ImageDocument::apply_batch` is atomic in memory and `undo()` restores state ID.
- Slice 1 added host-owned provider bounds and incomplete-stream rejection.
- `ToolContext` currently misuses `SessionId` as run provenance, creates `ProposalId(1)`, and binds document state `0`.
- `restamp_proposal` currently erases original source binding; `ReviewDecision` currently receives the pre-apply state despite its post-apply contract.
- `AddManualCandidate` and candidate moves create local review deltas after agent artifact production; these must remain supported.
- Result Workspace already has deterministic Simulator/image-artifact infrastructure at 1100×760 and 640×420.
- iced 0.14 provides `Task::future`, `Task::perform`, and `Task::run`; ordering is still product-owned through correlated reducer messages.
- `rollshot-preset` demonstrates lock/temp/fsync/rename layout, but its parent-directory sync is best effort. Reuse the pattern, not its error semantics or domain.
- `ResultWorkspace` has no durable logical document ID and every `ImageDocument` may begin at state `0`; content binding must supplement state ID.

## NOT in scope

- Multiple attempts, automatic/UI retry, provider fallback, or handoff.
- Workflow dependencies, scheduler, child agents, parallel execution, or jobs.
- Action Guide task/artifact migration.
- Durable provider/tool/Rig/`AgentSession` resume.
- Durable `ImageDocument` persistence; applying intent prevents false approval but does not preserve unsaved edits after process death.
- Artifact revision editing/republication; local review deltas belong to receipts, not a new promoted revision.
- Indexed/database-backed store or arbitrary task-count ceiling before measured scale requires one.
- User-configurable retention/archive/delete UI.
- Publication/export/expected-output receipts.
- Durable audit log/event replay (Slice 6).
- New visual design or golden baseline.

## File Structure

### Create

- `crates/rollshot-agent/src/product_task.rs` — private-field task/artifact schemas, canonical V1 bytes/digests, reducers, promotion, validation, contract tests.
- `crates/rollshot-app/src/result_workspace/workbench/task_store.rs` — app-owned locked exact-CAS store, commit outcomes, reconciliation, pruning, permissions, failpoints, tests.
- `docs/superpowers/spikes/2026-07-27-product-task-artifact-decision.md` — proposed Gate G1 evidence after implementation.

### Modify — identity migration

- `crates/rollshot-agent/src/domain.rs`
- `crates/rollshot-edit-proposal/src/proposal.rs`
- `crates/rollshot-edit-proposal/src/review.rs`
- `crates/rollshot-edit-proposal/src/policy.rs`
- `crates/rollshot-agent/src/tools.rs`
- `crates/rollshot-action/src/caption_proposal.rs`
- `crates/rollshot-action/src/visual_annotation_proposal.rs`
- `crates/rollshot-app/src/result_workspace/canvas.rs`
- `crates/rollshot-app/src/result_workspace/workbench/eval/layer2.rs`
- `crates/rollshot-app/src/result_workspace/workbench/review.rs`
- `crates/rollshot-app/src/result_workspace/workbench/run.rs`
- `crates/rollshot-app/src/result_workspace/workbench/state.rs`
- `crates/rollshot-app/src/timeline_workspace/annotation.rs`
- `crates/rollshot-app/src/timeline_workspace/caption_agent.rs`
- `crates/rollshot-app/src/timeline_workspace/mod.rs`
- `crates/rollshot-app/src/timeline_workspace/update.rs`
- `crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs`
- `crates/rollshot-automation/tests/executor_contract.rs`
- `crates/rollshot-automation/tests/output_contract.rs`
- `crates/rollshot-automation-rquickjs/tests/end_to_end.rs`
- `crates/rollshot-automation-rquickjs/tests/resources.rs`
- `crates/rollshot-vision/tests/integration.rs`
- `crates/rollshot-vision/tests/ocr_integration.rs`
- `crates/rollshot-vision/tests/region_features.rs`

### Modify — Product Task implementation/integration

- `crates/rollshot-agent/Cargo.toml` — direct SHA-256 dependency.
- `crates/rollshot-agent/src/lib.rs` — export contract module.
- `crates/rollshot-agent/src/runtime.rs` — serde only for bounded persisted enum/scalars actually used.
- `crates/rollshot-agent/src/driver.rs` — serializable bounded dry-run scalars and promotion-compatible handoff.
- `crates/rollshot-app/Cargo.toml` — direct UUID v4 dependency.
- `crates/rollshot-app/src/result_workspace/mod.rs` — cached base-image digest and UI evidence tests.
- `crates/rollshot-app/src/result_workspace/workbench/mod.rs` — store module, correlated messages, task/review operation state.
- `crates/rollshot-app/src/result_workspace/update.rs` — ID allocation, async run/restore/review reducer integration.
- `crates/rollshot-app/src/result_workspace/workbench/view.rs` — only if needed to disable existing gestures during review I/O; no copy/layout change.
- `Cargo.lock` — dependency feature resolution.

No new crate or root workspace member is created.

---

### Task 1: Migrate RunId, ProposalId, and agent provenance to opaque strings

**Files:** all 24 paths under “Modify — identity migration”.

**Interfaces:**
- Produces: validated serde-transparent `RunId(String)` and `ProposalId(String)` with `parse`/`as_str`; `ProvenanceSource::Agent { run_id: String }`.
- Preserves: numeric proposal-local `CandidateId` and every existing Action Guide/automation/vision behavior.

- [ ] **Step 1: Write RED ID tests**

```rust
#[test]
fn run_id_requires_run_uuid_prefix() {
    let id = RunId::parse("run-00000000-0000-4000-8000-000000000001").unwrap();
    assert_eq!(id.as_str(), "run-00000000-0000-4000-8000-000000000001");
    assert!(RunId::parse("proposal-00000000-0000-4000-8000-000000000001").is_err());
    assert!(RunId::parse("run-../escape").is_err());
}

#[test]
fn proposal_id_serde_rejects_wrong_prefix() {
    let id = ProposalId::parse("proposal-00000000-0000-4000-8000-000000000002").unwrap();
    assert_eq!(serde_json::from_str::<ProposalId>(&serde_json::to_string(&id).unwrap()).unwrap(), id);
    assert!(serde_json::from_str::<ProposalId>(r#""task-00000000-0000-4000-8000-000000000002""#).is_err());
}
```

- [ ] **Step 2: Run RED**

```bash
rtk cargo test -p rollshot-agent run_id_requires_run_uuid_prefix -- --nocapture
rtk cargo test -p rollshot-edit-proposal proposal_id_serde_rejects_wrong_prefix -- --nocapture
```

Expected: compile failures because existing IDs are numeric and have no parser.

- [ ] **Step 3: Implement strict ID parsing independently in each owning crate**

Use the same algorithm (not a cross-crate helper that would create a cycle):

```rust
fn valid_uuid_suffix(value: &str, prefix: &str) -> bool {
    let Some(suffix) = value.strip_prefix(prefix) else { return false };
    suffix.len() == 36 && suffix.bytes().enumerate().all(|(i, b)| match i {
        8 | 13 | 18 | 23 => b == b'-',
        _ => b.is_ascii_hexdigit(),
    })
}
```

Custom `Deserialize` calls `parse`, so persisted invalid IDs fail at decode.
Change agent provenance to `Agent { run_id: String }`.

- [ ] **Step 4: Mechanically migrate every constructor/fixture**

Use stable deterministic UUID literals per fixture. Add local helpers where a
file has multiple constructions:

```rust
fn proposal_id(n: u128) -> ProposalId {
    ProposalId::parse(format!("proposal-{n:08x}-0000-4000-8000-000000000000")).unwrap()
}
```

For `RunId`, use `run-...`; for `ProvenanceSource::Agent`, use
`run_id.as_str().to_owned()`. Do not cast UUID strings to numeric hashes.

- [ ] **Step 5: Verify the whole migration**

```bash
rtk rg -n 'ProposalId\([0-9]|RunId::new\([0-9]|run_id: .*session_id' crates --glob '*.rs'
rtk cargo test -p rollshot-edit-proposal
rtk cargo test -p rollshot-automation
rtk cargo test -p rollshot-automation-rquickjs
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-vision --no-default-features
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-app
```

Expected: search has no production/fixture legacy constructor; all affected suites pass.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-agent/src/domain.rs crates/rollshot-edit-proposal/src crates/rollshot-agent/src/tools.rs crates/rollshot-action/src/caption_proposal.rs crates/rollshot-action/src/visual_annotation_proposal.rs crates/rollshot-app/src/result_workspace crates/rollshot-app/src/timeline_workspace crates/rollshot-automation/tests crates/rollshot-automation-rquickjs/tests crates/rollshot-vision/tests
rtk git commit -m "refactor(agent): distinguish run and proposal identities"
```

---

### Task 2: Add Product Task reducers and canonical V1 digest contracts

**Files:**
- Create: `crates/rollshot-agent/src/product_task.rs`
- Modify: `crates/rollshot-agent/Cargo.toml`
- Modify: `crates/rollshot-agent/src/lib.rs`
- Modify: `crates/rollshot-agent/src/runtime.rs`
- Modify: `crates/rollshot-agent/src/driver.rs`

**Interfaces:**
- Produces: `ProductTaskId`, `TaskAttemptId`, `ArtifactId`, `ArtifactRevision`, `DocumentContentBinding`, `ProductTaskSnapshot`, `TaskStatus`, `TaskTerminal`, `ProductArtifactMetadata`, `SmartRedactionReviewPayload`, `LocalReviewDeltaV1`, `ReviewReceipt`, `PromotionContext`, canonical digest helpers, and reducer methods.
- All snapshot fields are private; read-only accessors expose required values.

- [ ] **Step 1: Write RED reducer and privacy tests**

```rust
#[test]
fn reducers_increment_exact_snapshot_revision() {
    let created = created_task_fixture();
    let running = created.start_attempt(attempt_fixture(), 20).unwrap();
    assert_eq!(created.snapshot_revision(), 0);
    assert_eq!(running.snapshot_revision(), 1);
    assert_eq!(running.status(), TaskStatus::Running);
}

#[test]
fn terminal_task_cannot_restart() {
    let cancelled = running_task_fixture().record_terminal(TaskTerminal::Cancelled, 30).unwrap();
    assert!(matches!(cancelled.start_attempt(attempt_fixture(), 40), Err(TaskContractError::IllegalTransition { .. })));
}

#[test]
fn terminal_transition_clears_pending_payload() {
    let ready = ready_task_fixture();
    let rejected = ready.reject(reject_receipt_fixture(), 40).unwrap();
    assert!(rejected.pending_artifact_payload().is_none());
    assert!(rejected.artifact_metadata().is_some());
}
```

Add running/applying→interrupted, timestamp regression, mismatched attempt/run/proposal/artifact, review revision mismatch, and custom `Debug` no-payload tests.

- [ ] **Step 2: Write RED canonicalization/content-binding tests**

```rust
#[test]
fn canonical_config_digest_ignores_map_insertion_order_and_excludes_secret() {
    let a = config_fixture([("b", "2"), ("a", "1")], Some("secret-a"));
    let b = config_fixture([("a", "1"), ("b", "2")], Some("secret-b"));
    assert_eq!(canonical_config_digest(&a).unwrap(), canonical_config_digest(&b).unwrap());
    assert!(!String::from_utf8(canonical_config_bytes(&a).unwrap()).unwrap().contains("secret"));
}

#[test]
fn document_binding_distinguishes_same_state_id_on_different_images() {
    let a = document_binding([1; 32], annotations_fixture(), 0).unwrap();
    let b = document_binding([2; 32], annotations_fixture(), 0).unwrap();
    assert_ne!(a, b);
}

#[test]
fn canonical_payload_rejects_non_finite_values() {
    let mut payload = payload_fixture();
    payload.dry_run.affected_area = f32::NAN;
    assert_eq!(canonical_payload_bytes(&payload), Err(CanonicalError::NonFiniteFloat));
}
```

Add fixed expected byte/digest goldens for V1 payload, proposal, config, source, and annotation-state DTOs.

- [ ] **Step 3: Run RED**

```bash
rtk cargo test -p rollshot-agent product_task::tests -- --nocapture
```

Expected: missing module/types/helpers.

- [ ] **Step 4: Implement private schemas and pure reducers**

Add direct `sha2 = "0.10"`. Keep `UsageSnapshot` out of persistence unless a receipt field explicitly needs a bounded scalar; serialize `BudgetDimension` and `DryRunEvidence` only where used.

Core API:

```rust
impl ProductTaskSnapshot {
    pub fn new(... ) -> Result<Self, TaskContractError>;
    pub fn start_attempt(&self, attempt: TaskAttempt, now: i64) -> Result<Self, TaskContractError>;
    pub fn record_ready_for_review(&self, metadata: ProductArtifactMetadata, payload: SmartRedactionReviewPayload, now: i64) -> Result<Self, TaskContractError>;
    pub fn record_terminal(&self, terminal: TaskTerminal, now: i64) -> Result<Self, TaskContractError>;
    pub fn begin_apply(&self, now: i64) -> Result<Self, TaskContractError>;
    pub fn complete_apply(&self, receipt: ReviewReceipt, now: i64) -> Result<Self, TaskContractError>;
    pub fn reject(&self, receipt: ReviewReceipt, now: i64) -> Result<Self, TaskContractError>;
    pub fn mark_stale(&self, now: i64) -> Result<Self, TaskContractError>;
    pub fn reconcile_interrupted(&self, now: i64) -> Result<Option<Self>, TaskContractError>;
}
```

Each reducer clones private state, validates exact relation/timestamp/status, increments revision once, and clears payload only on terminal result.

- [ ] **Step 5: Implement one canonical V1 boundary**

Use fixed DTOs, `BTreeMap`, finite checks, bounded collections/strings, and `serde_json::to_vec` only inside `canonical_v1_bytes`. SHA-256 lowercase hex is produced by one helper. `RunConfigFingerprintV1` contains provider/model/payload mode/run kind/budget only—never keys, paths, prompts, OCR, or environment.

`DocumentContentBinding` stores cached base-image digest, canonical annotation-state digest, and state ID. Promotion recomputes payload/proposal/source/config receipts and rejects every identity/content disagreement.

- [ ] **Step 6: Run GREEN**

```bash
rtk cargo test -p rollshot-agent product_task::tests -- --nocapture
rtk cargo test -p rollshot-agent
```

Expected: reducer, golden digest, adversarial, and privacy tests pass.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-agent/Cargo.toml crates/rollshot-agent/src/lib.rs crates/rollshot-agent/src/product_task.rs crates/rollshot-agent/src/runtime.rs crates/rollshot-agent/src/driver.rs
rtk git commit -m "feat(agent): add product task artifact contracts"
```

---

### Task 3: Implement exact-CAS TaskStore with commit-point outcomes

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/workbench/task_store.rs`
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/mod.rs`
- Modify: `Cargo.lock`

**Interfaces:**
- Produces: `TaskStore::{open, create, load, compare_and_swap, reconcile_for_source}`, `StoreCommitOutcome::{Committed, CommitVisibleDurabilityUncertain}`, `TaskStoreError`, deterministic test failpoints, UUID ID generation.

- [ ] **Step 1: Add UUID and RED CAS/atomic tests**

Add `uuid = { version = "1", features = ["v4"] }`.

```rust
#[test]
fn stale_same_status_writer_loses_exact_cas() {
    let store = store();
    let expected = running_task_fixture();
    store.create(&expected).unwrap();
    let first = expected.record_terminal(TaskTerminal::Cancelled, 20).unwrap();
    store.compare_and_swap(&expected, &first).unwrap();
    let second = expected.record_terminal(TaskTerminal::RuntimeFailure, 21).unwrap();
    assert!(matches!(store.compare_and_swap(&expected, &second), Err(TaskStoreError::Conflict)));
}

#[test]
fn post_rename_sync_failure_is_commit_visible_not_precommit() {
    let store = store_with_failpoint(Failpoint::DirectorySync);
    let expected = ready_task_fixture();
    store.create_without_failpoint(&expected).unwrap();
    let replacement = expected.begin_apply(20).unwrap();
    assert_eq!(store.compare_and_swap(&expected, &replacement).unwrap(), StoreCommitOutcome::CommitVisibleDurabilityUncertain);
    assert_eq!(store.load(expected.task_id()).unwrap(), replacement);
}
```

Also test temp write, file sync, and rename failures preserve old snapshot; oversize metadata boundary (4 MiB accepted, 4 MiB+1 rejected before read); corrupt/schema/digest/symlink/non-regular rejection; Unix 0700/0600; lock/CAS concurrency; reconcile/prune/temp cleanup; and many-file linear scan without hard stop.

- [ ] **Step 2: Run RED**

```bash
rtk cargo test -p rollshot-app task_store::tests -- --nocapture
```

Expected: missing store module/API.

- [ ] **Step 3: Implement safe path/read/permissions**

Validate ID before joining. Use `symlink_metadata`, require regular file, reject size before `Vec` allocation, and custom bounded error `Debug`/`Display`. On Unix create directory/file modes 0700/0600 and test actual modes; on other platforms retain deny-symlink/non-regular checks.

- [ ] **Step 4: Implement exact CAS and commit classification**

Under one fs4 exclusive lock:

1. load/validate current;
2. require `current == expected`;
3. require replacement same task and revision `expected + 1`;
4. serialize/validate ≤4 MiB;
5. unique sibling temp, write, file sync;
6. rename;
7. parent sync.

Errors before successful rename are `PreCommit`. If parent sync fails, re-read and compare replacement; matching bytes return `CommitVisibleDurabilityUncertain`; mismatch/corruption returns a typed integrity failure that callers do not treat as safe rollback.

- [ ] **Step 5: Implement source-scoped reconciliation/maintenance**

`reconcile_for_source(binding, now)`:

- scans deterministic sorted task files with no arbitrary count error;
- reconciles running/applying snapshots by CAS;
- prunes terminal metadata older than exactly 30 days;
- deletes only store temp-prefix files;
- ignores unrelated base-image digests;
- marks same-base-image but mismatching annotation-state pending reviews stale by CAS; and
- returns newest fully compatible ready review.

- [ ] **Step 6: Run GREEN**

```bash
rtk cargo test -p rollshot-app task_store::tests -- --nocapture
```

Expected: CAS, all commit boundaries, permissions, corruption, resource, reconciliation, and privacy tests pass.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/Cargo.toml Cargo.lock crates/rollshot-app/src/result_workspace/workbench/mod.rs crates/rollshot-app/src/result_workspace/workbench/task_store.rs
rtk git commit -m "feat(app): persist product tasks with exact cas"
```

---

### Task 4: Bind real identities/content and persist run outcomes before delivery

**Files:**
- Modify: `crates/rollshot-agent/src/tools.rs`
- Modify: `crates/rollshot-agent/src/product_task.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`

**Interfaces:**
- Produces: cached base-image digest, exact run/proposal/document binding in `ToolContext`, `RunPersistenceContext`, correlated `RunEvent/RunFailed/RunTerminal`, persist-before-delivery helper.

- [ ] **Step 1: Write RED identity/content-flow tests**

```rust
assert_eq!(proposal.id, ProposalId::parse(PROPOSAL_ID).unwrap());
assert_eq!(proposal.base_document_state_id, 42);
assert_eq!(proposal.provenance.source, ProvenanceSource::Agent { run_id: RUN_ID.to_owned() });
```

Add two state-0 documents with different base-image digests and assert promotion metadata differs. Add cached base digest constructor test.

- [ ] **Step 2: Write RED ordering/correlation tests**

Use temp store and scripted provider:

- running snapshot exists before vision/provider setup;
- setup failure persists bounded terminal before `RunFailed`;
- ready artifact exists before `RunTerminal` is observed;
- store pre-commit failure delivers no proposal;
- old task/run `RunEvent`, `RunFailed`, and `RunTerminal` do not mutate a newer active run.

- [ ] **Step 3: Run RED**

```bash
rtk cargo test -p rollshot-agent dry_run_uses_run_proposal_and_content_binding -- --nocapture
rtk cargo test -p rollshot-app running_is_persisted_before_setup -- --nocapture
rtk cargo test -p rollshot-app ready_artifact_precedes_correlated_terminal -- --nocapture
rtk cargo test -p rollshot-app stale_run_messages_are_ignored -- --nocapture
```

Expected: missing identity fields/persistence protocol.

- [ ] **Step 4: Cache base digest and allocate IDs exactly once**

Add cached `[u8; 32]` to `ResultWorkspace`, computed in constructors from immutable RGBA bytes. When Send is accepted, allocate task/run/proposal/artifact UUID IDs once and capture canonical content/config binding in `PendingRunParams`; disclosure messages reuse those IDs.

- [ ] **Step 5: Bind ToolContext and promote ReadyForReview**

Add immutable `run_id`, `proposal_id`, and `DocumentContentBinding` to `ToolContext`; delete `ProposalId(1)`, state `0`, and session-derived provenance. Promotion validates every relation and canonical receipt, excludes terminal prose/bytes, then reducer-produces ready snapshot.

- [ ] **Step 6: Persist every outcome in spawn_blocking before message delivery**

Correlated messages:

```rust
RunEvent { task_id: ProductTaskId, run_id: RunId, event: RunEvent },
RunFailed { task_id: ProductTaskId, run_id: RunId, error: WorkbenchError },
RunTerminal { task_id: ProductTaskId, run_id: RunId, terminal: RunTerminalState },
```

Create running snapshot before setup. Use one async persistence helper for setup failures, joined terminals, and promoted ready artifact. `Committed` and commit-visible uncertainty permit correlated delivery (the latter adds bounded warning); pre-commit failure yields only bounded store failure and no proposal. Join panic remains running for reconciliation.

- [ ] **Step 7: Run GREEN/regressions**

```bash
rtk cargo test -p rollshot-agent dry_run_uses_run_proposal_and_content_binding -- --nocapture
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-app running_is_persisted_before_setup -- --nocapture
rtk cargo test -p rollshot-app ready_artifact_precedes_correlated_terminal -- --nocapture
rtk cargo test -p rollshot-app stale_run_messages_are_ignored -- --nocapture
```

Expected: all pass; no uncorrelated production run message remains.

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-agent/src/tools.rs crates/rollshot-agent/src/product_task.rs crates/rollshot-app/src/result_workspace/mod.rs crates/rollshot-app/src/result_workspace/workbench/mod.rs crates/rollshot-app/src/result_workspace/workbench/run.rs crates/rollshot-app/src/result_workspace/update.rs
rtk git commit -m "feat(agent): promote review artifacts before delivery"
```

---

### Task 5: Restore only source-compatible reviews with operation tokens

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/state.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`

**Interfaces:**
- Produces: `RestoreOperationId`, `TaskRestoreFinished { operation_id, source_binding, result }`, source-scoped restore, stale-message guards.

- [ ] **Step 1: Write RED restore/adversarial-order tests**

Test:

1. matching content restores exact artifact without provider call;
2. unrelated image with same state ID is ignored and its task remains ready;
3. same image changed annotations marks only related task stale;
4. running/applying becomes interrupted and is not restored;
5. old restore completion delivered after a new restore/run is ignored.

```rust
assert!(workbench(&workspace).pending_proposal.is_none());
assert_eq!(store.load(unrelated.task_id()).unwrap().status(), TaskStatus::ReadyForReview);
```

- [ ] **Step 2: Run RED**

```bash
rtk cargo test -p rollshot-app restore_compatible_review -- --nocapture
rtk cargo test -p rollshot-app same_state_different_image_is_ignored -- --nocapture
rtk cargo test -p rollshot-app stale_restore_completion_is_ignored -- --nocapture
```

Expected: missing restore token/source-scoped API.

- [ ] **Step 3: Add explicit restore state/messages**

Workbench stores active task/run/artifact IDs, source binding, task-store root, and current restore operation. `Message::SmartRedaction` enters workbench immediately and launches `spawn_blocking(store.reconcile_for_source(...))`. No I/O occurs in view/update.

- [ ] **Step 4: Handle completion only on exact token/content match**

Before populating proposal/draft/review state, verify operation ID, cached base digest, current canonical annotation-state digest, task/artifact digest, and current mode. New restore/run invalidates old token. Stale related artifact is already CAS-marked by store; unrelated task is untouched.

- [ ] **Step 5: Run GREEN**

```bash
rtk cargo test -p rollshot-app restore_compatible_review -- --nocapture
rtk cargo test -p rollshot-app same_state_different_image_is_ignored -- --nocapture
rtk cargo test -p rollshot-app stale_restore_completion_is_ignored -- --nocapture
rtk cargo test -p rollshot-app workbench -- --nocapture
```

Expected: all source-scoped and ordering tests pass.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/mod.rs crates/rollshot-app/src/result_workspace/workbench/state.rs crates/rollshot-app/src/result_workspace/update.rs
rtk git commit -m "feat(app): restore source-bound review artifacts"
```

---

### Task 6: Persist review decisions through a nonblocking apply reducer

**Files:**
- Modify: `crates/rollshot-agent/src/product_task.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/state.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/review.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify only if required to disable existing actions: `crates/rollshot-app/src/result_workspace/workbench/view.rs`

**Interfaces:**
- Produces: `ReviewOperationId`, applying/receipt/compensation/reject correlated messages, exact artifact verification, `LocalReviewDeltaV1`, truthful post-state receipt, commit-aware rollback.

- [ ] **Step 1: Write RED domain review tests**

```rust
#[test]
fn apply_rejects_stale_without_mutation() {
    let proposal = proposal_for_state(7);
    let mut document = document_at_state(8);
    let before = document.state_id();
    assert_eq!(apply_candidates(&proposal, &review_all(&proposal), &mut document), Err(WorkbenchError::StaleArtifact));
    assert_eq!(document.state_id(), before);
}

#[test]
fn apply_returns_actual_post_state_and_local_additions() {
    let (proposal, review) = review_with_moved_and_manual_candidate(7);
    let mut document = document_at_state(7);
    let outcome = apply_candidates(&proposal, &review, &mut document).unwrap();
    assert_eq!(outcome.decision.resulting_document_state_id, document.state_id());
    assert_eq!(outcome.local_delta.added_candidates.len(), 1);
    assert_eq!(outcome.local_delta.modified_candidates.len(), 1);
}
```

Delete tests for `restamp_proposal`; add zero-op result (no state change/no undo requirement).

- [ ] **Step 2: Write RED asynchronous reducer/failure-matrix tests**

Cover:

- Apply starts async CAS ready→applying and disables candidate/document gestures;
- stale ApplyingPersisted/ReceiptPersisted tokens are ignored;
- document changed while applying write pending aborts before apply and compensates;
- final pre-commit failure after mutation undoes and restores exact original state/task ready;
- zero-op pre-commit failure does not call undo;
- commit-visible directory-sync uncertainty does not undo completed document;
- undo failure or compensation failure leaves applying and bounded error, no assert/panic;
- discard persists reject before clearing UI;
- manual additions/modifications appear in receipt bound to original artifact revision.

- [ ] **Step 3: Run RED**

```bash
rtk cargo test -p rollshot-app apply_rejects_stale_without_mutation -- --nocapture
rtk cargo test -p rollshot-app apply_returns_actual_post_state_and_local_additions -- --nocapture
rtk cargo test -p rollshot-app receipt_precommit_failure_rolls_back -- --nocapture
rtk cargo test -p rollshot-app commit_visible_receipt_failure_does_not_undo -- --nocapture
rtk cargo test -p rollshot-app stale_review_completion_is_ignored -- --nocapture
```

Expected: current synchronous/restamped apply fails.

- [ ] **Step 4: Remove restamping and return a typed apply outcome**

`apply_candidates` verifies original proposal state, validates current review edits/manual additions, lowers original + local delta, applies one batch, and returns `ReviewApplyOutcome { decision, local_delta, pre_state, post_state }`. It builds final `ReviewDecision` only after successful apply. No product-path `assert!`.

- [ ] **Step 5: Implement the three-phase iced reducer**

```text
Apply requested(token) -> spawn_blocking CAS Ready→Applying
ApplyingPersisted(token) -> recheck IDs/content; apply_batch; spawn_blocking CAS Applying→Completed
ReceiptPersisted(token) -> clear UI on Committed/CommitVisible
```

All candidate gestures and other document mutations are disabled/ignored while operation is active. Pre-commit final failure performs guarded undo only when `post_state != pre_state`, then async compensation CAS. Commit-visible uncertainty keeps document/task completed and surfaces warning. Reject uses its own token/persist-before-clear flow.

- [ ] **Step 6: Run GREEN**

```bash
rtk cargo test -p rollshot-app apply_rejects_stale_without_mutation -- --nocapture
rtk cargo test -p rollshot-app apply_returns_actual_post_state_and_local_additions -- --nocapture
rtk cargo test -p rollshot-app receipt_ -- --nocapture
rtk cargo test -p rollshot-app stale_review_completion_is_ignored -- --nocapture
rtk cargo test -p rollshot-app discard_candidates_persists_rejection -- --nocapture
rtk cargo test -p rollshot-edit-proposal
```

Expected: entire commit/rollback/manual-delta matrix passes.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-agent/src/product_task.rs crates/rollshot-app/src/result_workspace/workbench/mod.rs crates/rollshot-app/src/result_workspace/workbench/state.rs crates/rollshot-app/src/result_workspace/workbench/review.rs crates/rollshot-app/src/result_workspace/workbench/view.rs crates/rollshot-app/src/result_workspace/update.rs
rtk git commit -m "fix(app): bind review receipts to artifact revisions"
```

Omit `view.rs` if unchanged.

---

### Task 7: Prove iced restoration/staleness behavior and visual parity

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Modify only if a stable existing selector is needed without copy/layout change: `crates/rollshot-app/src/result_workspace/workbench/view.rs`
- Create during verification only (never commit): `target/ui-artifacts/product-task-artifact/*`

**Interfaces:** structural Simulator assertions and expected/restored/diff PNGs at 1100×760 and 640×420; no golden path.

- [ ] **Step 1: Re-run visual capability preflight**

Semantically inspect the URL-bar fixture and report capability/provider/probe/pixel diff/CI block. If semantic inspection is unavailable, use a capable clean reviewer or explicit human mode; do not self-accept pixels.

- [ ] **Step 2: Write RED structural interaction tests**

For expected in-memory and actual restored states at both sizes:

```rust
let apply = ui.find("Apply 1 redactions").expect("exact apply button");
assert_eq!(apply.bounds(), apply.visible_bounds().unwrap());
let messages = ui.click("Apply 1 redactions").expect("click apply");
assert!(messages.iter().any(|m| matches!(m, Message::Workbench(WorkbenchMessage::ApplyCandidates))));
```

Use stable candidate widget ID/fixture-controlled exact label, not `find("1")`. For stale state assert exact Apply button is absent/disabled and click emits no apply message.

- [ ] **Step 3: Run RED then GREEN**

```bash
rtk cargo test -p rollshot-app restored_review_matches_existing_review_structure -- --nocapture
rtk cargo test -p rollshot-app stale_restored_review_has_no_apply_action -- --nocapture
```

Expected: RED before test fixture/restoration support; GREEN without visual layout changes.

- [ ] **Step 4: Generate exact expected/restored/diff evidence**

Render pinned fonts, Dark theme, and both viewports:

```bash
rtk cargo test -p rollshot-app render_product_task_restore_visual_evidence -- --ignored --nocapture
rtk bash -lc 'set -e; for size in 1100x760 640x420; do metric=$(compare -metric AE target/ui-artifacts/product-task-artifact/expected-$size.png target/ui-artifacts/product-task-artifact/restored-$size.png target/ui-artifacts/product-task-artifact/diff-$size.png 2>&1 || true); test "$metric" = 0; done'
```

Expected: AE exactly `0` for both. Artifacts include expected, restored, and diff for each size; no baseline update command/path exists.

- [ ] **Step 5: Clean-context semantic visual review**

Provide only requirement, auto mode, changed files, scenario manifest, structural output, six image paths, allowed baselines `none`, update command `none`. Reviewer inspects every image and accepts/rejects. Fix and repeat on rejection.

- [ ] **Step 6: Commit tests**

```bash
rtk git add crates/rollshot-app/src/result_workspace/mod.rs crates/rollshot-app/src/result_workspace/workbench/view.rs
rtk git commit -m "test(app): cover restored agent review state"
```

Omit unchanged `view.rs`; never add `target/ui-artifacts`.

---

### Task 8: Verify, independently review, and propose Gate G1

**Files:**
- Modify only for accepted review fixes: Tasks 1–7 files.
- Create: `docs/superpowers/spikes/2026-07-27-product-task-artifact-decision.md`

**Interfaces:** clean test/privacy/visual/review evidence and proposed—not passed—Gate G1 decision.

- [ ] **Step 1: Run targeted and affected suites**

```bash
rtk cargo test -p rollshot-edit-proposal
rtk cargo test -p rollshot-automation
rtk cargo test -p rollshot-automation-rquickjs
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-vision --no-default-features
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-app task_store::tests -- --nocapture
rtk cargo test -p rollshot-app restored_review -- --nocapture
rtk cargo test -p rollshot-app receipt_ -- --nocapture
rtk cargo test -p rollshot-app
```

Expected: all pass; no ignored required behavioral test.

- [ ] **Step 2: Run privacy/schema/legacy audits**

```bash
rtk rg -n 'ProposalId\([0-9]|RunId::new\([0-9]|run_id: .*session_id|restamp_proposal|base_document_state_id: 0' crates --glob '*.rs'
rtk rg -n 'assistant_text|user_message|attachment_bytes|api_key|ocr_text|rig_core|provider_response' crates/rollshot-agent/src/product_task.rs crates/rollshot-app/src/result_workspace/workbench/task_store.rs
rtk rg -n 'rollshot::agent_task::' crates/rollshot-agent/src crates/rollshot-app/src/result_workspace
```

Expected: first search has no active/fixture legacy usage (explicit isolated test zero must be justified); second has no persisted-field leak and custom privacy tests cover false-positive test names; tracing targets are stable/bounded.

- [ ] **Step 3: Run quality verification**

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings
rtk git diff --check
rtk git status --short
```

Expected: all pass. OCR is excluded because it has a dedicated provisioned lane and this slice does not touch it; affected vision no-default tests already ran.

- [ ] **Step 4: Independent code review**

Invoke `requesting-code-review` with umbrella, child spec, this plan, commits, UI evidence/verdict, and ask:

- Can any run/setup path start before durable running?
- Can any terminal/UI review appear before commit-visible artifact transition?
- Can same-state different-image content restore or stale the wrong task?
- Can stale writers bypass exact CAS?
- Does directory-sync uncertainty ever trigger contradictory undo?
- Can late iced messages mutate a newer task/operation?
- Can document changes race applying/receipt phases?
- Are manual additions/modifications fully represented in receipt?
- Do files/logs/debug exclude forbidden private data and obey 0700/0600?
- Did scope stay one Smart Redaction task without DAG/retry/job/audit expansion?

Apply accepted fixes as sole writer; rerun Steps 1–3 and Task 7 evidence if UI behavior changed; commit scoped fixes.

- [ ] **Step 5: Write proposed Gate G1 decision**

Create the decision with `Status: Proposed for user approval`, exact identity/content trace, CAS and commit-boundary evidence, reconciliation, stale/cross-document tests, canonical digests, async correlation, local review delta/post-state, privacy/retention/permissions, command counts, visual verdict, independent review, residual risks, and “Slice 3 may begin only after user approval.”

- [ ] **Step 6: Verify and commit decision proposal**

```bash
rtk rg -n 'TBD|TODO|FIXME|XXX' docs/superpowers/spikes/2026-07-27-product-task-artifact-decision.md
rtk git diff --check
rtk git add docs/superpowers/spikes/2026-07-27-product-task-artifact-decision.md
rtk git commit -m "docs(agent): propose product task artifact gate decision"
```

Expected: scan empty, diff clean. Do not claim Gate G1 passed before explicit approval.

---

## Test Coverage Table

| Task / behavior | Unit | Integration | UI/smoke | Manual only |
|---|---:|---:|---:|---:|
| 1 / string ID migration + downstream fixtures | ✓ | ✓ workspace crates | — | no |
| 2 / private reducers + exact revisions | ✓ | — | — | no |
| 2 / canonical digest/config privacy/finite floats | ✓ | ✓ promotion | — | no |
| 2 / different-image same-state binding | ✓ | ✓ restore | — | no |
| 3 / exact CAS competing writers | ✓ | ✓ filesystem | — | no |
| 3 / write/sync/rename/post-rename outcomes | ✓ | ✓ filesystem | — | no |
| 3 / corrupt/oversize/symlink/mode rejection | ✓ | ✓ filesystem | — | no |
| 3 / reconciliation/prune/many-file scan | ✓ | ✓ filesystem | — | no |
| 4 / running before setup/provider | ✓ | ✓ run/store | — | no |
| 4 / promote before correlated terminal | ✓ | ✓ iced stream/store | — | no |
| 4 / stale run event/failure/terminal ignored | ✓ | ✓ reducer | — | no |
| 5 / compatible source-scoped restore | ✓ | ✓ store/update | ✓ Simulator | no |
| 5 / unrelated same-state task ignored | ✓ | ✓ store/update | — | no |
| 5 / stale restore completion ignored | ✓ | ✓ reducer | — | no |
| 6 / truthful post-state + local review delta | ✓ | ✓ document/store | — | no |
| 6 / zero-op/precommit/commit-visible/compensation matrix | ✓ | ✓ failpoint/document | — | no |
| 6 / stale review completion/mutation gate | ✓ | ✓ reducer | — | no |
| 6 / reject persisted before UI clear | ✓ | ✓ store/update | — | no |
| 7 / exact Apply interaction at two sizes | — | — | ✓ Simulator | no |
| 7 / expected vs restored AE=0 + semantic review | — | — | ✓ image evidence | no |
| 8 / existing provider/automation/vision/action/app regressions | ✓ | ✓ | ✓ app suite | no |

## Failure Modes

| New path | Failure | Handling / test | User result |
|---|---|---|---|
| ID/path | malformed/traversal ID | strict deserialize/path tests | bounded store error |
| Source lookup | another document shares state ID | base-image + annotation digest tests | unrelated task ignored |
| CAS | two stale writers | exact expected snapshot conflict | one transition wins; no merge |
| Running create | precommit write/sync/rename failure | failpoints; no setup start | run does not start |
| Directory sync | rename visible, sync fails | re-read and commit-visible outcome | warning; no contradictory rollback |
| Process crash running | unknown provider/tool effect | startup CAS interrupted | no false success |
| Promotion | identity/source/config/digest mismatch | negative promotion tests | no review delivered |
| Load | corrupt/schema/oversize/digest/symlink | fail closed before payload exposure | Apply unavailable |
| Restore | old completion arrives late | operation token/content recheck | ignored |
| Run UI | old event/failure/terminal arrives late | task+run correlation | ignored |
| Review intent | document changes while CAS pending | content recheck + compensation | no apply |
| Apply | lowering/apply failure | atomic document behavior + task compensation | review remains |
| Receipt precommit | mutation happened, file not replaced | guarded undo + CAS ready | review remains |
| Receipt commit-visible | parent sync uncertain | keep completed/document, warn | no contradiction |
| Zero-op receipt failure | no document state change | no undo; compensate task only | review remains |
| Undo/compensation | rollback cannot be proven | leave applying; typed error | interrupted on restart |
| Manual review | local candidate not in artifact | typed local delta receipt | behavior preserved/auditable |
| Reject | store fails | clear UI only after commit-visible save | review remains |
| Privacy | broad mode/path/debug leak | permission/custom Debug/tracing tests | fail closed |
| Retention | prune/delete failure | bounded maintenance error, valid snapshots untouched | metadata may remain |

No known failure has neither a typed outcome nor planned test. The five critical gaps found by the first engineering review—cross-document binding, CAS, commit visibility, async correlation, and manual review lineage—are explicitly covered.

## Performance and Resource Bounds

- Cache immutable base-image SHA-256 once in `ResultWorkspace`; hash canonical annotation state only at task creation/restore/apply boundaries, never per frame.
- Reject >4 MiB task files from metadata before allocation; canonical DTOs bound strings/collections/floats.
- SHA-256/JSON run once per boundary, not stream chunk or render frame.
- Every filesystem operation uses `spawn_blocking`; store lock covers only load/validate/CAS/write.
- Directory scan is deterministic O(n), prunes terminal records, and has a many-file resource test; no user-locking 256-file error.
- Existing run-event channel remains bounded at 64; no new unbounded queue.
- No image clone, animation, subscription, or per-frame allocation is introduced.

## Worktree / Subagent Parallelization Strategy

Sequential one-lane execution:

```text
Task 1 identity migration
→ Task 2 contracts/digests
→ Task 3 exact-CAS store
→ Task 4 run promotion
→ Task 5 restore correlation
→ Task 6 review reducer
→ Task 7 UI evidence
→ Task 8 verification/Gate proposal
```

All tasks share contracts or workbench lifecycle in dependency order. No worktree. Fresh read-only subagents are used only for task reviews, clean visual verdict, and final independent review.

## Auto Decisions Applied from Plan Engineering Review

- D1/D17: retained all Gate G1 scope; removed speculative 256-file limit.
- D2: added base-image and canonical annotation-state binding.
- D3: private reducers plus exact expected-snapshot CAS.
- D4/D13: explicit precommit vs committed vs commit-visible sync uncertainty and full failpoint matrix.
- D5/D15: one canonical V1 digest contract with golden/adversarial/privacy tests.
- D6/D14: correlation for every async message plus out-of-order tests.
- D7: asynchronous applying/receipt/compensation reducer.
- D8: typed local review modifications/manual additions in receipt.
- D9/D18: Unix permissions, bounded diagnostics, cached image digest, pre-read size bound.
- D10/D11: dedicated complete identity migration task and corrected file/API/commit declarations.
- D12: lane-correct clippy excluding dedicated OCR crate and exact AE=0 command.
- D16: exact Apply label, emitted-message assertions, stable selectors, visible-bound parity.

## Plan Self-Review Trace

- Spec coverage: every revised child-spec goal and Gate G1 criterion maps to Tasks 1–8.
- Placeholder scan: no implementation placeholder; the Gate decision scan command intentionally names placeholder tokens.
- Type consistency: string task/run/proposal/artifact IDs; attempt ordinal 1; artifact revision 1; snapshot revision monotonic.
- TDD: every behavioral task has explicit RED command before implementation and GREEN command after.
- File declarations: identity fallout from repository-wide `ProposalId` search is explicitly listed; implementation and Gate decision files are declared.
- Commit boundaries: structural ID migration is separate from behavior; every task ends atomically.
- Complexity: 3 created files, no new crate/top-level module, 8 tasks; plan-eng complexity threshold is not triggered.
- Required outputs: NOT in scope, What already exists, failure modes, test table, performance bounds, sequential strategy, auto decisions, and visual preflight are present.
- Unresolved decisions: zero.
