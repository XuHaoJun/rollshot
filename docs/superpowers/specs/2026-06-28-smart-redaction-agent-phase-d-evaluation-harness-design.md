# Smart Redaction Agent Phase D Evaluation Harness

**Date:** 2026-06-28
**Status:** Draft for user review

## Goal

Give the Smart Redaction authoring harness a deterministic regression gate plus a
documented way to seed it from a live model.

Two outcomes:

1. A CI gate that fails when a change breaks the authoring path's ability to turn
   a known intent into useful redaction candidates — without calling a model API
   in CI.
2. A documented workflow for running a real model once against a fixture,
   reviewing what it produced, and freezing it as a reproducible fixture.

The harness measures whether the path can reliably produce useful presets:
source validity, candidate coverage of expected rectangles, and false-positive
area, plus reported-only signals (turns, candidate count, unnecessary user-input
requests).

## Approach Rationale

opencode (a mature coding agent; see `learn-projects/opencode`) has no separate
eval framework. It records real LLM HTTP interactions to JSON cassettes, redacts
secrets, and replays them deterministically in CI; the cassette is the golden
artifact. Rollshot already has the seed of this pattern in
`crates/rollshot-agent/tests/provider_contract.rs` (`wiremock` +
`fixtures/provider_streams.json` with provenance metadata), but it only exercises
a single adapter stream, not the full agent loop.

Phase D extends that pattern to the whole authoring loop, and adds a second,
cheaper scoring layer extracted from the same recording. Recording a run is both
the live-seeding step and the source of both deterministic artifacts, so the
"both layers" decision does not double cost: the golden source is a byproduct of
the cassette, and the scoring function and fixture image are shared.

## Two Layers From One Recording

A single recorded authoring run produces both deterministic CI artifacts:

- **Layer 1 — Cassette full-loop replay.** A `wiremock` server replays the
  recorded ordered HTTP interactions. `AnthropicAdapter` / `OpenAIAdapter` are
  constructed with `base_url` pointing at that server, and the full
  `AgentRunner::run_with_provider` loop runs against the synthetic fixture image
  and a prepared `RealAutomationHost`. Asserts the run reaches `ReadyForReview`
  and the dry-run candidates meet the scoring thresholds. This exercises the
  whole system: driver loop, tool registry, host preparation, dry-run, scoring.

- **Layer 2 — Golden-source geometry scoring.** The detector JavaScript the
  recorded run converged on is extracted to `golden_source.js` and run directly
  through `validate_source` then `dry_run` against the same fixture image and
  host — no model, no loop. Scored against the same `expected_rects` with the
  same scoring function. Because it bypasses the model, it stays stable across
  prompt changes and localizes regressions.

Diagnostic value of running both:

- Layer 1 fails, Layer 2 passes → regression is in the loop, prompt, or replay
  (re-record needed).
- Layer 2 fails → regression is in the JS API, host preparation, or scoring (a
  real execution-path bug).

## Fixture Format

One directory per scenario under `crates/rollshot-app/tests/eval/fixtures/`
(data files only; see "Where It Lives" for why the harness itself is a
crate-internal test module rather than a `tests/` integration test):

```
<intent>/
  meta.json            # intent label, provider, model, payload_mode,
                       # required_capability ("region_features" | "ocr"),
                       # score thresholds (optional per-fixture overrides)
  image.png            # synthetic rendered UI (non-sensitive)
  expected_rects.json  # [{x,y,width,height,label}]
  cassette.json        # recorded ordered HTTP interactions (redacted)
  golden_source.js     # detector JS extracted from the recording, human-reviewed
```

`cassette.json` carries provenance metadata mirroring the existing
`provider_streams.json` style: `recordedAt`, `provider`, `model`, and a
`substitutions` note describing redaction.

## Synthetic Fixture Images

A small renderer test helper uses the `image` crate (already a workspace
dependency) to draw fake UI with obviously-fake data: a browser chrome with a URL
bar, a bookmarks bar, a desktop/file-manager folder grid, and an email/contact
list. No real screenshots enter the repository.

Constraints:

- Text-driven intents require the rendered text to be legible to the OCR engine.
  A fixture whose golden source cannot locate its target text is a renderer
  defect, not an OCR defect; seeding must verify the golden actually finds the
  text.
- Region-feature intents require visually distinct regions (color/edge contrast)
  so `RegionFeatures` queries can anchor on them.
- Images are regenerable: the renderer is deterministic and the committed PNGs
  can be reproduced from it.

## Cassette Recording, Replay, and Redaction

- **Record mode** is the env-gated mode of the same test
  (`ROLLSHOT_RECORD_EVAL=1` plus the relevant provider API key). The adapter is
  pointed at the real provider, the full run executes, and each HTTP
  request/response interaction is captured in order to `cassette.json`.
- **Replay mode** is the default. The cassette is served by `wiremock` in order;
  no API key is required. A missing cassette under CI is a hard failure
  (fail-fast), never a silent skip.
- **Redaction** runs before a cassette is written:
  - Strip `authorization` / `x-api-key` headers and any credential-shaped fields.
  - The first request body carries the screenshot as a base64 image block
    (vision). Replace that block with attachment metadata
    (`media_type`, `width`, `height`, `byte_count`, `sha256`) that references the
    committed `image.png`. Subsequent turns are text-only (`ModelRequest` has no
    image field) and need no image redaction.
- **Request matching** for replay uses turn index plus a stable metadata subset
  (model, turn number, presence of the attachment hash), not full base64 image
  bytes. This keeps cassettes small and satisfies the roadmap rule "store
  redacted attachment metadata, not raw screenshots."

## Scoring Metrics

