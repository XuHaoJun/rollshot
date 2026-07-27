# Provider Boundary Reliability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make provider establishment and stream polling honestly cancelable and deadline-bounded, reject unproven partial completions, and conditionally upgrade `rollshot-agent` from Rig 0.39 to 0.40.

**Architecture:** `AgentRunner` owns the outer cancel/deadline select around provider establishment and every stream poll; adapter cooperation is optional. A standalone retained spike tests Rig 0.39 and 0.40 completion evidence before production changes. H2 failure stops the plan; after H2 passes, production changes use the passing Rig candidate and preserve Rollshot's provider-neutral public boundary.

**Tech Stack:** Rust, Tokio cancellation and paused time, futures streams, Rig 0.39/0.40, Wiremock SSE fixtures, Cargo, existing `rollshot-agent` provider contracts.

## Global Constraints

- Provider reliability is mandatory; Rig 0.40 is conditional.
- `AgentRunner` owns cancellation, wall-time classification, partial-result discard, and turn commit.
- `ProviderAdapter::stream` continues to accept `StreamBounds`; do not redesign the public trait.
- Cancellation wins a same-poll cancellation/deadline tie.
- Bare stream EOF is not positive completion evidence.
- No partial assistant turn, completed tool call, tool execution, or review artifact may survive failure.
- Use only local deterministic fixtures; do not call live Anthropic or OpenAI endpoints.
- Do not add retries, provider fallback, cost accounting, transport rewrites, Rig patches, or later umbrella slices.
- The spike remains standalone under `spikes/provider-boundary/` and must not enter the root workspace or become a production dependency.
- Stop after H2 failure. Do not continue to host changes or dependency migration until a new boundary design is approved.
- Use privacy-safe structured `tracing` with stable `rollshot::*` targets in production paths; do not add `println!`, `eprintln!`, or `dbg!` there.

---

## File Structure

### New files

- `spikes/provider-boundary/Cargo.toml` — standalone dual-Rig probe crate.
- `spikes/provider-boundary/Cargo.lock` — exact retained spike dependency resolution.
- `spikes/provider-boundary/src/main.rs` — selected-version provider probe and bounded JSON observations.
- `spikes/provider-boundary/fixtures/cases.json` — copied normal/incomplete Anthropic/OpenAI fixtures with provenance.
- `spikes/provider-boundary/FINDINGS.md` — H1/H2/H3 evidence and recommendation.
- `docs/superpowers/spikes/2026-07-26-provider-boundary-decision.md` — Gate G0 decision.

### Existing files that may change after H2 passes

- `crates/rollshot-agent/src/driver.rs` — private host-owned await helper, establishment/poll controls, valid-completion commit gate, and unit tests.
- `crates/rollshot-agent/src/provider.rs` — strict completion normalization and no bare-EOF synthetic success.
- `crates/rollshot-agent/tests/provider_contract.rs` — stalled-adapter and incomplete-stream contracts.
- `crates/rollshot-agent/Cargo.toml` — conditional exact Rig 0.40 dependency.
- `Cargo.lock` — conditional Rig 0.40 resolution.

Do not split `driver.rs` or `provider.rs`; this slice follows their current responsibilities.

---

### Task 1: Create the standalone retained spike

**Files:**
- Create: `spikes/provider-boundary/Cargo.toml`
- Create: `spikes/provider-boundary/src/main.rs`
- Create: `spikes/provider-boundary/fixtures/cases.json`
- Create: `spikes/provider-boundary/FINDINGS.md`
- Create via Cargo: `spikes/provider-boundary/Cargo.lock`

**Interfaces:**
- Consumes: published `rig-core` 0.39.0/0.40.0 and four local SSE fixtures.
- Produces: `provider-boundary-spike`, compiled with exactly one of `rig-039` or `rig-040`.

- [ ] **Step 1: Create directories and copy exact fixtures**

```bash
rtk mkdir -p spikes/provider-boundary/src spikes/provider-boundary/fixtures
rtk python3 - <<'PY'
import json
from pathlib import Path
source = Path('crates/rollshot-agent/tests/fixtures/provider_streams.json')
target = Path('spikes/provider-boundary/fixtures/cases.json')
keys = [
    'anthropic_text_only',
    'anthropic_incomplete_stream',
    'openai_text_only',
    'openai_incomplete_stream',
]
data = json.loads(source.read_text())
target.write_text(json.dumps({key: data[key] for key in keys}, indent=2) + '\n')
PY
```

Expected: exactly four entries; each retains `provenance` and `chunks`.

- [ ] **Step 2: Write the standalone manifest**

Create `spikes/provider-boundary/Cargo.toml`:

```toml
[package]
name = "provider-boundary-spike"
version = "0.0.0"
edition = "2024"
publish = false

[features]
default = []
rig-039 = ["dep:rig-core-039"]
rig-040 = ["dep:rig-core-040"]

[dependencies]
futures-util = "0.3"
rig-core-039 = { package = "rig-core", version = "=0.39.0", optional = true }
rig-core-040 = { package = "rig-core", version = "=0.40.0", optional = true }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
wiremock = "0.6"

[workspace]
```

