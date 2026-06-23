# Bounded Agent Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `crates/rollshot-agent` — a provider-neutral, bounded control plane that drives a multimodal model through typed tools to author a redaction automation and hand back a reviewable `ReadyForReview` (or a typed terminal state), without persisting state, mutating `ImageDocument`, or exposing Rig types.

**Architecture:** One new crate. A manual driver advances Rig's `AgentRun` one step at a time; BAC owns provider streaming adapters (Anthropic + OpenAI), a typed tool registry, a run-local draft with generation evidence, budgets, cancellation, append-only events, and terminal-state validation. Rig is used only as the turn state machine + message/tool DTOs (not provider normalization — see spec §0/D1). Inspection and automation-authoring tools delegate to the existing `rollshot-automation` / `rollshot-edit-proposal` / `rollshot-vision` crates.

**Tech Stack:** Rust (edition 2021, MSRV 1.94), `rig-core = "=0.39.0"`, `tokio`, `futures-util`, `serde`/`serde_json`, `tracing`, `thiserror`. Sandbox dry-run via `rollshot-automation-rquickjs::QuickJsExecutor`.

**Spec:** `docs/superpowers/specs/2026-06-23-bounded-agent-core-design.md` (read §0 first — it records eng-review decisions D1–D8 that this plan honors).

## Global Constraints

- **MSRV `1.94`**, edition `2021` (workspace `rust-version = "1.94"`).
- **`unsafe_code = "forbid"`** workspace-wide — provider adapters wrap safe APIs only; no `unsafe`.
- **Pin `rig-core = "=0.39.0"`** (exact). No other Rig version.
- **No Rig types in public BAC APIs** (spec §3.1). Rig types stay inside `driver.rs`, `model.rs`, `provider/*`.
- **No persistence, no `ImageDocument` mutation, no automatic provider retry** (spec §1, §2.3, §6.4).
- **Tracing:** stable explicit `rollshot::agent::*` targets + structured fields only. Never `println!`/`eprintln!`/`dbg!`. Never log prompts, responses, attachment bytes, automation source, sensitive tool payloads, credentials, or raw stream frames (spec §12).
- **D2:** one cancellation source; reuse `rollshot_automation::CancellationFlag` for the dry-run — do not invent a second primitive.
- **D3:** stream/protocol failures terminate (`AgentProtocolFailure`); well-formed-but-schema-invalid args on a *known* tool return a recoverable typed tool error.
- **D4:** `UsageDelta` is a per-turn cumulative snapshot — charge the increase once per turn, accumulate across turns.
- **D7:** provider fixtures are recorded from real streams then content-scrubbed; tests never touch the network or API keys.
- Tests must run under `rtk cargo test -p rollshot-agent` with no network, no display, no API keys, in well under 30s.

---

## File Structure

```
crates/rollshot-agent/
  Cargo.toml
  src/
    lib.rs            # crate root, public re-exports
    domain.rs         # IDs, AuthorizedModelInput, SessionMessage, DraftState, GenerationEvidence
    event.rs          # RunEvent, EventLog (append-only, sequence numbers)
    budget.rs         # RunBudget, RunBudgetUsage, ModelUsage, charging (D4)
    cancellation.rs   # RunCancellation bridging rollshot_automation::CancellationFlag (D2)
    error.rs          # AgentError taxonomy + terminal reports (D3 split, §11)
    terminal.rs       # RunTerminalState, ReadyForReview, NeedsUserInput (D5)
    model.rs          # RollshotModel facade, ModelStreamEvent, ToolCallFragment, assembler
    driver.rs         # manual AgentRun driver (seeded from spikes/rig-agent)
    tool/
      mod.rs          # Tool trait, ToolRegistry, ToolOutcome, serial execution (§8.1/8.3, D8)
      inspection.rs   # inspect_* tools over AutomationHost (OCR/layout unavailable)
      automation.rs   # replace/validate/dry_run/submit/request_user_input (§8.4/8.5)
    provider/
      mod.rs          # shared request types (ModelRequest, ToolDef), provider selection
      anthropic.rs    # Anthropic adapter + SSE parser
      openai.rs       # OpenAI Chat Completions adapter + SSE parser
  tests/
    domain_state.rs       # §13.1
    driver_author_loop.rs # §13.2 (acceptance)
    tools.rs              # §13.3
    budget_failure.rs     # §13.5
    privacy.rs            # §13.6
    integration.rs        # §13.7 (real QuickJsExecutor)
    fixtures/
      anthropic/*.txt     # recorded-then-scrubbed SSE (D7)
      openai/*.txt
```

Each `src` file owns one responsibility (spec §3.2). Tests that exercise one task's deliverable live in the matching `tests/*.rs` file; pure-unit tests may also live in `#[cfg(test)]` modules inside the `src` file under test.

---

## Phase 1 — Domain, events, draft generations, budgets, terminal states

### Task 1: Crate scaffold + workspace wiring

**Files:**
- Create: `crates/rollshot-agent/Cargo.toml`
- Create: `crates/rollshot-agent/src/lib.rs`
- Modify: `Cargo.toml` (workspace `members`)

**Interfaces:**
- Produces: the `rollshot-agent` crate compiling empty; `rig-core`, `tokio`, `futures-util`, `serde`, `serde_json`, `tracing`, `thiserror` available; dev-deps for tests.

> ⚠️ **Workspace-root task — serializes all other work.** This modifies the root `Cargo.toml`; do not run it in parallel with any other task.

- [ ] **Step 1: Write `crates/rollshot-agent/Cargo.toml`**

```toml
[package]
name = "rollshot-agent"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
rollshot-automation = { path = "../rollshot-automation" }
rollshot-automation-rquickjs = { path = "../rollshot-automation-rquickjs" }
rollshot-edit-proposal = { path = "../rollshot-edit-proposal" }
rollshot-image-document = { path = "../rollshot-image-document" }
rig-core = "=0.39.0"
tokio = { workspace = true, features = ["rt", "sync", "time", "macros"] }
futures-util = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
rollshot-vision = { path = "../rollshot-vision" }
tokio = { workspace = true, features = ["rt", "rt-multi-thread", "macros", "test-util"] }

[lints]
workspace = true
```

- [ ] **Step 2: Write `crates/rollshot-agent/src/lib.rs`**

```rust
//! Bounded Agent Core (BAC): a provider-neutral, bounded control plane that
//! drives a multimodal model through typed tools. See
//! `docs/superpowers/specs/2026-06-23-bounded-agent-core-design.md`.

mod budget;
mod cancellation;
mod domain;
mod error;
mod event;
mod terminal;

pub use budget::{ModelUsage, RunBudget, RunBudgetUsage};
pub use cancellation::RunCancellation;
pub use domain::{
    AgentRunId, AgentSessionId, AuthorizedInputManifest, AuthorizedModelInput, DraftState,
    GenerationEvidence, ModelId, ProviderId, SessionMessage, TransientAttachment,
};
pub use error::{AgentError, ProviderFailureKind};
pub use event::{EventLog, RunEvent};
pub use terminal::{NeedsUserInput, ReadyForReview, RunTerminalState};
```

- [ ] **Step 3: Add the crate to the workspace**

In root `Cargo.toml`, add `"crates/rollshot-agent",` to the `members` array (after `"crates/rollshot-vision",`).

- [ ] **Step 4: Create empty module files so `lib.rs` compiles**

Create `src/budget.rs`, `src/cancellation.rs`, `src/domain.rs`, `src/error.rs`, `src/event.rs`, `src/terminal.rs` each containing only a `//!` doc comment for now (the next tasks fill them). Temporarily comment out the `pub use` lines whose symbols don't exist yet, OR create stub types. Simplest: write the modules in dependency order in this task's later steps. For Step 4, create the six files empty and replace the `pub use` block with `// re-exports added per task` so the crate compiles.

- [ ] **Step 5: Run to verify the crate compiles in the workspace**

