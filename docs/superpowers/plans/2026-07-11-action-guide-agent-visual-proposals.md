# Action Guide Agent Visual Proposals Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Explicitly authorize one reviewed keyframe for agent-generated, user-reviewable number callouts, text notes, and opaque redactions.

**Architecture:** `rollshot-action` owns a provider-free visual proposal contract that validates a whole batch and lowers it to existing `rollshot_image_document::EditOp` values. `rollshot-agent` runs a single bounded image task. Timeline Workspace gates it with consent, then reviews and atomically applies proposals in the existing annotation modal.

**Tech Stack:** Rust 2024; iced 0.14; `rollshot-action`; `rollshot-agent`; `rollshot-image-document`; `serde_json`; `tracing`.

## Global Constraints

- One visual run sends exactly one selected, reviewed keyframe, and only after **Continue** in the consent dialog.
- Supported payloads are only `NumberCallout`, `TextNote`, and `OpaqueRedaction`.
- Validate the entire untrusted batch before it reaches UI; never apply a partial valid subset.
- Applying requires matching step source, keyframe, dimensions, and `ImageDocument::state_id()`. A stale proposal never applies; after Rollshot itself accepts one item, it rebases only the remaining items to the resulting state id so the user can continue reviewing that same batch.
- **Accept all** performs exactly one `ImageDocument::apply_batch` call.
- Caption suggestions stay text-only, guide-wide, and never invoke consent or attach pixels.
- Do not export provider/model data, prompts, responses, consent, attachment bytes, rationales, or run ids.
- Use stable structured `rollshot::*` tracing targets; never log prompt text or attachment bytes.
- Before UI changes, invoke `iced-rs`. Verify Action Guide behavior with `rtk cargo test -p rollshot-app --features action-guide`; run formatting and both workspace and Action Guide feature clippy checks before handoff.

---

## File Structure

| Path | Responsibility |
|---|---|
| `crates/rollshot-action/src/visual_annotation_proposal.rs` | Typed visual payloads, validation, staleness, and `EditOp` lowering. |
| `crates/rollshot-agent/src/visual_annotation.rs` | Bounded profile, structured tool schema, and safe terminal outputs. |
| `crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs` | Post-consent PNG authorization and normalized-coordinate conversion. |
| `crates/rollshot-app/src/timeline_workspace/annotation.rs` | Non-mutating ghost projection and atomic proposal application. |
| `crates/rollshot-app/src/timeline_workspace/{mod,update,view}.rs` | Consent, lifecycle, review state, and iced UX. |

### Task 1: Define and test the visual proposal contract

**Files:**
- Create: `crates/rollshot-action/src/visual_annotation_proposal.rs`
- Modify: `crates/rollshot-action/src/lib.rs`
- Test: inline tests in `visual_annotation_proposal.rs`

**Interfaces:**
- Produces `VisualAnnotationProposal::from_agent_drafts`, `validate_item`, `reject`, `reject_all`, and `pending_edit_ops`.
- Uses `CandidateId`, `FrameId`, `GuideStep`, `ImagePoint`, `ImageRect`, and `EditOp`.
- Has no provider type, prompt, response, or attachment bytes.

- [ ] **Step 1: Write failing contract tests**

Add a test that builds a three-item batch and expects three lowerable operations:

```rust
let proposal = VisualAnnotationProposal::from_agent_drafts(
    VisualAnnotationProposalId(9), 9, &step(), 41, 320, 240,
    vec![
        draft_callout(1, (16.0, 20.0), (80.0, 30.0)),
        draft_note(2, (24.0, 40.0), "Click Save"),
        draft_redaction(3, 100.0, 50.0, 80.0, 30.0),
    ],
).expect("valid batch");
assert_eq!(proposal.pending_edit_ops().unwrap().len(), 3);
```

Add separate tests rejecting non-finite/out-of-bounds points, blank or >500-character notes, zero-area/out-of-bounds redactions, invalid confidence, duplicate ids, a batch over `MAX_VISUAL_SUGGESTIONS`, and any single invalid item. Test `validate_item` against missing step, changed source/keyframe/state id/dimensions. Test that `pending_edit_ops` returns `NotFullyPending` and no operations after an item is rejected or stale.

