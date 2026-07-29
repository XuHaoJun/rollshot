# Action Guide Caption Review Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Repair every reviewed Slice A defect so caption tasks run, persist, restore, cancel, audit, and reconcile correctly in the active Linux and macOS products.

**Architecture:** `TaskStore` owns task-liveness file locks and ordered audited state. The caption worker owns its complete durable lifecycle through promotion or terminal persistence. Product shells own one store and clone it into workspaces; iced messages carry persisted snapshots back to UI state.

**Tech Stack:** Rust, iced 0.14, tokio, fs4, serde, rollshot-agent, rollshot-action, rollshot-app.

## Global Constraints

- Preserve all existing caption user-visible copy and add no UI affordance.
- Keep `rollshot-agent` independent of `rollshot-action`.
- Do not modify `run_visual_annotation_with_provider` behavior.
- Compile `rollshot-app` with `action-guide` both enabled and disabled.
- Use one `TaskStore` per process; cross-process opens must not interrupt live tasks.
- Runtime diagnostics use structured `tracing` with stable `rollshot::*` targets.
- Every behavior change starts with a failing observable-contract test.

---

### Task 1: Make startup reconciliation liveness-aware

**Files:**
- Modify: `crates/rollshot-app/src/agent_store/task_store.rs`

**Interfaces:**
- Consumes: existing `ProductTaskId`, `TaskStatus`, `fs4::FileExt`.
- Produces: private `TaskLivenessRegistry`, `TaskStore::task_is_live`, automatic acquisition in `create_audited`, and automatic release after terminal audited transitions.

- [ ] **Step 1: Add failing concurrent-open tests**

Add tests that keep the first store alive after `create_audited` and `start_attempt`, open a second store on the same directory, and assert the task remains `Running`. Add a companion test that drops the first store before reopening and asserts `Interrupted`. Use both Smart Redaction and caption task kinds.

```rust
#[test]
fn second_live_store_does_not_interrupt_running_task() {
    let dir = tempfile::tempdir().unwrap();
    let first = crate::agent_store::open_process_store(dir.path()).unwrap();
    let running = seed_running_caption(&first);

    let second = crate::agent_store::open_process_store(dir.path()).unwrap();

    assert_eq!(second.load(running.task_id()).unwrap().status(), TaskStatus::Running);
}

#[test]
fn reopening_after_owner_drop_interrupts_running_task() {
    let dir = tempfile::tempdir().unwrap();
    let task_id = {
        let first = crate::agent_store::open_process_store(dir.path()).unwrap();
        seed_running_caption(&first).task_id().clone()
    };

    let reopened = crate::agent_store::open_process_store(dir.path()).unwrap();
    assert_eq!(reopened.load(&task_id).unwrap().status(), TaskStatus::Interrupted);
}
```

- [ ] **Step 2: Run the tests and verify the live-owner case fails**

Run: `rtk cargo test -p rollshot-app --features action-guide second_live_store_does_not_interrupt`

Expected: FAIL because `TaskStore::open` marks the first store's running task interrupted.

- [ ] **Step 3: Implement task-ID liveness locks**

Create `<config>/agent-tasks/live/` with mode `0700`. Add a `Mutex<HashMap<String, fs::File>>` to `TaskStore`. Before an audited create, open `<live>/<task-id>.lock`, acquire `FileExt::try_lock`, and retain the file in the map. If create fails before the snapshot is visible, remove the entry. After a successful audited transition to a terminal status, remove the entry and best-effort remove the lock file.

Add:

```rust
fn task_is_live(&self, task_id: &ProductTaskId) -> Result<bool, TaskStoreError>;
fn begin_task_liveness(&self, task_id: &ProductTaskId) -> Result<(), TaskStoreError>;
fn end_task_liveness(&self, task_id: &ProductTaskId);
```

`task_is_live` returns true for a local map entry or `TryLockError::WouldBlock`; acquiring the probe lock means no live owner and returns false.

- [ ] **Step 4: Gate both startup reconciliation arms**

