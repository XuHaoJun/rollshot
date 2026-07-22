# Provider and context boundaries comparison

**Research date:** 2026-07-23 (Asia/Taipei)

**Status:** In Progress (Round 4 capability comparison)

**Umbrella revision:** 1

**Research round:** 4

**Systems/capabilities:** Provider-neutral request/message/streaming/usage/stop/
Tool call abstractions; model context limits, compaction triggers, and capability
negotiation; provider switching/handoff and Rollshot-owned state; the Rig
retain/fork/replace/remove boundary reassessed with accumulated evidence.

**Evidence baseline:** Rollshot `7ef47b819c96207a90d10718eeff06521b6b2dfa`;
Pi `dd6bea41efa8caa7a10fe5a6401676dc5699f83f`; oh-my-pi
`7b141199d524b859c357fc89654f10b62b9f3df1`; Codex
`4a443994bd12f49f2f08b21a2f224d9d42b9e734`; Claude Code source
`2ca5ddabfed5f220812ea11f029eda03b21bc4c1`; Rig reference checkout
`2f37dfcd0156bdceab3eabe6f0a953f9202e2d77` (v0.40.0) plus the locally resolved
`rig-core-0.39.0` crate Rollshot actually pins (Cargo.lock checksum
`80a4bc7a93b329c4e1a66d5fd211d79990e7331e3c701f057c29f135f548686d`); deferred
workload references brag `357a805e76a93a528ac6cccac28c8da3e893272b` and
Hyperframes `807078c7cde9d5c8403588722d1cd9397c513a0d`.

**Evidence mode:** static source and test-source inspection of the current
Rollshot tree and the pinned reference checkouts, plus the reviewed Round 0
baseline, four reviewed system profiles, and reviewed Round 2–4 capability
documents. No provider request, stream, compaction, model switch, or credential
flow was executed. The reference roots are absent from Rollshot's code-review
graph (7,979 nodes, 65,744 edges, 405 files), so their pinned trees were
inspected with bounded searches after the required graph-first checks on
Rollshot roots.

This document compares implemented semantics, not similarly named types. It
does **not** select a final Rollshot provider architecture and does **not**
select a Rig disposition; Step 4 (Section 8) is an evidence update whose final
selection belongs to the Round 6 decision matrix.

## 1. Problem statement and workload pressure

Rollshot's agent foundation must decide how much of the provider contract it
owns. A facade that erases every provider detail cannot represent provider-
native continuity (thinking signatures, response identifiers, prompt-cache
state, deferred tool loading). A facade that passes provider details through
unchecked couples product state — budgets, cancellation, review terminals,
durable artifacts — to one vendor's wire format. Context handling is the same
ownership question: a context window is provider/model metadata, while the
decision to compact, degrade, or terminate is product policy.

The workload ladder pressures three different answers:

| Workload | Current evidence | Provider/context pressure actually established |
|---|---|---|
| **Smart Redaction** | One bounded run per invocation; Anthropic or OpenAI adapter built fresh from workbench config; serial tools; 16-dimension budget; typed terminal; no attachments sent on the Smart Redaction path. [E:R1–E:R5, W1] | Provider-neutral streaming with tool-call/result continuity is demonstrated as necessary. Cross-run provider continuity, context compaction, capability negotiation, and provider cost accounting are **not** established by this workload [A:R-CTX, A:R-CAP]. |
| **Action Guide** | Independent bounded visual-annotation runs (image attachments on the first model turn) and direct caption calls through the same facade; durable project state is product-owned, not provider-owned. [E:R6, W2] | A facade usable both inside the Rig-driven loop and as a direct one-shot completion path. Vision attachment support is required at the request boundary; whether the model is vision-capable is currently assumed, not negotiated [A:R-CAP]. |
| **Deferred brag + Hyperframes** | Multi-stage artifact workflow with checkpoints, background jobs, and optional workers; no Rollshot implementation. [W3–W6] | If adopted, longer horizons pressure context-window policy (when to compact versus re-project from artifacts), per-stage model/provider selection, and usage/cost accounting across stages. It does not mandate any specific provider breadth or a general multi-provider platform. |

Two false equivalences this document avoids: **provider-configurable is not
provider-neutral** (Codex is configurable but Responses-wire-only, §4.3), and
**a provider stop reason is not a product terminal** (Rollshot's
`RunTerminalState` is host-owned; `StopReason` only describes one model call).

## 2. Vocabulary and non-equivalent concepts

| Term | Meaning in this comparison | Non-equivalence rule |
|---|---|---|
| **Provider adapter** | Code that translates one host request into one provider's API and normalizes the response stream. | Implementing several adapters does not make the host contract provider-neutral; the contract's information content decides that. |
| **Provider-neutral contract** | The host-owned request/message/usage/stop vocabulary every adapter must satisfy. | “Neutral” ranges from strict erasure (no provider-identifying state) to continuity-preserving (opaque provider payloads retained); these are materially different designs, not one concept. |
| **Wire coupling** | The host contract is shaped by one provider's API (for example, Responses items). | Endpoint/base-URL configurability under one wire shape is not wire neutrality. |
| **Opaque provider payload** | Provider-issued bytes/identifiers (thinking signatures, response IDs, cache markers) the host stores but does not interpret. | Storing an opaque payload is not understanding it; handoff rules must state which payloads survive a provider switch. |
| **Provider session state** | Mutable per-provider transport state persisted between turns (for example, oh-my-pi's `providerSessionState`). | Not equivalent to conversation history, a durable agent session, or a compaction boundary. |
| **Usage** | Provider-reported token counts for one model call. | Input/output/total is a strict subset of cache-aware usage (cache read/write, reasoning, orchestration, cost). Reported usage is not charged cost. |
| **Stop reason** | Why one model call ended (`EndTurn`, `ToolUse`, `length`, provider error). | Not a run/workflow terminal; not a cancellation outcome; not a budget verdict. |
| **Context window** | A model's provider-declared token capacity, possibly scaled by a host reservation policy. | Not a budget dimension; exceeding it is a provider/protocol event, while a budget verdict is host policy. |
| **Compaction trigger** | The threshold or signal that starts context reduction (token estimate, provider overflow, output-length stop, manual command, idle maintenance). | A trigger is not the reduction mechanism; summary, provider-native replacement, pruning, and re-projection are different mechanisms (compared in `context-compaction.md`). |
| **Capability negotiation** | The host learns or is told what a model supports (vision, tools, parallel calls, reasoning, context size) and adapts requests. | Advertising a tool schema every turn is not negotiation; assuming vision support is not negotiation. |
| **Provider handoff** | Continuing work across a provider/model change: mid-run switch, session resume under another model, or a document handoff into a new session. | Configuring a different provider for the next independent run is selection, not handoff. |

## 3. Current Rollshot behavior

### 3.1 The public facade and the private Rig translation

The Rollshot-owned public model surface lives in
`crates/rollshot-agent/src/model.rs` and exposes no Rig types: `ModelRequest`
(model, prompt, history, turn, tool_definitions, system_prompt, max_tokens,
attachments), `ModelMessage` (User, Assistant, AssistantToolCall, ToolResult),
`ToolDefinition` (name, description, JSON-schema parameters),
`ModelStreamEvent` (seven variants), `ModelUsage`, `ModelCompletion`,
`StopReason`, `ModelError`, and `ModelAttachment` whose `Debug` redacts bytes.
The provider module is `pub(crate)` in `src/lib.rs:4`; only
`ProviderAdapter`, `AnthropicAdapter`, `OpenAIAdapter`, and `StreamBounds`
are re-exported (`src/lib.rs:9`). A bounded audit confirms `rig_core`
references are confined to `driver.rs`, `model.rs`, and `provider.rs`
[A:R-LEAK]. [E:R1, E:R2]

The concrete adapters are **not** hand-written HTTP transports. They build Rig
`CompletionRequest`s (`provider.rs:97–147`) and call Rig's Anthropic/OpenAI
completion-model streaming (`provider.rs:84–91`, `269–276`). OpenAI explicitly
sets `parallel_tool_calls: false` (`provider.rs:248`), matching Rollshot's
serial tool policy. `max_tokens` defaults to 4,096 when unset
(`provider.rs:54`, `142`), and both production drivers pass `max_tokens: None`
(`driver.rs:944`, `1473`), so 4,096 is the effective ceiling today. [E:R2,
E:R3]

### 3.2 Normalization Rollshot already owns around Rig

The translation layer is thicker than a pass-through. Verified behavior in
`provider.rs` and `model.rs` [E:R2, E:R3, A:R-STOP]:

- **Error taxonomy:** `rig_to_model_error` maps Rig `CompletionError` variants
  onto Rollshot's three-variant `ModelError` (ProviderFailure, ProtocolFailure,
  StreamIncomplete), string-matching authentication and rate-limit cases and
  truncating messages at 500 characters (`provider.rs:181–212`).