- [ ] **Step 2: Verify the new test fails**

Run: `rtk cargo test -p rollshot-action visual_annotation_proposal::tests::valid_three_primitive_batch_lowers_to_three_edit_ops`

Expected: FAIL because the module and API do not exist.

- [ ] **Step 3: Implement the provider-free contract**

Implement these exact types:

```rust
pub enum VisualAnnotationPayload {
    NumberCallout { tip: ImagePoint, bubble: ImagePoint },
    TextNote { position: ImagePoint, text: String },
    OpaqueRedaction { bounds: ImageRect },
}
pub struct VisualAnnotationSuggestionDraft {
    pub id: VisualAnnotationSuggestionId,
    pub payload: VisualAnnotationPayload,
    pub confidence: f32,
    pub rationale: Option<String>,
}
pub struct VisualAnnotationBase {
    pub step_source: CandidateId,
    pub keyframe: FrameId,
    pub document_state_id: u64,
    pub image_width: u32,
    pub image_height: u32,
}
```

Store `Agent { run_id }` provenance and `Pending | Accepted | Rejected | Stale` status per suggestion. `from_agent_drafts` trims rationale, requires 1..=`MAX_VISUAL_SUGGESTIONS` items, and rejects the entire batch if any field is invalid. `pending_edit_ops` maps exactly to `EditOp::AddNumberCallout`, `AddTextNote`, and `AddRedaction`. Re-export all public types from `lib.rs`.

- [ ] **Step 4: Verify core tests pass**

Run: `rtk cargo test -p rollshot-action visual_annotation_proposal::tests`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-action/src/visual_annotation_proposal.rs crates/rollshot-action/src/lib.rs
rtk git commit -m "feat(action): add visual annotation proposals"
```

### Task 2: Add one bounded multi-primitive agent profile

**Files:**
- Create: `crates/rollshot-agent/src/visual_annotation.rs`
- Modify: `crates/rollshot-agent/src/{driver,lib}.rs`
- Test: inline tests in `visual_annotation.rs` and `crates/rollshot-agent/tests/provider_contract.rs`

**Interfaces:**
- Consumes one `AuthorizedModelInput` attachment and `RunCancellation`.
- Produces `VisualAnnotationRunTerminal::{Suggested, NoSuggestion, Cancelled, BudgetExhausted, ProviderFailure, ProtocolFailure}`.
- `Suggested` carries normalized drafts only, never provider payload or image bytes.

- [ ] **Step 1: Write failing schema and privacy tests**

Parse a tool call containing one proposal of each kind:

```rust
let drafts = parse_visual_annotation_tool_args(&json!({
  "suggestions": [
    {"id":1,"kind":"number_callout","tip":{"x":0.1,"y":0.2},
     "bubble":{"x":0.4,"y":0.2},"confidence":0.8,"rationale":null},
    {"id":2,"kind":"text_note","position":{"x":0.3,"y":0.4},
     "text":"Click Save","confidence":0.9,"rationale":"Visible action"},
    {"id":3,"kind":"opaque_redaction","bounds":{"x":0.5,"y":0.1,"width":0.2,"height":0.1},
     "confidence":0.7,"rationale":"Account data"}
  ]
})).unwrap();
assert_eq!(drafts.len(), 3);
```

Reject extra fields, incorrect kind-specific fields, normalized values outside `0.0..=1.0`, empty output, and oversized batches. In `provider_contract` verify one attachment, two turns, one tool call, 30-second deadline, 4 KiB argument/result limits, cancellation, and debug redaction.

- [ ] **Step 2: Verify the profile test fails**

Run: `rtk cargo test -p rollshot-agent visual_annotation::tests::decodes_normalized_visual_annotation_batch`

Expected: FAIL because the profile does not exist.

- [ ] **Step 3: Implement the profile**

Add `submit_visual_annotation_suggestions` with strict JSON schema. Require exact fields:

```text
number_callout  -> id, kind, tip{x,y}, bubble{x,y}, confidence, rationale
text_note       -> id, kind, position{x,y}, text, confidence, rationale
opaque_redaction -> id, kind, bounds{x,y,width,height}, confidence, rationale
```

Implement `visual_annotation_run_budget()` with the existing callout limits. Add `AgentRunner::run_visual_annotation_with_provider` using the existing authorized-input/cancellation pattern. Prefer a completed tool call over text JSON; accept text JSON only when no tool call completed.

- [ ] **Step 4: Verify agent suites pass**

Run: `rtk cargo test -p rollshot-agent visual_annotation::tests && rtk cargo test -p rollshot-agent --test provider_contract`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-agent/src/visual_annotation.rs crates/rollshot-agent/src/driver.rs crates/rollshot-agent/src/lib.rs crates/rollshot-agent/tests/provider_contract.rs
rtk git commit -m "feat(agent): add visual annotation suggestion run"
```

