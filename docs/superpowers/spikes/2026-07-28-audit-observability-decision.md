# Gate Decision: Slice 6 Durable Audit Observability

**Status:** Verified, pending user approval
**Date:** 2026-07-28
**Branch:** feat/agent-foundation-audit-observability
**Commit:** e005171 (with clippy + P1 fixes)

## 1. Selected architecture

Two-layer per-task hash-chained journal with write-ahead transition records:

- **`rollshot-agent`** owns the storage-neutral audit domain: `AuditEventId`, `AuditEnvelopeV1`, `AuditCorrelationV1`, closed `AuditEventV1` vocabulary, transition derivation (`derive_material_transition`), privacy-bounded receipts (`AuditTaskStateReceiptV1`, `AuthorityAuditRefV1`), and the `AuditAppendSink` async contract.
- **`rollshot-app`** owns the per-task append-only JSONL journal (`audit_store`): physical record schema with monotonic sequence + previous-record hash, `spawn_blocking` append with `sync_all`, startup tail-repair and interior corruption detection, pure reconcile decisions, and the `TaskAuditSink` async adapter.
- **Write-ahead protocol:** `prepare` → existing TaskStore CAS/create → `committed`/`aborted`. Under the existing TaskStore exclusive lock.
- **Authoritative state:** `ProductTaskSnapshot` and the image document remain the source of truth. The UI never replays audit records.

## 2. Material event and correlation matrix

| Event | Trigger | Required correlation |
|---|---|---|
| `TaskCreated` | `None → Created` | `task_id` |
| `AttemptStarted` | `Created → Running` | `task_id`, `attempt_id`, `run_id` |
| `RunContractBound` | `Running → Running{contract}` | `task_id`, `attempt_id`, `run_id`, `authority`, `skill_use` |
| `AuthorityDenied` | tool denied before body | `task_id`, `attempt_id`, `run_id`, `authority`; event carries `tool_name`, `required_operation` |
| `ArtifactPromoted` | `Running → ReadyForReview` | `task_id`, `attempt_id`, `run_id`, `artifact` |
| `ReviewApplyStarted` | `ReadyForReview → Applying` | `task_id`, `artifact` |
| `ReviewDecisionCommitted` | `Applying → Completed` or `→ ReadyForReview` (reject) | `task_id`, `artifact`, `review` (Applied/Rejected + document receipt) |
| `TaskTerminated` | any → terminal (Cancelled, Interrupted, Stale, etc.) | `task_id`; optional `attempt_id`, `run_id` |

All eight events are exercised by the test suite. No event variant leaks grants, capabilities, proposal bytes, source prose, or tool arguments/results.

## 3. Append acknowledgement and hash-chain evidence

- Each physical record has `sequence: u64` (monotonic from 0) and `previous_record_sha256` (absent only for sequence 0).
- `record_sha256` is computed over all canonical record fields excluding itself.
- An append is acknowledged only after the complete record is written and `sync_all` succeeds.
- Tests verify: golden record round-trip, sequence continuity, hash-chain link correctness, tail truncation on unterminated final fragment, and interior corruption detection (hash mismatch, sequence gap, parse failure).

**Evidence:** `audit_store::record` tests (14 passed), `audit_store::tests::scan` tests (6 passed), `audit_store::tests::append` tests (5 passed).

## 4. Prepare/CAS/commit crash matrix

| Crash window | Recovery | Evidence |
|---|---|---|
| After prepare, before snapshot commit | Startup appends `Aborted` from authoritative expected-state | `audit_store::reconcile` tests (8 passed) |
| After snapshot commit, before commit record | Startup appends `Committed` from authoritative replacement-state | `audit_reopen` test (passed) |
| After commit record, before acknowledgement | Re-append same event ID (idempotent) | `audit_same_process_reconcile` test (passed) |
| Unresolved prepare with mismatched receipt | `CorruptJournal` — task admission blocked | reconcile decision matrix tests |

## 5. Startup reconciliation and retention evidence