- **Synthetic completion:** if a provider stream ends without a final item —
  the comment names Anthropic's SSE loop breaking on `message_delta` —
  `stream_to_model_events` finishes the assembler and emits a synthetic
  `Completed` with usage accumulated from `UsageDelta` events, explicitly
  noting that missing provider usage is not zero (`provider.rs:366–403`).
- **Stop-reason rewriting:** when a completed turn contains tool calls but the
  provider's stop reason disagrees, the adapter replaces the emitted stop
  reason with `ToolUse` (`provider.rs:329–342`).
- **Usage capture:** only `input_tokens`/`output_tokens` (and their sum) are
  captured into `ModelUsage`; there are no cache-read/write, reasoning,
  orchestration, or cost fields (`model.rs:119–124`).
- **Stream-item erasure:** reasoning items are ingested by Rig's assembler but
  deliberately not surfaced as Rollshot events, and a wildcard arm silently
  drops any other unmodeled stream item (`model.rs:348–353`). *Inference:*
  under a future Rig upgrade that adds provider stream variants (v0.40 added
  `StreamedAssistantContent::Unknown`, Section 8.2), this wildcard would keep
  compiling while silently discarding the new items; the behavior is correct
  at the pinned version but is an upgrade-review hazard, not a runtime defect
  today.
- **Declared but unconstructed stop variants:** `StopReason::MaxTokens` and
  `StopReason::Unknown(String)` exist in the enum, but a bounded audit found
  no construction site in `crates/rollshot-agent/src`; streamed turns resolve
  to `EndTurn` or `ToolUse` only [A:R-STOP].

### 3.3 Request, message, and tool-call threading

`push_model_messages` is the single translation point from Rig history into
Rollshot's provider-neutral history, preserving user text, assistant text,
assistant tool calls (id, name, JSON arguments), and tool results correlated
by call ID (`model.rs:267–315`). `build_completion_request` reassembles the
Rig request: chat history, optional prompt, attachments as a trailing user
image message, tool definitions, and the system prompt as `preamble`
(`provider.rs:97–147`). Tool-call argument assembly is split: Rig ingests and
validates stream items; the Rollshot driver stores argument deltas per call,
charges their bytes, concatenates, and parses the final JSON itself (Round 0
baseline [E:R7]; `driver.rs` streamed-turn path). [E:R2, E:R3]

### 3.4 Context limits, compaction, and capability negotiation

A bounded audit across `crates/rollshot-agent/src` found **no model context-
window metadata, reservation policy, compaction trigger, or compaction
mechanism** in the investigated scope [A:R-CTX]. The only token ceilings are
budget dimensions (`InputTokens`/`OutputTokens` in `runtime.rs`) and the
4,096-token output default above; neither is a context-window policy. A
provider context-overflow error would arrive as a generic
`ModelError::ProviderFailure`/`ProtocolFailure` via `rig_to_model_error`
string matching — there is no dedicated overflow class and no retry-with-
reduction path [A:R-CTX, A:R-STOP].

A second bounded audit found **no capability negotiation**: no model
descriptor, no vision/reasoning/parallel-tool capability check, and no
unsupported-capability degradation path [A:R-CAP]. Attachments are sent
whenever the driver has them (visual annotation's first model turn,
`driver.rs:1375`, `1449–1454`); whether the configured model accepts images is
assumed. Full tool schemas are advertised on every request
(`provider.rs:125–133`); there is no deferred tool loading. Authority is
therefore entirely static and host-owned — registered tools, authorized
inputs, finite budgets — which keeps authority and availability cleanly
separated, but at the cost of assuming the configured model can do what the
run asks.

### 3.5 Provider selection and handoff state

Provider choice is **workbench-owned, per run, and config-backed**
(`provider_config.rs`): `ProviderKind::{Anthropic, OpenAI}`, a model string,
optional base URL, and a `KeySource::Env` that persists only the environment
variable **name**, never the key (`provider_config.rs:5–51`, `91–99`).
`build_adapter` constructs a fresh adapter per invocation
(`provider_config.rs:108–126`; called from `workbench/run.rs:780` per Smart
Redaction run and from `timeline_workspace/update.rs:1162`, `1396` per
annotation/caption call). Changing the configured provider changes the next
run's adapter; there is no mid-run switch, no provider-continuation state, and
no cross-run history — each run builds a fresh Rig `AgentRun` and
`with_history` is never called [A:R-HANDOFF; baseline E:R7]. Rollshot
consequently has no provider-handoff problem today, bounded to the inspected
paths: nothing provider-specific survives a run, because nothing at all
survives a run. [E:R4, E:R5]

State that is currently and explicitly Rollshot-owned, not provider- or
Rig-owned (baseline [E:R7]; re-verified): consent and input authorization
(`AuthorizedModelInput`, `domain.rs:107–209`), the 16-dimension
`RunBudget`/`BudgetTracker`, `RunCancellation`, the typed `Tool` registry and
serial scheduling policy, draft generations and validation/dry-run evidence,
the review proposal and `RunTerminalState` taxonomy, `RunEvent` policy, and
Action Guide's durable project records. The provider config itself — including
credential handling — is product-owned at the workbench boundary. [E:R4,
E:R5, E:R6]

## 4. Per-system behavior

### 4.1 Pi: small neutral contract that deliberately retains provider continuity

`pi-ai` owns the provider-facing contracts (`packages/ai/src/types.ts`,
`models.ts`). `Context` is `{ systemPrompt?, messages, tools? }`; `Message` is
User/Assistant/ToolResult. The streaming protocol is
`AssistantMessageEvent`: `start`, incremental partial-message updates, then
exactly one terminal — `done` carrying the final `AssistantMessage`, or
`error` carrying a final message whose `stopReason` is `error`/`aborted`;
failures are encoded as terminal assistant messages rather than thrown
transport errors (`types.ts:468–480`; profile §13 [E:P1]). [E:P1, E:P2]

