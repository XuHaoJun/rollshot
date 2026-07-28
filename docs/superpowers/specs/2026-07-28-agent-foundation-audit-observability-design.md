# Agent Foundation Slice 6: Durable Audit Observability Design

**Date:** 2026-07-28  
**Status:** Approved in brainstorming auto mode  
**Parent:**
[`2026-07-26-agent-foundation-umbrella-design.md`](2026-07-26-agent-foundation-umbrella-design.md)  
**Slice:** 6 of 6 — Durable Audit Observability

## 1. Decision summary

Slice 6 adds durable, privacy-safe evidence for material Smart Redaction Product
Task transitions without making audit records authoritative product state.
`ProductTaskSnapshot` remains the source of truth. Existing `RunEvent` values
remain transient display hints and may be dropped.

The selected design has two layers:

1. `rollshot-agent` owns a provider-neutral versioned audit vocabulary,
   correlation model, transition validation, and append-sink contract.
2. `rollshot-app` owns a per-Product-Task append-only journal beside the existing
   `TaskStore`. Material task mutations use a write-ahead prepare → task snapshot
   commit → audit commit protocol. Startup reconciliation resolves prepared
   records from authoritative task state before new audited mutations are
   admitted.

Each physical journal record has a monotonic sequence and previous-record hash.
An append is acknowledged only after the complete record is written and the file
is synchronized. A truncated unacknowledged tail can be removed during startup;
an interior parse, sequence, or hash failure is corruption and fails closed.

The first product-path proof is the existing Smart Redaction author/improve
workflow. In that workflow, `complete_apply` is the product publication/commit
boundary: the reviewed artifact has been applied to the authoritative image
document state. External Save/Export is not Product-Task-bound today and is not
given invented audit semantics in this slice.

## 2. Start readiness and current-code drift

Slice 6 depends on Slice 2 and defaults to starting after Gate G2. The current
`main` satisfies that start condition:

- Slice 2 Product Task and Artifact Promotion is merged.
- Slice 3 Authority and Static Skills is merged.
- Slice 4 Live Job Registry is merged but is not a Slice 6 dependency.
- Slice 5 Context Continuity is merged but is not a Slice 6 dependency.

Slice 5's gate record is not fully closed. It is still marked “Verified, pending
user approval,” and records that the plan-required formal independent review was
not performed. That is an umbrella/G3 completion blocker to close separately.
Slice 6 must not absorb, restate, or silently waive it. It does not prevent Slice
6 design or implementation because the umbrella makes Slice 6 eligible after
Gate G1 and places its default start after Gate G2.

Material drift from the umbrella's research baseline:

- `crates/rollshot-agent/src/runtime.rs` contains an `AuditEvent` enum with only
  `TurnStarted`, `BudgetCharged`, `CancellationRequested`, and `RunCompleted`.
  LSP finds no production references; only two serialization tests use it. This
  is dormant vocabulary, not an active audit path.
- `ProductTaskSnapshot` now owns typed task, attempt, run-contract, artifact,
  review, terminal, and revision state. V2 run-contract receipts bind authority
  and skill-use digests to the active attempt and promoted artifact.
- `TaskStore` already provides exact CAS under an exclusive lock, sibling-temp
  write, file sync, rename, parent-directory sync, commit visibility
  classification, crash reconciliation, and 30-day terminal pruning.
- Smart Redaction task persistence remains app-owned under
  `<config>/agent-tasks/`; `rollshot-agent` remains filesystem-neutral.
- `RunEventSink` feeds a bounded `try_send` channel. Its display events are
  deliberately lossy, and the result workspace already repairs terminal display
  state from correlated terminal/task state.
- Authority denial is typed as `ToolError::AuthorityDenied { tool, operation }`
  at the fail-closed tool boundary, but it currently becomes only a transient
  driver failure and structured diagnostic.
- There is no agent-owned external publication model. The durable product commit
  available to this slice is the reviewed artifact application recorded by
  `ProductTaskSnapshot::complete_apply`.

No new user-facing iced UI is required. The design changes persistence and
state-repair behavior only.

## 3. Problem

