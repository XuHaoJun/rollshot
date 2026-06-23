# Bounded Agent Core Handoff

**Completed subproject:** Parent §12, Subproject 4 — Bounded Agent Core
**Crate:** `rollshot-agent`

## Public API

### Domain (`domain.rs`)

- `SessionId`, `RunId` — opaque newtype IDs with `new(u64)` / `get()`.
- `MediaType` — `Png | Jpeg`, with `from_mime(&str) -> Option<Self>`.
- `AttachmentDescriptor` — media type, dimensions, byte count.
- `AuthorizedInputManifest` — provider, model, descriptors; `total_bytes()`.
- `AuthorizedModelInput` — validated constructor `new(provider, model, user_message, descriptors, attachment_bytes) -> Result<Self, InputError>`. Debug impl redacts user text and attachment bytes.
- `AgentSession` — in-memory exchange log: `push_user`, `push_assistant`, `exchanges()`.
- `InputError` — `DescriptorMismatch | UnsupportedMediaType | AttachmentCountOverflow | PerAttachmentOverflow | TotalByteOverflow`.
- `SessionError` — `IncompleteTurn`.

### Model types (`model.rs`)

All provider-neutral; no Rig types leak.

- `ModelRequest`, `ModelMessage`, `ModelStreamEvent`, `ModelUsage`, `ModelCompletion`, `StopReason`, `ModelError`, `ToolDefinition`.

### Provider adapters (`provider.rs`)

- `ProviderAdapter` trait — `stream(request, bounds) -> Stream<StreamEvent>`.
- `AnthropicAdapter::new(api_key) -> Self`.
- `OpenAIAdapter::new(api_key) -> Self`.
- `StreamBounds` — cancellation flag + deadline.

### Runtime (`runtime.rs`)

- `RunBudget` — 16 budget dimensions; `unlimited()` constructor.
- `BudgetDimension` — `WallTime | ModelCalls | InputTokens | OutputTokens | Cost | ToolCalls | PerToolCalls | ArgumentBytes | ResultBytes | SourceBytes | Attachments | ValidationAttempts | DryRunAttempts | CapabilityCalls | CandidateCount | AffectedArea`.
- `BudgetTracker` — `new(budget)`, `check_*` methods, `commit_charge()`.
- `UsageSnapshot` — current usage counters.
- `RunCancellation` — `new()`, `cancel()`, `is_cancelled()`, `token()`, `automation_flag()`.
- `RunEvent` — `TextChunk | ToolCallStart | ToolCallEnd | TurnComplete`.
- `RunEventSink` trait, `NullEventSink`.

### Tools (`tools.rs`)

- `Tool` trait — `name()`, `json_schema()`, `call(arguments) -> ToolFuture`.
- `ToolRegistry` — `new(limits)`, `register(tool)`, `dispatch(call)`.
- `ToolRegistryLimits` — `max_argument_bytes`, `max_result_bytes`, `per_tool_call_limit`; `permissive()` default (256 KiB / 256 KiB / u32::MAX).
- `ToolContext` — `draft`, `source`, `validation_limits`, `execution_policy`, `automation_cancellation`, `session_id`, `image_dims`.
- `ToolError`, `ToolOutcome`, `ToolCall`.

### Driver (`driver.rs`)

- `AgentConfig` — `max_turns`, `max_assistant_bytes`, `max_argument_bytes`, `max_result_bytes`.
- `AgentRunner::new(config)`, `run(input, session, registry, budget, cancellation, event_sink, tool_ctx, model_turn_fn) -> RunTerminalState`.
- `RunTerminalState` — `ReadyForReview | NeedsUserInput | Cancelled | BudgetExhausted | SourceValidationFailure | RuntimeFailure | AgentProtocolFailure | ProviderFailure`.
- `ReadyForReview` — `session_id`, `assistant_text`, `generation`, `usage`.
- `NeedsUserInput` — `session_id`, `generation`, `assistant_text`.
- `DriverError` — `BudgetExhausted | Cancelled | ProviderFailure | AgentProtocolFailure`.

