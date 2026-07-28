# Action Guide Agent Foundation Migration Umbrella Design

**Date:** 2026-07-28
**Status:** Live governing umbrella until Gate B1 completes
**Area:** Agent foundation, Action Guide
**Predecessor program:**
[`docs/superpowers/specs/2026-07-26-agent-foundation-umbrella-design.md`](2026-07-26-agent-foundation-umbrella-design.md)
**Motivating deferred idea:**
[`docs/ideas/2026-07-22-agent-skills-action-guide-launch-video.md`](../../ideas/2026-07-22-agent-skills-action-guide-launch-video.md)

## 1. Purpose

The agent foundation program delivered durable Product Task identity, typed
artifact promotion, immutable authority snapshots, a bundled static skill
catalog, a live job registry, context continuity, and durable audit evidence.
All of it is proven on exactly one workload: Smart Redaction.

The Action Guide agent flows — caption suggestions and visual annotation
suggestions — predate that foundation and sit entirely outside it. They own no
durable task identity, no authority snapshot, no skill, and no audit evidence.
Their proposals exist only in process memory.

This umbrella migrates both Action Guide agent flows onto the foundation
contracts. Its purpose is not feature work. It is to establish, with a second
and third real consumer, that the foundation contracts express workloads that
are not Smart Redaction — and to leave the shared contracts in a shape a future
skill can plug into without further surgery.

## 2. Prerequisite and governance relationship

The predecessor umbrella's §22 requires user approval of the Gate G3 completion
decision, after which it becomes a historical snapshot and future agent
foundation changes require a new dated design.

**The user confirmed Gate G3 completion on 2026-07-28.** The predecessor
umbrella is therefore a historical snapshot, and this document is the new dated
design rather than an amendment to it. The predecessor file is deliberately left
unedited; its status is recorded here, not retroactively inside it.

Two predecessor boundaries continue to govern this program:

- its §21 launch-video boundary: no gate in this umbrella authorizes
  launch-video work; and
- its deferred-capability restart conditions, which this umbrella does not
  reopen.

## 3. Source-of-truth policy

Current code is the source of truth. Every observation in this umbrella was read
from code on 2026-07-28 and is cited with a path and line so a child spec can
detect drift cheaply. At the start of each child-spec workflow:

1. re-read the cited sites and record material drift;
2. verify the gap still exists;
3. preserve this umbrella's approved boundaries unless new evidence requires an
   explicit amendment; and
4. design against the current product path.

This umbrella remains live until Gate B1 completes. A finished child spec or
plan becomes a historical snapshot and must not be retroactively edited to hide
implementation drift.

## 4. Current-state baseline

### 4.1 Foundation parts that are already domain-neutral

- The audit vocabulary is domain-neutral: `AuditEventKindV1` covers
  `TaskCreated`, `AttemptStarted`, `RunContractBound`, `AuthorityDenied`,
  `ArtifactPromoted`, `ReviewApplyStarted`, `ReviewDecisionCommitted`, and
  `TaskTerminated` (`crates/rollshot-agent/src/audit.rs:239`).
- `AuthoritySnapshot`, the static skill catalog, `RunBudget`, cancellation, the
  job registry, and the CAS/lock/atomic-rename persistence mechanism are
  mechanism, not domain.

### 4.2 Foundation parts that are Smart Redaction shaped

- `TaskKind` has two variants, both Smart Redaction
  (`crates/rollshot-agent/src/product_task.rs:126`).
- `ArtifactKind` has one variant, `SmartRedaction`
  (`crates/rollshot-agent/src/product_task.rs:169`).
- `SourceBinding` is a struct of `base_image_sha256`,
  `annotation_state_sha256`, `document_state_id`, `preset_id`, and
  `active_preset_revision_id`.
- `ProductArtifactMetadata` carries `dry_run_candidate_count` and
  `dry_run_affected_area` as flat fields on the generic type.
- `pending_proposal_payload` is documented as serialized `EditProposal` JSON.
- `DisclosureCeiling` offers only `OcrLayoutOnly` and `FullScreenshot`
  (`crates/rollshot-agent/src/authority.rs:32`); `RunOperation` offers six
  draft/automation/image/review grants
  (`crates/rollshot-agent/src/authority.rs:56`).
