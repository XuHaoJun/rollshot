# Action Guide Caption Review Remediation Design

**Date:** 2026-07-29
**Status:** Approved correction to Slice A
**Parent:** `2026-07-28-action-guide-agent-foundation-captions-design.md`

## 1. Goal

Make the implemented caption provenance flow satisfy the live Slice A contract end to end. The correction adds no UI surface and preserves existing copy. It repairs product-shell store ownership, restore, bounded-run fallback and audit behavior, cancellation, ordered review persistence, terminal persistence, and cross-process reconciliation.

## 2. Invariants

1. Each Rollshot process opens one `TaskStore`; every timeline workspace in that process receives an `Arc` clone.
2. A caption task that reaches `Running` reaches exactly one durable terminal state or `ReadyForReview` before its worker returns.
3. Promotion persists both the kind-specific artifact payload and the serialized `CaptionProposal` needed for restore.
4. Restore returns the task snapshot together with the proposal, so later review decisions continue the same durable lifecycle.
5. Review persistence is ordered. `ReadyForReview -> Applying -> Completed|Rejected` cannot run as detached competing CAS writes.
6. The workspace retains a clone of `RunCancellation` until the run completes and cancels it on existing close/leave/new-run transitions.
7. Text-only model completion returns `TextCompleted` and remains decodable by the existing caption JSON fallback.
8. The single-submit runner enforces the supplied `RunBudget`, including argument and result bytes, and appends `AuthorityDenied` through the supplied audit sink.
9. Opening a store does not imply every other process is dead. Startup reconciliation may resolve only work proven to belong to an abandoned process.

## 3. Architecture

### 3.1 Process-owned store

The Linux Action Guide product and macOS Action Guide product open the store once during product boot. Their state owns the `Arc<TaskStore>` and clones it into workspaces created from recordings, imports, or existing projects. The standalone Linux post-capture timeline path retains its existing one-store boot behavior.

### 3.2 Caption worker lifecycle

The caption worker owns task creation, attempt start, run-contract binding, provider execution, decode, promotion, and terminal persistence. Its success value contains the already-persisted `ReadyForReview` snapshot plus the proposal. Its error path persists an appropriate `TaskTerminal` before returning user-facing copy. Promotion failure is therefore a worker failure, not an in-memory review state.

The worker constructs a `TaskAuditSink` for the task and passes it to the single-submit runner. An authority-denial audit append failure remains fail-closed.

### 3.3 Restore

Project-open initialization computes the current durable source binding after the process store is attached. Reconciliation returns a matching `ReadyForReview` snapshot. The serialized proposal is decoded and both values populate workspace state without a provider call.

### 3.4 Ordered review decisions

A review decision produces one background command that loads the current persisted snapshot and performs the required ordered transitions under the store's audited CAS API. For a first-and-final decision it persists `begin_apply` before the terminal transition in the same command. The command returns the resulting snapshot to the iced update loop.

`Accept all` uses the same path. Final status is `Completed` when at least one suggestion was accepted and `Rejected` when none were accepted. The task contract gains a legal `Applying -> Rejected` transition rather than bypassing the mandatory `Applying` phase.

### 3.5 Process ownership and reconciliation

Each active task holds a lock-backed liveness file keyed by its existing task ID from creation until promotion or terminal persistence. Startup reconciliation probes that lock without blocking and interrupts `Created`, `Running`, or `Applying` only when no live holder exists. A crashed process releases the OS lock automatically, so no new persisted owner field or schema version is needed. Ephemeral `ReadyForReview` remains stale after restart regardless of owner because it has no durable apply target. Legacy schema 1-3 tasks use the same task ID and therefore need no compatibility shim.

## 4. Error Semantics

- Cancellation, budget exhaustion, provider failure, protocol failure, authority denial, decode failure, and promotion failure persist a terminal transition.
- Audit append failure maps to `TaskTerminal::AuditFailure` and does not promote.
- UI state changes to reviewable only after durable promotion succeeds.
- Review persistence errors keep the prior durable snapshot in state and show existing error handling; they never advance memory beyond disk.

## 5. Tests

Add failing tests for:

- Linux and macOS product-created workspaces receiving the one process store.
- Production promotion storing a proposal payload and production project-open restoration returning the snapshot.
- Text-only JSON completion producing `TextCompleted` and decoding successfully.
- Cancellation through the workspace-owned clone during a run.
- Every non-success terminal persisting `TaskTerminated`.
- All-rejected, mixed accept/reject, one-suggestion, and Accept-all ordered review transitions.
- Authority-denied append through the actual single-submit audit sink.
- 4 KiB argument/result budget enforcement.
- A second live process store open leaving the first process's running task untouched, while an abandoned owner is reconciled.

Run the existing `rollshot-agent`, `rollshot-action`, feature-off app, and Action Guide app suites plus formatting and clippy. User-visible iced behavior must be exercised through the repository's iced UI workflow; baseline approval remains independent.