The current product can persist authoritative task and artifact state, but it
cannot answer, after restart, which material transitions were observed durably:

- a Product Task was created or an attempt started;
- an immutable authority and skill-use contract was bound;
- an authority check denied a tool call;
- a validated artifact was promoted for review;
- review began, committed an application, or rejected the artifact; or
- a task ended in a typed failure, cancellation, interruption, or stale state.

Appending an audit record after a task CAS is insufficient: a crash can commit
the task state and lose the audit append. Appending before a CAS is also
insufficient: the journal can claim a transition that never became product
truth. The slice therefore needs a bounded transaction protocol that preserves
product-state authority while making every completed material transition
recoverably auditable.

The audit path must not retain screenshot pixels, model prose, tool arguments,
proposal payloads, credentials, authority grants, or full skill bodies. It must
also avoid turning the result workspace into an audit-replay client or the task
store into a general event-sourcing system.

## 4. Goals

1. Define a versioned, provider-neutral audit envelope and closed material-event
   vocabulary.
2. Correlate events to exact Product Task, attempt, run, skill use, artifact,
   proposal, review, and product-commit identities as applicable.
3. Make acknowledged appends durable and detect silent interior loss,
   reordering, duplication, or corruption.
4. Couple Product Task mutations to audit evidence through a crash-recoverable
   write-ahead protocol.
5. Persist authority denials before returning their terminal run failure.
6. Reconcile incomplete audit transactions from authoritative task state before
   admitting new audited mutations.
7. Prove that dropping every transient `RunEvent` does not prevent the product
   from repairing visible terminal/review state from authoritative snapshots.
8. Apply the existing Product Task retention boundary to its audit evidence.
9. Preserve current provider, budget, cancellation, authority, skill, artifact,
   continuity, and review contracts except where an audit failure must fail
   closed.

## 5. Non-goals

This slice does not add:

- event-sourced Product Tasks or audit-driven state reconstruction;
- reconnectable event replay, remote audit streaming, or a telemetry backend;
- audit-backed UI history, filtering, search, export, or user-visible copy;
- screenshot pixels, image payloads, prompt/source prose, tool arguments or
  results, raw Action Guide semantic input, provider messages, credentials,
  authority grants, or complete skill bodies in audit storage;
- Action Guide project history, non-agent Save/Export tracking, or publication
  semantics without a Product Task binding;
- audit evidence for transient token chunks, tool progress, source diffs, turn
  boundaries, budget charges, or routine cancellation polling;
- cross-task global ordering beyond physical append order inside one task
  journal;
- signatures, remote attestation, encryption-at-rest, key management, or an
  adversarial tamper-proof ledger;
- schema negotiation, arbitrary event plugins, dynamic event kinds, or a generic
  event bus;
- durable job recovery, workflow scheduling, child agents, fan-out, or launch
  video behavior; or
- closure of Slice 5's pending approval and formal-review evidence.

## 6. Considered approaches

### 6.1 Selected — per-task hash-chained journal with write-ahead transition records

Each Product Task has one bounded append-only journal. Material state mutations
append and sync a prepared audit transaction, commit the authoritative snapshot
through existing TaskStore CAS, then append and sync the transaction commit.
Startup reconciliation decides any unresolved prepare by comparing its expected
and replacement revisions and transition receipt with the authoritative task
snapshot.

This adds more protocol code than best-effort logging, but it is the minimum
approach that closes both crash windows without moving product truth into the
journal. Per-task files align correlation, locking, and retention with existing
TaskStore ownership.

### 6.2 Rejected — append after successful TaskStore CAS

This is simple in normal execution, but a crash between CAS and append leaves a
material transition with no evidence. A later startup cannot reconstruct every
intermediate transition from only the newest snapshot. A synthetic “state
observed” event would disclose the gap rather than satisfy the gate.

### 6.3 Rejected — embed audit history in `ProductTaskSnapshot`

Embedding events in each rewritten snapshot makes state and evidence atomic, but
it is not append-only on disk, increases snapshot size on every transition, and
blurs authoritative state with audit history. It also pushes the Product Task
model toward event sourcing and complicates its current 4 MiB snapshot bound.

