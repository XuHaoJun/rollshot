# Action Guide Agent Callout Design

**Date:** 2026-07-10
**Status:** Approved design
**Scope:** Selected-step agent suggestion for one Number Callout

## Goal

Let a user select one reviewed Action Guide step and ask Rollshot to suggest the single most important Number Callout target. The agent sees that step's original keyframe and presentation metadata, but its result remains a reviewable ghost annotation until the user accepts it.

This phase completes the visual-proposal portion of the Action Guide Storyboard umbrella direction without adding Text Note or Opaque Redaction suggestions.

## Product Decisions

- `Suggest Callout` applies only to the currently selected step.
- A run returns at most one Number Callout suggestion.
- The agent chooses only the callout tip. Rollshot places the numbered bubble deterministically.
- The agent infers the target from the step title, caption, kind, and original keyframe. There is no user prompt field.
- A low-confidence run may return no suggestion. It must not invent a target merely to complete the request.
- The existing annotation modal owns loading, ghost preview, accept, reject, retry, and cancellation.
- Text Note and Opaque Redaction suggestions are out of scope.

## Non-Goals

- Suggesting callouts for every Guide step in one run.
- Returning multiple callouts for one step.
- Agent-controlled bubble placement or annotation numbering.
- Prompt-driven annotation.
- URL, file-ID, video, PDF, or general-purpose media input.
- Replacing Rollshot's provider abstraction with Rig providers.
- Exposing Rig types through Rollshot public APIs.
- Automatically applying agent output to an `ImageDocument`.

## Architecture

```text
Selected Guide Step
  title + caption + kind
  source + keyframe
  annotation document state_id
  original keyframe PNG
          |
          v
AuthorizedModelInput
  exactly one PNG/JPEG attachment
  existing byte and dimension limits
          |
          v
Rollshot ModelRequest
  provider-neutral ModelAttachment
          |
          +--> Anthropic adapter --> provider image source block
          |
          +--> OpenAI adapter -----> provider image content
          |
          v
Rig AgentRun
  sans-I/O turn and tool-call state only
          |
          v
submit_callout_suggestion
  suggestion { tip, confidence, rationale }
  or no_suggestion { reason }
          |
          v
Callout proposal validation
          |
          v
Annotation modal ghost preview
          |
     Accept | Reject
          |
          v
ImageDocument::add_number_callout
```

Ownership remains explicit:

- `rollshot-agent` owns the multimodal request contract, provider conversion, bounded run, tool submission, budgets, cancellation, and privacy-safe diagnostics.
- `rollshot-action` owns the step-bound callout proposal, provenance, status transitions, and staleness checks that depend on Guide identity.
- `rollshot-app` owns selected-step orchestration and annotation-modal interaction.
- `rollshot-image-document` remains the only source of truth for committed annotations and history.
- Rig remains an internal sans-I/O turn state machine. Rollshot does not adopt Rig provider types or provider implementations.

## Provider-Neutral Image Input

Extend Rollshot's `ModelRequest` with attachments that can only be constructed from already-authorized input. The minimal logical contract is:

```rust
pub struct ModelAttachment {
    media_type: MediaType,
    width: u32,
    height: u32,
    bytes: Arc<[u8]>,
}
```

`ModelAttachment` has private fields, a crate-private constructor used only by the authorized-input conversion, and crate-private metadata/byte accessors for the in-crate provider adapters. These invariants are required:

- A callout run passes exactly one attachment.
- Only PNG and JPEG are accepted.
- Existing attachment-count, per-attachment, and total-byte limits remain authoritative.
- `Debug` output contains metadata only and never includes attachment bytes.
- Text-only requests carry no attachments and preserve current caption behavior.
- Anthropic and OpenAI adapters convert the provider-neutral attachment into their native image payloads.
- Provider adapters must report unsupported image input as a recoverable provider/model capability failure. The app must not fall back to a text-only coordinate guess.

Rig 0.39.0 image messages and provider conversions are reference implementations for media representation and payload mapping. Rollshot retains its own public contract, streaming adapters, authorization, and privacy rules.