- The app-side store lives inside the Smart Redaction UI module
  (`crates/rollshot-app/src/result_workspace/workbench/task_store.rs`,
  `.../audit_store/`), opened at
  `crates/rollshot-app/src/result_workspace/update.rs:1664`.

### 4.3 The Action Guide agent flows

Caption suggestions (`crates/rollshot-app/src/timeline_workspace/caption_agent.rs`):

- a single-shot provider call bounded only by a 30-second timeout (line 273);
- no `AgentRunner`, no `RunBudget`, no tool registry, no cancellation surface;
- the prompt is hardcoded Rust (`build_caption_prompt`);
- provenance already exists in `rollshot-action`:
  `CaptionProposalOrigin::{DurableProject { revision, projection_digest },
  EphemeralGuide { guide_digest } }`
  (`crates/rollshot-action/src/caption_proposal.rs:18`), with staleness checked
  at apply time through `CaptionApplyContext` (line 31).

Visual annotation suggestions
(`crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs`):

- already a bounded run: `AgentRunner` + `AgentConfig` (line 221),
  `visual_annotation_run_budget()`, `RunCancellation`, and
  `AuthorizedModelInput` attachment authorization;
- a consent dialog gates it, and the UI state machine is
  `Idle → ConsentPending → Running → PendingReview`;
- the file-level `#![allow(dead_code)]` comment claiming it is unwired is stale;
  the flow is wired.

Neither flow uses Product Task, the task store, the audit journal, or a skill.
Both hold their proposal in workspace memory with per-suggestion accept/reject.

Slice 5 of the predecessor program already gave Action Guide durable
re-projection — `ActionGuideContextProjectionV1`
(`crates/rollshot-action/src/project/continuity.rs:98`) — but implemented inside
`rollshot-action`, independent of `rollshot_agent::continuity`. The two are
parallel implementations of the same idea, not one contract.

### 4.4 The load-bearing decoupling

`rollshot-action` does not depend on `rollshot-agent`, and `rollshot-agent`
depends only on `rollshot-automation`, `rollshot-edit-proposal`, and
`rollshot-image-document`. `rollshot-app` is the sole translator. `rollshot-agent`
compiles unconditionally; Action Guide sits behind the non-default
`action-guide` feature on `rollshot-app`.

### 4.5 The forcing constraint

`RunContractReceiptV1.skill_use` is a required field, not an `Option`
(`crates/rollshot-agent/src/product_task.rs:628`). Binding a run contract —
and therefore emitting `RunContractBound` and holding an authority receipt —
is impossible without a resolved `SkillUse`. Each migrated flow must own a
bundled skill package; the alternative of making the field optional was
considered and rejected because it opens a contract path where a run holds
authority with no skill provenance.

## 5. Goals

1. One shared set of task, artifact, authority, and audit contracts serving
   Smart Redaction and both Action Guide agent flows.
2. Durable, revision-bound, reviewable artifacts for Action Guide caption and
   visual annotation suggestions.
3. Bundled instruction skills replacing hardcoded prompts on both flows, with
   digest-recorded provenance.
4. Complete durable audit coverage for Action Guide agent runs, including runs
   against unsaved guides.
5. Restoration of unresolved proposals into the existing review surfaces after
   restart, and deterministic reconciliation of everything else.
6. A falsifiable demonstration that the shared contracts generalize: Slice B
   must plug in without changing the shape of any shared contract.

## 6. Non-goals

- Launch-video product design, `LaunchTeaserPlan`, or teaser rendering.
- Reading user project files, or any new filesystem grant or authority.
- Any new user-visible UI surface: no pending-agent-task list, no audit
  inspection view, no new widgets.
- Refactoring the driver's existing run shapes into one. Visual annotation keeps
  its bespoke `run_visual_annotation_with_provider` entry point; Slice A chooses
  between an equivalent bespoke entry point and the existing generic
  tool-registry loop for captions, but neither slice rewrites the other's shape.
- Bringing storyboard, GIF, or MP4 export, or the video-import job, under
  artifact promotion.
- User-authored, remote, dynamically loaded, or self-modifying skills.
- Cross-run transcript persistence or semantic memory.
- Unifying `ActionGuideContextProjectionV1` with
  `rollshot_agent::continuity::ContinuityProjectionV1`.
