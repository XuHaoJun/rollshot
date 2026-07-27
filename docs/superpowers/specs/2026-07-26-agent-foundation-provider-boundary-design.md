# Rollshot Agent Foundation: Provider Boundary Reliability Design

**Date:** 2026-07-26  
**Status:** Approved child design  
**Umbrella:**
[`2026-07-26-agent-foundation-umbrella-design.md`](2026-07-26-agent-foundation-umbrella-design.md)  
**Research source:**
[`docs/researchs/agent-foundation/`](../../researchs/agent-foundation/)  
**Slice:** 1 of 6 — Phase 0, Boundary Evidence

## 1. Purpose

This slice makes Rollshot's provider boundary reliably cancelable, bounded by
the run wall-time budget, and honest about partial or interrupted streams. It
also determines whether the workspace can safely migrate from the pinned
`rig-core` 0.39 to 0.40 without expanding Rollshot's provider-neutral public
boundary.

The slice combines two deliberately separated activities:

1. a retained, isolated spike that produces reproducible evidence; and
2. a production TDD change that applies only the minimum behavior supported by
   that evidence.

Rig 0.40 is an allowed conditional migration, not a mandatory outcome. Provider
reliability is the required outcome.

## 2. Current baseline

At the design baseline:

- `rollshot-agent` pins `rig-core = "=0.39.0"` with `test-utils`;
- production Rig references are concentrated in `driver.rs`, `model.rs`, and
  `provider.rs` behind Rollshot-owned model and provider contracts;
- `ProviderAdapter::stream` accepts `StreamBounds` and returns a future that
  resolves to a stream;
- Anthropic and OpenAI adapters await Rig stream establishment before creating
  the bounded event stream;
- `stream_to_model_events` checks cancellation before polling and selects a
  deadline while polling, but cancellation cannot wake an already-blocked
  `stream.next()`;
- `AgentRunner::drive_streamed_turn` awaits both stream establishment and stream
  items without its own cancel/deadline select;
- a custom adapter can therefore ignore `StreamBounds` and stall the run;
- the Action Guide caption path independently wraps establishment and stream
  consumption in `timeout_at`, so hard-bound ownership is inconsistent across
  callers; and
- the existing provider contract suite passes 34 tests but does not cover an
  establishment future that never resolves or cancellation while an item poll
  remains pending.

`stream_to_model_events` currently synthesizes `Completed` when a Rig stream
ends without one. This is needed for at least one observed normal Anthropic
fixture path, but an unqualified EOF can also represent truncation. The spike
must establish whether normal completion and interrupted EOF can be
distinguished before changing this behavior.

## 3. Decisions this slice must support

The slice answers four questions:

1. Can Rollshot terminate provider establishment and stalled stream polling
   without trusting adapter cooperation?
2. Can Rollshot distinguish valid completion from partial or truncated stream
   termination well enough to prevent partial output from becoming a successful
   turn?
3. Can Rig 0.40 preserve the state-machine, stream-assembly, tool-threading, and
   provider-contract behavior Rollshot currently consumes?
4. If all migration-specific evidence passes, should the workspace upgrade to
   Rig 0.40 now?

The decision outcomes are:

- **reliability fix + Rig 0.40:** mandatory reliability gates pass and the 0.40
  migration gate passes;
- **reliability fix + retain Rig 0.39:** reliability gates pass but the 0.40
  migration gate fails or reveals disproportionate migration scope; or
- **stop and redesign:** a mandatory reliability gate cannot pass without a
  Rig fork, transport rewrite, provider-specific public state, or another
  umbrella-level boundary change.

## 4. Scope

### 4.1 Included

- deterministic provider-establishment stall tests;
- deterministic established-stream stall tests;
- cancel, deadline, provider-error, EOF, and race classification;
- partial text and partial tool-argument integrity;
- local Anthropic and OpenAI protocol fixtures;
- comparison of Rig 0.39 and 0.40 normalized stream endings;
- host-owned hard bounds in the bounded agent runner;
- minimum cooperative adapter cleanup improvements supported by evidence;
- conditional `rig-core` 0.40 migration;
- full `rollshot-agent` regression verification;
- retained spike findings; and
- a Gate G0 decision record.

### 4.2 Excluded