The contract is unified and application-owned **but continuity-preserving**:
`AssistantMessage` retains `api`, `provider`, `model`, `responseModel` (for
proxied auto-routing), and `responseId`; `TextContent.textSignature`,
`ThinkingContent.thinkingSignature` (including redacted-thinking payloads
passed back for continuity), and `ToolCall.thoughtSignature` (Google-specific)
carry opaque provider state inside the normalized types
(`types.ts:329–357`, `390–403`). `Usage` is cache- and cost-aware:
input/output/cacheRead/cacheWrite/cacheWrite1h (Anthropic-only split),
optional reasoning, totalTokens, and a per-bucket cost object
(`types.ts:359–380`). `StopReason` is `stop | length | toolUse | error |
aborted` (`types.ts:382`). `ToolResultMessage` supports image content and
`addedToolNames`, the load point for providers with native deferred tool
loading (`types.ts:405–424`). [E:P1]

Provider dispatch is one injected `StreamFn`; the agent loop depends only on
it, and coding-agent extensions can register providers and intercept final
headers/payloads (profile §13 [E:P1]). Model changes are persisted as session-
tree entries and the active model plus thinking level is restored on resume;
the profile's unresolved question 5 records that cross-provider resume
handling of provider-specific thinking/response state is **not settled**
(profile §13, §16). Pi's own compaction trigger is estimated context
exceeding `contextWindow - reserveTokens` (default reserve 16,384 in
`packages/coding-agent/src/core/compaction/compaction.ts:128–134`) or a
provider overflow report, with at most one compact-and-retry recovery
(profile §6 [E:P3]). [E:P1, E:P3]

### 4.2 oh-my-pi: the same shape, extended into explicit opaque-payload state

oh-my-pi keeps the Pi-lineage neutral loop boundary and pushes the
continuity-preserving direction further (profile §13 [E:O1]). Its message
types add `providerPayload` — “provider-specific opaque payload used to
reconstruct transport-native history” — on assistant and developer messages, a
`providerSessionState` per-provider mutable map persisted between turns,
message `attribution`, and a Codex-compaction classification field
(`packages/ai/src/types.ts:420–430`, `665–690`, `749`). Its catalog `Usage`
adds an orchestration bucket (provider-side billed tokens outside the
conversation prompt/cache buckets), Copilot premium-request counters, and
reasoning detail (`packages/catalog/src/types.ts:95–119`). [E:O1, E:O2]

Breadth is explicit and registry-based: 14 reserved built-in API adapter
identifiers, 61 top-level catalog provider IDs, extension-registered streaming
functions under non-reserved API names, and merged bundled/`models.yml`/
runtime-discovered local models with provider-scoped credentials (profile §13
[E:O1]). The profile notes this breadth expands compatibility, retry, privacy,
and test surface. [E:O1]

Context/compaction is the richest trigger set among the four: manual,
provider overflow, incomplete output due to length, post-turn threshold
maintenance, optional mid-run checks, and idle maintenance; strategies include
local summary, provider-native remote compaction with opaque-state
preservation where supported plus local fallback, deterministic `snapcompact`
(which **requires a vision-capable continuation model** — an explicit
capability constraint), a session-to-session `handoff` document that starts a
new session without appending a compaction entry, surgical `shake`, and
pruning/elision (profile §6 [E:O3]). [E:O3]

### 4.3 Codex: endpoint-configurable but Responses-wire-coupled

Codex has a runtime `ModelProvider` trait (`codex-rs/model-provider/src/
provider.rs:101`) owning provider info, auth/account state, capability upper
bounds, model catalog construction, helper-model preferences, and error
mapping; `ModelProviderInfo` permits custom base URL, several auth modes,
headers, retry limits, and timeouts (`model-provider-info/src/lib.rs`;
profile §13 [E:C1]). Model metadata carries capability/context information —
for example `supports_parallel_tool_calls` and `context_window` values such as
272,000 in provider test fixtures (`model-provider/src/provider.rs:464–467`),
and provider capability flags can bound namespace tools, image generation, and
web search (profile §13). [E:C1]

The wire contract is **not neutral**: `WireApi` has exactly one accepted
variant, `Responses`, and `wire_api = "chat"` is rejected
(`model-provider-info/src/lib.rs:57–111`); the core client builds
`/responses`, `/responses/compact`, memory, and realtime calls and retains
Responses-specific IDs, request items, streaming events, sticky turn state,
and optional WebSocket incremental state. Amazon Bedrock integrates through an
OpenAI-compatible Responses/Mantle boundary, not a distinct message protocol.
A native Anthropic Messages or arbitrary provider wire adapter was **not found
in the investigated scope** (profile §13 and its bounded audit [E:C1]). [E:C1]

Context policy is model-metadata-driven: `model_context_window()` resolves the
model's declared window scaled by `effective_context_window_percent`
(`core/src/session/turn_context.rs:208–215`), tracked per auto-compact window
(`core/src/state/session.rs:38`, `190–198`). Compaction is built-in and
provider-split: local summary compaction (default for ineligible providers)
or, only for recognized OpenAI/Azure Responses providers, remote compaction —
`remote_compaction_v2` is stable and default-on through the normal Responses
stream with a 64k retained-message budget and two transport retries
(profile §6 [E:C2]). Resuming a thread under a different model emits a
warning rather than blocking (reviewed persistence evidence
`persistence-checkpoint-resume.md` §5.1 [E:C3]). [E:C2, E:C3]

### 4.4 Claude Code: single-provider contract with provider-native context management

The visible loop is Anthropic-shaped: Anthropic SDK resources, Claude thinking
and cache metadata, Claude model resolution, and provider context-management
strategies; memory relevance selection explicitly uses a Claude model
(profile §13 [E:L1]). A general third-party model adapter was **not found in
the investigated scope**, bounded to the profile's named orchestration/
provider files [E:L1]. MCP is a tool/resource extension boundary, not a model
abstraction; CCR/teleport and bridge are remote session transports, not
inference providers. [E:L1]

Context management is provider-native and policy-layered (profile §6 [E:L2]):
traditional full compaction reserves up to 20,000 tokens for the summary and
auto-starts roughly 13,000 tokens below the effective window, with a
three-failure circuit breaker and prompt-too-long recovery that drops oldest
API-round groups up to three times; per-call thinking management preserves or
trims thinking; feature-gated API tool-result/use clearing (internal-only
opt-in) uses a 180,000-token trigger and 40,000-token target; cached
microcompact edits API cache context. Resume restores the selected model among
other session state (profile §7 [E:L2]). All of it assumes one provider's
capabilities. [E:L2]

## 5. Step 1 — abstraction comparison: request, message, streaming, usage, stop, Tool call

Every cell cites positive evidence or a bounded audit; “not found” claims are
scoped in Section 12.1.

