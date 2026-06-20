# Rig 0.39 Integration Spike - Findings

## Status

- Lifecycle: active
- Decision owner: TBD
- Started: 2026-06-20
- Last updated: 2026-06-20

## Decision

Can Rollshot drive Rig 0.39.x as a sans-IO `AgentRun` behind its own provider
facade with budgets/cancellation/usage accounting, at the workspace MSRV floor
(1.89), or must we hand-roll a provider trait?

## Environment

- OS: Linux 6.8.0-124-generic (x86_64)
- Rust stable: 1.89.0 (workspace floor)
- rig-core: 0.39.0 (crates.io)
- Spike crate: `spikes/rig-agent/` — isolated `[workspace]`, never in root workspace
- Test mode: all scripted/fake model — no network, no API key

## Risk Results

| Risk | Gate | Evidence | Result | Notes |
|---|---|---|---|---|
| Manual `AgentRun` driving without `agent.prompt()` | hard | automated | **PASS** | `next_step()` / `model_response()` / `tool_results()` cycle confirmed; 3-turn sequence drives cleanly |
| Tool schema + structured tool-call normalization | soft | automated | **PASS** | Tool call arrives with fully parsed JSON args; field access verified |
| Cancellation — clean teardown, no panic | hard | automated | **PASS** | Both `tokio::time::timeout` drop and `CancellationToken` cancel cleanly; no panic |
| Usage accounting per response | soft | automated | **PASS** | Injected usage aggregates correctly (30 in + 12 out across 2 turns); mid-run `run.usage()` also works |
| Multimodal message construction | soft | compile + automated | **PASS** | `UserContent::image_raw()` + text builds and serializes; no `image` feature gate needed |
| rig MSRV — Rust 1.85 | soft | compile | **FAIL** (expected) | `icu_*` transitive deps require rustc 1.86 |
| rig MSRV — Rust 1.86 | soft | compile | **FAIL** | `rig-derive` E0658: let-chains require 1.88 |
| rig MSRV — Rust 1.87 | soft | compile | **FAIL** | `rig-derive` E0658: let-chains require 1.88 |
| rig MSRV — Rust 1.88 | soft | compile | **PASS** | Full build succeeds; measured MSRV = **1.88** |
| rig MSRV — Rust 1.89 (workspace floor) | soft | compile | **PASS** | Full build succeeds at workspace floor — no conflict |
| Privacy-safe tracing — no prompt/response content leak | soft | automated | **PASS** | At `RUST_LOG=trace`, rig's `AgentRun` emits **zero** tracing events. No prompt or response text leaks. |
| `RollshotModel` facade — provider swap at runtime | soft | automated | **PASS** | Two scripted providers swap behind `dyn RollshotModel`; different args per provider confirmed |
| Live vision round-trip (provider-specific tool + image) | soft | — | **UNTESTED (optional/manual)** | Requires API key and user consent; Step 8 not executed |
| macOS parity (build + CI) | soft | — | **UNTESTED — pending controller CI** | Linux evidence only |

## Observations

### Step 1 — Scaffold

Crate created at `spikes/rig-agent/` with isolated `[workspace]`. Dependencies:
`rig-core = "0.39"`, `tokio`, `serde`, `serde_json`, `tracing`, `tracing-subscriber`,
`tokio-util`. No `image` feature needed — `UserContent::Image` is unconditionally
available. `rig_core::agent::PromptResponse` is publicly re-exported from `rig_core::agent`
(the `prompt_request` module is `pub(crate)` only — an important path detail).

```
cargo build  →  171 crates compiled, success
```

### Step 2 — MSRV Probe

Commands run from `spikes/rig-agent/`:

```
cargo +1.85.0 build  →  FAIL: icu_* transitive deps require rustc 1.86
cargo +1.86.0 build  →  FAIL: rig-derive E0658 (let-chains, require 1.88)
cargo +1.87.0 build  →  FAIL: rig-derive E0658 (let-chains, require 1.88)
cargo +1.88.0 build  →  PASS: full compile (28.72s)
cargo +1.89.0 build  →  PASS: full compile (workspace floor — free)
```

**Measured MSRV: 1.88.** rig-core's `Cargo.toml` has no `rust-version` field. The
`edition = "2024"` workspace requires >= 1.85 for resolution, but the actual floor is
set by `rig-derive`'s use of let-chains (stabilised in Rust 1.88). The workspace floor
(1.89) sits above rig's MSRV — **no MSRV conflict**.

Cross-cut input for Task 6: rig 0.39 contributes no MSRV pressure beyond the workspace floor.

### Step 3 — Manual Multi-Turn Driving (HARD GATE)

Test: `step3_manual_multi_turn_driving` in `tests/driver.rs`. PASS.

Protocol confirmed:
1. `AgentRun::new(prompt).max_turns(N)` — creates the machine.
2. `run.next_step()` → `CallModel { turn, .. }` — driver calls model.
3. `run.model_response(ModelTurn { ... })` → `ModelTurnOutcome::Continue` — feeds result.
4. `run.next_step()` → `CallTools { calls }` — driver executes tools.
5. `run.tool_results(vec![UserContent::tool_result(...)])` — feeds results.
6. Repeat until `AgentRunStep::Done(response)`.

Three-turn sequence (inspect_ocr → replace_automation_source → final text) drives
cleanly. `agent.prompt()` was **never called**. The machine is synchronous and zero-async;
no async runtime is needed for the core driving loop.

### Step 4 — Tool Schema + Normalization

Test: `step4_tool_schema_and_normalization`. PASS.

`inspect_ocr{region: "top_half", max_results: 10}` arrives as a `ToolCall` with
`function.arguments` as `serde_json::Value`. Direct field access works:
`tc.function.arguments["region"].as_str()` → `"top_half"`. The driver (not rig) is
responsible for runtime schema enforcement against a `ToolDefinition`.

### Step 5 — Cancellation

Two tests: `step5_cancellation_via_timeout_drop` and `step5b_cancellation_via_token`. Both PASS.

**Timeout drop**: wraps the run in `tokio::time::timeout(50ms)`. Simulated model call
(`tokio::time::sleep(9999s)`) is dropped when timeout fires. No panic.

**CancellationToken**: token cancelled after 20ms. `tokio::select!` on model call detects
cancellation, returns error with current turn count. No panic.

Rig's sans-IO `AgentRun` drops cleanly when the enclosing async future is cancelled.
No internal tasks to leak; no background work to join.

### Step 6 — Usage Accounting + Multimodal

**Usage**: turn 1 injects `{10 in, 5 out}`, turn 2 injects `{20 in, 7 out}`.
`response.usage` = `{30 in, 12 out}`. `run.usage()` mid-run matches. All per-response
and cumulative token counts are readable without any provider-specific parsing.

**Multimodal**: `UserContent::image_raw(png_bytes, Some(ImageMediaType::PNG), None)` +
`UserContent::text(...)` combined into `Message::User` via `OneOrMany::many`. Serialises
to JSON with `"type": "image"` and `"media_type": "png"`. No `image` feature gate needed.
Provider-specific image encoding (base64 wire format, size limits) is UNTESTED here.

### Step 7 — Privacy-Safe Tracing + RollshotModel Facade

**Tracing**: at `RUST_LOG=trace`, rig's `AgentRun` (sans-IO) emits **zero** tracing
events. The HTTP provider implementations in rig-core do add telemetry spans, but since
Rollshot drives `AgentRun` directly (bypassing provider HTTP clients), those spans never
fire. Privacy strategy: use `EnvFilter` with `rollshot=<level>` to suppress all rig-origin
spans if live provider clients are ever added. No suppression is needed on the AgentRun path.

