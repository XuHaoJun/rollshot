# Smart Redaction Agent Phase D Evaluation Harness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a deterministic CI gate for the Smart Redaction authoring path — synthetic-image fixtures scored two ways (full-loop cassette replay + extracted golden-source geometry scoring) — plus the recording path and docs that let a developer seed fixtures from a live model.

**Architecture:** The harness is a crate-internal `#[cfg(test)]` module tree in `rollshot-app` (a bin-only crate), because the real product authoring wiring (`build_authoring_tool_registry`, the canonical region/OCR catalogs, `prepare_vision_context`) is private to its workbench module and `rollshot-agent` depends on neither `rollshot-vision` nor `rollshot-app`. Replay reuses the existing `wiremock`-at-`base_url` pattern from `provider_contract.rs`, extended to ordered multi-turn responses. Recording uses a reverse-proxy at `base_url` that tees raw SSE.

**Tech Stack:** Rust, `image` + `imageproc`/`ab_glyph` (synthetic rendering), `wiremock` (replay), `rollshot-vision` `RealAutomationHost`, `rollshot-automation` `execute_to_proposal`/`validate_source`, `rollshot-automation-rquickjs` `QuickJsExecutor`, `rollshot-agent` `AgentRunner::run_with_provider`, `sha2` (attachment redaction), `tokio` (async tests).

## Global Constraints

- No raw or sanitized real screenshots in the repository — fixture images are synthetic only.
- Cassettes are redacted before commit: strip `authorization`/`x-api-key`; replace the first request's base64 image block with attachment metadata (`media_type`, `width`, `height`, `byte_count`, `sha256`) referencing the committed `image.png`.
- No live model calls in CI. Record mode is env-gated (`ROLLSHOT_RECORD_EVAL=1` + provider API key) and never runs in CI; a missing cassette under CI replay is a hard failure, never a silent skip.
- OCR-required fixtures only run under an `ocr`-enabled build (`rollshot-app/ocr` → `rollshot-vision/ocr`), i.e. the `ci-ocr.yml` lane. When the `ocr` feature is absent they are skipped (not failed), and the skip is logged.
- Scoring gates: source validity (hard), per-expected-rect coverage ≥ threshold (hard, coverage = intersected fraction of each expected rect, NOT IoU), false-positive area ratio ≤ threshold (hard). Turns / candidate count / unnecessary-user-input are reported only.
- Tools/diagnostics rules from AGENTS.md apply: use `tracing` with `rollshot::*` targets for any retained runtime diagnostics; no `println!`/`dbg!`. Prefix shell commands with `rtk`.
- The harness must exercise the genuine product authoring path; reconstructing the catalogs/registry instead of reusing the workbench helpers is disallowed (drift).

## Engineering Review Lock-In

This section is the auto-mode engineering-review amendment. It supersedes any conflicting task text below.

### Auto Decisions Applied

Auto decision D1 — Reduce MVP scope to the deterministic harness plus self-test cassette

Context: The original plan has 13 tasks and more than 12 new artifacts when fixture data/docs are counted.
ELI10: The plan is trying to build the harness and fully seed every live-model fixture at the same time. The harness is the thing that lets us verify future seeded cases; seeding six live model cases requires an API key and manual review, so it is not necessary to prove the harness works. If we ship the harness with a deterministic self-test cassette, developers can safely seed the rest in a follow-up.
Stakes if we pick wrong: We either block useful infrastructure on manual live-model work, or we ship a harness that does not actually gate full-loop replay.
Recommendation: 1A because it keeps the CI gate real while deferring the API-key/manual-review work.
Completeness: A=8/10, B=10/10
Pros / cons:
A) Harness + self-test cassette now; six live cassettes follow-up (recommended, human: ~1 day / AI: ~45 min, low risk, low maintenance)
  ✅ Produces a deterministic default CI gate with no live dependency.
  ❌ Does not immediately prove all six OCR intents against a real provider cassette.
B) Keep six live cassettes in this plan (human: ~2-3 days / AI: ~2 hours plus API access, medium risk, medium maintenance)
  ✅ Maximizes immediate fixture coverage.
  ❌ Makes plan completion depend on live provider behavior and manual review.
Net: Build the reusable gate first; seed broad live coverage as a separate, explicit follow-up.

Auto decision D2 — Make geometry scoring exact for overlapping rectangles

Context: The original `coverage_of` sums pairwise intersections and clamps, which overcounts overlapping candidates inside one expected rect.
ELI10: If two redaction boxes overlap the same target, the old math can count the overlapped pixels twice. That can make a bad result look like it covered the full target. The scorer is the gate, so it should be stricter than the model.
Stakes if we pick wrong: CI can pass a redaction that misses part of the sensitive area.
Recommendation: 2A because exact union coverage is the simplest defensible scoring rule.
Completeness: A=10/10, B=6/10
Pros / cons:
A) Implement exact axis-aligned union area helpers with overlap tests (recommended, human: ~0.5 day / AI: ~20 min, low risk, low maintenance)
  ✅ Handles overlapping candidates and overlapping expected regions predictably.
  ❌ Slightly more code than summed pairwise intersections.
B) Keep pairwise sum + clamp (human: ~0 / AI: ~0, medium risk, low maintenance)
  ✅ Smallest diff.
  ❌ Can produce false passes.
Net: The extra helper is worth it because this function is the core safety gate.

Auto decision D3 — Move workbench visibility promotion before Layer 2/Layer 1 reuse

Context: `workbench/run.rs` is currently a private module; Task 5 imports it before Task 6 promotes visibility.
ELI10: The plan asks the implementer to write code that cannot see the helper module yet. That creates a predictable compile failure unrelated to the behavior under test. Visibility changes should happen immediately before the first reuse.
Stakes if we pick wrong: The red/green loop fails for plumbing reasons and implementers patch ad hoc.
Recommendation: 3A because explicit ordering keeps the plan executable.
Completeness: A=10/10, B=5/10
Pros / cons:
A) Execute visibility promotion before Layer 2, or fold it into Task 5 Step 1 (recommended, human: ~10 min / AI: ~5 min, low risk, low maintenance)
  ✅ Keeps import paths and TDD steps coherent.
  ❌ Moves a refactor earlier than originally listed.
B) Leave Task 6 after Task 5 (human: ~0 / AI: ~0, medium risk, low maintenance)
  ✅ Preserves original numbering.
  ❌ Guarantees avoidable compile churn.
Net: Make the change easy, then make the easy change.

Auto decision D4 — Store redacted request metadata in cassettes

Context: The original `CassetteFile` only stores `status` and `sse_body`, but the constraints require auth stripping and image-body replacement.
ELI10: If the cassette does not store request data, the redaction code cannot prove it stripped secrets from request data. If the recorder ever writes full requests for debugging, there is no typed shape enforcing the safe form. The cassette format should make unsafe data unrepresentable.
Stakes if we pick wrong: A live recording can leak an API key or screenshot base64 into git.
Recommendation: 4A because it mirrors the opencode recorded-test shape while narrowing the stored request to safe fields.
Completeness: A=10/10, B=4/10
Pros / cons:
A) Add `RecordedRequest` + `RecordedResponse` with safe headers/body summary and pure redaction tests (recommended, human: ~0.5 day / AI: ~30 min, low risk, medium maintenance)
  ✅ Makes redaction testable without live model calls.
  ❌ Adds a few more cassette fields.
B) Keep response-only cassettes (human: ~0 / AI: ~0, high risk, low maintenance)
  ✅ Simpler replay code.
  ❌ Does not satisfy the stated redaction guarantee.
Net: Store only enough request metadata to audit replay and prove privacy.

Auto decision D5 — Replace the brittle `wiremock::Request` unit test with a helper-level responder test

Context: Task 7 constructs `wiremock::Request` directly and compares debug strings, which is version-fragile and does not assert the body.
ELI10: A test that checks debug output can pass or fail for reasons unrelated to replay order. The thing we need to know is whether the responder returns interaction A, then B, and then fails loudly when exhausted. That can be tested through a small helper without depending on private `wiremock` internals.
Stakes if we pick wrong: The cassette responder can regress while the test still gives false confidence.
Recommendation: 5A because it tests behavior directly.
Completeness: A=10/10, B=5/10
Pros / cons:
A) Factor `next_interaction_for_test`/`next_response` and assert body/order/exhaustion directly (recommended, human: ~20 min / AI: ~10 min, low risk, low maintenance)
  ✅ Stable across `wiremock` versions.
  ❌ Slightly exposes test-only helper code.
B) Keep direct `Request` construction (human: ~0 / AI: ~0, medium risk, low maintenance)
  ✅ Looks closer to the responder trait.
  ❌ Asserts implementation detail rather than replay semantics.
Net: Prefer explicit behavior tests over clever trait-object tests.

Auto decision D6 — Layer 1 self-test is mandatory before final verification

Context: Task 9 allowed the full-loop `#[tokio::test]` to be ignored if hand-authored SSE was fiddly.
ELI10: Layer 2 proves golden-source geometry, but it does not prove the agent calls tools, validates, dry-runs, and submits through the product registry. The whole point of Layer 1 is to catch that wiring. If the Layer 1 self-test is ignored, the plan does not meet its stated goal.
Stakes if we pick wrong: CI can miss breakage in the full authoring loop.
Recommendation: 6A because a deterministic self-test cassette is the smallest real full-loop gate.
Completeness: A=10/10, B=6/10
Pros / cons:
A) Require a passing non-ignored `layer1_selftest_replay_reaches_ready_and_scores` before final verification (recommended, human: ~0.5-1 day / AI: ~45 min, medium risk, low maintenance)
  ✅ Proves the real product authoring path.
  ❌ Requires either hand-authored Anthropic SSE or recorder output before completion.
B) Allow ignored Layer 1 test until later (human: ~0 / AI: ~0, high risk, low maintenance)
  ✅ Lets the plan land sooner.
  ❌ Leaves the highest-risk path ungated.
Net: A harness without an active full-loop gate is under-engineered.

Auto decision D7 — Add seeded-state semantics for missing goldens/cassettes