- Changing Smart Redaction behavior. Only its contract shape and its store's
  module location move.
- Any `rollshot-agent` dependency on `rollshot-action`.
- Parallel implementation of Slice A and Slice B.

## 7. Program architecture

```text
Slice A — Action Guide Caption Provenance
│  owns the shared-contract surgery, driven by its first real consumer
│
▼ Gate A1: durable, revision-bound, reviewable caption artifact proven
│
Slice B — Action Guide Visual Annotation Provenance
│  the falsification test
│
▼ Gate B1: second consumer plugs in with no shared-contract shape change
│
▼ Umbrella complete
```

The two slices are strictly sequential. Parallel implementation is not
authorized: both slices touch the same shared contracts, the same app-side
store module, and the same review-restore machinery.

The shared-contract surgery lands inside Slice A rather than in a separate
contract-only slice. A contract slice with no consumer can only be validated
against Smart Redaction regression and unit tests, which is precisely how a
contract comes to look generic without ever being exercised.

## 8. Cross-slice ownership and data flow

```text
User action in a workspace (consent where required)
        │
        ▼
immutable AuthoritySnapshot  +  resolved SkillUse
        │
        ▼
durable Product Task (kind-tagged) → attempt → bounded agent run
        │
        ▼
validated suggestion batch
        │
        ▼
typed Product Artifact (revision-bound) → explicit review → guide mutation
        │
        └── durable material audit events
```

Ownership is explicit:

- `rollshot-agent` owns the task, artifact, authority, skill, and audit
  contracts, and must not learn about Action Guide types;
- `rollshot-action` owns the guide model, the caption and visual annotation
  proposal models, and apply semantics;
- `rollshot-app` is the only translator between the two, extending the pattern
  visual annotation already uses (agent returns normalized coordinates; the app
  converts to pixel space and `rollshot-action` types);
- the product owns consent, review decisions, and guide mutation truth;
- skills provide instructions only, never authority.

## 9. Shared contract changes — owned by Slice A

Slice A must land all six. Slice B must need none of them changed.

1. **`SourceBinding` becomes a domain-tagged enum.** Today's five fields move
   unchanged into a `SmartRedaction` variant. Action Guide variants carry
   `revision + projection_digest` for a durable project origin and
   `guide_digest` for an ephemeral guide origin.
2. **`dry_run_candidate_count` and `dry_run_affected_area` leave
   `ProductArtifactMetadata`** and move into a kind-specific artifact summary
   enum. Both are Smart Redaction dry-run concepts; captions have no dry-run.
3. **Existing on-disk snapshots must still load.** Persisted task JSON carries
   the flat pre-migration fields. A read path must map them into the new shape,
   proven by a V1 fixture load test. This is additive and read-compatible, not a
   destructive schema bump; the precedent is the existing V1/V2 artifact
   metadata pair and its `#[serde(default)]` fields.
4. **`pending_proposal_payload` interpretation dispatches on `ArtifactKind`.**
   No new field. `canonical_payload_sha256` remains the integrity check.
5. **`DisclosureCeiling` gains a zero-image level.** Captions transmit no pixels
   at all, so neither existing variant is honest in the durable authority
   receipt. This level buys provenance honesty, not enforcement:
   `validate_model_input` counts attachments only, and `OcrLayoutOnly` already
   rejects all of them, so the new level behaves identically there. A caption
   run's actual teeth are its grant set, which never includes
   `InspectPreparedImage`, and an empty prepared-capability set. The enum derives
   `PartialOrd` and `Ord` and is compared as a ceiling, so the new variant must
   order as strictly less than `OcrLayoutOnly`, and Slice A must re-check every
   existing ordering comparison. Inserting it in the wrong position silently
   changes the meaning of existing comparisons.
6. **The app-side store moves out of the Smart Redaction UI module.**
   `task_store.rs` and `audit_store/` move to a shared app module that compiles
   with the `action-guide` feature disabled. Only Action Guide task-kind
   construction sites are feature-gated.