**Facade**: `RollshotModel` trait with `dyn` dispatch. Two scripted providers
(`ProviderAlpha`, `ProviderBeta`) swap at runtime; different JSON tool args per provider
confirmed. The real implementation replaces `scripted_turn` with an `async fn complete()`
returning a `ModelTurn` assembled from the provider's response.

## Final Recommendation

### GO — rig 0.39.x is suitable for Rollshot's manual `AgentRun` driving

**Supporting evidence:**
- Hard gate (Step 3) PASS: `AgentRun` is a genuine sans-IO state machine with a clean
  driving protocol. Confirmed across 6 automated tests.
- Hard gate (Step 5) PASS: cancellation is clean on both timeout and token paths. No panics.
- Usage accounting (Step 6) PASS: per-response and cumulative token counts readable.
- Multimodal (Step 6) PASS: image + text messages build/serialize without feature gates.
- MSRV (Step 2): rig 0.39 measured MSRV = **1.88**, below workspace floor (1.89). No conflict.
- Privacy (Step 7): rig's `AgentRun` emits no tracing on the scripted path. Zero leakage risk.
- Facade (Step 7): `dyn RollshotModel` with provider swap at runtime proven.

**Facade shape (for product implementation):**
```rust
trait RollshotModel: Send + Sync {
    fn name(&self) -> &'static str;
    async fn complete(
        &self,
        prompt: &Message,
        history: &[Message],
    ) -> Result<ModelTurn, RollshotError>;
}

// Drive loop (simplified):
loop {
    match run.next_step()? {
        AgentRunStep::CallModel { prompt, history, .. } => {
            let turn = model.complete(&prompt, &history).await?;
            run.model_response(turn)?;
        }
        AgentRunStep::CallTools { calls } => {
            // execute tools, feed run.tool_results()
        }
        AgentRunStep::Done(response) => break response,
    }
}
```

**Rejected alternative:** hand-rolled provider trait + raw HTTP. Cost: ~500–1500 LoC for
request serialisation, SSE streaming, retry logic, and error normalisation per provider.
Wrapping rig's `CompletionModel` behind `RollshotModel` costs ~50 LoC per provider and
inherits rig's normalisation for free.

**Fallback triggers:**
1. rig pushes MSRV above the workspace floor in a minor release → evaluate downgrade or hand-roll.
2. rig emits prompt/response text via `tracing` in a future release on the AgentRun path →
   add `EnvFilter` to suppress rig's target.
3. rig's `AgentRun` protocol changes in a breaking way (`AgentRunStep` is exhaustive) →
   evaluate migration cost against the hand-roll baseline.

**Remaining risks:**
1. **Provider-specific structured tool behaviour: UNTESTED.** Whether Anthropic or OpenAI
   providers correctly encode `ToolDefinition` JSON schemas and parse tool-call wire format
   responses is not confirmed. The downstream agent-core subproject must close this gap via
   a recorded fixture (spec §11.6) or live test (Step 8). Status: OPEN.
2. **macOS parity: UNTESTED.** All probes ran on Linux. The workspace floor check on
   `macos-14` CI must confirm rig 0.39 builds there. Status: UNTESTED — pending controller CI.
3. **rig minor release MSRV drift.** rig has no published `rust-version` field. Measured
   1.88 is a point-in-time result against 0.39.0. Pin `rig-core = "=0.39.0"` in production
   until MSRV stability is confirmed across minor releases.

**Product handoff:**
- Implement `RollshotModel` in a new `rollshot-agent` crate (or `rollshot-image-document`)
  wrapping rig's `AgentRun` behind the facade (spec §4.1).
- Close the live tool-behaviour gap with a recorded fixture or Step 8 live test.
- Pin `rig-core` at the exact version until MSRV is confirmed stable.
- Run Spike CI on `macos-14` to confirm macOS rows.

When the decision has been consumed, set `Lifecycle` to `retained-reference`.