| Abstraction | Rollshot | Pi | oh-my-pi | Codex | Claude Code |
|---|---|---|---|---|---|
| **Request model** | `ModelRequest`: model, prompt, history, turn, tool defs, system prompt, max_tokens, attachments; provider-erased. [E:R1] | `Context { systemPrompt?, messages, tools? }` plus stream options; provider/API selected by `Model`. [E:P1] | Same shape, extended with session/provider options (concurrency caps, compaction class). [E:O1, E:O2] | Responses-shaped request items built by the core client; config selects endpoint, not wire. [E:C1] | Anthropic request shape with thinking/cache parameters; no neutral request type found [E:L1]. |
| **Message/content model** | Four-variant `ModelMessage`; text plus image attachments only; no provider IDs or signatures. [E:R1] | User/Assistant/ToolResult with text/thinking/image/tool-call content; retains api/provider/model/responseId and opaque signatures. [E:P1] | Pi shape plus `providerPayload`, `providerSessionState`, `attribution`, developer messages. [E:O2] | Responses items with provider IDs and sticky turn state. [E:C1] | Anthropic message types with thinking/cache metadata. [E:L1] |
| **Streaming protocol** | Seven `ModelStreamEvent` variants; adapter synthesizes `Completed` when the provider stream ends silently and rewrites stop reason on tool-call turns. [E:R2, A:R-STOP] | `AssistantMessageEvent` partial-message protocol; failures become terminal assistant messages. [E:P1, E:P2] | Same protocol with retry-recovery and healing wrappers. [E:O2] | Responses streaming events mapped into protocol turn/item lifecycle. [E:C1] | Anthropic stream mapped to message/result loop. [E:L1] |
| **Usage model** | `ModelUsage { input, output, total }` only; no cache, reasoning, orchestration, or cost fields; cost budget is never charged (reviewed budgets evidence). [E:R1, E:R8] | Cache-aware usage with cost per bucket; Anthropic 1h cache split; optional reasoning. [E:P1] | Adds orchestration bucket, premium-request counter, reasoning detail. [E:O2] | Token usage drives auto-compact windows; cost telemetry exists in protocol/app surfaces (reviewed events evidence). [E:C2] | Cache-read/write usage visible in transcripts and compact metrics. [E:L2] |
| **Stop reason** | `EndTurn | ToolUse | MaxTokens | Unknown(String)`; MaxTokens/Unknown declared but never constructed [A:R-STOP]. | `stop | length | toolUse | error | aborted`, with error as a first-class reason. [E:P1] | Same, plus retry-recovery classification. [E:O2] | Turn/item status plus typed abort reasons at the protocol layer. [E:C1] | Provider stop reasons plus loop-level result classification. [E:L1] |
| **Tool-call representation** | `AssistantToolCall { id, name, arguments }` / `ToolResult { tool_call_id, result }`; argument deltas are Rollshot-accumulated and JSON-parsed in the driver. [E:R1, E:R3] | `ToolCall { id, name, arguments, thoughtSignature? }`; partial-call assembly in providers/loop. [E:P1] | Same, with structured child-output schemas. [E:O1] | Responses-native tool call/output items. [E:C1] | Anthropic tool_use/tool_result blocks. [E:L1] |
| **Tool-call/result pairing authority** | Rig enforces a complete result set per pending call before the next request; Rollshot executes serially in model order (reviewed tools evidence). [E:R3, E:R7] | Loop correlates by call ID; parallel completion with source-order persistence (profile §8). [E:P1] | Same correlation with shared/exclusive scheduling. [E:O1] | Router/registry with typed statuses and cancellation (profile §8). [E:C1] | Tool-use/result pairing in the Anthropic message stream. [E:L1] |
| **Parallel tool calls** | Disabled: serial policy in the registry/driver and `parallel_tool_calls: false` on OpenAI. [E:R2, E:R5] | Supported; per-file mutation queue for side-effect ordering (profile §8). [E:P1] | Supported with shared/exclusive declarations. [E:O1] | Model metadata flag; per-handler parallel admission. [E:C1] | Conservative batching with visible tool-batch limits (profile §8). [E:L1] |
| **Deferred tool loading** | Not found; full schemas advertised every request [A:R-CAP]. | `addedToolNames` load point for providers with native support. [E:P1] | Same mechanism, broader providers. [E:O1] | Deferred discovery data re-injected at compaction (profile §6). [E:C2] | Deferred tool/agent/MCP discovery data re-injected at compaction. [E:L2] |

## 6. Step 2 — context limits, compaction triggers, and capability negotiation

The compaction **mechanisms** are compared in `context-compaction.md`; this
section compares the trigger/policy and negotiation layer only.

| Aspect | Rollshot | Pi | oh-my-pi | Codex | Claude Code |
|---|---|---|---|---|---|
| **Context-window source of truth** | None found [A:R-CTX]; only budget token dimensions and a 4,096 output default. [E:R1, E:R2] | Model/context metadata consulted by the coding-agent compactor. [E:P3] | Same, across bundled/custom/discovered models. [E:O1, E:O3] | Model metadata `context_window` scaled by `effective_context_window_percent`. [E:C2] | Effective window derived from Claude model limits. [E:L2] |
| **Reservation policy** | Not found [A:R-CTX]. | `reserveTokens` (default 16,384). [E:P3] | Threshold maintenance per strategy. [E:O3] | Percent-based reservation in the auto-compact window. [E:C2] | Up to 20,000-token summary reserve; auto-start ~13,000 below window. [E:L2] |
| **Trigger set** | Not found [A:R-CTX]; a provider overflow would surface as a generic provider/protocol error [A:R-STOP]. | Threshold estimate or provider overflow; one compact-and-retry recovery. [E:P3] | Manual, overflow, length-stop, post-turn and mid-run maintenance, idle. [E:O3] | Auto window tracking plus manual; overflow removes oldest items until the request fits (local path). [E:C2] | Auto threshold, manual, prompt-too-long drop recovery, three-failure circuit breaker. [E:L2] |
| **Provider coupling of the trigger path** | None (no path) [A:R-CTX]. | Provider-neutral estimate plus overflow signal. [E:P3] | Mixed: local strategies plus provider-native remote compaction. [E:O3] | Split: local summary default; remote v2 only for recognized OpenAI/Azure Responses providers. [E:C2] | Anthropic-native (cache edits, API clearing gates). [E:L2] |
| **Capability negotiation (vision/tools/reasoning/parallel)** | Not found; vision assumed when attachments exist [A:R-CAP]. | Thinking levels; `addedToolNames` deferred loading; signatures carried per provider. [E:P1] | Same plus snapcompact's documented vision-model requirement. [E:O2, E:O3] | Model metadata (context, reasoning, parallel tools) and provider capability flags bound tools/features. [E:C1] | Per-call thinking strategy; internal-only API clearing gates. [E:L2] |
| **Unsupported-capability behavior** | Not found; failure would arrive as a provider error [A:R-CAP, A:R-STOP]. | Terminal assistant message with `stopReason: error`. [E:P1, E:P2] | Same, plus documented strategy fallbacks (e.g., remote→local compaction). [E:O3] | Capability flags exclude features up front; Bedrock routed through a compatible wire. [E:C1] | Feature gates hide unavailable mechanisms; external build lacks hidden reducers. [E:L2] |
| **Cost accounting** | Declared budget dimension, never charged (reviewed budgets evidence). [E:R8] | Per-bucket cost in `Usage`. [E:P1] | Usage plus observed request-cost storage. [E:O2] | Usage/telemetry surfaces; exec/app surfaces differ (reviewed events evidence). [E:C2] | Usage/cost visible in transcripts. [E:L2] |

## 7. Step 3 — provider switching, handoff, and Rollshot-owned state

### 7.1 Switching and handoff comparison