7. **`DocumentContentBinding` becomes domain-tagged, or `AuthorityBinding` holds
   the source binding directly.** `AuthorityBinding` requires a
   `DocumentContentBinding` (`crates/rollshot-agent/src/authority.rs:75`), whose
   constructor requires a base-image digest and an `AnnotationStateV1`
   (`crates/rollshot-agent/src/product_task.rs:546`). A caption run has neither,
   so without this change it cannot construct an `AuthoritySnapshot` at all and
   Gate A1 item 1 is unreachable. Three attached sites move with it: the snapshot
   digest hashes the three document fields (`authority.rs:254`), `authorize_tool`
   compares a supplied binding for equality as its per-call staleness guard
   (`authority.rs:165`), and the receipt exposes `document_binding_digest`
   (`authority.rs:208`). The child spec chooses the shape. A degenerate
   zero-valued binding is not permitted: it writes a false claim into durable
   provenance and reduces the staleness guard to a vacuous comparison while
   appearing to keep it.

New `TaskKind`, `ArtifactKind`, `RunOperation`, source-binding, and
artifact-summary variants are additive and are expected in both slices. Additive
variants are not contract-shape changes.

## 10. New concurrency exposure

`TaskStore::acquire_lock` opens a fresh file handle on `.lock` and takes a
blocking fs4 exclusive lock per operation
(`crates/rollshot-app/src/result_workspace/workbench/task_store.rs:797`).
`TaskStore::open` additionally reconciles all audit journals before returning.

Today only one workspace uses the store, so cross-domain contention has never
occurred. After sharing, Smart Redaction and Action Guide can each drive an
agent run at the same time.

This umbrella therefore fixes:

- exactly one `TaskStore` instance per process, owned above both workspaces and
  shared by reference. Two instances in one process hold distinct file
  descriptors, which flock treats as unrelated holders; they will block each
  other, and nested acquisition self-deadlocks.
- Slice A must add a test exercising concurrent audited operations from two
  domains against one store.

## 11. Cross-slice failure invariants

- Authority is fail-closed. Every grant is checked independently at the
  tool or dispatch boundary; a missing grant is a typed denial that emits
  `AuthorityDenied`.
- A caption-kind task can never obtain an image-disclosure grant, proven by
  test.
- Provider failure, cancellation, and budget exhaustion never promote an
  artifact.
- Cancellation does not automatically retry side effects.
- Staleness is never silently substituted: a mismatched project revision,
  projection digest, guide digest, or artifact revision is a typed rejection.
- An ephemeral-origin task can never reach `Completed` after a restart. There is
  no durable target to apply to; reconciliation must resolve it to `Stale`.
- Audit append failure resolves through the existing
  `TaskTerminal::AuditFailure`.
- No Action Guide error type crosses into `rollshot-agent`'s public contracts.

## 12. Cross-slice privacy invariants

An artifact payload necessarily contains guide-derived text — captions and step
titles — because that is what the user reviews. It is stored under the existing
`agent-tasks/` tree with its existing 0700/0600 modes and 30-day prune, at the
same sensitivity as the `EditProposal` JSON Smart Redaction already persists.

Explicitly prohibited:

- keyframe or screenshot pixel bytes in the task store, artifact payload, or
  audit journal. A visual annotation artifact stores coordinates and text only;
- raw Action Guide semantic input;
- provider credentials or provider-native conversation internals;
- full skill bodies. Digests only.

Every new serialization, tracing, audit, and persistence path added by either
slice needs a privacy test or bounded inspection evidence. Runtime diagnostics
use privacy-safe structured `tracing` events with stable `rollshot::*` targets.

## 13. Slice A — Action Guide Caption Provenance

### 13.1 Problem

Caption suggestion is a single-shot provider call bounded only by a 30-second
timeout, with no durable task identity, authority snapshot, skill, audit
evidence, or budget, and a proposal that lives only in memory. The shared
contracts cannot currently express a task that transmits no image, performs no
dry run, and binds to a project revision.

### 13.2 Child spec must answer

- The exact shapes and names of the `SourceBinding` variants, the artifact
  summary enum, and the new `TaskKind` and `ArtifactKind` variants.
- The zero-image disclosure level's name, its ordering position, and the result
  of auditing every existing ceiling comparison.
- Which `RunOperation` grants a caption run receives and how each is enforced
  independently.
- The caption skill's package id, resource layout, and body, and how the
  hardcoded prompt maps into it. Behavior change must be measured and recorded,
  not assumed absent.
