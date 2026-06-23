# Bounded Agent Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> `superpowers:executing-plans`. This plan is intentionally sequential because
> every task changes `crates/rollshot-agent`.

**Goal:** Build `crates/rollshot-agent`, a provider-neutral and bounded control
plane that uses Anthropic or OpenAI Chat Completions to author a redaction
automation through typed tools, then returns either a reviewable
`ReadyForReview` value or an explicit terminal state without persisting state,
mutating `ImageDocument`, or exposing Rig types.

**Architecture:** Add one crate. BAC owns its public domain, tools, budgets,
cancellation, privacy-safe event interface, and run terminal states. Rig 0.39
remains private and supplies the turn state machine, provider clients, streamed
completion decoding, and `StreamedTurnAssembler`. BAC adapts Rig provider
streams into its public events and executes Rollshot tools serially.

**Tech stack:** Rust 2021, workspace MSRV 1.94, `rig-core = "=0.39.0"`,
`tokio`, `tokio-util`, `futures-util`, `serde`, `serde_json`, `schemars`,
`tracing`, `thiserror`, and existing Rollshot automation/proposal crates.

**Design input:** `docs/superpowers/specs/2026-06-23-bounded-agent-core-design.md`.
The reviewed plan below supersedes that document's D1 implementation choice:
inspection of Rig 0.39 found a complete public streamed-turn protocol and both
required provider streaming implementations. The product goal and safety
requirements remain unchanged.

---

## Engineering Review Decisions

### Auto decision D1 — Reduce the 28-task custom stack

Context: The prior plan created more than 20 files across 28 tasks and rebuilt
provider streaming machinery already present in Rig 0.39.

ELI10: The old plan proposed building a second engine beside the engine already
in the dependency. That creates two places for stream framing, tool-call
assembly, usage accounting, and error behavior to disagree.

Stakes if we pick wrong: BAC ships later with a larger protocol surface and
more provider-specific failure modes.

Recommendation: **1A** because the smallest complete product is one BAC crate
with thin provider adapters over Rig's tested streaming path.

Note: options differ in kind, not coverage — no completeness score.

Pros / cons:

A) **1A — Nine-task thin-adapter plan (recommended)** `(human: ~5–8 days / AI: ~1–2 sessions)`
  ✅ Preserves the full goal while removing accidental protocol complexity.
  ❌ Couples the private implementation to pinned Rig 0.39 behavior.

B) **1B — Keep the 28-task custom implementation** `(human: ~3–5 weeks / AI: several sessions)`
  ✅ Gives complete ownership of provider wire parsing.
  ❌ Duplicates mature code and greatly increases maintenance.

Net: Reuse the dependency for commodity streaming and spend BAC complexity on
Rollshot policy, bounds, and review semantics. This follows “boring by default”
and DRY.

### Auto decision D2 — Use Rig's streamed-turn protocol

Context: Rig 0.39 exposes `StreamedTurnAssembler`, `StreamedTurnEvent`,
`AgentRun::streamed_turn`, and invalid-tool-call resolution.

ELI10: Rig already knows how to join partial text and partial JSON tool calls
into one correct turn. BAC should feed it stream items and react to its explicit
events instead of inventing another assembler.

Stakes if we pick wrong: fragmented JSON, unknown tools, or usage events can
advance the state machine incorrectly.

Recommendation: **2A** because it is the only path that uses the dependency's
documented hand-driven streaming contract.

Note: options differ in kind, not coverage — no completeness score.

Pros / cons:

A) **2A — `StreamedTurnAssembler` + `AgentRun::streamed_turn` (recommended)** `(human: ~1 day / AI: ~30 min)`
  ✅ Handles text, reasoning, tool deltas, invalid calls, and turn completion coherently.
  ❌ Requires a characterization test when upgrading Rig.

B) **2B — BAC `CallAssembler` + `model_response`**
  ✅ Keeps more logic under BAC names.
  ❌ Conflicts with Rig's streamed-call invariant and duplicates assembly.

Net: Pin Rig and test its boundary rather than shadowing it. This is explicit
and minimizes the diff.

### Auto decision D3 — Build each request from Rig's current prompt and history

Context: `AgentRunStep::CallModel` returns `{ prompt, history, turn }`; the old
plan reused a fixed request and therefore omitted prior tool results.

ELI10: An agent can only learn from a tool if the next model request contains
the tool result. Every turn must be built from the state machine's current
prompt and complete history.

Stakes if we pick wrong: the model repeatedly calls tools without seeing their
results and cannot finish the author loop.

Recommendation: **3A** because it preserves Rig's canonical conversation state.

Completeness: A=10/10, B=3/10.

Pros / cons:

A) **3A — Convert `prompt` + `history` on every call (recommended)** `(human: ~0.5 day / AI: ~20 min)`
  ✅ Tool results and completed assistant turns reach the next provider call.
  ❌ Requires private conversion tests for supported Rig message variants.

B) **3B — Reuse the initial `ModelRequest`**
  ✅ Less request-building code.
  ❌ Produces a non-functional multi-turn agent.

Net: Conversation continuity is required behavior, not optional completeness.

### Auto decision D4 — Separate transient text events from retained audit data

Context: The old append-only event log retained every text delta, duplicating
the complete provider response despite the privacy rules.

ELI10: Product UI needs text while it arrives, but BAC does not need to keep a
second transcript. A callback can deliver text transiently while retained
records contain only IDs, counts, and classifications.

Stakes if we pick wrong: sensitive prompts or responses remain in memory and
diagnostics longer than intended.

Recommendation: **4A** because it supports streaming without creating a hidden
conversation store.

Completeness: A=10/10, B=5/10.

Pros / cons:

A) **4A — `RunEventSink` for transient events plus metadata-only audit events (recommended)** `(human: ~1 day / AI: ~30 min)`
  ✅ Preserves UI streaming and privacy boundaries.
  ❌ Callers must decide whether and how to retain displayed text.

B) **4B — Append all deltas to `EventLog`**
  ✅ Simple replay in tests.
  ❌ Duplicates sensitive provider output and grows with every token.

Net: Keep the session's completed messages as the only conversation state.

### Auto decision D5 — Use race-free cancellation with one public source

