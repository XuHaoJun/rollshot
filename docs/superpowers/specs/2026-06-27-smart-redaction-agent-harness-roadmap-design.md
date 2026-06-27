# Smart Redaction Agent Harness Roadmap

**Date:** 2026-06-27
**Status:** Active roadmap; Phase C complete

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

## Progress

### Completed

- Phase A: Authoring Stabilization was implemented on
  `feat/smart-redaction-agent-harness-roadmap`.
  - The provider request now carries a Rollshot JavaScript authoring guide with
    prompt-example validation.
  - The product workbench registry exposes only truthful Phase A authoring tools.
  - Workbench dry-run and existing-preset execution prepare canonical
    region-feature queries before QuickJS.
  - Dry-run results include bounded candidate previews.
- Phase B1: RegionFeatures-First Inspection Surface was implemented on
  `feat/smart-redaction-agent-harness-roadmap`.
  - Product Smart Redaction runs expose truthful image context,
    region-feature inspection, capability availability, and authoring state.
  - Region-feature observations are bounded, named, and backed by prepared
    `RealAutomationHost` context.
- Phase B2: OCR-Enabled Inspection Surface was implemented on
  `feat/smart-redaction-agent-harness-roadmap`.
  - OCR-enabled builds expose `inspect_ocr` backed by prepared canonical OCR
    regions.
  - Default builds keep OCR unavailable and do not register a fake OCR tool.
  - The authoring prompt now directs text-driven requests through OCR
    inspection before source changes.
- Phase B3: Capability Handle Visibility was implemented on
  `feat/smart-redaction-agent-harness-roadmap`.
  - Authoring inspection exposes bounded capability-handle summaries and
    template-match availability metadata.
  - Product Smart Redaction currently reports template matching unavailable
    because the product handle map is empty.
- Phase B4: Layout and Template Inspection Follow-Up was completed as a
  truthful registry check on `feat/smart-redaction-agent-harness-roadmap`.
  - Product Smart Redaction keeps layout/template inspection tools out of the
    registry until their underlying lifecycle returns real, testable data.
  - Template-handle creation and persistence remain Phase F work.
- Phase C: Source Editing Ergonomics was implemented on
  `feat/smart-redaction-agent-harness-roadmap`.
  - The agent can read current source, generation, validation summary, and
    recent evidence before editing.
  - The agent can perform exact-replace source edits with stale-generation
    checks and recoverable mismatch feedback.
  - Source mutations emit bounded diff summaries for the run stream and
    workbench activity drawer.

### Remaining Gaps

- The agent has source replacement and validation tools, but still lacks a rich
  equivalent of "read the file and environment before editing."
- Layout and template inspection remain unavailable in product authoring runs
  until they return truthful data.
- `RealAutomationHost` now prepares canonical region-feature results and,
  behind OCR-enabled builds, canonical OCR results for dry-run and product
  authoring inspection. Template matching and arbitrary region inspection remain
  unavailable in product authoring runs.
- `AutomationInput.capability_handles` is empty in the product workbench path,
  so fixture-style template presets cannot work there yet.

## Roadmap

### Phase A: Authoring Stabilization — Complete

Make the existing harness usable without changing its architecture.

- Add a Rollshot JavaScript authoring guide to the model request.
- Ensure workbench dry-run and existing-preset execution prepare capabilities
  before QuickJS when the source requires them.
- Expose only truthful inspection tools. Either wire them to real prepared
  context or remove them from the product registry until they are real.
- Improve validation and dry-run tool descriptions/results enough for the model
  to iterate.
- Add focused tests for request prompt content, prompt examples, tool contracts,
  host preparation, and a region-feature-based preset.

### Phase B: Visual Inspection Surface

Give the model compact, safe, structured observations about the current image.

Phase B is split into multiple implementation specs so inspection can improve
without mixing OCR model/toolchain risk, template lifecycle design, and source
editing ergonomics into one change.

#### Phase B1: RegionFeatures-First Inspection Surface — Complete

- Add real inspection tools for image dimensions, canonical prepared
  region-feature observations, capability availability, and current authoring
  state.
- Return bounded observations with coordinates, feature summaries, and explicit
  error/availability codes.
- Register these truthful tools in product Smart Redaction runs.
- Keep raw pixels and OCR text out of tool results.
- Keep OCR/layout/template-match inspection unavailable but structured.

#### Phase B2: OCR-Enabled Inspection Surface — Complete

- Add `inspect_ocr` only when the `rollshot-vision/ocr` feature is compiled and
  the OCR engine can prepare bounded regions.
- Return bounded OCR matches with coordinates, confidence, and redacted/truncated
  text policy decided in the B2 spec.
- Make OCR-disabled builds report structured unavailable responses without
  exposing a fake working tool.

#### Phase B3: Capability Handle Visibility — Complete

- Expose available `AutomationInput.capability_handles` and capability
  availability metadata to the model.
- Do not invent template-handle persistence here; only surface handles that
  already exist.

#### Phase B4: Layout and Template Inspection Follow-Up — Complete

- Add real layout or template inspection only after the underlying capability
  lifecycle is designed and testable.
- Keep layout/template stubs out of the product registry until they return
  truthful data.

### Phase C: Source Editing Ergonomics — Complete

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

- Resolved (Phase C): source editing uses exact-replace with a required unique
  `old` match; `replace_source` remains the full-rewrite escape hatch. Unified
  diff and AST-aware operations were rejected as ill-suited to short detector
  scripts. See
  `2026-06-27-smart-redaction-agent-phase-c-source-editing-ergonomics-design.md`.
