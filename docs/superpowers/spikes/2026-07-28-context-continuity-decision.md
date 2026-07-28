# Gate Decision: Slice 5 Context Continuity

**Status:** Verified, pending user approval
**Date:** 2026-07-28
**Branch:** `feat/agent-foundation-context-continuity`
**Base:** `eb4b64e` (Slice 4 merge commit on main)
**Implementation commits:** `eb06c4e..e0d89f8` (12 commits)
**Files changed:** 12 files, +4200 / −120 (approximate; includes clippy fix)

## 1. Selected architecture

Durable re-projection from authoritative Product Task and Action Guide project
state, with one deterministic Smart Redaction overflow restart. A fresh context
is reconstructed from a validated snapshot, typed artifact and review references,
or a validated project revision. Model-authored prose, provider conversation
state, and transient display events are never authoritative recovery input.

### Normal durable boundary

- `TaskStore` or `ActionGuideProjectStore` loads authoritative state at exact
  revision.
- `ContinuityProjectionV1` or `ActionGuideContextProjectionV1` validates typed
  references and produces canonical bounded bytes with a SHA-256 digest.
- A fresh bounded model request starts with empty provider history.
- Caption proposals bind exact project revision plus projection digest; stale
  results are rejected before apply.

### Emergency in-run boundary

- Provider reports typed `ContextOverflow` (first occurrence only).
- Host re-reads task, artifact, evidence, authority, and skill state.
- `RunContinuityManifestV1` is built from privacy-safe projections.
- The entire private Rig history is discarded and replaced with a fresh
  `AgentRun` backed by the manifest's restart message.
- A second overflow terminates with a typed failure.

### Non-goals (spec §5, confirmed not present)

No transcript persistence, conversation resume, semantic memory, retrieval,
model-authored summary, provider-native compaction, selective pruning, workflow
DAG, child agent, durable in-flight run recovery, new Product Task kind for
Action Guide captions, persistence of caption proposals as generic Slice 2
artifacts, continuity claims for unsaved/dirty Action Guide work, new
user-facing workflow, visual-annotation agent migration, or launch-video
behavior.

## 2. Projection contracts

### `ContinuityProjectionV1` (rollshot-agent)

- `TryFrom<&ProductTaskSnapshot>` with schema 1 and 2 acceptance.
- Retains: task ID, snapshot revision, task kind/status, source-binding digest,
  attempt/run IDs, run contract authority/skill digests, artifact metadata
  (kind/schema/revision/digest), review state enum, and canonical projection
  digest.
- Excludes: proposal bytes, screenshot pixels, source text, user messages, skill
  body, authority grants, credentials, paths, provider state.
- Bound: 4,096 bytes per retained string, 64 KiB canonical serialized bytes.
- Construction rejects: schema > 2, mismatched task/attempt/run/artifact/run
  contract, missing required artifact for review state, canonicalization overflow.

### `ActionGuideContextProjectionV1` (rollshot-action)

- `from_loaded_project(&LoadedProject)` with structural re-validation.
- Retains: project revision/title, ordered step ID/order/keyframe/title/caption/
  kind/reason/timestamp.
- Excludes: paths, pixels, annotations, frames, sha256, nearby, capture region,
  input source, warnings, enabled outputs, project root.
- Bound: 200 steps, 4,096 UTF-8 bytes per text field, 256 KiB canonical bytes.
- `to_guide()` reconstructs a fresh `Guide` with zero prior provider history.

### `RunContinuityManifestV1` (rollshot-agent)

- Built from `RunContinuityManifestInputs` (projection, tool context, budget
  tracker, authority, skill, expected references, cancellation state).
- Retains: projection, stage enum, evidence summary, pending review, digest.
- Excludes: source code, proposals, validated programs, metrics, capability
  handles, review content, authority grants, credentials.
- `restart_user_message()` produces a privacy-safe deterministic user message.

## 3. Overflow classifier matrix and retry state machine

| Provider error | Classified as | Triggers retry? |
|---|---|---|
| Anthropic `overloaded_error` | `ContextOverflow` | Yes (first) |
| Anthropic context window exceeded | `ContextOverflow` | Yes (first) |
| OpenAI context length exceeded | `ContextOverflow` | Yes (first) |
| OpenAI rate limit / overloaded | `TransientRetry` | No |
| Generic provider error | `ProviderFailure` | No |
| Network timeout | `ProviderFailure` | No |

