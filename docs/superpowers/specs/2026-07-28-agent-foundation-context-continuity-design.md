# Agent Foundation Slice 5: Context Continuity Design

**Date:** 2026-07-28
**Status:** Approved in brainstorming auto mode
**Area:** Agent foundation / artifact-first context continuity
**Governing umbrella:**
[`2026-07-26-agent-foundation-umbrella-design.md`](2026-07-26-agent-foundation-umbrella-design.md)
**Prior slice evidence:**
[`2026-07-27-live-job-registry-decision.md`](../spikes/2026-07-27-live-job-registry-decision.md)

## 1. Decision summary

Slice 5 makes durable product state, not model history, the source of continuity
across context boundaries. A fresh context is reconstructed from a validated
Product Task snapshot, typed artifact and review references, or a validated
Action Guide project revision. Model-authored prose, provider conversation
state, and transient display events are never authoritative recovery input.

The product proof is the existing Action Guide caption-suggestion workload. A
project-backed request receives a bounded `ActionGuideContextProjectionV1`
created from a validated durable project revision. The request remains a fresh,
empty-history provider call. Its `CaptionProposal` is bound to the exact project
revision and canonical projection digest, so a save, replacement, or stale
projection is rejected deterministically before apply. Unsaved and dirty guides
retain their existing in-memory proposal behavior but do not claim restart
continuity.

A separate emergency safety valve applies only inside one bounded Smart
Redaction run. On the first explicitly classified provider context overflow,
Rollshot discards the entire private Rig conversation and builds a deterministic
`RunContinuityManifestV1` from host-owned task, artifact, skill, evidence,
authority-reference, and budget state. It starts a fresh Rig state machine and
retries the interrupted model step once. No model summarizer runs, no transcript
or compacted prose is persisted, and a second overflow terminates with a typed
failure.

The manifest references authority but never carries or reconstructs it. The
current immutable `AuthoritySnapshot`, current skill digest, Product Task
snapshot revision, source generation, and artifact revision are revalidated
before the retry and before every later tool call. Review approval and consent
remain product-owned records outside the manifest.

## 2. Start readiness and current-code drift

Slice 5 depends on Slice 2, and its default start point follows Gate G2. Both are
merged on `main`:

- Slice 2: `1745133` / PR #103;
- Slice 3: `3d69781` / PR #104;
- Slice 4: `eb4b64e` / PR #105.

Slice 4 is not a technical dependency of Slice 5, but Phase 3 parallel
implementation was not authorized. Planning may proceed because Slice 4 is
merged, its decision record contains passing focused/full suites and an
independent review with no correctness or security defect, and the user has
explicitly requested Slice 5.

Current focused verification on 2026-07-28 observed:

- `rtk cargo test -p rollshot-agent jobs`: 24 passed;
- `rtk cargo test -p rollshot-action video_import`: an initial run reported one
  `decoder_unavailable` result in the stalled-decoder cancellation fixture; the
  exact test then passed twice and the complete 57-test filtered suite passed on
  immediate rerun;
- `rtk cargo test -p rollshot-app --features action-guide action_guide_home`:
  70 passed.

The one-off spawn classification is recorded as non-reproduced test-environment
evidence, not silently reported as a clean first run. It does not alter Slice 5
architecture. A later reproducible failure belongs to Slice 4 maintenance and
must not be hidden in Slice 5.

The historical Slice 4 decision record also labels its closing decision “Gate
G4” and describes different Slice 5/6 topics than the governing umbrella. Code,
merged PR scope, and the umbrella establish that the implementation is Slice 4
Live Job Registry and that this child is Slice 5 Context Continuity. The
historical decision record is not rewritten.

Material drift from the 2026-07-22 context-compaction research baseline:

- durable `ProductTaskSnapshot` V2 state now binds task, attempt, run, source,
  authority receipt, skill receipt, artifact metadata/payload, review receipt,
  and snapshot revision;
- `TaskStore` atomically persists and CAS-updates Product Task snapshots and
  restores a current `ReadyForReview` handoff without transcript state;
- `AuthoritySnapshot` and static `SkillUse` are immutable and digest-bound;
- Action Guide project manifests are durable, revisioned, validated, and loaded
  through `rollshot-action::project`;
