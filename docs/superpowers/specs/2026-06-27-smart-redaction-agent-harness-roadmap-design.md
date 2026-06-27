# Smart Redaction Agent Harness Roadmap

**Date:** 2026-06-27
**Status:** Draft for user review

## Context

`docs/ideas/2026-06-14-smart-redaction-presets.md` defines the product thesis:
users teach Rollshot once with natural language, Rollshot generates a small
local JavaScript detector, and later screenshots reuse that detector locally
with mandatory user review.

The persistence, validation, QuickJS runtime, bounded agent loop, and workbench
handoff are partially built. The weak part is the authoring harness. The current
agent can call source replacement, validation, dry-run, and submit tools, but it
does not yet have enough domain context or visual inspection feedback to
reliably write useful preset JavaScript. The intended model is closer to a code
agent than a one-shot vision classifier: the agent should inspect context, edit a
program, run checks, read failures, and iterate until a reviewable draft exists.

## Thesis

Treat a screenshot as a constrained document and a preset source file as the
editable program that mutates that document. The harness should make the model
behave like it is editing code:

1. Read current state and available APIs.
2. Make a bounded source change.
3. Validate static language constraints.
4. Dry-run against the current image.
5. Use structured failures and candidate evidence to refine.
6. Submit only when the current generation has successful validation and
   dry-run evidence.

The model should not be expected to infer hidden Rollshot contracts from a
screenshot attachment alone.

## Current Gaps

- The system prompt is too small for source authoring. It does not include the
  JavaScript subset, output shape, capability API, examples, or common recovery
  loop.
- The agent has source replacement and validation tools, but no good equivalent
  of "read the file and environment before editing."
- The workbench registers the authoring tools and a context summary, but no
  useful screenshot inspection tools.
- Inspection tools for OCR, layout, and region features exist as stubs that
  report unavailable, which can mislead the model if exposed.
- `RealAutomationHost` requires capability preparation before QuickJS execution,
  but the workbench path currently constructs a fresh host without preparing
  OCR, template, or region-feature results.
- `AutomationInput.capability_handles` is empty in the product workbench path,
  so fixture-style template presets cannot work there yet.
- Dry-run returns only candidate count, affected area, and capability-call
  count. It does not return enough bounded candidate evidence for the model to
  understand whether it found the requested target.

## Roadmap

### Phase A: Authoring Stabilization

Make the existing harness usable without changing its architecture.

- Add a Rollshot JavaScript authoring guide to the model request.
- Ensure workbench dry-run and existing-preset execution prepare capabilities
  before QuickJS when the source requires them.
- Expose only truthful inspection tools. Either wire them to real prepared
  context or remove them from the product registry until they are real.
- Improve validation and dry-run tool descriptions/results enough for the model
  to iterate.
- Add focused tests for request prompt content, tool contracts, host preparation,
  and a region-feature-based preset.

This phase is the immediate implementation target.

### Phase B: Visual Inspection Surface

Give the model compact, safe, structured observations about the current image.

- Add real inspection tools for image dimensions, region features, OCR when
  compiled/enabled, and available capability handles.
- Return bounded observations with coordinates, confidence, and error codes.
- Keep raw pixels out of tool results unless the user explicitly chose full
  screenshot upload.
- Make "OCR/layout-only mode" actually provide local context when full upload is
  disabled.

### Phase C: Source Editing Ergonomics

Move from whole-source replacement toward code-agent-style editing.

- Add a read-current-source tool that returns generation, source, validation
  summary, and recent evidence.
- Add an edit-source tool using exact replace or patch semantics with stale
  generation checks.
- Keep `replace_source` as the low-level escape hatch, but steer normal model
  behavior toward smaller edits.
- Include source diffs in run events and review UI so users can inspect preset
  changes like code.

### Phase D: Evaluation Harness

Measure whether the harness can reliably produce useful presets.

- Add fixture tasks for common intents: URL bar, bookmarks, desktop folders,
  emails, names, and account IDs.
- Evaluate source validity, candidate overlap with expected rectangles,
  candidate count, false-positive area, number of turns, and whether the run
  asked for unnecessary user input.
- Store model/provider transcripts with redacted attachment metadata, not raw
  screenshots.
- Gate prompt/tool changes with deterministic provider mocks and optional live
  evals.

### Phase E: Improve Existing Preset Loop

Close the loop after a preset misses or overfires.

- Feed reviewed candidate edits, deleted false positives, and manually added
  missing rectangles back into a new authoring run.
- Preserve parent revision linkage and clear provenance.
- Make the agent explain what changed in the detector before review.
- Keep every revision immutable and manually accepted.

### Phase F: Capability and Template Lifecycle

Make reusable visual detectors work beyond the first screenshot.

- Define how template handles are created, named, persisted, and passed through
  `AutomationInput.capability_handles`.
- Add capability availability metadata to presets so the UI can explain why a
  preset cannot run.
- Decide which capabilities are first-class for v1: region features and OCR are
  likely first; template handles need more lifecycle design.

## Architecture Principles

- Rust owns expensive and security-sensitive vision work.
- JavaScript combines bounded detector results into candidate rectangles.
- The model edits JavaScript, not product state directly.
- Tool results must be structured, compact, and truthful.
- Every successful run remains "ready for review", never "safe".
- Missing capability preparation is a runtime error to fix in the harness, not
  a reason for the model to generate weaker scripts.

## Verification Strategy

- Unit-test each tool contract with stale generations, malformed input, and
  budget exhaustion.
- Contract-test provider requests for system prompt sections and tool schemas.
- Integration-test `validate_source -> dry_run -> submit_for_review` through the
  workbench path.
- Add fixture-level tests for `RealAutomationHost` preparation and
  `AutomationInput` capability handles.
- Add evaluation fixtures before making large prompt or editing-tool changes.

## Non-Goals

- No unattended export.
- No marketplace or shared preset format.
- No arbitrary JS/Node/Python/OpenCV execution.
- No claim that generated redactions are complete.
- No broad UI redesign in the harness roadmap itself.

## Open Decisions

- Whether Phase B should expose OCR results in OCR/layout-only mode when the OCR
  Cargo feature is disabled by default.
- Whether template handles belong in Phase A or should wait for Phase F. Phase A
  should not invent a persistence format for templates.
- Whether source editing should use exact-replace, unified diff, or AST-aware
  operations in Phase C.