Context: Task 10 skips missing `golden_source.js` for unseeded fixtures, while the global constraints say missing cassettes under CI replay must fail.
ELI10: Some fixtures are intentionally unseeded in the MVP, but once a fixture is marked seeded, missing files are broken, not optional. The test needs a clear source of truth so local development can skip unseeded fixtures while CI fails on missing seeded artifacts. A boolean in `meta.json` is enough.
Stakes if we pick wrong: CI can silently skip a fixture that the team thinks is covered.
Recommendation: 7A because explicit fixture state is boring and auditable.
Completeness: A=10/10, B=6/10
Pros / cons:
A) Add `seeded: bool` to `FixtureMeta`; fail when seeded artifacts are missing or when `CI=true` expects seeded artifacts (recommended, human: ~0.5 day / AI: ~20 min, low risk, low maintenance)
  ✅ Makes skip/fail behavior explicit.
  ❌ Requires updating generated `meta.json`.
B) Keep path-exists-only skipping (human: ~0 / AI: ~0, medium risk, low maintenance)
  ✅ Less metadata.
  ❌ Silent coverage drift is easy.
Net: State beats inference for CI gates.

Auto decision D8 — Add pure recorder-redaction tests before live proxy work

Context: Task 11 starts with a live reverse-proxy spike, but privacy redaction can be tested deterministically first.
ELI10: The hardest privacy bugs are easy to test with fake JSON: headers get removed, image base64 gets replaced, and the committed PNG hash matches metadata. We should not wait for a live Anthropic call to test that. The live spike should only answer the streaming fidelity question.
Stakes if we pick wrong: A live recording can be written before the redaction rules are proven.
Recommendation: 8A because complete privacy tests are cheap with AI-assisted execution.
Completeness: A=10/10, B=7/10
Pros / cons:
A) Add pure `redact_recorded_request`/`redact_cassette` tests before the proxy spike (recommended, human: ~0.5 day / AI: ~25 min, low risk, low maintenance)
  ✅ Proves no auth or image base64 survives in committed JSON.
  ❌ Adds one extra test step.
B) Test redaction only through record mode (human: ~0 / AI: ~0, medium risk, low maintenance)
  ✅ Fewer test fixtures.
  ❌ Requires live credentials to exercise privacy behavior.
Net: Privacy guarantees should be unit-testable without a provider.

Auto decision D9 — Keep CI workflow edits out unless current workflows do not run the gates

Context: Existing `.github/workflows/ci.yml` and `.github/workflows/ci-ocr.yml` already exist; the plan only needs to document/verify that `rollshot-app` eval tests are covered.
ELI10: Adding or rewriting CI workflows is unnecessary if the new tests are picked up by existing cargo lanes. But if the OCR lane does not build `rollshot-app --features ocr`, the plan must add the minimal workflow edit. This keeps distribution boring.
Stakes if we pick wrong: The gate may pass locally but never run in CI.
Recommendation: 9A because it is the smallest distribution check that still protects the goal.
Completeness: A=9/10, B=6/10
Pros / cons:
A) Verify existing CI coverage and only patch workflows if needed (recommended, human: ~20 min / AI: ~10 min, low risk, low maintenance)
  ✅ Avoids unnecessary CI churn.
  ❌ Requires reading workflow commands during implementation.
B) Always add a new workflow/job (human: ~0.5 day / AI: ~20 min, medium risk, medium maintenance)
  ✅ Makes eval coverage highly visible.
  ❌ Adds CI surface area before proving it is needed.
Net: Boring by default: reuse the existing lanes if they already cover the gate.

### NOT in Scope

- Seeding provider-recorded `golden_source.js` + `cassette.json` for all six OCR intents in this MVP. Rationale: requires live provider credentials and human review; the MVP ships the recorder and a deterministic full-loop self-test so broad seeding can happen as a follow-up.
- Adding a new standalone CI workflow unless existing `ci.yml` / `ci-ocr.yml` do not run the eval gates. Rationale: existing workflows already own default and OCR lanes; patch only if verification shows a gap.
- Supporting providers beyond the currently wired Anthropic replay/record path. Rationale: `run_with_provider` and `AnthropicAdapter` are enough to prove the authoring path; provider-matrix coverage is follow-up.
- Real screenshot fixtures or sanitized real screenshots. Rationale: fixture images stay synthetic by design.

### What Already Exists

- `crates/rollshot-app/src/result_workspace/workbench/run.rs` already builds the product authoring registry, canonical region/OCR catalogs, `prepare_vision_context`, and `AgentRunner::run_with_provider` flow. The plan must promote/reuse these helpers, not rebuild the registry.
- `crates/rollshot-agent/tests/provider_contract.rs` already uses `wiremock` with provider adapters and fixture SSE bodies. The plan extends that pattern to ordered multi-turn replay.
- `crates/rollshot-automation` already exposes `validate_source` and `execute_to_proposal`; Layer 2 should call these directly instead of inventing a scoring-only executor.
- `crates/rollshot-vision/tests/*` already exercise `RealAutomationHost` with prepared region/OCR fixtures; this plan adds product-path eval coverage, not host capability coverage from scratch.
- `learn-projects/opencode/packages/llm/test/recorded-*.ts` shows a mature cassette model: record/replay split, env-gated recording, metadata/tags, duplicate cassette detection, and redacted request/response storage. This plan adopts the shape but keeps Rollshot's CI stricter for seeded fixtures.

### Test Coverage Table

Task / behavior                                                   Unit  Integ  E2E / smoke  Manual only
────────────────────────────────────────────────────────────────  ────  ─────  ───────────  ───────────
Task 1 / scoring full cover, missed rect, false positives          ✓     —      —            no
Task 1 / overlapping candidates and zero-area rectangles            ✓     —      —            no
Task 2 / all six synthetic renderers are bounded and non-flat       ✓     —      —            no
Task 4 / fixture loader matches generated expected rects/meta       ✓     —      —            no
Task 5 / Layer 2 validates and executes golden source               ✓     ✓      —            no
Task 7 / cassette serde, ordered replay, exhaustion failure         ✓     ✓      —            no
Task 7 / cassette request redaction removes auth/image base64       ✓     —      —            no
Task 8 / Layer 1 full-loop replay reaches `ReadyForReview`          —     ✓      ✓            no
Task 10 / seeded/unseeded fixture gate and OCR-aware skip/fail      ✓     ✓      —            no
Task 11 / recorder byte-for-byte SSE replay fidelity                —     ✓      ✓            yes, env-gated ignored test
Task 11 / recorder writes only redacted cassette JSON               ✓     ✓      —            no
Task 12 / docs commands and CI coverage note                        —     —      —            yes, doc review

### Failure Modes

- Scoring math overcounts overlap: covered by Task 1 overlap tests; handled by exact union helpers; user sees a failing eval report, not a silent pass.
- Fixture file missing or stale: covered by Task 4 loader/staleness tests; handled by panics with file paths in tests; user sees a clear test failure.
- OCR fixture runs without `ocr` feature: covered by Task 10; handled by explicit logged skip only for unseeded/OCR-disabled local cases; user sees skip text.
- Seeded golden or cassette missing: covered by Task 10; handled by `seeded: true` checks and `CI=true` behavior; user sees a hard failure.
- Cassette exhausted before the agent reaches review: covered by Task 7 responder exhaustion test and Task 8 Layer 1 replay; handled by a panic/error that names the missing model-call index; user sees a clear failure.
- Provider SSE bytes not replayable after recording: covered by Task 11 spike/ignored live round-trip; handled by blocking recorder implementation until parsed live and replay events match; user sees spike failure.
- Recorder writes auth header or screenshot base64: covered by Task 7/11 redaction tests; handled by safe cassette structs and redaction helpers; user sees a failing privacy test.
- Full-loop terminal state is not `ReadyForReview`: covered by Task 8/9 Layer 1 self-test; handled by returning `Err("non-terminal-ready: ...")`; user sees a clear assertion failure.

Critical gaps after review: none, provided D4/D6/D7/D8 amendments are implemented.

### Worktree / Subagent Parallelization Strategy

Sequential execution is preferred until Task 4 because all early tasks build the same `crates/rollshot-app/src/result_workspace/workbench/eval/` module tree and generated fixture data. After Task 4, two lanes are possible but must merge before Layer 1.

| Task | Modules touched | Depends on |
|------|-----------------|------------|
| Task 1: Geometry scoring | `crates/rollshot-app/src/result_workspace/workbench/eval/`, `workbench/mod.rs` | — |
| Task 2-4: Renderers + fixture data | `crates/rollshot-app/src/result_workspace/workbench/eval/`, `crates/rollshot-app/tests/eval/fixtures/` | Task 1 |
| Task 5-6: Workbench visibility + Layer 2 | `crates/rollshot-app/src/result_workspace/workbench/` | Tasks 1-4 |
| Task 7-8: Cassette + Layer 1 | `crates/rollshot-app/src/result_workspace/workbench/eval/`, `crates/rollshot-app/Cargo.toml` | Tasks 1-6 |
| Task 10-11: Gate iteration + recorder | `crates/rollshot-app/src/result_workspace/workbench/eval/`, `crates/rollshot-app/Cargo.toml` | Tasks 7-8 |
| Task 12: Docs | `docs/`, `README.md` | Task 11 for final commands |
| Task 13: Verification | workspace | all prior |

Parallel lanes:
- Lane A: Tasks 1 → 2 → 3 → 4 → 5/6 → 7/8 → 10/11 → 13 (sequential, same module tree).
- Lane B: Task 12 docs can draft in parallel after Task 7, but final command text waits for Task 11.

Execution order: run Lane A as the main implementation lane; optionally draft Lane B docs after cassette shapes settle; merge docs before final verification.

Conflict flags: Tasks 2-11 all touch `crates/rollshot-app/src/result_workspace/workbench/eval/`; do not assign them to independent subagents without a merge coordinator. `crates/rollshot-app/Cargo.toml` changes in Tasks 7 and 11 serialize dependency edits.

## File Structure

Crate-internal test module tree under `rollshot-app` (all `#[cfg(test)]`):