- [ ] **Step 3: Write feature guards and shared probe types**

Create `spikes/provider-boundary/src/main.rs`:

```rust
#[cfg(all(feature = "rig-039", feature = "rig-040"))]
compile_error!("enable exactly one of rig-039 or rig-040");
#[cfg(not(any(feature = "rig-039", feature = "rig-040")))]
compile_error!("enable exactly one of rig-039 or rig-040");

#[cfg(feature = "rig-039")]
use rig_core_039 as rig;
#[cfg(feature = "rig-040")]
use rig_core_040 as rig;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct Fixture {
    chunks: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Observation {
    Text { text: String },
    ToolCall { id: String, name: String },
    Final { total_tokens: u64 },
    Error { category: String },
    End,
}

fn fixture(name: &str) -> Fixture {
    let all: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/cases.json"
    ))
    .expect("fixture JSON must parse");
    serde_json::from_value(all[name].clone()).expect("named fixture must parse")
}

fn main() {
    eprintln!("completion probe is added in Task 2");
}
```

The disposable spike may use intentional stderr UX. Production files may not.

- [ ] **Step 4: Create initial findings with explicit UNTESTED states**

Create `spikes/provider-boundary/FINDINGS.md` from the repo spike template with:

```markdown
# Provider Boundary Reliability Feasibility Spike - Findings

## Status

- Lifecycle: active
- Decision owner: Rollshot agent-foundation Gate G0
- Started: 2026-07-26
- Last updated: 2026-07-26

## Decision

Determine whether Rig 0.39 or 0.40 distinguishes normal provider completion from incomplete EOF strongly enough to proceed with the host-owned reliability fix.

## Environment

- Evidence scope: local compile, automated, and runtime evidence
- Live providers: UNTESTED and out of scope
- Hardware: UNTESTED and not required

## Risk Results

| Risk | Gate | Evidence | Result | Notes / artifacts |
|---|---|---|---|---|
| Normal versus partial completion | H2 hard | runtime | UNTESTED | `fixtures/cases.json` |
| Host wakes ignored bounds | H1 hard | automated | UNTESTED | Production tests after H2 |
| Rig 0.40 compatibility | H3 upgrade | compile/automated | UNTESTED | Conditional after H1/H2 |

## Observations

No probe command has run yet.

## Final Recommendation

- Go / no-go: UNTESTED — blocked on H2 probes
- Supporting evidence: UNTESTED — no runtime observation recorded
- Rejected alternatives: provider trait redesign; Rig patch/fork; transport rewrite; live-provider acceptance
- Fallback triggers: H2 failure stops this plan; H3 failure retains a passing 0.39 path when available
- Remaining risks: external provider cost; live infrastructure latency; socket cleanup; interrupted-stream billing
- Product handoff: UNTESTED — no handoff before H2
```

- [ ] **Step 5: Verify isolation and feature guards**

```bash
rtk cargo metadata --manifest-path spikes/provider-boundary/Cargo.toml --no-deps --format-version 1
rtk cargo check --manifest-path spikes/provider-boundary/Cargo.toml --features rig-039
rtk cargo check --manifest-path spikes/provider-boundary/Cargo.toml --features rig-040
rtk bash -lc '! cargo check --manifest-path spikes/provider-boundary/Cargo.toml'
rtk bash -lc '! cargo check --manifest-path spikes/provider-boundary/Cargo.toml --features rig-039,rig-040'
```

Expected: the first metadata command names `spikes/provider-boundary` as workspace root; both single-version checks pass; no-feature and both-feature checks fail with the explicit guard.

- [ ] **Step 6: Commit the scaffold**

```bash
rtk git add spikes/provider-boundary
rtk git commit -m "spike(agent): scaffold provider boundary evidence"
```

---

### Task 2: Probe Rig 0.39 and 0.40 completion evidence

**Files:**
- Modify: `spikes/provider-boundary/src/main.rs`
- Create during execution: `spikes/provider-boundary/evidence/*.json`
- Modify: `spikes/provider-boundary/FINDINGS.md`

**Interfaces:**
- Consumes: CLI provider `anthropic` or `openai`, plus one exact fixture name.
- Produces: JSON arrays of `Observation`; Task 3 mechanically evaluates H2.

- [ ] **Step 1: Add the CLI and local SSE server**

Replace the stub `main` and add:

```rust
async fn sse_server(case: &Fixture) -> wiremock::MockServer {
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    let response = ResponseTemplate::new(200)
        .set_body_bytes(case.chunks.join("").into_bytes())
        .insert_header("content-type", "text/event-stream");
    Mock::given(wiremock::matchers::any())
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let provider = args.next().expect("provider argument");
    let fixture_name = args.next().expect("fixture argument");
    assert!(args.next().is_none(), "only provider and fixture are accepted");
    let observations = match provider.as_str() {
        "anthropic" => probe_anthropic(&fixture_name).await,
        "openai" => probe_openai(&fixture_name).await,
        other => panic!("unsupported provider: {other}"),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&observations).expect("serialize observations")
    );
}
```

