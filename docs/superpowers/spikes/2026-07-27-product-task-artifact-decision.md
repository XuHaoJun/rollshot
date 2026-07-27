# Gate G1: Product Task Artifact Promotion — Decision Proposal

**Date:** 2026-07-27
**Status:** Proposed for user approval
**Branch:** feat/agent-foundation-product-task-artifact
**Base:** d3a1139
**Commits:** 301127e..ca31804 (14 behavioral + 2 formatting)

---

## Scope

One Smart Redaction product task with durable artifact promotion, exact CAS filesystem store, content-bound restore, async correlation, and privacy-bounded review receipts. No DAG, retry, job, or audit expansion.

---

## Identity / Content Trace

| Concept | Type | Example | Validation |
|---|---|---|---|
| ProductTaskId | `String` (uuid) | `task-<uuid>` | strict prefix + UUID suffix parse |
| RunId | `String` (uuid) | `run-<uuid>` | same validation |
| ArtifactId | `String` (uuid) | `artifact-<uuid>` | same validation |
| ArtifactRevision | `u32` | `1` | monotonic, starts at 1 |
| TaskAttemptId | `u32` | `1` | starts at 1 |
| Snapshot revision | `u64` | monotonic | incremented by every reducer |

All IDs are opaque strings validated at parse boundaries. No integer IDs leak into persistence or display.

---

## CAS Evidence

**Implementation:** `TaskStore::compare_and_swap` (task_store.rs:543)

1. Acquire exclusive `fs4` file lock on `.lock` (mode 0600)
2. Read current file bytes
3. Compare with `expected` serialized bytes — reject if mismatch
4. Serialize `replacement`, write to `.tmp-<uuid>` sibling
5. `fsync` temp file
6. `rename` temp → target
7. Classify rename + parent-directory `fsync` outcome as `StoreCommitOutcome`
8. Release lock

**Failpoints tested:** `TempWrite`, `FileSync`, `Rename`, `PostRenameSync` — each produces a typed outcome, no silent corruption.

**Stale writer test:** Two concurrent writers with same expected → one wins, one gets `CasMismatch`. No merge, no overwrite.

---

## Commit-Boundary Evidence

| Outcome | Meaning | Test |
|---|---|---|
| `Committed` | rename succeeded, fsync ok | ✓ |
| `CommitVisibleRenameDone` | rename visible, parent fsync uncertain | ✓ warning, no rollback |
| `PreCommitFailed` | temp write/fsync failed | ✓ no file touched |
| `CasMismatch` | expected ≠ actual | ✓ one transition wins |

Parent-sync uncertainty: the store keeps the completed state and logs a warning. No contradictory undo is attempted.

---

## Reconciliation

On startup, the store scans the `tasks/` directory, loads all snapshots, prunes terminal records older than 30 days, and validates each file (regular file, not symlink, ≤4 MiB, valid JSON, schema version ≤1). The scan is deterministic O(n) with a many-file resource test.

---

## Stale / Cross-Document Tests

| Scenario | Handling | Test |
|---|---|---|
| Old run event arrives late | task_id + run_id correlation check → ignored | ✓ |
| Old terminal arrives late | same correlation → ignored | ✓ |
| Same-state different-image | base-image SHA-256 + annotation-state SHA-256 binding → unrelated task ignored | ✓ |
| Old restore completion arrives | operation token + content recheck → ignored | ✓ |
| Document changes during CAS pending | content recheck + compensation → no apply | ✓ |

---

## Canonical Digests

One canonical V1 digest contract: `canonical_v1_bytes` → `canonical_v1_digest` (SHA-256 hex).

| DTO | Digest used at | Golden test | Adversarial test | Privacy test |
|---|---|---|---|---|
| `RunConfigFingerprintV1` | promotion, restore | ✓ | ✓ | ✓ (no secrets/keys/paths) |
| `SmartRedactionReviewPayload` | promotion, restore | ✓ | ✓ | ✓ (no pixels/text/OCR) |
| `PayloadProposalV1` | promotion | ✓ | ✓ | ✓ |
| `PayloadSourceV1` | promotion | ✓ | ✓ | ✓ |
| `AnnotationStateV1` | content binding | ✓ | ✓ | ✓ |

Floats validated finite before JSON conversion. Strings/collections bounded (4096/256). Objects canonicalized via BTreeMap (sorted keys).

---

## Async Correlation

Every async iced message carries `(task_id, run_id)` correlation. The run reducer matches both before applying. Out-of-order arrival (text chunks, terminal, failure) is tested. Late messages for stale/unknown runs are dropped silently.

The `RunEvent` channel remains bounded at 64 (pre-existing). No new unbounded queue introduced.

---

## Local Review Delta / Post-State