- Action Guide caption and visual-annotation requests already start with empty
  provider history, but their proposal types do not carry a durable project
  revision;
- caption proposals perform step-level stale checks, and visual proposals
  perform step/keyframe/document-state stale checks;
- Smart Redaction still uses one memory-only Rig `AgentRun`; provider failures
  have no provider-neutral context-overflow category and no compact/retry path;
- `AgentSession` stores only completed display exchanges in memory and is not a
  durable continuation source; and
- no semantic memory, transcript persistence, provider-native compaction, or
  typed continuity manifest exists.

## 3. Problem

Rollshot now has durable task and artifact truth, but the model boundary still
has two unsafe failure modes:

1. callers can accidentally treat live `AgentSession` or model prose as the
   context needed to continue after a durable product boundary; and
2. a long bounded run has no controlled response to a genuine provider context
   overflow other than a generic provider failure.

Action Guide exposes the first gap clearly. Its project store has a durable
revision, while its agent proposals are created from an in-memory `Guide` clone
and identify only a process-local numeric run. After reopen, a fresh provider
call can be made, but there is no typed proof that its input came from revision
R or that its result remains applicable to R.

Smart Redaction exposes the second gap. Rig correctly carries paired tool calls
and results inside one run, while important draft, evidence, task, skill,
authority, budget, and artifact state is already host-owned. Repeatedly
summarizing transcript prose would duplicate sensitive data and weaken those
typed boundaries. The emergency path should instead restart from host truth.

## 4. Goals

1. Make artifact and project re-projection the only authoritative cross-context
   recovery strategy.
2. Define a provider-neutral, privacy-safe Product Task continuity projection
   that validates exact task snapshot, source, attempt, run, artifact, skill,
   and review references.
3. Prove an Action Guide caption request can start from a validated durable
   project revision with no previous model history.
4. Bind project-backed caption proposals to the exact revision and canonical
   projection digest and reject stale results before apply.
5. Add one typed, deterministic, run-local emergency manifest for Smart
   Redaction context overflow.
6. Permit at most one overflow-triggered fresh Rig restart and preserve all
   existing budgets, cancellation, authority, validation, dry-run, terminal,
   and review rules.
7. Fail closed at task, artifact, skill, authority, tool-evidence, terminal, and
   review boundaries.
8. Keep pixels, full skill bodies, raw transcript, user prose, provider-native
   state, credentials, and full artifact payloads out of continuity records,
   `Debug`, tracing, and persistence.

## 5. Non-goals

This slice does not add:

- transcript persistence, conversation resume, semantic memory, retrieval, or a
  project/user memory service;
- a model-authored summary, summary chain, handoff document, retained transcript
  tail, or recent-file attachment;
- provider-native compaction, provider context-management edits, provider
  capability negotiation, opaque continuation tokens, or cache-specific policy;
- selective tool-result pruning, shake, snip, artifact spill, or a general
  context-window optimizer;
- retries for ordinary provider, tool, validation, evidence, terminal, or review
  failures;
- more than one retry for an explicitly classified context overflow;
- durable in-flight run recovery, instruction-pointer resume, reconnectable
  events, remote job recovery, workflow DAGs, or child agents;
- a new Product Task kind for Action Guide captions or a Product Task fabricated
  around direct Action Guide use;
- persistence of caption proposals as generic Slice 2 artifacts;
- continuity claims for unsaved or dirty Action Guide work;
- a new user-facing workflow, layout, copy requirement, or visual baseline;
- migration of the visual-annotation agent in the proof increment; or
- launch-video behavior or any deferred umbrella capability.

## 6. Considered approaches

### 6.1 Selected: durable re-projection plus one deterministic emergency restart

Normal boundaries end the old model context. The host reloads authoritative
state, validates typed references, and builds a fresh bounded request. Within a
Smart Redaction run, the first classified overflow replaces the entire private
Rig state with a deterministic manifest-backed request generated from the same
host state.

This approach matches the current Product Task and Action Guide stores, avoids
creating another sensitive durable derivative, and makes loss visible through
schema and stale checks. Replacing the entire Rig history also preserves
provider protocol integrity: no tool result is retained without its call.

### 6.2 Rejected: model-authored full transcript compaction