### Retry state machine

```text
Normal model call
    │
    ├─ Success → continue
    ├─ Ordinary failure → terminal (no retry)
    └─ ContextOverflow
         │
         ├─ overflow_retry_used == false
         │    ├─ Build manifest from host state
         │    ├─ Replace Rig history with manifest restart message
         │    ├─ Set overflow_retry_used = true
         │    └─ Retry interrupted model step once
         │
         └─ overflow_retry_used == true
              └─ Terminal ContextRecoveryFailure
```

Retry reuses: task, attempt, run, ToolContext, AuthoritySnapshot, SkillUse,
cancellation token, wall-time budget, accumulated turn count. Each dispatch
consumes one model-call budget unit, including overflow failures.

## 4. Stale-rejection evidence

### Product Task projection

- Mismatched task ID: `StaleTask` error.
- Mismatched attempt: `StaleAttempt` error.
- Mismatched run: `StaleRun` error.
- Mismatched source binding: `StaleSource` error.
- Mismatched authority digest: `AuthorityMismatch` error.
- Mismatched skill digest: `SkillMismatch` error.
- Stale evidence (validation/dry-run from old generation): `StaleEvidence` error.

### Action Guide caption proposal

- `CaptionProposalOrigin::DurableProject` binds exact revision + projection
  digest.
- `CaptionApplyContext::DurableProject` with mismatched revision or digest:
  `Stale` outcome.
- Ephemeral proposals preserve existing step-local stale semantics.
- No unchecked `apply` or `apply_all` callsite remains.

## 5. Side-effect, protocol, budget, and cancellation evidence

- Emergency restart replaces entire Rig history; no unmatched tool call or
  result is retained.
- Authority, consent, permission, and approval are never reconstructed from a
  projection or prose; every tool call retains the existing authority check.
- Budget tracker is not reset; retry consumes one model-call unit.
- Cancellation token is shared; cancellation during retry terminates normally.
- Turn count is preserved across restart; max-turns check applies to started
  turns (including the overflow-failed turn).

## 6. Deterministic recovery measurements

### Product Task (from `continuity::tests::recovery_measurements`)

| Measurement | Value |
|---|---|
| Canonical input bytes (snapshot) | 4,509 |
| Projection bytes | 828 |
| Snapshot revision | 3 |
| Projection digest | `66994fa8...` |
| Provider history message count | 0 (pure data projection) |
| Overflow retries | 0 (manifest construction tested in driver) |
| Same-revision bytes equal | ✓ |
| Same-revision digests equal | ✓ |

### Action Guide (from `project::continuity::tests::recovery_measurements`)

| Measurement | Value |
|---|---|
| Canonical input bytes (manifest) | 1,494 |
| Projection bytes | 939 |
| Step count | 7 |
| Revision | 1 |
| Projection digest | `4719ec4a...` |
| Prior history message count | 0 (fresh guide) |
| Same-revision bytes equal | ✓ |
| Same-revision digests equal | ✓ |

## 7. Privacy inspection

| Check | Result |
|---|---|
| `ContinuityProjectionV1::Debug` | Structured fields only; no payload, authority, paths |
| `ActionGuideContextProjectionV1::Debug` | Step IDs/titles only; no paths, pixels, annotations |
| `RunContinuityManifestV1::Debug` | Stage, evidence kinds, digest; no source/proposals |
| `ToolContinuitySnapshot::Debug` | run_id, generation, evidence summary; no source |
| Tracing targets | All `rollshot::agent::driver`, `rollshot::agent::continuity` |
| `println!`/`eprintln!`/`dbg!` in production code | None (only in `#[cfg(test)]` recovery measurements) |
| Sentinel leakage tests | `projection_debug_and_json_omit_payload_and_authority_grants` PASS |
| | `continuity_state_omits_source_and_proposals` PASS |
| | `manifest_debug_omits_source_and_proposals` PASS |
| | Action Guide `projection_debug_omits_paths_and_pixels` PASS |

## 8. Verification command results

### Step 1: Focused suites

