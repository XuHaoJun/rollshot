# Smart Redaction Agent Phase A Stabilization

**Date:** 2026-06-27
**Status:** Draft for user review

## Goal

Make the current Smart Redaction agent harness reliably able to produce a
reviewable JavaScript preset draft for simple screenshot-redaction intents.

This phase does not try to finish the whole roadmap. It fixes the hard runtime
and authoring-context gaps that currently make the main agent feel nearly
unusable.

## Success Criteria

- A provider request for a Smart Redaction run includes a stable authoring guide
  covering the Rollshot JS subset, `rollshot.*` API, output shape, examples, and
  required validate/dry-run/submit loop.
- The model is not exposed to inspection tools that always return
  `capability_unavailable` in the product run.
- A preset that uses `rollshot.regionFeatures(...)` can dry-run through the
  workbench path without `vision_index_unavailable`.
- Existing-preset execution and agent dry-run use the same capability
  preparation rules for supported capabilities.
- Tool results give the model enough bounded feedback to distinguish "valid
  source with zero candidates" from "runtime failed" and "policy rejected".
- Tests cover the new prompt/tool contract and at least one prepared
  region-feature preset path.

## Scope

### In Scope

- `rollshot-agent` prompt and tool-description improvements.
- `rollshot-agent` tool result shape improvements where they are already part
  of the authoring loop.
- `rollshot-app` workbench registration of truthful tools only.
- `rollshot-app` workbench capability preparation before dry-run and existing
  preset execution.
- Focused tests in `rollshot-agent` and `rollshot-app`. Add `rollshot-vision`
  tests only if the implementation changes vision preparation helpers.

### Out of Scope

- A full patch/edit-source tool.
- Template-handle persistence.
- A preset marketplace or import/export format.
- New object detection models.
- Enabling OCR by default.
- A broad workbench UI redesign.

## Design

### 1. Authoring Guide

Add a stable Smart Redaction authoring guide to the model request. It should be
either part of the system prompt or a separate prompt section assembled by
`AgentRunner`.

The guide must include:

- Required source shape: exactly one synchronous `function main(input)`.
- Available input fields:
  - `input.imageWidth`
  - `input.imageHeight`
  - `input.region`
  - `input.annotations`
  - `input.capabilityHandles`
- Supported capabilities:
  - `rollshot.ocr(query)`
  - `rollshot.layout(query)` if available, with current limitation called out.
  - `rollshot.regionFeatures(query)`
  - `rollshot.templateMatch(query)` only when handles are available.
- Output shape:
  - `{ candidates: [...] }`
  - candidate kind `addRedaction`
  - `bounds`, `confidence`, `label`, optional `rationale`
- Examples for:
  - OCR matches to redactions.
  - Region-feature top strip redaction.
  - Returning zero candidates when a confident detector is absent.
- Recovery rules:
  - Run `validate_source`.
  - Run `dry_run`.
  - If validation fails, edit the source and retry.
  - If dry-run fails with capability unavailable, choose a supported capability
    or ask for input instead of submitting.
  - Submit only after current-generation validation and dry-run both succeed.

The prompt should be static and source-controlled. Avoid embedding screenshot
content, user text, raw OCR text, or attachment bytes in logs.

### 2. Truthful Tool Registry

The product workbench should only register tools that are actually useful in
that context.

For Phase A:

- Keep:
  - `replace_source`
  - `validate_source`
  - `dry_run`
  - `submit_for_review`
  - `request_user_input`
  - `inspect_context_summary`
- Do not register `inspect_ocr`, `inspect_layout`, or
  `inspect_region_features` unless they return real data from the current image.

The stubs may remain in tests or internal code if useful, but the model should
not see them in product Smart Redaction runs.

### 3. Capability Preparation

Introduce a small workbench-side preparation layer for supported capabilities.

Phase A should support region features first because it does not require OCR
model availability or template lifecycle design. Before executing QuickJS, the
workbench should prepare region features for a small set of canonical safe
regions that generated scripts are expected to use:

- full image only when it is under the existing region-feature area cap
- top strip derived from image dimensions
- left strip derived from image dimensions
- right strip derived from image dimensions
- bottom strip derived from image dimensions

The exact strip formulas should be deterministic and documented in code. The
authoring guide examples must use the same formulas, so a generated script that
follows the guide hits prepared regions exactly. Strip regions must stay under
the existing region-feature area cap.

Dry-run and existing-preset execution should both use this preparation layer.
This avoids one path succeeding while the other fails.

Do not silently prepare arbitrary regions requested by JavaScript during QuickJS
execution. The runtime contract stays prepare-then-cached-callback.

### 4. Dry-Run Feedback

Keep dry-run results bounded, but make them more diagnostic.

Extend the dry-run result with:

- candidate count
- affected area
- capability calls
- candidate preview for the first few candidates:
  - kind
  - bounds
  - confidence
  - label
- structured failure message for validation, runtime, and policy failures

If changing the public result struct is too broad, keep the existing fields and
improve error messages first. The implementation plan should choose the smaller
change after checking downstream test impact.

### 5. Prompt and Tool Contract Tests

Add tests that fail if the agent request regresses.

Required assertions:

- `run_model_turn_with_provider` sends the Smart Redaction system prompt.
- The prompt includes the authoring guide markers for source shape, output
  shape, capability API, and validate/dry-run/submit loop.
- The product registry includes only truthful tools for Phase A.
- Tool schemas remain valid JSON object schemas.

### 6. Workbench Runtime Tests

Add a focused workbench test using a small synthetic image and a preset source
that calls `rollshot.regionFeatures`.

The test should prove:

- validation succeeds,
- capability preparation happens before QuickJS,
- dry-run returns a proposal instead of `vision_index_unavailable`,
- existing-preset execution and agent dry-run share the same preparation
  behavior where practical.

If direct workbench testing is awkward, introduce a small helper function that
prepares the host and is unit-tested without spawning an iced task.

## Error Handling

- Validation failures remain recoverable tool errors so the model can edit and
  retry.
- Runtime failures caused by unsupported capabilities should be explicit and
  should not become empty successful proposals.
- Stale generation errors remain recoverable and should tell the model the
  expected generation.
- Missing provider configuration remains a workbench configuration error.

## Testing Commands

Use the narrow commands first:

```bash
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-app result_workspace::workbench
rtk cargo test -p rollshot-vision region_features
```

Before claiming the phase complete, run:

```bash
rtk cargo test
rtk cargo fmt --check
```

Use workspace clippy if the implementation touches shared contracts beyond the
files named in this spec.

## Risks

- Prompt changes can improve one provider and regress another. Provider-contract
  tests should check request shape, not model quality.
- Preparing too many regions can waste CPU and hide capability-lifecycle design
  problems. Phase A should prepare a small fixed set only.
- Returning too much dry-run detail can leak sensitive text. Candidate previews
  should include geometry and labels, not raw screenshot pixels or OCR text.
- Template presets will remain limited until template handles have a lifecycle.
  That is acceptable for Phase A.

## Deferred Work

- Real OCR/layout inspection tools.
- Source patch editing.
- Fixture-based model evals.
- Improve-existing-preset loop from reviewed edits.
- Template handle creation and persistence.