- restructuring the Action Guide caption path;
- removing or redesigning the public `ProviderAdapter` trait;
- a new generalized provider wrapper;
- retries, exponential backoff, provider fallback, or provider handoff;
- provider cost accounting;
- live Anthropic or OpenAI calls;
- a hand-written HTTP transport;
- a Rig fork or patch maintained by Rollshot;
- skills, Product Task, artifact-promotion, job, context, or audit work from
  later umbrella slices; and
- forcing the Rig 0.40 migration after its gate fails.

## 5. Ownership model

### 5.1 Host ownership

Rollshot's bounded caller owns correctness for:

- cancellation;
- wall-time budget enforcement;
- terminal classification;
- partial-result discard;
- commit of a valid provider turn into Rig and product state; and
- the decision to execute tools.

A provider adapter is not trusted to enforce these policies correctly. The
bounded caller must remain safe if a fake or custom adapter ignores
`StreamBounds` completely.

### 5.2 Adapter ownership

A concrete adapter owns:

- provider request construction;
- provider transport and Rig client integration;
- conversion from provider/Rig errors to Rollshot `ModelError`;
- normalization of provider stream items into `ModelStreamEvent`; and
- cooperative transport cancellation and cleanup where available.

Adapter cooperation may reduce resource lifetime, but it is not the sole source
of hard-bound correctness.

### 5.3 Product and Rig ownership

- The product owns the run budget and cancellation request.
- `AgentRunner` owns one bounded run and its terminal.
- Rig remains a private state-machine and provider implementation detail.
- Rig does not own Rollshot terminal taxonomy, budget semantics, cancellation
  policy, or product review state.

## 6. Control architecture

`AgentRunner::drive_streamed_turn` enforces two host-owned control points.

### 6.1 Stream establishment

```text
provider.stream(request, bounds)
├── cancellation ready → DriverError::Cancelled
├── deadline ready     → BudgetExhausted(WallTime)
└── provider resolves  → stream or ProviderFailure
```

A pending provider-establishment future must be dropped when cancellation or
wall-time wins.

### 6.2 Stream polling

```text
stream.next()
├── cancellation ready → DriverError::Cancelled
├── deadline ready     → BudgetExhausted(WallTime)
└── item resolves      → process event, error, completion, or end
```

Every new item poll is bounded. No adapter can keep the runner blocked after a
control signal becomes observable.

### 6.3 Deterministic ordering

At each control point the runner:

1. synchronously checks cancellation;
2. checks the wall-time budget at the current Tokio instant;
3. enters a select over cancellation, deadline, and provider progress;
4. uses the signal that becomes ready first; and
5. treats cancellation as the deterministic tie-break if cancellation and
   deadline are ready on the same poll.

Provider progress cannot override an already-ready cancellation or deadline on
the same poll.

### 6.4 Existing trait boundary

`ProviderAdapter::stream` continues to accept `StreamBounds` in this slice.
This avoids combining a reliability change with a public trait migration.
Concrete adapters may use the same signals for cooperative cleanup, but the
outer host guard remains authoritative.

If the spike proves that retaining `StreamBounds` prevents honest
classification or creates an unavoidable double-race, the slice stops and
records a trigger for a separate transport-only adapter design. It does not
silently introduce that redesign.

## 7. Terminal classification

The bounded runner classifies terminal causes as follows:

| Cause observed first | Terminal |
|---|---|
| User or product cancellation | `RunTerminalState::Cancelled` |
| Run wall-time deadline | `RunTerminalState::BudgetExhausted { dimension: WallTime }` |
| Provider HTTP, protocol, or stream error | `RunTerminalState::ProviderFailure` |
| Stream ends without proven valid completion | `RunTerminalState::ProviderFailure` |
| Valid provider completion | Existing success/tool path |

Provider-native error variants remain private. Error strings continue through
existing privacy-safe sanitization and do not contain credentials, request
bodies, image bytes, or full provider responses.

Cancellation is not represented as a provider failure. A wall-time deadline is
a Rollshot budget terminal, not a provider timeout terminal.

## 8. Partial-result commit boundary

Provider output is run-local candidate state until valid completion is proven.

Before that boundary:

- text chunks may be emitted as transient display events;
- assistant text remains in a turn-local buffer;
- tool names and argument deltas remain in turn-local buffers;
- no tool executes;
- no Rig streamed turn is recorded;
- no authoritative assistant exchange is committed;
- no review artifact can be created; and
- no partial output is treated as a successful turn.

On cancellation, deadline, provider error, invalid EOF, or malformed completion:

- local text and tool buffers are discarded;
- Rig and product state remain at the last valid boundary;
- transient UI state is reconciled from the typed terminal; and
- no implicit retry occurs.

Usage reported before failure may be incomplete. This slice does not invent a
cost or token receipt that the provider did not supply. Potential external cost
for a cancelled request is recorded as a residual risk.

## 9. Completion integrity

### 9.1 Required experiment

The spike observes Rig 0.39 and 0.40 behavior for:

1. Anthropic normal message stop;
2. OpenAI normal finish and `[DONE]`;
3. partial text followed by EOF;
4. partial tool JSON followed by EOF;
5. provider error after partial text;
6. provider error after partial tool arguments; and
7. a valid completion followed by a transport that remains open.

For every case, findings record the raw fixture sequence, normalized Rig items,
Rollshot events, and terminal classification.

### 9.2 Completion receipt rule

A successful turn requires positive protocol evidence of completion. Bare
`Stream::None` is not itself positive evidence.

A synthetic Rollshot `Completed` may be emitted only when the adapter can prove
that a provider-specific normal completion receipt was already observed but Rig
did not expose a final normalized item. The proof must be deterministic and
covered by provider contract tests.

If neither Rig 0.39 nor 0.40 exposes enough information to distinguish normal
end from truncation without patching Rig or rewriting the transport, mandatory
integrity Gate H2 fails. The slice stops and records a fork/vendor or boundary
redesign trigger.

### 9.3 Valid completion behavior

Once a valid completion receipt and all required tool-call data have been
assembled, the bounded caller need not wait for an unrelated later EOF before
committing the turn. The exact provider-normalization point must be selected by
the spike evidence, not assumed from the current synthetic path.

## 10. Spike design

### 10.1 Location and isolation

The retained spike lives at:

```text
spikes/provider-boundary/
├── Cargo.toml
├── src/
├── fixtures/
└── FINDINGS.md
```

The Rust spike is standalone and is not added to the root workspace. Production
crates do not import or depend on it. Temporary production instrumentation, if
strictly required, must be recorded and reverted before a spike milestone is
committed.

### 10.2 Environments

Required evidence is fully local and reproducible:

- deterministic fake `ProviderAdapter` implementations;
- local Wiremock delayed-response and SSE fixtures;
- Tokio paused-time tests;
- the exact production Rig 0.39 dependency;
- the local Rig 0.40 reference revision or exact published 0.40 crate; and
- the current project Rust toolchain.

No provider API key, external request, platform-specific hardware, or manual UI
observation is required.

### 10.3 Evidence levels and results

Every milestone in `FINDINGS.md` records:

- exact environment and command;
- evidence level: `compile`, `automated`, `runtime`, or `hardware`;
- result: `PASS`, `FAIL`, `MITIGATED`, or `UNTESTED`;
- observation and artifact path;
- limitations; and
- decision consequence.

Hardware evidence is expected to be `UNTESTED` because the slice has no
hardware claim.

### 10.4 Retention

After Gate G0 consumes the result, the spike becomes a retained historical
reference. It is not kept synchronized with production code and is never
imported by the workspace.

## 11. Risk gates and stopping rules

### 11.1 H1 — Control gate

A fake adapter that ignores `StreamBounds` must still terminate under:

- cancellation during establishment;
- deadline during establishment;
- cancellation during a pending stream-item poll; and
- deadline during a pending stream-item poll.

Tests use barriers or channels to prove that the future reached the intended
pending state before firing the control signal.

H1 is mandatory for Gate G0.

### 11.2 H2 — Integrity gate

Partial text or partial tool arguments followed by cancellation, deadline,
provider error, or invalid EOF must not produce:

- a successful Rig turn;
- a completed tool call;
- tool execution;
- an authoritative assistant exchange; or
- a review artifact.

Normal Anthropic and OpenAI fixtures must still complete once.

H2 is mandatory for Gate G0.

### 11.3 H3 — Rig 0.40 migration gate

Rig 0.40 must:

- compile against Rollshot's consumed surface;
- preserve state-machine and tool-result threading contracts;
- preserve or improve completion evidence;
- pass all existing and new provider/driver tests;
- preserve private translation boundaries; and
- avoid a material new security or maintenance burden.

H3 is mandatory for the 0.40 upgrade, but not for retaining 0.39 after H1 and
H2 pass. If H3 fails, the decision record explains why 0.39 remains pinned.

### 11.4 Hard stopping conditions

Stop the migration or the whole slice, as applicable, when passing a gate would
require:

- patching or forking Rig internals;
- writing a replacement Anthropic or OpenAI transport;
- exposing provider-specific public terminal or message types;
- changing the umbrella's provider-neutral ownership boundary;
- using nondeterministic live-provider evidence as the acceptance basis; or
- expanding into retries, provider fallback, cost accounting, or later slices.

H1 or H2 failure stops Gate G0 and triggers a new boundary/fork design. H3
failure stops only the 0.40 migration when H1 and H2 are already satisfied on
0.39.

## 12. Failure matrix

| Scenario | Expected terminal | Events/state assertion |
|---|---|---|
| Establishment pending; cancel first | `Cancelled` | No provider event, assistant turn, tool, or artifact commit |
| Establishment pending; deadline first | `BudgetExhausted(WallTime)` | No provider event, assistant turn, tool, or artifact commit |
| Partial text; poll pending; cancel | `Cancelled` | Transient text allowed; no assistant or Rig turn commit |
| Partial tool JSON; poll pending; deadline | `BudgetExhausted(WallTime)` | No complete call and no tool execution |
| Provider error before output | `ProviderFailure` | No assistant turn, tool, or artifact commit |
| Provider error after partial output | `ProviderFailure` | Partial buffers discarded |
| EOF without valid completion | `ProviderFailure` | No assistant turn, tool, or artifact commit |
| Valid completion receipt | Existing success path | Exactly one turn commit |
| Cancel ready before deadline | `Cancelled` | No partial assistant/tool/artifact commit |
| Deadline ready before cancel | `BudgetExhausted(WallTime)` | No partial assistant/tool/artifact commit |
| Cancel and deadline ready on same poll | `Cancelled` | Deterministic tie-break; no partial assistant/tool/artifact commit |

## 13. Deterministic test strategy

Tests must avoid timing guesses:

- barriers or channels establish that the tested future is pending;
- paused Tokio time advances deadlines deterministically;
- cancellation is triggered explicitly after the pending-state barrier;
- a real-time timeout is only a deadlock watchdog and not performance evidence;
- race tests control signal ordering rather than relying on the OS scheduler;
- spies assert tool invocation count;
- session assertions allow the already-recorded user input but prove no partial
  assistant exchange; Rig-state assertions prove no completed partial turn; and
- event assertions distinguish transient chunks from authoritative completion.

The implementation plan must include red-green evidence for the new production
regression tests.

## 14. Production implementation sequence

The future implementation plan preserves this ordering:

1. record baseline environment and existing test result;
2. create RED spike fixtures for blocked establishment and polling;
3. run completion-integrity probes on Rig 0.39 and 0.40;
4. make the H1/H2 checkpoint decision;
5. translate minimum reproductions into production contract tests;
6. confirm the production tests fail on the current implementation;
7. add the minimum outer host guards and completion validation on Rig 0.39;
8. verify the reliability behavior on 0.39;
9. attempt the conditional Rig 0.40 migration;
10. make only required API adaptations;
11. run targeted and full verification;
12. obtain independent code review; and
13. write the Gate G0 decision record.

The implementation plan may split these into smaller TDD tasks but may not
reorder migration before H1/H2 evidence.

## 15. Conditional Rig 0.40 migration

If H3 passes, the production migration:

- updates `crates/rollshot-agent/Cargo.toml`;
- updates `Cargo.lock` through Cargo;
- adapts only the consumed Rig API surface;
- retains Rollshot's public `ModelRequest`, `ModelMessage`,
  `ModelStreamEvent`, `ModelError`, `ProviderAdapter`, and terminal contracts;
- leaves product budget, cancellation, tools, and review ownership in Rollshot;
  and
- removes the resolved 0.39 dependency from the workspace graph.

