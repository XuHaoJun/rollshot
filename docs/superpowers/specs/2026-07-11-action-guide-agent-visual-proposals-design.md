# Action Guide Agent Visual Proposals Design

## Purpose

Complete P5 of the Action Guide Storyboard umbrella PRD with agent-assisted, reviewable visual annotations. A user can ask Rollshot to inspect the currently selected, reviewed keyframe and receive zero or more proposed number callouts, text notes, and opaque redactions. Nothing changes the keyframe or annotation document until the user explicitly accepts it.

Caption suggestions remain a separate, guide-wide text-only flow: they do not send pixels to a provider and can update the title and caption of many steps.

## Current Baseline

The current product already has:

- `CaptionProposal` in `rollshot-action`, generated from guide metadata and reviewed per suggestion or in bulk in Timeline Workspace.
- A selected-step `CalloutProposal`, backed by one authorized PNG attachment, bounded agent execution, a ghost overlay, and explicit accept/reject.
- Per-step `ImageDocument` state in Timeline Workspace, supporting manual number callouts, text notes, opaque redactions, undo/redo, and flattened Storyboard rendering.
- `AuthorizedModelInput` and `ModelAttachment`, which enforce attachment count, dimensions, byte counts, and debug redaction before a provider sees a visual attachment.

The gaps are that visual suggestions support only one callout, use a parallel proposal lifecycle, and do not provide a consent boundary or review model that scales to all existing annotation primitives.

## Scope

### In scope

- One selected Action Guide step per visual-suggestion run.
- Agent proposals for `NumberCallout`, `TextNote`, and `OpaqueRedaction`.
- A single Rollshot-owned proposal contract, validation path, stale detection, provenance, and review UI for all three primitives.
- Explicit informed consent immediately before a run sends the selected reviewed keyframe to the configured cloud provider.
- User controls to accept or reject each valid suggestion, and accept all valid suggestions in a run.
- Reusing existing `ImageDocument` operations and flattening behavior.
- Transitioning the current callout suggestion flow onto the common contract without changing caption suggestion semantics.

### Out of scope

- Automatic application, automatic publication, or automatic redaction.
- Sending more than one keyframe in a visual-suggestion run.
- Guide-wide or cross-step visual planning.
- Persisting provider prompts, provider responses, or image attachment bytes in Action Guide exports.
- Claiming that an Issue Pack is redacted: it may still contain original, reviewed keyframes.
- New annotation primitives, hosted collaboration, tracker integrations, or a local-model runtime.

## Product Experience

1. In the selected-step panel, the user chooses **Suggest annotations**.
2. A confirmation dialog identifies the configured provider/model and says that this single reviewed keyframe will be sent for analysis. It names the three possible suggestion types and makes no implied promise of full redaction.
3. **Cancel** sends nothing. **Continue** starts one bounded run and disables conflicting annotation mutations while it is running.
4. The annotation modal displays ghost projections and a compact review list. Each row shows primitive type, confidence, optional rationale, and **Accept** / **Reject**. A valid pending batch also exposes **Accept all** and **Reject all**.
5. Accepted operations enter the existing per-step `ImageDocument`, becoming normal user-editable annotations with its existing undo/redo behavior. Rejected items disappear; neither result is exported as agent provenance.
6. If the selected step or annotation state changes while the run is pending, stale suggestions are visibly unavailable and cannot apply. The user may start a fresh run after reviewing the changed image.

Existing **Suggest Callout** becomes **Suggest annotations**. Caption controls and caption review remain separate because their input and privacy model are different.

## Architecture

### Core proposal contract

`rollshot-action` owns a new visual proposal domain model rather than exposing agent or provider types. Its `VisualAnnotationProposal` represents one bounded run and contains:

- a proposal/run identifier and `Agent { run_id }` provenance;
- an immutable step identity (`CandidateId`) and base `FrameId`;
- the annotation document's base state identifier;
- source image dimensions used for coordinate validation;
- a sequence of uniquely identified pending suggestions;
- confidence and optional rationale per suggestion;
- a validated, typed visual edit payload.

The payload is an Action Guide-owned enum with variants for number callout, text note, and opaque redaction. It converts to the existing `rollshot_image_document::EditOp`; no duplicate rasterizer, annotation graph, or export format is introduced.

The current `CalloutProposal` migrates to this model through a compatibility adapter during the transition, then its specialized state and agent task are removed once all callers use the common visual contract. This preserves the existing provider budget and public behavior of callout suggestions while eliminating a permanently separate lifecycle.

### Validation and staleness

Construction validates all untrusted agent output before it reaches the UI:

- primitive payload shape and finite coordinates;
- confidence in `0.0..=1.0`;
- non-empty, bounded user-visible text;
- non-empty, in-bounds redaction rectangles;
- callout tip and bubble coordinates in image bounds;
- unique suggestion identifiers and a bounded suggestion count;
- image dimensions that match the authorized attachment metadata.