### 6.4 Deferred — SQLite or a global segmented audit database

A database can provide durable appends and richer queries, but it does not make
the existing filesystem snapshot and a separate database transaction atomic.
It would still require a coordination protocol and would introduce a second
storage engine for a single bounded workload. Reconsider only when multiple
Product Task families require shared audit queries or retention policies.

## 7. Ownership and architecture

```text
rollshot-agent
├── AuditEventV1 / AuditEnvelopeV1 / AuditCorrelationV1
├── MaterialTaskTransitionV1 validation
├── AuditAppendSink async contract
└── typed audit failure categories

rollshot-app
└── <config>/agent-tasks/
    ├── .lock
    ├── tasks/task-<uuid>.json       authoritative ProductTaskSnapshot
    └── audit/task-<uuid>.jsonl      append-only evidence journal

Material task mutation
validate old/new snapshot + derive event
        │
        ▼
append Prepared(event, old_revision, new_revision) + sync
        │
        ▼
existing exact TaskStore CAS / create
        │
        ├── no state commit → append Aborted + sync
        │
        └── state commit visible
                │
                ▼
        append Committed + sync
                │
                ▼
        acknowledge product transition
```

Ownership rules:

- Product reducers own legal state transitions.
- `ProductTaskSnapshot` and the image document remain authoritative product
  state.
- `rollshot-agent::audit` owns event meaning, correlation validation, privacy
  bounds, and storage-neutral append semantics.
- `rollshot-app::result_workspace::workbench::audit_store` owns files, locking,
  synchronization, hash-chain validation, startup reconciliation, and retention.
- `TaskStore` owns task snapshot commit visibility. An audited wrapper or
  audited methods coordinate the journal and existing create/CAS operations
  under the same task-store lock; raw material-transition writes are migrated
  away from product callsites.
- `AgentRunner` owns emitting the already-typed authority-denial event through
  an injected audit sink before it converts the denial to a terminal result.
- `RunEventSink` remains display-only and has no durability obligation.
- The UI reconstructs from task/document state, never from audit replay.

The concrete file-store type must not enter `rollshot-agent` public APIs. The
sink contract must not expose app paths, filesystem errors, or provider types.

## 8. Audit domain contract

### 8.1 Identity and envelope

`AuditEnvelopeV1` is immutable after construction and contains only:

- `schema_version = 1`;
- opaque `AuditEventId` generated by the product host;
- `occurred_at_unix_ms` supplied by the product boundary;
- one closed `AuditEventV1` variant;
- one validated `AuditCorrelationV1`; and
- an event payload digest computed from canonical V1 bytes.

Physical journal sequence and chain fields belong to the storage record, not the
domain event:

- per-task `sequence: u64`, starting at zero;
- `previous_record_sha256`, absent only for the first retained record;
- `record_sha256` over canonical record fields excluding itself; and
- a private physical record kind: `prepared`, `committed`, `aborted`,
  `standalone`, or `bootstrap`.

`AuditEventId` is stable across prepare/commit records. Retry after an uncertain
append uses the same event ID and transaction ID. A different payload under an
existing ID is corruption, not idempotency.

### 8.2 Correlation

Every event has `task_id`. Optional fields become mandatory per event variant:

- `attempt_id` and `run_id` for attempt/run events;
- authority snapshot digest and policy revision for run-contract or denial
  evidence;
- skill source authority, package ID, resource ID, package digest, invocation
  kind, and catalog revision for skill-use evidence;
- artifact ID, artifact revision, kind, schema version, canonical payload
  digest, proposal ID, and exact source binding digest for artifact/review
  evidence; and
- resulting document state ID and digest, when available, for an applied product
  commit.

Correlation validators reject missing, contradictory, oversized, malformed, or
cross-task fields before any append. They retain receipts and digests, never the
underlying authority grants, skill body, proposal body, or document contents.

### 8.3 Material event vocabulary

V1 defines only these logical events:

1. `TaskCreated` — a new Product Task snapshot became authoritative.
2. `AttemptStarted` — a bounded attempt/run entered `Running`.
3. `RunContractBound` — the exact immutable authority receipt and `SkillUse`
   receipt became bound to the active attempt. This is the durable skill-use
   transition; no separate event duplicates the same state change.