| Aspect | Rollshot | Pi | oh-my-pi | Codex | Claude Code |
|---|---|---|---|---|---|
| **Selection point** | Workbench config; fresh adapter per run/call. [E:R4, E:R5] | Session tree model-change entries; restored on resume. [E:P1] | Same, plus per-strategy provider choices (e.g., `/compact remote`). [E:O1, E:O3] | Config/provider info at thread/turn scope. [E:C1] | Session model selection restored on resume. [E:L2] |
| **Mid-run switch** | Not found [A:R-HANDOFF]. | Not found in the investigated loop; model changes apply across turns via session state [E:P1]. | Mid-run handoff falls back to in-place compaction [E:O3]. | Not found as a turn-level operation in reviewed sources [E:C1]. | Not found in the investigated scope [E:L1]. |
| **Provider-specific continuation state** | None survives a run, because nothing survives a run [A:R-HANDOFF]. | History retains provider/API IDs and opaque thinking/response signatures; cross-provider resume handling is an open question (profile §16 Q5). [E:P1] | `providerPayload`/`providerSessionState` persist transport-native state; history rewrites close provider sessions; child revival uses current auth/model and must handle staleness. [E:O2, E:O3] | Sticky turn state and Responses items; resume under a different model warns. [E:C1, E:C3] | Thinking/cache metadata; compaction resets cache baselines. [E:L2] |
| **Handoff mechanism** | Not found [A:R-HANDOFF]. | Not found; sessions continue in place. [E:P1] | `handoff` strategy: generated document plus a new session, deliberately not a compaction entry. [E:O3] | Not found; compaction is in-thread. [E:C2] | Not found in the investigated scope [E:L1]. |
| **Failure on switch/resume mismatch** | Not applicable today (fresh run; nothing to mismatch) [A:R-HANDOFF]. | Unresolved (profile Q5). [E:P1] | Fallback policies named per strategy. [E:O3] | Warning, not a block. [E:C3] | Not found in the investigated scope [E:L1]. |

### 7.2 State that must remain Rollshot-owned

Regardless of provider design, the reviewed evidence consistently places these
outside the provider boundary. For Rollshot they are **already** product-owned
and no candidate pattern in Section 9 may move them into a provider or library
boundary [E:R4–E:R8]:

1. **Consent and input authorization** — `AuthorizedModelInput` validation and
   payload-mode selection; attachment bytes enter requests only through it.
2. **Budgets and accounting policy** — the 16-dimension `RunBudget`, charge
   timing, exhaustion terminals, and any future cost-charging function.
3. **Cancellation** — one `RunCancellation` fan-out into provider streams,
   automation execution, and deadlines.
4. **Tool authority** — registration, per-run availability, serial scheduling,
   argument/result limits, and terminal-tool stop semantics.
5. **Review and terminal taxonomy** — `RunTerminalState`, draft generations,
   validation/dry-run evidence, and the proposal handoff.
6. **Durable product records** — Action Guide project manifests/revisions and
   any future run/checkpoint store, including retention and deletion policy.
7. **Credential custody** — only the env-var name persists; key resolution and
   adapter construction stay at the workbench boundary.
8. **Context policy** — if adopted: trigger thresholds, reservation, what is
   preserved, and overflow terminals are product decisions even when a
   provider offers native compaction (Codex's remote path and Claude's cache
   edits are provider-coupled conveniences, not ownership transfers [E:C2,
   E:L2]).

Availability versus authority stays explicit: a configured provider or an
advertised tool schema makes a capability *available*; only the workbench's
consent, registry, budget, and review boundaries make it *authorized*. None of
the four reference systems moves product authority into the provider layer —
even Claude Code, the most provider-coupled, keeps approvals and permissions
host-side (reviewed permissions evidence).

## 8. Step 4 — Rig provider and state-machine boundary reassessment

This section updates the Round 0 baseline's four-option table with evidence
accumulated across the reviewed profiles and Round 2–4 capability documents,
plus a focused drift measurement between the pinned `rig-core-0.39.0` and the
v0.40.0 reference checkout [A:G-DRIFT]. It selects nothing; dispositions
belong to the Round 6 decision matrix.

### 8.1 What accumulated evidence confirms

1. **Facade integrity holds.** The public model/provider surface remains
   Rig-free; `rig_core` references remain confined to `driver.rs`, `model.rs`,
   `provider.rs`; the provider module is `pub(crate)` with four re-exports
   [A:R-LEAK]. All four options remain technically open, as the baseline
   claimed. [E:R1, E:R7]
2. **The delegated invariants are still load-bearing.** Reviewed evidence
   shows Rollshot's driver relies on Rig for the exhaustive
   CallModel/CallTools/Done protocol and out-of-order rejection (baseline),
   turn counting and `max_turns` enforcement mapped to
   `BudgetExhausted { ModelCalls }` (budgets document), complete tool-result
   pairing before the next model request (tools and context-compaction
   documents), and streamed-turn validation including unknown-tool rejection
   (`model.rs:373–377` and tests). Any non-retain option must re-prove these,
   not merely recompile around them. [E:R3, E:R7, E:R8]
3. **Rollshot already owns substantial provider-edge normalization.** Error
   taxonomy mapping, synthetic completion, stop-rename rewriting, usage
   accumulation, cancellation/deadline bounding, and argument-delta
   charging/parsing are Rollshot code around Rig, not Rig behavior (Section
   3.2). The “replace/remove would re-implement everything” framing is
   therefore weaker than a pure pass-through assumption: part of the edge is
   already owned and test-covered by `tests/provider_contract.rs` (wiremock
   SSE fixtures: text/tool streaming, cumulative usage, unknown event types,
   malformed JSON, incomplete streams, 401/429/500 mapping, endpoint shapes,
   debug redaction). [E:R2, E:R3, E:R9]
4. **Rig's serialization is not a resume story for Rollshot.** Pinned 0.39
   module docs state the format embeds the full conversation and “carries no
   cross-version stability guarantee yet: resume with the same rig version”
   (`rig-core-0.39.0/src/agent/run/mod.rs:1–22`); Rollshot does not persist it,
   and the reviewed persistence evidence shows serialization cannot answer
   side-effect/idempotency questions anyway. “Retain Rig to get future resume”
   is weak evidence. [E:R7; persistence document §4.2]
5. **The reference systems neither require nor discourage Rig.** Pi shows the
   loop can depend on one injected stream function; Codex shows wire coupling
   is a choice, not an inevitability; oh-my-pi shows registry breadth carries
   compatibility/test cost; Claude Code shows a single-provider product can
   ship without a neutral layer. Rollshot's existing facade already achieves
   the Pi-shaped boundary with Rig as one implementation underneath. *Inference:*
   the reference evidence informs what the contract should contain; it does
   not by itself argue for or against the library that currently sits beneath
   it. [E:P1, E:C1, E:O1, E:L1]

### 8.2 What accumulated evidence newly quantifies or weakens

1. **Upstream drift on the exact consumed surface is measurable and active**
   [A:G-DRIFT]. Between pinned 0.39.0 and the v0.40.0 checkout (one minor
   release):
   - `agent/run/mod.rs`: ~890 changed lines in a 1,734-line file (2,422 lines
     at v0.40): new `output_mode` module (`OutputMode`,
     `with_output_validation`, `with_output_tool_name`), hook module
     relocation (`prompt_request::hooks` → `agent::hook`),
     `UnknownToolCall` error restructuring, new invalid-tool-call budget
     tests.
   - `agent/run/streamed.rs`: ~78 changed lines: new
     `StreamedAssistantContent::Unknown` forwarding for unmodeled provider
     items (explicitly kept out of accumulation), `tool_result_user_content`
     → `tool_result_message` rename.
   - Provider stacks Rollshot streams through: `anthropic/completion.rs` ~257
     changed lines, `anthropic/streaming.rs` ~331, `openai/completion/
     streaming.rs` ~201; the 0.39 Anthropic `decoders/` directory (757 lines)
     no longer exists at v0.40.
   *Inference:* “retain” is not a static choice. Staying pinned accumulates
   unreviewed upstream delta on the consumed state machine and both provider
   stacks; upgrading requires re-reviewing roughly this volume per minor
   release against Rollshot's translation assumptions (for example, the
   wildcard arm in `drive_streamed_turn` would silently swallow a future
   added variant rather than fail to compile). Neither direction is free; the
   decision matrix needs an effort estimate, which this document does not
   provide.
