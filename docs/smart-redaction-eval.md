# Smart Redaction Evaluation Harness

Test-only evaluation gate for the Phase D smart-redaction pipeline. Scores
redaction candidates two ways — full-loop cassette replay and extracted
golden-source geometry — against synthetic-image fixtures with known ground
truth. Designed to catch regressions in the automation host, executor, and
model-prompt contract without requiring a live API key or real screenshots.

## Architecture

The harness uses a **two-layer model**:

```text
 Layer 1 — Cassette replay (full loop)
 ┌─────────────────────────────────────────────────────────────┐
 │  synthetic image.png + meta.json                            │
 │       │                                                     │
 │       ▼                                                     │
 │  AgentRunner → AnthropicAdapter → wiremock CassetteResponder│
 │       │            (cassette.json replays SSE responses)     │
 │       ▼                                                     │
 │  product authoring tool registry (replace_source,            │
 │    validate_source, dry_run, submit_for_review)              │
 │       │                                                     │
 │       ▼                                                     │
 │  RunTerminalState::ReadyForReview → proposal.candidates     │
 │       │                                                     │
 │       ▼                                                     │
 │  score_candidates(expected_rects, candidates, thresholds)   │
 └─────────────────────────────────────────────────────────────┘

 Layer 2 — Golden-source scoring (isolated)
 ┌─────────────────────────────────────────────────────────────┐
 │  synthetic image.png + golden_source.js                     │
 │       │                                                     │
 │       ▼                                                     │
 │  validate_source → execute_to_proposal (QuickJsExecutor)    │
 │       │                                                     │
 │       ▼                                                     │
 │  score_candidates(expected_rects, candidates, thresholds)   │
 └─────────────────────────────────────────────────────────────┘
```

**Layer 1** replays the full agent loop against a recorded cassette. It
exercises the prompt, tool registry, Anthropic adapter, and executor end to
end. The cassette is served by a `wiremock::MockServer` that returns
pre-recorded SSE responses in order.

**Layer 2** bypasses the agent loop entirely. It validates and executes the
extracted `golden_source.js` directly through the automation executor. This
isolates the geometry logic from model variance and is the primary regression
gate.

Both layers feed candidates into the same `score_candidates()` function, which
checks **coverage** (fraction of expected area covered) and **false-positive
ratio** (candidate area outside expected bounds).

## Fixture layout

Each fixture lives under `crates/rollshot-app/tests/eval/fixtures/<intent>/`:

```text
<intent>/
├── image.png            # synthetic screenshot (rendered by code, not captured)
├── meta.json            # intent name, provider, model, required_capability, seeded
├── expected_rects.json  # ground-truth redaction rectangles
├── golden_source.js     # extracted automation source (Layer 2 input)
└── cassette.json        # recorded model interactions (Layer 1 input)
```

The `selftest_region` fixture is the MVP deterministic cassette. It uses
region-only features (no OCR) and has all five files committed.

Six additional provider-backed intents are defined:
`url_bar`, `bookmarks`, `desktop_folders`, `emails`, `names`, `account_ids`.
These require the `ocr` feature and are seeded via the deferred workflow
described below.

## Running the eval

```bash
# Full gate (region-only fixtures, no OCR required).
rtk cargo test -p rollshot-app eval

# With OCR enabled (includes the six provider-backed intents).
rtk cargo test -p rollshot-app --features ocr eval

# Specific layer-2 test only.
rtk cargo test -p rollshot-app eval::cases::layer2_selftest_golden_passes_scoring

# Specific layer-1 test only (async, replays the full cassette).
rtk cargo test -p rollshot-app eval::cases::layer1_selftest_replay_reaches_ready_and_scores
```

## Adding a new fixture

### Step 1: Add or update the renderer

Each fixture has a render function in `crates/rollshot-app/src/result_workspace/workbench/eval/render.rs`.
The renderer produces a synthetic `RgbaImage` and a `Vec<ExpectedRect>` — the
ground truth. To add a new intent:

1. Write a `render_<intent>()` function in `render.rs` that returns a
   `RenderedFixture`.
2. Add an entry to `intent_specs()` in `fixture.rs` with the intent name,
   required capability, and render function pointer.

### Step 2: Regenerate committed fixtures

```bash
rtk cargo test -p rollshot-app eval::fixture::tests::regenerate_fixtures -- --ignored
```

This writes `image.png`, `expected_rects.json`, and `meta.json` for every
intent. The `meta.json` starts with `"seeded": false` until the full cassette
and golden source are committed.

### Step 3: Record a cassette from a live model

Recording uses the `[provider]` section in Rollshot's platform config file
(`dirs::config_dir()/rollshot/config.toml`) and resolves the configured key
source at runtime. See `docs/config.md` for the supported provider parameters.
Configure the app provider settings first, then run the recorder. It talks to
the configured Anthropic-compatible endpoint through a local reverse-proxy that
captures every request/response pair.

```bash
ROLLSHOT_RECORD_EVAL=1 EVAL_INTENT=<intent> \
  rtk cargo test -p rollshot-app --features ocr eval::record::record_one_fixture -- --ignored --nocapture
```