- Whether the caption run uses a bespoke `AgentRunner` entry point parallel to
  `run_visual_annotation_with_provider` or the generic tool-registry loop, and
  its `RunBudget` dimension values.
- How the durable and ephemeral origins flow through task creation, artifact
  promotion, review, restore, and reconciliation.
- Where the single shared `TaskStore` is owned and how both workspaces obtain
  it.
- Which states restore into the existing review surface, which reconcile away,
  and what the user sees in each case.
- The V1 on-disk mapping and its fixtures.

### 13.3 Plan boundary

1. Lock current caption behavior and current Smart Redaction store behavior
   with tests first, as the regression net.
2. Perform the contract surgery with V1 fixture load tests, without behavior
   change.
3. Move the store to the shared module; add the two-domain concurrency test;
   keep Smart Redaction green.
4. Add the bundled caption skill and record prompt regression evidence.
5. Wire the bounded run: authority snapshot, run-contract bind, budget,
   cancellation.
6. Add artifact promotion, review receipt, and stale rejection.
7. Restore into the existing review surface.
8. Add reconciliation: `Running` to `Interrupted`, ephemeral `ReadyForReview`
   to `Stale`.
9. Add audit coverage and privacy tests.

### 13.4 Gate A1

1. A caption run creates a durable Product Task under a new `TaskKind`, records
   attempt and run identity, and binds a `RunContractReceiptV1` carrying the
   authority receipt and the caption skill digest.
2. Suggestions are promoted as a typed artifact under a new `ArtifactKind`,
   bound to `revision + projection_digest` for a durable origin or
   `guide_digest` for an ephemeral origin.
3. Per-suggestion accept and reject commit a review receipt bound to the
   artifact revision.
4. Stale rejection is deterministic when the project revision or projection
   digest changes.
5. After restart, a `Running` caption task reconciles to `Interrupted`, and an
   ephemeral-origin `ReadyForReview` task reconciles to `Stale`.
6. A durable `ReadyForReview` task with a matching revision repopulates the
   existing review surface with no provider call.
7. The run honors a `RunBudget` and cancellation.
8. `TaskCreated`, `AttemptStarted`, `RunContractBound`, `ArtifactPromoted`,
   `ReviewDecisionCommitted`, and `TaskTerminated` are all durably appended and
   privacy-safe.
9. Pre-migration on-disk Smart Redaction tasks still load, and the Smart
   Redaction workflow does not regress.
10. The restore path satisfies the repository's iced UI testing workflow,
    including independent visual review.

## 14. Slice B — Action Guide Visual Annotation Provenance

### 14.1 Problem

Visual annotation already runs bounded, with a budget, cancellation, authorized
attachment input, and a consent dialog, but owns no durable task identity,
authority snapshot, skill, or audit evidence, and its proposal lives only in
memory. Its second role is to falsify or confirm Slice A's contract design.

### 14.2 Child spec must answer

- How the consent dialog's decision becomes an immutable `AuthoritySnapshot`:
  exactly what the user consented to, and what the digest covers.
- Whether image disclosure reuses `InspectPreparedImage` or requires a distinct
  operation. These are semantically different: one is a prepared vision
  capability, the other sends raw PNG bytes to the model as an attachment.
- Which source-binding variant a per-step keyframe run uses — reusing Slice A's
  is preferred, adding one is permitted.
- What `document_state_id` binds against once artifact revisions exist.
- How attachment bytes are proven absent from the store and the audit journal.
- Which shared-contract additions it needs. Any change that is not purely
  additive stops the slice and amends this umbrella.

### 14.3 Plan boundary

1. Lock the current terminal-to-user-visible mapping with tests.
2. Map consent to an `AuthoritySnapshot`; bind the run contract with the new
   bundled visual annotation skill.
3. Add task creation, artifact promotion, and review receipt.
4. Add restore and reconciliation.
5. Add audit coverage and attachment privacy tests.
6. Assert, as an explicit test or review artifact, that no shared contract
   changed shape.

### 14.4 Gate B1