A summary plus recent tail could retain conversational nuance, but Rollshot's
active workloads already store stronger state in task, artifact, draft,
evidence, review, and project contracts. A summary adds latency, privacy and
deletion obligations, cache churn, omission risk, and another model failure
inside recovery. It cannot recreate authority or approval.

### 6.3 Deferred: cache-aware selective tool-result reduction

Deterministic pruning can outperform full summarization for tool-heavy runs, but
it requires durable recovery artifacts, cut-point and call/result invariants,
provider cache measurements, and retention authorization. The current Product
Task payloads are not a general spill store. This mechanism can restart only if
measured context pressure proves the deterministic full restart loses necessary
quality or exceeds cost targets.

## 7. Architecture and ownership

```text
Normal durable boundary

TaskStore / Action Guide project store
        │ validated load at exact revision
        ▼
typed continuity projection
        │ canonical validation + digest
        ▼
fresh bounded model request (empty history)
        │
        ▼
typed proposal bound to source revision
        │
        └── current authoritative revision check before review/apply

Emergency in-run boundary

provider reports typed ContextOverflow
        │ first occurrence only
        ▼
re-read host-owned run/task/artifact/evidence state
        │ validate authority + skill + generation + revisions
        ▼
RunContinuityManifestV1 + deterministic projection text
        │ discard entire private Rig history
        ▼
fresh Rig AgentRun, same Product Task attempt/run and remaining budget
        │ retry interrupted model step once
        └── second overflow or projection failure → typed terminal
```

Ownership remains explicit:

- Product Task and Action Guide stores own durable state and revision truth.
- Product code owns loading, user consent, review decisions, and apply checks.
- `ContinuityProjectionV1` and `ActionGuideContextProjectionV1` are immutable,
  validated read models. They do not mutate stores or grant authority.
- `RunContinuityManifestV1` is a run-local emergency projection. It is not a
  durable task snapshot, transcript, approval record, or artifact payload.
- `AgentRunner` owns the one-overflow-retry guard and private Rig replacement.
- `ToolContext` remains authoritative for current draft generation,
  generation-bound validation/dry-run evidence, and pending review handoff.
- `BudgetTracker` remains authoritative for used and remaining budget; retry
  does not reset any dimension.
- `AuthoritySnapshot` and `ToolRegistry` continue to own authorization. A digest
  in a projection is only a stale-check reference.
- `SkillUse` remains immutable run input. A package/digest reference cannot load
  a different skill or grant a tool operation.
- Providers classify overflow privately and expose only a provider-neutral
  category.

## 8. Durable continuity contracts

### 8.1 `ContinuityProjectionV1`

`rollshot-agent` defines an immutable V1 projection from a validated
`ProductTaskSnapshot`. Construction validates and retains only bounded typed
references:

- `schema_version = 1`;
- `ProductTaskId` and exact `snapshot_revision`;
- `TaskKind`, `TaskStatus`, and canonical source-binding digest;
- current `TaskAttemptId` and `RunId`, when an attempt exists;
- active `RunContractReceiptV1` reference, including authority and skill receipt
  digests, when the task is running;
- current `ArtifactId`, `ArtifactRevision`, `ArtifactKind`, schema version, and
  canonical payload digest, when an artifact exists;
- review state as a closed enum: `None`, `PendingExactRevision`,
  `AcceptedExactRevision`, `RejectedExactRevision`, or `Stale`;
- a digest of the exact review receipt when one exists; and
- a canonical projection digest over all fields.

The projection excludes proposal bytes, screenshot pixels, source text, user
messages, complete skill content, authority grants, credentials, paths, and
provider/model conversation state.

Construction fails for internally inconsistent combinations: an artifact whose
task/attempt/run differs from the snapshot, a review receipt for another
artifact revision, an active contract that does not match the current attempt,
an unsupported schema, an absent required artifact for review state, or a
canonicalization overflow.

A caller that needs payload content must load it from the authoritative task or
artifact store and revalidate its digest separately. The projection never acts
as a payload cache.

### 8.2 Recovery boundary

A durable recovery request carries both:

1. an expected reference supplied by the caller (`task_id`, expected snapshot
   revision or exact artifact revision); and
2. a newly constructed projection from the current store load.

The boundary returns typed `Missing`, `UnsupportedSchema`, `Corrupt`,
`RevisionChanged`, `ArtifactChanged`, `ReviewChanged`, or `ProjectionInvalid`
errors. It never substitutes the newest task/artifact merely because the
expected one is missing or stale.