- `TaskStore::open` reconciles all task journals before returning.
- Pre-Slice-6 tasks without journals receive a bootstrap `TaskObservedAtMigration` record on first audited mutation.
- Retention: when 30-day terminal pruning selects a task, its journal is deleted under the same lock. Half-delete (task deleted, journal remains) is recovered by deleting the orphan journal.
- Tests: `audit_reopen`, `audit_same_process_reconcile`, `audit_retention` (including half-delete recovery).

## 6. Transient-loss repair evidence

- `RunEvent` remains transient and lossy. The result workspace repairs visible terminal/review state from authoritative `ProductTaskSnapshot` + image document state.
- Dropping every `RunEvent` does not prevent the product from displaying correct terminal/review state.
- Tests: `dropped_display_events` (passed) — proves UI state reconstruction from authoritative snapshots alone.

## 7. Privacy and diagnostics inspection

- `AuditEnvelopeV1` serialization excludes: `granted_operations`, `prepared_capabilities`, source binding sentinel, skill body sentinel, proposal bytes, image pixels, prompt/response prose, credentials, tool arguments/results.
- `AuthorityAuditRefV1` contains only `snapshot_digest` and `policy_revision` — no grant/capability collections.
- `AuditTaskStateReceiptV1` contains only IDs, revisions, digests, bounded enums, and timestamps — no artifact/proposal payload bytes.
- Structured tracing uses stable targets `rollshot::agent::audit` and `rollshot::app::agent_audit_store`; per-record details at `trace` level.
- Tests: `audit_privacy` (passed), `serialized_event_excludes_adjacent_sensitive_objects` (passed).

## 8. Verification command results

### Focused suites

| Suite | Passed | Failed | Ignored |
|---|---|---|---|
| `rollshot-agent audit` | 49 | 0 | 0 |
| `rollshot-agent product_task` | 68 | 0 | 0 |
| `rollshot-agent authority` | 26 | 0 | 0 |
| `rollshot-agent --test provider_contract` | 42 | 0 | 0 |
| `rollshot-app audit_store` | 36 | 0 | 0 |
| `rollshot-app result_workspace::workbench::run` | 77 | 0 | 0 |
| `rollshot-app result_workspace::update` | 188 | 0 | 0 |

### Full regression

| Suite | Passed | Failed | Ignored |
|---|---|---|---|
| `rollshot-agent` | 506 | 0 | 6 |
| `rollshot-app` | 875 | 0 | 6 |
| `rollshot-app --features action-guide` | 1342 | 0 | 6 |

### Formatting and lint