Applying a proposal checks a fresh workspace snapshot, not only its original construction inputs. A suggestion is stale and cannot apply if its step was deleted, its source/keyframe does not match, the image dimensions differ, or the per-step `ImageDocument` state identifier changed. Accept-all applies in stable order only while every remaining item passes the same check; otherwise it reports staleness and applies none of the batch.

Replacing a keyframe clears the step annotation document under the existing P4 rule and invalidates all pending visual suggestions for that step. Deleting a step also discards its pending proposal. Late async results retain the existing monotonic run-id guard.

### Agent boundary

`rollshot-agent` receives one `AuthorizedModelInput` attachment: a PNG of the selected retained keyframe. The existing byte-count, dimension, attachment count, and debug-redaction checks remain mandatory. A new bounded visual annotation task profile replaces the callout-specific prompt/tool schema and returns a Rollshot-owned terminal result with no provider payload or attachment bytes.

The prompt may use the selected step's reviewed title, caption, kind, and reason as context, but must treat the attached image as the visual source of truth. The structured tool output returns zero or more primitive proposals in normalized image coordinates. The app converts those coordinates to image space, invokes core validation, and surfaces only valid suggestions.

One run has one attachment, a fixed short wall-clock deadline, bounded model turns/tokens/tool calls/result bytes, and no retained raw provider response.

### Workspace state and UI

Timeline Workspace replaces `CalloutSuggestionState` with a visual-suggestion state machine:

```text
Idle -> ConsentPending -> Running -> PendingReview
                         |            | accept/reject/dismiss -> Idle
                         +-> NoSuggestion / Failed -> Idle
```

`ConsentPending` holds only selected-step identity and provider/model display data; it has no image bytes. The PNG is encoded only after the user presses **Continue**. `Running` owns cancellation and the run id. `PendingReview` holds the validated proposal and drives both ghost rendering and list actions.

The annotation modal remains the single editing surface. Manual tools stay available before consent and after review; they are disabled only while a run is active. Any manual edit after the proposal is produced causes the proposal to show as stale rather than silently rebasing it.

## Privacy and Safety Copy

The confirmation dialog states, in direct product language:

> Rollshot will send this one reviewed keyframe to {provider} using {model} to suggest callouts, notes, or redactions. Review every suggestion before it changes your guide. Original keyframes and Issue Packs may still contain unredacted evidence.

The workspace never sends a visual attachment for **Suggest Captions**. Neither consent, provider responses, image bytes, prompts, rationale, nor run identifiers are included in Storyboard, Guide, or Issue Pack outputs. Runtime diagnostics use existing stable `rollshot::*` targets and structured metadata only; they never log attachment bytes or prompt content.

## Failure Handling

- No selected step or no retained keyframe: do not open consent; show a local, recoverable message.
- User cancels consent: return to Idle without encoding or sending pixels.
- Provider, protocol, budget, timeout, or validation failure: discard the untrusted output and show a sanitized recoverable message.
- The model returns no useful items: show a non-error no-suggestion state.
- Any stale item: never apply it; explain that the step or annotations changed and require a fresh run.
- A partial invalid model response is rejected as a whole run rather than presenting an ambiguous subset.

## Verification Strategy

Unit tests in `rollshot-action` cover construction, type/range/text/bounds validation, conversion to each `EditOp`, unique ids, provenance, all stale conditions, atomic accept-all, and adapter parity for the old callout flow.

`rollshot-agent` contract tests cover the structured tool schema, budget, single-attachment authorization, normalized-coordinate decoding, provider terminal mapping, cancellation, timeout, and byte/prompt privacy boundaries.

Timeline Workspace tests cover consent cancel/continue, no bytes before continue, run-id race rejection, each per-item review action, ghost projection, manual-edit and keyframe-replacement invalidation, deleting the step, and unchanged caption-only behavior. End-to-end headless tests confirm accepted operations appear in flattened Storyboard output while original keyframes remain untouched. Run the focused package tests plus workspace formatting and clippy; the agent changes additionally run `provider_contract` and existing cancellation/privacy suites.

## Acceptance Criteria

- Users explicitly approve sending exactly one selected, reviewed keyframe before any visual agent run.
- The agent may propose callouts, text notes, and opaque redactions, all of which are validated before review.
- Users can accept/reject individual proposals and atomically accept/reject a valid batch; no suggestion applies automatically.
- Accepted proposals become normal editable `ImageDocument` annotations and appear only in flattened preview/export output; retained source keyframes are unchanged.
- Caption proposals stay guide-wide, text-only, and do not trigger visual consent or image transmission.
- A deleted/replaced step or changed annotation document makes visual suggestions stale and impossible to apply.
- Provider bytes, prompts, consent, and provenance are absent from exports and diagnostic payloads.
- Existing manual annotation, caption proposal, Storyboard, and Issue Pack behavior remains intact.

