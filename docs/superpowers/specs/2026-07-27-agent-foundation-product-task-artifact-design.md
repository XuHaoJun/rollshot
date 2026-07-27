# Rollshot Agent Foundation: Product Task and Artifact Promotion Design

**Date:** 2026-07-27  
**Status:** Approved in brainstorming auto mode  
**Umbrella:**
[`2026-07-26-agent-foundation-umbrella-design.md`](2026-07-26-agent-foundation-umbrella-design.md)  
**Research source:**
[`docs/researchs/agent-foundation/`](../../researchs/agent-foundation/)  
**Slice:** 2 of 6 — Phase 1, Durable Product Contracts

## 1. Purpose

This slice gives one Smart Redaction request a durable product-owned identity
around its bounded agent run, promotes a successful `ReadyForReview` handoff
into a typed revision-bound artifact, and records the user's eventual review
decision against that exact artifact revision.

The result is deliberately smaller than a workflow platform:

- one Product Task represents one user-authorized Smart Redaction request;
- one task contains one current bounded attempt in this slice;
- `AgentRunner` continues to own only the live run;
- the product owns task, artifact, review, and document truth;
- task snapshots, not transcripts or Rig state, are the durable recovery unit;
  and
- no task dependency, scheduler, child agent, job, or expected-output graph is
  introduced.

## 2. Start condition and current-code drift

Gate G0 is satisfied in the current tree:

- `spikes/provider-boundary/FINDINGS.md` is `retained-reference`, records user
  approval on 2026-07-27, and names decision commit `2b15a80`;
- `rollshot-agent` now uses exact `rig-core = 0.40.0`;
- host-owned establishment and item-poll bounds are implemented; and
- incomplete provider turns are rejected before authoritative commit.

Material drift from the research baseline:

1. The research described Rig 0.39; current code has completed the private 0.40
   migration without exposing Rig types.
2. `RunId` exists in `rollshot-agent::domain` but is an unused `u64` wrapper.
   Smart Redaction proposal provenance currently writes `SessionId` into the
   `run_id` field, so session and run identity are not actually distinct.
3. `DryRunTool` still creates `ProposalId(1)` and
   `base_document_state_id: 0`; the workbench later restamps the proposal to the
   live state immediately before apply. That protects the current apply call
   from malformed geometry but erases original source binding and cannot
   support durable stale-artifact rejection.
4. `ReviewDecision::resulting_document_state_id` is still populated before
   `ImageDocument::apply_batch`, although its contract says it is the state
   after apply.
5. `ReadyForReview`, `RunBudget`, and live workbench state remain in memory.
   `EditProposal`, `ValidatedAutomation`, and `ReviewDecision` are already
   serializable.
6. `AuditEvent` remains declared but is not a production durable log. Slice 2
   does not activate it; durable audit observability remains Slice 6.

## 3. Goals

1. Distinguish Product Task, attempt, run, proposal, artifact, and artifact
   revision identities.
2. Bind an artifact to content identity—base-image SHA-256 plus canonical
   annotation-state digest and state ID—rather than to the non-global state ID
   alone.
3. Persist a task's `running` state before provider work begins.
4. Atomically promote `ReadyForReview` into one task snapshot containing the
   typed artifact and transition the task to `ready_for_review` before the UI
   receives the terminal.
5. Restore the same pending review artifact after reopening Smart Redaction
   only when its source content and document state still match.
6. Reconcile a task left `running` or `applying` by a process crash to an honest
   interrupted terminal; never infer provider/tool/document success.
7. Reject stale artifacts deterministically before document mutation.
8. Bind approve/reject decisions and local review edits to the exact artifact ID
   and revision and record the real post-apply document state.
9. Keep every filesystem operation off the iced update thread and correlate
   every asynchronous run, restore, and review message to its originating
   task/run/operation.
10. Keep persisted data privacy-bounded and remove review payloads after a
    terminal decision.
11. Preserve current Smart Redaction budgets, cancellation, validation, dry-run,
    proposal review, and provider-neutral contracts.

## 4. Non-goals

- More than one concurrently active attempt per Product Task.
- Automatic retry, retry backoff, provider fallback, or provider handoff.
- Task dependencies, readiness, leases, a workflow DAG, or a scheduler.
- Child agents, fan-out, parallel tool calls, or background jobs.
- Generic expected-output completion contracts.
- Cross-run transcript, `AgentSession`, or Rig `AgentRun` persistence.
- Durable reconstruction of a provider stream, tool future, consent, or
  authority from stored prose.