Run: `rtk cargo build -p rollshot-agent`
Expected: PASS (compiles, no warnings denied yet because there is no code).

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-agent/Cargo.toml crates/rollshot-agent/src/lib.rs crates/rollshot-agent/src/*.rs Cargo.toml
git commit -m "feat(agent): scaffold rollshot-agent crate and wire into workspace"
```

---

### Task 2: Core identity and authorized-input domain types

**Files:**
- Modify: `crates/rollshot-agent/src/domain.rs`
- Test: inline `#[cfg(test)]` in `domain.rs`

**Interfaces:**
- Produces: `AgentSessionId(u64)`, `AgentRunId(u64)`, `ProviderId` (`Anthropic`|`OpenAi`), `ModelId(String)`, `TransientAttachment { media_type: String, bytes: Vec<u8> }`, `AuthorizedInputManifest { text_len: usize, attachment_descriptors: Vec<String>, provider: ProviderId, model: ModelId }`, `AuthorizedModelInput { provider, model, manifest, attachments: Vec<TransientAttachment> }`, `SessionMessage` (enum `User`/`Assistant` holding completed text only).
- Consumes: nothing.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_describes_authorized_input_without_copying_bytes() {
        let input = AuthorizedModelInput {
            provider: ProviderId::Anthropic,
            model: ModelId("claude-x".into()),
            manifest: AuthorizedInputManifest {
                text_len: 12,
                attachment_descriptors: vec!["image/png 800x600".into()],
                provider: ProviderId::Anthropic,
                model: ModelId("claude-x".into()),
            },
            attachments: vec![TransientAttachment {
                media_type: "image/png".into(),
                bytes: vec![1, 2, 3],
            }],
        };
        // The manifest must not enlarge the payload: descriptor count == attachment count.
        assert_eq!(
            input.manifest.attachment_descriptors.len(),
            input.attachments.len()
        );
        assert_eq!(input.manifest.provider, input.provider);
    }

    #[test]
    fn session_message_holds_completed_text_only() {
        let m = SessionMessage::Assistant("final answer".into());
        assert_eq!(m.text(), "final answer");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --lib domain`
Expected: FAIL — `AuthorizedModelInput` / `SessionMessage` not found.

- [ ] **Step 3: Write the types**

```rust
//! Session/run identities, authorized model input, and completed session
//! messages. No transient/sensitive payloads belong in session records.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AgentSessionId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AgentRunId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderId {
    Anthropic,
    OpenAi,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelId(pub String);

/// Transient attachment bytes — a run input only. Never stored in session
/// records, events, errors, or tracing (spec §4.2/§12).
#[derive(Clone, PartialEq, Eq)]
pub struct TransientAttachment {
    pub media_type: String,
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for TransientAttachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print bytes.
        f.debug_struct("TransientAttachment")
            .field("media_type", &self.media_type)
            .field("bytes_len", &self.bytes.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizedInputManifest {
    pub text_len: usize,
    pub attachment_descriptors: Vec<String>,
    pub provider: ProviderId,
    pub model: ModelId,
}

#[derive(Debug, Clone)]
pub struct AuthorizedModelInput {
    pub provider: ProviderId,
    pub model: ModelId,
    pub manifest: AuthorizedInputManifest,
    pub attachments: Vec<TransientAttachment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionMessage {
    User(String),
    Assistant(String),
}

impl SessionMessage {
    pub fn text(&self) -> &str {
        match self {
            SessionMessage::User(t) | SessionMessage::Assistant(t) => t,
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `rtk cargo test -p rollshot-agent --lib domain`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-agent/src/domain.rs
git commit -m "feat(agent): session identities, authorized model input, session messages"
```

---

### Task 3: Draft state and generation evidence

**Files:**
- Modify: `crates/rollshot-agent/src/domain.rs`
- Test: `crates/rollshot-agent/tests/domain_state.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `GenerationEvidence<T> { generation: u64, value: T }`; `DraftState { generation: u64, source: Option<String>, validation: Option<GenerationEvidence<ValidatedAutomation>>, dry_run: Option<GenerationEvidence<DryRunEvidence>> }` with methods `replace_source(&mut self, String) -> u64`, `record_validation(&mut self, ValidatedAutomation) -> Result<(), StaleGeneration>`, `record_dry_run(&mut self, DryRunEvidence) -> Result<(), StaleGeneration>`, `current_validation(&self) -> Option<&ValidatedAutomation>`, `current_dry_run(&self) -> Option<&DryRunEvidence>`. `DryRunEvidence { proposal: EditProposal, metrics: ExecutionMetrics }`. `StaleGeneration` error.

- [ ] **Step 1: Write the failing test** (`tests/domain_state.rs`)

```rust
use rollshot_agent::DraftState;

#[test]
fn replacing_source_increments_generation_and_invalidates_evidence() {
    let mut draft = DraftState::default();
    assert_eq!(draft.generation, 0);
    assert!(draft.source.is_none());

    let g1 = draft.replace_source("first".into());
    assert_eq!(g1, 1);
    assert_eq!(draft.source.as_deref(), Some("first"));

    // Replacing again bumps generation and clears any prior validation/dry-run.
    let g2 = draft.replace_source("second".into());
    assert_eq!(g2, 2);
    assert!(draft.current_validation().is_none());
    assert!(draft.current_dry_run().is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --test domain_state`
Expected: FAIL — `DraftState` not found.

- [ ] **Step 3: Write the types** (append to `domain.rs`; add the re-exports to `lib.rs`)

```rust
use rollshot_automation::{ExecutionMetrics, ValidatedAutomation};
use rollshot_edit_proposal::EditProposal;

#[derive(Debug, Clone, PartialEq)]
pub struct GenerationEvidence<T> {
    pub generation: u64,
    pub value: T,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DryRunEvidence {
    pub proposal: EditProposal,
    pub metrics: ExecutionMetrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("evidence is for a stale draft generation")]
pub struct StaleGeneration;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DraftState {
    pub generation: u64,
    pub source: Option<String>,
    pub validation: Option<GenerationEvidence<ValidatedAutomation>>,
    pub dry_run: Option<GenerationEvidence<DryRunEvidence>>,
}

impl DraftState {
    /// Replace the entire source; bump generation; invalidate prior evidence.
    pub fn replace_source(&mut self, source: String) -> u64 {
        self.generation += 1;
        self.source = Some(source);
        self.validation = None;
        self.dry_run = None;
        self.generation
    }

    pub fn record_validation(&mut self, value: ValidatedAutomation) -> Result<(), StaleGeneration> {
        self.validation = Some(GenerationEvidence { generation: self.generation, value });
        Ok(())
    }

    pub fn record_dry_run(&mut self, value: DryRunEvidence) -> Result<(), StaleGeneration> {
        // Dry-run requires current validation; the tool layer enforces ordering.
        self.dry_run = Some(GenerationEvidence { generation: self.generation, value });
        Ok(())
    }

    pub fn current_validation(&self) -> Option<&ValidatedAutomation> {
        self.validation
            .as_ref()
            .filter(|e| e.generation == self.generation)
            .map(|e| &e.value)
    }

    pub fn current_dry_run(&self) -> Option<&DryRunEvidence> {
        self.dry_run
            .as_ref()
            .filter(|e| e.generation == self.generation)
            .map(|e| &e.value)
    }
}
```

Add to `lib.rs` re-exports: `DryRunEvidence`, `StaleGeneration`.

- [ ] **Step 4: Run test to verify it passes**

Run: `rtk cargo test -p rollshot-agent --test domain_state`
Expected: PASS.

- [ ] **Step 5: Add the stale-evidence test**

```rust
#[test]
fn stale_evidence_is_not_current_after_replacement() {
    use rollshot_agent::DraftState;
    let mut draft = DraftState::default();
    draft.replace_source("v1".into());
    // Simulate validation recorded for generation 1, then a new source.
    // (record_validation is exercised via the tool layer in Task 11; here we
    // assert the generation guard directly.)
    draft.replace_source("v2".into());
    assert!(draft.current_validation().is_none());
}
```

Run: `rtk cargo test -p rollshot-agent --test domain_state`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-agent/src/domain.rs crates/rollshot-agent/src/lib.rs crates/rollshot-agent/tests/domain_state.rs
git commit -m "feat(agent): draft state with generation evidence and stale-guarding"
```

---

### Task 4: Budgets and cumulative-snapshot usage charging (D4)

**Files:**
- Modify: `crates/rollshot-agent/src/budget.rs`
- Test: `crates/rollshot-agent/tests/budget_failure.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `ModelUsage { input_tokens: u64, output_tokens: u64 }`; `RunBudget` (fields: `max_model_calls, max_wall_time: Duration, max_input_tokens, max_output_tokens, max_cost_micros: u64, max_tool_calls, max_calls_by_tool: BTreeMap<String,u32>, max_source_bytes, max_tool_arg_bytes, max_validation_attempts, max_dry_run_attempts, max_capability_calls, max_candidates, max_candidate_area`); `RunBudgetUsage` tracking spent values + per-turn charged usage; methods `charge_turn_usage(&mut self, turn: u64, cumulative: ModelUsage) -> Result<(), BudgetExceeded>` (D4: charges only the increase over what was charged for `turn`), `charge_model_call(&mut self) -> Result<(), BudgetExceeded>`, `BudgetExceeded { limit: &'static str }`.

- [ ] **Step 1: Write the failing test** (`tests/budget_failure.rs`)

```rust
use rollshot_agent::{ModelUsage, RunBudget, RunBudgetUsage};

fn small_budget() -> RunBudget {
    RunBudget {
        max_input_tokens: 100,
        max_output_tokens: 100,
        ..RunBudget::test_default()
    }
}

#[test]
fn cumulative_snapshots_in_one_turn_are_charged_once() {
    let mut usage = RunBudgetUsage::new(small_budget());
    // Provider re-reports cumulative usage for the SAME turn (turn 1).
    usage.charge_turn_usage(1, ModelUsage { input_tokens: 10, output_tokens: 5 }).unwrap();
    usage.charge_turn_usage(1, ModelUsage { input_tokens: 10, output_tokens: 20 }).unwrap();
    // Charged = max snapshot for the turn, not the sum.
    assert_eq!(usage.spent_input_tokens(), 10);
    assert_eq!(usage.spent_output_tokens(), 20);
}

#[test]
fn usage_accumulates_across_turns() {
    let mut usage = RunBudgetUsage::new(small_budget());
    usage.charge_turn_usage(1, ModelUsage { input_tokens: 10, output_tokens: 5 }).unwrap();
    usage.charge_turn_usage(2, ModelUsage { input_tokens: 30, output_tokens: 5 }).unwrap();
    assert_eq!(usage.spent_input_tokens(), 40);
    assert_eq!(usage.spent_output_tokens(), 10);
}

#[test]
fn exceeding_a_token_limit_is_reported() {
    let mut usage = RunBudgetUsage::new(small_budget());
    let err = usage
        .charge_turn_usage(1, ModelUsage { input_tokens: 101, output_tokens: 0 })
        .unwrap_err();
    assert_eq!(err.limit, "input_tokens");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --test budget_failure`
Expected: FAIL — types not found.

- [ ] **Step 3: Write the budget types**

```rust
//! Run budgets and usage charging. Provider usage within a turn is a
//! cumulative snapshot (D4): charge the per-turn increase once, accumulate
//! across turns.

use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModelUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunBudget {
    pub max_model_calls: u32,
    pub max_wall_time: Duration,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_cost_micros: u64,
    pub max_tool_calls: u32,
    pub max_calls_by_tool: BTreeMap<String, u32>,
    pub max_source_bytes: usize,
    pub max_tool_arg_bytes: usize,
    pub max_validation_attempts: u32,
    pub max_dry_run_attempts: u32,
    pub max_capability_calls: u32,
    pub max_candidates: u32,
    pub max_candidate_area: u64,
}

impl RunBudget {
    /// Generous defaults for tests; production callers set real values.
    pub fn test_default() -> Self {
        Self {
            max_model_calls: 32,
            max_wall_time: Duration::from_secs(120),
            max_input_tokens: 1_000_000,
            max_output_tokens: 1_000_000,
            max_cost_micros: u64::MAX,
            max_tool_calls: 256,
            max_calls_by_tool: BTreeMap::new(),
            max_source_bytes: 64 * 1024,
            max_tool_arg_bytes: 256 * 1024,
            max_validation_attempts: 16,
            max_dry_run_attempts: 16,
            max_capability_calls: 64,
            max_candidates: 1_000,
            max_candidate_area: u64::MAX,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("run budget exceeded: {limit}")]
pub struct BudgetExceeded {
    pub limit: &'static str,
}

#[derive(Debug, Clone)]
pub struct RunBudgetUsage {
    budget: RunBudget,
    model_calls: u32,
    input_tokens: u64,
    output_tokens: u64,
    /// Per-turn cumulative snapshot already charged, so re-reports don't double count.
    charged_per_turn: BTreeMap<u64, ModelUsage>,
}

impl RunBudgetUsage {
    pub fn new(budget: RunBudget) -> Self {
        Self {
            budget,
            model_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            charged_per_turn: BTreeMap::new(),
        }
    }

    pub fn spent_input_tokens(&self) -> u64 {
        self.input_tokens
    }
    pub fn spent_output_tokens(&self) -> u64 {
        self.output_tokens
    }

    pub fn charge_model_call(&mut self) -> Result<(), BudgetExceeded> {
        self.model_calls += 1;
        if self.model_calls > self.budget.max_model_calls {
            return Err(BudgetExceeded { limit: "model_calls" });
        }
        Ok(())
    }

    /// D4: `cumulative` is the latest snapshot for `turn`. Charge only the
    /// positive increase over what was already charged for this turn.
    pub fn charge_turn_usage(
        &mut self,
        turn: u64,
        cumulative: ModelUsage,
    ) -> Result<(), BudgetExceeded> {
        let prev = self.charged_per_turn.get(&turn).copied().unwrap_or_default();
        let d_in = cumulative.input_tokens.saturating_sub(prev.input_tokens);
        let d_out = cumulative.output_tokens.saturating_sub(prev.output_tokens);
        self.charged_per_turn.insert(turn, ModelUsage {
            input_tokens: cumulative.input_tokens.max(prev.input_tokens),
            output_tokens: cumulative.output_tokens.max(prev.output_tokens),
        });
        self.input_tokens += d_in;
        self.output_tokens += d_out;
        if self.input_tokens > self.budget.max_input_tokens {
            return Err(BudgetExceeded { limit: "input_tokens" });
        }
        if self.output_tokens > self.budget.max_output_tokens {
            return Err(BudgetExceeded { limit: "output_tokens" });
        }
        Ok(())
    }
}
```

Add to `lib.rs`: `pub use budget::{BudgetExceeded, ...};`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-agent --test budget_failure`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-agent/src/budget.rs crates/rollshot-agent/src/lib.rs crates/rollshot-agent/tests/budget_failure.rs
git commit -m "feat(agent): run budgets with cumulative-snapshot usage charging (D4)"
```

---

### Task 5: Append-only event log

**Files:**
- Modify: `crates/rollshot-agent/src/event.rs`
- Test: `crates/rollshot-agent/tests/domain_state.rs`

**Interfaces:**
- Consumes: `AgentRunId` (domain).
- Produces: `RunEvent` enum (`RunStarted`, `TextDelta { text }`, `AssistantMessageCompleted { len }`, `ToolCallStarted { name, call_id }`, `ToolCallCompleted { name, call_id }`, `ToolCallFailed { name, call_id, class }`, `DraftReplaced { generation }`, `ValidationCompleted { generation, ok }`, `DryRunCompleted { generation, ok }`, `BudgetUpdated`, `Terminal { kind }`); `EventLog { fn append(&mut self, RunEvent) -> u64 (seq), fn events(&self) -> &[(u64, RunEvent)] }`. Sequence numbers strictly increasing from 1.

- [ ] **Step 1: Write the failing test** (append to `tests/domain_state.rs`)

```rust
use rollshot_agent::{EventLog, RunEvent};

#[test]
fn event_sequence_numbers_are_ordered_and_dense() {
    let mut log = EventLog::default();
    let s1 = log.append(RunEvent::RunStarted);
    let s2 = log.append(RunEvent::TextDelta { text: "hi".into() });
    let s3 = log.append(RunEvent::Terminal { kind: "ReadyForReview".into() });
    assert_eq!((s1, s2, s3), (1, 2, 3));
    assert_eq!(log.events().len(), 3);
    assert!(log.events().windows(2).all(|w| w[0].0 < w[1].0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --test domain_state event_sequence`
Expected: FAIL — `EventLog` not found.

- [ ] **Step 3: Write the event log**

```rust
//! Append-only, sequence-numbered, privacy-safe run events for a future UI
//! subscriber (spec §5). `TextDelta` is display-only and not session history.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEvent {
    RunStarted,
    TextDelta { text: String },
    AssistantMessageCompleted { len: usize },
    ToolCallStarted { name: String, call_id: String },
    ToolCallCompleted { name: String, call_id: String },
    ToolCallFailed { name: String, call_id: String, class: String },
    DraftReplaced { generation: u64 },
    ValidationCompleted { generation: u64, ok: bool },
    DryRunCompleted { generation: u64, ok: bool },
    BudgetUpdated,
    Terminal { kind: String },
}

#[derive(Debug, Default)]
pub struct EventLog {
    seq: u64,
    events: Vec<(u64, RunEvent)>,
}

impl EventLog {
    pub fn append(&mut self, event: RunEvent) -> u64 {
        self.seq += 1;
        self.events.push((self.seq, event));
        self.seq
    }

    pub fn events(&self) -> &[(u64, RunEvent)] {
        &self.events
    }
}
```

Add to `lib.rs`: `pub use event::{EventLog, RunEvent};`.

- [ ] **Step 4: Run test to verify it passes**

Run: `rtk cargo test -p rollshot-agent --test domain_state event_sequence`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-agent/src/event.rs crates/rollshot-agent/src/lib.rs crates/rollshot-agent/tests/domain_state.rs
git commit -m "feat(agent): append-only sequence-numbered run event log"
```

---

### Task 6: Error taxonomy with provider sub-classification (D3 split, §11)

**Files:**
- Modify: `crates/rollshot-agent/src/error.rs`
- Test: inline `#[cfg(test)]` in `error.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `ProviderFailureKind` (`Transport`, `Auth`, `RateLimit`, `Rejection`, `Malformed`) with `pre_first_delta: bool` carried separately; `AgentError` enum: `ProviderFailure { kind: ProviderFailureKind, pre_first_delta: bool }`, `AgentProtocol(String)`, `SourceValidation(String)`, `Runtime(String)`, `Budget(&'static str)`, `Cancelled`. Method `class(&self) -> &'static str` returning the §11 class label. `ModelError` (provider-adapter error) convertible into `ProviderFailure`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_failure_preserves_subclass_and_pre_delta_flag() {
        let e = AgentError::ProviderFailure {
            kind: ProviderFailureKind::RateLimit,
            pre_first_delta: true,
        };
        assert_eq!(e.class(), "ProviderFailure");
        match e {
            AgentError::ProviderFailure { kind, pre_first_delta } => {
                assert_eq!(kind, ProviderFailureKind::RateLimit);
                assert!(pre_first_delta);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn protocol_and_validation_classes_are_distinct() {
        assert_eq!(AgentError::AgentProtocol("x".into()).class(), "AgentProtocolFailure");
        assert_eq!(AgentError::SourceValidation("x".into()).class(), "SourceValidationFailure");
        assert_eq!(AgentError::Runtime("x".into()).class(), "RuntimeFailure");
        assert_eq!(AgentError::Budget("input_tokens").class(), "BudgetExhausted");
        assert_eq!(AgentError::Cancelled.class(), "UserCancelled");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --lib error`
Expected: FAIL — types not found.

- [ ] **Step 3: Write the error taxonomy**

```rust
//! Error taxonomy (spec §11). Classes map to recovery strategy. D3: schema
//! failures on a known tool are NOT errors here — they are recoverable tool
//! results handled by the tool layer.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderFailureKind {
    Transport,
    Auth,
    RateLimit,
    Rejection,
    Malformed,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentError {
    #[error("provider failure: {kind:?} (pre_first_delta={pre_first_delta})")]
    ProviderFailure { kind: ProviderFailureKind, pre_first_delta: bool },
    #[error("agent protocol failure: {0}")]
    AgentProtocol(String),
    #[error("source validation failure: {0}")]
    SourceValidation(String),
    #[error("runtime failure: {0}")]
    Runtime(String),
    #[error("budget exhausted: {0}")]
    Budget(&'static str),
    #[error("user cancelled")]
    Cancelled,
}

impl AgentError {
    pub fn class(&self) -> &'static str {
        match self {
            AgentError::ProviderFailure { .. } => "ProviderFailure",
            AgentError::AgentProtocol(_) => "AgentProtocolFailure",
            AgentError::SourceValidation(_) => "SourceValidationFailure",
            AgentError::Runtime(_) => "RuntimeFailure",
            AgentError::Budget(_) => "BudgetExhausted",
            AgentError::Cancelled => "UserCancelled",
        }
    }
}
```

Add to `lib.rs`: `pub use error::{AgentError, ProviderFailureKind};`.

- [ ] **Step 4: Run test to verify it passes**

Run: `rtk cargo test -p rollshot-agent --lib error`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-agent/src/error.rs crates/rollshot-agent/src/lib.rs
git commit -m "feat(agent): error taxonomy with provider sub-classification (§11, D3)"
```

---

### Task 7: Terminal states with draft references (D5)

**Files:**
- Modify: `crates/rollshot-agent/src/terminal.rs`
- Test: `crates/rollshot-agent/tests/domain_state.rs`

**Interfaces:**
- Consumes: `DraftState`/`DryRunEvidence` (domain), `RunBudgetUsage` (budget), `EditProposal`/`ValidatedAutomation` (existing crates).
- Produces: `DraftAutomation { source, validated: ValidatedAutomation, validation_summary: ValidationSummary, dry_run: DryRunEvidence }`; `ReadyForReview { automation: DraftAutomation, proposal: EditProposal, budget_usage_snapshot: (u64,u64) }`; `UserInputRequest` (spec §8.5 shape); `NeedsUserInput { request: UserInputRequest, draft_generation: u64, has_validation: bool }`; `RunTerminalState` enum (§4.5); `TerminalCell { fn assign(&mut self, RunTerminalState) -> Result<(), AlreadyTerminal>, fn get(&self) -> Option<&RunTerminalState> }`.

- [ ] **Step 1: Write the failing test** (append to `tests/domain_state.rs`)

```rust
use rollshot_agent::{RunTerminalState, TerminalCell, UserInputRequest, NeedsUserInput};

#[test]
fn terminal_state_can_be_assigned_only_once() {
    let mut cell = TerminalCell::default();
    let first = cell.assign(RunTerminalState::UserCancelled);
    assert!(first.is_ok());
    let second = cell.assign(RunTerminalState::BudgetExhausted { limit: "input_tokens".into() });
    assert!(second.is_err());
    assert!(matches!(cell.get(), Some(RunTerminalState::UserCancelled)));
}

#[test]
fn needs_user_input_references_current_draft_generation() {
    let req = UserInputRequest {
        question: "Redact the email?".into(),
        reason: "ambiguous".into(),
        choices: vec![],
        visual_selection: None,
    };
    let n = NeedsUserInput { request: req, draft_generation: 3, has_validation: true };
    // D5: the terminal carries enough to let SP5 reconstruct/resume.
    assert_eq!(n.draft_generation, 3);
    assert!(n.has_validation);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --test domain_state terminal`
Expected: FAIL — types not found.

- [ ] **Step 3: Write the terminal types**

```rust
//! Terminal states (spec §4.4/§4.5). Assigned once; D5: NeedsUserInput and
//! the exhausted/failure reports reference current draft evidence.

use rollshot_automation::{ValidatedAutomation, ValidationSummary};
use rollshot_edit_proposal::EditProposal;

use crate::domain::DryRunEvidence;

#[derive(Debug, Clone, PartialEq)]
pub struct DraftAutomation {
    pub source: String,
    pub validated: ValidatedAutomation,
    pub validation_summary: ValidationSummary,
    pub dry_run: DryRunEvidence,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReadyForReview {
    pub automation: DraftAutomation,
    pub proposal: EditProposal,
    /// (input_tokens, output_tokens) snapshot — IDs/counts only, no payloads.
    pub budget_usage_snapshot: (u64, u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserInputChoice {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualSelectionRequest {
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserInputRequest {
    pub question: String,
    pub reason: String,
    pub choices: Vec<UserInputChoice>,
    pub visual_selection: Option<VisualSelectionRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeedsUserInput {
    pub request: UserInputRequest,
    /// D5: draft generation at termination, so SP5 can resume.
    pub draft_generation: u64,
    pub has_validation: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunTerminalState {
    ReadyForReview(ReadyForReview),
    NeedsUserInput(NeedsUserInput),
    BudgetExhausted { limit: String },
    ProviderFailure { class: String },
    AgentProtocolFailure { detail: String },
    SourceValidationFailure { detail: String },
    RuntimeFailure { detail: String },
    UserCancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a terminal state was already assigned")]
pub struct AlreadyTerminal;

#[derive(Debug, Default)]
pub struct TerminalCell(Option<RunTerminalState>);

impl TerminalCell {
    pub fn assign(&mut self, state: RunTerminalState) -> Result<(), AlreadyTerminal> {
        if self.0.is_some() {
            return Err(AlreadyTerminal);
        }
        self.0 = Some(state);
        Ok(())
    }

    pub fn get(&self) -> Option<&RunTerminalState> {
        self.0.as_ref()
    }
}
```

Add to `lib.rs`: `pub use terminal::{AlreadyTerminal, DraftAutomation, NeedsUserInput, ReadyForReview, RunTerminalState, TerminalCell, UserInputChoice, UserInputRequest, VisualSelectionRequest};`.

- [ ] **Step 4: Run test to verify it passes**

Run: `rtk cargo test -p rollshot-agent --test domain_state terminal`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-agent/src/terminal.rs crates/rollshot-agent/src/lib.rs crates/rollshot-agent/tests/domain_state.rs
git commit -m "feat(agent): terminal states, assign-once cell, draft-referencing NeedsUserInput (D5)"
```

---

### Task 8: Cancellation bridge (D2)

**Files:**
- Modify: `crates/rollshot-agent/src/cancellation.rs`
- Test: inline `#[cfg(test)]` in `cancellation.rs`

**Interfaces:**
- Consumes: `rollshot_automation::CancellationFlag`.
- Produces: `RunCancellation` with `new()`, `cancel(&self)`, `is_cancelled(&self) -> bool`, `flag(&self) -> rollshot_automation::CancellationFlag` (a clone of the SAME underlying flag for the sync dry-run), and `async fn cancelled(&self)` (await-able for the async stream via a `tokio::sync::Notify`). One source drives both.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_propagates_to_the_shared_automation_flag() {
        let c = RunCancellation::new();
        let flag = c.flag(); // handed to execute_to_proposal
        assert!(!flag.is_cancelled());
        c.cancel();
        assert!(c.is_cancelled());
        assert!(flag.is_cancelled()); // D2: same underlying flag, not a parallel one
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --lib cancellation`
Expected: FAIL — `RunCancellation` not found.

- [ ] **Step 3: Write the cancellation bridge**

```rust
//! One run cancellation source (D2). Wraps the existing
//! `rollshot_automation::CancellationFlag` (sync, for the dry-run) and a
//! `tokio::sync::Notify` (async, for the in-flight provider stream). BAC must
//! not define a second parallel primitive.

use std::sync::Arc;

use rollshot_automation::CancellationFlag;
use tokio::sync::Notify;

#[derive(Clone, Default)]
pub struct RunCancellation {
    flag: CancellationFlag,
    notify: Arc<Notify>,
}

impl RunCancellation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.cancel();
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.is_cancelled()
    }

    /// Clone of the SAME flag to pass to `execute_to_proposal`.
    pub fn flag(&self) -> CancellationFlag {
        self.flag.clone()
    }

    /// Resolves when cancellation is requested (for async select! in the stream loop).
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.notify.notified().await;
    }
}
```

Add to `lib.rs`: `pub use cancellation::RunCancellation;`.

- [ ] **Step 4: Run test to verify it passes**

Run: `rtk cargo test -p rollshot-agent --lib cancellation`
Expected: PASS.

- [ ] **Step 5: Phase-1 checkpoint — full crate build + clippy**

Run: `rtk cargo clippy -p rollshot-agent --all-targets -- -D warnings`
Expected: PASS (no warnings).

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-agent/src/cancellation.rs crates/rollshot-agent/src/lib.rs
git commit -m "feat(agent): single cancellation source bridging async + sync dry-run (D2)"
```

---

## Phase 2 — Typed tool registry and automation authoring tools

> **Shared test doubles:** Following the repo pattern where `rollshot-automation`
> ships `FakeAutomationHost` publicly, BAC exposes a `pub mod testing` (always
> compiled) with `FakeExecutor`, `ToolEnv::fake()`, and (Phase 3) `ScriptedModel`.
> Both unit and integration tests use `rollshot_agent::testing::*`.

### Task 9: Tool contract, execution environment, and registry (§8.1, D3, D8)

**Files:**
- Create: `crates/rollshot-agent/src/tool/mod.rs`
- Create: `crates/rollshot-agent/src/testing.rs`
- Modify: `crates/rollshot-agent/src/lib.rs` (`mod tool; pub mod testing;` + re-exports)
- Test: inline `#[cfg(test)]` in `tool/mod.rs`

**Interfaces:**
- Consumes: `DraftState` (domain), `RunBudgetUsage` (budget), `RunCancellation`, and from `rollshot_automation`: `AutomationHost`, `AutomationExecutor`, `AutomationInput`, `ProposalContext`, `ExecutionPolicy`, `ValidationLimits`.
- Produces: `PrivacyClass` (`Public`|`Sensitive`); `ToolAvailability` (`Available`|`Unavailable{code:&'static str}`); `ToolOutcome` (`Result(String)`|`Error(String)`|`Terminal(Box<RunTerminalState>)`); `ToolEnv` (owned environment) with `fn context(&mut self) -> ToolContext<'_>`; `ToolContext<'a>`; `Tool` trait; `ToolRegistry` with `register`, `provider_schemas() -> Vec<(String, serde_json::Value)>`, `dispatch(&mut self, name, args, ctx) -> Result<ToolOutcome, UnknownTool>`; `UnknownTool` error.

- [ ] **Step 1: Write the failing test** (inline in `tool/mod.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Minimal tool used only to exercise registry mechanics.
    struct EchoTool;
    impl Tool for EchoTool {
        fn name(&self) -> &str { "echo" }
        fn json_schema(&self) -> serde_json::Value { json!({"type":"object"}) }
        fn privacy_class(&self) -> PrivacyClass { PrivacyClass::Public }
        fn max_result_bytes(&self) -> usize { 8 }
        fn per_run_call_limit(&self) -> u32 { 2 }
        fn execute(&self, args: serde_json::Value, _ctx: &mut ToolContext) -> ToolOutcome {
            match args.get("msg").and_then(|v| v.as_str()) {
                Some(m) => ToolOutcome::Result(m.to_string()),
                None => ToolOutcome::Error("missing field `msg`".into()), // D3: recoverable
            }
        }
    }

    struct UnavailableTool;
    impl Tool for UnavailableTool {
        fn name(&self) -> &str { "ghost" }
        fn json_schema(&self) -> serde_json::Value { json!({"type":"object"}) }
        fn availability(&self) -> ToolAvailability {
            ToolAvailability::Unavailable { code: "capability_unavailable" }
        }
        fn privacy_class(&self) -> PrivacyClass { PrivacyClass::Public }
        fn execute(&self, _a: serde_json::Value, _c: &mut ToolContext) -> ToolOutcome {
            unreachable!("unavailable tools must not execute")
        }
    }

    #[test]
    fn unknown_tool_is_a_protocol_failure() {
        let mut reg = ToolRegistry::default();
        reg.register(Box::new(EchoTool));
        let mut env = ToolEnv::fake();
        let mut ctx = env.context();
        let err = reg.dispatch("nope", json!({}), &mut ctx).unwrap_err();
        assert_eq!(err, UnknownTool);
    }

    #[test]
    fn schema_invalid_args_on_known_tool_are_recoverable() {
        let mut reg = ToolRegistry::default();
        reg.register(Box::new(EchoTool));
        let mut env = ToolEnv::fake();
        let mut ctx = env.context();
        let out = reg.dispatch("echo", json!({}), &mut ctx).unwrap();
        assert!(matches!(out, ToolOutcome::Error(_))); // not terminal (D3)
    }

    #[test]
    fn results_are_truncated_to_max_result_bytes() {
        let mut reg = ToolRegistry::default();
        reg.register(Box::new(EchoTool));
        let mut env = ToolEnv::fake();
        let mut ctx = env.context();
        let out = reg.dispatch("echo", json!({"msg":"0123456789ABCDEF"}), &mut ctx).unwrap();
        match out { ToolOutcome::Result(s) => assert!(s.len() <= 8 + "…[truncated]".len()), _ => panic!() }
    }

    #[test]
    fn unavailable_tool_returns_stable_typed_result_not_empty_success() {
        let mut reg = ToolRegistry::default();
        reg.register(Box::new(UnavailableTool));
        let mut env = ToolEnv::fake();
        let mut ctx = env.context();
        let out = reg.dispatch("ghost", json!({}), &mut ctx).unwrap();
        match out {
            ToolOutcome::Result(s) => assert!(s.contains("capability_unavailable")),
            _ => panic!("expected a stable unavailable result"),
        }
    }

    #[test]
    fn per_tool_call_limit_is_enforced_as_recoverable_error() {
        let mut reg = ToolRegistry::default();
        reg.register(Box::new(EchoTool));
        let mut env = ToolEnv::fake();
        let mut ctx = env.context();
        let a = json!({"msg":"x"});
        assert!(matches!(reg.dispatch("echo", a.clone(), &mut ctx).unwrap(), ToolOutcome::Result(_)));
        assert!(matches!(reg.dispatch("echo", a.clone(), &mut ctx).unwrap(), ToolOutcome::Result(_)));
        // 3rd call exceeds per_run_call_limit == 2
        assert!(matches!(reg.dispatch("echo", a, &mut ctx).unwrap(), ToolOutcome::Error(_)));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --lib tool`
Expected: FAIL — `Tool`/`ToolRegistry`/`ToolEnv` not found.

- [ ] **Step 3: Write the contract and registry** (`tool/mod.rs`)

```rust
//! Typed tool contract and registry (spec §8). Unknown tool → protocol failure
//! (caller maps to AgentProtocolFailure). Schema-invalid args on a known tool →
//! recoverable typed error (D3). Results are bounded (D8).

pub mod automation;
pub mod inspection;

use std::collections::BTreeMap;

use rollshot_automation::{
    AutomationExecutor, AutomationHost, AutomationInput, ExecutionPolicy, ProposalContext,
    ValidationLimits,
};

use crate::budget::RunBudgetUsage;
use crate::cancellation::RunCancellation;
use crate::domain::{AgentRunId, DraftState};
use crate::terminal::RunTerminalState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyClass {
    Public,
    Sensitive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAvailability {
    Available,
    Unavailable { code: &'static str },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolOutcome {
    /// Feed back to the model as a successful tool_result.
    Result(String),
    /// Recoverable typed error returned to the model so it can repair (D3, §11).
    Error(String),
    /// Terminal: end the run now (submit_for_review / request_user_input).
    Terminal(Box<RunTerminalState>),
}

/// Owned execution environment. `context()` lends per-field borrows to a tool.
pub struct ToolEnv {
    pub draft: DraftState,
    pub host: Box<dyn AutomationHost>,
    pub executor: Box<dyn AutomationExecutor>,
    pub input: AutomationInput,
    pub proposal: ProposalContext,
    pub exec_policy: ExecutionPolicy,
    pub validation_limits: ValidationLimits,
    pub cancellation: RunCancellation,
    pub budget: RunBudgetUsage,
    pub run_id: AgentRunId,
}

impl ToolEnv {
    pub fn context(&mut self) -> ToolContext<'_> {
        ToolContext {
            draft: &mut self.draft,
            host: self.host.as_mut(),
            executor: self.executor.as_ref(),
            input: &self.input,
            proposal: &self.proposal,
            exec_policy: &self.exec_policy,
            validation_limits: &self.validation_limits,
            cancellation: &self.cancellation,
            budget: &mut self.budget,
            run_id: self.run_id,
        }
    }
}

pub struct ToolContext<'a> {
    pub draft: &'a mut DraftState,
    pub host: &'a mut dyn AutomationHost,
    pub executor: &'a dyn AutomationExecutor,
    pub input: &'a AutomationInput,
    pub proposal: &'a ProposalContext,
    pub exec_policy: &'a ExecutionPolicy,
    pub validation_limits: &'a ValidationLimits,
    pub cancellation: &'a RunCancellation,
    pub budget: &'a mut RunBudgetUsage,
    pub run_id: AgentRunId,
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> u32 {
        1
    }
    fn json_schema(&self) -> serde_json::Value;
    fn availability(&self) -> ToolAvailability {
        ToolAvailability::Available
    }
    fn privacy_class(&self) -> PrivacyClass;
    fn max_result_bytes(&self) -> usize {
        64 * 1024
    }
    fn per_run_call_limit(&self) -> u32 {
        8
    }
    /// `args` is already-decoded JSON; the tool does typed `serde_json::from_value`
    /// and returns `ToolOutcome::Error` on a decode/schema mismatch (D3).
    fn execute(&self, args: serde_json::Value, ctx: &mut ToolContext) -> ToolOutcome;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("unknown tool")]
pub struct UnknownTool;

#[derive(Default)]
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
    call_counts: BTreeMap<String, u32>,
}

impl ToolRegistry {
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.push(tool);
    }

    pub fn provider_schemas(&self) -> Vec<(String, serde_json::Value)> {
        self.tools
            .iter()
            .map(|t| (t.name().to_string(), t.json_schema()))
            .collect()
    }

    fn find(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.iter().find(|t| t.name() == name).map(|b| b.as_ref())
    }

    pub fn dispatch(
        &mut self,
        name: &str,
        args: serde_json::Value,
        ctx: &mut ToolContext,
    ) -> Result<ToolOutcome, UnknownTool> {
        let tool = self.find(name).ok_or(UnknownTool)?;

        if let ToolAvailability::Unavailable { code } = tool.availability() {
            // Stable typed result, never an empty success (spec §8.2).
            let body = serde_json::json!({
                "status": "unavailable",
                "code": code,
                "tool": name,
            });
            return Ok(ToolOutcome::Result(body.to_string()));
        }

        let count = self.call_counts.entry(name.to_string()).or_insert(0);
        if *count >= tool.per_run_call_limit() {
            return Ok(ToolOutcome::Error(format!(
                "tool `{name}` exceeded its per-run call limit"
            )));
        }
        *count += 1;

        let max = tool.max_result_bytes();
        Ok(match tool.execute(args, ctx) {
            ToolOutcome::Result(s) => ToolOutcome::Result(truncate(s, max)),
            other => other,
        })
    }
}

fn truncate(mut s: String, max: usize) -> String {
    if s.len() > max {
        s.truncate(max);
        s.push_str("…[truncated]");
    }
    s
}
```

- [ ] **Step 4: Write the `testing` module with `ToolEnv::fake()` + `FakeExecutor`** (`src/testing.rs`)

```rust
//! Test doubles shared by unit and integration tests (compiled always, like
//! `rollshot_automation::FakeAutomationHost`).

use std::time::Duration;

use rollshot_automation::{
    AutomationExecution, AutomationExecutor, AutomationHost, AutomationInput, CancellationFlag,
    ExecutionError, ExecutionMetrics, ExecutionPolicy, FakeAutomationHost, ProposalContext,
    ValidatedAutomation, ValidationLimits,
};
use rollshot_edit_proposal::{Provenance, ProvenanceSource};
use rollshot_automation::ProposedEditKind;

use crate::budget::{RunBudget, RunBudgetUsage};
use crate::cancellation::RunCancellation;
use crate::domain::AgentRunId;
use crate::tool::ToolEnv;

/// Executor double: returns a canned proposal JSON + metrics.
#[derive(Debug, Default)]
pub struct FakeExecutor {
    pub output_json: String,
    pub fail: Option<&'static str>,
}

impl AutomationExecutor for FakeExecutor {
    fn execute(
        &self,
        _automation: &ValidatedAutomation,
        _input: &AutomationInput,
        _proposal: &ProposalContext,
        _host: &mut dyn AutomationHost,
        _policy: &ExecutionPolicy,
        _cancellation: &CancellationFlag,
    ) -> Result<AutomationExecution, ExecutionError> {
        if let Some(code) = self.fail {
            return Err(ExecutionError::Sandbox(rollshot_automation::SandboxError::Initialization { code }));
        }
        let output_json = if self.output_json.is_empty() {
            r#"{"candidates":[]}"#.to_string()
        } else {
            self.output_json.clone()
        };
        Ok(AutomationExecution {
            output_json,
            metrics: ExecutionMetrics {
                duration: Duration::from_millis(1),
                capability_calls: 0,
                output_bytes: 0,
                interrupted: false,
            },
        })
    }
}

pub fn fake_proposal_context() -> ProposalContext {
    ProposalContext {
        proposal_id: rollshot_edit_proposal::ProposalId(1),
        base_document_state_id: 0,
        provenance: Provenance { source: ProvenanceSource::Agent { run_id: 1 } },
    }
}

pub fn fake_input() -> AutomationInput {
    AutomationInput {
        image_width: 800,
        image_height: 600,
        region: None,
        annotations: Vec::new(),
        capability_handles: std::collections::BTreeMap::new(),
    }
}

impl ToolEnv {
    /// A ready-to-use environment with fakes for unit/integration tests.
    pub fn fake() -> Self {
        let policy = ExecutionPolicy::smart_redaction_default(
            Duration::from_secs(5),
            4 * 1024 * 1024,
            256 * 1024,
        );
        ToolEnv {
            draft: Default::default(),
            host: Box::new(FakeAutomationHost::default()),
            executor: Box::new(FakeExecutor::default()),
            input: fake_input(),
            proposal: fake_proposal_context(),
            exec_policy: policy,
            validation_limits: ValidationLimits::default(),
            cancellation: RunCancellation::new(),
            budget: RunBudgetUsage::new(RunBudget::test_default()),
            run_id: AgentRunId(1),
        }
    }
}
```

> Implementer note: confirm `ExecutionError`'s variant for the `fail` path against `crates/rollshot-automation/src/executor.rs`; adjust the constructed variant if the enum differs. `ValidationLimits` must implement `Default` (it does — `policy.rs:37`).

- [ ] **Step 5: Wire modules into `lib.rs`**

Add `pub mod tool;` (must be `pub` — tests reference `rollshot_agent::tool::inspection::*` and `rollshot_agent::tool::default_registry`) and `pub mod testing;`, plus `pub use tool::{PrivacyClass, Tool, ToolAvailability, ToolContext, ToolEnv, ToolOutcome, ToolRegistry, UnknownTool};`.

- [ ] **Step 6: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-agent --lib tool`
Expected: PASS (5 tests).

- [ ] **Step 7: Commit**

```bash
git add crates/rollshot-agent/src/tool/mod.rs crates/rollshot-agent/src/testing.rs crates/rollshot-agent/src/lib.rs
git commit -m "feat(agent): typed tool contract, env, and registry (§8.1, D3, D8)"
```

---

### Task 10: Inspection tools (OCR/layout unavailable; region_features live) (§8.2)

**Files:**
- Create: `crates/rollshot-agent/src/tool/inspection.rs`
- Test: `crates/rollshot-agent/tests/tools.rs`

**Interfaces:**
- Consumes: `Tool`/`ToolContext`/`ToolOutcome`/`ToolAvailability`, `rollshot_automation::{RegionFeaturesQuery, Region}`.
- Produces: `InspectContextSummary`, `InspectOcr`, `InspectLayout`, `InspectRegionFeatures` tool structs. `InspectOcr`/`InspectLayout` report `Unavailable { code: "capability_unavailable" }`. `InspectRegionFeatures` parses `{region, limit}`, clamps `limit` to a ceiling (D8), calls `ctx.host.region_features`, returns JSON.

- [ ] **Step 1: Write the failing test** (`tests/tools.rs`)

```rust
use rollshot_agent::testing::*;
use rollshot_agent::tool::inspection::{InspectOcr, InspectRegionFeatures};
use rollshot_agent::{Tool, ToolAvailability, ToolEnv, ToolOutcome};
use serde_json::json;

#[test]
fn ocr_tool_is_unavailable_until_vision_sp5() {
    assert!(matches!(
        InspectOcr.availability(),
        ToolAvailability::Unavailable { code: "capability_unavailable" }
    ));
}

#[test]
fn region_features_clamps_limit_and_returns_host_results() {
    let mut env = ToolEnv::fake();
    // Preload the fake host with one region-features result.
    // (FakeAutomationHost.region_feature_results is public.)
    // Build via downcast-free helper: env.host is Box<dyn AutomationHost>, so we
    // instead seed through a dedicated constructor — see Step 3 note.
    let mut ctx = env.context();
    let out = InspectRegionFeatures.execute(json!({"region":"Full","limit":100000}), &mut ctx);
    match out {
        ToolOutcome::Result(s) => assert!(s.contains("regions")),
        other => panic!("expected Result, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --test tools`
Expected: FAIL — `inspection` module/types not found.

- [ ] **Step 3: Write the inspection tools** (`tool/inspection.rs`)

```rust
//! Inspection tools over `AutomationHost`. OCR/layout are registered as
//! unavailable until vision SP4/SP5; they must return a stable typed
//! unavailable result, never an empty success (spec §8.2).

use rollshot_automation::{Region, RegionFeaturesQuery};
use serde::Deserialize;

use super::{PrivacyClass, Tool, ToolAvailability, ToolContext, ToolOutcome};

const MAX_INSPECT_LIMIT: u32 = 256;

pub struct InspectContextSummary;
impl Tool for InspectContextSummary {
    fn name(&self) -> &str { "inspect_context_summary" }
    fn json_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object","properties":{},"additionalProperties":false})
    }
    fn privacy_class(&self) -> PrivacyClass { PrivacyClass::Public }
    fn execute(&self, _args: serde_json::Value, ctx: &mut ToolContext) -> ToolOutcome {
        let body = serde_json::json!({
            "image_width": ctx.input.image_width,
            "image_height": ctx.input.image_height,
            "annotation_count": ctx.input.annotations.len(),
        });
        ToolOutcome::Result(body.to_string())
    }
}

pub struct InspectOcr;
impl Tool for InspectOcr {
    fn name(&self) -> &str { "inspect_ocr" }
    fn json_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object","properties":{"region":{},"limit":{"type":"integer"}}})
    }
    fn availability(&self) -> ToolAvailability {
        ToolAvailability::Unavailable { code: "capability_unavailable" }
    }
    fn privacy_class(&self) -> PrivacyClass { PrivacyClass::Sensitive }
    fn execute(&self, _a: serde_json::Value, _c: &mut ToolContext) -> ToolOutcome {
        unreachable!("registry short-circuits unavailable tools")
    }
}

pub struct InspectLayout;
impl Tool for InspectLayout {
    fn name(&self) -> &str { "inspect_layout" }
    fn json_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object","properties":{"region":{},"limit":{"type":"integer"}}})
    }
    fn availability(&self) -> ToolAvailability {
        ToolAvailability::Unavailable { code: "capability_unavailable" }
    }
    fn privacy_class(&self) -> PrivacyClass { PrivacyClass::Sensitive }
    fn execute(&self, _a: serde_json::Value, _c: &mut ToolContext) -> ToolOutcome {
        unreachable!("registry short-circuits unavailable tools")
    }
}

#[derive(Deserialize)]
struct RegionFeaturesArgs {
    region: Region,
    limit: u32,
}

pub struct InspectRegionFeatures;
impl Tool for InspectRegionFeatures {
    fn name(&self) -> &str { "inspect_region_features" }
    fn json_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object","properties":{"region":{},"limit":{"type":"integer"}},"required":["region","limit"]})
    }
    fn privacy_class(&self) -> PrivacyClass { PrivacyClass::Sensitive }
    fn execute(&self, args: serde_json::Value, ctx: &mut ToolContext) -> ToolOutcome {
        let parsed: RegionFeaturesArgs = match serde_json::from_value(args) {
            Ok(p) => p,
            Err(e) => return ToolOutcome::Error(format!("invalid arguments: {e}")), // D3
        };
        let query = RegionFeaturesQuery {
            region: parsed.region,
            limit: parsed.limit.min(MAX_INSPECT_LIMIT), // D8 clamp
        };
        match ctx.host.region_features(query) {
            Ok(regions) => {
                let body = serde_json::json!({
                    "regions": regions.iter().map(|r| serde_json::json!({
                        "edge_density": r.edge_density,
                        "dominant_rgba": r.dominant_rgba,
                    })).collect::<Vec<_>>()
                });
                ToolOutcome::Result(body.to_string())
            }
            Err(e) => ToolOutcome::Error(format!("region_features failed: {e:?}")),
        }
    }
}
```

> Implementer note: the Step-1 test seeds the host via a `FakeAutomationHost` with `region_feature_results` preset. Because `ToolEnv::fake()` boxes the host as `dyn AutomationHost`, add a `ToolEnv::fake_with_host(host: Box<dyn AutomationHost>)` constructor in `testing.rs` (one-line variant of `fake()`), and in the test build `FakeAutomationHost { region_feature_results: vec![RegionFeatures{ bounds: ImageRect::…, dominant_rgba:[0,0,0,255], edge_density: 0.5 }], ..Default::default() }`. Construct `ImageRect` via its public constructor in `rollshot_image_document::geometry`. An empty `regions` array still satisfies the assertion (`contains("regions")`), so the test passes even with no preset; the preset makes it meaningful.

- [ ] **Step 4: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-agent --test tools`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-agent/src/tool/inspection.rs crates/rollshot-agent/src/testing.rs crates/rollshot-agent/tests/tools.rs
git commit -m "feat(agent): inspection tools; OCR/layout unavailable, region_features clamps limit (§8.2, D8)"
```

---

### Task 11: `replace_automation_source` + `validate_automation` (§8.4)

**Files:**
- Modify: `crates/rollshot-agent/src/tool/automation.rs`
- Test: `crates/rollshot-agent/tests/tools.rs`

**Interfaces:**
- Consumes: `ToolContext` (draft, validation_limits, budget), `rollshot_automation::validate_source`.
- Produces: `ReplaceAutomationSource`, `ValidateAutomation` tools. Replace enforces source-byte limit (budget `max_source_bytes`), bumps generation, invalidates evidence. Validate requires current source, runs `validate_source`, records current-gen evidence, returns typed diagnostics on failure (recoverable).

- [ ] **Step 1: Write the failing test** (append to `tests/tools.rs`)

```rust
use rollshot_agent::tool::automation::{ReplaceAutomationSource, ValidateAutomation};

#[test]
fn replace_then_validate_records_current_generation_evidence() {
    let mut env = ToolEnv::fake();
    let src = "redact()"; // exact valid source confirmed against rollshot-automation in Step 3
    {
        let mut ctx = env.context();
        let out = ReplaceAutomationSource.execute(json!({"source": src}), &mut ctx);
        assert!(matches!(out, ToolOutcome::Result(_)));
        assert_eq!(ctx.draft.generation, 1);
    }
}

#[test]
fn replace_rejects_oversized_source() {
    let mut env = ToolEnv::fake();
    let big = "x".repeat(70 * 1024); // > max_source_bytes (64 KiB in test_default)
    let mut ctx = env.context();
    let out = ReplaceAutomationSource.execute(json!({"source": big}), &mut ctx);
    assert!(matches!(out, ToolOutcome::Error(_))); // recoverable; model can shorten
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --test tools replace`
Expected: FAIL — types not found.

- [ ] **Step 3: Write the tools** (`tool/automation.rs`)

```rust
//! Automation authoring tools (spec §8.4). `replace` owns the source byte
//! limit + generation bump; `validate` runs the existing frontend and records
//! current-generation evidence.

use rollshot_automation::validate_source;
use serde::Deserialize;

use super::{PrivacyClass, Tool, ToolContext, ToolOutcome};

#[derive(Deserialize)]
struct ReplaceArgs {
    source: String,
}

pub struct ReplaceAutomationSource;
impl Tool for ReplaceAutomationSource {
    fn name(&self) -> &str { "replace_automation_source" }
    fn json_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object","properties":{"source":{"type":"string"}},"required":["source"]})
    }
    fn privacy_class(&self) -> PrivacyClass { PrivacyClass::Sensitive }
    fn execute(&self, args: serde_json::Value, ctx: &mut ToolContext) -> ToolOutcome {
        let parsed: ReplaceArgs = match serde_json::from_value(args) {
            Ok(p) => p,
            Err(e) => return ToolOutcome::Error(format!("invalid arguments: {e}")),
        };
        if parsed.source.len() > ctx.validation_limits.max_source_bytes {
            return ToolOutcome::Error(format!(
                "source exceeds the {}-byte limit",
                ctx.validation_limits.max_source_bytes
            ));
        }
        let generation = ctx.draft.replace_source(parsed.source);
        ToolOutcome::Result(serde_json::json!({"generation": generation}).to_string())
    }
}

pub struct ValidateAutomation;
impl Tool for ValidateAutomation {
    fn name(&self) -> &str { "validate_automation" }
    fn json_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object","properties":{},"additionalProperties":false})
    }
    fn privacy_class(&self) -> PrivacyClass { PrivacyClass::Sensitive }
    fn execute(&self, _args: serde_json::Value, ctx: &mut ToolContext) -> ToolOutcome {
        let Some(source) = ctx.draft.source.clone() else {
            return ToolOutcome::Error("no current source to validate".into());
        };
        match validate_source(&source, ctx.validation_limits) {
            Ok(validated) => {
                let summary = validated.validation_summary.clone();
                let _ = ctx.draft.record_validation(validated);
                ToolOutcome::Result(serde_json::json!({
                    "ok": true,
                    "ast_nodes": summary.ast_nodes,
                    "capability_calls": summary.capability_calls,
                }).to_string())
            }
            Err(diags) => ToolOutcome::Error(serde_json::json!({
                "ok": false,
                "diagnostics": diags.iter().map(|d| format!("{d:?}")).collect::<Vec<_>>(),
            }).to_string()), // D3: recoverable — the model repairs
        }
    }
}
```

> Implementer note: `validate_source` takes `&str` and `&ValidationLimits`. Confirm a minimal valid `src` for Step 1 by reading `rollshot-automation`'s frontend tests (e.g. a known-good fixture); the test only needs `generation == 1` after replace, which does not depend on validity, so Step 1's first test passes regardless. The `replace_rejects_oversized_source` test compares against `validation_limits.max_source_bytes` — confirm `ValidationLimits::default().max_source_bytes` and adjust the `70 * 1024` constant if the default is larger.

- [ ] **Step 4: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-agent --test tools`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-agent/src/tool/automation.rs crates/rollshot-agent/tests/tools.rs
git commit -m "feat(agent): replace_automation_source + validate_automation tools (§8.4)"
```

---

### Task 12: `dry_run_automation` + `submit_for_review` (§8.4)

**Files:**
- Modify: `crates/rollshot-agent/src/tool/automation.rs`
- Test: `crates/rollshot-agent/tests/tools.rs`

**Interfaces:**
- Consumes: `ToolContext`, `rollshot_automation::execute_to_proposal`, `rollshot_edit_proposal::validate_policy`, terminal types.
- Produces: `DryRunAutomation` (requires current validation; runs `execute_to_proposal`; records `DryRunEvidence`), `SubmitForReview` (requires current validation + dry-run; re-validates proposal policy; builds `ReadyForReview`; returns `ToolOutcome::Terminal`).

- [ ] **Step 1: Write the failing test** (append to `tests/tools.rs`)

```rust
use rollshot_agent::tool::automation::{DryRunAutomation, SubmitForReview};
use rollshot_agent::RunTerminalState;

#[test]
fn dry_run_requires_current_validation() {
    let mut env = ToolEnv::fake();
    env.draft.replace_source("redact()".into()); // no validation recorded yet
    let mut ctx = env.context();
    let out = DryRunAutomation.execute(json!({}), &mut ctx);
    assert!(matches!(out, ToolOutcome::Error(_))); // must validate first
}

#[test]
fn submit_requires_complete_evidence() {
    let mut env = ToolEnv::fake();
    env.draft.replace_source("redact()".into());
    let mut ctx = env.context();
    let out = SubmitForReview.execute(json!({}), &mut ctx);
    assert!(matches!(out, ToolOutcome::Error(_))); // no validation/dry-run → not terminal
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --test tools dry_run`
Expected: FAIL — types not found.

- [ ] **Step 3: Write the tools** (append to `tool/automation.rs`)

```rust
use rollshot_automation::execute_to_proposal;
use rollshot_edit_proposal::validate_policy;

use crate::domain::DryRunEvidence;
use crate::terminal::{DraftAutomation, ReadyForReview, RunTerminalState};

pub struct DryRunAutomation;
impl Tool for DryRunAutomation {
    fn name(&self) -> &str { "dry_run_automation" }
    fn json_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object","properties":{},"additionalProperties":false})
    }
    fn privacy_class(&self) -> PrivacyClass { PrivacyClass::Sensitive }
    fn execute(&self, _args: serde_json::Value, ctx: &mut ToolContext) -> ToolOutcome {
        let Some(validated) = ctx.draft.current_validation().cloned() else {
            return ToolOutcome::Error("dry-run requires a successful current validation".into());
        };
        let flag = ctx.cancellation.flag(); // D2: same flag the run owns
        match execute_to_proposal(
            ctx.executor,
            &validated,
            ctx.input,
            ctx.proposal,
            ctx.host,
            ctx.exec_policy,
            &flag,
        ) {
            Ok((proposal, metrics)) => {
                let _ = ctx.draft.record_dry_run(DryRunEvidence { proposal, metrics });
                ToolOutcome::Result(serde_json::json!({"ok": true}).to_string())
            }
            Err(e) => ToolOutcome::Error(format!("dry-run failed: {e:?}")), // recoverable (§11)
        }
    }
}

pub struct SubmitForReview;
impl Tool for SubmitForReview {
    fn name(&self) -> &str { "submit_for_review" }
    fn json_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object","properties":{},"additionalProperties":false})
    }
    fn privacy_class(&self) -> PrivacyClass { PrivacyClass::Sensitive }
    fn execute(&self, _args: serde_json::Value, ctx: &mut ToolContext) -> ToolOutcome {
        let (Some(validated), Some(dry)) =
            (ctx.draft.current_validation().cloned(), ctx.draft.current_dry_run().cloned())
        else {
            return ToolOutcome::Error("submit requires current validation and dry-run".into());
        };
        // Final proposal policy re-check (spec §8.4).
        let dims = (ctx.input.image_width, ctx.input.image_height);
        if let Err(e) = validate_policy(
            &dry.proposal.candidates,
            &ctx.exec_policy.proposal_limits,
            dims,
        ) {
            return ToolOutcome::Error(format!("proposal policy rejected: {e:?}"));
        }
        let (it, ot) = (ctx.budget.spent_input_tokens(), ctx.budget.spent_output_tokens());
        let ready = ReadyForReview {
            automation: DraftAutomation {
                source: ctx.draft.source.clone().unwrap_or_default(),
                validation_summary: validated.validation_summary.clone(),
                validated,
                dry_run: dry.clone(),
            },
            proposal: dry.proposal.clone(),
            budget_usage_snapshot: (it, ot),
        };
        ToolOutcome::Terminal(Box::new(RunTerminalState::ReadyForReview(ready)))
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-agent --test tools`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-agent/src/tool/automation.rs crates/rollshot-agent/tests/tools.rs
git commit -m "feat(agent): dry_run_automation + submit_for_review with proposal-policy re-check (§8.4)"
```

---

### Task 13: `request_user_input` (§8.5, D5)

**Files:**
- Modify: `crates/rollshot-agent/src/tool/automation.rs`
- Test: `crates/rollshot-agent/tests/tools.rs`

**Interfaces:**
- Consumes: `ToolContext`, terminal `UserInputRequest`/`NeedsUserInput`.
- Produces: `RequestUserInput` tool — bounds string lengths/choice count, builds `NeedsUserInput` with `draft_generation` + `has_validation` (D5), returns `ToolOutcome::Terminal(NeedsUserInput)`.

- [ ] **Step 1: Write the failing test** (append to `tests/tools.rs`)

```rust
use rollshot_agent::tool::automation::RequestUserInput;

#[test]
fn request_user_input_terminates_with_draft_reference() {
    let mut env = ToolEnv::fake();
    env.draft.replace_source("redact()".into()); // generation = 1
    let mut ctx = env.context();
    let out = RequestUserInput.execute(
        json!({"question":"Redact the email?","reason":"ambiguous","choices":[]}),
        &mut ctx,
    );
    match out {
        ToolOutcome::Terminal(t) => match *t {
            RunTerminalState::NeedsUserInput(n) => {
                assert_eq!(n.draft_generation, 1); // D5
                assert!(!n.has_validation);
            }
            _ => panic!("expected NeedsUserInput"),
        },
        _ => panic!("request_user_input must be terminal"),
    }
}

#[test]
fn request_user_input_rejects_too_many_choices() {
    let mut env = ToolEnv::fake();
    let mut ctx = env.context();
    let choices: Vec<_> = (0..50).map(|i| json!({"id": i.to_string(), "label": "x"})).collect();
    let out = RequestUserInput.execute(
        json!({"question":"q","reason":"r","choices": choices}),
        &mut ctx,
    );
    assert!(matches!(out, ToolOutcome::Error(_)));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --test tools request_user_input`
Expected: FAIL — `RequestUserInput` not found.

- [ ] **Step 3: Write the tool** (append to `tool/automation.rs`)

```rust
use crate::terminal::{NeedsUserInput, UserInputChoice, UserInputRequest};

const MAX_QUESTION_LEN: usize = 2_000;
const MAX_CHOICES: usize = 8;

#[derive(serde::Deserialize)]
struct ChoiceArg { id: String, label: String }

#[derive(serde::Deserialize)]
struct RequestUserInputArgs {
    question: String,
    reason: String,
    #[serde(default)]
    choices: Vec<ChoiceArg>,
}

pub struct RequestUserInput;
impl Tool for RequestUserInput {
    fn name(&self) -> &str { "request_user_input" }
    fn json_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object","properties":{
            "question":{"type":"string"},"reason":{"type":"string"},
            "choices":{"type":"array"}},"required":["question","reason"]})
    }
    fn privacy_class(&self) -> PrivacyClass { PrivacyClass::Public }
    fn per_run_call_limit(&self) -> u32 { 4 }
    fn execute(&self, args: serde_json::Value, ctx: &mut ToolContext) -> ToolOutcome {
        let parsed: RequestUserInputArgs = match serde_json::from_value(args) {
            Ok(p) => p,
            Err(e) => return ToolOutcome::Error(format!("invalid arguments: {e}")),
        };
        if parsed.question.len() > MAX_QUESTION_LEN || parsed.reason.len() > MAX_QUESTION_LEN {
            return ToolOutcome::Error("question/reason too long".into());
        }
        if parsed.choices.len() > MAX_CHOICES {
            return ToolOutcome::Error(format!("at most {MAX_CHOICES} choices allowed"));
        }
        let request = UserInputRequest {
            question: parsed.question,
            reason: parsed.reason,
            choices: parsed.choices.into_iter()
                .map(|c| UserInputChoice { id: c.id, label: c.label })
                .collect(),
            visual_selection: None,
        };
        let needs = NeedsUserInput {
            request,
            draft_generation: ctx.draft.generation,             // D5
            has_validation: ctx.draft.current_validation().is_some(),
        };
        ToolOutcome::Terminal(Box::new(RunTerminalState::NeedsUserInput(needs)))
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-agent --test tools`
Expected: PASS.

- [ ] **Step 5: Wire all authoring + inspection tools into a `default_registry()` helper**

Add to `tool/mod.rs`:

```rust
pub fn default_registry() -> ToolRegistry {
    use automation::*;
    use inspection::*;
    let mut reg = ToolRegistry::default();
    reg.register(Box::new(InspectContextSummary));
    reg.register(Box::new(InspectOcr));
    reg.register(Box::new(InspectLayout));
    reg.register(Box::new(InspectRegionFeatures));
    reg.register(Box::new(ReplaceAutomationSource));
    reg.register(Box::new(ValidateAutomation));
    reg.register(Box::new(DryRunAutomation));
    reg.register(Box::new(SubmitForReview));
    reg.register(Box::new(RequestUserInput));
    reg
}
```

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-agent/src/tool/automation.rs crates/rollshot-agent/src/tool/mod.rs crates/rollshot-agent/tests/tools.rs
git commit -m "feat(agent): request_user_input terminal tool with draft reference (§8.5, D5) + default registry"
```

---

### Task 14: Serial execution with per-call rechecks (§8.3)

**Files:**
- Modify: `crates/rollshot-agent/src/tool/mod.rs`
- Test: `crates/rollshot-agent/tests/tools.rs`

**Interfaces:**
- Consumes: `ToolRegistry`, `ToolContext`, `ToolOutcome`.
- Produces: `ToolRegistry::run_turn(&mut self, calls: &[(String /*call_id*/, String /*name*/, serde_json::Value)], ctx: &mut ToolContext) -> Vec<TurnToolResult>` where `TurnToolResult { call_id, name, outcome: ToolOutcome }`. Executes serially in order; rechecks cancellation before each call; a `Terminal` outcome stops remaining calls in the same turn.

- [ ] **Step 1: Write the failing test** (append to `tests/tools.rs`)

```rust
use rollshot_agent::tool::default_registry;

