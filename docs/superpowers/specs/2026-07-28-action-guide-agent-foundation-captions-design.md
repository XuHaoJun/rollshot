# Action Guide Caption Provenance Design (Slice A)

**Date:** 2026-07-28
**Status:** Live child spec for Slice A
**Area:** Agent foundation, Action Guide
**Governing umbrella:**
[`2026-07-28-action-guide-agent-foundation-umbrella-design.md`](2026-07-28-action-guide-agent-foundation-umbrella-design.md)
**Amendments applied:**
[`2026-07-28-action-guide-authority-binding-amendment-decision.md`](../spikes/2026-07-28-action-guide-authority-binding-amendment-decision.md)
(§9 item 7 added, item 5 rationale corrected, item 4 widened)

## 1. Scope

Migrate the Action Guide caption suggestion flow onto the shared agent
foundation contracts, and carry the shared-contract surgery that makes a
non-Smart-Redaction workload expressible. Slice B (visual annotation) then plugs
in without changing any shared contract's shape.

This slice adds no product feature and no new UI surface. It changes two
user-visible behaviors, both consequences of putting the flow on the foundation
rather than additions to it:

1. an unresolved caption proposal survives a restart and reappears in the
   existing review surface; and
2. a suggestion run now stops when the user leaves the workspace, starts another
   run, or closes the project, where today it continues to completion
   invisibly. See §4.4.

All observations below were read from code on 2026-07-28 and are cited with path
and line. The plan must re-read them and record drift.

## 2. Current-state baseline

`crates/rollshot-app/src/timeline_workspace/caption_agent.rs`:

- `suggest_captions_with_timeout` is a single-shot provider stream bounded only
  by a 30-second `tokio::time::timeout_at` (line 273). There is no
  `AgentRunner`, no `RunBudget`, no `ToolRegistry`, and no reachable
  cancellation: the `RunCancellation` is constructed, passed into
  `StreamBounds`, and never triggered by the UI.
- The instruction text is hardcoded in `build_caption_prompt`, placed in the
  user message. The system prompt is an inline string literal.
- `PreparedCaptionContext` already carries the provenance this slice needs:
  `Durable { guide, projection }` exposing `projection.revision()` and
  `projection.digest()`, or `Ephemeral { guide, guide_digest }`.
- `parse_caption_tool_args` and `parse_caption_response` perform strict decoding
  and reject empty captions.

`crates/rollshot-action/src/caption_proposal.rs`:

- `CaptionProposalOrigin::{DurableProject { revision, projection_digest },
  EphemeralGuide { guide_digest } }` (line 18).
- `CaptionApplyContext` gates apply on `revision`, `projection_digest`, and
  `clean` (line 31).
- `CaptionProposal::apply(guide, context, id)` (line 182) has no
  edit-then-accept path: a suggestion is accepted or rejected as authored.
- `has_pending()` already reports whether any suggestion awaits a decision.

The proposal lives only in workspace memory
(`timeline_workspace/mod.rs:322`: `caption_proposal: Option<CaptionProposal>`),
with per-suggestion accept/reject and an accept-all.

Provider configuration is already reachable: the timeline workspace calls
`result_workspace::workbench::{load_provider_config, build_adapter}`
(`timeline_workspace/update.rs:1237`, `:1253`).

## 3. Contract changes

### 3.1 `SourceBinding` becomes domain-tagged

```rust
pub enum SourceBinding {
    SmartRedaction {
        base_image_sha256: [u8; 32],
        annotation_state_sha256: [u8; 32],
        document_state_id: u32,
        preset_id: String,
        active_preset_revision_id: Option<String>,
    },
    ActionGuideProject {
        project_root_sha256: [u8; 32],
        revision: u64,
        projection_digest: String,
    },
    ActionGuideEphemeralGuide {
        guide_digest: String,
    },
}
```

The `SmartRedaction` variant's fields are today's five, unchanged.

`project_root_sha256` is the SHA-256 of the canonicalized project root path. The
project manifest has no stable identity — `ProjectManifestV2` carries
`schema_version`, `revision`, `title`, region, source, outputs, frames, steps,
and import warnings, and nothing else — so the directory path is the only
available identity. Adding a manifest UUID was rejected: it changes a persisted
product format that nothing else in this slice needs. The accepted consequence
is that moving or renaming a project directory orphans its pending tasks, which
then reconcile to `Stale`; the user re-runs the suggestion.