Context: The prior `AtomicBool` plus `Notify` design had a missed-wakeup window.

ELI10: Cancellation must wake an async provider stream and stop synchronous
QuickJS work. One `RunCancellation` handle will own both the async token and the
existing automation flag, and `cancel()` updates both.

Stakes if we pick wrong: a cancelled run can remain blocked until the provider
or timeout eventually responds.

Recommendation: **5A** because `CancellationToken` is race-free and the existing
`CancellationFlag` remains the dry-run mechanism.

Note: options differ in kind, not coverage — no completeness score.

Pros / cons:

A) **5A — `tokio_util::CancellationToken` bridged to `CancellationFlag` (recommended)** `(human: ~0.5 day / AI: ~15 min)`
  ✅ Async waits wake reliably and QuickJS sees the same logical cancellation.
  ❌ The wrapper internally maintains two representations.

B) **5B — `AtomicBool` + `Notify`**
  ✅ Avoids one dependency.
  ❌ Easy to implement with a missed notification race.

Net: One public source with two purpose-specific internal views is safer than a
clever custom waiter.

### Auto decision D6 — Keep tool execution typed, async, and serial

Context: The old synchronous object-safe tool trait could not represent pending
work or cancellation cleanly, and schema validation was scattered.

ELI10: Providers send JSON, but each tool should immediately decode that JSON
into a strict Rust argument type. Tools run one at a time, return bounded JSON,
and receive cancellation and budget context.

Stakes if we pick wrong: malformed arguments panic, multiple tools race on the
draft, or oversized results exhaust memory.

Recommendation: **6A** because boxed async futures keep the registry object-safe
without premature generic infrastructure.

Completeness: A=10/10, B=6/10.

Pros / cons:

A) **6A — Typed serde/schemars tools returning `BoxFuture`, serial registry (recommended)** `(human: ~2 days / AI: ~1 hour)`
  ✅ Centralizes strict decoding, output limits, cancellation, and call counts.
  ❌ Has small dynamic-dispatch overhead outside any hot path.

B) **6B — Synchronous untyped JSON callbacks**
  ✅ Fewer trait types.
  ❌ Cannot cleanly suspend, cancel, or guarantee argument shape.

Net: Explicit typed boundaries are worth more than avoiding a boxed future.

### Auto decision D7 — Inject inspection instead of pretending vision is prepared

Context: `RealAutomationHost` can answer region features only after a caller
prepares the exact query plan, which BAC does not own.

ELI10: The agent core should ask an inspection service for facts, but it should
not silently create product-specific vision indexes. An unavailable result is a
valid typed answer until the product wires a prepared provider.

Stakes if we pick wrong: region tools appear to work but always fail or perform
hidden expensive preparation.

Recommendation: **7A** because it keeps the crate boundary honest.

Note: options differ in kind, not coverage — no completeness score.

Pros / cons:

A) **7A — Inject `InspectionProvider`; default unsupported capabilities (recommended)** `(human: ~1 day / AI: ~30 min)`
  ✅ BAC tests deterministically and product integration remains explicit.
  ❌ OCR/layout/region capability needs later product wiring.

B) **7B — Construct `RealAutomationHost` inside BAC**
  ✅ Fewer caller-visible dependencies.
  ❌ Couples BAC to query preparation and image lifecycle it does not own.

Net: Dependency injection prevents a false abstraction and respects existing
crate responsibilities.

### Auto decision D8 — Reuse Rig's Anthropic and OpenAI streaming providers

Context: Rig 0.39 includes both required provider clients, SSE handling, usage
mapping, and streaming tool-call decoding; OpenAI can be switched to Chat
Completions with `.completions_api()`.

ELI10: BAC still chooses the provider and enforces Rollshot policy, but it does
not parse wire frames itself. Mock HTTP tests feed real-format SSE through Rig's
public provider clients to prove the boundary.

Stakes if we pick wrong: custom parsers mishandle chunk boundaries, CRLF,
multi-line events, cumulative usage, or provider error bodies.

Recommendation: **8A** because it retains production provider support with far
less wire-protocol code.

Note: options differ in kind, not coverage — no completeness score.

Pros / cons:

A) **8A — Thin adapters over Rig providers + mock-server fixtures (recommended)** `(human: ~2 days / AI: ~1 hour)`
  ✅ True incremental streaming, shared client reuse, and provider error mapping.
  ❌ Upstream behavior must be pinned and characterized.

B) **8B — Hand-written reqwest/SSE adapters**
  ✅ Maximum wire-level control.
  ❌ Rebuilds parser and provider maintenance obligations.

Net: Provider protocol parsing is commodity infrastructure here; BAC policy is
the product-specific work.

### Auto decision D9 — Rewrite plan steps around compiled APIs, not placeholders

Context: The old plan contained “confirm API,” empty ignored smoke tests,
non-red tests, temporary stubs, and shell commands that violated repo rules.

ELI10: A plan is executable only when every behavior starts with a failing test,
has a concrete implementation step, and ends with a command whose result is
known.

Stakes if we pick wrong: implementers improvise architecture mid-task and green
tests do not prove the intended behavior.

Recommendation: **9A** because a smaller task list allows every step to have a
real RED/GREEN/commit boundary.

Completeness: A=10/10, B=4/10.

Pros / cons:

A) **9A — Replace the plan with nine complete TDD tasks (recommended)** `(human: ~1 day / AI: ~45 min)`
  ✅ Files, tests, commands, and commits remain consistent.
  ❌ Removes detailed speculative code snippets that looked concrete.

B) **9B — Patch individual placeholders**
  ✅ Smaller documentation diff.
  ❌ Leaves contradictory architecture spread across 3,972 lines.

Net: A coherent rewrite is the smaller implementation risk.

### Auto decision D10 — Require deterministic contract, failure, and privacy tests

Context: The old fixtures could be synthetic despite claiming real provenance,
and privacy tests covered only one event string.

ELI10: CI must test both provider formats without real API keys, every terminal
class, stale generation, budget exhaustion, cancellation, and sentinel secret
absence in tracing and retained records.

Stakes if we pick wrong: the happy path passes while production failures become
silent, leaky, or non-terminal.