4. `AuthorityDenied` — a named registered tool and required `RunOperation` were
   denied against the current authority snapshot before the tool body ran.
5. `ArtifactPromoted` — a validated artifact revision and proposal became
   `ReadyForReview`.
6. `ReviewApplyStarted` — the authoritative task entered `Applying` before the
   image-document side effect.
7. `ReviewDecisionCommitted` — the exact artifact revision was either rejected
   or applied. The applied variant includes the resulting document receipt and
   is the current Smart Redaction product publication/commit event.
8. `TaskTerminated` — the task entered one typed non-review terminal: needs user
   input, cancelled, budget exhausted, source validation failure, runtime
   failure, protocol failure, provider failure, interrupted, stale, context
   overflow, or context recovery failure.

No event is emitted for `TurnStarted`, `BudgetCharged`, ordinary
`CancellationRequested`, text chunks, tool start/end, source diffs, or
`TurnComplete`. The dormant `runtime::AuditEvent` vocabulary is removed rather
than retained as a second convention.

### 8.4 Product transition mapping

The event derivation validator accepts exact old/new snapshots and permits only:

| Product operation | Required event |
|---|---|
| `TaskStore::create` | `TaskCreated` |
| `start_attempt` | `AttemptStarted` |
| `bind_run_contract` | `RunContractBound` |
| `record_ready_for_review` | `ArtifactPromoted` |
| `begin_apply` | `ReviewApplyStarted` |
| `complete_apply` | `ReviewDecisionCommitted::Applied` |
| `reject` | `ReviewDecisionCommitted::Rejected` |
| `record_terminal`, `mark_stale`, `reconcile_interrupted` | `TaskTerminated` |

It proves task identity, `old_revision + 1 == new_revision`, legal status pair,
timestamp monotonicity, and exact attempt/run/artifact/review/run-contract
receipts. No caller may supply an arbitrary event label for a snapshot pair.

`AuthorityDenied` is standalone because it does not mutate the Product Task.
It still requires a currently bound task/attempt/run/authority correlation and
must be durably appended before the driver returns the denial terminal.

## 9. Persistence protocol

### 9.1 Layout and file safety

The app adds:

```text
<config>/agent-tasks/
├── .lock
├── tasks/
└── audit/
    └── task-<uuid>.jsonl
```

The existing task ID path validator is reused. The audit directory is mode 0700
and files are mode 0600 on Unix. Reads reject symlinks, special files, task-ID
filename mismatch, unsupported schemas, and files over the explicit journal
bound before allocating for the whole file.

Each JSON record occupies exactly one newline-terminated line. JSON strings
escape embedded newlines. Record serialization is canonical and bounded. The
store never uses debug strings or provider/native errors as payload fields.

### 9.2 Append acknowledgement

Under the existing exclusive store lock, append performs:

1. validate the current journal through its last complete record;
2. assign the next checked sequence and previous hash;
3. serialize one bounded record;
4. `write_all` the record and newline;
5. synchronize the file;
6. synchronize the parent directory when creating the journal; and
7. return an acknowledgement containing event ID, transaction ID when present,
   sequence, and record hash.

No caller-visible acknowledgement is returned before synchronization succeeds.
Sequence overflow, file-size overflow, unsupported schema, or lock contention
fails closed.

A sync failure after bytes become visible returns a typed
`AppendVisibleDurabilityUncertain` receipt, not a definite failure. The task is
blocked from further audited transitions until same-process or startup
reconciliation determines whether the exact record is present and durable.
Retry uses the same identity and accepts only byte-identical visible content.

### 9.3 Audited state transition

For a task create or mutation:

1. derive and validate the exact logical event from authoritative old/new state;
2. append and sync `Prepared` with event, expected revision (or task absence for
   create), replacement revision, and a privacy-safe transition receipt digest;
3. execute the existing task create/CAS;
4. if no task state became visible, append and sync `Aborted` with a bounded
   reason category;