## Default budget values (`AgentConfig::default`)

| Field | Default |
|---|---|
| `max_turns` | 10 |
| `max_assistant_bytes` | 4 MiB |
| `max_argument_bytes` | 256 KiB |
| `max_result_bytes` | 256 KiB |
| `DEFAULT_MAX_TOKENS` (provider) | 4096 |

`RunBudget::unlimited()` sets all 16 dimensions to their type MAX.

## Rig version

`rig-core = "=0.39.0"` — pinned exact. Features: `test-utils`.

## Fixture source and scrub procedure

**File:** `crates/rollshot-agent/tests/fixtures/provider_streams.json`

Fixtures are hand-reconstructed from public API documentation:

- **Anthropic fixtures** sourced from `https://docs.anthropic.com/en/api/messages-streaming`, retrieved 2026-06-23. Event names: `message_start`, `content_block_start`, `content_block_delta`, `content_block_stop`, `message_delta`, `message_stop`.
- **OpenAI fixtures** sourced from OpenAI Chat Completions streaming docs.

**Scrub procedure:** All IDs replaced with synthetic `msg_test_*` / `toolu_test_*` / `chatcmpl-test-*` values. Model names replaced with `claude-sonnet-4-6` / `gpt-4o`. Text content replaced with test payloads. No real API keys, account IDs, or session tokens present. Each fixture records `provenance.source_url`, `retrieved` date, `original_event_names`, and `substitutions` description.

## Test commands and results

```
rtk cargo test -p rollshot-agent     → 139 passed (3 suites, 0.20s)
rtk cargo fmt --check               → PASS
rtk cargo clippy -p rollshot-agent --all-targets -- -D warnings → PASS
rtk cargo test --workspace          → 1198 passed, 5 ignored (59 suites, 28.15s)
rtk cargo clippy --workspace --all-targets -- -D warnings → PASS
```

## Unsupported inspection capabilities

The `InspectionProvider` trait and vision capabilities (`templateMatch`, `regionFeatures`, `inspectLayout`) are defined in `rollshot-automation` but return `capability_unavailable` in the production `RealAutomationHost`. Real adapters are deferred; they must prepare bounded results outside QuickJS and keep host callbacks under 1 ms.

## No persistence/resume guarantee

`AgentSession`, `DraftState`, and `ToolContext` are in-memory only. No serialization to disk. No resume-after-interruption. If the process exits, session state is lost. Future persistence (Subproject 5) must store canonical source, Workflow IR, schema versions, capability manifest, static cost, validation limits, validation summary, and revision provenance — but not oxc ASTs, runtime contexts, raw OCR, or raw host results.

## Product integration follow-ups

1. **Vision queries:** The prepared template-match and region-features queries from `rollshot-vision` need product wiring to call `prepare_template_match` / `prepare_region_features` before the QuickJS callback and cache results under canonical keys.
2. **Review UI:** `ReadyForReview` and `NeedsUserInput` terminal states require a product UI to display assistant text, show the edit proposal, collect user input, and resume the session.
3. **Provider configuration:** API keys are passed directly to `AnthropicAdapter::new` / `OpenAIAdapter::new`. Product layer must source these from config/keychain and handle missing-key UX.
4. **Event sink:** `RunEventSink` receives `TextChunk`, `ToolCallStart`, `ToolCallEnd`, `TurnComplete`. Product must wire these to the overlay/preview UI for live progress.
5. **Budget tuning:** Default budgets are conservative. Product should expose budget configuration and tune `max_turns`, token limits, and cost ceiling per provider/model.

## Privacy boundary

- No public API exposes Rig types (`rig_core` is used only internally in `provider.rs`, `driver.rs`, `model.rs`).
- No `println!`, `eprintln!`, or `dbg!` in production code.
- All `tracing` calls use stable `rollshot::agent::driver` targets with structured fields; no sensitive payloads (session IDs, provider/model names, budget dimensions only).
- `AuthorizedModelInput::Debug` redacts user text and attachment bytes.