Recommendation: **10A** because complete deterministic tests are cheap relative
to debugging provider incidents.

Completeness: A=10/10, B=6/10.

Pros / cons:

A) **10A — Mock HTTP provider fixtures plus exhaustive local failure matrix (recommended)** `(human: ~2 days / AI: ~1 hour)`
  ✅ Runs offline and exercises the real provider adapters.
  ❌ Fixture updates are required when pinned Rig behavior changes.

B) **10B — Scripted model happy path plus optional live smoke**
  ✅ Less test code.
  ❌ Does not close provider framing, cancellation, or privacy risk in CI.

Net: Systems over heroes means CI must reproduce the ugly paths.

### Auto decision D11 — Bound streams, arguments, outputs, attachments, and time

Context: The old plan buffered full HTTP bodies, accumulated unbounded text, and
truncated serialized JSON after allocation.

ELI10: A hostile or broken provider can send forever, and a tool can return a
huge value. BAC must stop before limits are exceeded, never produce invalid
truncated JSON, and reuse provider clients.

Stakes if we pick wrong: memory grows without bound, cancellation is delayed,
or corrupted tool results reach the next turn.

Recommendation: **11A** because hard pre/post checks make resource behavior
predictable.

Completeness: A=10/10, B=5/10.

Pros / cons:

A) **11A — Incremental streams, deadlines, byte caps, checked accounting (recommended)** `(human: ~1.5 days / AI: ~45 min)`
  ✅ Gives explicit ceilings and deterministic terminal errors.
  ❌ Adds limit plumbing to every provider/tool boundary.

B) **11B — Rely on provider and process defaults**
  ✅ Less code.
  ❌ Allows runaway memory, time, and cost.

Net: Bounds are BAC's defining responsibility and cannot be delegated.

---

## Scope

### In scope

- One new `rollshot-agent` library crate.
- In-memory sessions and runs with completed messages only.
- Anthropic Messages streaming and OpenAI Chat Completions streaming through
  pinned Rig 0.39 provider clients.
- A provider-neutral BAC API with no public Rig types.
- Typed inspection and automation-authoring tools.
- Serial tool execution.
- Draft generations and stale-evidence rejection.
- Validation, proposal policy check, QuickJS dry-run, and review handoff.
- Hard budgets for time, model calls, tokens, estimated cost, tools, arguments,
  source, attachments, capabilities, proposal candidates, and affected area.
- Race-free cancellation across async streams and synchronous automation.
- Transient streaming events and metadata-only retained audit data.
- Deterministic scripted-model and mock-provider tests.

### NOT in scope

- Persistence, resume after process exit, or a durable event store — BAC state
  is intentionally in memory.
- `ImageDocument` mutation or automatic proposal acceptance — review remains a
  product-layer decision.
- UI for consent, provider selection, credentials, review, or user questions —
  this plan produces typed states only.
- Automatic provider retry, fallback, or failover — failures terminate with a
  typed report to avoid duplicate tool effects and hidden cost.
- OpenAI Responses API migration — Chat Completions is the approved initial
  target; keep the facade provider-neutral so a later plan can migrate it.
- BAC-owned OCR/layout/index construction — callers inject an
  `InspectionProvider`; unsupported capabilities return typed unavailable
  results.
- Live-provider tests in required CI — mock HTTP fixtures are mandatory; an
  opt-in ignored smoke test may be added later when credential handling is
  approved.
- Publishing `rollshot-agent` outside the workspace — it is an internal
  library consumed by later product integration.
- Public test-support APIs — scripted models and fake inspection providers stay
  in unit tests.

---

## What Already Exists

| Existing code | Reuse decision |
|---|---|
| `rollshot_automation::validate_source` and `ValidationLimits` | Reuse for source validation; do not implement a second parser. |
| `rollshot_automation::{AutomationExecutor, ExecutionPolicy, CancellationFlag}` | Reuse for dry-run execution and synchronous cancellation. |
| `rollshot_automation::execute_to_proposal` | Reuse to produce an `EditProposal`. |
| `rollshot_automation_rquickjs::QuickJsExecutor` | Reuse as the real dry-run executor. |
| `rollshot_edit_proposal::{EditProposal, validate_policy}` | Reuse for candidate and image-bound policy enforcement. |
| `rollshot_vision::RealAutomationHost` | Do not construct inside BAC; adapt it later behind `InspectionProvider` after product query preparation. |
| Rig 0.39 `AgentRun` | Reuse as the private turn state machine. |
| Rig 0.39 `StreamedTurnAssembler` and `AgentRun::streamed_turn` | Reuse instead of BAC `CallAssembler`. |
| Rig 0.39 Anthropic and OpenAI providers | Reuse for HTTP/SSE parsing and provider DTO normalization. |
| `spikes/rig-agent` scripted tests | Reuse behavior and lessons, not spike source files. |

---

## Data Flow

Add this diagram as a module-level doc comment in `src/driver.rs`:

```text
AuthorizedModelInput
        |
        v
  AgentSession + DraftState + RunBudget
        |
        v
 Rig AgentRun::next_step()
   | CallModel { prompt, history, turn }
   |        |
   |        v
   |   RollshotModel facade
   |        |
   |        +--> Anthropic Rig provider
   |        `--> OpenAI Chat Completions Rig provider
   |                 |
   |                 v
   |       StreamedAssistantContent
   |                 |
   |                 v
   |       StreamedTurnAssembler
   |                 |
   |                 v
   |       AgentRun::streamed_turn()
   |
   | CallTools { calls } -- serial --> ToolRegistry
   |                                  |
   |                                  +--> DraftState generation
   |                                  +--> validation/proposal/QuickJS
   |                                  `--> InspectionProvider
   |
   ` Done --> ReadyForReview | typed terminal state
```

State transitions:

```text
Running
  | submit_review(valid evidence)       -> ReadyForReview
  | request_user_input                  -> NeedsUserInput
  | cancel                              -> Cancelled
  | budget/deadline exceeded            -> BudgetExceeded
  | provider/stream/protocol failure    -> ProviderFailure / AgentProtocolFailure
  | validation or dry-run exhausted     -> SourceValidationFailure / RuntimeFailure