`ReviewReceipt` contains:
- `artifact_id` + `artifact_revision` — exact artifact identity
- `local_review_delta: LocalReviewDeltaV1` — typed modifications + manual additions
- `post_apply_annotation_state_digest` — actual post-apply state

Manual candidates are fully represented in the receipt as `ManualCandidateV1` entries with geometry and label.

---

## Privacy / Retention / Permissions

| Control | Status |
|---|---|
| Directory mode | 0700 on `agent-tasks/` ✓ |
| File mode | 0600 on `.lock` and `task-*.json` ✓ |
| Custom `Debug` on `ProductTaskSnapshot` | Redacts pending payload bytes ✓ |
| `truncate_error` on all error paths | ≤80 chars, no full paths/payloads ✓ |
| No `assistant_text`, `user_message`, `attachment_bytes`, `api_key`, `ocr_text`, `provider_response` in persisted structs | ✓ audit clean |
| No `rig_core` or provider-native types in product_task.rs | ✓ |
| Retention: prune terminal records >30 days | ✓ bounded maintenance error on failure |

---

## Command Counts

| Suite | Passed | Ignored | Notes |
|---|---:|---:|---|
| rollshot-edit-proposal | 15 | 0 | |
| rollshot-automation | 37 | 0 | |
| rollshot-automation-rquickjs | 388 | 0 | |
| rollshot-agent | 60 | 0 | |
| rollshot-vision --no-default-features | 24 | 0 | |
| rollshot-action | 284 | 0 | |
| rollshot-app | 785 | 6 | 6 feature-gated (action-guide, ocr) |
| **Total** | **1,593** | **6** | |

Formatting: `cargo fmt --check` ✓
Lint: `cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings` — 2 pre-existing `too_many_arguments` warnings in `tools.rs` (unchanged from base d3a1139), no new warnings.
Whitespace: `git diff --check` ✓

---

## Visual Verdict

Task 7 verified the Simulator evidence:
- Exact Apply interaction at two image sizes (small + full)
- Expected vs restored AE=0 pixel comparison
- Semantic review of redacted output

No visual regressions detected. UI selectors remain stable.

---

## Independent Review (10 Questions)

| Question | Answer | Evidence |
|---|---|---|
| Can any run/setup path start before durable running? | No | `start_agent_run` persists terminal via `persist_terminal_outcome` inside spawned task before yielding `RunTerminal` to iced |
| Can any terminal/UI review appear before commit-visible artifact transition? | No | `persist_terminal_outcome` returns `Some(err)` on failure → caller suppresses proposal |
| Can same-state different-image content restore or stale the wrong task? | No | `SourceBinding` includes base-image SHA-256 + annotation-state SHA-256; restore checks both |
| Can stale writers bypass exact CAS? | No | `compare_and_swap` reads current bytes, compares with expected, rejects on mismatch under exclusive lock |
| Does directory-sync uncertainty ever trigger contradictory undo? | No | `CommitVisibleRenameDone` keeps completed state + warning; no rollback attempted |
| Can late iced messages mutate a newer task/operation? | No | Run reducer checks `(task_id, run_id)` correlation; stale events dropped |
| Can document changes race applying/receipt phases? | No | Review reducer uses CAS for all transitions; document change → CAS fail → no apply |
| Are manual additions/modifications fully represented in receipt? | Yes | `ReviewReceipt.local_review_delta` includes `ManualCandidateV1` entries |
| Do files/logs/debug exclude forbidden private data and obey 0700/0600? | Yes | Privacy audit clean; directory 0700, files 0600, custom Debug redaction |
| Did scope stay one Smart Redaction task? | Yes | `ArtifactKind::SmartRedaction` only; no DAG/retry/job/audit expansion |

---

## Residual Risks

1. **Pre-existing clippy warnings** in `tools.rs` (too_many_arguments) — not introduced by this work, not blocking.
2. **Process crash during running** — if the process crashes between creating the task and persisting the terminal, the task store will have an interrupted snapshot. On restart, the store scan will find it and it will be pruned after 30 days. No false success is possible because the terminal was never persisted.
3. **Provider/tool side effects during crashed run** — the agent may have made external calls before crashing. These are not rolled back. This is inherent to the agent architecture and not introduced by this work.

---

## Decision

**Slice 3 may begin only after user approval.**

This is a proposal, not a passed gate. The user must explicitly approve before proceeding to Slice 3 implementation.

---

## Verification Commit

```
ca31804 style(agent): fix cargo-fmt import order and trailing comma
4599ee5 style(workspace): apply cargo fmt to task-artifact files
```

All behavioral commits from Tasks 1–7 are already on the branch (301127e..d3a1139). The two formatting commits above are the only changes made during Task 8 verification.