- Durable `ImageDocument` persistence. Content binding and applying intent avoid
  false approval but do not make unsaved edits survive process death.
- Action Guide proposal migration in this slice.
- Artifact revision editing, publication, export receipts, archive UI,
  user-configurable retention, or a universal content-addressed blob store.
- An indexed/database-backed TaskStore or arbitrary task-count ceiling before
  measured scale requires one.
- Durable audit-event production; Slice 6 owns that concern.
- New Smart Redaction controls, layout, copy, theme, or visual tokens.

## 5. Considered approaches

### 5.1 Selected — app-owned atomic task snapshot with embedded typed artifact

Add framework-neutral Product Task and artifact contracts to
`rollshot-agent`, while the product app owns a filesystem `TaskStore` under its
configuration root. One JSON file is the authoritative snapshot for one task.
The snapshot embeds a pending Smart Redaction review artifact; a single atomic
replacement commits task transition and artifact handoff together.

This follows the existing ownership boundary, preserves typed domain payloads,
and gives crash reconciliation without creating a general ledger.

### 5.2 Rejected — new universal artifact-ledger crate

A new crate could define product-wide heads, immutable revisions, publication,
retention, and expected outputs. Slice 2 has only one required concrete payload
and no publication or cross-product scheduler. The additional crate, migration,
and generic storage abstractions would spend complexity before a second
consumer proves the boundary.

### 5.3 Rejected — extend `rollshot-preset`

`rollshot-preset` already has atomic revision storage, but a provider attempt,
review handoff, interruption, or rejected proposal is not a preset revision.
Reusing that store would couple transient task lifecycle to reusable automation
ownership and create misleading APIs.

## 6. Ownership and module boundaries

```text
rollshot-app (product owner)
├── creates ProductTask + Attempt + RunId
├── owns TaskStore and retention
├── captures source document state
├── promotes ReadyForReview before UI delivery
├── restores a compatible pending artifact
└── owns review decision + deterministic document apply

rollshot-agent (bounded execution + framework-neutral contracts)
├── ProductTask / TaskAttempt state contracts
├── ProductArtifact metadata and SmartRedactionReviewPayload
├── transition validation and crash reconciliation
├── AgentRunner live execution
└── ToolContext emits proposal with real RunId + source state

rollshot-edit-proposal
├── EditProposal / ProposalId / candidate payload
├── privacy-safe agent provenance carrying the distinct RunId
└── ReviewDecision / lowering

rollshot-image-document
└── authoritative in-memory document state and atomic apply_batch
```

Code location does not change authority: placing serializable contracts in
`rollshot-agent` does not let `AgentRunner` create, approve, or persist Product
Tasks. The app supplies IDs, timestamps, source binding, provider/model
provenance, and storage.

## 7. Identity model

All externally serialized IDs are opaque validated strings. The app generates
UUID-v4 values with stable prefixes; persisted code never derives authority
from a prefix.

| Identity | Example | Owner | Meaning |
|---|---|---|---|
| `ProductTaskId` | `task-<uuid>` | Product app | One user-authorized Smart Redaction request |
| `TaskAttemptId` | ordinal `1` | Product task | One bounded execution attempt inside that task |
| `RunId` | `run-<uuid>` | Product app, consumed by runner/tool context | One `AgentRunner` invocation |
| `ProposalId` | `proposal-<uuid>` | Product app, consumed by dry-run tool | Domain proposal identity; never used as a task or run ID |
| `ArtifactId` | `artifact-<uuid>` | Product app | Logical promoted review artifact |
| `ArtifactRevision` | `1` | Product artifact | Immutable payload revision reviewed by the user |

The Product Task snapshot stores all six relations explicitly. The legacy
`RunId`, `ProposalId`, and `ProvenanceSource::Agent` numeric values change to
opaque string IDs. This is an internal schema migration with no currently
persisted proposal store to migrate. `CandidateId` remains a proposal-local
numeric sequence because it identifies candidates only within one proposal.

A task created by this slice contains one attempt with ordinal `1`. The schema
uses a vector so a later approved retry design can append rather than overwrite
attempt evidence, but Slice 2 exposes no automatic or UI retry transition.