```

---

## File Structure

Create:

- `crates/rollshot-agent/Cargo.toml`
- `crates/rollshot-agent/src/lib.rs`
- `crates/rollshot-agent/src/domain.rs`
- `crates/rollshot-agent/src/runtime.rs`
- `crates/rollshot-agent/src/tools.rs`
- `crates/rollshot-agent/src/model.rs`
- `crates/rollshot-agent/src/driver.rs`
- `crates/rollshot-agent/src/provider.rs`
- `crates/rollshot-agent/tests/provider_contract.rs`
- `crates/rollshot-agent/tests/fixtures/provider_streams.json`
- `docs/superpowers/handoffs/2026-06-23-bounded-agent-core.md`

Modify:

- `Cargo.toml`
- `Cargo.lock`

Unit and acceptance tests for domain, runtime, tools, driver, privacy, and the
real QuickJS integration live in `#[cfg(test)]` modules beside their private
implementation. The provider contract is the only integration-test file
because it must exercise the public provider adapter through a mock HTTP server.

---

## Public Boundary

`lib.rs` exports only Rollshot-owned types:

- `AgentSession`, `AgentSessionId`, `AgentRunId`
- `AuthorizedModelInput`, `AuthorizedInputManifest`, `TransientAttachment`
- `AgentConfig`, `ProviderConfig`, `ProviderId`, `ModelId`
- `RunBudget`, `RunBudgetUsage`
- `RunCancellation`
- `RunEvent`, `RunEventSink`, `AuditEvent`
- `InspectionProvider`, `InspectionError`
- `AgentRunner`, `AgentError`, `RunTerminalState`
- `ReadyForReview`, `NeedsUserInput`

Rig types, provider clients, credentials, provider request/response DTOs,
stream frames, tool registry internals, and QuickJS types remain private.

---

## Task Dependency and Execution Strategy

All tasks touch `crates/rollshot-agent`; execute sequentially. Task 1 also
modifies the workspace root and therefore serializes all later work.

| Task | Modules touched | Depends on |
|---|---|---|
| 1. Crate and domain | workspace, `rollshot-agent` | — |
| 2. Runtime bounds and state | `rollshot-agent` | 1 |
| 3. Typed tools | `rollshot-agent` | 2 |
| 4. Model facade and Rig stream boundary | `rollshot-agent` | 1, 2 |
| 5. Driver author loop | `rollshot-agent` | 3, 4 |
| 6. Anthropic contract | `rollshot-agent` | 4, 5 |
| 7. OpenAI contract | `rollshot-agent` | 4, 5, 6 |
| 8. Failure, cancellation, privacy, QuickJS | `rollshot-agent` | 2–7 |
| 9. Workspace verification and handoff | workspace, docs | 1–8 |

Parallelization strategy: **Sequential execution, no parallelization
opportunity.** Tasks 6 and 7 look independent but intentionally share
`provider.rs`, the fixture file, and the provider contract test; parallel work
would create unnecessary merge conflicts.

---

## Task 1: Crate Scaffold and Public Domain

**Files:**

- Create: `crates/rollshot-agent/Cargo.toml`
- Create: `crates/rollshot-agent/src/lib.rs`
- Create: `crates/rollshot-agent/src/domain.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

**Behavior:**

- The workspace builds a new internal library crate.
- Public IDs are opaque newtypes.
- `AuthorizedModelInput` owns the user message and transient attachments.
- `AuthorizedInputManifest` describes exactly the authorized provider, model,
  media type, dimensions, and byte counts without copying payload bytes.
- `AgentSession` stores only completed user/assistant text; partial turns and
  attachments never become session messages.
- Input construction rejects descriptor mismatch, unsupported media type,
  attachment-count overflow, per-attachment overflow, and total-byte overflow.

- [ ] **Step 1: Add the compile-only crate scaffold**

  Add `crates/rollshot-agent` to workspace members and create its manifest,
  `lib.rs`, and `domain.rs`. Add these workspace dependencies when absent:

  ```toml
  tokio-util = "0.7"
  schemars = "1"
  wiremock = "0.6"
  ```

  Pin `rig-core = "=0.39.0"` in the crate. Add the existing Rollshot
  automation/proposal/image-document crates plus workspace `tokio`,
  `tokio-util`, `futures-util`, `serde`, `serde_json`, `schemars`, `tracing`,
  and `thiserror`; use `wiremock` only as a dev-dependency.

  Run: `rtk cargo metadata --no-deps`

  Expected: PASS and includes `rollshot-agent`.

- [ ] **Step 2: Write RED unit tests in `domain.rs`**

  Cover:

  - unique session/run IDs;
  - manifest/provider/model consistency;
  - descriptor count and byte count consistency;
  - unsupported media type;
  - per-attachment and total attachment limits;
  - session append only after a completed turn;
  - `Debug` output contains descriptors but not attachment sentinel bytes.

  Run: `rtk cargo test -p rollshot-agent domain::tests`

  Expected: FAIL to compile because the tested domain types are not defined.

- [ ] **Step 3: Implement the smallest domain that passes**

  Use checked byte arithmetic. Redact attachment bytes and user text from
  custom `Debug` implementations. Keep constructors fallible so invalid input
  never reaches a provider.

  Run: `rtk cargo test -p rollshot-agent domain::tests`

  Expected: PASS.

- [ ] **Step 4: Verify formatting and commit**

  Run: `rtk cargo fmt --check`

  Expected: PASS.

  Commit:

  ```bash
  rtk git add Cargo.toml Cargo.lock crates/rollshot-agent
  rtk git commit -m "feat(agent): add bounded agent domain"
  ```

---

## Task 2: Runtime State, Budgets, Cancellation, and Events

**Files:**

- Create: `crates/rollshot-agent/src/runtime.rs`
- Modify: `crates/rollshot-agent/src/lib.rs`

**Behavior:**

- `DraftState` increments generation with checked arithmetic.
- Validation, policy, and dry-run evidence records the exact source generation.
- Recording evidence requires an expected generation and rejects stale results.
- `RunBudget` covers wall time, model calls, input/output tokens, estimated
  cost, tool calls, per-tool calls, argument bytes, result bytes, source bytes,
  attachments, validation attempts, dry-run attempts, capability calls,
  candidate count, and affected area.
- Budget charges are checked before committing mutations and usage snapshots
  charge only positive cumulative deltas once per turn.
- `RunCancellation` bridges `CancellationToken` and `CancellationFlag`.
- `RunEventSink` receives transient text/tool lifecycle events.
- Retained `AuditEvent` contains metadata only.
- Terminal states are explicit and carry no prompt, source, attachment, or raw
  provider payload.

- [ ] **Step 1: Write RED runtime tests**

  Cover every budget dimension, cumulative usage de-duplication, checked
  overflow, paused-time deadline expiry, cancellation before wait,
  cancellation during wait, stale generation evidence, evidence invalidation
  after source replacement, and metadata-only audit serialization.

  Run: `rtk cargo test -p rollshot-agent runtime::tests`

  Expected: FAIL because runtime types do not exist.

- [ ] **Step 2: Implement runtime types**

  Use `tokio::time::Instant` for deadlines and Tokio paused time in tests. Do
  not use `Notify`. `cancel()` must call both the async token and the automation
  flag exactly once from the same public handle.

  The event sink must be synchronous and non-blocking; callers that need async
  delivery provide their own bounded channel. Do not retain `TextDelta`.

  Run: `rtk cargo test -p rollshot-agent runtime::tests`

  Expected: PASS.

- [ ] **Step 3: Verify crate tests and commit**

  Run: `rtk cargo test -p rollshot-agent`

  Expected: PASS.

  Commit:

  ```bash
  rtk git add crates/rollshot-agent/src
  rtk git commit -m "feat(agent): add bounded runtime state"
  ```

---

## Task 3: Typed Serial Tool Registry

**Files:**

- Create: `crates/rollshot-agent/src/tools.rs`
- Modify: `crates/rollshot-agent/src/lib.rs`

**Behavior:**

- Tool definitions derive strict JSON schema with `schemars`.
- Every argument struct uses `#[serde(deny_unknown_fields)]`.
- Known-tool decode/schema failures return a bounded recoverable tool result.
- Unknown tools and incomplete tool calls are terminal protocol failures.
- Tools execute serially in provider order.
- Tool argument and serialized result byte limits are checked; oversized valid
  JSON is rejected, never byte-truncated.