- [ ] **Step 2: Add the shared completion request**

```rust
fn request() -> rig::completion::CompletionRequest {
    rig::completion::CompletionRequest {
        model: Some("test-model".to_string()),
        preamble: None,
        chat_history: rig::OneOrMany::one(rig::message::Message::user("probe")),
        documents: vec![],
        tools: vec![],
        temperature: None,
        max_tokens: Some(64),
        tool_choice: None,
        additional_params: None,
        output_schema: None,
    }
}
```

If 0.40 changes this private construction surface, add a version-gated `request()` implementation and record the compiler error plus exact adaptation in findings. Do not change `Observation`.

- [ ] **Step 3: Implement the Anthropic probe**

```rust
async fn probe_anthropic(name: &str) -> Vec<Observation> {
    use futures_util::StreamExt;
    use rig::client::CompletionClient;
    use rig::completion::{CompletionModel, GetTokenUsage};
    use rig::streaming::StreamedAssistantContent;

    let case = fixture(name);
    let server = sse_server(&case).await;
    let client = rig::providers::anthropic::Client::builder()
        .api_key("spike-key")
        .base_url(&server.uri())
        .build()
        .expect("anthropic client");
    let model = client.completion_model("test-model");
    let mut stream = model.stream(request()).await.expect("stream establishment");
    let mut out = Vec::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(StreamedAssistantContent::Text(text)) => {
                out.push(Observation::Text { text: text.text });
            }
            Ok(StreamedAssistantContent::ToolCall { tool_call, .. }) => {
                out.push(Observation::ToolCall {
                    id: tool_call.id,
                    name: tool_call.function.name,
                });
            }
            Ok(StreamedAssistantContent::Final(response)) => {
                out.push(Observation::Final {
                    total_tokens: response.token_usage().total_tokens,
                });
            }
            Ok(_) => {}
            Err(error) => {
                out.push(Observation::Error {
                    category: format!("{error:?}"),
                });
                break;
            }
        }
    }
    out.push(Observation::End);
    out
}
```

- [ ] **Step 4: Implement the OpenAI probe**

```rust
async fn probe_openai(name: &str) -> Vec<Observation> {
    use futures_util::StreamExt;
    use rig::client::CompletionClient;
    use rig::completion::{CompletionModel, GetTokenUsage};
    use rig::streaming::StreamedAssistantContent;

    let case = fixture(name);
    let server = sse_server(&case).await;
    let client = rig::providers::openai::Client::builder()
        .api_key("spike-key")
        .base_url(&server.uri())
        .build()
        .expect("openai client")
        .completions_api();
    let model = client.completion_model("test-model");
    let mut stream = model.stream(request()).await.expect("stream establishment");
    let mut out = Vec::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(StreamedAssistantContent::Text(text)) => {
                out.push(Observation::Text { text: text.text });
            }
            Ok(StreamedAssistantContent::ToolCall { tool_call, .. }) => {
                out.push(Observation::ToolCall {
                    id: tool_call.id,
                    name: tool_call.function.name,
                });
            }
            Ok(StreamedAssistantContent::Final(response)) => {
                out.push(Observation::Final {
                    total_tokens: response.token_usage().total_tokens,
                });
            }
            Ok(_) => {}
            Err(error) => {
                out.push(Observation::Error {
                    category: format!("{error:?}"),
                });
                break;
            }
        }
    }
    out.push(Observation::End);
    out
}
```

- [ ] **Step 5: Run all eight local probes**

```bash
rtk mkdir -p spikes/provider-boundary/evidence
rtk bash -lc '
set -eu
for version in rig-039 rig-040; do
  for pair in \
    "anthropic anthropic_text_only" \
    "anthropic anthropic_incomplete_stream" \
    "openai openai_text_only" \
    "openai openai_incomplete_stream"; do
    set -- $pair
    cargo run --quiet \
      --manifest-path spikes/provider-boundary/Cargo.toml \
      --no-default-features --features "$version" -- "$1" "$2" \
      > "spikes/provider-boundary/evidence/${version}-${2}.json"
  done
done
'
```

Expected: eight JSON files and no external request.

- [ ] **Step 6: Evaluate H2 mechanically**

```bash
rtk python3 - <<'PY'
import json
from pathlib import Path
root = Path('spikes/provider-boundary/evidence')
for version in ('rig-039', 'rig-040'):
    normal_ok = True
    incomplete_ok = True
    for provider in ('anthropic', 'openai'):
        normal = json.loads((root / f'{version}-{provider}_text_only.json').read_text())
        incomplete = json.loads((root / f'{version}-{provider}_incomplete_stream.json').read_text())
        normal_kinds = [item['kind'] for item in normal]
        incomplete_kinds = [item['kind'] for item in incomplete]
        normal_ok &= 'final' in normal_kinds and 'error' not in normal_kinds
        incomplete_ok &= 'error' in incomplete_kinds and 'final' not in incomplete_kinds
    print(f'{version}: normal_ok={normal_ok} incomplete_ok={incomplete_ok} H2={"PASS" if normal_ok and incomplete_ok else "FAIL"}')
PY
```

