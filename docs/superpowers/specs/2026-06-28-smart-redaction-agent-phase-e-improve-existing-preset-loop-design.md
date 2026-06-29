# Smart Redaction Agent Phase E Improve Existing Preset Loop

**Date:** 2026-06-28
**Status:** Draft for user review

## Goal

Close the loop after a generated preset misses content or overfires. When the
user rejects false positives, resizes candidates, or manually adds missing
redactions, Rollshot should feed that correction evidence into a new bounded
agent run against the active preset source, then save the accepted detector as an
immutable child revision.

## Scope

Phase E builds the first usable improve flow:

1. Convert candidate review state into structured, privacy-safe correction
   evidence.
2. Wire `AskAgentToRevise` to start `RunKind::Improve` from the active revision
   and the correction evidence.
3. Teach the agent prompt that improve runs must preserve working detector
   behavior while fixing the reviewed misses/overfires.
4. Save accepted drafts as child revisions of the active revision with clear
   provenance.
5. Add focused regression coverage for one missed-target correction and one
   overfire correction using deterministic app tests.

The phase does not redesign preset management, add template-handle persistence,
or make agent output automatically trusted. Every revision still requires manual
review before it becomes active.

## Current Code Shape

The workbench already has most of the plumbing:

- `crates/rollshot-app/src/result_workspace/workbench/mod.rs` defines
  `RunKind::{Author, Improve}` and `WorkbenchMessage::AskAgentToRevise`.
- `crates/rollshot-app/src/result_workspace/update.rs` starts authoring runs
  through `PendingRunParams`, but `AskAgentToRevise` is currently a no-op.
- `crates/rollshot-app/src/result_workspace/workbench/review.rs` can apply
  reviewed candidates, save revisions, and assemble partial correction evidence.
- `crates/rollshot-preset/src/domain.rs` and `store.rs` already support
  immutable revisions with `parent_id`.
- `crates/rollshot-agent/src/driver.rs` owns the system prompt and authoring
  loop rules.

Reference reading from `learn-projects/claude-code-source-code` reinforces the
shape: agent harnesses should make session/tool-result state explicit and feed
user feedback back as structured input, not as hidden UI state. Phase E follows
that pattern by turning review corrections into a compact user-visible improve
instruction.

## Design

### Correction Evidence Model

Add a small workbench-local evidence model in `workbench/review.rs`. It should
not live in `rollshot-edit-proposal` yet because this is product-agent context,
not the framework-neutral proposal contract.

Evidence contains:

- rejected candidates: candidate id, label, original bounds;
- resized candidates: candidate id, label, original bounds, corrected bounds;
- manually added candidates: candidate id and bounds;
- counts for accepted, rejected, resized, and added candidates.

Bounds are image-space rectangles. Labels are existing short candidate labels.
No screenshot bytes, OCR text beyond existing labels, prompts, provider output,
or raw transcripts are stored in evidence.

Manual additions should be identified by `ProvenanceSource::Manual`, not by
candidate id ranges. This avoids relying on the current `next_manual_candidate_id`
implementation detail.

### Improve Run Input

Extend `PendingRunParams` with an optional correction-evidence summary string or
small struct. `RunKind::Author` leaves it empty. `RunKind::Improve` requires:

- active revision source;
- active revision id;
- pending proposal;
- non-empty correction evidence.

The workbench should build the improve user message itself instead of relying on
freeform composer text. The message should be concise and deterministic:

```text
Improve the current Smart Redaction detector using this reviewed evidence.
Preserve existing useful detections, remove overfires, and add missed targets.

Correction evidence:
...
```

This text is sent through the existing `AuthorizedModelInput.user_message` path,
so provider adapters and transcript/cassette recording keep the same shape.

### Agent Prompt Contract

Update the smart-redaction system prompt in `rollshot-agent` so the model knows
how improve runs differ from first authoring:

- start by reading the current source;
- treat rejected candidates as false positives;
- treat resized candidates as target geometry corrections;
- treat manual candidates as missed targets to add;
- preserve unrelated working behavior;
- explain what changed before `submit_for_review`.

No new tool is required for v1. The correction evidence rides in the user
message, and existing tools (`read_current_source`, `edit_source`,
`validate_source`, `dry_run`, `submit_for_review`) remain the whole editing
surface.

### Revision Save

When saving an improved draft, pass the active revision id as `parent_rev_id`.
Use existing immutable revision storage. The provenance remains
`RevisionOrigin::AgentRun`, with:

- `source_run_ref`: session id, as today;
- `note`: a compact string such as
  `improved from rev-123; 1 rejected, 1 resized, 1 manually added`.

First-time authoring runs continue to use `None` for `parent_rev_id`.

### UI Behavior

`AskAgentToRevise` is enabled only when:

- an active revision exists;
- a pending proposal exists;
- correction evidence is non-empty;
- no run is currently active.

The button starts the standard disclosure flow. The screenshot payload choices
remain unchanged. The composer is not required for improve runs; the workbench
generates the improve instruction from review state.

### Evaluation Protection

Phase E should not pause for live recording or for seeding all six Phase D
provider-backed fixtures. Instead:

- keep the existing Phase D selftest gate active;
- add focused unit/integration tests for correction evidence and improve-run
  parameter assembly;
- add deterministic miss and overfire tests that exercise improve evidence
  assembly without calling a live model.

This gives Phase E product coverage without turning it into a broad eval-seeding
project.

## Error Handling

- If `AskAgentToRevise` is invoked without an active revision, pending proposal,
  or non-empty evidence, do nothing and leave the workbench state unchanged.
- If revision saving cannot find a parent active revision, return the existing
  typed store/config error instead of saving an orphan improvement.
- If the model reaches `ReadyForReview`, candidates still enter normal review;
  no improved revision is activated until the user saves it.
- If the user discards candidates or the draft, correction evidence is discarded
  with the pending proposal.

## Testing

Focused tests should cover:

- correction evidence counts and bounds for rejected, resized, and manual
  candidates;
- manual additions detected by provenance;
- `AskAgentToRevise` assembles `RunKind::Improve` params with active source and
  correction evidence;
- save revision passes active revision id as parent for improve runs and `None`
  for first authoring;
- the agent system prompt mentions improve correction semantics and explanation
  before submit;
- default eval still passes, and OCR eval is run when touching OCR-gated
  fixtures.

Verification commands:

```bash
rtk cargo test -p rollshot-app workbench::review
rtk cargo test -p rollshot-app result_workspace::workbench::run
rtk cargo test -p rollshot-agent provider_contract
rtk cargo test -p rollshot-app eval
rtk cargo fmt --check
```

Run `rtk cargo test -p rollshot-app --features ocr eval` if Phase E adds or
seeds OCR-gated eval fixtures.

## Non-Goals

- No automatic acceptance of improved redactions.
- No new model provider behavior or separate agent runtime.
- No new durable correction-history database.
- No template-handle lifecycle or template matching.
- No broad preset-management UI.

## Follow-Up

- Seed provider-backed improve cassettes after the Phase E prompt and correction
  evidence format stabilize.