### Task 3: Encode and authorize the image only after consent

**Files:**
- Create: `crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Test: inline tests in `visual_annotation_agent.rs`

**Interfaces:**
- Consumes `VisualAnnotationTaskInput { run_id, step, document_state_id, image }` after consent.
- Produces `VisualAnnotationTaskResult::{Proposal, NoSuggestion}`.
- Converts normalized agent results into `VisualAnnotationProposal` using actual image dimensions.

- [ ] **Step 1: Write failing conversion tests**

```rust
let proposal = suggestion_batch_to_proposal(
    7, &step(), 12, 400, 200, agent_batch()
).expect("proposal");
assert_eq!(proposal.suggestions.len(), 3);
assert_eq!(proposal.suggestions[0].base.image_width, 400);
```

Test `encode_visual_annotation_attachment` so descriptor byte count equals the PNG vector length and dimensions equal source dimensions. Test that `VisualSuggestionConsent` contains no `RgbaImage`, `Vec<u8>`, or `ModelAttachment`.

- [ ] **Step 2: Verify conversion tests fail**

Run: `rtk cargo test -p rollshot-app --features action-guide visual_annotation_agent::tests::normalized_agent_batch_becomes_valid_core_proposal`

Expected: FAIL.

- [ ] **Step 3: Implement the post-consent task**

Move only the safe PNG encoding/`AuthorizedModelInput` logic from `callout_agent.rs`. Build a prompt identifying the image as visual source of truth, invoke the new runner, scale normalized values, then call `VisualAnnotationProposal::from_agent_drafts`. Return a sanitized `NoSuggestion` for encode, protocol, or core-validation failure.

Add:

```rust
pub(crate) struct VisualSuggestionConsent {
    pub source: rollshot_action::CandidateId,
    pub keyframe: rollshot_action::FrameId,
    pub provider: String,
    pub model: String,
}
```

- [ ] **Step 4: Verify app-task tests pass**

Run: `rtk cargo test -p rollshot-app --features action-guide visual_annotation_agent::tests`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/visual_annotation_agent.rs crates/rollshot-app/src/timeline_workspace/mod.rs
rtk git commit -m "feat(action): prepare visual annotation suggestions"
```

### Task 4: Replace callout state with consent and review lifecycle

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/{mod,update}.rs`
- Test: inline tests in `mod.rs` and `update.rs`

**Interfaces:**
- Produces a single visual suggestion state used by Tasks 5–6.
- Enforces run-id matching and invalidates a review when the step/keyframe/document changes.

- [ ] **Step 1: Write failing lifecycle tests**

Assert these transitions:

```text
SuggestVisualAnnotationsRequested -> ConsentPending
VisualSuggestionConsentCancelled  -> Idle
VisualSuggestionConsentConfirmed  -> Running(run_id)
matching VisualAnnotationProposalLoaded -> PendingReview
older VisualAnnotationProposalLoaded -> Running(current_run_id)
```

Also prove `DeleteStep`, `ReplaceKeyframe`, successful manual annotation, undo, and redo discard a pending review. Prove cancellation starts no `Task::perform` branch. Add tests where provider/model configuration changes between request and confirmation: the state stays `ConsentPending`, no task starts, and the consent copy updates only after the user explicitly continues again.

- [ ] **Step 2: Verify lifecycle test fails**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::update::tests::visual_consent_cancel_keeps_workspace_idle`