5. if replacement state became visible, append and sync `Committed`; and
6. acknowledge the product transition only after the committed record is
   durable.

Prepared and aborted records are protocol evidence, not logical material events
returned by committed-event readers.

If task persistence reports `CommitVisibleDurabilityUncertain`, the audit layer
re-reads the task and journal under the lock. It commits the audit transaction
only when the exact replacement revision and transition receipt are visible;
otherwise it returns a typed uncertain/corrupt result and blocks progression.

### 9.4 Startup reconciliation

`TaskStore::open` or an immediately adjacent startup step must reconcile all
audit journals before Product Task restore or new audited writes:

1. deterministically enumerate validated task and audit files;
2. validate every complete record's schema, sequence, hash, identity, and
   transaction linkage;
3. if the final bytes are not newline-terminated, treat only that final fragment
   as unacknowledged and truncate it back to the last verified offset;
4. reject any malformed complete line, interior sequence gap, hash mismatch,
   duplicate identity with different bytes, commit without prepare, or
   contradictory outcome as corruption;
5. for each unresolved prepare, load the authoritative task snapshot under the
   store lock;
6. append `Committed` only when the exact replacement revision and transition
   receipt are visible;
7. append `Aborted` only when task absence/expected revision proves the mutation
   did not commit; and
8. reject any state that matches neither side rather than guessing.

No task may advance while its journal has an unresolved or uncertain
transaction. Reconciliation never mutates product state from audit data.
Existing Product Task reconciliation remains responsible for changing
`Running`/`Applying` to `Interrupted`; that resulting task CAS is itself an
audited material transition.

### 9.5 Ordering and concurrency

Ordering is per Product Task only. The existing exclusive TaskStore lock
serializes journal append and task CAS. The event timestamp is evidence supplied
by the product boundary; sequence defines physical order and must not be derived
from wall-clock order.

Idempotent retry accepts an existing byte-identical event/transaction outcome
and returns its acknowledgement. Conflicting reuse is corruption. V1 does not
promise a total order across Product Tasks.

### 9.6 Retention

Audit retention follows the existing Product Task retention boundary. While a
task snapshot exists, its journal is retained. When startup reconciliation
prunes a terminal task older than 30 days, it deletes the matching task journal
in the same locked cleanup pass. It never deletes an audit journal for an active,
ready-for-review, applying, uncertain, corrupt, or unreconciled task.

Retention removes the entire task and its evidence rather than rewriting a
journal prefix, so the append-only and hash-chain invariants remain simple. No
independent audit retention configuration is added in V1.

## 10. Integration behavior

### 10.1 Product Task and artifact transitions

Every production Smart Redaction task create/CAS callsite that performs a
material transition migrates to the audited boundary. The lower-level raw
create/CAS implementation may remain private for protocol composition and
focused store tests, but product callsites cannot bypass audit derivation.

Artifact promotion evidence is appended only after the validated artifact,
proposal, source binding, task/attempt/run identities, and V2 run contract all
match. Partial provider output, failed validation/dry-run, or incomplete tool
arguments never produce `ArtifactPromoted`.

### 10.2 Skill-use evidence

`RunContractBound` records the existing immutable `SkillUseReceiptV1` fields and
authority receipt digests already copied into Product Task V2. It does not reload
the catalog, persist the instruction body, or derive authority from skill
content. Stale digest or catalog selection failure prevents the run contract and
its audit event from being committed.

### 10.3 Authority denial

The authority check remains immediately before tool execution and before the
tool body. On `ToolError::AuthorityDenied`, the runner constructs a bounded
`AuthorityDenied` event from the current task/attempt/run binding, authority
snapshot receipt, registered tool name, and required `RunOperation`.

The runner awaits the injected durable audit sink before returning the run
terminal. If denial evidence cannot be acknowledged, the run terminates as
`RunTerminalState::AuditFailure { category }`; it never retries or executes the
denied tool. The original denial is retained only in process-local error context
and privacy-safe structured tracing, not promoted as a successful terminal.

### 10.4 Review and product commit