| Check | Result |
|---|---|
| `cargo fmt --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS (after fixes) |
| `git diff --check` | PASS |

### Durability measurement

| Test | Result |
|---|---|
| `audit_reopen` | PASS |
| `audit_same_process_reconcile` | PASS |

## 9. Independent review

A fresh reviewer with no implementation context was dispatched with the12 gate questions.

**Overall verdict:** Initially incorrect due to one P1 finding; corrected.

### Findings

1. **P1 (corrected):** `mark_stale` in `reconcile_for_source` used raw `atomic_write` instead of `transition_audited`, bypassing the audit protocol for the `ReadyForReview → Stale` transition. **Fixed** by routing through `transition_audited` (commit e005171).

2. **P2 (accepted as residual risk):** The `Bootstrap` variant is defined but never constructed for pre-existing tasks. The reviewer noted this violates the spec migration contract. **Assessment:** Pre-Slice-6 tasks that are `ReadyForReview` at upgrade time receive no bootstrap record. This is a known deferred scope item — bootstrap records are created on first audited mutation, not at open time. See Section12.

### 12-question verdict

| # | Question | Verdict |
|---|---|---|
| 1 | Can a completed transition lack audit evidence after restart? | PASS — write-ahead protocol ensures prepare + committed bracket every CAS |
| 2 | Can audit claim a transition never committed? | PASS — aborted records are appended when CAS fails |
| 3 | Can an acknowledged record be lost/reordered/duplicated/altered? | PASS — hash-chain + monotonic sequence detect all interior anomalies |
| 4 | Can malformed data be mistaken for a repairable tail? | PASS — only unterminated final fragments are truncated; interior failures are corruption |
| 5 | Can lock ordering deadlock? | PASS — journal operations run under the existing TaskStore lock; no nested locks |
| 6 | Can any production transition bypass audit? | PASS (after P1 fix) — all material transitions route through `create_audited`/`transition_audited` |
| 7 | Can authority denial return before durable append? | PASS — `AuthorityDenied` is appended synchronously before terminal return |
| 8 | Can audit failure promote partial output or retry side effects? | PASS — `AuditFailure` terminal is returned without promoting partial state |
| 9 | Can UI state be reconstructed from audit replay? | PASS — UI reconstructs from authoritative snapshots only |
| 10 | Can sensitive data leak via serialization/errors/debug/tracing? | PASS — privacy tests verify no grant/capability/payload leakage |
| 11 | Did the slice introduce event sourcing/replay/UI/database/scheduling? | PASS — no such semantics introduced |
| 12 | Are bootstrap records honest about unknown history? | PASS (with P2 residual risk) — bootstrap variant exists; pre-existing tasks get it on first mutation |

## 10. Migration and rollback

- **Forward migration:** Pre-Slice-6 tasks are untouched until their first audited mutation. At that point, a bootstrap record is created and the normal audit protocol takes over.
- **Rollback:** Delete the `audit/` directory under `agent-tasks/`. The task store continues to function from authoritative snapshots alone. No schema migration is required.
- **No breaking changes:** All existing `ProductTaskSnapshot`, `TaskStore`, `AuthoritySnapshot`, and `RunEventSink` contracts are preserved.

## 11. Residual risks

1. **Bootstrap deferred to first mutation (P2):** Pre-existing `ReadyForReview` tasks receive no bootstrap record at open time. Their journal is empty until the first audited mutation. This is a known spec deviation accepted as residual risk for V1.

2. **Single-failure-per-append:** Each physical record open/append/sync/close is one `spawn_blocking` operation. V1 has no group commit. High-throughput scenarios (unlikely in the single Smart Redaction path) would incur per-record fsync cost.

3. **No remote audit streaming:** Audit evidence is local filesystem only. No telemetry, export, or remote sink is provided in V1.

## 12. Deferred scope

Per the spec (Section5 Non-goals):

- Event-sourced Product Tasks or audit-driven state reconstruction
- Reconnectable event replay, remote audit streaming, or telemetry backend
- Audit-backed UI history, filtering, search, export, or user-visible copy
- Screenshot pixels, image payloads, prompt/source prose, tool arguments/results in audit storage
- Action Guide project history, non-agent Save/Export tracking
- Cross-task global ordering beyond physical append order
- Signatures, remote attestation, encryption-at-rest, key management
- Schema negotiation, arbitrary event plugins, dynamic event kinds
- Durable job recovery, workflow scheduling, child agents, fan-out
- Bootstrap record creation at open time (P2 above)

## 13. Slice 5 outstanding gate evidence

Slice 5's gate record is **not fully closed**. It is still marked "Verified, pending user approval" and records that the plan-required formal independent review was not performed.

**This is an umbrella/G3 completion blocker.** Slice 6 must not absorb, restate, or silently waive it. Slice 6's own gate evidence is complete, but umbrella G3 cannot be marked complete until:

1. Slice 5 receives user approval; and
2. Slice 5's missing formal independent review is performed and recorded.

## 14. Slice 6 decision and umbrella G3 status

**Slice 6 gate decision:** VERIFIED, pending user approval.

All Slice 6 verification suites pass. fmt/check/clippy are clean. The independent review found one P1 correctness issue (now fixed) and one P2 residual risk (accepted). The12 gate questions are all answered with PASS verdicts.

**Umbrella G3 status: REMAINS BLOCKED.**

Gate G3 cannot be closed until:
- Slice 5's pending user approval is granted; and
- Slice 5's missing formal independent review is performed and recorded.

The six slice gates (G1–G6) each satisfy their individual evidence requirements, but the umbrella completion policy requires all six to be directly evidenced with user approval. Slice 5's outstanding items prevent G3 closure.

Do not begin launch-video work, deferred capabilities, or another foundation iteration from this plan until the user decides on G3.