For an ordinary fresh continuation, a changed snapshot is not automatically an
error when the product explicitly requests “latest.” The resulting request is
bound to the newly selected revision. Exact-review and apply paths must always
request an exact revision.

### 8.3 Existing Smart Redaction restore

The current result-workspace restore path already loads a durable
`ProductTaskSnapshot` and rejects unrelated/stale source bindings. Slice 5 makes
that behavior explicit by constructing `ContinuityProjectionV1` before
presenting or applying restored review state. The UI still renders from the
snapshot payload, not from projection prose. No transcript or previous
`AgentSession` exchange is loaded.

## 9. Action Guide proof contract

### 9.1 `ActionGuideContextProjectionV1`

`rollshot-action::project` defines a projection that can be constructed only
from a successfully validated `LoadedProject` and its `ProjectManifestV2`.
It contains:

- `schema_version = 1`;
- exact durable project `revision`;
- a canonical digest of the validated manifest fields used by the caption run;
- bounded guide title and ordered step entries;
- for each entry: stable step source, index, keyframe ID, title, caption, action
  kind, reason, and timestamp; and
- a canonical projection digest.

It contains no root path, image pixels, annotations, raw semantic input,
provider/model name, credentials, or prior conversation. Construction enforces
existing project validation plus the existing maximum generated-step bound and
explicit aggregate text/serialized-byte bounds. Oversize projects receive a
typed projection-limit failure before a provider request.

### 9.2 Product-path selection

When caption suggestions start:

- a saved, clean, validated project uses `DurableProject { revision,
  projection_digest }` and the fresh request is built from the projection;
- an unsaved or dirty workspace uses `EphemeralGuide { guide_digest }`, matching
  current behavior but carrying no restart-continuity claim;
- no implicit save is triggered, no dirty state is discarded, and no request is
  relabeled durable merely because it has a project path.

For the durable branch, product code reloads the project through the existing
validated store boundary before dispatch. It compares the loaded revision with
the captured `ProjectSession` revision and rechecks that the workspace is still
clean before launching the provider request. A load, revision, or cleanliness
race aborts that operation with a typed stale result; it never falls back to an
ephemeral request after the user selected the durable branch.

The same existing button, provider configuration, timeout, review UI, and apply
copy remain. This is a contract change, not a new visible workflow.

### 9.3 Proposal binding and apply

`CaptionProposal` gains a closed origin:

- `DurableProject { revision, projection_digest }`; or
- `EphemeralGuide { guide_digest }`.

Each suggestion continues to carry its exact step base. `CaptionProposal::apply`
and `apply_all` require a `CaptionApplyContext` supplied by product code; there
is no unchecked apply entry point. For a durable proposal the context proves:

1. the current workspace is still the same saved project session;
2. its `base_revision` equals the proposal revision;
3. no dirty mutation has occurred since the projection was created; and
4. the suggestion's existing source/title/caption/keyframe base still matches.

Any failure marks the affected proposal/suggestion stale and performs no
mutation. Saving an otherwise identical project to revision R+1 invalidates an
R-bound proposal deliberately; revision identity, not semantic coincidence, is
the review boundary.

Ephemeral proposals keep the existing per-step base check. They are not
restored after close and do not participate in the Slice 5 restart proof.

### 9.4 Fresh-context proof

The gate fixture must:

1. create and atomically save an Action Guide project at revision R;
2. destroy all in-memory workspace, provider request, and proposal state;
3. load and validate the project from disk;
4. construct `ActionGuideContextProjectionV1`;
5. run the existing caption provider adapter with empty history;
6. produce a proposal bound to R and the projection digest;
7. accept it while the current project remains R; and
8. save or replace the project as R+1 and prove the same proposal is rejected.

No previous transcript, process-local run ID, or model prose may be consulted in
steps 3–8.

## 10. Emergency continuity manifest

### 10.1 Scope

`RunContinuityManifestV1` exists only for a current Smart Redaction Product Task
attempt. It is built on demand after the first typed context overflow. It is not
written to `TaskStore`, appended to `AgentSession`, or retained after terminal.

The manifest contains:

- `schema_version = 1`;
- exact task ID, task snapshot revision, attempt ID, and run ID;
- source-binding digest and current draft generation;
- current run stage as a closed enum: `Drafting`, `NeedsValidation`,
  `NeedsDryRun`, `ReadyToSubmit`, or `AwaitingUserInput`;
- generation-bound validation and dry-run receipt digests, when present;
- current artifact reference and review state from `ContinuityProjectionV1`,
  when present;
- skill package ID, skill content digest, invocation kind, and invocation
  provenance reference;
- authority snapshot digest and policy revision as references only;
- budget limits and already-used counts for every existing dimension;
- the exact set of currently executable tool names derived by the host; and
- canonical manifest digest.

The manifest does not contain grants, consent text, disclosure payload, source
program text, proposal body, screenshot pixels, tool-result bodies, assistant
prose, user prose, full skill body, provider state, paths, or credentials.

### 10.2 Deterministic projection text

The restart request consists of:

1. the normal host-owned Smart Redaction system prompt and exact selected skill;
2. a fixed code-owned instruction explaining that prior conversation was
   discarded after context overflow;
3. a deterministic serialization of `RunContinuityManifestV1`;
4. a bounded host-generated summary of current draft/evidence state using
   existing typed `ToolContext` accessors; and
5. the exact next allowed action derived from the run-stage enum.

No model call creates or validates this projection. The serialization order is
canonical and capped. Oversize or unavailable required state is a typed recovery
failure, not a reason to drop fields.

### 10.3 Validation before restart

Immediately before replacing Rig state, the driver verifies:

- task, attempt, run, and task snapshot revision still match;
- current source binding and draft generation still match;
- validation/dry-run evidence refers to the current generation;
- current `AuthoritySnapshot` digest and policy revision match the manifest
  reference;
- current `SkillUse` package and digest match;
- no terminal or pending review result appeared during projection;
- cancellation has not won; and
- the remaining budget can afford one model call without resetting usage.

The same checks are performed at the normal tool boundary through existing
`ToolRegistry` authority and `ToolContext` generation checks. The manifest does
not replace either boundary.

### 10.4 Whole-history replacement

The driver discards the complete private Rig history and creates a new
`AgentRun` for the same Product Task attempt and `RunId`. It does not cut at an
arbitrary message, retain a tail, or copy individual tool results. Therefore the
new provider request contains no unmatched tool call/result pair.

Previously emitted display text remains transient and may be marked superseded
by a `RunEvent::ContextReprojected { attempt: 1 }` hint. Product and terminal
state are repaired from authoritative state. The event contains no manifest or
prose.

`AgentSession` records only the final completed exchange. It does not persist the
pre-overflow transcript or make it visible as recovered truth.

## 11. Provider-neutral overflow and retry policy

### 11.1 Classification

`ModelError` adds a closed `ContextOverflow` category. Anthropic and OpenAI
adapters map only their documented status/code combinations at the private HTTP
boundary. Unknown 4xx/5xx responses, strings from a streamed error without a
recognized code, media-size errors, output-token exhaustion, transport errors,
timeouts, and protocol failures remain their existing categories.

Provider status codes, SDK types, raw bodies, request IDs, and native compact
objects do not enter public Rollshot types. Diagnostics record only the stable
`context_overflow` category and bounded numeric usage already permitted.

### 11.2 Retry state machine

```text
normal CallModel
    ├── success ───────────────────────────────► continue
    ├── ordinary failure ──────────────────────► existing typed terminal
    └── ContextOverflow, retry_used = false
            ├── projection/validation fails ───► ContextRecoveryFailure
            ├── cancellation wins ─────────────► Cancelled
            └── replace whole Rig state; retry_used = true
                    ├── success ────────────────► continue
                    ├── ContextOverflow ────────► ContextOverflow
                    └── other failure ──────────► existing typed terminal
```

Only the interrupted model step is retried. Completed side-effecting tools are
not re-executed automatically; their typed results are represented by current
host state. The retry reuses the same `BudgetTracker`, wall-time deadline,
cancellation token, Product Task attempt, run ID, `ToolContext`, authority
snapshot, and skill use. The model-call budget is charged once at each provider
dispatch, including a failed overflow attempt; reported token and cost usage is
charged when available. Missing provider usage never resets or invents token or
cost values. The retry is unavailable when the remaining model-call budget
cannot fund the additional dispatch.