Do not reinterpret non-zero usage, clean EOF, or synthesized `Final` as protocol proof.

- [ ] **Step 7: Record and commit runtime evidence**

Add exact OS, Rust/Cargo versions, Rollshot commit, eight commands, output paths, observations, and H2 result to `FINDINGS.md`.

```bash
rtk rustc --version
rtk cargo --version
rtk uname -a
rtk git rev-parse HEAD
rtk git add spikes/provider-boundary
rtk git commit -m "spike(agent): probe rig completion evidence"
```

---

### Task 3: Enforce the H2 hard checkpoint

**Files:**
- Modify: `spikes/provider-boundary/FINDINGS.md`
- Create on failure: `docs/superpowers/spikes/2026-07-26-provider-boundary-decision.md`

**Interfaces:**
- Consumes: eight Task 2 observations.
- Produces: selected candidate `rig-040` or `rig-039`, or a NO-GO that terminates execution.

- [ ] **Step 1: Select a candidate with the fixed rule**

1. Select `rig-040` if both normal providers emit `Final` without `Error` and both incomplete providers emit `Error` without `Final`.
2. Otherwise select `rig-039` if it meets the same condition.
3. Otherwise set H2 to `FAIL` and stop.

- [ ] **Step 2A: Record a passing candidate**

For `rig-040`, write these exact findings lines:

```markdown
- Go / no-go: GO for production H1/H2 work with Rig 0.40 as the candidate
- Supporting evidence: Rig 0.40 normal Anthropic/OpenAI emitted Final without Error; incomplete Anthropic/OpenAI emitted Error without Final
- Product handoff: proceed to host controls, then activate exact Rig 0.40 before production H2 tests
```

For `rig-039`, use:

```markdown
- Go / no-go: GO for production H1/H2 work with Rig 0.39 as the candidate
- Supporting evidence: Rig 0.39 normal Anthropic/OpenAI emitted Final without Error; incomplete Anthropic/OpenAI emitted Error without Final
- Product handoff: proceed on exact Rig 0.39; record Rig 0.40 as failed H2 migration evidence
```

Set H2 to `PASS` and commit:

```bash
rtk git add spikes/provider-boundary/FINDINGS.md
rtk git commit -m "spike(agent): close provider completion gate"
```

- [ ] **Step 2B: On H2 failure, write the fixed NO-GO record and stop**

Create `docs/superpowers/spikes/2026-07-26-provider-boundary-decision.md` with:

```markdown
# Provider Boundary Gate G0 Decision

**Date:** 2026-07-26
**Status:** NO-GO — boundary redesign required
**Gate:** H2 completion integrity

## Evidence

Neither Rig 0.39 nor Rig 0.40 distinguished normal Anthropic/OpenAI completion from incomplete EOF according to `spikes/provider-boundary/FINDINGS.md`. Incomplete streams produced a Final or ended without a typed error, so EOF cannot support an honest completion contract.

## Decision

Stop Slice 1 before production changes. Do not implement the host guard in isolation and do not upgrade Rig. Start a new boundary/fork design that can preserve provider completion receipts without a transport rewrite or provider-specific public state.

## Rejected shortcuts

- Non-zero usage as a completion receipt.
- Stream EOF as success.
- Live-provider acceptance.
- Rig patching or copied provider transports inside this slice.

## Residual risk

Provider establishment and pending item polls remain vulnerable to ignored `StreamBounds` until a revised design is approved.
```

Update findings to `Go / no-go: NO-GO`, set H2 to `FAIL`, and commit:

```bash
rtk git add spikes/provider-boundary/FINDINGS.md docs/superpowers/spikes/2026-07-26-provider-boundary-decision.md
rtk git commit -m "docs(agent): stop provider boundary at integrity gate"
```

**STOP:** Report the NO-GO to the user. Do not execute Tasks 4–8.

---

### Task 4: Add host-owned establishment and poll controls

**Entry gate:** H2 is `PASS`.

**Files:**
- Modify: `crates/rollshot-agent/src/driver.rs`
- Modify: `crates/rollshot-agent/tests/provider_contract.rs`
- Modify: `spikes/provider-boundary/FINDINGS.md`

**Interfaces:**
- Consumes: `RunCancellation`, Tokio deadline, `DriverError`, and any provider future.
- Produces: private `await_provider_progress<F, T>(...) -> Result<T, DriverError>`, used at both control points.

- [ ] **Step 1: Write RED helper tests**

Add near existing provider-stream driver tests:

```rust
#[tokio::test]
async fn provider_progress_cancel_wakes_pending_future() {
    let cancellation = RunCancellation::new();
    cancellation.cancel();
    let result = await_provider_progress(
        &cancellation,
        tokio::time::Instant::now() + std::time::Duration::from_secs(30),
        std::future::pending::<()>(),
    )
    .await;
    assert_eq!(result, Err(DriverError::Cancelled));
}

#[tokio::test(start_paused = true)]
async fn provider_progress_deadline_wakes_pending_future() {
    let cancellation = RunCancellation::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    let future = await_provider_progress(&cancellation, deadline, std::future::pending::<()>());
    tokio::pin!(future);
    tokio::time::advance(std::time::Duration::from_secs(10)).await;
    assert_eq!(
        future.await,
        Err(DriverError::BudgetExhausted(BudgetDimension::WallTime))
    );
}

#[tokio::test(start_paused = true)]
async fn provider_progress_same_poll_tie_prefers_cancel() {
    let cancellation = RunCancellation::new();
    cancellation.cancel();
    let result = await_provider_progress(
        &cancellation,
        tokio::time::Instant::now(),
        std::future::ready(()),
    )
    .await;
    assert_eq!(result, Err(DriverError::Cancelled));
}
```

- [ ] **Step 2: Verify RED**

```bash
rtk cargo test -p rollshot-agent provider_progress_ -- --nocapture
```

Expected: compile failure naming missing `await_provider_progress`.

- [ ] **Step 3: Implement the private helper**

Add beside `DriverError`:

```rust
async fn await_provider_progress<F, T>(
    cancellation: &RunCancellation,
    deadline: tokio::time::Instant,
    future: F,
) -> Result<T, DriverError>
where
    F: std::future::Future<Output = T>,
{
    if cancellation.is_cancelled() {
        return Err(DriverError::Cancelled);
    }
    if tokio::time::Instant::now() >= deadline {
        return Err(DriverError::BudgetExhausted(BudgetDimension::WallTime));
    }
    tokio::pin!(future);
    tokio::select! {
        biased;
        _ = cancellation.wait() => Err(DriverError::Cancelled),
        _ = tokio::time::sleep_until(deadline) => {
            Err(DriverError::BudgetExhausted(BudgetDimension::WallTime))
        }
        output = &mut future => Ok(output),
    }
}
```

- [ ] **Step 4: Apply it to provider establishment and item polling**

Establishment:

```rust
let mut stream = await_provider_progress(
    cancellation,
    deadline,
    provider.stream(request, bounds),
)
.await?
.map_err(|error| DriverError::ProviderFailure(error.to_string()))?;
```

Polling, while preserving current EOF behavior until Task 6:

```rust
loop {
    let next = await_provider_progress(cancellation, deadline, stream.next()).await?;
    let Some(event_result) = next else {
        break;
    };
    // Existing wall-time recheck and event handling remain here.
}
```

- [ ] **Step 5: Add ignored-bounds provider fixtures**

Inside the existing `visual_annotation` test module in `provider_contract.rs`, next to `ScriptedProvider`, add `BudgetDimension` to the runtime imports and define:

```rust
#[derive(Clone, Copy)]
enum PendingMode {
    Establishment,
    AfterText,
}

struct PendingProvider {
    mode: PendingMode,
    entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}
```

Implement `ProviderAdapter::stream` so `Establishment` sends `entered` then awaits `std::future::pending()`, while `AfterText` returns an `async_stream::stream!` that yields `TextDelta("partial")`, sends `entered`, then remains pending. Ignore `_bounds` deliberately.

Add a shared runner fixture:

```rust
fn spawn_pending_run(
    mode: PendingMode,
) -> (
    tokio::sync::oneshot::Receiver<()>,
    RunCancellation,
    tokio::task::JoinHandle<VisualAnnotationRunTerminal>,
) {
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let provider = PendingProvider {
        mode,
        entered: Mutex::new(Some(entered_tx)),
    };
    let cancellation = RunCancellation::new();
    let run_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        let mut budget = visual_annotation_run_budget();
        budget.wall_time = std::time::Duration::from_secs(10);
        AgentRunner::new(AgentConfig {
            max_turns: 2,
            ..AgentConfig::default()
        })
        .run_visual_annotation_with_provider(
            authorized_input_with_one_png(),
            &provider,
            budget,
            &run_cancellation,
        )
        .await
    });
    (entered_rx, cancellation, task)
}
```

Then add the four contracts:

```rust
#[tokio::test]
async fn runner_cancels_pending_provider_establishment() {
    let (entered, cancellation, task) = spawn_pending_run(PendingMode::Establishment);
    entered.await.expect("provider entered establishment");
    cancellation.cancel();
    let terminal = tokio::time::timeout(std::time::Duration::from_secs(1), task)
        .await
        .expect("runner must not hang")
        .expect("runner task");
    assert_eq!(terminal, VisualAnnotationRunTerminal::Cancelled);
}

#[tokio::test(start_paused = true)]
async fn runner_deadlines_pending_provider_establishment() {
    let (entered, _cancellation, task) = spawn_pending_run(PendingMode::Establishment);
    entered.await.expect("provider entered establishment");
    tokio::time::advance(std::time::Duration::from_secs(10)).await;
    assert_eq!(
        task.await.expect("runner task"),
        VisualAnnotationRunTerminal::BudgetExhausted {
            dimension: BudgetDimension::WallTime,
        }
    );
}

#[tokio::test]
async fn runner_cancels_pending_provider_item_after_partial_text() {
    let (entered, cancellation, task) = spawn_pending_run(PendingMode::AfterText);
    entered.await.expect("provider entered pending item poll");
    cancellation.cancel();
    let terminal = tokio::time::timeout(std::time::Duration::from_secs(1), task)
        .await
        .expect("runner must not hang")
        .expect("runner task");
    assert_eq!(terminal, VisualAnnotationRunTerminal::Cancelled);
}

#[tokio::test(start_paused = true)]
async fn runner_deadlines_pending_provider_item_after_partial_text() {
    let (entered, _cancellation, task) = spawn_pending_run(PendingMode::AfterText);
    entered.await.expect("provider entered pending item poll");
    tokio::time::advance(std::time::Duration::from_secs(10)).await;
    assert_eq!(
        task.await.expect("runner task"),
        VisualAnnotationRunTerminal::BudgetExhausted {
            dimension: BudgetDimension::WallTime,
        }
    );
}
```

- [ ] **Step 6: Run GREEN verification**

```bash
rtk cargo test -p rollshot-agent provider_progress_ -- --nocapture
rtk cargo test -p rollshot-agent --test provider_contract pending_provider -- --nocapture
rtk cargo test -p rollshot-agent
```

Expected: helper tests and all four full paths pass; existing tests remain green.

- [ ] **Step 7: Record H1 and commit**

Set H1 to `PASS` with exact test commands in findings.

```bash
rtk git add crates/rollshot-agent/src/driver.rs crates/rollshot-agent/tests/provider_contract.rs spikes/provider-boundary/FINDINGS.md
rtk git commit -m "fix(agent): bound provider stream progress"
```

---

### Task 5: Activate the H2-selected Rig candidate

**Entry gate:** H1/H2 pass.

**Files:**
- Conditionally modify: `crates/rollshot-agent/Cargo.toml`
- Conditionally modify: `Cargo.lock`
- Conditionally modify for private compile adaptation: `crates/rollshot-agent/src/driver.rs`
- Conditionally modify for private compile adaptation: `crates/rollshot-agent/src/model.rs`
- Conditionally modify for private compile adaptation: `crates/rollshot-agent/src/provider.rs`
- Modify: `spikes/provider-boundary/FINDINGS.md`

**Interfaces:**
- Consumes: candidate recorded by Task 3.
- Produces: a compiling workspace on that exact candidate before production H2 tests.

- [ ] **Step 1A: If candidate is Rig 0.39, verify and record no dependency change**

```bash
rtk cargo tree -p rollshot-agent -i rig-core
rtk cargo check -p rollshot-agent --all-targets
```

Expected: exact 0.39.0 and compile success. Record H3 as `FAIL for 0.40 adoption — Rig 0.40 failed H2` while retaining 0.39 as the reliability candidate. Commit findings only.

- [ ] **Step 1B: If candidate is Rig 0.40, update exact dependency**

Change `crates/rollshot-agent/Cargo.toml` to:

```toml
rig-core = { version = "=0.40.0", features = ["test-utils"] }
```

Then:

```bash
rtk cargo update -p rig-core:0.39.0 --precise 0.40.0
rtk cargo check -p rollshot-agent --all-targets
```

Allowed compile adaptations are restricted to `driver.rs`, `model.rs`, and `provider.rs`. Keep `.max_turns(self.config.max_turns)` unchanged because Rig 0.40 now defines it as the exact total model-call budget. Route the new `StreamedAssistantContent::Unknown` through the existing non-surfaced wildcard path. Do not enable Rig hooks, output modes, concurrent tools, retries, or new public types.

- [ ] **Step 2: Apply the compile hard stop**

If 0.40 requires changes outside the three private translation files, restore manifest/lockfile and private adaptation hunks, set H3 to `FAIL`, and:

- continue on 0.39 only when Task 2 showed 0.39 H2 `PASS`;
- otherwise write a NO-GO decision and stop because no compiling H2 candidate remains.

Use `rtk git restore` or `rtk git restore -p`; never use a hard reset.

- [ ] **Step 3: Commit the active candidate**

For 0.40:

```bash
rtk git add crates/rollshot-agent/Cargo.toml Cargo.lock crates/rollshot-agent/src/driver.rs crates/rollshot-agent/src/model.rs crates/rollshot-agent/src/provider.rs spikes/provider-boundary/FINDINGS.md
rtk git commit -m "build(agent): activate rig 0.40 candidate"
```

Omit unchanged translation files. For 0.39, commit only findings:

```bash
rtk git add spikes/provider-boundary/FINDINGS.md
rtk git commit -m "docs(agent): retain rig 0.39 candidate"
```

---

