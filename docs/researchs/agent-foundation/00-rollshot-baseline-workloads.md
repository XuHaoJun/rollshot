# Rollshot agent foundation: Round 0 baseline and workloads

**Research date:** 2026-07-22 (Asia/Taipei)
**Status:** In Progress
**Umbrella revision:** 1
**Research round:** 0
**Systems/capabilities:** Rollshot bounded agent; Rig boundary; Smart Redaction,
Action Guide, and brag/Hyperframes workload requirements
**Evidence baseline:** Rollshot `42afd1fcdfa58e9b76912a02e140ab820d233f9d`;
Rig `2f37dfcd0156bdceab3eabe6f0a953f9202e2d77`; brag
`357a805e76a93a528ac6cccac28c8da3e893272b`; Hyperframes
`807078c7cde9d5c8403588722d1cd9397c513a0d`
**Evidence mode:** Static source and test inspection only; no provider, UI, or
long-running workflow was exercised for this round.

This document establishes the current-state vocabulary and workload ladder for
later agent-foundation research. It describes what exists and what the cited
workloads require; it does not select a future architecture.

## Reproducibility

| Source | Inspected revision |
|---|---|
| Rollshot | `42afd1fcdfa58e9b76912a02e140ab820d233f9d` |
| Rig reference checkout | `2f37dfcd0156bdceab3eabe6f0a953f9202e2d77` (`v0.40.0`) |
| brag | `357a805e76a93a528ac6cccac28c8da3e893272b` |
| Hyperframes | `807078c7cde9d5c8403588722d1cd9397c513a0d` |

Rollshot actually pins `rig-core = "=0.39.0"`; `Cargo.lock` resolves that version
with checksum
`80a4bc7a93b329c4e1a66d5fd211d79990e7331e3c701f057c29f135f548686d`.
Therefore the exact consumed API was checked against the locally resolved
`rig-core-0.39.0` source as well as the named Rig reference checkout. The
checkout is useful evidence of the upstream boundary but must not silently
stand in for the pinned version when their semantics differ. [R7, G1, G2]

Code is the source of truth for Rollshot. Historical plans/specs are intent
snapshots and other research documents are forward-looking; neither overrides
the inspected implementation. [R1]

## Terminology

These terms stay distinct in later documents:

| Term | Round 0 meaning |
|---|---|
| **Conversation** | Provider-visible ordered model messages. Within one current Rollshot run, Rig owns and threads this history. It is not the same thing as `AgentSession`. |
| **Session** | Rollshot's `AgentSession`: a `SessionId`, completed user/assistant text pairs, and at most one pending user message. It is an in-memory product/domain record, not a persisted Rig run. [R2] |
| **Run** | One invocation of `AgentRunner`, with a fresh Rig `AgentRun`, one budget tracker, one cancellation source, one tool context, and one terminal outcome. [R3, R5] |
| **Turn** | One model call plus any resulting tool batch and results inside a run. Rig supplies the one-based model-call index; Rollshot accounts usage and streams selected events. [R3, G1] |
| **Task** | A bounded unit the product asks an agent/model to perform, such as Smart Redaction or one Action Guide visual-annotation suggestion. It is not currently a durable `rollshot-agent` record. |
| **Workflow** | Multiple dependency-related stages, checkpoints, jobs, or tasks progressing toward artifacts. Neither an `AgentSession` nor a Rig `AgentRun` is by itself a workflow record. |
| **Tool call** | One model-requested invocation, correlated with a result by call ID. Rig validates and threads call/result messages; Rollshot registers and executes tools. [R5, G1] |
| **External job/process** | Work whose lifecycle can outlast the model turn that started it, such as a preview server, audio generation, or render. No such abstraction was found in the investigated `rollshot-agent` files. Hyperframes supplies the workload evidence for it. [H1, H2] |
| **Artifact** | A named output whose existence/content is a completion contract: a review proposal, persisted Action Guide project, storyboard frame, render, or share copy. Artifacts are not conversation memory. |
| **Checkpoint** | A user decision that gates later work. `NeedsUserInput` ends a current Rollshot run; Hyperframes checkpoints instead gate a longer workflow. [R3, H3] |
| **Resume** | Reconstruct enough durable state to continue after a process/session boundary. Resuming a conversation, a serialized Rig run, and an artifact-driven workflow are three non-equivalent operations. |

