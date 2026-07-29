# Action Guide Agent Foundation — Visual Annotation Provenance Design

**Date:** 2026-07-29
**Status:** Design discussion approved; written spec pending user review
**Area:** Agent foundation, Action Guide visual annotation
**Branch:** `feat/action-guide-agent-foundation-visual-annotation`
**Governing umbrella:**
[`2026-07-28-action-guide-agent-foundation-umbrella-design.md`](2026-07-28-action-guide-agent-foundation-umbrella-design.md)
**Predecessor slice:**
[`2026-07-28-action-guide-agent-foundation-captions-design.md`](2026-07-28-action-guide-agent-foundation-captions-design.md)

## 1. Purpose

Slice A proved the shared agent foundation against Action Guide caption
suggestions. Slice B migrates the existing per-step visual annotation suggestion
flow onto the same durable task, authority, skill, artifact, review, restore,
and audit contracts.

This slice is the umbrella's falsification test. It may add domain variants, but
it must not reshape the contracts Slice A generalized. Gate B1 fails if the
implementation changes existing `SourceBinding` or `ProductArtifactMetadata`
variants or fields, the `TaskStore` API, or the audit vocabulary.

The product behavior is frozen for this migration. The existing consent,
prompt, terminal-to-message mapping, review controls, layout, and user-visible
copy remain byte-for-byte unchanged. This slice adds provenance and durability;
it does not improve the visual annotation feature.

## 2. Verified current state

The following observations were re-read from current code after Slice A landed
at HEAD `ee0354e`.

### 2.1 Product flow

`crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs`:

- `VisualAnnotationTaskInput` carries a local `run_id`, cloned `GuideStep`,
  in-process `document_state_id`, and cloned source `RgbaImage`;
- `encode_visual_annotation_attachment` PNG-encodes the selected step's source
  keyframe and builds its `AttachmentDescriptor`;
- `suggest_visual_annotation_task` constructs `AuthorizedModelInput` and calls
  `AgentRunner::run_visual_annotation_with_provider`;
- `build_visual_annotation_prompt` is hardcoded Rust; and
- `map_terminal_to_result` owns the current user-visible terminal mapping.

`crates/rollshot-app/src/timeline_workspace/update.rs` implements:

```text
Idle → ConsentPending → Running → PendingReview
                         ├──────→ NoSuggestion
                         └──────→ Failed
```

The review surface supports Accept all, Reject all, Dismiss, per-suggestion
Accept, and per-suggestion Reject. Review state exists only in
`TimelineWorkspace::visual_annotation_suggestion`.

### 2.2 Existing bounds and authority gap

`AgentRunner::run_visual_annotation_with_provider` already enforces the run
budget, cancellation, one terminal tool, and attachment charging. It currently:

- accepts no `AuthoritySnapshot` or `SkillUse`;
- sends the attachment without a disclosure grant check;
- uses `NullEventSink`;
- hardcodes `AgentTaskProfile::VisualAnnotation.system_prompt()`; and
- maps an impossible audit failure to `ProtocolFailure` because audit is absent.

The consent dialog says that Rollshot sends one reviewed keyframe to the named
provider/model. The immutable authority snapshot must represent exactly that
consent, not broader guide or filesystem authority.

### 2.3 Durable freshness gap

`ActionGuideContextProjectionV1` deliberately excludes pixels, frame digests,
frame dimensions, and annotations. Slice A's `ActionGuideProject` binding
therefore cannot establish that the selected keyframe or annotation document is
still the content reviewed by the model.

`ImageDocument::from_persisted_annotations` reconstructs `state_id` and
`next_state_id` as zero. The current proposal's `document_state_id` is useful
for in-process race rejection, but it cannot be compared across restart.

Reusing Slice A's source binding would also make caption and visual-annotation
tasks share the same source identity. `TaskStore::reconcile_for_source` has no
kind filter and Gate B1 forbids changing its API, so the two workloads could
hide one another's newest restore candidate.

### 2.4 Persistence gap

`VisualAnnotationProposal` contains a cloned `GuideStep` and does not implement
serde. The proposal needs a minimal serializable base rather than making the
whole `GuideStep` model serializable for one consumer.

### 2.5 Drift since the umbrella baseline