Two methods replace the two inline comparisons currently hardcoded in
`reconcile_for_source` (`task_store.rs:1292`, `:1299`):

- `identity_matches(&self, other: &SourceBinding) -> bool` — is this task about
  the same source at all? `SmartRedaction` compares `base_image_sha256`;
  `ActionGuideProject` compares `project_root_sha256`;
  `ActionGuideEphemeralGuide` compares `guide_digest`. Variants of different
  kinds never match.
- `freshness_matches(&self, other: &SourceBinding) -> bool` — is it still valid
  for the current state? `SmartRedaction` compares
  `annotation_state_sha256`; `ActionGuideProject` compares `revision` and
  `projection_digest`; `ActionGuideEphemeralGuide` is trivially true, because an
  ephemeral guide's identity and freshness are the same digest. Staleness for
  ephemeral origins is not this comparison's job — it is enforced by the
  open-time sweep in §5.4.

Today's flat accessors become variant-scoped. Four sites were audited and three
change; the rest are test fixtures:

| Site | Change |
|---|---|
| `task_store.rs:1292`, `:1299` | Replaced by `identity_matches` / `freshness_matches` |
| `continuity.rs:262`–`:266` | Per-variant canonical hash with a domain separator per variant |
| `result_workspace/update.rs:2762` | Guarded on the `SmartRedaction` variant |
| `product_task.rs:652` | `ValidateFinite` stays trivially `Ok` |

### 3.2 `ArtifactSummary` replaces two flat fields

```rust
pub enum ArtifactSummary {
    SmartRedaction { dry_run_candidate_count: u32, dry_run_affected_area: f32 },
    ActionGuideCaptions { suggestion_count: u32 },
}
```

`ProductArtifactMetadata` holds `summary: ArtifactSummary` where it holds
`dry_run_candidate_count: u32` and `dry_run_affected_area: f32` today. Captions
have no dry run, so no honest value exists for either field.

### 3.3 The artifact payload surface becomes kind-agnostic

`record_ready_for_review` takes `payload: SmartRedactionReviewPayload` and
serializes it internally (`product_task.rs:948`). Its payload parameter becomes
`Vec<u8>` serialized by the caller. `PromotionContext`'s `source:
PayloadSourceV1` and `proposal: PayloadProposalV1` (`product_task.rs:607`) move
with it for the same reason.

Both `pending_artifact_payload` and `pending_proposal_payload` become bytes whose
interpretation dispatches on `ArtifactKind`. No snapshot field is added.
`canonical_payload_sha256` remains the integrity check, computed over the
caller-serialized bytes.

The caption artifact payload is the canonical JSON of the promoted suggestion
batch: for each suggestion, its identifier, target step source, suggested title,
suggested caption, confidence, and rationale. The caption proposal payload is
the serialized `CaptionProposal` needed to rebuild the review surface without a
provider call, mirroring how the workbench restores an `EditProposal`.

### 3.4 New variants

- `TaskKind::ActionGuideCaptions`
- `ArtifactKind::ActionGuideCaptions`
- `DisclosureCeiling::TextMetadataOnly`

`RunOperation` needs no new variant. See §4.3.

### 3.5 `DisclosureCeiling::TextMetadataOnly`

Declared first in the enum, so it orders as strictly less than `OcrLayoutOnly`.

The umbrella required Slice A to audit every existing ordering comparison. **The
audit result is that none exist.** The only consumer of the value is the `match`
in `validate_model_input` (`authority.rs:182`); the remaining uses are the
receipt field (`:206`), the accessor (`:237`), the continuity copy (`:267`),
`Debug` (`:288`), and the error payload (`:312`). The derived `PartialOrd` and
`Ord` are currently unused. Declaration order is therefore safe today and is
fixed now so future ceiling comparisons are correct by construction.

`validate_model_input` gains a `TextMetadataOnly` arm requiring exactly zero
attachments. This is the same observable rule `OcrLayoutOnly` already enforces.
The level exists for provenance honesty: `disclosure_ceiling` is recorded in the
durable authority receipt, and recording `OcrLayoutOnly` for a run that never
touched OCR, layout, or any image would be a false claim. A caption run's actual
restraint comes from its grant set and its empty prepared-capability set.

### 3.6 `AuthoritySubject` replaces the document binding

```rust
pub enum AuthoritySubject {
    Document(DocumentContentBinding),
    ActionGuideProject {
        project_root_sha256: [u8; 32],
        revision: u64,
        projection_digest: String,
    },
    ActionGuideEphemeralGuide { guide_digest: String },
}
```

`AuthorityBinding` holds `subject: AuthoritySubject` in place of
`document_binding: DocumentContentBinding` (`authority.rs:75`).
`authorize_tool(run_id, subject: &AuthoritySubject, required)` compares the
supplied subject for equality, preserving its role as the per-tool-call
staleness guard (`authority.rs:165`).

An enum was chosen over having `AuthorityBinding` hold the `SourceBinding`
directly. The latter would widen what the Smart Redaction authority digest
covers, adding `preset_id` and `active_preset_revision_id`, which silently
changes existing semantics in a slice that has no business changing them.

The receipt keeps its persisted field name. The Rust field may be renamed, but
the serialized key stays `document_binding_digest` via `#[serde(rename)]`, or
old task JSON containing `RunContractReceiptV1` stops loading.