### Task 6: Enforce completion before committing a provider turn

**Entry gate:** the H2-selected candidate compiles and Task 4 is green.

**Files:**
- Modify: `crates/rollshot-agent/src/provider.rs`
- Modify: `crates/rollshot-agent/src/driver.rs`
- Modify: `crates/rollshot-agent/tests/provider_contract.rs`
- Modify: `spikes/provider-boundary/FINDINGS.md`

**Interfaces:**
- Consumes: `ModelStreamEvent::Completed`, `StreamedTurnAssembler`, turn-local buffers, and Task 4 controlled polls.
- Produces: `StreamIncomplete` on unproven EOF, tool completions before one `Completed`, and immediate return after valid completion.

- [ ] **Step 1: Tighten incomplete-stream tests**

Replace the current permissive incomplete tests with assertions equivalent to:

```rust
let (events, error) = collect_events(&mut stream).await;
assert!(matches!(error, Some(ModelError::StreamIncomplete(_))));
assert!(!events.iter().any(|event| {
    matches!(event, ModelStreamEvent::Completed(_))
}));
```

Use test names `anthropic_incomplete_stream_is_not_completed` and `openai_incomplete_stream_is_not_completed`. Do not change fixture bytes.

- [ ] **Step 2: Add valid-completion/open-tail and partial-tool tests**

Extend `PendingMode` with `AfterCompleted`. Yield one valid `Completed`, signal the barrier, then remain pending. Add `runner_does_not_wait_for_eof_after_valid_completion`; assert the runner returns before cancellation/deadline. Its eventual existing protocol terminal is acceptable because the fixture submits no product tool.

Add a driver test whose stream yields a tool start, the incomplete delta `{"unfinished"`, then `Err(ModelError::StreamIncomplete("fixture ended"))`. Assert terminal `ProviderFailure`, zero spy-tool calls, no completed assistant exchange, and no `ToolCallComplete` event.

- [ ] **Step 3: Verify RED**

```bash
rtk cargo test -p rollshot-agent --test provider_contract incomplete_stream_is_not_completed -- --nocapture
rtk cargo test -p rollshot-agent --test provider_contract runner_does_not_wait_for_eof_after_valid_completion -- --nocapture
rtk cargo test -p rollshot-agent partial_tool -- --nocapture
```

Expected: current synthetic completion/open-tail behavior fails at least the first two contracts.

- [ ] **Step 4: Make provider normalization require Completed**

In `stream_to_model_events`, store but defer a completion:

```rust
let mut completion: Option<ModelCompletion> = None;
```

When normalized events contain `Completed`, save it and break the provider loop after all other events from that item are processed. Do not yield it yet. After the loop:

```rust
let Some(mut completion) = completion else {
    yield Err(ModelError::StreamIncomplete(
        "provider stream ended before completion".to_string(),
    ));
    return;
};
let final_choice = rig_core::OneOrMany::one(
    rig_core::message::AssistantContent::text("")
);
let turn = asm.finish(None, &final_choice);
let has_tool_calls = turn.choice.iter().any(|item| {
    matches!(item, rig_core::message::AssistantContent::ToolCall(_))
});
if has_tool_calls {
    completion.stop_reason = StopReason::ToolUse;
}
for event in emit_tool_call_completions(&turn) {
    yield Ok(event);
}
yield Ok(ModelStreamEvent::Completed(completion));
```

Delete the bare-EOF synthetic `Completed` branch. Accumulated usage cannot authorize success.

- [ ] **Step 5: Require Completed in the driver**

Add `saw_completed = false`. Set it in the `Completed` arm, preserve usage, and break after processing that event. Before building the final Rig choice:

```rust
if !saw_completed {
    return Err(DriverError::ProviderFailure(
        "provider stream ended before completion".to_string(),
    ));
}
```

No assignment to `last_assistant_text`, budget charge, `record_streamed_completion_call`, or `streamed_turn` may occur before this check.

- [ ] **Step 6: Run GREEN completion verification**

```bash
rtk cargo test -p rollshot-agent --test provider_contract incomplete_stream_is_not_completed -- --nocapture
rtk cargo test -p rollshot-agent --test provider_contract runner_does_not_wait_for_eof_after_valid_completion -- --nocapture
rtk cargo test -p rollshot-agent partial_tool -- --nocapture
rtk cargo test -p rollshot-agent --test provider_contract
rtk cargo test -p rollshot-agent
```

Expected: normal Anthropic/OpenAI paths remain successful; incomplete streams never complete; partial tools never execute. If these contracts cannot pass without a forbidden Rig patch, transport rewrite, EOF inference, or public provider-specific type, mark H2 production validation `FAIL`, write a NO-GO Gate G0 record using Task 3's structure, and stop before Task 7.

- [ ] **Step 7: Record production H2 and commit**

```bash
rtk git add crates/rollshot-agent/src/provider.rs crates/rollshot-agent/src/driver.rs crates/rollshot-agent/tests/provider_contract.rs spikes/provider-boundary/FINDINGS.md
rtk git commit -m "fix(agent): reject incomplete provider turns"
```