Before reconciling `Created | Running | Applying`, skip the task when `task_is_live` is true. Before staling an ephemeral `ReadyForReview`, also skip it when live. Keep the Created grace window for legacy/non-audited callers.

- [ ] **Step 5: Run the focused and task-store suites**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide second_live_store_does_not_interrupt
rtk cargo test -p rollshot-app --features action-guide task_store
```

Expected: PASS.

---

### Task 2: Restore single-submit transport contracts

**Files:**
- Modify: `crates/rollshot-agent/src/driver.rs`

**Interfaces:**
- Consumes: `RunBudget`, `BudgetTracker`, `AuditAppendSink`, `authority_denied_envelope`.
- Produces: reachable `SingleSubmitTerminal::TextCompleted`, enforced argument/result byte budgets, and audited authority denial.

- [ ] **Step 1: Replace the contradictory text-only test**

Change the existing text-only test to send valid caption JSON as assistant text and assert:

```rust
assert_eq!(
    terminal,
    SingleSubmitTerminal::TextCompleted {
        text: r#"{"suggestions":[]}"#.to_owned(),
    }
);
```

Run: `rtk cargo test -p rollshot-agent single_submit_returns_text_completion`

Expected: FAIL with `ProtocolFailure`.

- [ ] **Step 2: Add failing budget tests**

Use a 4-byte `argument_bytes` budget with a larger tool payload and assert `BudgetExhausted { dimension: ArgumentBytes }`. Use a stub tool returning a result larger than a 4-byte `result_bytes` budget and assert `ResultBytes`.

- [ ] **Step 3: Add the production authority-audit test**

Pass a collecting `AuditAppendSink` into `run_single_submit_with_provider`, omit `SubmitReviewCandidate`, and assert one committed `AuthorityDenied` envelope correlated to the authority task/attempt/run.

- [ ] **Step 4: Implement transport charging and fallback**

On `Done`, return `TextCompleted { text: last_assistant_text }` when non-empty; return `ProtocolFailure` only for empty completion. Before authorization, serialize tool arguments and charge `UsageSnapshot { argument_bytes, .. }`. After the stub result serializes, charge `result_bytes` before threading it into rig.

- [ ] **Step 5: Append authority denial through the sink**

Rename `_audit_sink` to `audit_sink`. On denial, require a sink, build the existing privacy-safe authority-denied envelope, append it, and map append failure to `ProtocolFailure` only after emitting structured audit-failure diagnostics. Keep the typed `AuthorityDenied` terminal after a successful append.

- [ ] **Step 6: Run the single-submit suite**

Run: `rtk cargo test -p rollshot-agent single_submit`

Expected: PASS.

---

### Task 3: Add a legal all-rejected task transition

**Files:**
- Modify: `crates/rollshot-agent/src/product_task.rs`
- Modify: `crates/rollshot-agent/src/audit.rs` only if transition derivation currently rejects `Applying -> Rejected`.

**Interfaces:**
- Produces: `ProductTaskSnapshot::reject_apply(receipt, now) -> Result<Self, TaskContractError>` for `Applying -> Rejected`.

- [ ] **Step 1: Write the failing transition test**

```rust
#[test]
fn applying_batch_with_no_accepts_can_be_rejected() {
    let applying = ready_task_fixture().begin_apply(40).unwrap();
    let rejected = applying.reject_apply(review_receipt_fixture(), 50).unwrap();
    assert_eq!(rejected.status(), TaskStatus::Rejected);
    assert!(rejected.pending_artifact_payload().is_none());
    assert!(rejected.pending_proposal_payload().is_none());
}
```

Run: `rtk cargo test -p rollshot-agent applying_batch_with_no_accepts`

Expected: FAIL because `reject_apply` does not exist.

- [ ] **Step 2: Implement the transition**

Mirror `complete_apply` receipt validation and payload clearing, but require `TaskStatus::Applying` and set `TaskStatus::Rejected`. Keep existing `reject` unchanged for direct `ReadyForReview -> Rejected` callers.

- [ ] **Step 3: Verify audit derivation**

Add or update the material-transition test so `Applying -> Rejected` derives `ReviewDecisionCommitted`.

- [ ] **Step 4: Run product-task and audit tests**

Run: `rtk cargo test -p rollshot-agent product_task audit`

Expected: PASS.

---

### Task 4: Move the caption lifecycle into the worker

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/caption_agent.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/agent_store/audit_store/mod.rs` only to expose the existing `TaskAuditSink` constructor to the caption module.