## 8. Product Task snapshot

The version-1 snapshot is immutable to callers except through reducer methods.
Its fields are private and exposed through read-only accessors.

```text
ProductTaskSnapshot
├── store_schema_version = 1
├── snapshot_revision
├── task_id
├── kind = smart_redaction_author | smart_redaction_improve
├── source_binding
│   ├── base_image_sha256
│   ├── annotation_state_sha256
│   ├── document_state_id
│   ├── preset_id
│   └── optional active_preset_revision_id
├── status
├── attempts[]
│   ├── attempt_id
│   ├── run_id
│   ├── started_at_unix_ms
│   ├── finished_at_unix_ms?
│   └── terminal?
├── artifact_metadata?
├── pending_artifact_payload?
├── review_receipt?
├── created_at_unix_ms
└── updated_at_unix_ms
```

`base_image_sha256` hashes the immutable RGBA source once when the Result
Workspace is created and is cached for the workspace lifetime. The canonical
annotation-state digest hashes a versioned DTO containing image dimensions,
ordered annotations, and the document state ID. Two separate documents with
identical source pixels and identical annotations are content-compatible by
design; numeric `state_id` alone is never sufficient.

`snapshot_revision` increments exactly once per successful transition and is
the optimistic-concurrency token used by `TaskStore::compare_and_swap`. A file
lock serializes writes; exact CAS prevents a later lock holder from applying a
transition based on a stale same-status snapshot.

`TaskTerminal` is product-level and privacy-bounded: `needs_user_input`,
`cancelled`, `budget_exhausted { dimension }`, `source_validation_failure`,
`runtime_failure`, `agent_protocol_failure`, `provider_failure`, `interrupted`,
and `stale`. Provider/agent error strings remain live UI data and are not
persisted.

## 9. State machine

```text
created
   │ CAS running snapshot committed
   ▼
running
   ├── ReadyForReview promoted + CAS ──▶ ready_for_review
   ├── NeedsUserInput ─────────────────▶ needs_user_input
   ├── Cancelled ──────────────────────▶ cancelled
   └── bounded failure ────────────────▶ failed

ready_for_review
   ├── source mismatch ────────────────▶ stale
   ├── user rejects ───────────────────▶ rejected
   └── CAS applying intent ────────────▶ applying
                                             ├── CAS receipt ──▶ completed
                                             └── pre-commit failure + compensation
                                                                  └── ready_for_review

startup reconciliation
   running  ──CAS──▶ interrupted
   applying ──CAS──▶ interrupted
```

Rules:

1. Every transition consumes an exact expected snapshot, checks current status,
   and produces revision `expected + 1`.
2. Transition timestamps are monotonic.
3. Artifact payload and `ready_for_review` enter one serialized snapshot.
4. `running` and `applying` are never resumed after process restart.
5. Restore/apply requires exact source-content, state, artifact digest, and
   provenance agreement.
6. Terminal status cannot return to `running` in Slice 2.
7. Public code cannot forge snapshots by mutating fields directly.

## 10. Typed artifact promotion and canonical digests

### 10.1 Artifact metadata

`ProductArtifactMetadata` contains artifact ID/revision/kind/schema, canonical
payload SHA-256, complete source binding, task/attempt/run/proposal identities,
provider and model IDs, a privacy-filtered run-config digest, validation
receipt summaries, and creation time. A digest is an integrity check, never an
authority token.

### 10.2 Smart Redaction review payload

The first payload contains `ValidatedAutomation`, the original immutable
`EditProposal`, successful dry-run metrics, source generation, and validation
summary. It excludes image bytes, user/assistant messages, transcripts,
provider responses/credentials, raw OCR, unrestricted tool arguments/results,
cancellation handles, Rig state, sessions, and futures.

### 10.3 Canonical V1 bytes

One shared V1 helper creates every source, annotation-state, config, proposal,
and artifact digest. It serializes fixed deny-unknown-fields DTOs whose maps are
`BTreeMap`s and whose fields appear in schema order. Before serialization it
rejects non-finite floats, oversized strings/collections, and unsupported
schema values. `serde_json::to_vec` is used only inside this helper; call sites
cannot invent alternate bytes.