## Current architecture

### Ownership and product integration

Smart Redaction is an active iced workbench path. The app owns provider
configuration, the finite `RunBudget`, `AgentSession`, consent-selected payload
mode, prepared vision context, tool registry/context, cancellation handle, and
the spawned task. It constructs `AuthorizedModelInput`, starts
`AgentRunner::run_with_provider`, translates `RunEvent`s into live activity,
and turns `ReadyForReview` into a pending proposal and draft for user review.
[R8]

The bounded core owns one run, not the surrounding workspace:

```text
iced workbench
  owns input consent, provider config, session value, budget, cancellation,
       prepared vision, review UI, presets and resulting proposal
          |
          v
AgentRunner (one invocation)
  fresh Rig AgentRun + BudgetTracker + ToolContext/ToolRegistry
          |
          +-- CallModel --> Rollshot ModelRequest --> ProviderAdapter
          |                                      --> Rig provider client/stream
          |
          +-- CallTools --> Rollshot registry, serial response order
          |               --> draft generation/evidence/proposal
          |
          `-- typed Rollshot terminal --> workbench review state
```

`AuthorizedModelInput` validates attachment count, byte count, dimensions,
media types, and aggregate size before a run. Its debug representation redacts
user text and bytes. [R2]

### Session and run boundaries

`AgentSession` stores completed text exchanges in a `Vec` and has no
serialization or storage implementation in `domain.rs`. `run_with_provider`
appends the current user message, but constructs a fresh Rig `AgentRun` from
that message without calling Rig's `with_history`; previous
`AgentSession::exchanges()` are not projected into provider history. [R2, R3]

In the inspected workbench path, the session is moved by value into the spawned
run after the workspace replaces it with a new empty session. The task does not
return the mutated session in its terminal message. Consequently, static
inspection shows no cross-run transcript continuity in this path, despite the
state comment describing an in-memory session between runs. This is a bounded
source conclusion, not a runtime observation. [R8]

Within a run, Rig owns conversation threading: assistant tool calls and the
corresponding Rollshot tool results become the next request's history. The
Rollshot-owned `ModelMessage` representation preserves user text, assistant
text, assistant tool calls, and tool results across those turns. [R3, R4]

### Model and provider boundary

The public model facade is Rollshot-owned: `ModelRequest`, `ModelMessage`,
`ToolDefinition`, `ModelStreamEvent`, `ModelUsage`, `ModelCompletion`,
`StopReason`, `ModelError`, `ProviderAdapter`, and `StreamBounds` expose no Rig
types. Concrete Anthropic and OpenAI adapters are re-exported, but their Rig
client fields remain private. [R4, R6]

Internally, the concrete adapters are not hand-written HTTP transports. They
use Rig's Anthropic/OpenAI clients, `CompletionRequest`, completion-model
streaming, message/content types, completion errors, and streaming response.
OpenAI explicitly requests `parallel_tool_calls: false`. Both adapters translate
back to Rollshot `ModelStreamEvent`s and enforce cancellation/deadline bounds.
[R6]

### Tools and serial execution

Rollshot owns the `Tool` trait and `ToolRegistry`. A registry holds concrete
tools, produces provider-neutral JSON-schema definitions, enforces argument,
result, and per-tool call limits, and executes a returned batch serially in
model response order. A hard error or the first successful terminal tool stops
the remainder of the batch. [R5]

Serial execution is a Rollshot policy, not a Rig requirement. Rig surfaces a
batch in emission order and explicitly leaves concurrency to the driver; its
`tool_results` accepts results in any order while requiring every pending call
to be answered exactly once. [R5, G1]

Smart Redaction's `ToolContext` holds the mutable automation source, draft
generation/evidence, validation and dry-run caches, proposal handoff, image
dimensions, capability handles, and the automation cancellation flag. This is
all run-local memory behind mutexes. Source replacement/editing advances a
generation and invalidates stale evidence/caches; submission succeeds only for
the current generation after validation and dry-run evidence. [R5]

### Budgets, cancellation, events, and terminals

`RunBudget`/`UsageSnapshot` define 16 dimensions: wall time, model calls, input
tokens, output tokens, cost, tool calls, per-tool calls, argument bytes, result
bytes, source bytes, attachments, validation attempts, dry-run attempts,
capability calls, candidate count, and affected area. The workbench supplies a
finite Smart Redaction budget. Cost has a field and enforcement comparison,
but the driver currently never charges provider prices, so its documented
value stays zero. [R5, R8]

`RunCancellation` fans one `cancel()` call into a Tokio cancellation token and
the automation executor's cancellation flag. The driver checks it before and
between model/tool work, providers bound stream polling by the same source and
a deadline, and dry-run execution receives the paired automation flag. [R5,
R6]

The declared `RunEvent` vocabulary is text chunk, tool start/end, source
change, and turn complete. The inspected production driver emits the first
four; `TurnComplete` emission was found only in `runtime.rs` test code. The
workbench uses a bounded channel with `try_send`, so transient events may be
dropped; terminal assistant text is treated as authoritative and reconciles
the UI. No event log or reconnect reconstruction was found in the investigated
path. [R3, R5, R8]

Smart Redaction returns explicit `RunTerminalState` variants:
`ReadyForReview`, `NeedsUserInput`, `Cancelled`, dimensioned
`BudgetExhausted`, `SourceValidationFailure`, `RuntimeFailure`,
`AgentProtocolFailure`, and `ProviderFailure`. Successful submission ends the
run immediately; it does not execute another model turn. [R3]

### Persistence and concurrency

The current `rollshot-agent` run state, budget tracker, tool context, registry
counters, cancellation, and events are in memory. Although pinned Rig 0.39's
`AgentRun` derives `Serialize`/`Deserialize`, Rollshot neither serializes it nor
exposes it in a Rollshot persistence contract. The Rig source also warns that
serialized state contains the conversation and has no cross-version stability
guarantee. [G1]

One `AgentRunner` invocation advances one Rig state machine and awaits one
serial tool batch at a time. No `rollshot-agent` task graph, child-agent
registry, worker scheduler, background-job handle, checkpoint store, or resume
router was found in `domain.rs`, `driver.rs`, `model.rs`, `provider.rs`,
`runtime.rs`, or `tools.rs`. This is a bounded absence in the investigated
scope, not proof about all Rollshot code or possible external orchestration.

Action Guide separately has durable product artifacts: its project manifest
stores revisions, frames, ordered steps, captions, annotations, capability and
output settings; `save_project[_as]` and `load_project` provide filesystem
persistence. Those product records should not be mislabeled as durable agent
run state. [A1, A2]

## Rig boundary

### Exact consumed surface

At the pinned 0.39 boundary, Rollshot consumes four classes of Rig API:

1. **Prompt-loop state machine:** `AgentRun`, `AgentRunStep`, and
   `PendingToolCall`; `next_step`, `turn`, `record_streamed_completion_call`,
   `streamed_turn`, and `tool_results`.
2. **Turn/message assembly:** `StreamedTurnAssembler`, `StreamedTurn`,
   `StreamedTurnEvent`, `StreamedAssistantContent`, `AssistantContent`,
   `ToolCall`, `ToolFunction`, `UserContent`, `ToolResultContent`, `Message`,
   and `OneOrMany`.
3. **Usage and provider machinery:** `Usage`, `GetTokenUsage`,
   `CompletionRequest`, `CompletionClient`, `CompletionModel`,
   `StreamingCompletionResponse`, `CompletionError`, and the concrete
   Anthropic/OpenAI clients.
4. **Test-only harness:** `MockResponse` and scripted Rig stream items.

Direct `rig_core` references in production are confined to `driver.rs`,
`model.rs`, and `provider.rs`; `Cargo.toml` enables Rig's `test-utils` feature
for the crate. No Rig type appears in the inspected public method/trait fields
listed under the Rollshot model/provider facade. [R3, R4, R6, R7]

### Invariants Rollshot currently delegates to Rig

- An exhaustive `CallModel` / `CallTools` / `Done` driving protocol, including
  rejecting out-of-order calls.
- Turn counting and maximum-depth protocol behavior.
- Accumulation of assistant messages, tool calls, and correlated tool-result
  messages into the next request history.
- Validation that streamed tool names are in the advertised/allowed set and
  assembly of argument deltas into a complete turn.
- Exactly-once streamed completion-call accounting inside the Rig machine.
- Requiring a non-empty, complete set of results for every pending tool call
  before the next model request.

Rollshot does **not** delegate its product budget, cancellation, authorization,
tool implementations, serial scheduling policy, draft generations, validation,
review proposal, terminal taxonomy, UI events, or persistence policy to Rig.
[R3, R5, G1]

### Available outcomes, without selection

Rig is not a constraint, upstream compatibility is not a goal, and this round
makes no choice:

| Outcome | Boundary meaning | Surface Rollshot would own or continue to depend on |
|---|---|---|
| **Retain** | Keep the pinned external crate behind the current private translation boundary. | Continue depending on the state machine, streaming assembler, provider clients/messages, and their transitive updates while preserving Rollshot public contracts. |
| **Fork/vendor** | Copy or fork the consumed Rig code and evolve it for Rollshot without compatibility reluctance. | Own security fixes, provider protocol changes, serialization/privacy behavior, tests, and maintenance for the selected state-machine, stream, message, and/or provider portions. |
| **Replace** | Substitute another library or independently designed component under Rollshot's facade. | Re-prove every delegated invariant above and adapt or replace the concrete provider transports without leaking the replacement into product contracts. |
| **Remove** | Eliminate Rig and implement only the Rollshot-specific loop/provider behavior still required, or narrow workloads so a general state machine is unnecessary. | Own a smaller bespoke protocol, tool-result/history threading, stream assembly, provider transport, and adversarial tests; delete unused general Rig capabilities rather than recreating them speculatively. |

The later comparison must quantify code, test, security, and maintenance cost
for these surfaces. Avoidance of upstream divergence is not a scoring factor.

## Workload profiles

The ladder describes pressure on the foundation, not a promise that one engine
must execute all three workloads.

### Pressure 1: Smart Redaction — bounded review-producing run

**Observed shape.** The current product author/improve flow authorizes at most
the chosen screenshot payload, exposes a finite inspection/authoring tool set,
iterates source generation → validation → dry run, and terminates with a typed
proposal for user review or an actionable failure/clarification state. The
workbench owns consent, finite budget, cancellation, review, and preset/source
handoff. [R3, R5, R8]

**Capabilities demonstrated as necessary:**

- provider-neutral streaming with tool-call/result continuity;
- typed registered tools and explicit per-run availability through which tools
  are actually registered/advertised;
- deterministic serial calls and terminal-tool stop semantics;
- finite multidimensional budgets and cancellation into automation execution;
- generation-bound validation/dry-run evidence;
- a typed review artifact and terminal failure taxonomy; and
- privacy-safe input/debug/event handling plus explicit upload consent.

**Not established by this workload:** durable run resume, subagents, parallel
tool execution, task DAGs, or managed background jobs. Smart Redaction is the
baseline, not evidence that Rollshot needs a general workflow platform.

### Pressure 2: Action Guide — durable editable project around bounded tasks

**Observed shape.** A guide is an ordered editable list of reviewed steps with
titles, captions, keyframes, nearby replacement frames, and source candidate
IDs. The project manifest adds a revision, content-addressed frame metadata,
persisted annotations/explanations, capture/input provenance, enabled outputs,
and import warnings. Transactional save/load makes that state durable. [A1,
A2]

Current agent-adjacent work is heterogeneous rather than one long agent run:
the app invokes a fresh bounded visual-annotation agent for one reviewed step,
carrying `run_id`, `document_state_id`, source/keyframe context, image, a fresh
cancellation source, and a two-turn configuration. Caption suggestions instead
call the Rollshot provider facade directly for the reviewed guide and lower the
response into a typed `CaptionProposal`. [A3]

**Capabilities demonstrated as necessary if foundation orchestration owns
these tasks:**

- stable references from task output to project revision/document state,
  guide step source, and keyframe artifact;
- typed proposals that remain reviewable before mutating the durable guide;
- distinct task profiles and budgets rather than one universal prompt/tool set;
- separation of durable project/artifact state from ephemeral model history;
  and
- cancellation and stale-result rejection when the editable document changes.

**Not established by this workload:** a child-agent tree, parallel per-step
execution, durable conversation memory, or a general DAG scheduler. The
inspected code proves durable Action Guide artifacts and independent bounded
model tasks; whether a future product should orchestrate several concurrently
is an open product/architecture question.

### Pressure 3: brag + Hyperframes — artifact-driven multi-stage production

**Observed workflow shape.** brag reads a project, creates a timestamp-safe
output directory, writes a storyboard plan, hands a focused composition brief
to Hyperframes, gates on `hyperframes check`, then produces an MP4, selected
poster, and share copy. [B1]

Hyperframes expresses production as dependencies: assets must be installed
before parallel work; audio can render in the background while independent
frames build; assembly, transitions, captions, verification, and delivery have
artifact prerequisites. A background preview server supports a live storyboard
board. Collaborative runs pause for plan, optional sketch, and final-render
approval. [H1, H3]

Optional frame workers receive complete file-based dispatch packets and share
only the filesystem. Completion is the expected scene artifact, not an agent
notification. Missing artifacts trigger one clean re-dispatch; concurrency caps
change batching into waves, not scope. A serial inline fallback remains valid.
[H2]

**Capabilities this deferred workload would pressure:**

- project/resource inspection and skill-to-skill handoff with versioned inputs;
- explicit dependency/workflow state separate from conversational todos;
- typed expected artifacts, checks, revisions, and provenance;
- durable user checkpoint decisions and recovery from the last valid artifact;
- managed background process/job handles for preview, generation, and render,
  with start/wait/cancel/collect semantics;
- optional isolated child/worker contexts, bounded fan-out, waves, artifact
  verification, and selective retry; and
- progress/events that aggregate stages and workers without treating transient
  notifications as completion.

These requirements come from the cited brag/Hyperframes steps. Unused features
elsewhere in those projects are not evidence. The workload does not mandate
that Rollshot ship video generation, copy Hyperframes, or put every stage inside
`rollshot-agent`.

## Proven gaps

Each gap is a mismatch between current inspected code and at least one cited
workload, not a general platform wish list.

| Current gap (static evidence) | Workload pressure | Why it matters |
|---|---|---|
| No Rollshot-owned durable run/checkpoint/resume record in the investigated agent files; a fresh Rig run is created per invocation. [R3, G1] | Hyperframes; potentially multi-step Action Guide orchestration | Artifact/checkpoint continuation cannot be reconstructed from current run memory. Action Guide proves persistence should attach to product records, not merely a transcript. |
| `AgentSession` exchanges are neither fed into a new Rig run nor returned by the inspected workbench task. [R2, R3, R8] | Longer review cycles in Action Guide/Hyperframes | If conversational continuity is desired, it needs an explicit policy; durable workflow state must still remain separate. |
| No task/workflow/dependency state model in the investigated agent scope. | Hyperframes | Its stages have prerequisites, conditional stages, checkpoints, and artifact-scoped rework that cannot be represented by a single serial run terminal. |
| No managed external job/process lifecycle. | Hyperframes | Preview servers, audio/generation, and renders overlap model work and may outlive a turn. A blocking tool result is not the cited lifecycle. |
| No child-agent/worker registry, scoped child context, concurrency cap, wave scheduler, or artifact-based child completion. | Optional Hyperframes frame fan-out | The cited dispatch contract requires isolation, bounded fan-out, expected-artifact completion, and selective re-dispatch. This gap exists only if Rollshot adopts that optional execution mode. |
| Run events are transient and narrow; the workbench channel may drop them and relies on terminal reconciliation. [R5, R8] | Hyperframes multi-stage/worker progress and reconnect; longer Action Guide tasks | A longer workflow needs reconstructible state/progress, while transient display events may remain best-effort. |
| Cancellation stops the current provider/automation run but has no child/job cleanup graph. [R5, R6] | Hyperframes background work and optional workers | Parent cancellation would need explicit propagation and cleanup ownership. |
| Budget state is run-local; cost is never charged and no child/job/artifact hierarchy exists. [R5] | Current Smart Redaction cost ceiling; Hyperframes fan-out/jobs | Smart Redaction's configured cost ceiling is not enforced. Multi-worker/job work would need allocation distinct from one model loop's counters. |

## Current strengths

- Rollshot's public provider/model contracts are already independent of Rig,
  keeping retain/fork/replace/remove technically open. [R4, R6]
- Input authorization, redacted debug output, consent-selected payloads, bounded
  attachments, and privacy-filtered Action Guide semantics establish useful
  privacy boundaries. [R2, R8, A1]
- The tool registry is typed, JSON-schema-driven, bounded, deterministic, and
  terminates safely on the first successful terminal action. [R5]
- Draft generations and evidence invalidation prevent stale validation/dry-run
  results from authorizing a new source. [R5]
- Review is a typed product handoff: automation/proposal/evidence are returned
  for user judgment rather than applied by model prose. [R3]
- Budgets, shared cancellation, typed terminals, provider contract tests, and
  cancellation/privacy tests give the bounded loop strong local failure
  semantics even without durable orchestration. [R3, R5, R9]
- Action Guide already persists product artifacts independently of agent
  history, a sound boundary to preserve in later designs. [A2]

## Unknowns and bounded absences

1. Static inspection did not exercise live provider streams, the iced
   workbench, cancellation races, project crash recovery, Hyperframes workers,
   or rendering. Tests are implementation evidence, not production runtime
   observation.
2. It is not yet decided whether Action Guide needs foundation-owned
   orchestration beyond its current independent caption/annotation tasks.
3. It is not yet decided whether the deferred brag/Hyperframes workflow should
   run inside Rollshot, through an external skill host, or remain deferred.
4. Conversation retention, artifact retention, checkpoint privacy, deletion,
   and cross-version resume compatibility need explicit product policy before
   choosing a persistence design.
5. The Rig reference checkout is v0.40.0 while Rollshot pins 0.39.0. Later Rig
   decisions must use the pinned source for current behavior and treat newer
   checkout behavior as a candidate, not as proof of Rollshot behavior.
6. The exact code/test/security maintenance cost of retaining, vendoring,
   replacing, or removing Rig remains a later capability/synthesis question.
7. No task graph, background job manager, child-agent system, compact/memory
   layer, durable run store, or resume router was found in the six
   task-scoped `rollshot-agent` files. The search boundary does not prove those
   concepts cannot exist elsewhere or be supplied externally.

## Evidence index

All paths and symbols refer to the reproducibility baseline above. “Source”
means static implementation; “test” means executable test source inspected but
not necessarily run in this round.

| ID | Type | Path / symbol | Supports | Limit |
|---|---|---|---|---|
| R1 | policy/source | `AGENTS.md` §11; `docs/researchs/agent-foundation/README.md` §§1, 7 | Code-over-docs rule, workload framing, evidence discipline | Governance, not runtime behavior |
| R2 | source + unit tests | `crates/rollshot-agent/src/domain.rs`: `AuthorizedModelInput`, `AgentSession`, session/input tests | Authorization/privacy and in-memory exchange model | Does not prove app persistence |
| R3 | source + unit tests | `crates/rollshot-agent/src/driver.rs`: `AgentRunner::{run_with_provider,drive_streamed_turn,run_tool_turn}`, `RunTerminalState`, terminal/budget/cancellation tests | Run ownership, Rig driving protocol, terminals, tool-result threading | Scripted tests and static production path; no live provider run here |
| R4 | source + unit tests | `crates/rollshot-agent/src/model.rs`: public model types, `push_model_messages`, `drive_streamed_turn`, conversion tests | Rollshot public facade and private Rig translation | Does not cover provider HTTP behavior |
| R5 | source + unit tests | `crates/rollshot-agent/src/runtime.rs`: `RunBudget`, `BudgetTracker`, `DraftState`, `RunCancellation`, `RunEvent`; `tools.rs`: `Tool`, `ToolRegistry`, `ToolContext`, authoring tools and serial-order tests | Budgets, cancellation, events, serial tools, generation evidence | Run-local only; cost limitation is documented in source |
| R6 | source + contract tests | `crates/rollshot-agent/src/provider.rs`: `ProviderAdapter`, `AnthropicAdapter`, `OpenAIAdapter`, `build_completion_request`, `stream_to_model_events`; `tests/provider_contract.rs` | Private Rig provider machinery behind Rollshot events | Contract test source is not a live external-provider observation |
| R7 | source/lock | `crates/rollshot-agent/Cargo.toml`; `Cargo.lock` `rig-core` entry; `src/lib.rs` re-exports | Exact 0.39 pin and public module/export boundary | Lockfile does not prove which runtime branches execute |
| R8 | source + unit tests | `crates/rollshot-app/src/result_workspace/workbench/{mod.rs,run.rs,state.rs}`; `result_workspace/update.rs` workbench arms | Active Smart Redaction integration, finite budget, session move, event channel, consent/review ownership | UI path statically inspected, not launched |
| R9 | tests | `crates/rollshot-agent/tests/provider_contract.rs`; cancellation/privacy tests in `driver.rs`, `runtime.rs`, `tools.rs` | Existing failure/privacy verification surface | Test results are recorded in this task's verification report, not inferred here |
| G1 | source + tests | Locally resolved `rig-core-0.39.0/src/agent/run/{mod.rs,streamed.rs}`: `AgentRun`, `AgentRunStep`, `tool_results`, `record_streamed_completion_call`, `streamed_turn`, assembler and round-trip tests | Exact pinned state-machine/assembly invariants, serializability, driver-selected concurrency | Local registry path is machine-specific; version/checksum make it reproducible |
| G2 | reference source | `learn-projects/rig/crates/rig-core/src/agent/run/{mod.rs,streamed.rs}` at the recorded v0.40.0 hash | Current supporting Rig boundary and upstream evolution | Not the version compiled by Rollshot |
| A1 | source + unit tests | `crates/rollshot-action/src/{models.rs,guide.rs}`: `GuideStep`, `Guide` edit methods, privacy-filtered semantics | Editable guide/workflow artifact and privacy boundary | Headless model, not agent orchestration |
| A2 | source + tests | `crates/rollshot-action/src/project/{model.rs,store.rs}`: `ProjectManifestV2`, `ProjectStep`, `save_project[_as]`, `load_project` | Durable revision/frame/step/annotation state | Project persistence, not agent-run persistence |
| A3 | source + tests | `crates/rollshot-app/src/timeline_workspace/{visual_annotation_agent.rs,caption_agent.rs,update.rs}`; `crates/rollshot-action/src/{visual_annotation_proposal.rs,caption_proposal.rs}`: suggestion tasks, proposal validation, stale-result handling | Heterogeneous bounded tasks and typed proposal lowering tied to guide artifacts | No evidence of parallel dispatch or durable agent sessions |
| B1 | workflow specification | `learn-projects/brag/skills/brag/SKILL.md` steps 1–4 and gates | Project inspection, plan/brief handoff, validation, render/poster/share artifacts | Workflow requirement, not Rollshot implementation |
| H1 | workflow specification | `learn-projects/hyperframes/skills/hyperframes-core/references/production-loop.md` | Dependency stages, background audio/generation, artifact inputs/outputs, verify/deliver | Describes Hyperframes workflow behavior, not a Rollshot requirement by itself |
| H2 | workflow specification | `learn-projects/hyperframes/skills/hyperframes-core/references/subagent-dispatch.md` | Optional parallel workers, caps/waves, expected-artifact completion, re-dispatch/fallback | Applies only if the deferred workload adopts worker dispatch |
| H3 | workflow specification | `learn-projects/hyperframes/skills/hyperframes-core/references/review-loop.md` §§1–4 | Background preview, durable board artifacts/status, approval checkpoints, render gate | User-workflow contract, not runtime observation |