`ReviewApplyStarted` is committed with the existing `ReadyForReview → Applying`
snapshot before document mutation. `ReviewDecisionCommitted::Applied` binds the
exact artifact revision, accepted/rejected candidate IDs, bounded local-delta
receipt, and resulting document state receipt already accepted by
`complete_apply`. Candidate geometry, image pixels, proposal bytes, and complete
local-delta contents are excluded; only bounded counts/IDs and digests required
for provenance are retained.

`ReviewDecisionCommitted::Rejected` binds the same exact artifact revision and
review timestamp without a product mutation receipt.

For this slice, “publication” means committing the reviewed agent artifact into
the authoritative Rollshot image document. Saving/exporting a file is a separate
product action with no Product Task contract and remains outside the event
vocabulary.

### 10.5 Transient display loss and repair

`RunEvent` remains lossy. The UI must not open, query, or replay audit journals.
Tests close or saturate the event channel so all text/tool/source display events
are absent, then prove that:

- correlated terminal state repairs the active run display;
- a durable `ReadyForReview` snapshot restores the review artifact;
- completed/rejected/stale/interrupted task state is reconstructed from
  `TaskStore`; and
- audit corruption blocks task admission but is not interpreted as product
  state.

No new visual layout, copy, or interaction is introduced.

## 11. Error and failure semantics

The public audit contract uses bounded categories rather than native I/O strings:

- `Unavailable`;
- `LockContended`;
- `AppendPreCommitFailure`;
- `AppendVisibleDurabilityUncertain`;
- `UnsupportedSchema`;
- `CorruptJournal`;
- `SequenceOverflow`;
- `JournalTooLarge`;
- `CorrelationMismatch`;
- `TransitionMismatch`; and
- `ReconciliationRequired`.

Concrete `io::Error` remains a private source in `rollshot-app`. `Debug`,
`Display`, tracing, and serialized failures include only category, task/event ID,
sequence/revision where safe, and bounded static context. They never include
journal contents, full paths, provider errors, or sensitive payloads.

Failure rules:

- Failure before a prepared append acknowledgement leaves product state
  unchanged.
- Failure after prepare but before task commit is resolved to `Aborted`.
- Failure after task commit but before audit commit is uncertain and blocks
  further transitions until reconciliation; callers must not report the
  material transition as durably audited yet.
- A corrupt interior journal fails closed and is never silently truncated,
  skipped, or replaced.
- Audit failure cannot convert a partial provider/tool result into an artifact.
- Audit failure does not retry side effects or reconstruct consent, authority,
  or approval.
- Startup cannot repair product state by replaying audit records.

`RunTerminalState` gains a bounded audit-failure category. `TaskTerminal` does
not gain a terminal that would itself require an unavailable journal. When the
journal is unavailable, the authoritative snapshot remains at its last durable
state; after the journal is healthy, startup TaskStore reconciliation changes
any remaining `Running`/`Applying` snapshot to the existing `Interrupted`
terminal through the audited transition protocol.

## 12. Privacy and diagnostics

Allowed durable fields are limited to:

- schema versions;
- opaque task, attempt, run, event, transaction, artifact, proposal, skill, and
  resource identities;
- artifact, source-binding, authority-snapshot, skill-package, event, transition,
  and journal-record digests;
- bounded task/artifact/event/review/terminal enums;
- policy and catalog revisions;
- registered tool name and required operation for a denial;
- bounded candidate IDs/counts and resulting document state receipt; and
- timestamps and per-task sequence numbers.

The journal must not contain:

- screenshot or image pixels;
- annotation or proposal payload bytes;
- full source, prompt, response, summary, transcript, or tool argument/result;
- raw Action Guide semantic input;
- provider credentials, request IDs, messages, or native errors;
- complete skill bodies or arbitrary resource contents;
- authority grant sets, OS permission details beyond a bounded denial category;
- unrestricted filesystem paths; or
- transient `RunEvent` contents.

Tests serialize every event variant with sentinel secrets placed in all adjacent
runtime/product objects and assert absence from JSON, `Debug`, `Display`, and
captured `tracing` fields. Product diagnostics use stable explicit
`rollshot::agent::audit` and `rollshot::app::agent_audit_store` targets with
structured fields. Per-record high-volume diagnostics use `trace`.