- `crates/rollshot-app/src/result_workspace/workbench/eval/mod.rs` — module root; re-exports submodules; declared `#[cfg(test)] pub(crate) mod eval;` from `workbench/mod.rs`.
- `eval/scoring.rs` — pure geometry scoring (`ExpectedRect`, `Thresholds`, `ScoreReport`, `score_candidates`).
- `eval/render.rs` — synthetic rendering toolkit + six intent renderers returning `RenderedFixture`.
- `eval/fixture.rs` — on-disk fixture types + loader (`meta.json`, `expected_rects.json`, `image.png`, `golden_source.js`, `cassette.json`), `RequiredCapability`.
- `eval/cassette.rs` — cassette file types, ordered `CassetteResponder` (`wiremock::Respond`), redaction helpers.
- `eval/layer2.rs` — golden-source runner: `validate_source` + `execute_to_proposal` against a prepared host → candidate rects.
- `eval/layer1.rs` — full-loop runner: build product registry, point adapter at cassette-backed `wiremock`, `run_with_provider`, extract proposal → candidate rects.
- `eval/record.rs` — reverse-proxy recorder (added after the spike de-risks it).
- `eval/cases.rs` — the actual `#[test]`/`#[tokio::test]` cases (self-test fixture, bad-golden negative, per-fixture iteration).

Helpers in `workbench/run.rs` promoted to `pub(crate)` for reuse: `build_authoring_tool_registry`, `canonical_region_feature_catalog`, `canonical_ocr_catalog`, `authoring_inspection_context`, `product_capability_handles`. (`prepare_vision_context` is already `pub`.)

Fixture data (committed, data-only): `crates/rollshot-app/tests/eval/fixtures/<intent>/`.

Docs: `docs/smart-redaction-eval.md` + a README pointer.

---

### Task 1: Geometry scoring (pure)

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/workbench/eval/mod.rs`
- Create: `crates/rollshot-app/src/result_workspace/workbench/eval/scoring.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/mod.rs` (declare the module)

**Interfaces:**
- Produces: `ExpectedRect { x: f32, y: f32, width: f32, height: f32, label: String }` (serde); `Thresholds { min_coverage: f32, max_false_positive_ratio: f32 }` with `Thresholds::lenient()`; `ScoreReport { per_rect_coverage: Vec<(String, f32)>, min_coverage: f32, false_positive_ratio: f32, candidate_count: usize, gate_failures: Vec<String> }` with `ScoreReport::passed(&self) -> bool`; `fn score_candidates(expected: &[ExpectedRect], candidates: &[rollshot_image_document::ImageRect], thresholds: &Thresholds) -> ScoreReport`.
- Review amendment: coverage and false-positive area must use exact axis-aligned rectangle union area, not summed pairwise intersections. Add failing tests for overlapping candidates and zero-area rectangles before implementation.

- [ ] **Step 1: Declare the module**

In `crates/rollshot-app/src/result_workspace/workbench/mod.rs`, add near the other `mod` lines:

```rust
#[cfg(test)]
pub(crate) mod eval;
```

Create `crates/rollshot-app/src/result_workspace/workbench/eval/mod.rs`:

```rust
//! Phase D Smart Redaction evaluation harness (test-only).
//!
//! Deterministic gate over synthetic-image fixtures, scored two ways:
//! full-loop cassette replay (layer1) and extracted golden-source geometry
//! scoring (layer2). See `docs/smart-redaction-eval.md`.

pub(crate) mod scoring;
```

- [ ] **Step 2: Write the failing test**

Create `crates/rollshot-app/src/result_workspace/workbench/eval/scoring.rs`:

```rust
use rollshot_image_document::ImageRect;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ExpectedRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub label: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Thresholds {
    pub min_coverage: f32,
    pub max_false_positive_ratio: f32,
}