1. Everything equivalent to Gate A1 items 1 through 8, for visual annotation.
2. **No shared contract changed shape.** Permitted: new `TaskKind`,
   `ArtifactKind`, `RunOperation`, source-binding, and artifact-summary
   variants. A gate failure is any change to existing variants or fields of
   `SourceBinding` or `ProductArtifactMetadata`, to the `TaskStore` API, or to
   the audit vocabulary. Such a discovery stops the slice and requires an
   umbrella amendment; Slice B must not absorb it silently.
3. Consent maps to an `AuthoritySnapshot` with a `FullScreenshot` ceiling, and a
   test proves a caption-kind task can never obtain an image grant.

## 15. Verification policy

Each child plan includes:

1. state-machine and contract unit tests;
2. persistence, reconciliation, and crash-consistency tests reusing the store's
   existing `Failpoint` injection;
3. cancellation, staleness, and failure injection;
4. privacy-safe serialization, tracing, and audit tests;
5. regression coverage for Smart Redaction and for the migrated flow;
6. `rtk cargo test` for `rollshot-agent` and `rollshot-app`, run both with and
   without the `action-guide` feature;
7. `rtk cargo fmt --check`;
8. `rtk cargo clippy --workspace --all-targets -- -D warnings`, justified here
   because contract and store changes are cross-cutting;
9. the repository's iced UI testing workflow for the restore path, with an
   independent reviewer that the product-changing agent does not act as; and
10. independent code review before the gate decision.

Slice A additionally requires a V1 on-disk fixture load test, the two-domain
concurrency test, and recorded caption prompt regression evidence.

## 16. Child document contract

```text
current-code exploration
→ child design discussion
→ child spec approval
→ child spec commit
→ implementation plan
→ execution
→ verification and independent review
→ gate decision
```

Child documents:

- `docs/superpowers/specs/YYYY-MM-DD-action-guide-agent-foundation-SLUG-design.md`
- `docs/superpowers/plans/YYYY-MM-DD-action-guide-agent-foundation-SLUG.md`

The two slugs are `captions` and `visual-annotation`. `YYYY-MM-DD` is the child
document's actual creation date.

This umbrella spec lands on `main` independently, before either child spec is
written. Both slices then branch from `main` and inherit it, so neither slice's
branch carries governance for the other. Each slice's child spec, plan, and
implementation share one branch named for that slice's slug.

This umbrella workflow creates no child spec or implementation plan. Each is
written just in time after its start condition is satisfied.

## 17. Amendment policy

An amendment is required when evidence changes:

- a cross-slice ownership or authority boundary;
- the shared-contract change list in §9;
- the single-`TaskStore`-per-process rule;
- the ephemeral-task rule in §11;
- a program-level non-goal, including the no-new-UI-surface and no
  `rollshot-agent` to `rollshot-action` dependency rules;
- a gate's required evidence; or
- the definition of umbrella completion.

The process is: write a decision record describing the evidence and affected
slices, identify completed child documents that remain historical evidence,
present the change for user approval, update only this umbrella and future child
requirements, and do not rewrite completed child specs or plans.

A discovery contained within one slice belongs in that slice's child spec.

### 17.1 Amendment log

| Date | Amendment | Record |
|---|---|---|
| 2026-07-28 | §9 gains item 7 (authority binding); item 5's rationale corrected to provenance honesty rather than enforcement | [`2026-07-28-action-guide-authority-binding-amendment-decision.md`](../spikes/2026-07-28-action-guide-authority-binding-amendment-decision.md) |

## 18. Umbrella completion

This umbrella completes only when:

- Gate A1 and Gate B1 both pass;
- migrations, residual risks, and deferred scope are recorded;
- Smart Redaction behavior is unchanged and its stored tasks still load;
- no non-additive shared-contract change was absorbed without an amendment;
- no new UI surface, filesystem authority, or deferred platform capability was
  added; and
- the user approves the completion decision.

After completion this umbrella becomes a historical snapshot.

## 19. Launch-video boundary

Passing both gates satisfies one prerequisite the deferred launch-video idea
depends on: that the foundation contracts hold more than one workload, so a
future skill-driven artifact can plug in without contract surgery.

It does not satisfy the rest. The remaining gaps recorded on 2026-07-28 are a
teaser renderer, source-material retention sufficient for motion, optional
project-read authority, and the idea's own unrun concierge validation step.
Neither gate in this umbrella schedules or authorizes launch-video work; the
predecessor umbrella's §21 continues to govern that decision.