## 13. Migration and compatibility

1. Add the new audit domain module and tests without changing production
   persistence.
2. Add the app journal store, crash failpoints, validation, and startup
   reconciliation behind focused tests.
3. Add audited task create/transition APIs and migrate every Smart Redaction
   material-transition callsite. Keep raw store mutation private to the composed
   protocol and tests.
4. Connect `RunContractBound`, artifact, review, terminal, and startup
   reconciliation transitions.
5. Inject the durable sink into the active Smart Redaction runner and connect
   authority denial.
6. Remove the dormant `runtime::AuditEvent` enum and its tests once the new
   vocabulary is active; do not leave an alias or re-export.
7. Prove transient-event loss repair and run full affected-crate verification.

Existing task snapshot schemas remain readable. Audit journals are new; an
existing task without a journal receives a bootstrap `TaskObservedAtMigration`
protocol record only if it is still active or reviewable at first audited open.
That private bootstrap record records current revision and metadata digest but
is not presented as evidence for historical transitions that occurred before
Slice 6. Terminal tasks already past the retention boundary are pruned rather
than backfilled.

After bootstrap, every future material transition is audited. The system never
fabricates historical event times or claims that pre-Slice-6 transitions were
observed when they were not.

Rollback before any new audited mutation removes the empty audit directory and
restores direct TaskStore calls. After journals exist, rollback may ignore the
append-only sidecars but must not delete or rewrite them automatically. Task
snapshots remain the compatible product source of truth.

## 14. Test strategy

### 14.1 Domain contract tests

- canonical bytes and golden digest for every `AuditEventV1` variant;
- schema, ID, string, collection, and correlation bounds;
- required/forbidden correlation fields per variant;
- exact Product Task old/new transition derivation;
- rejection of wrong task, revision, attempt, run, artifact, proposal,
  run-contract, skill, authority, review, or document receipt;
- privacy-safe `Serialize`, `Debug`, and error output; and
- removal of the dormant runtime vocabulary with no production callsite left.

### 14.2 Journal contract tests

Deterministic failpoints cover:

- create/open, write, file sync, and parent-directory sync;
- crash after partial final line;
- crash after prepared sync but before task create/CAS;
- crash after task commit but before audit commit;
- crash after audit commit bytes become visible but before acknowledged sync;
- idempotent retry with the same IDs and bytes;
- conflicting identity reuse;
- interior malformed JSON, sequence gap, hash mismatch, duplicate sequence,
  commit-without-prepare, contradictory commit/abort, symlink, special file,
  oversize file, and sequence overflow;
- concurrent same-task transitions under the existing lock; and
- 30-day whole-task/journal retention with active/uncertain/corrupt exclusions.

For each acknowledged append, reopen a fresh store and prove the exact committed
event remains queryable and the chain validates. For every injected pre-ack
failure, prove the outcome is either absent, explicitly aborted, or
reconciliation-required—never silently claimed committed.

### 14.3 Product integration tests

The Smart Redaction fixture proves the ordered logical evidence:

```text
TaskCreated
→ AttemptStarted
→ RunContractBound
→ ArtifactPromoted
→ ReviewApplyStarted
→ ReviewDecisionCommitted::Applied
```

Separate fixtures prove rejection, each task terminal category, stale artifact
rejection, startup interruption, authority denial before tool execution, and
audit failure preventing success/promotion. The run-contract event must contain
the exact authority and bundled Smart Redaction skill receipts copied into the
artifact.

### 14.4 Transient-loss repair tests

Drop all `RunEvent` deliveries and verify authoritative terminal, review,
completed, rejected, stale, and interrupted states restore correctly. Corrupt
audit evidence and verify admission fails while the product snapshot remains
unchanged and is never reconstructed from the journal.

### 14.5 Verification commands

The implementation plan must include at least:

```bash
rtk cargo test -p rollshot-agent audit
rtk cargo test -p rollshot-agent product_task
rtk cargo test -p rollshot-app audit_store
rtk cargo test -p rollshot-app result_workspace
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-app
rtk cargo fmt --check
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings
```