**Residual uncertainty, to be resolved by test before the formula changes.**
`AuthoritySnapshot::snapshot_digest` is persisted inside `RunContractReceiptV1`.
Every comparison found is a comparison against itself — `run.rs:4536`,
`task_store.rs:2822` — or a copy into a continuity projection, and
`ContinuityProjectionV1` is computed on demand from a loaded snapshot rather than
persisted. Changing the digest formula therefore appears safe. The plan must
establish this by test before adopting a per-variant tagged hash. If a
recompute-and-compare against a persisted digest is found, the fallback is to
keep the `Document` arm's hash input byte-identical and give only the new arms a
domain separator.

### 3.7 Store schema version

Schema 1 and 2 already coexist: `new` writes 1, `new_v2` writes 2, `load`
rejects `> 2` (`task_store.rs:493`), and `record_ready_for_review` requires a
run contract when the version is `>= 2` (`product_task.rs:983`).

New tasks write schema 3. The load guards relax to `> 3`. Legacy files
deserialize through a two-arm untagged DTO that tries the tagged form first and
the legacy flat struct second, mapping the latter to `SourceBinding::
SmartRedaction`. Fixtures for both schema 1 and schema 2 are committed and
loaded by test.

Caption tasks always require a run contract, because they always resolve a
skill and an authority snapshot.

### 3.8 Store module move

`task_store.rs` and `audit_store/` move from
`crates/rollshot-app/src/result_workspace/workbench/` to
`crates/rollshot-app/src/agent_store/`, which must compile with the
`action-guide` feature disabled. Only Action Guide task-kind construction sites
are feature-gated.

Exactly one `TaskStore` instance exists per process. It is opened at application
initialization instead of inside `result_workspace/update.rs:1664`, and shared as
an `Arc` clone into both workspaces; the existing
`workbench.task_store: Option<Arc<TaskStore>>` field shape is retained on both
sides. `acquire_lock` takes a blocking fs4 exclusive lock per operation
(`task_store.rs:797`), and two instances in one process hold distinct file
descriptors that flock treats as unrelated holders, so a second instance would
block or self-deadlock.

## 4. The bounded run

### 4.1 A reusable single-submit profile

The caption run does not use `run_with_provider`. That entry point's signature is
already domain-neutral — it threads authority, skill use, continuity source, and
audit sink — but it requires a `ToolContext`
(`tools.rs:637`), a fourteen-field structure holding an automation draft and
source, `rollshot_automation` validation limits and execution policy, an
`EditProposal`, a `DocumentContentBinding`, image dimensions, and dry-run state.
A caption run can construct none of it.

Instead, Slice A extracts the shape of `run_visual_annotation_with_provider`
(`driver.rs:1692`–1989) into a parameterized bounded profile, hung off the
existing `AgentTaskProfile` enum (`driver.rs:177`, today a single
`VisualAnnotation` variant marked `dead_code`) with a `Captions` variant added.