Expected: FAIL.

- [ ] **Step 3: Implement state and messages**

Replace `CalloutSuggestionState` with:

```rust
pub(crate) enum VisualAnnotationSuggestionState {
    Idle,
    ConsentPending(VisualSuggestionConsent),
    Running { run_id: u64, cancellation: RunCancellation },
    PendingReview(rollshot_action::VisualAnnotationProposal),
    NoSuggestion { reason: Option<String> },
    Failed { message: String },
}
```

Add request, consent-cancel, consent-confirm, loaded-with-run-id, per-item accept/reject, accept-all/reject-all, and dismiss messages. On request, load configuration through the existing `load_provider_config` and `has_key` boundary before opening consent; store only displayed provider/model names, never the adapter, key, or image. On confirm, reload and compare provider/model configuration before re-fetching the source image and document state, incrementing the run id, building the adapter through `build_adapter`, and launching Task 3. If either value changed, remain in `ConsentPending`, show “Provider configuration changed. Review the consent again.”, and do not encode or send pixels. Reject late results. On state-changing manual actions, dismiss the review and display “Annotation suggestions are stale; regenerate them.” Update the existing state-machine doc-comment in `mod.rs` to show `Idle -> ConsentPending -> Running -> PendingReview`, cancellation, failure, and stale-invalidating transitions.

- [ ] **Step 4: Verify lifecycle tests pass**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::update::tests::visual_`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/update.rs
rtk git commit -m "feat(action): gate visual suggestions with consent"
```

### Task 5: Ghost, review, and atomically apply proposals

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/{annotation,update,view}.rs`
- Test: inline tests in all three modules

**Interfaces:**
- Consumes `VisualAnnotationProposal::pending_edit_ops` and existing `ImageDocument::apply_batch`.
- Produces non-mutating `Annotation` ghosts and individual/batch review controls.

- [ ] **Step 1: Write failing ghost/apply/view tests**

```rust
let ghosts = proposal_ghosts(&proposal()).expect("pending");
assert_eq!(ghosts.len(), 3);
assert!(matches!(ghosts[0], Annotation::NumberCallout { .. }));
```

Test one accepted text note adds one annotation, rejected item cannot reapply, accept-all adds all three with exactly one `state_id` increment, and stale accept-all changes neither count nor state. After one successful individual accept, assert the remaining pending items have their base `document_state_id` rebased to the new state and can be accepted; after any manual edit, assert they instead become stale. Test the view has “Suggest annotations,” confidence, rationale, per-item buttons, Accept all, Reject all, Dismiss, and the approved consent copy.

- [ ] **Step 2: Verify ghost test fails**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::annotation::tests::pending_visual_proposal_projects_all_three_ghost_primitives`

Expected: FAIL.

- [ ] **Step 3: Implement projection and UI**

Change `NumberAnnotationCanvas.suggested` from one optional annotation to `&[Annotation]` and render every ghost at reduced alpha. Use local-only ghost ids. For individual accept call `apply_batch(vec![op])`, mark that item accepted only after success, then rebase only the remaining pending items to the returned document state id. For Accept all call `apply_batch(all_ops)` once. Do not rebase after manual edit, undo, redo, deletion, or keyframe replacement.

Rename “Suggest Callout” to “Suggest annotations.” Add the consent modal with:

```text
Rollshot will send this one reviewed keyframe to {provider} using {model} to suggest callouts, notes, or redactions. Review every suggestion before it changes your guide. Original keyframes and Issue Packs may still contain unredacted evidence.
```

Show a scrollable pending-review list in the annotation modal. Disable manual mutation only while `Running`.