A configured `max_turns` limit applies across both Rig instances. Replacement
does not grant a fresh turn count.

### 11.3 Failure precedence

- cancellation observed before restart wins and returns `Cancelled`;
- a stale task/source/artifact/skill/authority reference returns
  `ContextRecoveryFailure(StaleReference)`;
- a tool/evidence conflict returns its existing typed failure and is never
  converted into overflow;
- a terminal or review handoff that committed concurrently wins; no retry can
  reopen it;
- ordinary provider failure after restart remains ordinary provider failure;
- a second overflow always ends the run; and
- partial assistant text or tool arguments from a failed stream are discarded
  and cannot become proposal, task, session, or artifact state.

## 12. Boundary failure matrix

| Injection point | Required result |
|---|---|
| Product Task missing/corrupt/unsupported | Typed recovery failure; no request |
| Task snapshot changes after projection | Stale reference; no request or apply |
| Artifact revision/payload digest changes | Stale artifact; no prose substitution |
| Skill package or digest changes | Stale skill; no fallback package |
| Authority digest/policy changes | Stale authority; new product authorization required |
| Tool result missing after pre-overflow execution | Manifest build fails; tool is not replayed |
| Validation or dry-run evidence targets old generation | Existing stale-evidence failure |
| Provider overflows before stream establishment | One deterministic restart |
| Provider overflows after partial text/arguments | Partial data discarded; same one-retry guard |
| Projection exceeds its bound | Typed projection-too-large failure |
| Cancellation races manifest construction | Cancelled; no retry |
| Terminal/review commits during construction | Terminal/review wins; no retry |
| Second context overflow | Typed `ContextOverflow` terminal |
| Action Guide project moves from R to R+1 | R-bound proposal is stale and cannot apply |
| Action Guide project is missing/corrupt on reopen | Typed project load failure; no request |

## 13. Persistence, privacy, and diagnostics

Durable persistence remains limited to the existing Product Task and Action
Guide project stores. Slice 5 adds no transcript, summary, emergency manifest,
caption proposal, provider state, or continuity-ledger file.

Canonical projection digests may be persisted only where an existing artifact
or proposal contract already permits a digest. The emergency manifest itself is
memory-only. Action Guide project manifests remain the durable source; a
caption proposal stores only the revision and projection digest in memory.

`Debug`, serialization, tracing, events, and error displays must prove absence
of:

- image/video pixels and attachment bytes;
- raw Action Guide semantic input;
- project/task filesystem paths;
- user and assistant prose;
- source program and tool-result bodies;
- complete skill bodies;
- authority grants, consent text, and credentials;
- full artifact/proposal payloads; and
- provider-native errors, response bodies, IDs, or continuation state.

Product diagnostics use stable structured targets:

- `rollshot::agent::continuity` for projection/restart categories;
- existing `rollshot::agent::driver` for run terminals; and
- `rollshot::action::caption_agent` for revision-bound request lifecycle.

Events contain identifiers/digests only when those fields are already approved
as privacy-safe provenance. No runtime path or model text is logged.

## 14. Recovery comparison and measurements

The implementation records deterministic fixture evidence rather than a flaky
wall-clock gate:

| Recovery path | Canonical input | Required comparison |
|---|---|---|
| Product Task fresh projection | Re-loaded `ProductTaskSnapshot` | Projection bytes/digest equal same-revision in-process projection |
| Action Guide clean restart | Re-loaded `ProjectManifestV2` | Projection bytes/digest and ordered step refs equal pre-close revision R |
| Emergency run recovery | Current task + `ToolContext` + tracker + authority + skill | Every required typed field recovered exactly; no prose field accepted |

Each fixture records input serialized bytes, projection serialized bytes,
reference count, and provider-history message count. The clean restart gate
requires provider-history count zero. Emergency recovery requires exactly one
manifest-backed user projection and no retained tool call/result messages.

Wall-clock observations may be recorded as non-gating evidence, but no
machine-dependent latency threshold is introduced. The existing provider
stream deadline and overall run wall-time budget remain the operational bounds.

## 15. Testing strategy

### 15.1 Product Task projection tests

Tests must cover:

- canonical same-revision projection and digest stability;
- task/attempt/run/source/authority/skill/artifact/review reference coverage;
- unsupported schema and every inconsistent reference combination;
- exact revision versus explicit latest selection;
- stale task, artifact, skill, authority, and review rejection;
- restore of `ReadyForReview` from TaskStore without `AgentSession` history;
- no payload/prose/pixel/path/grant leakage through serialization or `Debug`;
  and
- clean-reload projection parity with an in-process snapshot.

### 15.2 Action Guide proof tests

Tests must cover:

- validated project R to deterministic bounded projection;
- aggregate step/text/serialized-size limits;
- close/reload followed by a provider request with empty history;
- proposal origin bound to R and projection digest;
- acceptance while the workspace remains clean at R;
- stale rejection after save to R+1, project replacement, dirty mutation, or
  changed step base;
- unchanged ephemeral proposal behavior for unsaved/dirty guides;
- no path, pixel, annotations, raw semantic input, provider configuration, or
  transcript in the projection; and
- no visible UI/layout change.

### 15.3 Emergency manifest and retry tests

Use deterministic fake providers and barriers, not sleeps. Cover:

- establishment overflow and mid-stream overflow after partial text/tool args;
- exactly one retry and second-overflow terminal;
- complete replacement with no unmatched call/result messages;
- task, artifact, skill, authority, source, generation, evidence, terminal, and
  review failure injection;
- cancellation before overflow, during manifest construction, and before retry;
- no reset of model-call, token, tool, validation, proposal, source-byte,
  argument-byte, cost, or wall-time budgets;
- no duplicate tool side effects or proposal submission;
- final `AgentSession` contains only the completed post-reprojection exchange;
- manifest and event privacy; and
- ordinary provider errors never trigger recovery.

### 15.4 Provider contract tests

Both adapters require fixtures for documented context-overflow status/code
classification, lookalike unrecognized errors, redacted diagnostics,
establishment failure, and streamed failure. Public tests assert only
`ModelError::ContextOverflow`, never provider-native fields.

### 15.5 Verification commands

At minimum:

```bash
rtk cargo test -p rollshot-agent continuity
rtk cargo test -p rollshot-agent provider_contract
rtk cargo test -p rollshot-action project::continuity
rtk cargo test -p rollshot-action caption_proposal
rtk cargo test -p rollshot-app --features action-guide caption_agent
rtk cargo test -p rollshot-app --features action-guide timeline_workspace
rtk cargo test -p rollshot-app result_workspace
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-app --features action-guide
rtk cargo fmt --check
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings
```

Independent code review must inspect authoritative-source ownership, overflow
classification, one-retry enforcement, protocol pairing, side-effect replay,
budget continuity, revision staleness, privacy, and the active caption path.

No golden visual baseline change is expected. If implementation changes visible
iced behavior, the repository iced UI testing workflow becomes mandatory before
that edit.

## 16. Acceptance criteria and Slice 5 gate

Slice 5 passes only when all are true:

1. A fresh context recovers every required Product Task/artifact/review
   reference from current durable product state without transcript or prose.
2. `ContinuityProjectionV1` is canonical, versioned, bounded, privacy-safe, and
   rejects every inconsistent or stale exact reference.
3. A saved Action Guide project can be closed, reloaded at revision R, projected,
   and sent through the existing caption provider path with empty history.
4. A project-backed caption proposal is bound to R and its projection digest;
   it applies at clean R and fails deterministically after R+1, replacement,
   dirty mutation, or step-base drift.
5. Unsaved/dirty caption behavior remains available but is explicitly ephemeral
   and gains no false restart guarantee.
6. Authority, consent, permission, and approval are never reconstructed from
   prose or projection fields; each side effect still uses the live immutable
   authority boundary.
7. The first recognized context overflow may cause one whole-history,
   manifest-backed restart; no other failure causes an automatic retry.
8. A second overflow terminates with typed `ContextOverflow`; manifest build or
   stale validation terminates with typed `ContextRecoveryFailure`.
9. Partial text, partial tool arguments, completed side effects, terminals, and
   review handoffs are never promoted or replayed incorrectly across restart.
10. All budget dimensions, wall-time, cancellation, skill selection, validation,
    dry-run, terminal-tool, and review semantics remain unchanged across retry.