- `replace_source` is the only source mutation.
- `validate_source`, `dry_run`, and `submit_for_review` require matching
  generation evidence.
- `dry_run` uses `execute_to_proposal`, `validate_policy`, and the injected
  executor/host.
- `request_user_input` returns `NeedsUserInput` with current draft evidence.
- `InspectionProvider` exposes context summary and optional region features;
  unsupported OCR/layout/region functions return typed unavailable results.

- [ ] **Step 1: Write RED tests for registry policy**

  Cover:

  - duplicate tool name registration;
  - unknown tool terminal error;
  - known tool malformed JSON recoverable result;
  - unknown fields rejected;
  - argument/result byte limits;
  - per-tool call limits;
  - serial order for multiple calls;
  - terminal tool stops later calls;
  - cancellation before a tool begins.

  Run: `rtk cargo test -p rollshot-agent tools::tests::registry`

  Expected: FAIL because the registry does not exist.

- [ ] **Step 2: Implement object-safe async tools and registry**

  Use a boxed future rather than `async-trait`:

  ```rust
  type ToolFuture<'a> =
      Pin<Box<dyn Future<Output = Result<ToolOutcome, AgentError>> + Send + 'a>>;
  ```

  Registry lookup must not hold an immutable entry borrow while mutating call
  counters. Look up an index or clone an `Arc<dyn Tool>` first.

  Run: `rtk cargo test -p rollshot-agent tools::tests::registry`

  Expected: PASS.

- [ ] **Step 3: Write RED tests for authoring and inspection tools**

  Cover source replacement, validation failure/success, stale evidence,
  proposal policy failure, QuickJS fake execution failure/success, affected-area
  budget, review submission, user input request, available inspection, and
  unavailable inspection.

  Run: `rtk cargo test -p rollshot-agent tools::tests::authoring`

  Expected: FAIL because tool implementations do not exist.

- [ ] **Step 4: Implement the concrete tools**

  Reuse existing automation and proposal APIs. Do not mutate
  `ImageDocument`. Keep `InspectionProvider` independent of
  `rollshot-vision`; later product code can adapt a prepared
  `RealAutomationHost`.

  Run: `rtk cargo test -p rollshot-agent tools::tests`

  Expected: PASS.

- [ ] **Step 5: Commit**

  ```bash
  rtk git add crates/rollshot-agent/src
  rtk git commit -m "feat(agent): add typed bounded tools"
  ```

---

## Task 4: Provider-Neutral Model Facade and Rig Streaming Boundary

**Files:**

- Create: `crates/rollshot-agent/src/model.rs`
- Modify: `crates/rollshot-agent/src/lib.rs`

**Behavior:**

- Public model request/events contain no Rig types.
- Private conversions use every `CallModel { prompt, history, turn }`.
- Tool definitions include strict schemas.
- Model streams are incremental and expose text, tool lifecycle, cumulative
  usage, final metadata, and typed provider failures.
- The driver uses one `StreamedTurnAssembler` per turn and feeds its finished
  value to `AgentRun::streamed_turn`.
- Unknown or incomplete tool calls follow Rig's explicit invalid-call
  resolution path and BAC's terminal policy.
- A `ScriptedModel` exists only under `#[cfg(test)]`.

- [ ] **Step 1: Write RED characterization tests**

  Reproduce the Rig spike with:

  1. first turn streamed tool-call argument fragments;
  2. tool result returned to Rig;
  3. second `CallModel` request containing prior prompt, assistant tool call,
     and tool result;
  4. second turn streamed text;
  5. final `Done`.

  Also test interleaved text/tool deltas, an incomplete call, and an unknown
  tool name.

  Run: `rtk cargo test -p rollshot-agent model::tests`

  Expected: FAIL because the facade and stream bridge do not exist.