impl Thresholds {
    pub fn lenient() -> Self {
        Self {
            min_coverage: 0.6,
            max_false_positive_ratio: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ScoreReport {
    pub per_rect_coverage: Vec<(String, f32)>,
    pub min_coverage: f32,
    pub false_positive_ratio: f32,
    pub candidate_count: usize,
    pub gate_failures: Vec<String>,
}

impl ScoreReport {
    pub fn passed(&self) -> bool {
        self.gate_failures.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> ImageRect {
        ImageRect { x, y, width: w, height: h }
    }
    fn expected(label: &str, x: f32, y: f32, w: f32, h: f32) -> ExpectedRect {
        ExpectedRect { x, y, width: w, height: h, label: label.into() }
    }

    #[test]
    fn full_cover_no_false_positive_passes() {
        let exp = vec![expected("bar", 0.0, 0.0, 100.0, 10.0)];
        let cands = vec![rect(0.0, 0.0, 100.0, 10.0)];
        let report = score_candidates(&exp, &cands, &Thresholds::lenient());
        assert_eq!(report.min_coverage, 1.0);
        assert_eq!(report.false_positive_ratio, 0.0);
        assert!(report.passed(), "{:?}", report.gate_failures);
    }

    #[test]
    fn missed_rect_fails_coverage_gate() {
        let exp = vec![expected("bar", 0.0, 0.0, 100.0, 10.0)];
        let cands = vec![rect(0.0, 0.0, 40.0, 10.0)]; // 40% coverage
        let report = score_candidates(&exp, &cands, &Thresholds::lenient());
        assert!((report.min_coverage - 0.4).abs() < 1e-4);
        assert!(!report.passed());
        assert!(report.gate_failures.iter().any(|f| f.contains("coverage")));
    }

    #[test]
    fn excess_area_counts_as_false_positive() {
        let exp = vec![expected("bar", 0.0, 0.0, 100.0, 10.0)];
        // covers the bar fully, plus a 100x10 region entirely outside it
        let cands = vec![rect(0.0, 0.0, 100.0, 10.0), rect(0.0, 50.0, 100.0, 10.0)];
        let mut th = Thresholds::lenient();
        th.max_false_positive_ratio = 0.5;
        let report = score_candidates(&exp, &cands, &th);
        assert!((report.false_positive_ratio - 1.0).abs() < 1e-4);
        assert!(!report.passed());
        assert!(report.gate_failures.iter().any(|f| f.contains("false_positive")));
    }

    #[test]
    fn overlapping_candidates_do_not_double_count_coverage() {
        let exp = vec![expected("bar", 0.0, 0.0, 100.0, 10.0)];
        let cands = vec![rect(0.0, 0.0, 60.0, 10.0), rect(40.0, 0.0, 60.0, 10.0)];
        let report = score_candidates(&exp, &cands, &Thresholds::lenient());
        assert!((report.min_coverage - 1.0).abs() < 1e-4);
        assert!((report.false_positive_ratio - 0.0).abs() < 1e-4);
    }

    #[test]
    fn zero_area_expected_rect_fails_cleanly() {
        let exp = vec![expected("empty", 0.0, 0.0, 0.0, 10.0)];
        let cands = vec![rect(0.0, 0.0, 100.0, 10.0)];
        let report = score_candidates(&exp, &cands, &Thresholds::lenient());
        assert_eq!(report.min_coverage, 0.0);
        assert!(!report.passed());
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `rtk cargo test -p rollshot-app eval::scoring -- --nocapture`
Expected: FAIL — `cannot find function score_candidates`.

- [ ] **Step 4: Implement `score_candidates`**

Add above the `#[cfg(test)] mod tests` block in `scoring.rs`:

```rust
fn clipped_rect(a: &ImageRect, bx: f32, by: f32, bw: f32, bh: f32) -> Option<ImageRect> {
    let x0 = a.x.max(bx);
    let y0 = a.y.max(by);
    let x1 = (a.x + a.width).min(bx + bw);
    let y1 = (a.y + a.height).min(by + bh);
    let width = x1 - x0;
    let height = y1 - y0;
    (width > 0.0 && height > 0.0).then_some(ImageRect {
        x: x0,
        y: y0,
        width,
        height,
    })
}

fn rect_union_area(rects: &[ImageRect]) -> f32 {
    let rects: Vec<ImageRect> = rects
        .iter()
        .copied()
        .filter(|r| r.width > 0.0 && r.height > 0.0)
        .collect();
    if rects.is_empty() {
        return 0.0;
    }

    let mut xs = Vec::with_capacity(rects.len() * 2);
    let mut ys = Vec::with_capacity(rects.len() * 2);
    for r in &rects {
        xs.push(r.x);
        xs.push(r.x + r.width);
        ys.push(r.y);
        ys.push(r.y + r.height);
    }
    xs.sort_by(f32::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
    ys.sort_by(f32::total_cmp);
    ys.dedup_by(|a, b| (*a - *b).abs() < 1e-4);

    let mut area = 0.0;
    for xw in xs.windows(2) {
        for yw in ys.windows(2) {
            let (x0, x1) = (xw[0], xw[1]);
            let (y0, y1) = (yw[0], yw[1]);
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            let covered = rects.iter().any(|r| {
                r.x <= x0
                    && r.x + r.width >= x1
                    && r.y <= y0
                    && r.y + r.height >= y1
            });
            if covered {
                area += (x1 - x0) * (y1 - y0);
            }
        }
    }
    area
}

/// Coverage of one expected rect by the exact union of candidate rectangles.
fn coverage_of(expected: &ExpectedRect, candidates: &[ImageRect]) -> f32 {
    let area = expected.width * expected.height;
    if area <= 0.0 {
        return 0.0;
    }
    let clipped: Vec<ImageRect> = candidates
        .iter()
        .filter_map(|c| clipped_rect(c, expected.x, expected.y, expected.width, expected.height))
        .collect();
    let covered = rect_union_area(&clipped);
    (covered / area).min(1.0)
}

pub(crate) fn score_candidates(
    expected: &[ExpectedRect],
    candidates: &[ImageRect],
    thresholds: &Thresholds,
) -> ScoreReport {
    let per_rect_coverage: Vec<(String, f32)> = expected
        .iter()
        .map(|e| (e.label.clone(), coverage_of(e, candidates)))
        .collect();
    let min_coverage = per_rect_coverage
        .iter()
        .map(|(_, c)| *c)
        .fold(f32::INFINITY, f32::min);
    let min_coverage = if min_coverage.is_finite() { min_coverage } else { 0.0 };

    let total_expected_area: f32 = expected.iter().map(|e| e.width * e.height).sum();
    let total_candidate_area = rect_union_area(candidates);
    let clipped_inside: Vec<ImageRect> = candidates
        .iter()
        .flat_map(|c| {
            expected
                .iter()
                .filter_map(|e| clipped_rect(c, e.x, e.y, e.width, e.height))
        })
        .collect();
    let inside_area = rect_union_area(&clipped_inside);
    let outside_area = (total_candidate_area - inside_area).max(0.0);
    let false_positive_ratio = if total_expected_area > 0.0 {
        outside_area / total_expected_area
    } else {
        0.0
    };

    let mut gate_failures = Vec::new();
    if min_coverage < thresholds.min_coverage {
        gate_failures.push(format!(
            "coverage {min_coverage:.3} < {:.3}",
            thresholds.min_coverage
        ));
    }
    if false_positive_ratio > thresholds.max_false_positive_ratio {
        gate_failures.push(format!(
            "false_positive {false_positive_ratio:.3} > {:.3}",
            thresholds.max_false_positive_ratio
        ));
    }

    ScoreReport {
        per_rect_coverage,
        min_coverage,
        false_positive_ratio,
        candidate_count: candidates.len(),
        gate_failures,
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `rtk cargo test -p rollshot-app eval::scoring`
Expected: PASS (5 tests).

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/mod.rs crates/rollshot-app/src/result_workspace/workbench/eval/
rtk git commit -m "test(app): add phase d eval geometry scoring"
```

---

### Task 2: Synthetic rendering toolkit + URL-bar fixture

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/workbench/eval/render.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/eval/mod.rs` (add `pub(crate) mod render;`)

**Interfaces:**
- Consumes: `ExpectedRect` (Task 1).
- Produces: `RenderedFixture { image: image::RgbaImage, expected: Vec<ExpectedRect> }`; toolkit fns `fill(img, color)`, `draw_filled_rect(img, x, y, w, h, color)`, `draw_label(img, font, text, x, y, px, color)`; `fn render_url_bar() -> RenderedFixture`.

- [ ] **Step 1: Add the module declaration**

In `eval/mod.rs` add:

```rust
pub(crate) mod render;
```

- [ ] **Step 2: Write the failing test**

Create `crates/rollshot-app/src/result_workspace/workbench/eval/render.rs`:

```rust
use super::scoring::ExpectedRect;
use ab_glyph::{FontRef, PxScale};
use image::{Rgba, RgbaImage};
use imageproc::drawing::draw_text_mut;

pub(crate) struct RenderedFixture {
    pub image: RgbaImage,
    pub expected: Vec<ExpectedRect>,
}

const FONT_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../rollshot-image-document/assets/fonts/DejaVuSans.ttf"
));

fn font() -> FontRef<'static> {
    FontRef::try_from_slice(FONT_BYTES).expect("DejaVuSans font loads")
}

fn fill(img: &mut RgbaImage, color: [u8; 4]) {
    for px in img.pixels_mut() {
        *px = Rgba(color);
    }
}

fn draw_filled_rect(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: [u8; 4]) {
    let (iw, ih) = img.dimensions();
    for yy in y..(y + h).min(ih) {
        for xx in x..(x + w).min(iw) {
            img.put_pixel(xx, yy, Rgba(color));
        }
    }
}

fn draw_label(
    img: &mut RgbaImage,
    font: &FontRef<'static>,
    text: &str,
    x: u32,
    y: u32,
    px: f32,
    color: [u8; 4],
) {
    draw_text_mut(img, Rgba(color), x as i32, y as i32, PxScale::from(px), font, text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_bar_fixture_is_well_formed() {
        let f = render_url_bar();
        assert_eq!(f.image.dimensions(), (800, 200));
        assert_eq!(f.expected.len(), 1);
        let r = &f.expected[0];
        assert_eq!(r.label, "url_bar");
        // expected rect lies inside the image bounds
        assert!(r.x >= 0.0 && r.y >= 0.0);
        assert!(r.x + r.width <= 800.0 && r.y + r.height <= 200.0);
        // the bar region is not the same flat color as the page background
        let bg = f.image.get_pixel(5, 180);
        let bar = f.image.get_pixel(
            (r.x + r.width / 2.0) as u32,
            (r.y + r.height / 2.0) as u32,
        );
        assert_ne!(bg, bar);
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `rtk cargo test -p rollshot-app eval::render`
Expected: FAIL — `cannot find function render_url_bar`.

- [ ] **Step 4: Implement `render_url_bar`**

Add to `render.rs` above the test module:

```rust
const PAGE_BG: [u8; 4] = [245, 245, 245, 255];
const CHROME_BG: [u8; 4] = [60, 63, 70, 255];
const FIELD_BG: [u8; 4] = [255, 255, 255, 255];
const TEXT_DARK: [u8; 4] = [20, 20, 20, 255];

/// A browser chrome with a single URL field carrying obviously-fake text.
pub(crate) fn render_url_bar() -> RenderedFixture {
    let font = font();
    let mut img = RgbaImage::new(800, 200);
    fill(&mut img, PAGE_BG);
    // toolbar
    draw_filled_rect(&mut img, 0, 0, 800, 56, CHROME_BG);
    // url field
    let (fx, fy, fw, fh) = (120u32, 14u32, 600u32, 28u32);
    draw_filled_rect(&mut img, fx, fy, fw, fh, FIELD_BG);
    draw_label(&mut img, &font, "https://example.com/u/secret-12345", fx + 8, fy + 4, 20.0, TEXT_DARK);
    RenderedFixture {
        image: img,
        expected: vec![ExpectedRect {
            x: fx as f32,
            y: fy as f32,
            width: fw as f32,
            height: fh as f32,
            label: "url_bar".into(),
        }],
    }
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `rtk cargo test -p rollshot-app eval::render`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/eval/
rtk git commit -m "test(app): add eval render toolkit and url-bar fixture"
```

---

### Task 3: Remaining five intent renderers

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/eval/render.rs`

**Interfaces:**
- Produces: `render_bookmarks()`, `render_desktop_folders()`, `render_emails()`, `render_names()`, `render_account_ids()`, each `-> RenderedFixture`; `fn all_fixtures() -> Vec<(&'static str, RequiredCapability, RenderedFixture)>` is added in Task 5 (it consumes these).

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `render.rs`:

```rust
    #[test]
    fn all_five_remaining_fixtures_are_well_formed() {
        let cases: Vec<(RenderedFixture, usize)> = vec![
            (render_bookmarks(), 3),
            (render_desktop_folders(), 4),
            (render_emails(), 3),
            (render_names(), 3),
            (render_account_ids(), 3),
        ];
        for (f, want) in cases {
            let (w, h) = f.image.dimensions();
            assert!(w > 0 && h > 0);
            assert_eq!(f.expected.len(), want);
            for r in &f.expected {
                assert!(r.x >= 0.0 && r.y >= 0.0);
                assert!(r.x + r.width <= w as f32 && r.y + r.height <= h as f32);
                assert!(!r.label.is_empty());
            }
        }
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `rtk cargo test -p rollshot-app eval::render::tests::all_five_remaining_fixtures_are_well_formed`
Expected: FAIL — `cannot find function render_bookmarks`.

- [ ] **Step 3: Implement the five renderers**

Add to `render.rs` above the test module. Each draws a fake panel with obviously-fake data and returns one expected rect per redaction target.

```rust
const ROW_BG: [u8; 4] = [255, 255, 255, 255];
const ROW_ALT: [u8; 4] = [232, 235, 240, 255];
const ACCENT: [u8; 4] = [70, 110, 200, 255];

fn rows(img: &mut RgbaImage, font: &FontRef<'static>, x: u32, top: u32, labels: &[&str], rh: u32)
    -> Vec<ExpectedRect>
{
    let mut out = Vec::new();
    for (i, text) in labels.iter().enumerate() {
        let y = top + i as u32 * (rh + 8);
        let bg = if i % 2 == 0 { ROW_BG } else { ROW_ALT };
        draw_filled_rect(img, x, y, 360, rh, bg);
        draw_label(img, font, text, x + 8, y + 4, 18.0, TEXT_DARK);
        out.push(ExpectedRect {
            x: x as f32,
            y: y as f32,
            width: 360.0,
            height: rh as f32,
            label: format!("row_{i}"),
        });
    }
    out
}

pub(crate) fn render_bookmarks() -> RenderedFixture {
    let font = font();
    let mut img = RgbaImage::new(420, 220);
    fill(&mut img, PAGE_BG);
    draw_filled_rect(&mut img, 0, 0, 420, 30, ACCENT);
    let expected = rows(&mut img, &font, 20, 50, &[
        "Bookmark: secret-project-roadmap",
        "Bookmark: payroll-q3-internal",
        "Bookmark: vpn-admin-console",
    ], 28);
    RenderedFixture { image: img, expected }
}

pub(crate) fn render_desktop_folders() -> RenderedFixture {
    let font = font();
    let mut img = RgbaImage::new(480, 360);
    fill(&mut img, [40, 80, 60, 255]); // desktop wallpaper
    let mut expected = Vec::new();
    let names = ["Taxes 2025", "Client NDA", "Passwords", "HR Cases"];
    for (i, name) in names.iter().enumerate() {
        let col = (i % 2) as u32;
        let row = (i / 2) as u32;
        let x = 40 + col * 220;
        let y = 40 + row * 150;
        draw_filled_rect(&mut img, x, y, 80, 64, [230, 200, 120, 255]); // folder icon
        draw_filled_rect(&mut img, x, y + 70, 180, 26, [0, 0, 0, 160]); // label backdrop
        draw_label(&mut img, &font, name, x + 4, y + 72, 18.0, [255, 255, 255, 255]);
        expected.push(ExpectedRect {
            x: x as f32, y: (y + 70) as f32, width: 180.0, height: 26.0,
            label: format!("folder_{i}"),
        });
    }
    RenderedFixture { image: img, expected }
}

pub(crate) fn render_emails() -> RenderedFixture {
    let font = font();
    let mut img = RgbaImage::new(420, 200);
    fill(&mut img, PAGE_BG);
    let expected = rows(&mut img, &font, 20, 20, &[
        "ada.fake@example.com",
        "grace.test@example.org",
        "alan.sample@example.net",
    ], 28);
    RenderedFixture { image: img, expected }
}

pub(crate) fn render_names() -> RenderedFixture {
    let font = font();
    let mut img = RgbaImage::new(420, 200);
    fill(&mut img, PAGE_BG);
    let expected = rows(&mut img, &font, 20, 20, &[
        "Name: Ada Placeholder",
        "Name: Grace Sample",
        "Name: Alan Example",
    ], 28);
    RenderedFixture { image: img, expected }
}

pub(crate) fn render_account_ids() -> RenderedFixture {
    let font = font();
    let mut img = RgbaImage::new(420, 200);
    fill(&mut img, PAGE_BG);
    let expected = rows(&mut img, &font, 20, 20, &[
        "Account: ACME-0000-1111",
        "Account: ACME-2222-3333",
        "Account: ACME-4444-5555",
    ], 28);
    RenderedFixture { image: img, expected }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `rtk cargo test -p rollshot-app eval::render`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/eval/render.rs
rtk git commit -m "test(app): add five remaining eval intent renderers"
```

---

### Task 4: Fixture types, loader, and committed images

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/workbench/eval/fixture.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/eval/mod.rs`
- Create (data, generated in Step 5): `crates/rollshot-app/tests/eval/fixtures/<intent>/image.png` + `expected_rects.json` + `meta.json` for all six intents.

**Interfaces:**
- Consumes: `ExpectedRect` (Task 1), the six renderers (Task 3).
- Produces: `RequiredCapability { RegionFeatures, Ocr }` (serde, snake_case); `FixtureMeta { intent: String, provider: String, model: String, required_capability: RequiredCapability, seeded: bool }`; `fn fixtures_root() -> std::path::PathBuf`; `fn intent_specs() -> Vec<IntentSpec>` where `IntentSpec { name: &'static str, required_capability: RequiredCapability, render: fn() -> RenderedFixture }`; `fn load_expected(intent: &str) -> Vec<ExpectedRect>`; `fn load_meta(intent: &str) -> FixtureMeta`; `fn load_image(intent: &str) -> image::RgbaImage`.

- [ ] **Step 1: Add the module declaration**

In `eval/mod.rs` add `pub(crate) mod fixture;`.

- [ ] **Step 2: Write the failing test**

Create `crates/rollshot-app/src/result_workspace/workbench/eval/fixture.rs`:

```rust
use super::render::{
    render_account_ids, render_bookmarks, render_desktop_folders, render_emails, render_names,
    render_url_bar, RenderedFixture,
};
use super::scoring::ExpectedRect;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RequiredCapability {
    RegionFeatures,
    Ocr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FixtureMeta {
    pub intent: String,
    pub provider: String,
    pub model: String,
    pub required_capability: RequiredCapability,
    /// True only when both `golden_source.js` and `cassette.json` are committed
    /// and expected to gate CI for this fixture.
    pub seeded: bool,
}

pub(crate) struct IntentSpec {
    pub name: &'static str,
    pub required_capability: RequiredCapability,
    pub render: fn() -> RenderedFixture,
}

pub(crate) fn intent_specs() -> Vec<IntentSpec> {
    use RequiredCapability::*;
    vec![
        IntentSpec { name: "url_bar", required_capability: Ocr, render: render_url_bar },
        IntentSpec { name: "bookmarks", required_capability: Ocr, render: render_bookmarks },
        IntentSpec { name: "desktop_folders", required_capability: Ocr, render: render_desktop_folders },
        IntentSpec { name: "emails", required_capability: Ocr, render: render_emails },
        IntentSpec { name: "names", required_capability: Ocr, render: render_names },
        IntentSpec { name: "account_ids", required_capability: Ocr, render: render_account_ids },
    ]
}

pub(crate) fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/eval/fixtures")
}

pub(crate) fn load_expected(intent: &str) -> Vec<ExpectedRect> {
    let path = fixtures_root().join(intent).join("expected_rects.json");
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&data).expect("valid expected_rects.json")
}

pub(crate) fn load_meta(intent: &str) -> FixtureMeta {
    let path = fixtures_root().join(intent).join("meta.json");
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&data).expect("valid meta.json")
}

pub(crate) fn load_image(intent: &str) -> image::RgbaImage {
    let path = fixtures_root().join(intent).join("image.png");
    image::open(&path)
        .unwrap_or_else(|e| panic!("open {}: {e}", path.display()))
        .to_rgba8()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_intents_are_specified() {
        assert_eq!(intent_specs().len(), 6);
    }

    #[test]
    fn rendered_expected_rects_match_committed_json() {
        for spec in intent_specs() {
            let rendered = (spec.render)();
            let committed = load_expected(spec.name);
            assert_eq!(
                rendered.expected, committed,
                "expected_rects.json for {} is stale; re-run the regeneration test",
                spec.name
            );
        }
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `rtk cargo test -p rollshot-app eval::fixture`
Expected: FAIL — `six_intents_are_specified` passes, but `rendered_expected_rects_match_committed_json` fails reading missing files (panics). This confirms the loader compiles and the data is not yet generated.

- [ ] **Step 4: Add a regeneration helper (ignored test)**

Append to the `tests` module in `fixture.rs`:

```rust
    /// Regenerates committed fixture images + expected_rects + meta.
    /// Run manually: `cargo test -p rollshot-app eval::fixture::tests::regenerate_fixtures -- --ignored`
    #[test]
    #[ignore]
    fn regenerate_fixtures() {
        for spec in intent_specs() {
            let dir = fixtures_root().join(spec.name);
            std::fs::create_dir_all(&dir).unwrap();
            let rendered = (spec.render)();
            rendered
                .image
                .save(dir.join("image.png"))
                .expect("save image.png");
            std::fs::write(
                dir.join("expected_rects.json"),
                serde_json::to_string_pretty(&rendered.expected).unwrap(),
            )
            .unwrap();
            let meta = FixtureMeta {
                intent: spec.name.to_string(),
                provider: "anthropic".into(),
                model: "claude-opus-4-8".into(),
                required_capability: spec.required_capability,
                seeded: false,
            };
            std::fs::write(
                dir.join("meta.json"),
                serde_json::to_string_pretty(&meta).unwrap(),
            )
            .unwrap();
        }
    }
```

- [ ] **Step 5: Generate and commit the fixture data**

Run: `rtk cargo test -p rollshot-app eval::fixture::tests::regenerate_fixtures -- --ignored`
Then run: `rtk cargo test -p rollshot-app eval::fixture`
Expected: PASS (both non-ignored tests now pass against committed data).

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/eval/fixture.rs crates/rollshot-app/tests/eval/fixtures/
rtk git commit -m "test(app): add eval fixture types, loader, and six synthetic images"
```

---

### Task 5: Layer 2 — golden-source dry-run and score

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/workbench/eval/layer2.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/eval/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/mod.rs` / `run.rs` if Task 6 visibility promotion has not already been applied.

**Interfaces:**
- Consumes: `prepare_vision_context` (workbench/run.rs, already `pub`), `ImageRect`, the Layer-2 imports.
- Produces: `fn run_golden_source(image: &image::RgbaImage, golden_js: &str) -> Result<Vec<rollshot_image_document::ImageRect>, String>` — validates the JS, runs it via `execute_to_proposal` against a product-prepared host, and returns the `AddRedaction` bounds.

Review amendment: execute Task 6 before this task, or fold Task 6 Step 1 into this task before adding `layer2.rs`. `crate::result_workspace::workbench::run` is currently private unless `workbench/mod.rs` declares it `pub(crate) mod run;`.

- [ ] **Step 1: Add the module declaration**

In `eval/mod.rs` add `pub(crate) mod layer2;`.

- [ ] **Step 2: Write the failing test**

Create `crates/rollshot-app/src/result_workspace/workbench/eval/layer2.rs`:

```rust
use rollshot_automation::{
    execute_to_proposal, validate_source, AutomationInput, CancellationFlag, ExecutionPolicy,
    ProposalContext, ProposedEditKind, Region, ValidationLimits,
};
use rollshot_automation_rquickjs::QuickJsExecutor;
use rollshot_edit_proposal::{ProposalId, ProposedEdit, Provenance, ProvenanceSource};
use rollshot_image_document::ImageRect;

use crate::result_workspace::workbench::run::prepare_vision_context;

pub(crate) fn run_golden_source(
    image: &image::RgbaImage,
    golden_js: &str,
) -> Result<Vec<ImageRect>, String> {
    let (w, h) = image.dimensions();
    let automation = validate_source(golden_js, &ValidationLimits::default())
        .map_err(|e| format!("validate: {e:?}"))?;
    let vision = prepare_vision_context(image).map_err(|e| format!("prepare: {e:?}"))?;

    let input = AutomationInput {
        image_width: w,
        image_height: h,
        region: Some(Region::Full),
        annotations: Vec::new(),
        capability_handles: Default::default(),
    };
    let ctx = ProposalContext {
        proposal_id: ProposalId(1),
        base_document_state_id: 0,
        provenance: Provenance { source: ProvenanceSource::Manual },
    };
    let mut policy = ExecutionPolicy::smart_redaction_default(
        std::time::Duration::from_secs(5),
        16 * 1024 * 1024,
        256 * 1024,
    );
    policy.allowed_edit_kinds.insert(ProposedEditKind::AddRedaction);

    let cancellation = CancellationFlag::new();
    let mut host_guard = vision.host.lock().unwrap();
    let (proposal, _metrics) = execute_to_proposal(
        &QuickJsExecutor,
        &automation,
        &input,
        &ctx,
        &mut *host_guard,
        &policy,
        &cancellation,
    )
    .map_err(|e| format!("execute: {e:?}"))?;

    Ok(proposal
        .candidates
        .into_iter()
        .filter_map(|c| match c.edit {
            ProposedEdit::AddRedaction { bounds } => Some(bounds),
            _ => None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result_workspace::workbench::eval::render::render_url_bar;

    /// A region-feature golden that redacts the top strip where the URL bar
    /// lives. Uses only region features so it runs without the `ocr` feature.
    const TOP_STRIP_GOLDEN: &str = r#"
function main(input) {
  return {
    candidates: [{
      kind: 'addRedaction',
      bounds: { x: 120, y: 14, width: 600, height: 28 },
      confidence: 0.9,
      label: 'url'
    }]
  };
}
"#;

    #[test]
    fn golden_source_produces_candidate_over_url_bar() {
        let f = render_url_bar();
        let cands = run_golden_source(&f.image, TOP_STRIP_GOLDEN).expect("layer2 runs");
        assert_eq!(cands.len(), 1);
        let c = &cands[0];
        assert!((c.x - 120.0).abs() < 1.0 && (c.width - 600.0).abs() < 1.0);
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `rtk cargo test -p rollshot-app eval::layer2`
Expected: FAIL to compile first if `prepare_vision_context` path or `run` module is not `pub(crate)` — if so, confirm `pub fn prepare_vision_context` and that `mod run;` is reachable as `crate::result_workspace::workbench::run`. Then FAIL the assertion only if the golden is wrong. Fix until it compiles and the test asserts.

- [ ] **Step 4: Make `run` reachable + confirm pass**

If the import fails, in `workbench/mod.rs` ensure the module is declared `pub(crate) mod run;` (it is already `mod run;` — change to `pub(crate)` if needed for the eval module path). Re-run.

Run: `rtk cargo test -p rollshot-app eval::layer2`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/
rtk git commit -m "test(app): add eval layer2 golden-source scoring"
```

---

### Task 6: Promote workbench helpers to `pub(crate)` for full-loop reuse

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`

**Interfaces:**
- Produces (visibility only): `pub(crate) fn build_authoring_tool_registry(...)`, `pub(crate) fn canonical_region_feature_catalog(...)`, `pub(crate) fn canonical_ocr_catalog(...)`, `pub(crate) fn authoring_inspection_context(...)`, `pub(crate) fn product_capability_handles(...)`, `pub(crate) struct CanonicalRegionFeatureEntry`, `pub(crate) struct CanonicalOcrEntry`. Signatures are unchanged from their current `fn` form.

- [ ] **Step 1: Change visibility**

In `crates/rollshot-app/src/result_workspace/workbench/run.rs`, change each declaration from `fn`/`struct` to `pub(crate) fn`/`pub(crate) struct` for the symbols listed above (and their fields where the eval module reads them). Leave bodies untouched.

- [ ] **Step 2: Verify nothing else breaks**

Run: `rtk cargo build -p rollshot-app`
Expected: builds. (Widening visibility cannot break existing callers.)

Run: `rtk cargo test -p rollshot-app result_workspace::workbench::run`
Expected: PASS (existing workbench tests unaffected).

- [ ] **Step 3: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/run.rs
rtk git commit -m "refactor(app): expose authoring wiring to crate for eval reuse"
```

---

### Task 7: Cassette types, ordered responder, and add `wiremock`/`sha2` dev-deps

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/workbench/eval/cassette.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/eval/mod.rs`
- Modify: `crates/rollshot-app/Cargo.toml`

**Interfaces:**
- Produces: `CassetteFile { version: u32, metadata: CassetteMeta, attachment: Option<AttachmentMeta>, interactions: Vec<Interaction> }`; `Interaction { request: RecordedRequest, response: RecordedResponse }`; `RecordedRequest { method: String, url_path: String, headers: BTreeMap<String, String>, body_summary: RecordedRequestBody }`; `RecordedResponse { status: u16, headers: BTreeMap<String, String>, sse_body: String }`; `CassetteMeta { recorded_at: String, provider: String, model: String, substitutions: String }`; `AttachmentMeta { media_type: String, width: u32, height: u32, byte_count: u64, sha256: String }`; `struct CassetteResponder` impl `wiremock::Respond`; `fn load_cassette(intent: &str) -> CassetteFile`; `fn sha256_hex(bytes: &[u8]) -> String`.
- Review amendment: cassette JSON must store only redacted request data. Add pure tests proving `authorization`, `x-api-key`, and image base64 do not survive serialization.

- [ ] **Step 1: Add dev-dependencies**

In `crates/rollshot-app/Cargo.toml` under `[dev-dependencies]` add:

```toml
wiremock = "0.6"
sha2 = "0.10"
```

(Match the versions already used elsewhere in the workspace; `rollshot-agent` already depends on `wiremock` — copy its exact version from `crates/rollshot-agent/Cargo.toml`.)

- [ ] **Step 2: Add the module declaration**

In `eval/mod.rs` add `pub(crate) mod cassette;`.

- [ ] **Step 3: Write the failing test**

Create `crates/rollshot-app/src/result_workspace/workbench/eval/cassette.rs`:

```rust
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use wiremock::{Request, Respond, ResponseTemplate};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CassetteFile {
    pub version: u32,
    pub metadata: CassetteMeta,
    #[serde(default)]
    pub attachment: Option<AttachmentMeta>,
    pub interactions: Vec<Interaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CassetteMeta {
    pub recorded_at: String,
    pub provider: String,
    pub model: String,
    pub substitutions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AttachmentMeta {
    pub media_type: String,
    pub width: u32,
    pub height: u32,
    pub byte_count: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Interaction {
    pub request: RecordedRequest,
    pub response: RecordedResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RecordedRequest {
    pub method: String,
    pub url_path: String,
    pub headers: BTreeMap<String, String>,
    pub body_summary: RecordedRequestBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RecordedRequestBody {
    JsonWithoutImage { byte_count: u64, sha256: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RecordedResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub sse_body: String,
}

/// Replays a cassette's interactions in recorded order, one per request.
pub(crate) struct CassetteResponder {
    interactions: Vec<Interaction>,
    cursor: AtomicUsize,
}

impl CassetteResponder {
    pub fn new(interactions: Vec<Interaction>) -> Self {
        Self { interactions, cursor: AtomicUsize::new(0) }
    }

    fn next_interaction(&self) -> Interaction {
        let i = self.cursor.fetch_add(1, Ordering::SeqCst);
        self.interactions.get(i).cloned().unwrap_or_else(|| {
            panic!("cassette exhausted: model call {i} has no recorded interaction")
        })
    }
}

impl Respond for CassetteResponder {
    fn respond(&self, _req: &Request) -> ResponseTemplate {
        let interaction = self.next_interaction();
        ResponseTemplate::new(interaction.response.status)
            .insert_header("content-type", "text/event-stream")
            .set_body_bytes(interaction.response.sse_body.into_bytes())
    }
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn load_cassette(intent: &str) -> CassetteFile {
    let path = super::fixture::fixtures_root().join(intent).join("cassette.json");
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&data).expect("valid cassette.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interaction(body: &str) -> Interaction {
        Interaction {
            request: RecordedRequest {
                method: "POST".into(),
                url_path: "/v1/messages".into(),
                headers: BTreeMap::new(),
                body_summary: RecordedRequestBody::JsonWithoutImage {
                    byte_count: 0,
                    sha256: sha256_hex(b""),
                },
            },
            response: RecordedResponse {
                status: 200,
                headers: BTreeMap::new(),
                sse_body: body.into(),
            },
        }
    }

    #[test]
    fn responder_returns_interactions_in_order() {
        let responder = CassetteResponder::new(vec![
            interaction("a"),
            interaction("b"),
        ]);
        assert_eq!(responder.next_interaction().response.sse_body, "a");
        assert_eq!(responder.next_interaction().response.sse_body, "b");
    }

    #[test]
    #[should_panic(expected = "cassette exhausted")]
    fn responder_panics_clearly_when_exhausted() {
        let responder = CassetteResponder::new(vec![interaction("a")]);
        let _ = responder.next_interaction();
        let _ = responder.next_interaction();
    }

    #[test]
    fn sha256_is_stable() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
```

Add one more pure redaction test before the recorder task lands: build a synthetic request JSON containing an `authorization` header, `x-api-key` header, and an image base64 block; pass it through the redaction helper; serialize the resulting `CassetteFile`; assert the JSON contains neither secret header nor base64 and does contain the committed image `sha256`.

- [ ] **Step 4: Run the test to verify it fails, then passes**

Run: `rtk cargo test -p rollshot-app eval::cassette`
Expected: first FAIL (missing deps / module), then after Steps 1–3 PASS, including ordered replay, exhaustion, SHA-256, and redaction tests.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/Cargo.toml crates/rollshot-app/src/result_workspace/workbench/eval/cassette.rs
rtk git commit -m "test(app): add eval cassette types and ordered wiremock responder"
```

---

### Task 8: Layer 1 — full-loop cassette replay

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/workbench/eval/layer1.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/eval/mod.rs`

**Interfaces:**
- Consumes: `CassetteResponder`/`CassetteFile` (Task 7), the promoted workbench helpers (Task 6), `prepare_vision_context`, `score_candidates`.
- Produces: `async fn replay_full_loop(image: &image::RgbaImage, meta: &FixtureMeta, cassette: &CassetteFile) -> Result<Vec<rollshot_image_document::ImageRect>, String>` — serves the cassette via `wiremock`, builds the product registry, runs `run_with_provider`, and extracts the final proposal's `AddRedaction` bounds.

- [ ] **Step 1: Add the module declaration**

In `eval/mod.rs` add `pub(crate) mod layer1;`.

- [ ] **Step 2: Write the failing test (with a hand-authored self-test cassette)**

Create `crates/rollshot-app/src/result_workspace/workbench/eval/layer1.rs`:

```rust
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use rollshot_agent::domain::{AttachmentDescriptor, AuthorizedModelInput, MediaType, SessionId};
use rollshot_agent::driver::{AgentConfig, AgentRunner, RunTerminalState};
use rollshot_agent::AnthropicAdapter; // re-exported at crate root (see provider_contract.rs)
use rollshot_agent::runtime::{RunBudget, RunCancellation, RunEvent, RunEventSink};
use rollshot_agent::tools::ToolContext;
use rollshot_edit_proposal::ProposedEdit;
use rollshot_image_document::ImageRect;
use wiremock::{Mock, MockServer};

use super::cassette::{CassetteFile, CassetteResponder};
use super::fixture::FixtureMeta;
use crate::result_workspace::workbench::run::{
    authoring_inspection_context, build_authoring_tool_registry, canonical_ocr_catalog,
    canonical_region_feature_catalog, prepare_vision_context, product_capability_handles,
};
use crate::result_workspace::workbench::PayloadMode;

struct NullSink;
impl RunEventSink for NullSink {
    fn emit(&self, _event: RunEvent) {}
}

pub(crate) async fn replay_full_loop(
    image: &image::RgbaImage,
    meta: &FixtureMeta,
    cassette: &CassetteFile,
) -> Result<Vec<ImageRect>, String> {
    let (w, h) = image.dimensions();

    // 1. Serve the cassette in recorded order.
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(CassetteResponder::new(cassette.interactions.clone()))
        .mount(&server)
        .await;
    let adapter = AnthropicAdapter::new("test-key", &server.uri())
        .map_err(|e| format!("adapter: {e:?}"))?;

    // 2. Build the genuine product authoring wiring.
    let vision = prepare_vision_context(image).map_err(|e| format!("prepare: {e:?}"))?;
    let cancellation = RunCancellation::new();
    let tool_ctx = Arc::new(ToolContext::new_with_capability_handles(
        SessionId::new(1),
        String::new(),
        rollshot_automation::ValidationLimits::default(),
        rollshot_automation::ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(5),
            16 * 1024 * 1024,
            256 * 1024,
        ),
        (w, h),
        product_capability_handles(),
        &cancellation,
    ));
    let inspection = authoring_inspection_context(
        PayloadMode::FullScreenshot,
        &canonical_region_feature_catalog(w, h),
        &canonical_ocr_catalog(w, h),
    );
    let host = vision.host.clone() as Arc<StdMutex<dyn rollshot_automation::AutomationHost>>;
    let executor: Arc<dyn rollshot_automation::AutomationExecutor> =
        Arc::new(rollshot_automation_rquickjs::QuickJsExecutor);
    let registry = build_authoring_tool_registry(tool_ctx.clone(), executor, host, inspection)
        .map_err(|e| format!("registry: {e:?}"))?;

    // 3. Build the model input with the screenshot attachment.
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| format!("png: {e}"))?;
    let descriptor = AttachmentDescriptor {
        media_type: MediaType::Png,
        width: w,
        height: h,
        byte_count: png.len() as u64,
    };
    let input = AuthorizedModelInput::new(
        meta.provider.clone(),
        meta.model.clone(),
        format!("Redact the {} in this screenshot.", meta.intent),
        vec![descriptor],
        vec![png],
    )
    .map_err(|e| format!("input: {e:?}"))?;

    // 4. Run the full loop against the replayed cassette.
    let runner = AgentRunner::new(AgentConfig::default());
    let mut session = rollshot_agent::domain::AgentSession::new(SessionId::new(1));
    let terminal = runner
        .run_with_provider(
            input,
            &mut session,
            &registry,
            RunBudget::unlimited(),
            &cancellation,
            &NullSink,
            &tool_ctx,
            &adapter,
        )
        .await;

    let proposal = match terminal {
        RunTerminalState::ReadyForReview(r) => r.proposal,
        other => return Err(format!("non-terminal-ready: {other:?}")),
    };
    Ok(proposal
        .candidates
        .into_iter()
        .filter_map(|c| match c.edit {
            ProposedEdit::AddRedaction { bounds } => Some(bounds),
            _ => None,
        })
        .collect())
}
```

> The Layer-1 self-test that actually drives this needs a hand-authored cassette, created in Task 9. This task ends when the module compiles.

- [ ] **Step 3: Verify it compiles**

Run: `rtk cargo test -p rollshot-app eval::layer1 --no-run`
Expected: compiles. Fix any import-path or trait-object coercion errors (e.g. `SessionId`/`AgentSession` paths — confirm against `rollshot_agent::domain`).

- [ ] **Step 4: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/eval/
rtk git commit -m "test(app): add eval layer1 full-loop replay runner"
```

---

### Task 9: Self-test fixture + both-layer cases

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/workbench/eval/cases.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/eval/mod.rs`
- Create (data): `crates/rollshot-app/tests/eval/fixtures/selftest_region/` (`image.png`, `expected_rects.json`, `meta.json`, `golden_source.js`, `cassette.json`).

**Interfaces:**
- Consumes: everything above.
- Produces: the deterministic `#[test]`/`#[tokio::test]` gate cases.

- [ ] **Step 1: Add the module declaration**

In `eval/mod.rs` add `#[cfg(test)] mod cases;`.

- [ ] **Step 2: Author the self-test golden source**

Create `crates/rollshot-app/tests/eval/fixtures/selftest_region/golden_source.js` — a region-feature-only detector (no OCR) so the self-test runs in default CI:

```javascript
function main(input) {
  return {
    candidates: [{
      kind: 'addRedaction',
      bounds: { x: 120, y: 14, width: 600, height: 28 },
      confidence: 0.9,
      label: 'url'
    }]
  };
}
```

- [ ] **Step 3: Write the Layer-2 + scoring cases (deterministic, no cassette)**

Create `crates/rollshot-app/src/result_workspace/workbench/eval/cases.rs`:

```rust
use super::fixture::{load_expected, load_image};
use super::layer2::run_golden_source;
use super::scoring::{score_candidates, Thresholds};

const SELFTEST: &str = "selftest_region";

fn golden_for(intent: &str) -> String {
    let path = super::fixture::fixtures_root().join(intent).join("golden_source.js");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn layer2_selftest_golden_passes_scoring() {
    let image = load_image(SELFTEST);
    let expected = load_expected(SELFTEST);
    let cands = run_golden_source(&image, &golden_for(SELFTEST)).expect("layer2 runs");
    let report = score_candidates(&expected, &cands, &Thresholds::lenient());
    assert!(report.passed(), "selftest golden failed scoring: {:?}", report.gate_failures);
}

#[test]
fn layer2_bad_golden_fails_scoring() {
    let image = load_image(SELFTEST);
    let expected = load_expected(SELFTEST);
    // A golden that redacts the wrong place: zero coverage of the expected rect.
    let bad = r#"function main(input){return {candidates:[{kind:'addRedaction',bounds:{x:0,y:150,width:10,height:10},confidence:0.5,label:'x'}]};}"#;
    let cands = run_golden_source(&image, bad).expect("layer2 runs");
    let report = score_candidates(&expected, &cands, &Thresholds::lenient());
    assert!(!report.passed(), "bad golden unexpectedly passed");
}
```

- [ ] **Step 4: Generate the self-test fixture data**

Add an `#[ignore]` regeneration test to `cases.rs` that writes the self-test image (reuse the URL-bar renderer), its expected rect, and meta:

```rust
#[test]
#[ignore]
fn regenerate_selftest_fixture() {
    use super::fixture::{fixtures_root, FixtureMeta, RequiredCapability};
    use super::render::render_url_bar;
    let dir = fixtures_root().join(SELFTEST);
    std::fs::create_dir_all(&dir).unwrap();
    let rendered = render_url_bar();
    rendered.image.save(dir.join("image.png")).unwrap();
    std::fs::write(
        dir.join("expected_rects.json"),
        serde_json::to_string_pretty(&rendered.expected).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join("meta.json"),
        serde_json::to_string_pretty(&FixtureMeta {
            intent: "url_bar".into(),
            provider: "anthropic".into(),
            model: "claude-opus-4-8".into(),
            required_capability: RequiredCapability::RegionFeatures,
            seeded: true,
        })
        .unwrap(),
    )
    .unwrap();
}
```

Run: `rtk cargo test -p rollshot-app eval::cases::regenerate_selftest_fixture -- --ignored`

- [ ] **Step 5: Run the deterministic cases**

Run: `rtk cargo test -p rollshot-app eval::cases::layer2`
Expected: PASS (golden passes, bad golden fails).

- [ ] **Step 6: Author a hand-written self-test cassette and the Layer-1 case**

Hand-author `crates/rollshot-app/tests/eval/fixtures/selftest_region/cassette.json` containing the minimal ordered Anthropic SSE turns that drive the loop to `ReadyForReview`: one assistant turn that calls `replace_source` with the golden JS, then `validate_source`, `dry_run`, and `submit_for_review`. Use the existing `crates/rollshot-agent/tests/fixtures/provider_streams.json` chunks as the SSE shape reference (verbatim event framing). Then add to `cases.rs`:

```rust
#[tokio::test]
async fn layer1_selftest_replay_reaches_ready_and_scores() {
    use super::cassette::load_cassette;
    use super::fixture::load_meta;
    use super::layer1::replay_full_loop;
    use super::scoring::{score_candidates, Thresholds};

    let image = load_image(SELFTEST);
    let meta = load_meta(SELFTEST);
    let cassette = load_cassette(SELFTEST);
    let cands = replay_full_loop(&image, &meta, &cassette)
        .await
        .expect("layer1 replay reaches ReadyForReview");
    let report = score_candidates(&load_expected(SELFTEST), &cands, &Thresholds::lenient());
    assert!(report.passed(), "layer1 scoring failed: {:?}", report.gate_failures);
}
```

> Review amendment: this `#[tokio::test]` must be non-ignored and passing before final verification. If hand-framing the SSE proves fiddly, implement Task 11's recorder first, record `selftest_region`, then return here and enable the Layer-1 self-test. The Layer-2 cases alone do not satisfy the full-loop CI-gate goal.

- [ ] **Step 7: Run all deterministic eval tests**

Run: `rtk cargo test -p rollshot-app eval`
Expected: PASS (scoring, render, fixture, layer2, cassette, cases).

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/eval/ crates/rollshot-app/tests/eval/fixtures/selftest_region/
rtk git commit -m "test(app): add eval self-test fixture and both-layer cases"
```

---

### Task 10: Per-fixture gate iteration with OCR-aware skipping

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/eval/cases.rs`

**Interfaces:**
- Consumes: `intent_specs`, `load_*`, `run_golden_source`, `score_candidates`.

- [ ] **Step 1: Write the iteration test**

Add to `cases.rs`:

```rust
#[test]
fn layer2_gate_over_all_present_fixtures() {
    use super::fixture::{intent_specs, RequiredCapability};
    use super::scoring::{score_candidates, Thresholds};

    let ocr_enabled = cfg!(feature = "ocr");
    for spec in intent_specs() {
        let meta = load_meta(spec.name);
        if spec.required_capability == RequiredCapability::Ocr && !ocr_enabled {
            eprintln!("SKIP eval fixture {} (ocr feature disabled)", spec.name);
            continue;
        }
        let golden_path = super::fixture::fixtures_root().join(spec.name).join("golden_source.js");
        if !golden_path.exists() {
            if meta.seeded || std::env::var_os("CI").is_some() {
                panic!("seeded eval fixture {} is missing golden_source.js", spec.name);
            }
            eprintln!("SKIP eval fixture {} (golden not yet seeded)", spec.name);
            continue;
        }
        let image = load_image(spec.name);
        let expected = load_expected(spec.name);
        let golden = std::fs::read_to_string(&golden_path).unwrap();
        let cands = run_golden_source(&image, &golden)
            .unwrap_or_else(|e| panic!("{} layer2: {e}", spec.name));
        let report = score_candidates(&expected, &cands, &Thresholds::lenient());
        assert!(report.passed(), "{} failed gate: {:?}", spec.name, report.gate_failures);
    }
}
```

> The skip for not-yet-seeded goldens keeps CI green before live seeding while still gating every seeded fixture. The skip is logged (never silent), per the Global Constraints.

- [ ] **Step 2: Run**

Run: `rtk cargo test -p rollshot-app eval::cases::layer2_gate_over_all_present_fixtures -- --nocapture`
Expected: PASS, with SKIP lines for fixtures whose `meta.seeded == false` and whose golden is not yet seeded. Under `CI=true`, a missing seeded artifact is a hard failure.

- [ ] **Step 3: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/eval/cases.rs
rtk git commit -m "test(app): gate all seeded eval fixtures with ocr-aware skip"
```

---

### Task 11: Record-mode spike, then recorder implementation

This task carries a genuine runtime unknown (does teed SSE replay byte-faithfully through rig's parser?). Do the spike first.

**Files:**
- Create: `crates/rollshot-app/src/result_workspace/workbench/eval/record.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/eval/mod.rs`, `crates/rollshot-app/Cargo.toml`

**Interfaces:**
- Produces: `async fn record_cassette(intent: &str, real_base_url: &str, api_key: &str) -> Result<(), String>` — runs the live loop through a tee-ing reverse-proxy and writes a redacted `cassette.json`.

- [ ] **Step 1: Add pure redaction tests before live recording**

In `eval/cassette.rs` or `eval/record.rs`, add tests for the pure redaction helpers before any live proxy work:

- input request headers include `authorization` and `x-api-key`; serialized `CassetteFile` output does not contain either header or value.
- input request body includes an image base64 block; serialized output does not contain the base64 string and does contain `AttachmentMeta { media_type, width, height, byte_count, sha256 }`.
- response SSE body is preserved byte-for-byte.

Run: `rtk cargo test -p rollshot-app redaction`
Expected: FAIL before helper implementation, PASS after implementation.

- [ ] **Step 2: Spike — confirm tee-and-replay fidelity**

REQUIRED SUB-SKILL: Use the `rollshot-run-spike` skill. Spike goal: stand up a minimal reverse-proxy (hyper or `reqwest` + `tokio`) at `http://127.0.0.1:0` that forwards one `POST /v1/messages` to the real Anthropic API, tees the raw streamed bytes to a `String`, returns them to the caller, and then confirm that feeding those exact bytes back through a `wiremock` `MockServer` + `AnthropicAdapter` yields the same `ModelStreamEvent`s. Success criterion: the parsed events from the live call equal the parsed events from the replayed bytes. Capture findings (chunk-boundary handling, required headers) in the spike notes. Run gated by `ANTHROPIC_API_KEY` + `--ignored`.

- [ ] **Step 3: Implement the recorder using the spike's validated approach**

Add `pub(crate) mod record;` to `eval/mod.rs`, add any proxy dep the spike validated (e.g. `hyper`/`reqwest`) to `[dev-dependencies]`, and implement `record_cassette` per the spike: build the product registry + input exactly as Layer 1 (Task 8) but point the adapter at the proxy, run `run_with_provider`, collect the teed per-turn SSE into `Interaction`s, redact (strip auth headers; replace the first request's image block with `AttachmentMeta` via `sha256_hex` of the committed PNG), and write `cassette.json` with provenance.

- [ ] **Step 4: Gate it behind env**

The public entry is an `#[ignore]` test `record_one_fixture` reading `ROLLSHOT_RECORD_EVAL`, the intent name, and `ANTHROPIC_API_KEY` from env; it calls `record_cassette`. It never runs in CI.

- [ ] **Step 5: Verify the recorder round-trips on the self-test**

Run (with a key): `ROLLSHOT_RECORD_EVAL=1 EVAL_INTENT=selftest_region rtk cargo test -p rollshot-app eval::record::record_one_fixture -- --ignored --nocapture`
Then run the Layer-1 case from Task 9 Step 6 against the recorded cassette.
Expected: Layer-1 case PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/Cargo.toml crates/rollshot-app/src/result_workspace/workbench/eval/record.rs crates/rollshot-app/src/result_workspace/workbench/eval/mod.rs
rtk git commit -m "test(app): add env-gated eval cassette recorder"
```

---

### Task 12: Documentation + follow-up seeding instructions

**Files:**
- Create: `docs/smart-redaction-eval.md`
- Modify: `README.md` (developer-tooling pointer)
- No live-model fixture data is required in this MVP beyond the deterministic `selftest_region` cassette. Broad six-intent provider seeding is explicitly deferred to a follow-up.

- [ ] **Step 1: Write `docs/smart-redaction-eval.md`**

Mirror `docs/bench.md` structure. Cover, with exact commands: the two-layer model; how to add a fixture (renderer + `regenerate_fixtures`); how to record a cassette from a live model (`ROLLSHOT_RECORD_EVAL=1` + `ANTHROPIC_API_KEY`, the reverse-proxy, the redaction guarantees); how to review the extracted `golden_source.js`; re-recording after prompt/tool changes; and that OCR fixtures only run under `--features ocr`. State plainly that cassettes contain no raw screenshot (only attachment metadata + sha256).

- [ ] **Step 2: Add the README pointer**

In `README.md`, under the developer-tooling section, add one line pointing to `docs/smart-redaction-eval.md` for the Smart Redaction evaluation harness.

- [ ] **Step 3: Document the deferred six-fixture seeding workflow**

In `docs/smart-redaction-eval.md`, include the follow-up commands for seeding the six provider-backed fixtures, but do not require running them for this MVP. The documented follow-up is:

```bash
for intent in url_bar bookmarks desktop_folders emails names account_ids; do
  ROLLSHOT_RECORD_EVAL=1 EVAL_INTENT=$intent ANTHROPIC_API_KEY=... \
    rtk cargo test -p rollshot-app --features ocr eval::record::record_one_fixture -- --ignored --nocapture
done
rtk cargo test -p rollshot-app --features ocr eval
```

Expected for the follow-up plan: the full OCR-enabled gate passes over all six seeded fixtures, and each seeded fixture flips `meta.seeded` to `true`.

- [ ] **Step 4: Commit**

```bash
rtk git add docs/smart-redaction-eval.md README.md
rtk git commit -m "docs: add smart redaction eval harness guide"
```

---

### Task 13: Final verification

- [ ] **Step 1: Default build gate**

Run: `rtk cargo test -p rollshot-app eval`
Expected: PASS; OCR fixtures logged as SKIP.

- [ ] **Step 2: OCR-lane gate**

Run: `rtk cargo test -p rollshot-app --features ocr eval`
Expected: PASS over all seeded fixtures (`selftest_region` in this MVP) and logged skips for unseeded six-intent fixtures until the follow-up seeding plan flips `meta.seeded` to `true`.

- [ ] **Step 3: Format + lint**

Run: `rtk cargo fmt --check`
Run: `rtk cargo clippy -p rollshot-app --all-targets -- -D warnings`
Run: `rtk cargo clippy -p rollshot-app --all-targets --features ocr -- -D warnings`
Expected: clean.

- [ ] **Step 4: Confirm no real screenshots committed**

Confirm every `crates/rollshot-app/tests/eval/fixtures/*/image.png` is renderer-generated and every `cassette.json` contains `attachment` metadata with a `sha256` and no base64 image body.

- [ ] **Step 5: Confirm CI coverage**

Read `.github/workflows/ci.yml` and `.github/workflows/ci-ocr.yml`.
Expected: existing lanes run the default eval tests and an OCR-enabled `rollshot-app` eval path. If not, add the smallest workflow edit that runs:

```bash
rtk cargo test -p rollshot-app eval
rtk cargo test -p rollshot-app --features ocr eval
```

---

## Self-Review

**Spec coverage:**
- Two layers (cassette replay + golden-source scoring) → Tasks 5, 8, 9. ✓
- Synthetic images, six intents → Tasks 2, 3, 4. ✓
- Fixture format (`meta.json`, `image.png`, `expected_rects.json`, `cassette.json`, `golden_source.js`) → Tasks 4, 7, 9, 12. `selftest_region` is seeded in this MVP; six provider-backed OCR fixtures remain unseeded until the follow-up. ✓
- Cassette redaction (auth strip + image→metadata+sha256) → Task 11. ✓
- Scoring metrics (coverage hard gate, false-positive hard gate, source validity) → Task 1; reported-only signals are surfaced via `ScoreReport`/run terminal (turns/candidate-count available from `UsageSnapshot`; not gated). ✓
- CI placement / OCR-aware skip → Task 10, Task 13. ✓
- Lives as crate-internal test module in `rollshot-app` → all tasks; visibility promotion Task 6. ✓
- Docs + README → Task 12. ✓
- Record mode env-gated + missing-cassette-fails → Tasks 10 (skip logged for not-yet-seeded), 11 (env gate). Note: "missing cassette is a hard failure in CI" applies once a fixture is seeded; before seeding, the logged skip is intentional and the constraint is satisfied because unseeded fixtures have no cassette to miss.
- Broad six-intent live seeding → explicitly deferred in `NOT in scope`; docs include the follow-up commands. ✓

**Placeholder scan:** Task 11 keeps a real spike before recorder implementation. Task 9 Step 6 is no longer optional: the Layer-1 self-test must be non-ignored and passing before final verification, either with a hand-authored cassette or with the recorder-produced self-test cassette.

**Type consistency:** `ImageRect` (`x/y/width/height: f32`) is the candidate geometry across scoring/layer1/layer2; `ExpectedRect` adds `label`; `run_golden_source`/`replay_full_loop` both return `Vec<ImageRect>`; `score_candidates(&[ExpectedRect], &[ImageRect], &Thresholds)` is called identically in `layer2` tests and `cases`. `RequiredCapability` is used consistently in `fixture` and `cases`. Provider/model strings flow `meta.json` → `FixtureMeta` → `AuthorizedModelInput::new`.