2. **The fork/vendor surface is now quantified** [A:G-DRIFT]. Files containing
   the consumed 0.39 API total roughly: state machine + assembly 2,884 lines;
   message/completion/streaming core ≈3,880 lines
   (`completion/{mod,message,request}.rs`, `streaming.rs`); Anthropic provider
   stack ≈8,418 lines (including `decoders/`); OpenAI completions subset
   ≈4,083 lines. A fork could subset (state machine only, or machine plus the
   two provider stacks actually streamed), but the honest floor for
   “everything Rollshot touches” is on the order of 19k lines plus tests,
   not a weekend extraction.
3. **The provider breadth question is workload-gated, and current workloads
   are narrow.** Smart Redaction and Action Guide use exactly two providers
   through one streaming-completions path each; the deferred workload has no
   provider-breadth requirement in evidence. Umbrella decision discipline (no
   generality without a workload) applies to both “retain for Rig's many
   providers” and “replace with a broader library.” [E:R5, W1–W6]
4. **`test-utils` is enabled on the production dependency line**
   (`Cargo.toml:13`), so the pinned crate's test helpers are compiled into the
   production feature set. This is a minor hygiene item any disposition should
   record; it is not, by itself, evidence for any option.

### 8.3 Option-by-option evidence balance (non-selecting)

| Option | Evidence that strengthens | Evidence that weakens or complicates |
|---|---|---|
| **Retain** (pinned crate behind the private boundary) | Facade integrity holds [A:R-LEAK]; delegated invariants load-bearing and Rig-test-covered [E:R7]; consumed API pinned exactly with an upgrade-guard test (`rig_039_streamed_turn_api_compiles`); provider contract suite covers the Rollshot-owned edge [E:R9]. | Upstream drift on consumed files is active (~1,750 changed lines across the five consumed files in one minor release) [A:G-DRIFT]; pin-vs-upgrade review cost unquantified; serialization offers no cross-version resume; `test-utils` on the production dependency line. |
| **Fork/vendor** (copy the consumed code and evolve it) | Surface now quantified and subsettable [A:G-DRIFT]; Rollshot already owns the surrounding normalization and its tests [E:R2, E:R3]; upstream-compatibility reluctance is explicitly not a criterion (umbrella §2.2). | ≈19k-line floor plus provider security/protocol maintenance becomes Rollshot's; the six delegated invariants become Rollshot's to test adversarially; Anthropic/OpenAI streaming change cadence transfers fully in-house. |
| **Replace** (another library or bespoke component under the facade) | The facade already hides the implementation [A:R-LEAK]; the Rollshot-owned edge (errors, synthetic completion, stop rewriting) is already written and tested [E:R2, E:R9]; Pi demonstrates a one-function loop boundary is sufficient for a neutral contract [E:P1]. | Must re-prove the six delegated invariants plus stream-edge behaviors; a broader replacement library would repeat the generality Rig already provides without a workload; provider breadth beyond two vendors is unestablished [W1–W6]. |
| **Remove** (no library; own the small loop and two transports) | Current workloads need only a serial bounded loop, two streaming transports, and the already-owned normalization [E:R5, W1, W2]; Rig's general capabilities (serialization, many providers, output modes) are unused or unusable for Rollshot's resume questions [E:R7]. | Must own history threading/pairing, stream assembly/validation, and both SSE transports with adversarial tests; the v0.40 drift record shows provider streaming details change frequently [A:G-DRIFT]; deletion must not recreate a general state machine speculatively (umbrella discipline). |

### 8.4 What the Round 6 decision matrix must still resolve

1. An effort estimate for pin-versus-upgrade review per Rig release, using
   Section 8.2's per-file drift counts as the unit.
2. Whether any adopted future workload needs durable/resumable runs; if so,
   resume design is Rollshot-owned under every option, which downgrades
   serialization from a Rig benefit to a non-factor (persistence evidence).
3. Whether provider breadth beyond Anthropic/OpenAI is workload-justified;
   until then, both “Rig's providers” and “another library's providers” score
   zero (umbrella §10.4).
4. The adversarial test inventory each option must carry: the six delegated
   invariants, the stream-edge behaviors in `provider_contract.rs`, and the
   `model.rs` assembly tests.
5. Whether `test-utils` moves to dev-dependencies under the chosen option.

## 9. Candidate Rollshot patterns without final selection

Three materially different contract designs are compatible with the evidence.
None is selected; the decision matrix combines them with the Rig disposition
and the compaction/persistence dispositions.

### Pattern A — strict provider-erasure contract (extend the current direction)

Keep the normalized contract free of provider-identifying state: no provider
IDs, response IDs, signatures, or cache markers in `ModelMessage`; every run
self-contained; cross-run continuity comes only from Rollshot-owned records
(sessions, proposals, project manifests). Provider change = new run.

- **Fit evidence:** matches current implementation and the bounded workloads
  exactly [E:R1–E:R5, W1, W2]; smallest privacy surface (no opaque blobs to
  classify); switching has zero invalidation rules.
- **Trade-offs:** cannot represent provider-native continuity (thinking
  signatures, response IDs, prompt-cache breakpoints, deferred tool loading),
  so cache efficiency and some providers' features are unreachable; long
  deferred-workload horizons would re-send context or rely on re-projection
  (see `context-compaction.md` Pattern C); usage stays coarse without a
  `ModelUsage` extension.

### Pattern B — continuity-preserving normalized contract with opaque provider payloads (Pi/oh-my-pi-shaped)

Extend the normalized message model to retain provider identity plus opaque
per-item payloads (signatures, response IDs, cache markers) that Rollshot
stores but does not interpret; define per-pair handoff rules (for example,
Anthropic signatures dropped on a switch to OpenAI); let persistence and
compaction policy classify which payloads are retained.

- **Fit evidence:** Pi/oh-my-pi demonstrate the shape works across many
  providers while keeping one host contract [E:P1, E:O1, E:O2]; enables
  cache-aware accounting, deferred tool loading, and provider-native
  compaction options later [E:O3, E:C2].
- **Trade-offs:** larger contract and a new privacy classification task for
  opaque payloads (including redacted-thinking blobs); cross-provider
  invalidation rules are unsettled even in the reference (Pi profile Q5);
  without a workload that needs multi-turn provider continuity this is
  speculative generality today.

### Pattern C — capability-negotiating facade with Rollshot-owned model descriptors (Codex-shaped subset)

Add a small Rollshot-owned model descriptor (context window, vision, tool
calling, parallel calls, reasoning, cost rates) resolved per configured
provider/model; the driver consults it for output ceilings, attachment
eligibility, budget defaults, and — if later adopted — compaction triggers;
unsupported capabilities produce typed degradation before the request, not a
provider error after it.

- **Fit evidence:** Codex shows metadata-driven context windows and capability
  flags working in production [E:C1, E:C2]; fills the two bounded gaps found
  here (no context-window truth, no capability checks) [A:R-CTX, A:R-CAP];
  keeps negotiation host-owned, unlike Codex's wire-coupled version.