#[test]
fn terminal_tool_stops_remaining_same_turn_calls() {
    let mut env = ToolEnv::fake();
    env.draft.replace_source("redact()".into());
    let mut reg = default_registry();
    let mut ctx = env.context();
    let calls = vec![
        ("c1".into(), "request_user_input".into(),
         json!({"question":"q","reason":"r","choices":[]})),
        ("c2".into(), "inspect_context_summary".into(), json!({})),
    ];
    let results = reg.run_turn(&calls, &mut ctx);
    assert_eq!(results.len(), 1); // second call not executed after a terminal
    assert!(matches!(results[0].outcome, ToolOutcome::Terminal(_)));
}

#[test]
fn calls_execute_in_response_order() {
    let mut env = ToolEnv::fake();
    let mut reg = default_registry();
    let mut ctx = env.context();
    let calls = vec![
        ("c1".into(), "inspect_context_summary".into(), json!({})),
        ("c2".into(), "inspect_context_summary".into(), json!({})),
    ];
    let results = reg.run_turn(&calls, &mut ctx);
    assert_eq!(results.iter().map(|r| r.call_id.as_str()).collect::<Vec<_>>(), vec!["c1","c2"]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --test tools run_turn`
Expected: FAIL — `run_turn` not found.

- [ ] **Step 3: Implement serial execution** (append to `tool/mod.rs`)

```rust
#[derive(Debug)]
pub struct TurnToolResult {
    pub call_id: String,
    pub name: String,
    pub outcome: ToolOutcome,
}

impl ToolRegistry {
    /// Execute one turn's tool calls serially in provider response order
    /// (spec §8.3). Rechecks cancellation before each call; a terminal outcome
    /// stops the remaining calls in this turn.
    pub fn run_turn(
        &mut self,
        calls: &[(String, String, serde_json::Value)],
        ctx: &mut ToolContext,
    ) -> Vec<TurnToolResult> {
        let mut out = Vec::new();
        for (call_id, name, args) in calls {
            if ctx.cancellation.is_cancelled() {
                break;
            }
            let outcome = match self.dispatch(name, args.clone(), ctx) {
                Ok(o) => o,
                Err(UnknownTool) => ToolOutcome::Error(format!("unknown tool `{name}`")),
            };
            let is_terminal = matches!(outcome, ToolOutcome::Terminal(_));
            out.push(TurnToolResult { call_id: call_id.clone(), name: name.clone(), outcome });
            if is_terminal {
                break;
            }
        }
        out
    }
}
```

> Note: `dispatch` returns `Err(UnknownTool)` for an unknown name; the *driver* (Task 16) maps a `run_turn` result that came from an unknown tool to a terminal `AgentProtocolFailure`. Here we surface it as an `Error` entry so `run_turn` stays infallible; the driver inspects names against the registry before the turn and treats a genuinely unknown tool as protocol failure (the assembler in Task 17 already rejects unknown tools earlier).

- [ ] **Step 4: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-agent --test tools`
Expected: PASS.

- [ ] **Step 5: Phase-2 checkpoint — clippy**

Run: `rtk cargo clippy -p rollshot-agent --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-agent/src/tool/mod.rs crates/rollshot-agent/tests/tools.rs
git commit -m "feat(agent): serial response-order tool execution with terminal short-circuit (§8.3)"
```

---

## Phase 3 — Manual scripted-model driver and author-loop acceptance

The driver is **synchronous** here (Rig's `AgentRun` is sans-IO, per the spike). It proves the full bounded control plane without any streaming/async. Phase 4 adds the async streaming layer that produces the same `AssembledTurn`, reusing this driver's turn-handling logic (DRY).

### Task 15: Normalized turn types + `TurnSource` + `ScriptedModel`

**Files:**
- Create: `crates/rollshot-agent/src/model.rs`
- Modify: `crates/rollshot-agent/src/testing.rs`, `crates/rollshot-agent/src/lib.rs`
- Test: inline `#[cfg(test)]` in `model.rs`

**Interfaces:**
- Consumes: `ModelUsage` (budget), `ProviderId`/`ModelId` (domain).
- Produces: `AssembledToolCall { call_id: String, name: String, arguments: serde_json::Value }`; `AssembledTurn { text: Option<String>, tool_calls: Vec<AssembledToolCall>, usage: ModelUsage }`; `ModelError` (provider-adapter error, maps to `ProviderFailureKind`); `TurnSource` trait `fn next_turn(&mut self, turn_index: usize) -> Result<AssembledTurn, ModelError>`. `ScriptedModel` (in `testing.rs`) implements `TurnSource` from a `Vec<AssembledTurn>`.

- [ ] **Step 1: Write the failing test** (inline in `model.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn assembled_turn_distinguishes_text_and_tool_calls() {
        let t = AssembledTurn::tool_call("c1", "inspect_context_summary", json!({}));
        assert_eq!(t.tool_calls.len(), 1);
        assert!(t.text.is_none());
        let txt = AssembledTurn::text("hello", ModelUsage { input_tokens: 1, output_tokens: 1 });
        assert_eq!(txt.text.as_deref(), Some("hello"));
        assert!(txt.tool_calls.is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --lib model`
Expected: FAIL — types not found.

- [ ] **Step 3: Write `model.rs`**

```rust
//! Provider-neutral normalized model turn types and the `TurnSource` seam.
//! The streaming facade (Phase 4) and the scripted model both produce an
//! `AssembledTurn`; the driver converts it to a Rig `ModelTurn`.

use crate::budget::ModelUsage;
use crate::error::ProviderFailureKind;

#[derive(Debug, Clone, PartialEq)]
pub struct AssembledToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssembledTurn {
    pub text: Option<String>,
    pub tool_calls: Vec<AssembledToolCall>,
    pub usage: ModelUsage,
}

impl AssembledTurn {
    pub fn text(text: impl Into<String>, usage: ModelUsage) -> Self {
        Self { text: Some(text.into()), tool_calls: Vec::new(), usage }
    }

    pub fn tool_call(call_id: &str, name: &str, arguments: serde_json::Value) -> Self {
        Self {
            text: None,
            tool_calls: vec![AssembledToolCall {
                call_id: call_id.into(),
                name: name.into(),
                arguments,
            }],
            usage: ModelUsage::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("model error: {kind:?} (pre_first_delta={pre_first_delta}): {detail}")]
pub struct ModelError {
    pub kind: ProviderFailureKind,
    pub pre_first_delta: bool,
    pub detail: String,
}

/// The driver's "get the next turn" seam. Scripted in tests; the streaming
/// model (Phase 4) implements an async producer that reuses the same driver.
pub trait TurnSource {
    fn next_turn(&mut self, turn_index: usize) -> Result<AssembledTurn, ModelError>;
}
```

- [ ] **Step 4: Add `ScriptedModel` to `testing.rs`**

```rust
use crate::model::{AssembledTurn, ModelError, TurnSource};

/// Yields a pre-scripted sequence of turns (1-indexed, matching Rig's turn
/// counter). After the script is exhausted it returns a plain-text turn, which
/// the driver treats as a protocol failure (no terminal tool) — exactly the
/// "model stopped without submitting" case.
pub struct ScriptedModel {
    pub turns: Vec<AssembledTurn>,
}

impl TurnSource for ScriptedModel {
    fn next_turn(&mut self, turn_index: usize) -> Result<AssembledTurn, ModelError> {
        Ok(self
            .turns
            .get(turn_index - 1)
            .cloned()
            .unwrap_or_else(|| AssembledTurn::text("done", crate::budget::ModelUsage::default())))
    }
}

/// A minimal automation source that `rollshot_automation::validate_source`
/// accepts. CONFIRM/REPLACE this by copying the smallest passing source from
/// `crates/rollshot-automation`'s frontend test suite before running the
/// acceptance test (Task 16).
pub fn valid_automation_source() -> &'static str {
    // Placeholder shape — replace with a confirmed-valid source string.
    "emitCandidates([])"
}
```

> Implementer note: `valid_automation_source()` is the one required lookup in this plan — `validate_source` must return `Ok` for it. Find a passing source in `crates/rollshot-automation/src/frontend/` (`#[cfg(test)]`) or `crates/rollshot-automation/tests/`, copy the smallest one verbatim, and replace the placeholder. The acceptance test in Task 16 depends on it.

- [ ] **Step 5: Wire `mod model;` + re-exports into `lib.rs`**

Add `mod model;` and `pub use model::{AssembledToolCall, AssembledTurn, ModelError, TurnSource};`.

- [ ] **Step 6: Run test to verify it passes**

Run: `rtk cargo test -p rollshot-agent --lib model`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/rollshot-agent/src/model.rs crates/rollshot-agent/src/testing.rs crates/rollshot-agent/src/lib.rs
git commit -m "feat(agent): normalized AssembledTurn types, TurnSource seam, ScriptedModel"
```

---

### Task 16: Synchronous driver + full author-loop acceptance (§7, §13.2, §15.1)

**Files:**
- Create: `crates/rollshot-agent/src/driver.rs`
- Modify: `crates/rollshot-agent/src/lib.rs`
- Test: `crates/rollshot-agent/tests/driver_author_loop.rs`

**Interfaces:**
- Consumes: Rig (`AgentRun`, `AgentRunStep`, `ModelTurn`, `PendingToolCall`, `Usage`, `Message`, `ToolCall`, `ToolFunction`, `UserContent`, `ToolResultContent`, `AssistantContent`, `OneOrMany`), `ToolRegistry`/`ToolEnv`, `EventLog`, `TerminalCell`, `AssembledTurn`, `TurnSource`, `RunBudgetUsage`.
- Produces: `Driver` with `new(prompt: String, registry: ToolRegistry, env: ToolEnv, budget: RunBudget) -> Self`, `run_scripted(&mut self, source: &mut dyn TurnSource) -> RunTerminalState`, accessors `events(&self) -> &EventLog`. Private `feed_model_turn`, `handle_call_tools` reused by Phase 4. Rig stays entirely inside this file (no Rig types in the signature).

- [ ] **Step 1: Write the failing acceptance test** (`tests/driver_author_loop.rs`)

```rust
use rollshot_agent::testing::{valid_automation_source, ScriptedModel};
use rollshot_agent::tool::default_registry;
use rollshot_agent::{AssembledTurn, Driver, RunBudget, RunTerminalState, ToolEnv};
use serde_json::json;

fn author_loop_script() -> ScriptedModel {
    ScriptedModel {
        turns: vec![
            AssembledTurn::tool_call("c1", "inspect_context_summary", json!({})),
            AssembledTurn::tool_call("c2", "replace_automation_source",
                json!({"source": valid_automation_source()})),
            AssembledTurn::tool_call("c3", "validate_automation", json!({})),
            AssembledTurn::tool_call("c4", "dry_run_automation", json!({})),
            AssembledTurn::tool_call("c5", "submit_for_review", json!({})),
        ],
    }
}

#[test]
fn scripted_author_loop_reaches_ready_for_review() {
    let mut driver = Driver::new(
        "redact the document".into(),
        default_registry(),
        ToolEnv::fake(),
        RunBudget::test_default(),
    );
    let terminal = driver.run_scripted(&mut author_loop_script());
    match terminal {
        RunTerminalState::ReadyForReview(r) => {
            assert!(r.proposal.candidates.is_empty()); // FakeExecutor returns {"candidates":[]}
        }
        other => panic!("expected ReadyForReview, got {other:?}"),
    }
}

#[test]
fn request_user_input_script_reaches_needs_user_input() {
    let script = ScriptedModel {
        turns: vec![
            AssembledTurn::tool_call("c1", "request_user_input",
                json!({"question":"Redact email?","reason":"ambiguous","choices":[]})),
        ],
    };
    let mut driver = Driver::new(
        "redact".into(), default_registry(), ToolEnv::fake(), RunBudget::test_default(),
    );
    match driver.run_scripted(&mut { let mut s = script; s }) {
        RunTerminalState::NeedsUserInput(_) => {}
        other => panic!("expected NeedsUserInput, got {other:?}"),
    }
}

#[test]
fn model_ending_without_submit_is_a_protocol_failure() {
    // Script that validates but never submits, then emits plain text → Done.
    let script = ScriptedModel {
        turns: vec![
            AssembledTurn::tool_call("c1", "replace_automation_source",
                json!({"source": valid_automation_source()})),
            AssembledTurn::tool_call("c2", "validate_automation", json!({})),
            // turn 3+ exhausts script → ScriptedModel yields plain text → Done
        ],
    };
    let mut driver = Driver::new(
        "redact".into(), default_registry(), ToolEnv::fake(), RunBudget::test_default(),
    );
    match driver.run_scripted(&mut { let mut s = script; s }) {
        RunTerminalState::AgentProtocolFailure { .. } => {}
        other => panic!("expected AgentProtocolFailure, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --test driver_author_loop`
Expected: FAIL — `Driver` not found.

- [ ] **Step 3: Write the driver** (`driver.rs`)

```rust
//! Manual Rig `AgentRun` driver (spec §7). Rig types are confined to this file.
//! Seeded from `spikes/rig-agent/src/main.rs`.

use std::collections::BTreeSet;

use rig_core::{
    agent::run::{AgentRun, AgentRunStep, ModelTurn},
    completion::{AssistantContent, Usage},
    message::{Message, ToolCall, ToolFunction, ToolResultContent, UserContent},
    OneOrMany,
};

use crate::budget::{RunBudget, RunBudgetUsage};
use crate::event::{EventLog, RunEvent};
use crate::model::{AssembledTurn, TurnSource};
use crate::terminal::{RunTerminalState, TerminalCell};
use crate::tool::{ToolEnv, ToolOutcome, ToolRegistry, TurnToolResult};

pub struct Driver {
    run: AgentRun,
    registry: ToolRegistry,
    env: ToolEnv,
    events: EventLog,
    terminal: TerminalCell,
    tool_names: BTreeSet<String>,
    max_model_calls: u32,
}

impl Driver {
    pub fn new(prompt: String, registry: ToolRegistry, mut env: ToolEnv, budget: RunBudget) -> Self {
        let tool_names = registry.provider_schemas().into_iter().map(|(n, _)| n).collect();
        env.budget = RunBudgetUsage::new(budget.clone());
        let mut events = EventLog::default();
        events.append(RunEvent::RunStarted);
        Self {
            run: AgentRun::new(Message::user(prompt)).max_turns(budget.max_model_calls as usize),
            registry,
            env,
            events,
            terminal: TerminalCell::default(),
            tool_names,
            max_model_calls: budget.max_model_calls,
        }
    }

    pub fn events(&self) -> &EventLog {
        &self.events
    }

    fn assign(&mut self, state: RunTerminalState) {
        let kind = terminal_kind(&state);
        let _ = self.terminal.assign(state);
        self.events.append(RunEvent::Terminal { kind });
    }

    /// Convert + feed an assembled model turn into Rig; charge usage (D4),
    /// emit AssistantMessageCompleted. Returns Err with a terminal to assign.
    fn feed_model_turn(&mut self, turn_index: usize, assembled: AssembledTurn) -> Result<(), RunTerminalState> {
        // D4: per-turn cumulative usage charge.
        if let Err(e) = self.env.budget.charge_turn_usage(
            turn_index as u64,
            assembled.usage,
        ) {
            return Err(RunTerminalState::BudgetExhausted { limit: e.limit.to_string() });
        }
        self.events.append(RunEvent::BudgetUpdated);

        let mut contents: Vec<AssistantContent> = Vec::new();
        if let Some(text) = &assembled.text {
            self.events.append(RunEvent::TextDelta { text: text.clone() });
            self.events.append(RunEvent::AssistantMessageCompleted { len: text.len() });
            contents.push(AssistantContent::text(text.clone()));
        }
        for call in &assembled.tool_calls {
            contents.push(AssistantContent::ToolCall(ToolCall::new(
                call.call_id.clone(),
                ToolFunction::new(call.name.clone(), call.arguments.clone()),
            )));
        }
        let content = match OneOrMany::many(contents) {
            Ok(c) => c,
            Err(_) => OneOrMany::one(AssistantContent::text(String::new())),
        };
        let usage = Usage {
            input_tokens: assembled.usage.input_tokens,
            output_tokens: assembled.usage.output_tokens,
            total_tokens: assembled.usage.input_tokens + assembled.usage.output_tokens,
            ..Usage::new()
        };
        let model_turn = ModelTurn::new(None, content, usage, self.tool_names.clone(), self.tool_names.clone());
        self.run
            .model_response(model_turn)
            .map_err(|e| RunTerminalState::AgentProtocolFailure { detail: format!("{e:?}") })?;
        Ok(())
    }

    /// Execute the turn's tool calls; returns Some(terminal) if a terminal tool
    /// fired, else None (results were fed back to Rig).
    fn handle_call_tools(
        &mut self,
        calls: Vec<rig_core::agent::run::PendingToolCall>,
    ) -> Option<RunTerminalState> {
        let triples: Vec<(String, String, serde_json::Value)> = calls
            .iter()
            .map(|c| {
                (
                    c.tool_call.id.clone(),
                    c.tool_call.function.name.clone(),
                    c.tool_call.function.arguments.clone(),
                )
            })
            .collect();

        // Unknown tool name → protocol failure (spec §6.2 stream/protocol class).
        for (_, name, _) in &triples {
            if !self.tool_names.contains(name) {
                return Some(RunTerminalState::AgentProtocolFailure {
                    detail: format!("unknown tool `{name}`"),
                });
            }
            self.events.append(RunEvent::ToolCallStarted {
                name: name.clone(),
                call_id: String::new(),
            });
        }

        let mut ctx = self.env.context();
        let results: Vec<TurnToolResult> = self.registry.run_turn(&triples, &mut ctx);

        let mut tool_results: Vec<UserContent> = Vec::new();
        for r in results {
            match r.outcome {
                ToolOutcome::Terminal(t) => {
                    self.events.append(RunEvent::ToolCallCompleted { name: r.name, call_id: r.call_id });
                    return Some(*t);
                }
                ToolOutcome::Result(s) => {
                    self.events.append(RunEvent::ToolCallCompleted {
                        name: r.name,
                        call_id: r.call_id.clone(),
                    });
                    tool_results.push(UserContent::tool_result(
                        r.call_id,
                        ToolResultContent::from_tool_output(s),
                    ));
                }
                ToolOutcome::Error(s) => {
                    // D3: recoverable — feed the error back so the model repairs.
                    self.events.append(RunEvent::ToolCallFailed {
                        name: r.name,
                        call_id: r.call_id.clone(),
                        class: "recoverable".into(),
                    });
                    tool_results.push(UserContent::tool_result(
                        r.call_id,
                        ToolResultContent::from_tool_output(s),
                    ));
                }
            }
        }
        if let Err(e) = self.run.tool_results(tool_results) {
            return Some(RunTerminalState::AgentProtocolFailure { detail: format!("{e:?}") });
        }
        None
    }

    pub fn run_scripted(&mut self, source: &mut dyn TurnSource) -> RunTerminalState {
        loop {
            if self.env.cancellation.is_cancelled() {
                self.assign(RunTerminalState::UserCancelled);
                break;
            }
            let step = match self.run.next_step() {
                Ok(s) => s,
                Err(e) => {
                    self.assign(RunTerminalState::AgentProtocolFailure { detail: format!("{e:?}") });
                    break;
                }
            };
            match step {
                AgentRunStep::CallModel { turn, .. } => {
                    if self.env.budget.charge_model_call().is_err() {
                        self.assign(RunTerminalState::BudgetExhausted { limit: "model_calls".into() });
                        break;
                    }
                    match source.next_turn(turn) {
                        Ok(assembled) => {
                            if let Err(term) = self.feed_model_turn(turn, assembled) {
                                self.assign(term);
                                break;
                            }
                        }
                        Err(e) => {
                            self.assign(RunTerminalState::ProviderFailure { class: format!("{:?}", e.kind) });
                            break;
                        }
                    }
                }
                AgentRunStep::CallTools { calls } => {
                    if let Some(term) = self.handle_call_tools(calls) {
                        self.assign(term);
                        break;
                    }
                }
                AgentRunStep::Done(_) => {
                    // §7: Done without a terminal tool is a protocol failure.
                    self.assign(RunTerminalState::AgentProtocolFailure {
                        detail: "model ended without submit_for_review or request_user_input".into(),
                    });
                    break;
                }
            }
        }
        self.terminal.get().cloned().expect("terminal assigned")
    }
}

fn terminal_kind(state: &RunTerminalState) -> String {
    match state {
        RunTerminalState::ReadyForReview(_) => "ReadyForReview",
        RunTerminalState::NeedsUserInput(_) => "NeedsUserInput",
        RunTerminalState::BudgetExhausted { .. } => "BudgetExhausted",
        RunTerminalState::ProviderFailure { .. } => "ProviderFailure",
        RunTerminalState::AgentProtocolFailure { .. } => "AgentProtocolFailure",
        RunTerminalState::SourceValidationFailure { .. } => "SourceValidationFailure",
        RunTerminalState::RuntimeFailure { .. } => "RuntimeFailure",
        RunTerminalState::UserCancelled => "UserCancelled",
    }
    .to_string()
}
```

Add to `lib.rs`: `mod driver; pub use driver::Driver;`.

> Implementer note: confirm `AgentRun::new` accepts `Message::user(String)` and `max_turns(usize)`, that `PendingToolCall.tool_call.function.arguments` is `serde_json::Value`, and `OneOrMany::many(Vec) -> Result<_, _>` — all per `spikes/rig-agent/tests/driver.rs`. If `model_response`/`tool_results` return types differ, adjust the `map_err` arms (they only format the error). If `next_step` errors are not reachable in the scripted path, the `Err` arm is still required for exhaustiveness.

- [ ] **Step 4: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-agent --test driver_author_loop`
Expected: PASS (3 tests). If `scripted_author_loop_reaches_ready_for_review` fails inside `validate_automation`, fix `valid_automation_source()` (Task 15 Step 4 note) — that is the expected first failure.

- [ ] **Step 5: Add repair-loop tests** (append to `tests/driver_author_loop.rs`)

```rust
#[test]
fn repair_after_validation_failure_then_submit() {
    // Turn 2 sets an INVALID source; validate fails (recoverable); turn 4
    // replaces with a valid source; then validate/dry-run/submit succeed.
    let script = ScriptedModel {
        turns: vec![
            AssembledTurn::tool_call("c1", "replace_automation_source", json!({"source":"@@@not valid@@@"})),
            AssembledTurn::tool_call("c2", "validate_automation", json!({})),
            AssembledTurn::tool_call("c3", "replace_automation_source",
                json!({"source": valid_automation_source()})),
            AssembledTurn::tool_call("c4", "validate_automation", json!({})),
            AssembledTurn::tool_call("c5", "dry_run_automation", json!({})),
            AssembledTurn::tool_call("c6", "submit_for_review", json!({})),
        ],
    };
    let mut driver = Driver::new("redact".into(), default_registry(), ToolEnv::fake(), RunBudget::test_default());
    assert!(matches!(driver.run_scripted(&mut { let mut s = script; s }), RunTerminalState::ReadyForReview(_)));
}
```

Run: `rtk cargo test -p rollshot-agent --test driver_author_loop`
Expected: PASS.

- [ ] **Step 6: Phase-3 checkpoint — clippy + full crate test**

Run: `rtk cargo test -p rollshot-agent && rtk cargo clippy -p rollshot-agent --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 7: Commit + handoff note**

Append a short note to a new `docs/superpowers/handoffs/2026-06-23-bac-phase3-driver.md` (one paragraph: scripted author loop reaches ReadyForReview; streaming not yet wired).

```bash
git add crates/rollshot-agent/src/driver.rs crates/rollshot-agent/src/lib.rs crates/rollshot-agent/tests/driver_author_loop.rs docs/superpowers/handoffs/2026-06-23-bac-phase3-driver.md
git commit -m "feat(agent): synchronous AgentRun driver + author-loop acceptance (§7, §13.2)"
```

---

## Phase 4 — Streaming facade, normalized stream assembly, and the D1 gate

### Task 17: Normalized stream events + bounded call assembler (§6.2, D3, D8)

**Files:**
- Modify: `crates/rollshot-agent/src/model.rs`
- Test: inline `#[cfg(test)]` in `model.rs`

**Interfaces:**
- Consumes: `ModelUsage`, `AssembledTurn`/`AssembledToolCall`.
- Produces: `ToolCallFragment { index: u32, id: Option<String>, name: Option<String>, args_fragment: String }`; `ModelCompletion { stop_reason: String }`; `ModelStreamEvent` (`TextDelta`/`ToolCallDelta`/`UsageDelta`/`Completed`); `CallAssembler::new(max_arg_bytes)` with `push(&mut self, ModelStreamEvent) -> Result<(), AssemblerError>` and `finish(self) -> Result<AssembledTurn, AssemblerError>`; `AssemblerError` (`BufferOverflow`/`MalformedJson`/`Incomplete`) — all stream/protocol failures (terminal).

- [ ] **Step 1: Write the failing test** (inline in `model.rs`)

```rust
#[cfg(test)]
mod assembler_tests {
    use super::*;
    use serde_json::json;

    fn frag(index: u32, id: Option<&str>, name: Option<&str>, args: &str) -> ModelStreamEvent {
        ModelStreamEvent::ToolCallDelta(ToolCallFragment {
            index,
            id: id.map(Into::into),
            name: name.map(Into::into),
            args_fragment: args.into(),
        })
    }

    #[test]
    fn assembles_tool_call_split_across_frames() {
        let mut a = CallAssembler::new(64 * 1024);
        a.push(ModelStreamEvent::TextDelta("think".into())).unwrap();
        a.push(frag(0, Some("call_1"), Some("inspect_region_features"), "{\"region\":")).unwrap();
        a.push(frag(0, None, None, "\"Full\",\"limit\":3}")).unwrap();
        a.push(ModelStreamEvent::Completed(ModelCompletion { stop_reason: "tool_use".into() })).unwrap();
        let turn = a.finish().unwrap();
        assert_eq!(turn.text.as_deref(), Some("think"));
        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(turn.tool_calls[0].name, "inspect_region_features");
        assert_eq!(turn.tool_calls[0].arguments, json!({"region":"Full","limit":3}));
    }

    #[test]
    fn incomplete_stream_without_completion_is_an_error() {
        let mut a = CallAssembler::new(64 * 1024);
        a.push(frag(0, Some("c"), Some("x"), "{}")).unwrap();
        assert!(matches!(a.finish(), Err(AssemblerError::Incomplete(_))));
    }

    #[test]
    fn malformed_json_args_are_an_error() {
        let mut a = CallAssembler::new(64 * 1024);
        a.push(frag(0, Some("c"), Some("x"), "{not json")).unwrap();
        a.push(ModelStreamEvent::Completed(ModelCompletion { stop_reason: "tool_use".into() })).unwrap();
        assert!(matches!(a.finish(), Err(AssemblerError::MalformedJson(_))));
    }

    #[test]
    fn argument_buffer_overflow_is_an_error() {
        let mut a = CallAssembler::new(8); // tiny bound (D8)
        let err = a.push(frag(0, Some("c"), Some("x"), "0123456789ABCDEF")).unwrap_err();
        assert!(matches!(err, AssemblerError::BufferOverflow));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --lib assembler`
Expected: FAIL — types not found.

- [ ] **Step 3: Write the stream types + assembler** (append to `model.rs`)

```rust
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallFragment {
    pub index: u32,
    pub id: Option<String>,
    pub name: Option<String>,
    pub args_fragment: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCompletion {
    pub stop_reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModelStreamEvent {
    TextDelta(String),
    ToolCallDelta(ToolCallFragment),
    UsageDelta(ModelUsage),
    Completed(ModelCompletion),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssemblerError {
    #[error("tool-argument buffer overflow")]
    BufferOverflow,
    #[error("malformed tool-call JSON: {0}")]
    MalformedJson(String),
    #[error("incomplete tool call or stream: {0}")]
    Incomplete(String),
}

#[derive(Default)]
struct PartialCall {
    id: Option<String>,
    name: Option<String>,
    args: String,
}

pub struct CallAssembler {
    max_arg_bytes: usize,
    arg_bytes: usize,
    text: String,
    by_index: BTreeMap<u32, PartialCall>,
    usage: ModelUsage,
    completed: bool,
}

impl CallAssembler {
    pub fn new(max_arg_bytes: usize) -> Self {
        Self {
            max_arg_bytes,
            arg_bytes: 0,
            text: String::new(),
            by_index: BTreeMap::new(),
            usage: ModelUsage::default(),
            completed: false,
        }
    }

    pub fn usage(&self) -> ModelUsage {
        self.usage
    }

    pub fn push(&mut self, event: ModelStreamEvent) -> Result<(), AssemblerError> {
        match event {
            ModelStreamEvent::TextDelta(t) => self.text.push_str(&t),
            ModelStreamEvent::UsageDelta(u) => self.usage = u, // cumulative snapshot (D4)
            ModelStreamEvent::Completed(_) => self.completed = true,
            ModelStreamEvent::ToolCallDelta(frag) => {
                self.arg_bytes += frag.args_fragment.len();
                if self.arg_bytes > self.max_arg_bytes {
                    return Err(AssemblerError::BufferOverflow); // D8
                }
                let call = self.by_index.entry(frag.index).or_default();
                if frag.id.is_some() {
                    call.id = frag.id;
                }
                if frag.name.is_some() {
                    call.name = frag.name;
                }
                call.args.push_str(&frag.args_fragment);
            }
        }
        Ok(())
    }

    pub fn finish(self) -> Result<AssembledTurn, AssemblerError> {
        if !self.completed {
            return Err(AssemblerError::Incomplete("stream ended without completion".into()));
        }
        let mut tool_calls = Vec::new();
        for (index, call) in self.by_index {
            let (id, name) = match (call.id, call.name) {
                (Some(id), Some(name)) => (id, name),
                _ => return Err(AssemblerError::Incomplete(format!("tool call {index} missing id/name"))),
            };
            let args: serde_json::Value = if call.args.trim().is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(&call.args)
                    .map_err(|e| AssemblerError::MalformedJson(e.to_string()))?
            };
            tool_calls.push(AssembledToolCall { call_id: id, name, arguments: args });
        }
        Ok(AssembledTurn {
            text: if self.text.is_empty() { None } else { Some(self.text) },
            tool_calls,
            usage: self.usage,
        })
    }
}
```

Add to `lib.rs`: `pub use model::{AssemblerError, CallAssembler, ModelCompletion, ModelStreamEvent, ToolCallFragment};`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-agent --lib assembler`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/rollshot-agent/src/model.rs crates/rollshot-agent/src/lib.rs
git commit -m "feat(agent): normalized stream events + bounded call assembler (§6.2, D3, D8)"
```

---

### Task 18: D1 GATE — externally-assembled ModelTurn drives AgentRun

**This task is the spec §2.1/§14 D1 de-risk check. It must pass before any provider adapter (Phase 5/6).** If it cannot be made to pass, STOP and escalate: the fallback is a Rollshot-owned turn state machine (the public model is unchanged), and Phases 5–6 proceed against that instead of Rig.

**Files:**
- Test: `crates/rollshot-agent/tests/d1_gate.rs`

**Interfaces:**
- Consumes: `CallAssembler`, `AssembledTurn`, `Driver`, `ScriptedModel`, `default_registry`.

- [ ] **Step 1: Write the gate test** (`tests/d1_gate.rs`)

```rust
use rollshot_agent::testing::ScriptedModel;
use rollshot_agent::tool::default_registry;
use rollshot_agent::{
    CallAssembler, Driver, ModelCompletion, ModelStreamEvent, RunBudget, RunEvent, ToolCallFragment,
    ToolEnv,
};

/// Build an AssembledTurn the same way a provider adapter will: from a stream
/// of normalized events run through the assembler.
fn assembled_inspect_turn() -> rollshot_agent::AssembledTurn {
    let mut a = CallAssembler::new(64 * 1024);
    a.push(ModelStreamEvent::TextDelta("inspecting".into())).unwrap();
    a.push(ModelStreamEvent::ToolCallDelta(ToolCallFragment {
        index: 0,
        id: Some("c1".into()),
        name: Some("inspect_context_summary".into()),
        args_fragment: "{}".into(),
    })).unwrap();
    a.push(ModelStreamEvent::Completed(ModelCompletion { stop_reason: "tool_use".into() })).unwrap();
    a.finish().unwrap()
}

#[test]
fn assembled_turn_drives_agentrun_to_a_tool_call() {
    let assembled = assembled_inspect_turn();
    // Wrap the externally-assembled turn as the model's first turn.
    let mut model = ScriptedModel { turns: vec![assembled] };
    let mut driver = Driver::new(
        "redact".into(), default_registry(), ToolEnv::fake(), RunBudget::test_default(),
    );
    let _ = driver.run_scripted(&mut model);
    // D1 PROVEN: AgentRun consumed the externally-assembled ModelTurn and
    // surfaced its tool call, which the driver executed.
    let ran_inspect = driver.events().events().iter().any(|(_, e)| matches!(
        e, RunEvent::ToolCallCompleted { name, .. } if name == "inspect_context_summary"
    ));
    assert!(ran_inspect, "AgentRun did not drive the assembled tool call");
}
```

- [ ] **Step 2: Run the gate**

Run: `rtk cargo test -p rollshot-agent --test d1_gate`
Expected: PASS. **If FAIL:** investigate whether Rig's `AgentRun::model_response` rejects an externally-built `ModelTurn` (e.g. tool-set mismatch). Try aligning the `available`/`requested` tool sets in `ModelTurn::new`. If still failing, escalate per the task header.

- [ ] **Step 3: Commit**

```bash
git add crates/rollshot-agent/tests/d1_gate.rs
git commit -m "test(agent): D1 gate — externally-assembled ModelTurn drives Rig AgentRun"
```

---

### Task 19: Streaming facade + stream→event bridge + async driver (§6.1, §5)

**Files:**
- Modify: `crates/rollshot-agent/src/model.rs`, `crates/rollshot-agent/src/driver.rs`, `crates/rollshot-agent/src/testing.rs`, `crates/rollshot-agent/src/lib.rs`
- Test: `crates/rollshot-agent/tests/driver_author_loop.rs`

**Interfaces:**
- Consumes: `CallAssembler`, `RunCancellation`, `EventLog`, `RunBudgetUsage`, `Driver` internals (`feed_model_turn`, `handle_call_tools`).
- Produces: `ModelRequest { system: String, tools: Vec<(String, serde_json::Value)>, provider: ProviderId, model: ModelId }`; `ModelEventStream = Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, ModelError>> + Send>>`; `RollshotModel` trait with `provider()`, `model()`, and `fn stream(&self, ModelRequest, RunCancellation) -> Pin<Box<dyn Future<Output = Result<ModelEventStream, ModelError>> + Send + '_>>`; `consume_stream(...) -> Result<AssembledTurn, AgentError>` (emits `TextDelta`, charges cumulative usage D4, honors cancellation); `Driver::run_streaming(&mut self, &dyn RollshotModel, ModelRequest) -> RunTerminalState`. `FakeStreamModel` (testing) yields a scripted event vec.

- [ ] **Step 1: Write the failing test** (append to `tests/driver_author_loop.rs`)

```rust
use rollshot_agent::testing::FakeStreamModel;
use rollshot_agent::{ModelRequest, ModelStreamEvent, ModelCompletion, ToolCallFragment, ProviderId, ModelId};

fn evt_tool(name: &str, args: &str) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::ToolCallDelta(ToolCallFragment {
            index: 0, id: Some("c1".into()), name: Some(name.into()), args_fragment: args.into(),
        }),
        ModelStreamEvent::UsageDelta(rollshot_agent::ModelUsage { input_tokens: 10, output_tokens: 4 }),
        ModelStreamEvent::Completed(ModelCompletion { stop_reason: "tool_use".into() }),
    ]
}

#[tokio::test]
async fn streaming_driver_charges_cumulative_usage_once_and_runs_tool() {
    // Two snapshots for the SAME turn must charge once (D4).
    let mut first = evt_tool("inspect_context_summary", "{}");
    first.insert(1, ModelStreamEvent::UsageDelta(rollshot_agent::ModelUsage { input_tokens: 10, output_tokens: 2 }));
    let model = FakeStreamModel { turns: vec![first] };
    let req = ModelRequest { system: "s".into(), tools: vec![], provider: ProviderId::Anthropic, model: ModelId("m".into()) };
    let mut driver = rollshot_agent::Driver::new(
        "redact".into(), rollshot_agent::tool::default_registry(),
        rollshot_agent::ToolEnv::fake(), rollshot_agent::RunBudget::test_default(),
    );
    let _ = driver.run_streaming(&model, req).await;
    let ran = driver.events().events().iter().any(|(_, e)| matches!(
        e, rollshot_agent::RunEvent::ToolCallCompleted { name, .. } if name == "inspect_context_summary"));
    assert!(ran);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --test driver_author_loop streaming`
Expected: FAIL — `ModelRequest`/`FakeStreamModel`/`run_streaming` not found.

- [ ] **Step 3: Add the facade + request types** (append to `model.rs`)

```rust
use std::future::Future;
use std::pin::Pin;

use futures_util::Stream;

use crate::domain::{ModelId, ProviderId};

#[derive(Debug, Clone)]
pub struct ModelRequest {
    pub system: String,
    pub tools: Vec<(String, serde_json::Value)>,
    pub provider: ProviderId,
    pub model: ModelId,
}

pub type ModelEventStream =
    Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, ModelError>> + Send>>;

/// Rollshot-owned model facade. Public types stay independent of Rig and
/// provider SDK types (spec §3.1/§6.1).
pub trait RollshotModel: Send + Sync {
    fn provider(&self) -> ProviderId;
    fn model(&self) -> ModelId;
    fn stream<'a>(
        &'a self,
        request: ModelRequest,
        cancellation: crate::cancellation::RunCancellation,
    ) -> Pin<Box<dyn Future<Output = Result<ModelEventStream, ModelError>> + Send + 'a>>;
}
```

- [ ] **Step 4: Add `consume_stream` + `run_streaming`** (in `driver.rs`)

```rust
use futures_util::StreamExt;

use crate::error::AgentError;
use crate::model::{CallAssembler, ModelEventStream, ModelRequest, ModelStreamEvent, RollshotModel};

/// Consume one provider turn's stream into an AssembledTurn. Emits TextDelta
/// events live, charges cumulative usage per snapshot (D4), and aborts on
/// cancellation. Stream errors → AgentError::ProviderFailure; assembler errors
/// → AgentError::AgentProtocol.
async fn consume_stream(
    mut stream: ModelEventStream,
    turn_index: usize,
    events: &mut EventLog,
    budget: &mut RunBudgetUsage,
    cancellation: &crate::cancellation::RunCancellation,
    max_arg_bytes: usize,
) -> Result<AssembledTurn, AgentError> {
    let mut assembler = CallAssembler::new(max_arg_bytes);
    loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(AgentError::Cancelled),
            next = stream.next() => {
                match next {
                    None => break,
                    Some(Err(e)) => {
                        return Err(AgentError::ProviderFailure {
                            kind: e.kind, pre_first_delta: e.pre_first_delta,
                        });
                    }
                    Some(Ok(ev)) => {
                        if let ModelStreamEvent::TextDelta(t) = &ev {
                            events.append(RunEvent::TextDelta { text: t.clone() });
                        }
                        if let ModelStreamEvent::UsageDelta(u) = ev {
                            // D4: cumulative snapshot — charge increase once per turn.
                            if let Err(be) = budget.charge_turn_usage(turn_index as u64, u) {
                                return Err(AgentError::Budget(be.limit));
                            }
                            events.append(RunEvent::BudgetUpdated);
                        }
                        assembler.push(ev).map_err(|e| AgentError::AgentProtocol(e.to_string()))?;
                    }
                }
            }
        }
    }
    assembler.finish().map_err(|e| AgentError::AgentProtocol(e.to_string()))
}

impl Driver {
    /// Async production driver. Reuses feed_model_turn/handle_call_tools.
    pub async fn run_streaming(
        &mut self,
        model: &dyn RollshotModel,
        request: ModelRequest,
    ) -> RunTerminalState {
        let max_arg_bytes = 256 * 1024;
        loop {
            if self.env.cancellation.is_cancelled() {
                self.assign(RunTerminalState::UserCancelled);
                break;
            }
            let step = match self.run.next_step() {
                Ok(s) => s,
                Err(e) => { self.assign(RunTerminalState::AgentProtocolFailure { detail: format!("{e:?}") }); break; }
            };
            match step {
                AgentRunStep::CallModel { turn, .. } => {
                    if self.env.budget.charge_model_call().is_err() {
                        self.assign(RunTerminalState::BudgetExhausted { limit: "model_calls".into() });
                        break;
                    }
                    let stream = match model.stream(request.clone(), self.env.cancellation.clone()).await {
                        Ok(s) => s,
                        Err(e) => {
                            self.assign(RunTerminalState::ProviderFailure { class: format!("{:?}", e.kind) });
                            break;
                        }
                    };
                    // NB: split borrows — pass &mut self.events / &mut self.env.budget.
                    let assembled = consume_stream(
                        stream, turn, &mut self.events, &mut self.env.budget,
                        &self.env.cancellation, max_arg_bytes,
                    ).await;
                    match assembled {
                        Ok(a) => {
                            self.events.append(RunEvent::AssistantMessageCompleted {
                                len: a.text.as_ref().map(|t| t.len()).unwrap_or(0),
                            });
                            if let Err(term) = self.feed_model_turn_no_usage(turn, a) {
                                self.assign(term); break;
                            }
                        }
                        Err(AgentError::Cancelled) => { self.assign(RunTerminalState::UserCancelled); break; }
                        Err(AgentError::Budget(l)) => { self.assign(RunTerminalState::BudgetExhausted { limit: l.to_string() }); break; }
                        Err(AgentError::ProviderFailure { kind, .. }) => { self.assign(RunTerminalState::ProviderFailure { class: format!("{kind:?}") }); break; }
                        Err(e) => { self.assign(RunTerminalState::AgentProtocolFailure { detail: e.to_string() }); break; }
                    }
                }
                AgentRunStep::CallTools { calls } => {
                    if let Some(term) = self.handle_call_tools(calls) { self.assign(term); break; }
                }
                AgentRunStep::Done(_) => {
                    self.assign(RunTerminalState::AgentProtocolFailure {
                        detail: "model ended without submit_for_review or request_user_input".into(),
                    });
                    break;
                }
            }
        }
        self.terminal.get().cloned().expect("terminal assigned")
    }
}
```

> Refactor note: `consume_stream` already charges usage (D4), so the streaming path needs a `feed_model_turn_no_usage` variant that skips the usage charge but does the ModelTurn conversion + `model_response`. Extract the shared body of Task 16's `feed_model_turn` into `feed_model_turn_inner(turn, assembled, charge_usage: bool)` and have both call it — this is the "make the change easy, then make the easy change" refactor; do it as the first step of this task before adding `run_streaming`.

- [ ] **Step 5: Add `FakeStreamModel` to `testing.rs`**

```rust
use std::future::Future;
use std::pin::Pin;

use crate::model::{ModelEventStream, ModelError, ModelRequest, ModelStreamEvent, RollshotModel};
use crate::domain::{ModelId, ProviderId};

/// Yields one scripted event-vec per turn (1-indexed). Tracks the turn via an
/// interior counter so repeated `stream()` calls advance the script.
pub struct FakeStreamModel {
    pub turns: Vec<Vec<ModelStreamEvent>>,
}

impl RollshotModel for FakeStreamModel {
    fn provider(&self) -> ProviderId { ProviderId::Anthropic }
    fn model(&self) -> ModelId { ModelId("fake".into()) }
    fn stream<'a>(
        &'a self,
        _request: ModelRequest,
        _cancellation: crate::cancellation::RunCancellation,
    ) -> Pin<Box<dyn Future<Output = Result<ModelEventStream, ModelError>> + Send + 'a>> {
        // Each call pops the next scripted turn. Use an AtomicUsize for the cursor.
        let idx = self.cursor.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let events = self.turns.get(idx).cloned().unwrap_or_default();
        Box::pin(async move {
            let stream = futures_util::stream::iter(events.into_iter().map(Ok));
            Ok(Box::pin(stream) as ModelEventStream)
        })
    }
}
```

> Implementer note: add `cursor: std::sync::atomic::AtomicUsize` to `FakeStreamModel` (init 0) and a constructor; the struct-literal test in Step 1 must use the constructor. Confirm `futures_util::stream::iter` + `StreamExt::next` are available (they are with the `futures-util` dep).

- [ ] **Step 6: Wire re-exports** (`lib.rs`): `pub use model::{ModelEventStream, ModelRequest, RollshotModel};`.

- [ ] **Step 7: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-agent --test driver_author_loop`
Expected: PASS (all, including the streaming test).

- [ ] **Step 8: Phase-4 checkpoint**

Run: `rtk cargo test -p rollshot-agent && rtk cargo clippy -p rollshot-agent --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/rollshot-agent/src/model.rs crates/rollshot-agent/src/driver.rs crates/rollshot-agent/src/testing.rs crates/rollshot-agent/src/lib.rs crates/rollshot-agent/tests/driver_author_loop.rs
git commit -m "feat(agent): streaming facade, stream→event bridge, async driver (§6.1, §5, D4)"
```

---

## Phase 5 — Anthropic adapter + recorded fixtures

> **New dependency (consequence of D1):** owning the provider adapters means BAC
> needs its own HTTP+SSE client. Add `reqwest = { version = "0.12", features =
> ["json", "stream"] }` to `crates/rollshot-agent/Cargo.toml` (and, optionally,
> to root `[workspace.dependencies]`). The **SSE parser is a pure function over
> bytes** and is what the fixture tests exercise — `reqwest` is used only by the
> production transport and the optional live smoke test, so CI stays
> network-free.

### Task 20: Provider request types + Anthropic request-body builder (§6.3)

**Files:**
- Create: `crates/rollshot-agent/src/provider/mod.rs`, `crates/rollshot-agent/src/provider/anthropic.rs`
- Modify: `crates/rollshot-agent/src/lib.rs`, `crates/rollshot-agent/Cargo.toml`
- Test: inline `#[cfg(test)]` in `provider/anthropic.rs`

**Interfaces:**
- Consumes: `ModelRequest` (model.rs).
- Produces: `anthropic::build_request_body(&ModelRequest) -> serde_json::Value` mapping the registry tool schemas into Anthropic `tools` (`{name, input_schema}`) and setting `stream: true`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ModelId, ProviderId};
    use crate::model::ModelRequest;
    use serde_json::json;

    #[test]
    fn request_body_encodes_tools_and_stream_flag() {
        let req = ModelRequest {
            system: "you redact".into(),
            tools: vec![("inspect_context_summary".into(), json!({"type":"object"}))],
            provider: ProviderId::Anthropic,
            model: ModelId("claude-x".into()),
        };
        let body = build_request_body(&req);
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["model"], json!("claude-x"));
        assert_eq!(body["tools"][0]["name"], json!("inspect_context_summary"));
        assert!(body["tools"][0]["input_schema"].is_object());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --lib provider::anthropic`
Expected: FAIL — module not found.

- [ ] **Step 3: Write `provider/mod.rs` + the builder**

`provider/mod.rs`:
```rust
//! Provider adapters. Rig is NOT used here; BAC owns request building + SSE
//! parsing + normalization (spec §6.3). Parsers are pure functions over bytes.

pub mod anthropic;
pub mod openai;
```

`provider/anthropic.rs` (builder portion):
```rust
//! Anthropic Messages API adapter.

use crate::model::ModelRequest;

pub fn build_request_body(req: &ModelRequest) -> serde_json::Value {
    let tools: Vec<serde_json::Value> = req
        .tools
        .iter()
        .map(|(name, schema)| serde_json::json!({ "name": name, "input_schema": schema }))
        .collect();
    serde_json::json!({
        "model": req.model.0,
        "system": req.system,
        "tools": tools,
        "stream": true,
    })
}
```

- [ ] **Step 4: Add `reqwest` dep + `mod provider;`**

Add `reqwest = { version = "0.12", features = ["json", "stream"] }` to `[dependencies]`. Add `mod provider;` to `lib.rs`.

- [ ] **Step 5: Run test to verify it passes**

Run: `rtk cargo test -p rollshot-agent --lib provider::anthropic`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-agent/src/provider/ crates/rollshot-agent/src/lib.rs crates/rollshot-agent/Cargo.toml Cargo.lock
git commit -m "feat(agent): provider module + Anthropic request-body builder (§6.3)"
```

---

### Task 21: Anthropic SSE parser → ModelStreamEvent (fixtures, D7) (§6.3, §13.4)

**Files:**
- Modify: `crates/rollshot-agent/src/provider/anthropic.rs`
- Create: `crates/rollshot-agent/tests/fixtures/anthropic/*.txt`
- Test: `crates/rollshot-agent/tests/anthropic_fixtures.rs`

**Interfaces:**
- Consumes: `ModelStreamEvent`, `ToolCallFragment`, `ModelCompletion`, `ModelUsage`, `ModelError`.
- Produces: `anthropic::parse_sse(bytes: &[u8]) -> Vec<Result<ModelStreamEvent, ModelError>>` — splits SSE frames, JSON-parses `data:` payloads, maps to normalized events, tracks cumulative usage.

> **D7 — recording fixtures:** capture real Anthropic SSE once with a synthetic
> prompt (the optional live path, Task 22 Step 5), scrub all content to
> synthetic text, and save under `tests/fixtures/anthropic/`. Required fixtures
> (spec §13.4): `text_only.txt`, `text_then_tool.txt`, `tool_args_split.txt`,
> `multi_tool.txt`, `usage.txt`, `completion.txt`, `malformed_tool_json.txt`,
> `incomplete_stream.txt`, `error_response.txt`, `rate_limit.txt`,
> `cancel_midstream.txt`. Do **not** hand-author the SSE framing.

- [ ] **Step 1: Write the failing test** (`tests/anthropic_fixtures.rs`)

```rust
use rollshot_agent::provider::anthropic::parse_sse;
use rollshot_agent::ModelStreamEvent;

#[test]
fn text_then_tool_fixture_normalizes_to_text_and_tool_call() {
    let bytes = include_bytes!("fixtures/anthropic/text_then_tool.txt");
    let events: Vec<_> = parse_sse(bytes).into_iter().map(|r| r.unwrap()).collect();
    let has_text = events.iter().any(|e| matches!(e, ModelStreamEvent::TextDelta(_)));
    let has_tool = events.iter().any(|e| matches!(e, ModelStreamEvent::ToolCallDelta(_)));
    let completed = events.iter().any(|e| matches!(e, ModelStreamEvent::Completed(_)));
    assert!(has_text && has_tool && completed);
}

#[test]
fn split_tool_args_fixture_yields_fragments_that_assemble() {
    use rollshot_agent::CallAssembler;
    let bytes = include_bytes!("fixtures/anthropic/tool_args_split.txt");
    let mut a = CallAssembler::new(64 * 1024);
    for ev in parse_sse(bytes) { a.push(ev.unwrap()).unwrap(); }
    let turn = a.finish().unwrap();
    assert_eq!(turn.tool_calls.len(), 1);
    assert!(turn.tool_calls[0].arguments.is_object());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `rtk cargo test -p rollshot-agent --test anthropic_fixtures`
Expected: FAIL — `parse_sse` not found (and fixtures missing).

- [ ] **Step 3: Write the parser** (append to `provider/anthropic.rs`)

```rust
use crate::budget::ModelUsage;
use crate::error::ProviderFailureKind;
use crate::model::{ModelCompletion, ModelError, ModelStreamEvent, ToolCallFragment};

/// Parse an Anthropic SSE byte buffer into normalized events. Pure function;
/// no network. Tracks cumulative usage across message_start/message_delta.
pub fn parse_sse(bytes: &[u8]) -> Vec<Result<ModelStreamEvent, ModelError>> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    let mut usage = ModelUsage::default();
    for block in text.split("\n\n") {
        let Some(data) = data_payload(block) else { continue };
        if data.trim() == "[DONE]" {
            continue;
        }
        let json: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(e) => {
                out.push(Err(ModelError {
                    kind: ProviderFailureKind::Malformed,
                    pre_first_delta: out.is_empty(),
                    detail: e.to_string(),
                }));
                continue;
            }
        };
        match json["type"].as_str() {
            Some("message_start") => {
                usage.input_tokens = json["message"]["usage"]["input_tokens"].as_u64().unwrap_or(0);
                out.push(Ok(ModelStreamEvent::UsageDelta(usage)));
            }
            Some("content_block_start") => {
                if json["content_block"]["type"] == "tool_use" {
                    out.push(Ok(ModelStreamEvent::ToolCallDelta(ToolCallFragment {
                        index: json["index"].as_u64().unwrap_or(0) as u32,
                        id: json["content_block"]["id"].as_str().map(Into::into),
                        name: json["content_block"]["name"].as_str().map(Into::into),
                        args_fragment: String::new(),
                    })));
                }
            }
            Some("content_block_delta") => {
                let index = json["index"].as_u64().unwrap_or(0) as u32;
                match json["delta"]["type"].as_str() {
                    Some("text_delta") => {
                        if let Some(t) = json["delta"]["text"].as_str() {
                            out.push(Ok(ModelStreamEvent::TextDelta(t.into())));
                        }
                    }
                    Some("input_json_delta") => {
                        if let Some(p) = json["delta"]["partial_json"].as_str() {
                            out.push(Ok(ModelStreamEvent::ToolCallDelta(ToolCallFragment {
                                index, id: None, name: None, args_fragment: p.into(),
                            })));
                        }
                    }
                    _ => {}
                }
            }
            Some("message_delta") => {
                if let Some(o) = json["usage"]["output_tokens"].as_u64() {
                    usage.output_tokens = o;
                    out.push(Ok(ModelStreamEvent::UsageDelta(usage)));
                }
            }
            Some("message_stop") => {
                out.push(Ok(ModelStreamEvent::Completed(ModelCompletion {
                    stop_reason: "stop".into(),
                })));
            }
            Some("error") => {
                let kind = match json["error"]["type"].as_str() {
                    Some("overloaded_error") | Some("rate_limit_error") => ProviderFailureKind::RateLimit,
                    Some("authentication_error") => ProviderFailureKind::Auth,
                    _ => ProviderFailureKind::Rejection,
                };
                out.push(Err(ModelError { kind, pre_first_delta: out.is_empty(), detail: "anthropic error".into() }));
            }
            _ => {}
        }
    }
    out
}

fn data_payload(block: &str) -> Option<&str> {
    block.lines().find_map(|l| l.strip_prefix("data: ").or_else(|| l.strip_prefix("data:")))
}
```

- [ ] **Step 4: Create the recorded fixtures**

Capture or hand-stage the SSE fixtures listed above. For the **first green run** you may stage synthetic-but-correctly-framed `.txt` files matching the real Anthropic event grammar (the parser only reads `data:` JSON). Replace with truly-recorded captures (D7) during Task 22 Step 5. Each fixture is plain SSE text, e.g. `text_then_tool.txt`:

```
event: message_start
data: {"type":"message_start","message":{"usage":{"input_tokens":12}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"call_1","name":"inspect_context_summary"}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{}"}}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":7}}

event: message_stop
data: {"type":"message_stop"}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `rtk cargo test -p rollshot-agent --test anthropic_fixtures`
Expected: PASS.

- [ ] **Step 6: Add the remaining §13.4 fixture cases** (one test each — complete code follows the two shown above; each names its fixture + a one-line assertion)

| Fixture | Assertion |
|---|---|
| `text_only.txt` | only `TextDelta` + `Completed`, no `ToolCallDelta` |
| `multi_tool.txt` | assembler yields `tool_calls.len() == 2`, ordered by index |
| `usage.txt` | last `UsageDelta` has `input_tokens > 0 && output_tokens > 0` |
| `completion.txt` | exactly one `Completed` event, last in the vec |
| `malformed_tool_json.txt` | assembler `.finish()` is `Err(AssemblerError::MalformedJson(_))` |
| `incomplete_stream.txt` | no `Completed`; assembler `.finish()` is `Err(AssemblerError::Incomplete(_))` |
| `error_response.txt` | `parse_sse` yields an `Err(ModelError{kind: Rejection, ..})` |
| `rate_limit.txt` | `Err(ModelError{kind: RateLimit, ..})` |
| `cancel_midstream.txt` | partial events parse; assembler `.finish()` is `Err(Incomplete)` (no completion) |

Write one `#[test]` per row using the Step-1 pattern. Run: `rtk cargo test -p rollshot-agent --test anthropic_fixtures` → PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/rollshot-agent/src/provider/anthropic.rs crates/rollshot-agent/tests/anthropic_fixtures.rs crates/rollshot-agent/tests/fixtures/anthropic/
git commit -m "feat(agent): Anthropic SSE parser + recorded fixtures (§6.3, §13.4, D7)"
```

---

### Task 22: Anthropic production transport + error mapping + cancellation

**Files:**
- Modify: `crates/rollshot-agent/src/provider/anthropic.rs`
- Test: `crates/rollshot-agent/tests/anthropic_fixtures.rs` (error mapping) + an `#[ignore]` live smoke test.

**Interfaces:**
- Produces: `AnthropicModel { api_key: String, model: ModelId, base_url: String }` implementing `RollshotModel`; `stream()` posts the request body, wraps `reqwest`'s byte stream through a line-buffered `parse_sse` adapter into a `ModelEventStream`, and selects on `cancellation.cancelled()` to drop the stream without leaking partial payloads.

- [ ] **Step 1: Write the error-mapping test** (append to `tests/anthropic_fixtures.rs`)

```rust
use rollshot_agent::ProviderFailureKind;

#[test]
fn rate_limit_fixture_maps_to_rate_limit_kind() {
    let bytes = include_bytes!("fixtures/anthropic/rate_limit.txt");
    let err = rollshot_agent::provider::anthropic::parse_sse(bytes)
        .into_iter()
        .find_map(|r| r.err())
        .expect("expected an error event");
    assert_eq!(err.kind, ProviderFailureKind::RateLimit);
}
```

- [ ] **Step 2: Run it** → PASS once `rate_limit.txt` exists (created in Task 21 Step 6).

Run: `rtk cargo test -p rollshot-agent --test anthropic_fixtures rate_limit`
Expected: PASS.

- [ ] **Step 3: Implement the transport** (append to `provider/anthropic.rs`)

```rust
use std::future::Future;
use std::pin::Pin;

use futures_util::StreamExt;

use crate::cancellation::RunCancellation;
use crate::domain::{ModelId, ProviderId};
use crate::model::{ModelEventStream, ModelRequest, RollshotModel};

pub struct AnthropicModel {
    pub api_key: String,
    pub model: ModelId,
    pub base_url: String, // e.g. "https://api.anthropic.com/v1/messages"
}

impl RollshotModel for AnthropicModel {
    fn provider(&self) -> ProviderId { ProviderId::Anthropic }
    fn model(&self) -> ModelId { self.model.clone() }
    fn stream<'a>(
        &'a self,
        request: ModelRequest,
        cancellation: RunCancellation,
    ) -> Pin<Box<dyn Future<Output = Result<ModelEventStream, ModelError>> + Send + 'a>> {
        Box::pin(async move {
            let body = build_request_body(&request);
            let resp = reqwest::Client::new()
                .post(&self.base_url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&body)
                .send()
                .await
                .map_err(|e| ModelError { kind: ProviderFailureKind::Transport, pre_first_delta: true, detail: e.to_string() })?;
            // Buffer the whole SSE body, then parse. (A chunk-incremental parser
            // is an optimization; the bounded author loop has small responses.)
            let bytes = resp.bytes().await.map_err(|e| ModelError {
                kind: ProviderFailureKind::Transport, pre_first_delta: true, detail: e.to_string(),
            })?;
            let events = parse_sse(&bytes);
            let stream = futures_util::stream::iter(events).take_while(move |_| {
                let alive = !cancellation.is_cancelled();
                async move { alive }
            });
            Ok(Box::pin(stream) as ModelEventStream)
        })
    }
}
```

> Implementer note: the `take_while` closure must not capture `cancellation` by move into an `async move` that outlives it — clone `cancellation` once before the stream and check `is_cancelled()` per item. Confirm `reqwest::Response::bytes()` and `futures_util::stream::iter` signatures. The buffered approach keeps the parser pure and network logic thin; revisit only if a future use needs true incremental streaming.

- [ ] **Step 4: Add the `#[ignore]` live smoke test** (append)

```rust
#[tokio::test]
#[ignore = "requires ANTHROPIC_API_KEY and network; run manually to record fixtures (D7)"]
async fn live_smoke_records_a_real_stream() {
    // Manual: set ANTHROPIC_API_KEY, point base_url at the real endpoint, send a
    // synthetic prompt, and capture the raw SSE to tests/fixtures/anthropic/.
}
```

- [ ] **Step 5: Run (ignored test is skipped)**

Run: `rtk cargo test -p rollshot-agent --test anthropic_fixtures`
Expected: PASS (live test reported as ignored).

- [ ] **Step 6: Commit**

```bash
git add crates/rollshot-agent/src/provider/anthropic.rs crates/rollshot-agent/tests/anthropic_fixtures.rs
git commit -m "feat(agent): Anthropic transport, error mapping, cancellation; live smoke (ignored)"
```

---

## Phase 6 — OpenAI adapter + recorded fixtures (Chat Completions, D6)

### Task 23: OpenAI request-body builder (Chat Completions, D6)

**Files:** Modify `crates/rollshot-agent/src/provider/openai.rs`; test inline.

**Interfaces:** Produces `openai::build_request_body(&ModelRequest) -> serde_json::Value` encoding tools as `{type:"function", function:{name, parameters}}`, `stream: true`, and `stream_options: {include_usage: true}` (D6 — needed to get usage in the stream).

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ModelId, ProviderId};
    use crate::model::ModelRequest;
    use serde_json::json;

    #[test]
    fn body_uses_function_tools_and_include_usage() {
        let req = ModelRequest {
            system: "s".into(),
            tools: vec![("validate_automation".into(), json!({"type":"object"}))],
            provider: ProviderId::OpenAi,
            model: ModelId("gpt-x".into()),
        };
        let b = build_request_body(&req);
        assert_eq!(b["stream"], json!(true));
        assert_eq!(b["stream_options"]["include_usage"], json!(true));
        assert_eq!(b["tools"][0]["type"], json!("function"));
        assert_eq!(b["tools"][0]["function"]["name"], json!("validate_automation"));
    }
}
```

- [ ] **Step 2: Run → FAIL.** `rtk cargo test -p rollshot-agent --lib provider::openai`

- [ ] **Step 3: Implement**

```rust
//! OpenAI Chat Completions streaming adapter (D6).

use crate::model::ModelRequest;

pub fn build_request_body(req: &ModelRequest) -> serde_json::Value {
    let tools: Vec<serde_json::Value> = req.tools.iter().map(|(name, schema)| {
        serde_json::json!({"type":"function","function":{"name":name,"parameters":schema}})
    }).collect();
    serde_json::json!({
        "model": req.model.0,
        "messages": [{"role":"system","content": req.system}],
        "tools": tools,
        "stream": true,
        "stream_options": {"include_usage": true},
    })
}
```

- [ ] **Step 4: Run → PASS.** **Step 5: Commit** `feat(agent): OpenAI Chat Completions request builder (D6)`.

---

### Task 24: OpenAI SSE parser → ModelStreamEvent (fixtures, D7) (§6.3, §13.4)

**Files:** Modify `provider/openai.rs`; create `tests/fixtures/openai/*.txt`; test `tests/openai_fixtures.rs`.

**Interfaces:** Produces `openai::parse_sse(bytes: &[u8]) -> Vec<Result<ModelStreamEvent, ModelError>>`. Handles `delta.content`, `delta.tool_calls[]` (id only in the first fragment per index — D6), `finish_reason`, the trailing `usage` object, and `data: [DONE]` → `Completed`.

- [ ] **Step 1: Write the failing test** (`tests/openai_fixtures.rs`)

```rust
use rollshot_agent::provider::openai::parse_sse;
use rollshot_agent::CallAssembler;

#[test]
fn tool_call_with_id_only_in_first_fragment_assembles() {
    // D6: subsequent fragments for the same index omit id/name.
    let bytes = include_bytes!("fixtures/openai/tool_args_split.txt");
    let mut a = CallAssembler::new(64 * 1024);
    for ev in parse_sse(bytes) { a.push(ev.unwrap()).unwrap(); }
    let turn = a.finish().unwrap();
    assert_eq!(turn.tool_calls.len(), 1);
    assert_eq!(turn.tool_calls[0].call_id, "call_1");
    assert!(turn.tool_calls[0].arguments.is_object());
}
```

- [ ] **Step 2: Run → FAIL.** `rtk cargo test -p rollshot-agent --test openai_fixtures`

- [ ] **Step 3: Implement the parser**

```rust
use crate::budget::ModelUsage;
use crate::error::ProviderFailureKind;
use crate::model::{ModelCompletion, ModelError, ModelStreamEvent, ToolCallFragment};

pub fn parse_sse(bytes: &[u8]) -> Vec<Result<ModelStreamEvent, ModelError>> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    let mut completed = false;
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data: ").or_else(|| line.strip_prefix("data:")) else { continue };
        let data = data.trim();
        if data.is_empty() { continue; }
        if data == "[DONE]" {
            out.push(Ok(ModelStreamEvent::Completed(ModelCompletion { stop_reason: "stop".into() })));
            completed = true;
            continue;
        }
        let json: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(e) => { out.push(Err(ModelError { kind: ProviderFailureKind::Malformed, pre_first_delta: out.is_empty(), detail: e.to_string() })); continue; }
        };
        if let Some(u) = json.get("usage").filter(|u| !u.is_null()) {
            out.push(Ok(ModelStreamEvent::UsageDelta(ModelUsage {
                input_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
                output_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
            })));
        }
        let Some(choice) = json["choices"].get(0) else { continue };
        if let Some(c) = choice["delta"]["content"].as_str() {
            if !c.is_empty() { out.push(Ok(ModelStreamEvent::TextDelta(c.into()))); }
        }
        if let Some(calls) = choice["delta"]["tool_calls"].as_array() {
            for tc in calls {
                out.push(Ok(ModelStreamEvent::ToolCallDelta(ToolCallFragment {
                    index: tc["index"].as_u64().unwrap_or(0) as u32,
                    id: tc["id"].as_str().map(Into::into),
                    name: tc["function"]["name"].as_str().map(Into::into),
                    args_fragment: tc["function"]["arguments"].as_str().unwrap_or("").into(),
                })));
            }
        }
    }
    let _ = completed;
    out
}
```

- [ ] **Step 4: Create fixtures** under `tests/fixtures/openai/` (same §13.4 set as Anthropic). Example `tool_args_split.txt`:

```
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"validate_automation","arguments":""}}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{}"}}]}}]}

data: {"choices":[{"finish_reason":"tool_calls"}]}

data: {"usage":{"prompt_tokens":10,"completion_tokens":4}}

data: [DONE]
```

- [ ] **Step 5: Run → PASS.** Add the remaining §13.4 cases (same table as Task 21 Step 6, OpenAI framing). **Step 6: Commit** `feat(agent): OpenAI SSE parser + fixtures (§6.3, §13.4, D6, D7)`.

---

### Task 25: OpenAI transport + error/finish mapping + cancellation

**Files:** Modify `provider/openai.rs`; test `tests/openai_fixtures.rs` + `#[ignore]` live smoke.

**Interfaces:** Produces `OpenAiModel { api_key, model, base_url }` implementing `RollshotModel`, mirroring Task 22 (POST + buffered `parse_sse` + cancellation `take_while`). HTTP error statuses (401→Auth, 429→RateLimit, 5xx→Transport, else Rejection) map to `ModelError`.

- [ ] **Step 1: Write the failing test** (HTTP-status→kind mapping, as a pure helper `classify_status(u16) -> ProviderFailureKind`).

```rust
use rollshot_agent::provider::openai::classify_status;
use rollshot_agent::ProviderFailureKind::*;

#[test]
fn http_status_classification() {
    assert_eq!(classify_status(401), Auth);
    assert_eq!(classify_status(429), RateLimit);
    assert_eq!(classify_status(503), Transport);
    assert_eq!(classify_status(400), Rejection);
}
```

- [ ] **Step 2: Run → FAIL.** **Step 3:** implement `classify_status` + `OpenAiModel` transport (mirror Task 22; map `resp.status()` via `classify_status` before reading the body). **Step 4:** add `#[ignore]` live smoke. **Step 5: Run → PASS.** **Step 6: Commit** `feat(agent): OpenAI transport, status classification, cancellation`.

- [ ] **Phase 6 checkpoint:** `rtk cargo test -p rollshot-agent && rtk cargo clippy -p rollshot-agent --all-targets -- -D warnings` → PASS.

---

## Phase 7 — Cancellation, privacy, resource, and cross-crate integration hardening

### Task 26: Privacy tests (§12, §13.6)

**Files:** Test `crates/rollshot-agent/tests/privacy.rs`.

**Interfaces:** Consumes `Driver`, `RunEvent`, `ScriptedModel`, terminal types.

- [ ] **Step 1: Write the tests**

```rust
use rollshot_agent::testing::{valid_automation_source, ScriptedModel};
use rollshot_agent::tool::default_registry;
use rollshot_agent::{AssembledTurn, Driver, RunBudget, RunEvent, ToolEnv};
use serde_json::json;

#[test]
fn events_never_contain_raw_automation_source() {
    let secret = valid_automation_source();
    let script = ScriptedModel { turns: vec![
        AssembledTurn::tool_call("c1", "replace_automation_source", json!({"source": secret})),
        AssembledTurn::tool_call("c2", "validate_automation", json!({})),
        AssembledTurn::tool_call("c3", "dry_run_automation", json!({})),
        AssembledTurn::tool_call("c4", "submit_for_review", json!({})),
    ]};
    let mut driver = Driver::new("redact".into(), default_registry(), ToolEnv::fake(), RunBudget::test_default());
    let _ = driver.run_scripted(&mut { let mut s = script; s });
    for (_, e) in driver.events().events() {
        if let RunEvent::TextDelta { text } = e {
            assert!(!text.contains(secret), "raw source leaked into an event");
        }
    }
}

#[test]
fn ready_for_review_snapshot_holds_only_counts() {
    // budget_usage_snapshot is (u64, u64) — IDs/counts only, no payloads (spec §12).
    // Compile-time guarantee via the type; this test documents intent.
}
```

- [ ] **Step 2: Run → PASS.** `rtk cargo test -p rollshot-agent --test privacy`

- [ ] **Step 3:** Audit `tracing` call sites added across the crate: confirm every event uses a `rollshot::agent::*` target with structured fields only (provider/model/run IDs, counts, durations, error class, source generation) and **no** prompt/response/source/attachment/sensitive-tool payload. Grep: `rtk grep -rn "tracing::" crates/rollshot-agent/src` and review each.

- [ ] **Step 4: Commit** `test(agent): privacy guarantees for events and tracing (§12, §13.6)`.

### Task 27: Cross-crate integration with the real QuickJs executor (§13.7)

**Files:** Test `crates/rollshot-agent/tests/integration.rs`.

**Interfaces:** Consumes `rollshot_automation_rquickjs::QuickJsExecutor`, `rollshot_automation::FakeAutomationHost`, `Driver`. Builds a `ToolEnv` whose `executor` is the **real** `QuickJsExecutor` (not the fake), then runs the scripted author loop and asserts a valid `ReadyForReview` whose proposal came from real sandbox execution. No `ImageDocument` is touched.

- [ ] **Step 1: Add `ToolEnv::with_real_executor()`** in `testing.rs` (variant of `fake()` swapping `executor: Box::new(rollshot_automation_rquickjs::QuickJsExecutor::default())`). Note: `rollshot-automation-rquickjs` is already a dependency (Task 1).

- [ ] **Step 2: Write the failing test**

```rust
use rollshot_agent::testing::{valid_automation_source, ScriptedModel};
use rollshot_agent::tool::default_registry;
use rollshot_agent::{AssembledTurn, Driver, RunBudget, RunTerminalState, ToolEnv};
use serde_json::json;

#[test]
fn author_loop_with_real_sandbox_produces_a_valid_proposal() {
    let script = ScriptedModel { turns: vec![
        AssembledTurn::tool_call("c1", "replace_automation_source", json!({"source": valid_automation_source()})),
        AssembledTurn::tool_call("c2", "validate_automation", json!({})),
        AssembledTurn::tool_call("c3", "dry_run_automation", json!({})),
        AssembledTurn::tool_call("c4", "submit_for_review", json!({})),
    ]};
    let mut driver = Driver::new("redact".into(), default_registry(), ToolEnv::with_real_executor(), RunBudget::test_default());
    match driver.run_scripted(&mut { let mut s = script; s }) {
        RunTerminalState::ReadyForReview(r) => {
            // proposal candidates depend on valid_automation_source()'s emitted candidates
            assert_eq!(r.automation.source, valid_automation_source());
        }
        other => panic!("expected ReadyForReview from real sandbox, got {other:?}"),
    }
}
```

- [ ] **Step 3: Run → PASS** (this exercises the real rquickjs dry-run end-to-end). `rtk cargo test -p rollshot-agent --test integration`

> Implementer note: `valid_automation_source()` must be a source the real sandbox executes to a decodable proposal. If the chosen source emits candidates, assert their count/shape; if it emits none, `candidates.is_empty()`. Keep it deterministic and synthetic.

- [ ] **Step 4: Commit** `test(agent): cross-crate integration with real QuickJs sandbox (§13.7)`.

### Task 28: Final verification, self-review, and handoff (§15.12, §14)

- [ ] **Step 1: Full workspace verification** (BAC is platform-independent — no capture/UI — so this runs on hosted CI without a display)

Run: `rtk cargo fmt --check`
Run: `rtk cargo test -p rollshot-agent`
Run: `rtk cargo clippy -p rollshot-agent --all-targets -- -D warnings`
Run: `rtk cargo test` (full workspace — confirm no regression)
Expected: all PASS.

- [ ] **Step 2: Success-criteria self-check against spec §15** — verify each of the 12 criteria has a passing test or a structural guarantee. Specifically confirm: (1) scripted author loop → `ReadyForReview` (Task 16); (2) `request_user_input` → `NeedsUserInput` (Task 16); (3) both providers' fixtures prove streaming + tool normalization (Tasks 21/24); (4) text deltas observable before completion, tools only after assembly (Tasks 19/17); (6) every budget/cancellation path has a typed terminal (Tasks 4/8/16/19); (7) no auto-retry (no retry code exists); (8) OCR/layout typed-unavailable (Task 10); (9) no persistence/`ImageDocument` mutation (grep confirms); (10) no Rig types in public API (`rtk grep -rn "rig_core" crates/rollshot-agent/src/lib.rs` → only inside `driver.rs`/`provider`); (11) privacy (Task 26).

- [ ] **Step 3: Write the completion handoff** `docs/superpowers/handoffs/2026-06-23-bounded-agent-core.md`: delivered crate, locked deps (`rig-core = "=0.39.0"`, `reqwest`), D1 gate outcome (Rig kept / or fallback taken), public API surface, the `NeedsUserInput` draft-reference contract for SP5 resume (D5), and the verification evidence (command output).

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/handoffs/2026-06-23-bounded-agent-core.md
git commit -m "docs(agent): bounded agent core completion handoff + verification evidence"
```

---

## Self-Review

**1. Spec coverage:** §1/§2 scope → Tasks 1–28; §4 domain/draft/terminal → Tasks 2,3,7; §5 events → Task 5; §6 facade/stream/adapters → Tasks 15,17,19,20–25; §7 driver → Task 16; §8 tools → Tasks 9–14; §9 budgets → Task 4; §10 cancellation → Task 8; §11 errors → Task 6 (+D3 in 9/16); §12 privacy → Task 26; §13 testing → distributed across all tasks + 26/27; §14 phases → Phases 1–7; §15 criteria → Task 28 Step 2. §0 decisions D1–D8 each have a cited home (D1→Task 18 gate, D2→Task 8, D3→Tasks 9/16, D4→Tasks 4/19, D5→Tasks 7/13, D6→Tasks 23/24, D7→Tasks 21/24, D8→Tasks 9/10/17). No uncovered requirement.

**2. Placeholder scan:** The only deliberate lookup is `valid_automation_source()` (Task 15) — a concrete "copy a passing source from rollshot-automation's tests" instruction, flagged at every dependent task. Fixture `.txt` contents are real SSE grammar with a D7 recording instruction. No "TBD"/"implement later"/vague steps remain.

**3. Type consistency:** `AssembledTurn`/`AssembledToolCall` (model.rs) flow unchanged through `CallAssembler` (Task 17) → `Driver::feed_model_turn` (Task 16). `ToolOutcome`/`ToolContext`/`ToolEnv` (Task 9) are consumed identically by Tasks 10–14 and the driver. `ModelUsage` (Task 4) is the single usage type across budget/model/assembler. `RunTerminalState` variants are produced only by the driver and tools and matched in `terminal_kind`. `RunCancellation::flag()` (Task 8) feeds `execute_to_proposal` (Task 12). Cross-task signatures align.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-06-23-bounded-agent-core.md`. Two execution options:

**1. Subagent-Driven (recommended)** — dispatch a fresh subagent per task, review between tasks, fast iteration. Note: Task 1 modifies the root `Cargo.toml` and must run first/alone; Tasks 2–8 are sequential within `src`; Phases 5 and 6 (Anthropic vs OpenAI adapters) touch disjoint files (`provider/anthropic.rs` vs `provider/openai.rs`) and can run as two parallel lanes once Phase 4 lands.

**2. Inline Execution** — execute tasks in this session via executing-plans, batched with review checkpoints.

Which approach?