Slice A materially changed the shared baseline exactly as its gate record
describes: source and authority bindings are domain-tagged, artifact payloads
are kind-agnostic, the task store now lives under `agent_store`, the caption
skill and single-submit profile exist, and caption task restore/audit are live.
Gate A1 is verified.

The Slice B gap still exists. The visual flow has no Product Task, artifact,
skill, authority receipt, task-store integration, restore, or durable audit.
Its bespoke runner and current iced state machine remain the active product
path. This child spec therefore preserves the umbrella boundary rather than
requiring an amendment.

## 3. Goals

1. Every consent-confirmed visual annotation run owns a durable Product Task,
   attempt, run contract, authority receipt, skill digest, and material audit
   chain.
2. The one disclosed keyframe is independently authorized before provider
   dispatch.
3. Valid suggestions become a typed, revision-bound, pixel-free artifact.
4. Existing per-suggestion review produces a receipt bound to the exact artifact
   revision.
5. A matching durable proposal restores into the existing review surface with
   no provider call.
6. Ephemeral reviews stale on restart; abandoned active tasks interrupt.
7. Slice B lands with additive shared-contract variants only.
8. Existing visual annotation behavior and all user-visible copy remain frozen.

## 4. Non-goals

- No new UI surface, control, task list, copy, or layout.
- No prompt improvement or visual-annotation eval redesign.
- No guide-wide or multi-step visual suggestion run.
- No change to caption behavior or Smart Redaction behavior.
- No `rollshot-agent` dependency on `rollshot-action`.
- No unification of Action Guide and agent continuity projections.
- No persistence of PNG bytes, source pixels, flattened images, or provider
  conversation payloads.
- No replacement of the bespoke visual annotation runner with the generic
  single-submit runner.
- No launch-video work.

## 5. Shared-contract additions

All changes in this section are additive. Existing variants and fields remain
unchanged.

### 5.1 Task, artifact, and summary variants

Add:

```rust
TaskKind::ActionGuideVisualAnnotation
ArtifactKind::ActionGuideVisualAnnotation
ArtifactSummary::ActionGuideVisualAnnotation { suggestion_count: u32 }
```

The artifact schema version starts at 1. Its proposal ID is the existing visual
annotation proposal ID rendered as a decimal string.

### 5.2 Source bindings

Add two variants using primitives only:

```rust
SourceBinding::ActionGuideVisualAnnotationProject {
    project_root_sha256: [u8; 32],
    revision: u64,
    projection_digest: String,
    step_source: u64,
    keyframe: u64,
    keyframe_sha256: [u8; 32],
    annotation_state_sha256: [u8; 32],
}

SourceBinding::ActionGuideVisualAnnotationEphemeralGuide {
    guide_digest: String,
    step_source: u64,
    keyframe: u64,
    keyframe_sha256: [u8; 32],
    annotation_state_sha256: [u8; 32],
}
```

`rollshot-agent` continues to know only primitive identifiers. `rollshot-app`
translates `CandidateId` and `FrameId` into the `u64` fields.

Identity and freshness are separate:

- durable identity is `(project_root_sha256, step_source)`;
- durable freshness is revision, projection digest, keyframe ID, keyframe
  digest, and annotation-state digest;
- ephemeral identity/freshness is the complete variant; and
- bindings from another variant never match.

The durable project binding is used only when the workspace has a saved project
root and is clean. Unsaved or dirty workspaces use the ephemeral variant; no
false durable-restoration claim is written.

The keyframe digest is
`SHA-256("rollshot-action-guide-keyframe-v1\0" || width_le || height_le || raw_rgba)`.
It binds the exact unflattened source pixels that the PNG encodes without making
the digest depend on encoder output. The annotation-state digest is
`SHA-256("rollshot-action-guide-annotations-v1\0" || annotations_json)`, where
`annotations_json` is `serde_json::to_vec` of the ordered, validated persisted
annotation list used by `ActionGuidePresentation`. It does not hash pixels,
paths, or explanations. The digest keeps review staleness durable even though
`ImageDocument::state_id()` resets on load.

The task store's open-time ephemeral match expands to include
`ActionGuideVisualAnnotationEphemeralGuide`. No method signature or ownership
rule changes.

### 5.3 Screenshot attachment operation

Add:

```rust
RunOperation::DiscloseScreenshotAttachment
```

This is distinct from `InspectPreparedImage`. The latter authorizes a prepared
vision capability; the former authorizes dispatch of raw PNG bytes to a model.
Conflating them would let a grant for one disclosure path silently authorize the
other.

A visual annotation run receives exactly:

```text
DiscloseScreenshotAttachment
SubmitReviewCandidate
```

A caption run continues to receive only `SubmitReviewCandidate`. A cross-domain
test pins that caption authority cannot disclose an image.

No new audit event kind is needed. A missing operation, wrong run, wrong
subject, or disclosure violation uses the existing typed authority denial and
`AuthorityDenied` audit event.

## 6. Consent and immutable authority

Consent is captured only after provider/model configuration is loaded. The
existing dialog and its text are unchanged. Confirming consent creates an
`AuthoritySnapshot` with:

- `AuthoritySubject::Document(DocumentContentBinding)`;
- `DisclosureCeiling::FullScreenshot`;
- `existing_product_capture = true`;
- no prepared capabilities;
- grants `{DiscloseScreenshotAttachment, SubmitReviewCandidate}`; and
- the task, attempt, and durable `RunId` created for this run.

`DocumentContentBinding` uses:

- SHA-256 of the exact source keyframe image;
- an `AnnotationStateV1` containing the image dimensions, the checked current
  document state ID, and an empty annotation vector; and
- the same checked state ID as the binding state ID.

The attachment is the unflattened `ImageDocument::source()`, so no existing
annotation pixels are disclosed. The separate source-binding annotation digest
protects apply/restore freshness. Converting the image document's `u64` state
ID to `u32` is checked; overflow fails before any provider call instead of
truncating.

The runner enforces authority at two independent boundaries:

1. before provider dispatch, it checks run/subject match,
   `DiscloseScreenshotAttachment`, and `validate_model_input`; and
2. before accepting the terminal submit call, it checks run/subject match and
   `SubmitReviewCandidate`.

The existing `authorize_tool` signature may enforce the operation at both
boundaries; its documentation can be widened from “tool invocation” to
“run operation.” This does not change the API or receipt shape.

## 7. Static skill and frozen prompt

Add a bundled package:

```text
crates/rollshot-agent/skills/action-guide-visual-annotations/
├── skill.toml
└── SKILL.md
```

Package ID: `action-guide-visual-annotations`.

There are no resource files. `SKILL.md` contains the exact current static visual
annotation system instruction. The caller continues to append the current
per-step user prompt containing source, keyframe, and title. The composed model
request must remain byte-for-byte equal to the pre-migration request.

The catalog exposes a typed bundled-skill resolver parallel to the caption
resolver. The run contract records the resulting `SkillUseReceiptV1`; full
skill bodies never enter task snapshots, artifacts, receipts, audit events, or
tracing.

Tests pin:

- the recorded pre-migration system instruction;
- the recorded user prompt template and dynamic suffix;
- the final composed request bytes;
- a golden skill digest; and
- the current JSON fallback and terminal behavior.

The visual profile has no arbitrary system-prompt parameter. Its only
constructor takes the resolved `SkillUse` and derives the system prompt directly
from `skill_use.body()`. Equality with the frozen pre-migration instruction and
the golden skill digest proves the prompt came from the resolved package without
injecting the digest into the model request and changing its bytes.

## 8. Proposal and artifact model

### 8.1 Serializable proposal base

Replace the proposal's unused cloned `GuideStep` origin with serializable,
minimal domain values. Add:

```text
VisualAnnotationProposalOrigin
  DurableProject { revision, projection_digest }
  EphemeralGuide { guide_digest }

VisualAnnotationStepBase
  step_source
  keyframe
  document_state_id
  image_width / image_height
  keyframe_sha256
  annotation_state_sha256
```

The proposal, IDs, origin, step base, suggestions, payloads, provenance, and
statuses derive serde. Existing constructors continue to take `&GuideStep` and
build the minimal base, so callers do not duplicate translation logic.

`document_state_id` remains the fast in-process stale check. The two SHA-256
fields are the durable check. On restore, matching digests authorize rebasing
pending suggestions to the newly hydrated document's current state ID. A digest
mismatch never rebases; it marks the task stale.