- [ ] **Step 2: Implement the facade and private Rig conversions**

  Follow Rig 0.39's documented sequence:

  1. call `AgentRun::next_step`;
  2. on `CallModel`, build the request from that step's prompt and history;
  3. create `StreamedTurnAssembler` with executable/allowed tool names;
  4. ingest every `StreamedAssistantContent`;
  5. handle every `StreamedTurnEvent`;
  6. resolve invalid calls through `AgentRun`;
  7. on normal EOF, call `finish` then `AgentRun::streamed_turn`.

  Do not call `model_response` for streamed turns. Do not create a BAC
  `CallAssembler`.

  Run: `rtk cargo test -p rollshot-agent model::tests`

  Expected: PASS.

- [ ] **Step 3: Add an upgrade guard and commit**

  Add one test that names the pinned Rig version expectation and fails if the
  required streamed APIs or message conversions stop compiling.

  Run: `rtk cargo test -p rollshot-agent`

  Expected: PASS.

  Commit:

  ```bash
  rtk git add crates/rollshot-agent/src
  rtk git commit -m "feat(agent): bridge rig streamed turns"
  ```

---

## Task 5: Bounded Agent Driver and Author Loop

**Files:**

- Create: `crates/rollshot-agent/src/driver.rs`
- Modify: `crates/rollshot-agent/src/lib.rs`

**Behavior:**

- `AgentRunner` owns one run lifecycle.
- Before each model/tool operation it checks cancellation, deadline, and the
  relevant budget.
- Model usage is charged from cumulative per-turn snapshots without
  double-counting.
- Provider text is emitted transiently and bounded by maximum assistant bytes.
- Tool calls execute serially and every non-terminal call returns one result to
  Rig.
- Successful submission returns immutable `ReadyForReview`.
- A model `Done` after the latest validation or runtime failure maps to
  `SourceValidationFailure` or `RuntimeFailure`, not a generic protocol error.
- A terminal tool result ends the BAC run; BAC does not promise resumability.

- [ ] **Step 1: Write the RED full-loop test**

  Script:

  1. inspect context;
  2. replace source;
  3. validate;
  4. dry-run;
  5. submit for review;
  6. return final text.

  Assert tool order, generation evidence, second-turn history, exact usage
  charge, emitted event order, completed session messages, and
  `ReadyForReview`.

  Run: `rtk cargo test -p rollshot-agent driver::tests::full_author_loop`

  Expected: FAIL because the driver does not exist.

- [ ] **Step 2: Implement the driver state machine**

  Add the data-flow diagram from this plan as the module doc comment. Keep one
  top-level loop with exhaustive matches over `AgentRunStep`; extract model-turn
  and tool-turn helpers only when they each have one clear responsibility.

  Run: `rtk cargo test -p rollshot-agent driver::tests::full_author_loop`

  Expected: PASS.

- [ ] **Step 3: Add RED terminal-path tests**

  Cover:

  - `NeedsUserInput`;
  - cancellation before model call, during stream, and before tool;
  - model-call, token, tool, source, candidate, area, cost, and deadline limits;
  - provider EOF before completion;
  - provider auth/rate-limit/transport failure mapping;
  - unknown tool and incomplete tool call;
  - repeated validation and dry-run failures;
  - model completion without submission.

  Run: `rtk cargo test -p rollshot-agent driver::tests::terminal`

  Expected: FAIL until all branches are implemented.

- [ ] **Step 4: Implement terminal mappings**

  Every failure must emit one metadata-only audit event and return one typed
  terminal/error value. No branch may log or retain raw arguments, source,
  prompt, response, attachment, or credential data.

  Run: `rtk cargo test -p rollshot-agent driver::tests`

  Expected: PASS.

- [ ] **Step 5: Commit**

  ```bash
  rtk git add crates/rollshot-agent/src
  rtk git commit -m "feat(agent): drive bounded authoring runs"
  ```

---

## Task 6: Anthropic Production Adapter Contract

**Files:**

- Create: `crates/rollshot-agent/src/provider.rs`
- Create: `crates/rollshot-agent/tests/provider_contract.rs`
- Create: `crates/rollshot-agent/tests/fixtures/provider_streams.json`
- Modify: `crates/rollshot-agent/src/lib.rs`

**Behavior:**

- One reusable Rig Anthropic client per adapter.
- Request contains system prompt, current prompt, history, authorized
  attachments, strict tool schemas, model, and `max_tokens`.
- Only first-turn authorized attachments are sent unless a later prompt
  explicitly contains an authorized attachment reference.
- Real-format Anthropic SSE is consumed incrementally through Rig.
- Text, fragmented tool input, cumulative usage, stop reason, unknown event,
  provider error, malformed frame, and early EOF map deterministically.
- BAC performs no automatic retry.

- [ ] **Step 1: Add RED mock-server contract cases**

  In the fixture JSON, add content-scrubbed Anthropic streams for:

  - text-only;
  - tool input split across events;
  - text plus tool call;
  - cumulative usage;
  - unknown event type;
  - malformed JSON;
  - incomplete stream;
  - provider 401, 429, and 500.

  Store fixture provenance beside each case: official documentation URL,
  retrieval date, original event names, and a note describing content
  substitutions. Fixtures must preserve framing and field shape; they are not
  claimed to be live account captures.

  The mock server must send at least two chunks with a synchronization barrier;
  assert the first text event is observable before the response completes.

  Run: `rtk cargo test -p rollshot-agent --test provider_contract anthropic`

  Expected: FAIL because the Anthropic adapter does not exist.

- [ ] **Step 2: Implement the Anthropic adapter over Rig**

  Configure the Rig client with injected API key and base URL. Keep both out of
  `Debug`, events, errors, and tracing. Convert Rig stream output into BAC
  events; do not parse SSE in BAC.

  Run: `rtk cargo test -p rollshot-agent --test provider_contract anthropic`

  Expected: PASS with no network or environment variables.

- [ ] **Step 3: Commit**

  ```bash
  rtk git add crates/rollshot-agent/src crates/rollshot-agent/tests
  rtk git commit -m "feat(agent): add anthropic streaming adapter"
  ```

---

## Task 7: OpenAI Chat Completions Production Adapter Contract

**Files:**

- Modify: `crates/rollshot-agent/src/provider.rs`
- Modify: `crates/rollshot-agent/tests/provider_contract.rs`
- Modify: `crates/rollshot-agent/tests/fixtures/provider_streams.json`