The reverse-proxy (`TeeProxy` in `record.rs`) binds to `127.0.0.1:0` and
forwards all traffic to `https://api.anthropic.com`. It records:

- Request headers (authorization and x-api-key are stripped at write time)
- Request body (image base64 is replaced with attachment metadata + sha256)
- Response status and headers (set-cookie is stripped)
- Full SSE response body

**Cassettes contain no raw screenshot.** The `attachment` field stores only
media type, dimensions, byte count, and sha256. The `body_summary` field
replaces image base64 with `json_without_image` containing byte count and
sha256. Auth headers are stripped. The resulting `cassette.json` is safe to
commit.

The recorded cassette is written to `tests/eval/fixtures/<intent>/cassette.json`.

### Step 4: Extract and review golden_source.js

The cassette contains the model's tool calls. Extract the final
`replace_source` payload to get the golden automation source:

1. Read `cassette.json` and find the last `replace_source` tool_use in the SSE
   body.
2. Copy the `source` field from the tool input JSON.
3. Save it as `golden_source.js` in the fixture directory.

Review the extracted source for:

- Correct redaction bounds (compare against `expected_rects.json`)
- No hallucinated regions outside the expected area
- Reasonable confidence values

### Step 5: Mark as seeded

Update `meta.json` to `"seeded": true` once both `golden_source.js` and
`cassette.json` are committed. Seeded fixtures gate CI — if
`golden_source.js` is missing for a seeded fixture, the test panics.

## Re-recording after prompt/tool changes

When the system prompt, tool definitions, or authoring flow change, existing
cassettes become stale (the model's recorded responses no longer match what the
new prompt would produce). Re-record affected fixtures:

```bash
for intent in selftest_region url_bar bookmarks desktop_folders emails names account_ids; do
  ROLLSHOT_RECORD_EVAL=1 EVAL_INTENT=$intent \
    rtk cargo test -p rollshot-app --features ocr eval::record::record_one_fixture -- --ignored --nocapture
done
```

After re-recording, re-extract `golden_source.js` from each new cassette and
verify the scoring gate still passes:

```bash
rtk cargo test -p rollshot-app --features ocr eval
```

## OCR feature gating

Fixtures declare a `required_capability` in `meta.json`:

- `region_features` — runs without the `ocr` feature (uses visual region
  features only)
- `ocr` — requires `--features ocr`; skipped when the feature is disabled

The `layer2_gate_over_all_present_fixtures` test checks
`cfg!(feature = "ocr")` and skips OCR-gated fixtures with a message when the
feature is off. On CI, if a seeded fixture is missing its golden source, the
test panics instead of silently skipping.

## Scoring thresholds

The current thresholds are **lenient** (MVP calibration):

| Metric | Threshold |
|---|---|
| `min_coverage` | ≥ 0.6 (60% of expected area must be covered) |
| `max_false_positive_ratio` | ≤ 1.0 (candidate area outside expected ≤ expected area) |

These will tighten as the model and prompts improve. Thresholds are defined in
`scoring.rs::Thresholds::lenient()`.

## Deferred: six-fixture seeding workflow

The six provider-backed intents (`url_bar`, `bookmarks`, `desktop_folders`,
`emails`, `names`, `account_ids`) are defined but not yet seeded with live
cassettes. To seed them in a follow-up:

```bash
for intent in url_bar bookmarks desktop_folders emails names account_ids; do
  ROLLSHOT_RECORD_EVAL=1 EVAL_INTENT=$intent \
    rtk cargo test -p rollshot-app --features ocr eval::record::record_one_fixture -- --ignored --nocapture
done
rtk cargo test -p rollshot-app --features ocr eval
```

Expected outcome for the follow-up plan:

- The full OCR-enabled gate passes over all six seeded fixtures.
- Each seeded fixture flips `meta.seeded` to `true`.
- The `layer2_gate_over_all_present_fixtures` test exercises all six intents
  under `--features ocr` without skipping.

## Phase E improve-loop coverage

Phase E does not require live cassette seeding before implementation. The first
gate is deterministic app coverage for two correction modes:

- overfire: rejected candidate evidence is fed into an improve run;
- miss: manually added candidate evidence is fed into an improve run.

Provider-backed improve cassettes should be recorded after the Phase E prompt
and correction-evidence format stabilize.

## Known limitations

- **Synthetic images only.** Fixtures use programmatically rendered images, not
  real screenshots. They test the pipeline contract, not visual fidelity on
  real-world content.
- **Cassettes are prompt-version-specific.** Any change to the system prompt,
  tool schemas, or authoring flow invalidates existing cassettes. Re-record
  after such changes.
- **Lenient thresholds.** The MVP thresholds are intentionally loose. They will
  tighten as the pipeline matures and more fixtures are seeded.
- **CI gate active for selftest fixture.** The eval suite runs in CI via
  `cargo test -p rollshot-app eval` (default lane) and
  `cargo test -p rollshot-app --features ocr eval` (OCR lane). The six
  provider-backed fixtures are skipped until seeded; thresholds will tighten
  as fixtures stabilize.