### 8.2 Artifact payload

The artifact payload is caller-serialized, validated visual suggestion data:
normalized/pixel-space geometry, bounded text, confidence, rationale, origin,
and content digests. The pending proposal payload is serialized
`VisualAnnotationProposal` JSON used to restore the review surface.

Both payloads are pixel-free. `canonical_payload_sha256` covers the exact
artifact bytes passed to `record_ready_for_review`.

Promotion uses:

- `ArtifactKind::ActionGuideVisualAnnotation`;
- `ArtifactSummary::ActionGuideVisualAnnotation { suggestion_count }`;
- the source binding created before the run;
- provider/model IDs from the confirmed configuration;
- the bound run contract; and
- the existing artifact-revision mechanism.

## 9. Runner and durable task lifecycle

The bespoke `run_visual_annotation_with_provider` remains. It gains a
visual-annotation profile/context carrying the resolved skill, authority
snapshot, expected run/subject, and audit sink. It continues to own its existing
tool schema, normalized decoder, model-call loop, budget, cancellation, and JSON
fallback.

`NullEventSink` is replaced with the task audit sink. Audit failures remain
typed; they are not converted into protocol failures.

After the user confirms unchanged consent and the selected step/keyframe is
revalidated, the product executes:

```text
create_audited(ActionGuideVisualAnnotation)
→ start attempt
→ construct immutable authority
→ bind RunContractReceiptV1(authority receipt + skill receipt)
→ encode and authorize one PNG attachment
→ bounded bespoke run
→ validate proposal
→ caller-serialize and promote typed artifact
→ show existing PendingReview surface
```

A missing task store fails through the existing visual annotation failure copy
and makes no provider call. There is no unaudited fallback.

Once a task exists, PNG encoding, adapter construction, input construction,
authority, provider, protocol, budget, cancellation, promotion, persistence,
and audit failures all produce the corresponding durable terminal state. No
failure terminal promotes an artifact.

Provider/model configuration is reloaded on confirm as today. If it differs
from the consent snapshot, the flow returns to `ConsentPending` with the
existing copy and creates no task under stale consent.

The local monotonic `visual_annotation_agent_run_id` remains only for late iced
message rejection. Durable Product Task, attempt, and `RunId` values are the
provenance identities.

## 10. Review persistence

The workspace stores:

- the visual Product Task ID;
- the current `ReadyForReview` or `Applying` task snapshot; and
- a review-persistence-in-flight flag.

The existing controls and visible state remain unchanged. While persistence is
in flight, subsequent review messages are ignored so two decisions cannot race
on one snapshot revision.

Review follows the ordered pattern already used by captions:

1. validate the selected step, keyframe, dimensions, current state ID, keyframe
   digest, and annotation-state digest;
2. for Accept, apply the corresponding edit operation and mark the suggestion
   accepted only after the edit succeeds;
3. for Reject, mark only the target pending suggestion rejected;
4. on the first decision, audited-transition `ReadyForReview → Applying`;
5. while suggestions remain pending, retain the `Applying` snapshot; and
6. when all suggestions are decided, write one `ReviewReceipt` and transition
   to `Completed` if any candidate was applied, otherwise `Rejected`.

The receipt binds:

- artifact ID and exact artifact revision;
- proposal ID;
- applied and rejected suggestion IDs;
- an empty generic local delta;
- the resulting image-document state ID; and
- the resulting canonical annotation-state digest.

Accept all validates every pending item before batch apply and preserves the
current stale-count user message. Review persistence never changes those
messages.

Dismiss retains current behavior: it closes the in-memory review surface and
emits no review decision or new copy. A durable `ReadyForReview` task remains
restorable. If partial review already moved it to `Applying`, restart
reconciliation resolves the abandoned task to `Interrupted`; no receipt is
fabricated.

A persistence failure leaves the proposal visible and uses the existing
`Visual annotation suggestion failed. See the annotation modal for details.`
failure copy. Audit failure follows the existing
`TaskTerminal::AuditFailure` contract.

## 11. Restore and reconciliation

### 11.1 Durable restore

Restore runs when a saved project opens and when the selected step changes. It
is attempted only for a clean saved project and a hydrated selected step.