**Behavior:**

- One reusable Rig OpenAI client switched explicitly with `.completions_api()`.
- Request contains system prompt, current prompt, history, authorized image
  inputs, strict function schemas, and model.
- Set `parallel_tool_calls: false`; BAC still handles multiple returned calls
  serially if a provider emits them.
- Real-format Chat Completions SSE is consumed incrementally through Rig.
- Fragmented function name/arguments, call index/ID, usage, finish reason,
  `[DONE]`, provider errors, malformed frames, and early EOF map
  deterministically.
- BAC performs no automatic retry.

- [ ] **Step 1: Add RED OpenAI mock-server contract cases**

  Add fixture cases for:

  - text-only;
  - function arguments split across chunks;
  - multiple indexed tool calls;
  - usage chunk;
  - `[DONE]`;
  - malformed JSON;
  - incomplete stream;
  - provider 401, 429, and 500.

  Store the same provenance metadata required by Task 6.

  Assert the outbound request uses Chat Completions, strict schemas, and
  `parallel_tool_calls: false`. Assert first text arrives before response
  completion.

  Run: `rtk cargo test -p rollshot-agent --test provider_contract openai`

  Expected: FAIL because the OpenAI adapter does not exist.

- [ ] **Step 2: Implement the OpenAI adapter over Rig**

  Use the provider facade shared with Anthropic; do not duplicate driver logic
  or write a BAC SSE parser.

  Run: `rtk cargo test -p rollshot-agent --test provider_contract openai`

  Expected: PASS with no network or environment variables.

- [ ] **Step 3: Run both contracts and commit**

  Run: `rtk cargo test -p rollshot-agent --test provider_contract`

  Expected: PASS.

  Commit:

  ```bash
  rtk git add crates/rollshot-agent/src/provider.rs crates/rollshot-agent/tests
  rtk git commit -m "feat(agent): add openai chat streaming adapter"
  ```

---

## Task 8: Privacy, Cancellation, and Real QuickJS Integration

**Files:**

- Modify: `crates/rollshot-agent/src/driver.rs`
- Modify: `crates/rollshot-agent/src/provider.rs`
- Modify: `crates/rollshot-agent/src/runtime.rs`
- Modify: `crates/rollshot-agent/src/tools.rs`

**Behavior:**

- Provider stream waits select on cancellation and the run deadline.
- Dropping a cancelled stream ends network work; no retry is started.
- QuickJS receives the same run cancellation through `CancellationFlag`.
- Real `QuickJsExecutor` produces a proposal that passes policy and is returned
  in `ReadyForReview`.
- Secret sentinels never appear in tracing, `Debug`, terminal reports, audit
  events, or serialized session state.
- Assistant text, tool arguments, results, attachments, and source bytes obey
  hard limits before unbounded accumulation.

- [ ] **Step 1: Write RED cancellation and resource tests**

  Use paused Tokio time and controlled streams. Cover cancellation between
  chunks, deadline while idle, maximum assistant bytes, maximum argument bytes,
  maximum result bytes, and dropping the provider stream.

  Run: `rtk cargo test -p rollshot-agent driver::tests::resource_bounds`

  Expected: FAIL until every boundary is wired.

- [ ] **Step 2: Implement cancellation/deadline selection and bounds**

  Do not truncate JSON. Do not buffer a whole provider response. Use checked
  arithmetic and classify each exceeded limit as `BudgetExceeded`.

  Run: `rtk cargo test -p rollshot-agent driver::tests::resource_bounds`

  Expected: PASS.

- [ ] **Step 3: Write RED privacy and QuickJS tests**

  Place distinct sentinel strings in:

  - user prompt;
  - attachment bytes;
  - API key;
  - automation source;
  - tool arguments;
  - provider raw metadata that is not completed assistant text.

  Capture tracing and inspect every terminal report, audit event, error,
  provider/client `Debug` representation, and serialized session. The session
  may contain completed user/assistant text by design, but must not contain
  attachment bytes, automation source, tool arguments/results, credentials, or
  raw provider metadata. Also run a valid automation through the real
  `QuickJsExecutor` and verify the returned proposal and generation evidence.

  Run: `rtk cargo test -p rollshot-agent driver::tests::privacy_and_quickjs`

  Expected: FAIL until redaction and integration are complete.

- [ ] **Step 4: Implement privacy-safe diagnostics and real integration**

  All tracing events use stable `rollshot::agent::*` targets and structured
  fields. Log IDs, provider/model identifiers, counts, durations, limit names,
  and error classes only.

  Run: `rtk cargo test -p rollshot-agent`

  Expected: PASS.

- [ ] **Step 5: Commit**

  ```bash
  rtk git add crates/rollshot-agent/src
  rtk git commit -m "test(agent): harden cancellation and privacy"
  ```

---

## Task 9: Final Workspace Verification and Handoff

**Files:**

- Create: `docs/superpowers/handoffs/2026-06-23-bounded-agent-core.md`

**Behavior:**

- All crate and workspace checks pass.
- The handoff records public API, provider fixture provenance, budget defaults,
  known limitations, and product-integration follow-ups.

- [ ] **Step 1: Run focused verification**

  Run:

  ```bash
  rtk cargo test -p rollshot-agent
  rtk cargo fmt --check
  rtk cargo clippy -p rollshot-agent --all-targets -- -D warnings
  ```

  Expected: all PASS.

- [ ] **Step 2: Run workspace regression verification**

  Run:

  ```bash
  rtk cargo test --workspace
  rtk cargo clippy --workspace --all-targets -- -D warnings
  ```

  Expected: all PASS.

- [ ] **Step 3: Audit the public and privacy boundary**

  Run:

  ```bash
  rtk rg -n "pub .*rig|pub use rig|rig_core" crates/rollshot-agent/src
  rtk rg -n "println!|eprintln!|dbg!" crates/rollshot-agent
  rtk rg -n "tracing::|trace!|debug!|info!|warn!|error!" crates/rollshot-agent/src
  ```

  Expected:

  - no public Rig exposure;
  - no print diagnostics;
  - every tracing call uses a stable `rollshot::agent::*` target and contains no
    sensitive payload.