Computed from the dry-run candidate rectangles against `expected_rects` using one
shared scoring function used by both layers.

Hard gates (failure fails the test):

- **Source validity** — the detector source validates.
- **Coverage** — for each expected rectangle, the fraction of its area intersected
  by the union of candidate rectangles is at or above a coverage threshold.
  (Coverage, not IoU: over-coverage is penalized separately by false-positive
  area, so it should not also reduce the coverage score.)
- **False-positive area** — candidate area outside any expected rectangle,
  expressed as a fraction of total expected-rectangle area, is at or below a
  threshold.

Reported-only in v1 (recorded in test output, not gated):

- Number of turns.
- Candidate count.
- Whether the run requested user input unnecessarily (reached `NeedsUserInput`
  when a usable detector was expressible).

Thresholds start lenient and are tightened as fixtures stabilize. Defaults live
in the harness; `meta.json` may override per fixture.

## Intent Set (v1)

All six roadmap intents ship in v1:

- URL bar
- Bookmarks
- Desktop folders
- Emails
- Names
- Account IDs

Each fixture is tagged in `meta.json` with its `required_capability`.

## CI Placement

Most v1 intents are text-driven and need the off-by-default `ocr` feature
(`rollshot-app/ocr` → `rollshot-vision/ocr`), which already runs in the separate
path-filtered `ci-ocr.yml` lane. The gate therefore runs in two places:

- Region-feature-only fixtures run in the default workspace test suite
  (`cargo test --workspace` already exercises `rollshot-app`'s `#[cfg(test)]`
  modules).
- OCR-required fixtures run only under an `ocr`-enabled build, i.e. in the
  `ci-ocr.yml` lane, which must build `rollshot-app` with the `ocr` feature.

The harness selects which fixtures to execute from the compiled feature set:
OCR-required fixtures are skipped (not failed) when the `ocr` feature is absent,
and the skip is logged so an OCR fixture is never silently treated as covered in
default CI.

## Where It Lives

The real product authoring path is assembled in `rollshot-app`: `RealAutomationHost`
and `VisualIndex` come from `rollshot-vision`, but the canonical region/OCR
catalogs, `prepare_vision_context`, and `build_authoring_tool_registry` are private
to the workbench module, and `rollshot-agent` depends on neither `rollshot-vision`
nor `rollshot-app`. To exercise the genuine product path (not a reconstruction
that would drift), the harness must reach that private wiring.

`rollshot-app` is a binary-only crate (no `[lib]` target), so a `tests/`
integration test cannot link its internals. The harness is therefore a
**crate-internal `#[cfg(test)]` module** in `rollshot-app`, mirroring the existing
`mod tests` in `workbench/run.rs`, with test-only helper modules for the synthetic
renderer, cassette record/replay, and scoring. Fixture data lives under
`crates/rollshot-app/tests/eval/fixtures/`. `wiremock` is added as a `rollshot-app`
dev-dependency (it is already one for `rollshot-agent`).

No new crate and no new binary. Seeding is the env-gated record mode of the same
test. If the test module later grows unwieldy, extracting the host-preparation and
registry wiring into a small support library is deferred follow-up, not v1.

## Documentation

- New `docs/smart-redaction-eval.md`, mirroring `docs/bench.md`, covering: adding
  a fixture, rendering the synthetic image and annotating `expected_rects`,
  recording a cassette against a live model, reviewing the extracted
  `golden_source.js`, re-recording after prompt/tool changes, and the redaction
  guarantees.
- A README pointer to that doc from the developer-tooling section.

## Error Handling

- Missing cassette in CI replay mode → hard failure with the fixture name.
- A cassette whose recorded tool-call sequence no longer matches current tool
  schemas → fails clearly, signaling a re-record.
- A golden source that fails to validate → Layer 2 hard failure.
- Renderer failure or OCR failing to read a text fixture → surfaced during
  seeding, not masked.

## Testing / Verification

- The eval tests are themselves the verification artifact; they run in replay
  mode in the appropriate CI lane.
- A minimal self-test proves the harness machinery works without a real model:
  one fixture with a hand-written cassette and golden source, asserting both
  layers pass, plus a deliberately-bad golden asserting Layer 2 fails.
- `cargo fmt --check` and clippy on the new test code.

Verification commands:

```bash
rtk cargo test -p rollshot-app eval               # region-feature fixtures (default build)
rtk cargo test -p rollshot-app --features ocr eval # adds OCR-required fixtures
rtk cargo fmt --check
```

## Risks

- **Cassette brittleness to prompt/tool-schema changes.** Mitigated by treating a
  re-record as expected on such changes, keeping recording a one-command env-gated
  step, and using Layer 2 (model-free) to confirm whether a failure is loop/prompt
  or execution-path.
- **Synthetic images not exercising OCR realistically.** Rendered text is real
  text, so OCR applies; the seeding step verifies each text fixture's golden
  actually locates its target.
- **Threshold tuning.** Starting lenient risks a weak gate; tightening risks
  flakiness. Thresholds are explicit and per-fixture-overridable so they can be
  raised deliberately as fixtures prove stable.
- **OCR lane coverage gap.** OCR fixtures only run in `ci-ocr.yml`; the skip is
  logged in default CI so coverage is never silently overstated.

## Non-Goals

- No live model calls in CI.
- No continuous quality tracking, dashboards, or score history in v1.
- No real or sanitized screenshots in the repository.
- No template-matching fixtures (Phase F).
- No marketplace or shared fixture format.

## Deferred Work

- Live evaluation with score aggregation and trend tracking.
- Template-matching fixtures once Phase F defines the capability lifecycle.
- Expanding beyond the six v1 intents.
- Automated threshold tightening as fixtures stabilize.