## Bounded Agent Run

Generalize only the two task-specific parts currently fixed in the bounded runner:

1. The system prompt.
2. The terminal submission contract.

The callout task profile has these limits:

- At most two model turns.
- Exactly one authorized keyframe attachment charged against the run budget.
- Only the `submit_callout_suggestion` terminal tool is advertised.
- No automation authoring, OCR, template, edit-proposal, or image-inspection tools are available.
- A terminal submission is either `suggestion` or `no_suggestion`.
- Duplicate terminal submission, an unknown tool, malformed arguments, missing terminal submission, or exhausted turn budget produces an explicit terminal failure.
- Rig `AgentRun` continues to manage model/tool history, call IDs, and turn sequencing.

The terminal tool uses a tagged JSON object with `additionalProperties: false`. Its accepted payloads are:

```text
suggestion {
  tip: { x: number, y: number },
  confidence: number,
  rationale: string | null
}

no_suggestion {
  reason: string | null
}
```

The system prompt instructs the model to identify the single most important visual target implied by the step metadata, prefer no suggestion when the evidence is ambiguous, and never return bubble coordinates.

## Proposal Model

The proposal captures the precise base state used by the model:

```rust
CalloutSuggestionBase {
    step_source: CandidateId,
    keyframe: FrameId,
    document_state_id: u64,
    image_width: u32,
    image_height: u32,
}

CalloutSuggestion {
    id: CalloutSuggestionId,
    base: CalloutSuggestionBase,
    tip: ImagePoint,
    confidence: f32,
    rationale: Option<String>,
    provenance: Agent { run_id: u64 },
    status: Pending | Accepted | Rejected | Stale,
}
```

The implementation may use existing Rollshot identifier and provenance types where they preserve these semantics. It must not create a second committed annotation representation.

Proposal construction validates:

- `x`, `y`, and confidence are finite.
- The tip is inside the original image bounds.
- Confidence is within `0.0..=1.0`; malformed out-of-range tool payloads are rejected instead of silently normalized.
- Rationale and no-suggestion reason are trimmed and bounded in length.
- Only one suggestion is retained for a run.

`no_suggestion` is a successful run result with no proposal. It is not an agent or provider failure.

## Staleness and Acceptance

Acceptance rechecks all base-state invariants:

- The selected Guide step still exists.
- Its stable `source` matches the proposal.
- Its keyframe has not been replaced.
- The step annotation document still has the captured `state_id()`.
- Image dimensions still match.
- The proposal status is `Pending`.

Any mismatch marks the proposal `Stale`, removes the ghost, and leaves the document unchanged. The user may request a fresh suggestion.

On a valid acceptance:

1. Compute the bubble position deterministically from the tip, image bounds, and current committed annotations.
2. Use the document's current `next_number()` when rendering and committing the annotation.
3. Call `ImageDocument::add_number_callout(tip, bubble)` exactly once.
4. Mark the proposal `Accepted`.

The document operation creates one undo entry. Undoing the annotation does not make the old proposal pending again. Reject marks a pending proposal `Rejected` and never mutates the document.

## Deterministic Bubble Placement

The agent never controls the bubble. Rollshot uses a pure placement function:

1. Generate candidates at a fixed image-space offset in this order: upper-right, upper-left, lower-right, lower-left.
2. Reject candidates whose bubble bounds leave the image.
3. Score remaining candidates by overlap with a protected region around the tip and with bounds of existing committed annotations.
4. Choose the lowest-overlap candidate, using the fixed order as the tie-breaker.
5. If every candidate leaves the image, clamp the first candidate to the image bounds.

The offset and bubble radius use the same visual tokens as committed Number Callouts. Identical inputs must produce identical output.

## Annotation Modal UX

The selected-step panel exposes `Suggest Callout`. Activating it opens the existing annotation modal and starts the run.

```text
Suggest Callout
      |
      v
Annotation modal: loading + Cancel
      |
      +-- suggestion ----> ghost callout + Accept / Reject
      +-- no suggestion -> explanation + Retry / Close
      +-- failure -------> recoverable error + Retry / Close
```