**Interfaces:**
- Produces:
  - `CaptionRunSuccess { task_id, proposal, snapshot, provider_id, model_id }`.
  - Worker success only after `ReadyForReview` is audited.
  - Worker error only after a matching terminal transition is audited when a task exists.

- [ ] **Step 1: Add a production promotion/restore test**

Drive `suggest_captions_task` with a scripted provider, then load the returned task and assert both `pending_artifact_payload` and `pending_proposal_payload` are present; deserialize the latter as `CaptionProposal`.

- [ ] **Step 2: Add table-driven terminal persistence tests**

For cancellation, zero wall time, provider error, text decode error, and authority denial, run the real worker and assert the stored task is terminal and its journal includes `TaskTerminated`; authority denial additionally includes `AuthorityDenied`.

- [ ] **Step 3: Implement worker-owned promotion**

Move `promote_caption_ready_for_review` from `update.rs` into `caption_agent.rs`. Serialize:

```rust
let artifact_payload = caption_artifact_payload(&proposal);
let proposal_payload = serde_json::to_vec(&proposal)
    .map_err(|e| format!("serialize caption proposal: {e}"))?;
```

Pass `Some(proposal_payload)` to `record_ready_for_review`. Return the persisted snapshot in `CaptionRunSuccess`.

- [ ] **Step 4: Implement terminal persistence**

After the task reaches `Running`, route every error through one helper:

```rust
async fn persist_caption_terminal(
    store: Arc<TaskStore>,
    task_id: ProductTaskId,
    terminal: TaskTerminal,
    now: i64,
) -> Result<(), String>;
```

The helper reloads the current snapshot, calls `record_terminal`, and uses `transition_audited`. Map `SingleSubmitTerminal` and decode/promotion errors to the existing typed terminal categories before returning UI copy.

- [ ] **Step 5: Thread the actual audit sink**

Construct `TaskAuditSink::new(store.clone())` and pass `Some(&sink)` to `run_single_submit_with_provider`.

- [ ] **Step 6: Simplify the iced success handler**

`CaptionProposalLoaded(Ok(success))` sets `caption_task_id`, `caption_proposal`, and `caption_review_snapshot = Some(success.snapshot)` directly. Delete `CaptionReviewPromoted` and its detached promotion command.

- [ ] **Step 7: Run caption lifecycle suites**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide caption_agent
rtk cargo test -p rollshot-app --features action-guide caption_task
```

Expected: PASS.

---

### Task 5: Serialize review decisions and retain cancellation

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`

**Interfaces:**
- Produces: `Message::CaptionReviewPersisted(Result<ProductTaskSnapshot, String>)` and one ordered background operation per decision batch.

- [ ] **Step 1: Add failing review-path tests**

Drive real persisted snapshots through:

1. one suggestion accepted;
2. one suggestion rejected;
3. first accepted, last rejected;
4. Accept all.

Await the returned iced task in the existing test harness and assert final disk and memory status plus receipt candidate partitions.

- [ ] **Step 2: Implement one ordered persistence command**

After mutating the proposal, compute `has_pending` and `has_accepted`, clone the proposal and current snapshot, then run one `spawn_blocking` closure. It persists `begin_apply` first when needed. If the batch is final, it constructs the receipt from the applying snapshot and calls `complete_apply` or `reject_apply`, persisting the second transition only after the first succeeds. Return the final persisted snapshot to `CaptionReviewPersisted`.

Remove detached `std::thread::spawn` calls and never mutate `caption_review_snapshot` ahead of disk.

- [ ] **Step 3: Route Accept all through the same command**

After `apply_all`, invoke the ordered persistence helper exactly as individual accept/reject handlers do.