- **Trade-offs:** descriptor maintenance per model; Codex's own mismatch
  applies — configuration breadth can be mistaken for neutrality
  (profile §15); risks building negotiation for two known models beyond
  current workload need.

These patterns differ materially in information content (erasure vs opacity vs
negotiated metadata) and can combine (for example, C's descriptors with A's
erasure); combination is a decision-matrix question, not a foregone
conclusion.

## 10. Non-goals

- No Rig disposition is selected; Section 8 is an evidence update only.
- No provider beyond Anthropic/OpenAI is proposed; no workload establishes
  breadth.
- Do not equate endpoint/base-URL configurability with provider neutrality
  (Codex counterexample, §4.3).
- Do not treat a provider stop reason, stream end, or compaction event as a
  product terminal, budget verdict, or cancellation outcome.
- Do not treat compaction (any system's) as persistence, or a provider session
  state as a durable Rollshot session.
- Do not persist provider credentials, raw attachment bytes, or unclassified
  opaque provider payloads in a default configuration or audit store.
- Do not add capability negotiation, context policy, or opaque-payload
  retention without a workload that requires it.

## 11. Measurable evaluation criteria

Any later Rollshot design should be testable against these:

1. **Facade integrity:** `rig_core` appears in no public API type and in no
   file outside the named adapter/translation modules; a compile-fail test or
   lint enforces it.
2. **Round-trip fidelity:** a scripted multi-turn tool conversation
   reconstructs user/assistant/Tool call/result history faithfully through
   each adapter's request builder; property-tested, not example-tested.
3. **Terminal honesty:** every stream termination class (final, silent end,
   malformed item, HTTP error class, cancellation, deadline) maps to exactly
   one documented `ModelStreamEvent`/`ModelError` path; synthetic completions
   are distinguishable in tests from provider-reported ones.
4. **Usage truth:** reported usage is provider-reported or explicitly labeled
   estimated/unknown; a cost figure is never presented as enforced while no
   charging function exists.
5. **Context policy (if adopted):** window source, reservation, trigger, and
   overflow behavior are deterministic and unit-tested; overflow yields a
   typed outcome, never silent truncation of history or tool results.
6. **Handoff rules (if adopted):** a provider/model switch defines exactly
   which message and payload state survives; tests assert no cross-provider
   leakage of opaque state and no silent downgrade of attachments.
7. **Credential hygiene:** adapter `Debug`/log output contains no key, base
   URL, or payload bytes (partially test-covered today:
   `anthropic_api_key_not_in_debug`, `anthropic_base_url_not_in_debug` in
   `tests/provider_contract.rs`).

## 12. Evidence gaps and required spikes

1. Live-provider verification of stream edge behavior (silent stream end,
   usage timing, cache-token reporting, reasoning items) for Anthropic and
   OpenAI; `provider_contract.rs` is wiremock fixture evidence, not live
   observation.
2. A Rig pin-versus-upgrade effort measurement: apply the 0.39→0.40 diff to a
   branch and record review/test time, feeding Section 8.4 item 1.
3. If Pattern B is ever considered: a privacy-classification spike for opaque
   provider payloads (thinking signatures, redacted-thinking blobs, response
   IDs), including retention and cross-provider invalidation.
4. If any context policy is adopted: token-estimation accuracy per provider
   (local estimate versus provider-reported usage) and an overflow-terminal
   spike, since no overflow path exists today [A:R-CTX, A:R-STOP].
5. If a third provider is ever proposed: which contract fields (Pattern A vs
   B) the new wire requires, demonstrated by one vertical adapter spike.

### 12.1 Bounded absence and semantic audits

- **[A:R-CTX] Rollshot context-window/compaction audit.** Roots:
  `crates/rollshot-agent/src/{domain,driver,model,provider,runtime,tools}.rs`
  and `crates/rollshot-app/src/result_workspace/workbench`. Exact group:
  `compact|compaction|context[_ -]?(window|limit|length)|reserve[_ -]?tokens|token[_ -]?limit|max[_ -]?context`. Hits were the `max_tokens`
  request field and its 4,096 default, budget token dimensions,
  `BudgetError::Overflow`, and an instant-arithmetic comment. No model
  context-window metadata, reservation, threshold trigger, or compaction
  mechanism was **found in the investigated scope**.
- **[A:R-CAP] Rollshot capability-negotiation audit.** Same roots. Exact
  group: `capabilit|negotiat|supports_|vision[_ -]?capab|model[_ -]?(info|descriptor|metadata)|deferred[_ -]?tool`. Hits were automation
  capability handles in `ToolContext` (a different concept, excluded by
  direct inspection). No model capability descriptor, vision/reasoning/
  parallel-tool check, deferred tool loading, or unsupported-capability
  degradation was **found in the investigated scope**.
- **[A:R-HANDOFF] Rollshot provider handoff audit.** Roots:
  `crates/rollshot-agent/src`,
  `crates/rollshot-app/src/result_workspace/workbench`,
  `crates/rollshot-app/src/timeline_workspace/update.rs`. Exact group:
  `handoff|switch[_ -]?provider|provider[_ -]?change|with_history|resume|restore[_ -]?session`. Positive evidence: `build_adapter` is invoked per
  run/call (`workbench/run.rs:780`; `timeline_workspace/update.rs:1162`,
  `1396`) from the current `ProviderConfig`; `with_history` has zero hits.
  Mid-run provider switching, cross-run provider-continuation state, and a
  handoff mechanism were **not found in the investigated scope**.
- **[A:R-LEAK] Rollshot facade integrity audit.** Roots:
  `crates/rollshot-agent/src`. Exact group: `rig_core`. Hits are confined to
  `driver.rs`, `model.rs`, `provider.rs`; `src/lib.rs` declares
  `pub(crate) mod provider` and re-exports four items. A case-insensitive
  `rig` scan of the remaining agent sources returned only false positives
  (`right_strip`, `origin`). No Rig type appears in the public facade in the
  investigated scope (positive confirmation, not an absence claim).
- **[A:R-STOP] Rollshot stop-reason construction audit.** Roots:
  `crates/rollshot-agent/src`. Exact group: `StopReason::(MaxTokens|Unknown)`.
  Zero construction hits; streamed turns resolve to `EndTurn`/`ToolUse` via
  `model.rs:384–388` and `provider.rs:329–342`, `380–384`. The `MaxTokens`
  and `Unknown(String)` variants are declared but unconstructed in the
  investigated scope; a dedicated context-overflow error class was likewise
  **not found**.
- **[A:G-DRIFT] Rig 0.39→0.40 consumed-surface drift audit.** Roots: locally
  resolved `rig-core-0.39.0` (Cargo.lock checksum in the evidence baseline)
  versus `learn-projects/rig` at
  `2f37dfcd0156bdceab3eabe6f0a953f9202e2d77`. Method: `diff -u` on the files
  containing the consumed API, counting `+`/`-` lines. Results:
  `agent/run/mod.rs` ~890 (new `output_mode` module, hook relocation,
  `UnknownToolCall` restructuring); `agent/run/streamed.rs` ~78
  (`StreamedAssistantContent::Unknown` forwarding, `tool_result_message`
  rename); `providers/anthropic/completion.rs` ~257;
  `providers/anthropic/streaming.rs` ~331; `providers/openai/completion/
  streaming.rs` ~201; the 0.39 `anthropic/decoders/` tree (757 lines) is
  absent at v0.40. Line floors: consumed-API files total ≈19k lines at 0.39
  (state machine 2,884; completion/message/streaming core ≈3,880; Anthropic
  stack ≈8,418; OpenAI completions subset ≈4,083). Counts are diff-line
  approximations, not semantic review; the local registry path is
  machine-specific but version- and checksum-pinned.
- **[A:C-WIRE] / [A:L-PROVIDER] reference-provider absence claims.** Codex's
  non-Responses wire absence and Claude Code's third-party adapter absence
  are inherited from the reviewed profiles' bounded audits (Codex profile §13
  audit A7; Claude profile §13 bounded to its named orchestration/provider
  files). They are not re-derived here and remain scoped to those profiles'
  roots and terms.