The privacy-filtered config DTO contains provider ID, model ID, payload mode,
run kind, and bounded run-budget values. It never contains API keys, paths,
prompts, OCR text, or environment variables. Golden-byte/digest tests and
reordered-map tests freeze the V1 contract; a future change requires a new
schema version rather than silent digest drift.

### 10.4 Validation receipts

V1 records: (1) automation schema/API versions plus canonical source digest;
and (2) dry-run generation, candidate count, affected area, proposal digest,
and complete source binding. Promotion rejects inconsistent validated/handoff
source, task/attempt/run/proposal identity, generation, state, or config.

## 11. Persistence, CAS, and commit visibility

### 11.1 Layout and privacy

```text
<rollshot-config>/agent-tasks/       # mode 0700 on Unix
├── .lock                            # mode 0600
└── tasks/
    └── task-<uuid>.json             # mode 0600
```

IDs are validated before path construction. Symlinks and non-regular files fail
closed. A task file larger than 4 MiB is rejected from metadata before
allocation. Errors and tracing use bounded categories without absolute paths or
payload `Debug` output.

### 11.2 Exact compare-and-swap

Every mutation locks the store, loads and validates the current snapshot,
compares it byte-for-byte/structurally with the caller's expected snapshot,
validates the reducer-produced replacement revision, serializes and
re-validates it, writes a unique sibling temp, syncs the file, renames, and
syncs the parent directory. CAS conflict is typed and does not retry or merge.

### 11.3 Commit-point outcomes

Atomic write distinguishes:

- `PreCommitFailure`: rename did not make replacement visible; prior snapshot
  remains authoritative and compensation may restore product state;
- `Committed`: rename and directory sync succeeded; and
- `CommitVisibleDurabilityUncertain`: rename is visible and re-read matches the
  replacement, but parent-directory sync failed. Current process truth uses the
  replacement and must not blindly undo; the UI surfaces a bounded durability
  warning and startup revalidates whichever complete snapshot survives.

Tests inject file-write, file-sync, rename, and post-rename directory-sync
failures and exact CAS conflicts. The existing preset writer's best-effort
directory sync is precedent for layout only, not sufficient error semantics.

### 11.4 Run handoff ordering

```text
allocate task/run/proposal/artifact IDs once
  -> CAS-create running before vision/provider/tool work
  -> bounded run
  -> promote terminal/artifact
  -> CAS terminal snapshot
  -> only then emit task+run-correlated iced message
```

Every run event, failure, and terminal carries task and run identity. Setup
failure after `running` uses the same terminal persistence helper. Join panic
leaves `running`; reconciliation makes it interrupted.

### 11.5 Nonblocking review apply protocol

All store work runs in `spawn_blocking`; iced update/view never performs
filesystem I/O.

```text
Apply requested + operation token
  -> async CAS ready_for_review -> applying
  -> ApplyingPersisted(token, outcome)
  -> recheck active task/artifact/source and synchronously apply_batch
  -> capture actual post-state + local review delta
  -> async CAS applying -> completed + receipt
  -> ReceiptPersisted(token, outcome)
```

Candidate gestures and other document mutations are disabled while a review
operation is active. Every completion checks task ID, artifact revision, and
operation token; stale completions are ignored.

On pre-commit final-receipt failure after a mutating apply, `undo()` is called
without assertion, its result/state are checked, then compensation CAS returns
the task to ready. A zero-op apply performs no undo. If final receipt became
commit-visible, the document is not undone. Rollback/compensation failure leaves
`applying` for honest interrupted reconciliation.

## 12. Staleness and review receipts

`restamp_proposal` is removed. Restore and apply require agreement among task
binding, artifact binding, proposal state, current source/image/state digests,
artifact digest/revision, and task/attempt/run/proposal provenance. Unrelated
source-image tasks are ignored rather than marked stale; the newest compatible
source task is considered. A same-source task with changed annotation state is
atomically marked stale and never shown as applicable.

The promoted artifact remains immutable. Existing review interaction may move
artifact candidates or add manual candidates. `ReviewReceipt` therefore binds
the exact artifact ID/revision and records a typed `LocalReviewDeltaV1`:
validated modifications to artifact candidate IDs plus complete validated
manual additions with local provenance. This preserves current behavior without
pretending local additions were in the original artifact.