- [ ] **Step 4: Write the handoff**

  Record:

  - implemented public API;
  - exact Rig version;
  - Anthropic/OpenAI fixture source and scrub procedure;
  - test commands and results;
  - default budget values;
  - unsupported inspection capabilities;
  - no persistence/resume guarantee;
  - product work needed to adapt prepared vision queries and show review/user
    input UI.

  Run: `rtk sed -n '1,240p' docs/superpowers/handoffs/2026-06-23-bounded-agent-core.md`

  Expected: the handoff contains every item above and no secrets.

- [ ] **Step 5: Commit**

  ```bash
  rtk git add docs/superpowers/handoffs/2026-06-23-bounded-agent-core.md
  rtk git commit -m "docs(agent): add bounded agent core handoff"
  ```

---

## Test Coverage

| Task / behavior | Unit | Integration | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| 1 / authorized input and in-memory session | ✓ | — | — | no |
| 2 / generations, budgets, cancellation, audit events | ✓ | — | — | no |
| 3 / strict typed tools and serial execution | ✓ | — | — | no |
| 3 / validation, proposal policy, fake dry-run | ✓ | — | — | no |
| 4 / Rig streamed turn and history/tool-result continuity | ✓ | — | — | no |
| 5 / full scripted author loop | ✓ | — | — | no |
| 5 / all terminal classes and budget dimensions | ✓ | — | — | no |
| 6 / Anthropic request and real-format SSE through Rig | — | ✓ | — | no |
| 7 / OpenAI Chat request and real-format SSE through Rig | — | ✓ | — | no |
| 8 / cancellation, deadlines, memory ceilings | ✓ | ✓ | — | no |
| 8 / tracing/debug/serialization privacy sentinels | ✓ | — | — | no |
| 8 / real QuickJS proposal handoff | ✓ | — | — | no |
| 9 / workspace regression | — | ✓ | — | no |
| Live provider credentials and account policy | — | — | deferred | yes |

All required tests are deterministic: no real network, environment API keys,
display server, OS capture, sleeps, or wall-clock dependence.

---

## Failure Modes

| Codepath | Realistic failure | Test | Handling | User-visible result |
|---|---|---|---|---|
| Authorized input | descriptor/byte mismatch | Task 1 Step 1 | fallible constructor | clear input error |
| Draft evidence | async result targets old generation | Task 2 Step 1 | `StaleGeneration` | recoverable tool result |
| Budget usage | cumulative usage re-reported | Task 2 Step 1 | per-turn delta charge | no double charge |
| Cancellation | cancel occurs between checks | Task 2 Step 1, Task 8 Step 1 | `CancellationToken` select | `Cancelled` |
| Tool decode | known tool receives invalid JSON | Task 3 Step 1 | typed recoverable result | model can correct |
| Tool dispatch | unknown tool name | Task 3 Step 1, Task 5 Step 3 | protocol terminal | `AgentProtocolFailure` |
| Inspection | vision capability not prepared | Task 3 Step 3 | typed unavailable result | model can choose another path |
| Validation | invalid automation source | Task 3 Step 3, Task 5 Step 3 | evidence not recorded | `SourceValidationFailure` when exhausted |
| Dry-run | QuickJS runtime/sandbox failure | Task 3 Step 3, Task 5 Step 3 | typed execution result | `RuntimeFailure` when exhausted |
| Proposal policy | too many candidates/area | Task 3 Step 3, Task 5 Step 3 | budget/policy rejection | clear bounded failure |
| Rig stream assembly | incomplete fragmented call | Task 4 Step 1 | invalid-call protocol path | `AgentProtocolFailure` |
| Conversation continuity | tool result omitted next turn | Task 4 Step 1 | prompt/history conversion | test fails before release |
| Provider HTTP | auth, rate limit, 5xx | Tasks 6/7 Step 1 | provider error mapping, no retry | `ProviderFailure` with class |
| Provider stream | malformed SSE or early EOF | Tasks 6/7 Step 1 | Rig error mapping | `AgentProtocolFailure` |
| Provider idle | stream never produces next item | Task 8 Step 1 | deadline/cancellation select | `BudgetExceeded` or `Cancelled` |
| Provider output | unbounded text/tool arguments | Task 8 Step 1 | hard byte limits | `BudgetExceeded` |
| Diagnostics | secret reaches trace/debug/session | Task 8 Step 3 | custom redaction and metadata-only events | test blocks release |
| Workspace integration | new crate breaks lint/build | Task 9 Steps 1–2 | workspace verification | build failure, not silent |

Critical gaps: **0 after this plan is implemented.** Live provider-account
behavior remains explicitly manual/deferred and is not a silent production
path.

---

## Performance and Resource Requirements

- Provider bodies must remain streamed; no `bytes().await` or full-response
  buffer.
- Provider clients are constructed once per configured adapter and reused.
- Every stream wait is bounded by cancellation and the run deadline.
- Assistant text, tool-call arguments, tool results, source, attachments, and
  retained session text have explicit byte ceilings.
- JSON results over the limit are rejected whole; they are never truncated into
  invalid JSON.
- Tool calls execute serially, bounding concurrent executor/host work to one.
- Budget accounting uses checked arithmetic and commits charges only after the
  limit check succeeds.
- Retained audit events do not copy provider text or source.
- Fixture tests use chunk barriers, not sleeps, and finish under 30 seconds.
- No stitching/capture hot path is changed; core benchmarks are not required.

---

## Completion Criteria

The plan is complete only when:

1. A scripted model completes the author loop and returns `ReadyForReview`.
2. The next model turn demonstrably contains the prior tool call and result.
3. Anthropic and OpenAI Chat Completions mock-server fixtures stream text before
   response completion and assemble fragmented tool calls through Rig.
4. Every budget dimension, cancellation timing, terminal class, stale
   generation, and privacy sentinel has a deterministic test.
5. Real `QuickJsExecutor` produces a policy-valid proposal without mutating
   `ImageDocument`.
6. No public BAC API exposes Rig or provider SDK types.
7. Focused and workspace tests, fmt, and clippy all pass.
8. The handoff records limitations and next product integration work.