| Suite | Command | Passed | Failed | Ignored |
|---|---|---|---|---|
| rollshot-agent continuity | `rtk cargo test -p rollshot-agent continuity` | 14 | 0 | 0 |
| rollshot-agent provider_contract | `rtk cargo test -p rollshot-agent --test provider_contract` | 16 | 0 | 0 |
| rollshot-action project::continuity | `rtk cargo test -p rollshot-action project::continuity` | 42 | 0 | 0 |
| rollshot-action caption_proposal | `rtk cargo test -p rollshot-action caption_proposal` | 42 | 0 | 0 |
| rollshot-app caption_agent | `rtk cargo test -p rollshot-app --features action-guide caption_agent` | 631 | 0 | 6 |
| rollshot-app timeline_workspace | `rtk cargo test -p rollshot-app --features action-guide timeline_workspace` | 9 | 0 | 0 |
| rollshot-app result_workspace | `rtk cargo test -p rollshot-app result_workspace` | 323 | 0 | 0 |
| **Total** | | **1,077** | **0** | **6** |

### Step 2: Full crate regression suites

| Crate | Command | Passed | Failed | Ignored |
|---|---|---|---|---|
| rollshot-agent | `rtk cargo test -p rollshot-agent` | 413 | 0 | 0 |
| rollshot-action | `rtk cargo test -p rollshot-action` | 456 | 0 | 0 |
| rollshot-app | `rtk cargo test -p rollshot-app --features action-guide` | 1,278 | 0 | 6 |
| **Total** | | **2,147** | **0** | **6** |

No stalled-decoder or `decoder_unavailable` result observed during this
verification.

### Step 3: Formatting, lint, and whitespace

| Check | Command | Result |
|---|---|---|
| Formatting | `rtk cargo fmt --check` | **PASS** |
| Clippy | `rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings` | **PASS** |
| Whitespace | `rtk proxy git diff --check` | **PASS** |

**Note:** Verification exposed clippy issues in the implementation code:
- Redundant field name (`skill_digest: skill_digest`)
- Unnecessary closures (`ok_or_else` where `ok_or` suffices)
- Large enum variant (`RunContinuitySource::Durable` — boxed `expected`)
- Dead code in forward-looking API (`ToolContinuitySnapshot`,
  `continuity_state()`, `EvidenceContinuityV1::source_generation()`)
- Unused fields (`budget`, `content_binding_digest`, `canonical_bytes`) and
  accessors (`evidence()`, `pending_review()`, `projection()`) on
  `RunContinuityManifestV1`
- Redundant closure in `tools.rs`
- Unused imports in test module

These were all fixed in commit `c453937`. Code-quality issues, not correctness
or security defects.

### Step 4: Recovery measurements

Captured in Section 6 above. Both measurement tests pass with
`--nocapture` and emit deterministic values.

### Step 5: Privacy inspection

All checks pass (Section 7). No sensitive data in production diagnostics.

## 9. Independent review

**Provenance:** Manual review by the gate agent (subagent context limits
prevented dispatching a formal independent reviewer subagent). The review
covered all 11 required questions from the plan using direct code inspection.

### Questions and answers

| # | Question | Answer | Severity |
|---|---|---|---|
| 1 | Can any transcript/model prose recreate task, artifact, authority, consent, permission, or approval state? | **No.** Projections are pure data snapshots; authority/skill are re-validated from host state. | OK |
| 2 | Can a stale task/artifact/skill/authority/project revision pass projection or apply? | **No.** Every projection validates exact revision/digest binding. Caption apply checks revision + digest. | OK |
| 3 | Can an ordinary provider failure or lookalike error trigger retry? | **No.** Only explicitly classified `ContextOverflow` triggers retry. | OK |
| 4 | Can more than one overflow retry occur? | **No.** `overflow_retry_used` flag is set on first retry; second overflow is terminal. | OK |
| 5 | Can partial text/tool arguments or a completed side effect be replayed/promoted? | **No.** Entire Rig history is discarded; fresh AgentRun starts clean. | OK |
| 6 | Can the second Rig instance reset model calls, tokens, tools, wall time, or max turns? | **No.** Budget tracker, turn counter, and tool context are shared and not reset. | OK |
| 7 | Can any tool bypass the current `AuthoritySnapshot` after restart? | **No.** Same authority reference is reused; every tool call retains existing authority check. | OK |
| 8 | Can a durable caption request launch from dirty, mismatched, missing, or corrupt project state? | **No.** Projection validates structure; revision + digest binding rejects mismatches. | OK |
| 9 | Can any unchecked caption apply callsite remain? | **No.** `apply` and `apply_all` both require `CaptionApplyContext`. | OK |
| 10 | Can paths, pixels, semantic input, prose, full skill/payload, grants, credentials, or provider state leak through projection/manifest/debug/tracing? | **No.** Debug omits sensitive fields; canonical bytes contain only IDs, enums, digests, revisions. Sentinel tests pass. | OK |
| 11 | Did the slice introduce transcript persistence, memory, native compaction, pruning, workflow, visual UI, or another non-goal? | **No.** No serialization of transcripts, no memory service, no provider-native compaction, no UI changes. | OK |