Interaction rules:

- Annotation-mutating controls are disabled while the suggestion run is active.
- Cancel and modal close use the run's single `RunCancellation` and leave no proposal behind.
- The ghost uses a visually distinct suggested state and does not reserve an annotation number.
- Accept commits the next available number at that moment.
- Reject removes the ghost but keeps the modal open so the user may annotate manually.
- Closing the modal discards a pending proposal.
- Existing manual Number Callout, Text Note, Opaque Redaction, undo, and redo behavior remains unchanged outside the active run.

The Timeline Workspace and annotation modal are shared by the active Linux and macOS product paths. This design introduces no platform-specific behavior.

## Error Handling

- Missing provider credentials reuse the existing recoverable provider-setup message pattern.
- Provider or model image incompatibility is recoverable and never degrades to text-only guessing.
- Timeout, cancellation, provider failure, protocol failure, and budget exhaustion create no proposal.
- Invalid or out-of-bounds agent coordinates fail validation; Rollshot does not clamp the tip.
- `no_suggestion` displays a neutral message such as `No clear callout target found.`
- A stale acceptance removes the ghost and asks the user to regenerate.
- Runtime diagnostics use stable `rollshot::agent::*` and `rollshot::app::*` targets with structured fields.
- Diagnostics and `Debug` output must not contain attachment bytes, prompts, rationale text, raw tool arguments, API keys, or provider payloads.

## Testing

### `rollshot-agent`

- Authorized single-image construction and rejection of unsupported/count/size inputs.
- `ModelRequest` and event debug output redact attachment content.
- Anthropic and OpenAI request fixtures encode PNG and JPEG correctly.
- Text-only provider fixtures remain unchanged.
- Rig turn/tool threading reaches suggestion, no-suggestion, and protocol-failure terminals.
- Attachment, model-turn, token, wall-time, and cancellation budgets are enforced.
- Provider contract and privacy suites cover the new path.

### `rollshot-action`

- Proposal construction accepts one valid tip.
- Non-finite, out-of-bounds, and invalid-confidence payloads are rejected.
- Accept, reject, not-pending, missing-step, replaced-keyframe, and stale-document transitions are deterministic.
- Agent provenance is retained.

### `rollshot-image-document`

- Bubble placement covers each corner, edge clamping, overlap scoring, and deterministic tie-breaking.
- Accepting one suggestion creates exactly one undoable semantic edit.

### `rollshot-app`

- The action is available only with a selected step and configured provider.
- Loading disables mutation and cancel closes the run cleanly.
- Suggestion, no-suggestion, timeout, provider failure, and malformed response states are recoverable.
- Ghost preview does not mutate the document.
- Accept, reject, close, keyframe replacement, and document-state mutation enforce the staleness contract.
- Annotated Storyboard preview/export and Issue Pack behavior continue to use the committed flattened document only.

### Verification Commands

At minimum, the implementation plan must include:

```bash
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-action
rtk cargo test -p rollshot-image-document
rtk cargo test -p rollshot-app --features action-guide
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

At least one real-provider, vision-capable model smoke test is required on an available platform. Both platform builds share the same UI path, but runtime smoke testing on one platform does not prove the other platform's native capture launch behavior; capture launch is outside this feature's changed path.

## Acceptance Criteria

- A user can request one callout suggestion for the selected reviewed step.
- The configured vision-capable Anthropic or OpenAI model receives exactly the authorized keyframe plus step metadata.
- The agent returns at most one in-bounds tip or an explicit no-suggestion result.
- Rollshot, not the agent, places the bubble deterministically.
- The annotation modal shows a non-mutating ghost proposal.
- Accept creates one normal undoable Number Callout; reject creates none.
- Guide deletion, keyframe replacement, or a different current annotation `state_id` cannot cause a stale proposal to apply. Undo may make a proposal valid again only when it restores the exact captured `state_id`; redo then invalidates it again when the state differs.
- Original keyframe pixels remain unchanged.
- Existing caption suggestions and text-only provider requests continue to work.
- No attachment or sensitive provider content appears in diagnostics or debug output.