The upgrade is not permission to refactor adjacent agent code. Every source
change must trace to reliability or required 0.40 compatibility.

If H3 fails while H1 and H2 pass on 0.39, the slice retains the 0.39 pin and
records:

- the breaking surface;
- failed tests or security concern;
- rejected adaptation;
- conditions for retrying the upgrade; and
- whether fork/vendor deserves a later design.

## 16. Verification

### 16.1 Targeted tests

The slice must add executable coverage for:

- establishment cancellation;
- establishment deadline;
- pending-item cancellation;
- pending-item deadline;
- provider error before and after partial output;
- EOF without valid completion;
- partial tool arguments;
- cancel-before-deadline;
- deadline-before-cancel;
- same-poll tie;
- valid Anthropic completion; and
- valid OpenAI completion.

### 16.2 Required commands

Before Gate G0:

- `rtk cargo test -p rollshot-agent --test provider_contract`;
- `rtk cargo test -p rollshot-agent`;
- `rtk cargo fmt --check`;
- `rtk cargo clippy --workspace --all-targets -- -D warnings`;
- `rtk cargo tree -p rollshot-agent -i rig-core`; and
- a bounded search confirming production Rig references remain inside the
  private translation surface.

If the full workspace clippy exposes a pre-existing unrelated failure, the
failure must be recorded with exact evidence. The slice may not claim a clean
Gate G0 without resolving or explicitly obtaining approval for that residual
risk.

### 16.3 Independent review

An independent reviewer verifies:

- outer guards cannot be bypassed by an adapter;
- control-signal precedence matches the terminal matrix;
- no partial output crosses the commit boundary;
- completion evidence is protocol-backed;
- Rig 0.40 changes, if present, are minimal; and
- privacy-safe error behavior is retained.

## 17. Acceptance criteria

### 17.1 Reliability acceptance

- H1 and H2 pass with deterministic evidence.
- Cancellation and deadline wake both establishment and item polling.
- The runner returns the terminal named in the failure matrix.
- Partial text and tool arguments do not mutate authoritative state.
- A tool executes only after complete arguments and valid completion.
- Valid Anthropic and OpenAI fixtures remain successful.
- Provider errors remain sanitized and provider-neutral.

### 17.2 Rig migration acceptance

If Rig 0.40 is adopted:

- H3 passes;
- `cargo tree` resolves 0.40 for `rollshot-agent` and no longer resolves 0.39;
- the original 34 provider contracts plus all new contracts pass;
- the complete `rollshot-agent` test suite passes;
- formatting and clippy verification pass;
- no provider-specific type enters Rollshot's public model/provider/terminal
  contracts; and
- production Rig usage remains inside the private translation boundary.

If Rig 0.39 is retained, the decision record must show H1/H2 passing and the
specific H3 evidence that rejected the upgrade.

## 18. Residual risks

Gate G0 does not claim to eliminate:

- external provider cost already incurred before cancellation;
- real-provider infrastructure latency or outage behavior;
- lower-level socket cleanup guarantees not observable through Rig's public
  interface;
- exact external billing or final usage for interrupted streams; or
- future provider protocol drift after the pinned fixtures and dependency
  revisions.

These are documented risks, not reasons to weaken deterministic local
correctness.

## 19. Outputs

The slice produces:

1. this child design spec;
2. an approved implementation plan at
   `docs/superpowers/plans/YYYY-MM-DD-agent-foundation-provider-boundary.md`;
3. retained evidence at `spikes/provider-boundary/FINDINGS.md`;
4. production tests and the minimum reliability fix;
5. a conditional Rig 0.40 dependency migration;
6. an independent review result; and
7. a Gate G0 decision record at
   `docs/superpowers/spikes/YYYY-MM-DD-provider-boundary-decision.md`.

`YYYY-MM-DD` is the actual creation date of the future plan or decision record.

## 20. Gate G0

Gate G0 passes when:

- H1 and H2 pass;
- H3 either passes with a completed 0.40 migration or fails with an explicit
  retain-0.39 decision;
- all required verification evidence is recorded;
- the independent review is resolved;
- spike findings include rejected alternatives and residual risks;
- no excluded scope entered the implementation; and
- the user approves the Gate G0 decision.

Only then may the umbrella proceed to Slice 2, Product Task and Artifact
Promotion.