- [ ] **Step 4: Verify UI and application tests pass**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::annotation::tests && rtk cargo test -p rollshot-app --features action-guide timeline_workspace::view::tests`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/annotation.rs crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "feat(action): review visual annotation proposals"
```

### Task 6: Remove the callout-only path and prove regressions

**Files:**
- Delete: `crates/rollshot-action/src/callout_proposal.rs`
- Delete: `crates/rollshot-agent/src/callout.rs`
- Delete: `crates/rollshot-app/src/timeline_workspace/callout_agent.rs`
- Modify: affected `lib.rs`, `mod.rs`, `annotation.rs`, `update.rs`, and `view.rs` imports/exports
- Test: existing Storyboard, Issue Pack, and workspace inline modules

- [ ] **Step 1: Write failing export and privacy regressions**

Add a test that accepts a visual note/redaction, renders Storyboard, and asserts:

```rust
assert_ne!(rendered.image.as_raw(), original.as_raw());
assert_eq!(store.retained(step.keyframe).unwrap().image.as_raw(), original.as_raw());
```

Serialize Guide/Issue Pack output after acceptance and assert it contains no provider/model, run id, prompt, rationale, or attachment marker. Verify `SuggestCaptionsRequested` does not enter `ConsentPending` and constructs no attachment.

- [ ] **Step 2: Verify regression test fails**

Run: `rtk cargo test -p rollshot-app --features action-guide timeline_workspace::tests::accepted_visual_annotations_flatten_only_storyboard`

Expected: FAIL until migration reaches the shared visual pipeline.

- [ ] **Step 3: Delete old path and finish imports**

Delete callout-only proposal, agent profile, and app task only after every call site uses the visual path. Remove obsolete messages and `allow(dead_code)` markers. Do not alter `snapshot_storyboard` flattening, original keyframe inclusion, or reviewed-evidence warning copy.

- [ ] **Step 4: Run complete verification**

```bash
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-agent --test provider_contract
rtk cargo test --workspace --features rollshot-app/action-guide
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk cargo clippy --workspace --all-targets --features rollshot-app/action-guide -- -D warnings
```

Expected: all exit 0; no callout-only module references; export/privacy tests pass.