```rust
pub enum SingleSubmitTerminal {
    Submitted { arguments: serde_json::Value },
    Declined { reason: Option<String> },
    Cancelled,
    BudgetExhausted { dimension: BudgetDimension },
    ProviderFailure,
    ProtocolFailure,
    AuthorityDenied { operation: RunOperation },
}
```

The profile returns raw submitted arguments rather than typed drafts. Semantic
decoding stays with the caller, reusing today's `parse_caption_tool_args`. This
follows from the decoupling rule: caption draft types live in `rollshot-action`,
which `rollshot-agent` must not depend on. The profile enforces transport bounds
only — argument bytes, tool-call count, turn count.

The profile does four things `run_visual_annotation_with_provider` does not,
and those four are the point of this slice:

1. takes `authority: &AuthoritySnapshot` and calls
   `authorize_tool(run_id, subject, RunOperation::SubmitReviewCandidate)` before
   accepting a submitted payload; denial yields `AuthorityDenied` and appends
   the audit event;
2. takes `skill_use: &SkillUse` and composes the system prompt from it;
3. takes an audit sink, so `AuthorityDenied` is appendable from inside the run,
   as `run_with_provider` already does;
4. calls `validate_model_input` before dispatch, so the `TextMetadataOnly`
   ceiling is enforced rather than assumed.

`run_visual_annotation_with_provider` is not touched in this slice. Slice B moves
it onto the profile as part of its own migration.

### 4.2 The caption skill

Package at `crates/rollshot-agent/skills/action-guide-captions/`:

```toml
schema_version = 1
package_id = "action-guide-captions"
name = "Action Guide Captions"
description = "Suggest reviewable Action Guide titles and captions."
declared_version = "1"
main = "SKILL.md"
```

`SKILL.md` contains the three instruction sentences from today's
`build_caption_prompt`, verbatim. A test asserts the resolved skill body equals
that exact text. A `bundled_action_guide_captions_use()` resolver mirrors
`bundled_smart_redaction_use()`.

The system prompt is composed as
`{envelope}\n\n<rollshot-skill package="..." digest="...">\n{body}\n</rollshot-skill>`,
matching `compose_smart_redaction_prompt` (`driver.rs:166`). The user message
becomes only the dynamic `Steps: {json}` portion.

Byte-identical prompt preservation is impossible under this composition, so the
delta is recorded rather than denied. Exactly three things change, and the
instruction text itself does not:

1. the instruction text moves from the user message to the system prompt;
2. it is wrapped in the `<rollshot-skill>` element with a digest attribute;
3. today's inline system prompt line becomes a caption envelope constant.

Improving the instruction text is explicitly out of scope for this slice, so
that any future behavior change has a clean baseline to be measured against.

### 4.3 Authority construction

Grants are exactly `{RunOperation::SubmitReviewCandidate}`. No new variant is
needed: the guide content is composed into the prompt before the run rather than
fetched by a tool, so there is nothing to read and no read grant to hold.
`prepared_capabilities` is empty and `existing_product_capture` is false, which
`AuthoritySnapshot::new` accepts because `InspectPreparedImage` is absent.

The subject comes straight from `PreparedCaptionContext`:
`ActionGuideProject { project_root_sha256, revision, projection_digest }` for a
durable origin, `ActionGuideEphemeralGuide { guide_digest }` for an ephemeral
one.

The snapshot is then bound into the task as
`RunContractReceiptV1 { authority: authority.receipt(now), skill_use:
skill_use.receipt(), bound_at_unix_ms: now }` through an audited CAS transition,
following `run.rs:1240`–`:1265`.

### 4.4 Budget and preserved behavior

```rust
pub fn caption_run_budget() -> RunBudget    // 30s wall, 2 model calls,
                                            // 1 tool call, 0 attachments,
                                            // 32k input, 1_200 output tokens,
                                            // 4_096 argument and result bytes,
                                            // all Smart Redaction dimensions 0
```

Wall time is 30 seconds, matching today's timeout exactly, and output tokens
1200 match today's `max_tokens`. Two user-facing behaviors must be preserved
deliberately:

- Wall-time exhaustion arrives as `BudgetExhausted { wall_time }` where today it
  arrives as a timeout. The app maps it back to the existing
  `"Caption suggestions timed out."` string. No copy changes.
- Cancellation must be genuinely honored, but the umbrella forbids new UI. The
  workspace owns the `RunCancellation` and triggers it on existing exits —
  leaving the workspace, starting another suggestion run, closing the project —
  with no new widget.

## 5. Persistence, restore, and reconciliation

### 5.1 Lifecycle mapping

| Caption event | Transition | Audit event |
|---|---|---|
| Suggestions requested | create at schema 3 → `Created` | `TaskCreated` |
| Context prepared, run starting | `start_attempt` → `Running` | `AttemptStarted` |
| Authority and skill resolved | `bind_run_contract` | `RunContractBound` |
| Submit denied by authority | no state change | `AuthorityDenied` |
| Batch decoded and validated | `record_ready_for_review` → `ReadyForReview` | `ArtifactPromoted` |
| First accept or reject | `begin_apply` → `Applying` | `ReviewApplyStarted` |
| No suggestion left pending | `complete_apply` or `reject` | `ReviewDecisionCommitted` |
| Failure, cancel, budget | `record_terminal` | `TaskTerminated` |
| Revision moved under a pending task | `mark_stale` | `TaskTerminated` |
| Process died mid-run | `reconcile_interrupted` at open | `TaskTerminated` |

### 5.2 Batch review against a single apply pass

Captions decide per suggestion; the task lifecycle has one `Applying` pass. The
artifact is the whole batch. `begin_apply` fires on the first decision. When
`has_pending()` becomes false, the task closes through `complete_apply` if any
suggestion was accepted, or through `reject` if every suggestion was rejected.

A crash during a partly reviewed session leaves the task in `Applying`, which
reconciles to `Interrupted`. No applied caption is lost, because an accepted
suggestion is written into the project when it is applied; the guide is the
authoritative state and the task's `Applying` is in-flight bookkeeping only.
This is the umbrella's artifact-re-projection principle rather than an exception
to it.

`ReviewReceipt` is reused unchanged. `applied_candidates` and
`rejected_candidates` hold suggestion identifiers.
`resulting_document_state_id` and `resulting_document_digest` are already
`Option` and are `None`. `local_delta`'s `moved_candidates` and
`manual_additions` are honestly empty, because `CaptionProposal::apply` has no
edit-then-accept path.

`CaptionSuggestionId` is a `u64` while the receipt's candidate vectors are
`Vec<u32>`. The plan must assert the identifier bound rather than cast silently.

### 5.3 Restore

On entering the timeline workspace for a durable project, the app calls
`reconcile_for_source` with the project's binding. A `ReadyForReview` task whose
identity matches and whose freshness matches repopulates the existing
`caption_proposal` state from `pending_proposal_payload`, with no provider call.
Freshness mismatch takes the existing audited `mark_stale` path. Identity
mismatch is skipped.

The restored proposal does not bypass any existing guard: applying still runs
through `CaptionApplyContext`, which re-checks revision, projection digest, and
cleanliness.

### 5.4 Ephemeral reconciliation

An ephemeral-origin task can never be applied after a restart, because there is
no durable target. Enforcing this in the per-source matcher is impossible — a
matcher cannot distinguish processes.

It is enforced instead by a sweep at `TaskStore::open`, which already reconciles
all audit journals before returning. `open` runs once per process, which is
precisely "after restart". The sweep resolves every non-terminal
ephemeral-origin task: `Running` and `Applying` to `Interrupted`,
`ReadyForReview` to `Stale`.

Retention is unchanged: the existing 30-day prune applies, with no special case.

## 6. Failure semantics

- Authority is fail-closed. The submit tool authorizes before accepting a
  payload; a missing grant is a typed denial with an `AuthorityDenied` audit
  event and no state change.
- No terminal other than a validated `Submitted` batch may promote an artifact.
  `Declined`, `Cancelled`, `BudgetExhausted`, `ProviderFailure`,
  `ProtocolFailure`, and `AuthorityDenied` all resolve through `record_terminal`.