An applied receipt contains task/artifact/proposal IDs, applied/rejected
candidate partition, local delta, reviewed source binding, actual resulting
document state ID/digest, decision time, and actor `local_user`. A reject
receipt has no resulting state and never applies. Payload is cleared only after
a terminal CAS outcome is commit-visible.

## 13. Startup recovery and iced message correlation

Entering Smart Redaction allocates a restore-operation token and starts
asynchronous reconcile/source-scoped lookup. The completion carries token and
source binding. It mutates state only if token, workspace source digest, and
current document state still match. A newer run or restore invalidates older
tokens.

All `RunEvent`, `RunFailed`, `RunTerminal`, applying, receipt, compensation, and
reject completions carry task/run or review-operation identities. Reducer tests
deliver old completions after newer work and prove they cause no mutation.

A compatible pending artifact restores through the existing review UI without
provider call. Persistence never restores consent, authority, assistant text,
conversation, or `AgentSession`. No new controls or layout are introduced.

## 14. Retention and privacy

`ready_for_review` retains the bounded payload. `completed`, `rejected`, and
`stale` clear it only after commit-visible transition while retaining metadata,
digest, source binding, and review receipt. Terminal metadata is pruned after
30 days; pending review has no age-based deletion in V1. Store maintenance
removes only its own unreferenced temp prefix.

The base-image digest is cached, but image bytes are never stored. Unix
directories/files use 0700/0600 where supported. Custom bounded `Debug`,
`Display`, serialization, permission, and tracing tests prove that paths,
pixels, source text outside the pending payload contract, prompts, credentials,
raw OCR, provider-native values, and unrestricted errors do not leak.

## 15. Error model

Contract errors include invalid ID, illegal transition, timestamp/revision
regression, missing/conflicting attempt, provenance/digest/source mismatch,
non-finite canonical value, and stale source. Store errors include not found,
corrupt/oversize/unsupported schema, unsafe path/file type, CAS conflict,
pre-commit failure, and commit-visible durability uncertainty. Promotion and
review errors remain typed at their own boundaries. Durable records and tracing
never persist raw provider, prompt, payload, or absolute-path error strings.

## 16. Testing strategy

### 16.1 Contract, migration, and canonicalization

- dedicated workspace-wide `RunId`/`ProposalId` string migration tests;
- opaque ID serde and wrong-prefix/path rejection;
- private reducer legal/illegal transition and exact revision tests;
- running/applying reconciliation and terminal non-restart;
- canonical golden bytes/digests, reordered maps, finite-float rejection,
  config-secret exclusion, and cross-document content-binding tests; and
- payload clearing plus privacy-safe serialization/`Debug`.

### 16.2 Store and commit-boundary tests

- create/load/exact-CAS round trip and competing-writer conflict;
- unsupported schema, corrupt/oversize JSON, digest tamper, unsafe ID, symlink,
  non-regular file, and Unix mode rejection;
- injected temp-write, file-sync, rename, and post-rename directory-sync
  outcomes, including re-read classification;
- persisted startup reconciliation, 30-day pruning, orphan-temp cleanup, and
  deterministic many-file O(n) maintenance; and
- lock held only for load/CAS/write, never provider/document/iced waits.

### 16.3 Promotion, recovery, and review integration

- real task/run/proposal/document content identity through `ToolContext`;
- running persisted before every setup/provider path;
- terminal/artifact persisted before correlated iced delivery;
- setup/persistence failure yields no reviewable proposal;
- source-scoped restore ignores unrelated same-state tasks;
- source/state change makes only the related artifact stale;
- late restore/run/review messages are ignored by token/identity;
- apply binds exact artifact and actual resulting state/digest;
- local candidate modifications/additions appear in the receipt;
- zero-op apply, pre-commit receipt failure rollback, commit-visible sync
  uncertainty, compensation failure, and discard/reject paths; and
- existing Smart Redaction/provider/driver contracts remain green.

### 16.4 iced UI evidence

Auto-mode UI verification uses deterministic temp storage at 1100×760 and
640×420. Expected is the existing in-memory pending review; actual is the same
review restored from `TaskStore`; stale has no enabled Apply. Simulator tests
use the exact fixture-controlled label `Apply 1 redactions`, verify visible
bounds and emitted click message, and use stable candidate selectors rather
than ambiguous text.