The app recomputes the visual project source binding and calls the unchanged
`TaskStore::reconcile_for_source`. A matching newest `ReadyForReview` task must:

- have `TaskKind::ActionGuideVisualAnnotation`;
- have `ArtifactKind::ActionGuideVisualAnnotation`;
- decode its pending proposal payload;
- match step source, keyframe, image dimensions, image digest, annotation-state
  digest, artifact revision, and proposal ID; and
- contain no attachment or image bytes.

A valid proposal rebases only pending items to the newly hydrated current
`document_state_id`, then populates the existing `PendingReview` state. Restore
uses a panicking provider fixture to prove no model call occurs.

A same-identity freshness mismatch uses the existing audited stale path. Decode,
integrity, or kind mismatches fail closed and do not display a proposal.

The workspace has one visual review surface. Dismissing or changing selection
allows the selected step's own newest matching task to restore; no task-list UI
is added.

### 11.2 Ephemeral and active task reconciliation

The store open sweep treats
`ActionGuideVisualAnnotationEphemeralGuide` as ephemeral:

- abandoned `ReadyForReview` becomes `Stale`;
- abandoned `Created`, `Running`, or `Applying` becomes `Interrupted`; and
- a task owned by a live process remains untouched.

Manual annotation, undo/redo, keyframe replacement, and step deletion keep the
current frozen stale banner. If they invalidate a pending durable visual task,
the same action also schedules its audited transition to `Stale` rather than
only dropping memory state.

## 12. Failure semantics

- Authority is fail-closed at both disclosure and submit boundaries.
- A missing attachment grant sends zero provider requests.
- A wrong subject or run cannot reuse another consent snapshot.
- A `FullScreenshot` ceiling without `DiscloseScreenshotAttachment` is
  insufficient; both checks are required.
- Cancellation, provider failure, protocol failure, budget exhaustion, input
  failure, and audit failure never promote an artifact.
- Late iced task results cannot replace a newer run or task.
- Stale source, step, keyframe, image, annotation state, or artifact revision is
  never silently rebased.
- No `rollshot-action` error crosses into `rollshot-agent` public contracts.
- All current terminal-to-user-message strings remain byte-for-byte unchanged.

## 13. Privacy and diagnostics

Task files, proposal payloads, artifact payloads, receipts, audit journals, and
tracing must contain no:

- PNG signature or attachment bytes;
- raw keyframe or flattened image pixels;
- provider credential or provider-native conversation payload;
- full skill body;
- project filesystem path; or
- raw semantic input events.

Permitted persisted values are primitive step/keyframe IDs, geometry, bounded
review text, confidence/rationale, provider/model identifiers, digests,
revisions, task/run/artifact identifiers, decisions, and timestamps.

Runtime diagnostics use privacy-safe structured `tracing` events under stable
`rollshot::*` targets. No prompt, suggestion text, path, attachment, or provider
payload is logged.

## 14. UI behavior and iced verification

No widget, layout, control, or copy changes are intended. The changed behavior
is restoration into the existing review panel and durable suppression of
concurrent decisions.

Deterministic scenarios:

1. restored proposal at the existing default `1100×760` viewport;
2. restored proposal at the minimum supported viewport; and
3. the maximum 20 suggestions to exercise long-content/scroll behavior.

Each scenario first asserts that the existing header and Accept all, Reject all,
Dismiss, per-item Accept, and per-item Reject controls are present, visible,
enabled when appropriate, and unobscured. Interaction assertions prove review
messages are suppressed while persistence is in flight. Screenshots are
secondary evidence.

Visual preflight recorded during design:

```text
Visual capability: semantic
Provider: native:read
Probe: crates/rollshot-app/tests/eval/fixtures/url_bar/image.png — passed
Pixel diff: iced_test::Snapshot::matches_image
CI: artifact-only
```

The semantic probe identified the dark header, white URL bar, and visible
`https://example.com/u/secret-12345` text. The product-changing agent does not
write or approve golden baselines. Raw baseline/actual/diff evidence goes to a
clean-context independent reviewer under the repository's auto-mode contract.

This is shared Timeline Workspace UI, not a platform-split capture overlay.
Linux and macOS use the same view/update code for this surface; no native
capture-path UI change is authorized.

## 15. Test and verification policy

