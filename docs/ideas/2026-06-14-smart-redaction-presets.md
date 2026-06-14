# Smart Redaction Presets

**Date:** 2026-06-14
**Status:** Idea approved for preservation; not an implementation spec

## Product Thesis

Let users describe what should be hidden in natural language, then turn that
intent into a reusable local redaction preset.

For example:

- "Hide my browser bookmarks."
- "Hide document folders on my desktop."

The first run uses an LLM to generate a lightweight JavaScript detector. Later
runs execute that detector locally against similar screenshots. Every run
produces editable redaction candidates for the user to review before using
Rollshot's existing safe copy or save flow.

The signature value is not generic AI redaction. It is teaching Rollshot once,
then quickly reusing an inspectable detector without repeatedly calling an LLM.

## Product Decisions

- Users explicitly run a named preset, such as "Work Desktop Share."
- A preset checks app and layout applicability before proposing redactions.
- Redaction candidates always require user review. A successful detector run
  never means that the screenshot is safe.
- The default generation mode sends the complete screenshot to a vision LLM.
- Before every screenshot upload, the UI clearly identifies the provider and
  what will be sent.
- Users can switch to a mode that sends only locally generated OCR and layout
  information.
- No-match, low-confidence, or failed runs can offer to improve the preset by
  calling the LLM again.

## Generated Script Model

Use JavaScript running in an embedded QuickJS runtime. Scripts should stay
small and readable. Rust owns all expensive, privileged, and security-sensitive
operations.

JavaScript is responsible for:

- Combining detector results.
- Applying conditions and confidence thresholds.
- Producing candidate redaction rectangles.

Rust is responsible for:

- OCR and layout analysis.
- Color, edge, and region analysis.
- Template matching.
- Future object-detection capabilities when justified.
- Input, output, and rectangle validation.
- Runtime resource and permission enforcement.

The JavaScript environment exposes only a frozen, versioned `rollshot` API. It
has no filesystem, network, process, module-import, timer, async, Node.js, or
DOM capabilities.

Each run uses a fresh QuickJS context with memory, stack, wall-clock,
detector-call, and output-count limits. Rust rejects malformed, zero-area,
out-of-bounds, or excessive candidate rectangles.

TypeScript declarations should document the exposed API for both users and the
LLM.

## Preset Data

A preset stores:

- Name.
- Original natural-language intent.
- Generated JavaScript source.
- Rollshot capability API version.
- App and layout applicability hints.
- Created and updated timestamps.
- Optional non-sensitive fixtures and expected rectangles.

## First-Release Scope

Include:

- Named presets.
- Explicit vision-upload disclosure.
- OCR/layout-only generation mode.
- LLM-generated JavaScript.
- QuickJS capability sandbox.
- Rust OCR, layout, color/edge-region, and template-matching APIs.
- Candidate preview, edit, delete, retry, and improve flows.
- Integration with existing `ImageDocument` redactions and safe copy/save.

Defer:

- Silent or unattended safe export.
- Python, Lua, Node.js, and arbitrary OpenCV bindings.
- YOLO, model downloads, and model training.
- Script sharing or a marketplace.
- Automatically executing presets on every capture.

## Failure Semantics

Every preset run ends in one of these explicit states:

- Candidates found.
- No confident match.
- Script or runtime failure.
- Detector failure.

None of these states claims that all sensitive information was found. Manual
redaction tools remain available in every case.

## Verification Direction

- Unit-test capability input/output validation, sandbox escapes, and resource
  limits.
- Fixture-test generated scripts against expected rectangles and layout
  variations.
- Integration-test candidate creation through `ImageDocument` redactions and
  flattened safe export.
- Measure QuickJS cold-start and detector latency.
- Manually verify the result-workspace behavior on Linux and macOS.

## Main Risks

- Default full-screenshot upload creates a high trust burden. The disclosure
  must be immediate, specific, and repeated before each upload.
- Generated scripts may overfit the first screenshot and silently miss later
  layout changes. Mandatory preview is therefore part of the product, not a
  temporary limitation.
- The exposed Rust capability API can grow into a difficult general-purpose
  vision platform. Add capabilities only in response to validated preset use
  cases.
- Script sandboxing is a security boundary. Treat API exposure, runtime limits,
  and dependency updates as security-sensitive work.