Expected/restored PNGs and ImageMagick `compare -metric AE` diffs must report
exactly `0` changed pixels. The product-changing agent does not approve
baselines. A clean-context semantic reviewer receives requirement, manifest,
structural output, and all expected/actual/diff images; no golden update is
allowed.

```text
Visual capability: semantic
Provider: native:read
Probe: crates/rollshot-app/tests/eval/fixtures/url_bar/image.png — passed
Pixel diff: /usr/bin/compare
CI: artifact-only
```

No Linux/macOS capture overlay path changes. Only the shared Result Workspace
is affected; native capture UI runtime verification is not required.

## 17. Acceptance criteria and Gate G1

Gate G1 requires:

1. Every persisted proposal traces to task, attempt, run, proposal, artifact,
   artifact revision, base-image digest, annotation-state digest, and state ID.
2. `running` is durable before vision/provider/tool work.
3. Ready artifact and task transition are CAS-committed before correlated UI
   delivery.
4. Compatible content restores without provider call; unrelated same-state
   documents never restore or stale each other's tasks.
5. Running/applying reconcile to interrupted with no inferred partial success.
6. Exact CAS rejects competing stale writers.
7. Canonical V1 digest golden/adversarial tests pass.
8. Source/digest/provenance mismatch causes zero document mutation.
9. Every asynchronous completion is task/run/operation-correlated and stale
   messages are ignored.
10. Apply/reject receipts bind exact artifact revision; applied receipts include
    actual post-state and typed local review delta.
11. Pre-commit receipt failure compensates safely; commit-visible durability
    uncertainty never triggers contradictory rollback; zero-op paths do not undo.
12. No iced-thread filesystem I/O or product-path rollback assertion exists.
13. No pixels, transcript, user/assistant text, credentials, raw OCR, absolute
    paths, or provider-native payload serialize/log outside the explicitly
    bounded pending payload contract.
14. Pending payload clears after commit-visible completed/rejected/stale; terminal
    metadata prunes after 30 days; 4 MiB pre-read bound and Unix modes pass.
15. Existing provider contracts, budgets, cancellation, validation, dry-run,
    manual review behavior, and UI remain green.
16. Both viewport scenarios and AE=0 evidence pass clean semantic review.
17. Formatting, affected suites, lane-correct clippy, diff check, and independent
    code review pass.

## 18. Required verification

- `rtk cargo test -p rollshot-edit-proposal`;
- `rtk cargo test -p rollshot-agent`;
- focused Product Task/store/workbench ordering and rollback tests;
- `rtk cargo test -p rollshot-app`;
- affected automation, rquickjs, vision, and Action Guide suites changed by the
  identity migration;
- `rtk cargo fmt --check`;
- `rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings`
  plus the repository's dedicated OCR lane only if OCR code changes (it should
  not in this slice);
- `rtk git diff --check`;
- privacy/schema/tracing bounded searches;
- exact AE visual evidence and clean-context semantic review; and
- independent code review against this spec and plan.

## 19. Residual risks

- `ImageDocument` remains memory-only. The applying-intent protocol prevents a
  false durable approval but does not make unsaved document edits survive a
  process crash.
- UUID generation and task-file persistence rely on the local filesystem and
  one product store lock; network/distributed writers are unsupported.
- A pending review payload includes automation source and candidate geometry
  until review completes. It contains no image pixels, but it remains
  privacy-sensitive local product data.
- Fixed 30-day metadata retention has no user-facing control in this slice.
- The first artifact kind does not prove that Action Guide or publication
  artifacts should use the same payload/store shape.
- Crash/power-loss guarantees remain bounded by the host filesystem's file and
  directory sync semantics.

## 20. Outputs

This slice produces:

1. this child design spec;
2. an implementation plan at
   `docs/superpowers/plans/2026-07-27-agent-foundation-product-task-artifact.md`;
3. Product Task, attempt, artifact, review-receipt, and transition contracts;
4. an app-owned atomic task snapshot store;
5. Smart Redaction promotion, recovery, stale rejection, and truthful review
   integration;
6. deterministic persistence/UI/privacy evidence;
7. independent visual and code review results; and
8. a Gate G1 decision recorded after implementation verification.

Only after Gate G1 passes may the umbrella proceed to Slice 3, Authority and
Static Skills.