Manual smoke on each supported desktop platform: run `rtk cargo run -p rollshot-app --features action-guide -- action-guide`, record a short workflow, open **Annotate Step**, request visual suggestions, confirm the displayed provider/model, accept one item then Accept all, export a Storyboard, and verify the original retained keyframe is unchanged. The smoke uses a deliberately non-sensitive local test window and is not part of CI.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-action crates/rollshot-agent crates/rollshot-app
rtk git commit -m "refactor(action): unify visual suggestion flow"
```

## Plan Self-Review

- **Spec coverage:** Tasks 1–2 provide validated, bounded, provider-independent proposals; Task 3 guarantees post-consent image handling; Task 4 gives stale-safe lifecycle; Task 5 supplies review and atomic application; Task 6 verifies exports, captions, privacy, and old-path removal.
- **Placeholder scan:** Every task names exact files, interfaces, assertions, commands, and commits.
- **Type consistency:** `VisualAnnotationProposal` is created in Task 1, produced by Task 3, stored in Task 4, rendered/applied in Task 5, and is the sole path after Task 6.

## Engineering Review Addendum

### What already exists

- `rollshot-action::CalloutProposal` already validates a one-callout snapshot and stale state; Task 1 generalizes that behavior instead of introducing a second annotation model.
- `rollshot-image-document::ImageDocument::apply_batch` already gives one-history-entry atomic application; Task 5 reuses it for Accept all.
- `rollshot-agent::AuthorizedModelInput` already bounds and redacts one image attachment; Tasks 2–3 retain that authorization boundary.
- Timeline Workspace already has caption proposal review, callout ghosts, run-id protection, manual annotation tools, and flattened Storyboard input; Tasks 3–6 replace only the callout-specific path.
- `result_workspace::workbench::{load_provider_config,has_key,build_adapter}` already centralizes provider configuration; Tasks 3–4 reuse it and add no configuration store.

### NOT in scope

- Multi-step or whole-Guide image analysis: one selected keyframe keeps consent, cost, and stale detection understandable.
- Local-model installation and distribution: this plan consumes the existing provider configuration and publishes no new artifact.
- Automatic redaction or export safety certification: original Issue Pack keyframes remain reviewed evidence.
- New annotation primitives or a separate annotation renderer: existing `ImageDocument` operations are sufficient.
- Real-provider CI: deterministic fakes exercise protocol and authorization; one manual smoke run is required before release.

### Test coverage

| Task / behavior | Unit | Integration | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| Task 1 / validation, staleness, EditOp lowering | ✓ | — | — | no |
| Task 2 / schema, budget, provider terminal mapping | ✓ | ✓ | — | no |
| Task 3 / attachment byte/dimension authorization and conversion | ✓ | — | — | no |
| Task 4 / consent, config-change TOCTOU, run-id race, invalidation | ✓ | — | — | no |
| Task 5 / ghost rendering, individual rebase, atomic batch apply | ✓ | ✓ | — | no |
| Task 6 / Storyboard flattening and export privacy | ✓ | ✓ | ✓ synthetic | no |
| Release candidate / configured cloud provider consent copy and one visual run | — | — | — | yes |

### Failure modes

| New path | Failure | Test / handling | User outcome |
|---|---|---|---|
| Consent | Provider or model changes after review | Task 4 Step 1; remain `ConsentPending` without a task | “Provider configuration changed. Review the consent again.” |
| Image authorization | PNG encode or descriptor validation fails | Task 3 Step 1; `NoSuggestion` sanitizes failure | Recoverable suggestion failure; no proposal applies |
| Provider run | Timeout, cancellation, budget, protocol failure | Task 2 Step 1; terminal mapping in Task 2 Step 3 | No-suggestion/failure state with retry |
| Async delivery | Older run completes after a newer run | Task 4 Step 1; run-id match | Late result is ignored |
| Proposal review | Step/keyframe/document changes | Tasks 1 and 4; stale validation/dismissal | “Annotation suggestions are stale; regenerate them.” |
| Batch mutation | One operation is invalid | Task 5 Step 1; one `apply_batch` call | No annotations are changed |
| Export | Accepted annotations leak into original evidence or metadata | Task 6 Step 1 | Storyboard is flattened; original keyframe and exports remain unchanged |

No listed failure mode is silent or lacks both a test and a user-visible outcome.

### Data flow diagram

```text
selected reviewed step
        |
        v
provider config check -> ConsentPending --Cancel--> Idle
        |                       |
        | config unchanged      | Continue
        v                       v
   source PNG encode -> AuthorizedModelInput -> bounded provider run
                                             |
                                             v
                                  validated VisualAnnotationProposal
                                             |
                              +--------------+--------------+
                              |                             |
                        Accept one                     Accept all
                              |                             |
                   apply_batch([op])              apply_batch(all_ops)
                   rebase remaining items                 |
                              +-------------+---------------+
                                            v
                                  ImageDocument -> Storyboard flatten
```

### Worktree / subagent parallelization strategy

| Task | Modules touched | Depends on |
|---|---|---|
| Task 1 | `crates/rollshot-action/` | — |
| Task 2 | `crates/rollshot-agent/` | — |
| Task 3 | `crates/rollshot-app/timeline_workspace/` | Tasks 1, 2 |
| Task 4 | `crates/rollshot-app/timeline_workspace/` | Task 3 |
| Task 5 | `crates/rollshot-app/timeline_workspace/` | Task 4 |
| Task 6 | all three Action Guide modules | Tasks 1–5 |

Launch Tasks 1 and 2 in separate worktrees in parallel. Merge both, then execute Tasks 3 → 4 → 5 → 6 sequentially because each modifies the same Timeline Workspace state and UI. Task 6 is the integration and deletion gate; it must not run in parallel with any earlier task. No workspace-root task is planned.