- [ ] **Step 4: Retain and clear cancellation correctly**

Clone `state.caption_cancellation.as_ref().cloned().unwrap_or_default()` into the provider task instead of `take()`. Cancel and clear it on `CaptionProposalLoaded`, preparation failure, close workspace, project close, and before a new run.

- [ ] **Step 5: Add a workspace cancellation test**

Use a blocking scripted provider, trigger the existing close/leave effect, and assert the run returns `Cancelled` and the task becomes terminal.

- [ ] **Step 6: Run update and view tests**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::update`

Expected: PASS.

---

### Task 6: Wire one process store into Linux and macOS product workspaces

**Files:**
- Modify: `crates/rollshot-app/src/action_guide_linux_product.rs`
- Modify: `crates/rollshot-app/src/macos_product.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/project.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`

**Interfaces:**
- Produces:
  - `TimelineWorkspace::attach_task_store(Arc<TaskStore>)` for new/imported workspaces.
  - `project::from_loaded_project_with_store(loaded, access, Arc<TaskStore>)` for durable project open and immediate restore.

- [ ] **Step 1: Add failing Linux and macOS shell tests**

Construct each shell with a temporary process store, open/import a timeline, and assert `workspace.task_store` is the same `Arc` via `Arc::ptr_eq`. For a seeded durable caption task, assert opening the project populates both `caption_proposal` and `caption_review_snapshot` without a provider call.

- [ ] **Step 2: Add workspace attachment APIs**

`attach_task_store` stores the Arc. `from_loaded_project_with_store` builds the workspace, assigns the store, computes the loaded-project binding, restores a matching proposal, and retains both task ID and `ReadyForReview` snapshot.

Change `restore_caption_proposal` to return:

```rust
Option<(ProductTaskSnapshot, CaptionProposal)>
```

so callers cannot restore UI state without the durable snapshot.

- [ ] **Step 3: Make the Linux shell own the store**

Add `agent_task_store: Arc<TaskStore>` to `action_guide_linux_product::State`. Open it once in `run`, pass it to `State::new`, clone it into imported workspaces, and pass a clone into project-open tasks.

- [ ] **Step 4: Make the macOS shell own the store**

Add an action-guide-gated `agent_task_store: Arc<TaskStore>` to `MacosProduct`, initialize it once in `run_action_guide`, clone it into recording/imported workspaces, and pass a clone into project-open tasks. Keep non-Action Guide constructors feature-correct.

- [ ] **Step 5: Run platform-neutral product tests and both feature configurations**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide action_guide_linux_product
rtk cargo test -p rollshot-app --features action-guide macos_product
rtk cargo test -p rollshot-app
```

Expected: PASS on the host-supported targets; macOS-only native runtime remains compile-covered by cfg-aware tests.

---

### Task 7: UI evidence and full verification

**Files:**
- Modify only scenario/baseline files explicitly authorized by `testing-iced-ui`.
- Update: `docs/superpowers/spikes/2026-07-28-action-guide-captions-decision.md` after implementation works, correcting superseded residual-risk statements and evidence claims.

**Interfaces:**
- Consumes: all prior tasks.
- Produces: end-to-end evidence that restored captions remain reviewable and review decisions persist.

- [ ] **Step 1: Run the repo-local iced UI workflow**

Exercise opening a project with a stored caption proposal, accepting/rejecting it, closing/reopening, and verifying the proposal does not reappear after terminal persistence. Send raw evidence to the required independent reviewer; do not approve baselines in the implementing context.

- [ ] **Step 2: Smoke-test the active product flow**

Launch the Action Guide product, open a fixture project, request captions through a scripted/test provider configuration, exercise a review decision, close, and reopen. Observe the persisted final state and absence of a stale review card.

- [ ] **Step 3: Run full regression**

```bash
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-app
rtk cargo test -p rollshot-app --features action-guide
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all commands exit 0.

- [ ] **Step 4: Correct the gate decision record**

Remove the claim that text fallback is intentionally gone, record production-path lifecycle/restore tests rather than helper-only tests, and document liveness-aware cross-process reconciliation.