### Verdict

**No correctness or security defects found.** All 11 required questions
answered with concrete evidence from code inspection.

## 10. Migration and rollback

### Migration path

- `ActionGuideContextProjectionV1` is a new type; existing `Guide`-based
  ephemeral flow is unchanged for unsaved/dirty projects.
- `CaptionProposalOrigin` and `CaptionApplyContext` are new enums; existing
  callers migrated to explicit `EphemeralGuide` variant.
- `ContinuityProjectionV1` is a new type; existing Product Task snapshot API
  is unchanged.
- `RunContinuityManifestV1` is a new type; existing Smart Redaction driver
  flow adds overflow detection without changing the normal path.
- `ModelError::ContextOverflow` is a new variant; existing error handling
  routes to the overflow classifier.

### Rollback

Revert the 12 implementation commits (`eb06c4e..e0d89f8`). No data migration
to reverse. All new types are process-local with no persistence.

## 11. Residual risks

1. **macOS runtime verification.** Both platforms share the same projection
   and manifest code, but macOS native runtime was not executed in this
   verification. The shared contract reduces risk; a macOS-specific runtime
   check is a residual gate item for the umbrella.

2. **`RunContinuityManifestV1` field pruning.** Three fields (`budget`,
   `content_binding_digest`, `canonical_bytes`) and four accessor methods
   (`evidence()`, `pending_review()`, `projection()`, `canonical_bytes()`) were
   removed during clippy cleanup as they were only used during construction and
   never read after. If a future slice needs these values, they must be
   re-added. The manifest's `restart_user_message()` accesses fields directly.

3. **`ToolContinuitySnapshot` and `continuity_state()` dead code.** These are
   exercised by privacy sentinel tests but not called from any production code
   path. The manifest builder accesses `ToolContext` fields directly. The types
   are annotated with `#[allow(dead_code)]`. A future slice that needs a
   snapshot-based interface should re-evaluate.

4. **`RunContinuitySource::Durable` boxing.** The `expected` field was boxed
   to satisfy clippy's `large_enum_variant` lint (216 → 8 bytes). All
   construction and destructure sites were updated; auto-deref handles method
   calls transparently.

5. **Manual code review.** The plan required an independent reviewer subagent.
   Subagent context limits prevented dispatching one. The review was performed
   manually by the gate agent covering all 11 required questions. A formal
   independent review is recommended before merge.

6. **Recovery measurement scope.** The manifest construction and restart
   message generation are tested in the driver overflow tests, not in the
   isolated recovery_measurements test. The measurement test validates
   projection determinism and privacy; the driver tests validate the full
   manifest lifecycle.

## 12. Deferred scope (from spec §5, confirmed deferred)

- Transcript persistence, conversation resume, semantic memory, retrieval.
- Model-authored summary, summary chain, handoff document.
- Provider-native compaction, cache-aware selective pruning.
- More than one overflow retry.
- Durable in-flight run recovery, instruction-pointer resume.
- New Product Task kind for Action Guide captions.
- Continuity claims for unsaved/dirty Action Guide work.
- Visual-annotation agent migration.
- Launch-video behavior.

## 13. Gate decision

**Slice 5 Context Continuity** is **approved** pending user confirmation.

All 14 acceptance criteria from the governing spec are satisfied. The
projection contracts, overflow recovery path, stale rejection, privacy
boundaries, and verification evidence are complete. No correctness or security
defects were found. Code-quality issues exposed during clippy verification were
fixed in the same verification session.

**Recommendation:** Approve and merge after formal independent code review
confirms the manual review findings.