Implementation is test-first. The plan must include focused RED tests for:

### 15.1 Frozen behavior

- current consent text;
- current running, ready, no-suggestion, cancellation, budget, provider, and
  protocol messages;
- current static system instruction, dynamic user prompt, and final bytes;
- tool-call and JSON fallback terminal behavior; and
- existing per-item and batch review outcomes.

### 15.2 Contracts and authority

- serde round-trip for every new variant;
- durable identity and every freshness field independently;
- variant-domain mismatch;
- ephemeral sweep and live-owner exemption;
- attachment grant, disclosure ceiling, run, subject, and submit grant denial;
- caption inability to receive image disclosure;
- skill prompt/digest invariant; and
- unchanged loading of schema 1/2 Smart Redaction fixtures.

### 15.3 Lifecycle, persistence, and privacy

- full material audit chain from task creation through terminal review;
- provider/model IDs and run contract on promoted metadata;
- exact artifact-revision review receipt;
- restore without provider call;
- stale project revision, projection, step, keyframe, image, annotation state,
  and artifact revision;
- cancellation, budget, provider, protocol, audit, CAS, and failpoint paths;
- no promotion on failure; and
- bounded inspection proving all durable/tracing surfaces exclude prohibited
  data.

### 15.4 Commands

Run:

```text
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-app
rtk cargo test -p rollshot-app --features action-guide
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

No stitching path changes, so core benchmark comparison is not required.

The restore path also runs the repository's iced UI workflow and receives an
independent baseline verdict. The completed change receives independent code
review before Gate B1.

## 16. Gate B1 evidence contract

Gate B1 requires named evidence for:

1. durable task creation, attempt, authority receipt, skill digest, and run
   contract;
2. typed, source-bound, pixel-free artifact promotion;
3. review receipt bound to the exact artifact revision;
4. deterministic project/step/keyframe/image/annotation staleness;
5. active and ephemeral reconciliation after restart;
6. durable restore into the existing surface with no provider call;
7. existing budget and cancellation behavior;
8. complete privacy-safe material audit events;
9. `FullScreenshot` consent plus independent attachment and submit grants;
10. caption inability to disclose images;
11. iced restore evidence and independent visual verdict; and
12. no non-additive shared-contract change.

Item 12 is an explicit compatibility review artifact, not a source-text unit
test. It compares the Slice A baseline with the completed diff and records that:

- existing `SourceBinding` variants and fields are unchanged;
- existing `ProductArtifactMetadata` fields and compatibility deserializer are
  unchanged except for exhaustive handling of additive variants;
- `TaskStore` public API is unchanged;
- audit event vocabulary is unchanged;
- legacy fixtures still load; and
- all shared-contract changes are the additive variants named in §5.

Any non-additive discovery stops implementation and triggers the umbrella
amendment process. It must not be absorbed into Slice B.

## 17. Residual risks to record

1. Project identity remains a canonicalized root-path digest, inherited from
   Slice A. Moving a project stales its pending tasks.
2. Dirty or unsaved visual proposals are ephemeral and cannot restore after a
   restart. This is deliberate; no durable target exists.
3. A crash after local document mutation but before final review persistence can
   leave an `Applying` task that reconciles to `Interrupted`. The task does not
   fabricate a receipt or replay an edit.
4. The visual proposal model gains serde compatibility responsibility. Future
   field changes require an explicit schema/compatibility decision.
5. CI visual evidence remains artifact-only unless a verified semantic agent is
   added to the CI job.

## 18. Deferred scope

- Prompt quality or annotation-selection improvements.
- A visual annotation eval harness beyond the frozen regression net.
- Multiple simultaneous visual review surfaces or a pending-task browser.
- Durable restoration for dirty/unsaved guides.
- Stable project UUID migration.
- Dropping Slice A's legacy V1/V2 task compatibility shims.
- Launch-video, teaser rendering, or project-read authority.

## 19. Completion sequence

```text
approved child spec
→ implementation plan
→ test-first execution
→ focused and full verification
→ iced independent visual review
→ independent code review
→ Gate B1 decision
→ umbrella completion decision
→ user approval of umbrella completion
```

Passing Gate B1 does not authorize launch-video work. The umbrella remains live
until its completion decision is explicitly approved by the user.