---

### Task 7: Close H3, verify, and obtain independent review

**Files:**
- Modify: `spikes/provider-boundary/FINDINGS.md`
- Production files only if review finds a spec violation.

**Interfaces:**
- Consumes: H1/H2 implementation and active Rig candidate.
- Produces: final H3 result and independent review evidence.

- [ ] **Step 1: Run required verification**

```bash
rtk cargo fmt --check
rtk cargo test -p rollshot-agent --test provider_contract
rtk cargo test -p rollshot-agent
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk cargo tree -p rollshot-agent -i rig-core
rtk rg -n 'rig_core::|rig-core' crates/rollshot-agent --glob '*.rs' --glob 'Cargo.toml'
rtk git diff --check
rtk git status --short
```

Expected: all commands pass; tree shows the active exact version; production Rig references remain limited to `driver.rs`, `model.rs`, `provider.rs`, and the manifest.

- [ ] **Step 2: Re-run the eight spike probes**

Repeat Task 2's matrix and compare the JSON files. Any unexplained difference blocks Gate G0.

- [ ] **Step 3: Request independent review**

Invoke `requesting-code-review` with the child spec, this plan, Task 4–6 commits, findings, and these questions:

- Can any adapter bypass establishment/poll bounds?
- Does cancellation win the same-poll tie?
- Can partial text/tool state cross the commit boundary?
- Is completion proof protocol-backed rather than EOF/usage inference?
- Did Rig migration remain private and minimal?
- Do errors/logs remain privacy-safe?

The reviewer is read-only. Apply accepted fixes as the sole writer, repeat Steps 1–2, and commit fixes with a scoped `fix(agent): ...` or `test(agent): ...` message.

- [ ] **Step 4: Finalize findings**

Record exact commands, counts, H1/H2/H3 status, active Rig version, review verdict/fixes, rejected alternatives, and residual risks. Keep `Lifecycle: active` until user Gate G0 approval.

```bash
rtk git add spikes/provider-boundary/FINDINGS.md
rtk git commit -m "docs(agent): close provider boundary findings"
```

---

### Task 8: Write and present Gate G0 decision

**Files:**
- Create: `docs/superpowers/spikes/2026-07-26-provider-boundary-decision.md`
- Modify after approval: `spikes/provider-boundary/FINDINGS.md`

**Interfaces:**
- Consumes: exact H1/H2/H3 findings and reviewer verdict.
- Produces: proposed Gate G0 decision; after user approval, retained-reference lifecycle.

- [ ] **Step 1: Write the decision from recorded evidence**

Choose exactly one factual header from the active dependency tree:

```markdown
# Provider Boundary Gate G0 Decision

**Date:** 2026-07-26
**Status:** Proposed for user approval
**Rig outcome:** Rig 0.40 adopted
```

or:

```markdown
# Provider Boundary Gate G0 Decision

**Date:** 2026-07-26
**Status:** Proposed for user approval
**Rig outcome:** Rig 0.39 retained
```

Then add `Decision`, `H1 — Host control`, `H2 — Completion integrity`, `H3 — Rig migration`, `Independent review`, `Rejected alternatives`, `Residual risks`, and `Product handoff` headings. Under each gate, copy the exact commands/test counts and result from `FINDINGS.md`; do not summarize a failure as success. `Product handoff` says Slice 2 may begin only after user approval. Include the four approved residual risks: external cost before cancellation, live-provider latency/outage behavior, lower-level socket cleanup, and interrupted-stream billing/usage.

- [ ] **Step 2: Verify and commit the proposed decision**

```bash
rtk rg -n 'TB[D]|TO[D]O|FIXM[E]|XX[X]' docs/superpowers/spikes/2026-07-26-provider-boundary-decision.md
rtk python3 - <<'PY'
from pathlib import Path
text = Path('docs/superpowers/spikes/2026-07-26-provider-boundary-decision.md').read_text()
outcomes = ['**Rig outcome:** Rig 0.40 adopted', '**Rig outcome:** Rig 0.39 retained']
assert sum(outcome in text for outcome in outcomes) == 1
PY
rtk git diff --check
rtk git add docs/superpowers/spikes/2026-07-26-provider-boundary-decision.md
rtk git commit -m "docs(agent): propose provider boundary gate decision"
```

Expected: the scan returns no matches and diff check passes.

- [ ] **Step 3: Present Gate G0 for explicit approval**

Report selected Rig version, exact H1/H2/H3 results, final verification, reviewer verdict, decision path/commit, and residual risks. Do not begin Slice 2 in the same turn.

- [ ] **Step 4: After approval, retain the spike**

Set:

```markdown
- Lifecycle: retained-reference
```

Record the approval date and decision commit in findings, then:

```bash
rtk git add spikes/provider-boundary/FINDINGS.md
rtk git commit -m "docs(agent): retain provider boundary spike evidence"
```

Slice 1 completes only after this commit.