11. Provider public contracts expose no provider-native compaction or error
    types, and both supported adapters pass overflow-classification tests.
12. Persistence, `Debug`, events, tracing, and diagnostics contain no pixels,
    raw semantic input, paths, credentials, full skill bodies, full payloads,
    transcript prose, or provider-native state.
13. Deterministic recovery measurements show same-revision projection parity,
    zero prior-history messages for clean Action Guide restart, and no retained
    call/result pairs for emergency restart.
14. A decision record captures verification, independent review, migration,
    the initial non-reproduced Slice 4 fixture failure, residual risks, and
    deferred scope.

Passing this gate proves bounded context continuity. It does not authorize
semantic memory, conversation resume, provider-native compaction, selective
pruning, launch-video work, or Slice 6 audit observability.

## 17. Stop and rollback conditions

Stop and revise this design rather than weakening it if:

- a required recovery fact exists only in model prose or transient events;
- the emergency manifest would need full proposal/source/tool-result content to
  continue safely;
- a provider adapter cannot classify overflow without exposing raw/native error
  state publicly;
- replacing all Rig history cannot avoid replaying a completed side effect;
- the retry cannot preserve the original budget tracker and turn limit;
- Action Guide revision binding requires an implicit save or blocks existing
  unsaved/dirty caption use;
- a durable project projection requires paths, pixels, annotations, or raw
  semantic input;
- product review or authority would need to be serialized into the manifest;
- two providers require different public continuity contracts; or
- the active product proof requires new user-facing UI solely to expose the
  foundation.

Rollback is a clean reversal to generic provider failure on overflow, existing
Smart Redaction memory-only Rig state, existing TaskStore restore, and
step-base-only Action Guide proposals. No new durable migration or transcript
file must be removed.

## 18. Residual risks and deferred work

- Provider context-overflow codes can evolve. Unknown responses fail as ordinary
  provider errors until a tested private mapping is added; broad string matching
  is forbidden.
- Whole-history restart deliberately loses uncaptured conversational nuance. If
  measured task quality requires it, selective deterministic projection may be
  reconsidered before any model summary.
- Product Task payloads are not a general result-spill store. Large tool results
  remain bounded at their current tool contracts.
- Action Guide project-backed proof covers captions, not visual annotation
  images. Visual proposals already have stronger document-state checks and can
  adopt the same revision origin in a later focused change if needed.
- Unsaved and dirty projects cannot survive process restart. Slice 5 does not
  introduce autosave or drafts.
- `AgentSession` remains process-local presentation state. It is intentionally
  not made durable.
- Provider cache hit/write economics are not optimized because no warm-prefix
  mutation or native cache edit is introduced. Actual usage remains observable
  through existing model usage accounting.
- macOS runtime execution may remain unavailable on the Linux workstation; the
  caption and continuity paths are shared Rust code with no platform-specific
  branch.

Restart conditions:

- transcript persistence or semantic memory: a measured workload proves durable
  artifact/project re-projection loses required user value;
- selective pruning: bounded traces show full deterministic restart cannot make
  adequate headroom or quality;
- provider-native compaction: measured context pressure makes it a finalist and
  a separate provider-neutral fallback is approved;
- durable run resume: a real workflow must survive process death beyond current
  task/artifact recovery; and
- Action Guide visual continuity: a measured visual-proposal restart case cannot
  be served by the existing project/document-state boundaries.

## 19. Implementation-plan boundary

The implementation plan must:

1. add failing Product Task and Action Guide re-projection tests before
   production contracts;
2. add provider-neutral overflow classification fixtures before adapter changes;
3. implement bounded canonical task and Action Guide projections;
4. migrate project-backed caption suggestions to revision-bound projection while
   preserving ephemeral unsaved/dirty behavior;
5. add proposal revision/digest stale checks before apply;
6. add the typed emergency manifest and exhaustive failure-injection tests
   before wiring retry;
7. implement one whole-history retry without resetting budget, turn count,
   authority, skill, cancellation, evidence, terminal, or review state;
8. preserve current TaskStore restore and Smart Redaction review handoff while
   making the projection boundary explicit;
9. verify privacy, recovery parity, protocol pairing, and both provider
   contracts;
10. run independent code review before writing the Slice 5 gate decision; and
11. stop at the gate decision without beginning Slice 6.