## 13. Evidence index

Graph-first discovery on the Rollshot repo (7,979 nodes, 65,744 edges, 405
files) located `ProviderAdapter`, both adapters, `ModelRequest`/`ModelMessage`/
`ModelStreamEvent`, `push_model_messages`, the driver request builders, and the
workbench provider-config path before direct source inspection. Equivalent
graph queries for each ignored reference root returned zero nodes, so pinned
source trees were inspected with bounded searches.

| ID | Type | Status | Pinned source / symbol | Supports / limit |
|---|---|---|---|---|
| R1 | graph + source + test source | current Rollshot | `crates/rollshot-agent/src/model.rs`: `ModelRequest`, `ModelMessage`, `ModelStreamEvent`, `ModelUsage`, `StopReason`, `ModelError`, `push_model_messages`, `drive_streamed_turn` | Public facade shapes, usage fields, stream-item erasure, declared stop variants. Static; no provider executed. |
| R2 | graph + source + test source | current Rollshot | `crates/rollshot-agent/src/provider.rs`: `ProviderAdapter`, adapters, `build_completion_request`, `rig_to_model_error`, `stream_to_model_events` | Private Rig translation, normalization Rollshot owns, defaults (4,096), `parallel_tool_calls: false`, synthetic completion, stop rewrite. |
| R3 | graph + source + test source | current Rollshot | `crates/rollshot-agent/src/driver.rs`: `run_model_turn_with_provider`, `drive_streamed_turn`, visual-annotation attachment path | Request construction (empty attachments on Smart Redaction; first-turn attachments on visual annotation), deadline bounding, Rig driving. |
| R4 | source + test source | current Rollshot | `crates/rollshot-app/src/result_workspace/workbench/provider_config.rs` | Provider/model/key config, env-name-only custody, adapter construction, default base URLs. |
| R5 | source | current Rollshot | `crates/rollshot-app/src/result_workspace/workbench/run.rs:780`; `timeline_workspace/update.rs:1162`, `1396` | Per-run/per-call adapter construction; selection-not-handoff boundary. UI paths not launched. |
| R6 | source + reviewed baseline | current Rollshot | `crates/rollshot-action` project model/store via baseline A1/A2; caption/visual-annotation proposal paths | Durable product records are product-owned, separate from provider state. |
| R7 | source + test source | pinned rig-core 0.39.0 | `rig-core-0.39.0/src/agent/run/{mod.rs,streamed.rs}`: module docs lines 1–22, `AgentRun`, assembler | Sans-I/O serializable machine, no cross-version stability, consumed invariants. Local registry path machine-specific; checksum-pinned. |
| R8 | reviewed capability evidence | current Rollshot | `budgets-cancellation-retries.md` §3 and its audits | Token/cost budget behavior, `max_turns` mapping, cost never charged. |
| R9 | test source | current Rollshot | `crates/rollshot-agent/tests/provider_contract.rs` | Wiremock SSE contract coverage: text/tool streaming, usage, unknown events, malformed JSON, incomplete stream, 401/429/500, endpoint shapes, debug redaction. Mocks, not live providers. |
| P1 | source + reviewed profile | pinned Pi | `packages/ai/src/types.ts` (`Context`, `Message`, `AssistantMessage`, `Usage`, `StopReason`, `AssistantMessageEvent`), `models.ts`; profile §13/§16 | Continuity-preserving neutral contract, stream terminal protocol, cache/cost-aware usage, model-change persistence, open cross-provider question. |
| P2 | source | pinned Pi | `packages/ai/src/types.ts:468–480` | done/error terminal-message protocol; failure-as-message semantics. |
| P3 | source + reviewed profile | pinned Pi | `packages/coding-agent/src/core/compaction/compaction.ts:128–134`; profile §6 | `reserveTokens` 16,384 trigger, overflow recovery. Tests not executed. |
| O1 | source + reviewed profile | pinned oh-my-pi | profile §13; `packages/ai/src/api-registry.ts`; `packages/catalog` descriptors | 14 reserved API IDs, 61 catalog providers, extension API names, registry breadth and its cost. |
| O2 | source | pinned oh-my-pi | `packages/ai/src/types.ts:420–430`, `665–690`, `749`; `packages/catalog/src/types.ts:95–119` | `providerPayload`, `providerSessionState`, `attribution`, orchestration/premium usage. |
| O3 | source + reviewed profile | pinned oh-my-pi | profile §6 strategy table; compact-modes sources | Trigger set, remote compaction with fallback, snapcompact vision requirement, handoff semantics. |
| C1 | source + reviewed profile | pinned Codex | `codex-rs/model-provider/src/provider.rs:101`, `464–467`; `model-provider-info/src/lib.rs:57–111`; profile §13 | `ModelProvider` trait, capability/context metadata, Responses-only wire, Bedrock boundary, bounded non-Responses absence. |
| C2 | source + reviewed profile | pinned Codex | `core/src/session/turn_context.rs:208–215`; `core/src/state/session.rs:38`, `190–198`; profile §6 | Percent-scaled context window, auto-compact windows, local/remote compaction split (v2 stable default-on, 64k retained budget, two retries). |
| C3 | reviewed capability evidence | pinned Codex | `persistence-checkpoint-resume.md` §5.1 Codex row | Resume-under-different-model warning; sticky/live provider state not reconstructed. |
| L1 | source + reviewed profile | pinned Claude Code | profile §13 | Anthropic-shaped contract, bounded third-party-adapter absence, MCP/bridge non-equivalence. |
| L2 | source + reviewed profile | pinned Claude Code | profile §6, §7 | Compaction reserves/thresholds, circuit breaker, thinking management, internal API clearing gates, model restored on resume. Hidden reducers remain unverifiable. |
| W1 | source + test source | current product | Smart Redaction workbench and agent terminal paths | Bounded review-producing run workload. |
| W2 | source + test source | current product | Action Guide annotation/caption/provider-config paths | Independent bounded calls around durable project state. |
| W3–W6 | source | deferred references | brag `skills/brag/SKILL.md`; Hyperframes `production-loop.md`, `subagent-dispatch.md`, `review-loop.md` at the evidence-baseline hashes | Multi-stage artifact workflow pressures; not Rollshot behavior and not a provider-breadth requirement. |

**Confidence:** high for Rollshot facade shapes, adapter normalization,
config/credential custody, and the pinned-revision reference claims re-verified
here; medium for the bounded absences (scoped roots/terms in §12.1) and the
diff-line drift quantification (approximation, not semantic review); low-to-
medium for live provider stream behavior, upgrade effort, and any
runtime-gated reference behavior not executed. Reviewed profiles and
capability documents were used for routing and contradiction checks; focused
claims were rechecked against pinned sources.