- A decode failure after `Submitted` is a `record_terminal` with
  `AgentProtocolFailure`, not a partial promotion. The existing decoder already
  validates the whole batch before returning.
- Cancellation never retries.
- Staleness is never substituted: revision, projection digest, guide digest, and
  artifact revision mismatches are typed rejections.
- Audit append failure resolves through the existing
  `TaskTerminal::AuditFailure`.
- No `rollshot-action` error type crosses into `rollshot-agent`'s public API.

## 7. Privacy

A caption artifact payload contains guide-derived text — suggested titles and
captions, and the step titles they target — because that is what the user
reviews. It is stored under the existing `agent-tasks/` tree at its existing
0700 and 0600 modes and under its existing prune.

Prohibited and tested:

- no frame or keyframe bytes anywhere in the task JSON or audit journal. A
  caption run never holds any, and `attachments` is budgeted at zero;
- no full skill body in durable state. The receipt carries a digest;
- no caption or step text in the audit journal. Audit records identities,
  digests, and decisions; the text lives only in the task's artifact payload.

## 8. Test strategy and Gate A1 mapping

| Gate A1 item | Evidence |
|---|---|
| 1. Durable task, new kind, run contract bound | Task-transition test asserting `Created → Running → bind_run_contract`, with the receipt carrying both the authority digest and the caption skill digest |
| 2. Typed artifact bound to origin | Promotion tests for both origins, asserting the recorded binding and `canonical_payload_sha256` |
| 3. Review receipt bound to artifact revision | Accept and reject tests asserting the receipt's artifact revision and candidate vectors |
| 4. Deterministic stale rejection | Revision-bumped and digest-changed cases both rejected; the same input twice yields the same outcome |
| 5. Reconciliation after restart | `Running → Interrupted`; ephemeral `ReadyForReview → Stale` via the open-time sweep |
| 6. Restore without a provider call | Restore test with a provider adapter that panics if called |
| 7. Budget and cancellation honored | Wall-time exhaustion mapped to today's string; cancellation before and mid-stream both yield `Cancelled` |
| 8. Audit coverage, privacy-safe | Every event in the §5.1 mapping appended, including `AuthorityDenied`; journal asserted free of caption and step text |
| 9. Smart Redaction unregressed | Existing workbench suites green; schema 1 and 2 fixtures load |
| 10. Restore path UI evidence | The repository's iced UI testing workflow, with an independent reviewer that this slice's implementing agent does not act as |

Slice A additionally requires:

- schema 1 and schema 2 on-disk fixtures, loaded and asserted;
- a concurrency test driving audited operations from both domains against one
  `TaskStore`;
- a test asserting the skill body equals today's instruction text verbatim;
- the authority-digest recomputation audit described in §3.6, resolved before
  the hash formula changes;
- compilation and test runs both with and without the `action-guide` feature.

## 9. Slice-specific exclusions

- No change to `run_visual_annotation_with_provider` or any visual annotation
  behavior.
- No caption prompt improvement, and no caption evaluation harness.
- No new UI surface, affordance, or copy. Restore reuses the existing review
  surface; failure strings are preserved.
- No project manifest schema change.
- No `ToolContext` generalization.
- No move of `load_provider_config` or `build_adapter`.
- No unification of `ActionGuideContextProjectionV1` with
  `rollshot_agent::continuity`.
- No new `RunOperation` variant.

## 10. Residual risks and noted-not-done

| Item | Disposition |
|---|---|
| Authority digest formula change | Resolved by test before adoption; documented fallback in §3.6 |
| Project identity is a path digest | Accepted. Moving a project orphans pending tasks, which reconcile to `Stale` |
| Prompt relocation may shift model output | Instruction text proven identical; structural delta recorded; no eval harness in this slice |
| Two domains now share one lock | Covered by the concurrency test; single-instance rule enforced by construction |
| `load_provider_config` and `build_adapter` still live in the Smart Redaction module and are cross-imported by the timeline workspace | Noted, not done. Not required by captions |
| `visual_annotation_agent.rs` carries a stale `#![allow(dead_code)]` header comment claiming the flow is unwired | Noted, not done. Slice B owns that file |