If implementation changes Action Guide feature-gated paths through shared result
workspace code, also run:

```bash
rtk cargo test -p rollshot-app --features action-guide
```

No stitching benchmark is required. No iced visual baseline workflow is required
unless implementation introduces an unplanned user-visible UI change; such a
change requires stopping and revising this spec first.

## 15. Acceptance gate G3 evidence for Slice 6

Slice 6 passes only when all of the following are demonstrated:

1. Every V1 material transition has durable, correlated, schema-versioned
   evidence in the active Smart Redaction path.
2. Every caller-visible append acknowledgement survives fresh reopen and exact
   chain validation.
3. Acknowledged interior records cannot be silently lost, reordered, duplicated,
   or changed; corruption fails closed.
4. Each prepare/CAS/commit crash point resolves deterministically without using
   audit replay as product truth.
5. Authority denial is durably recorded before terminal return and never enters
   the denied tool body.
6. Artifact promotion cannot occur after audit failure, partial provider output,
   failed validation/dry-run, stale binding, or mismatched run contract.
7. Exact Product Task, attempt, run, authority, skill-use, artifact, proposal,
   review, and product-commit references are validated as applicable.
8. Dropped transient display events repair from authoritative terminal/task/
   document state without reading audit history.
9. Privacy sentinel tests prove no pixels, raw semantic input, credentials,
   prose/transcript, provider internals, proposal payload, full skill body,
   authority grants, or tool arguments/results enter durable records or
   diagnostics.
10. Retention removes only complete expired terminal task/journal pairs and does
    not rewrite live journals.
11. The dormant runtime audit vocabulary is removed; there is one active audit
    convention.
12. No event-sourcing, reconnectable replay, audit UI, global scheduler, remote
    sink, or external publication model is introduced.
13. A fresh independent code review validates crash consistency, failure
    classification, privacy, callsite completeness, and non-event-sourcing
    boundaries.
14. All affected tests, formatting, and required lint checks pass.

Gate G3 as an umbrella completion decision additionally requires closure of all
other slice gates. In particular, Slice 5's pending user approval and missing
formal independent review must be resolved separately before claiming the
umbrella complete.

## 16. Residual risks and deferred scope

Expected residual risks to record at implementation gate review:

- Filesystem durability semantics vary by platform. Failpoints prove Rollshot's
  classification and reconciliation logic, but destructive power-loss testing
  is outside deterministic CI.
- Hash chaining detects accidental interior loss/corruption; it is not a defense
  against an attacker who can rewrite the whole local journal.
- The current review receipt may omit a resulting document digest in some paths.
  The audit event records the exact available receipt and must not invent one;
  strengthening that product receipt requires a separate Product Task contract
  amendment if implementation evidence shows it is necessary.
- Existing pre-Slice-6 tasks can be bootstrapped only from current authoritative
  state. Their historical transitions are explicitly unknown, not backfilled.
- External Save/Export and Action Guide publishing lack an agent Product Task
  binding and remain outside V1 publication evidence.

Deferred restart conditions:

- A global segmented/database store requires multiple task families or shared
  audit queries.
- Remote export or attestation requires an approved compliance or support
  workload.
- Reconnectable replay requires a product workflow that cannot repair from
  authoritative state.
- Cryptographic signatures require an approved trust/key-management model.
- Separate audit retention controls require a product or policy need distinct
  from Product Task retention.

## 17. Implementation constraints

The implementation plan must preserve these stop points:

- Stop if current TaskStore locking cannot serialize journal append plus task
  create/CAS without a second independent lock order.
- Stop if any material production TaskStore mutation callsite cannot be migrated
  to the audited boundary.
- Stop if authority denial cannot be persisted before terminal return without
  executing blocking filesystem work on the async runtime thread.
- Stop if startup reconciliation would need to mutate Product Task state from an
  audit event rather than from current product reducers.
- Stop and revise the spec if external Save/Export, Action Guide publication, a
  new UI, a global audit database, or event replay becomes required.
- Do not close Gate G3 until Slice 5's separate gate evidence is resolved and all
  six slice gates satisfy the umbrella policy.
