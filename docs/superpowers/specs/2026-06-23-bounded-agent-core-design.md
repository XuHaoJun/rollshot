# Smart Redaction Bounded Agent Core (Parent Subproject 4) — Design

**Date:** 2026-06-23  
**Status:** Approved design  
**Parent design:** `docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md`  
**Dependency handoff:** `docs/superpowers/handoffs/2026-06-21-automation-frontend-runtime.md`  
**Spike decisions:** `docs/superpowers/spikes/2026-06-20-spike-decisions.md`

> This is parent-design Subproject 4, the **Bounded Agent Core** (BAC). It is
> unrelated to vision roadmap SP4 (`inspectLayout`). Vision SP3 template
> acquisition depends on BAC, but is not implemented by this subproject.

## 1. Summary

BAC provides the provider-neutral, bounded control plane that lets Rollshot
drive a multimodal model through typed tools without surrendering execution
control to a provider SDK or allowing the model to mutate product state.

The subproject adds one crate, `crates/rollshot-agent`, which owns:

- in-memory agent sessions and runs;
- a Rollshot-owned model facade with Anthropic and OpenAI adapters;
- streamed assistant text;
- manual Rig `AgentRun` driving;
- a typed, availability-aware tool registry;
- run budgets, cancellation, events, and terminal states;
- a run-local automation draft;
- validation, dry-run, and submit-for-review orchestration; and
- production outputs for `ReadyForReview` and `NeedsUserInput`.

BAC does not persist sessions or revisions, render UI, implement OCR/layout
algorithms, perform vision SP3 template acquisition, mutate `ImageDocument`, or
retry provider requests automatically.

The first end-to-end acceptance slice is a model-authored automation:

```text
inspect
  → replace full automation source
  → validate
  → dry-run
  → submit
  → ReadyForReview(DraftAutomation + EditProposal)
```

The same driver must also terminate correctly for user clarification,
cancellation, exhausted budgets, provider failures, source failures, and
runtime failures.

## 2. Locked Decisions and Scope

### 2.1 Dependency decisions

The retained Rig spike locks these choices:

- `rig-core = "=0.39.0"`;
- manual `AgentRun` driving through `next_step`, model responses, and tool
  results;
- no use of Rig's high-level `agent.prompt()` control loop;
- a Rollshot-owned public model facade;
- runtime provider selection behind that facade; and
- no Rig types in public BAC APIs.

BAC closes the spike's remaining provider risk with recorded Anthropic and
OpenAI wire fixtures covering tool schemas, streamed text, streamed tool-call
arguments, normalized calls, usage, completion, and errors.

### 2.2 In scope

- `crates/rollshot-agent`.
- Anthropic and OpenAI production provider adapters.
- Provider/model identity fixed for one run.
- Text streaming to product events.
- Complete-tool-call assembly before execution.
- In-memory `AgentSession` and `AgentRun` domain models.
- Typed authoring and inspection tool contracts.
- Tool availability and stable unavailable results.
- Response-order serial tool execution.
- Run-local mutable full-source draft.
- Automation validation and dry-run using existing crates.
- Explicit `submit_for_review`.
- Typed `request_user_input`.
- Hard budgets and cooperative cancellation.
- Privacy-safe diagnostics and event records.
- Scripted model tests and recorded provider fixtures.

### 2.3 Explicitly out of scope

- Filesystem, database, or cloud persistence.
- Immutable `Preset` / `AutomationRevision` storage.
- Session resume implementation.
- Preset Workbench or Result Workspace UI.
- Upload-consent UI or provider selection UI.
- Candidate editing and proposal application.
- Direct `ImageDocument` mutation.
- Vision SP3 template acquisition orchestration.
- Vision SP4 `inspectLayout` implementation.
- Vision SP5 OCR implementation.
- Product query-plan extraction and vision preparation.
- Automatic provider retry or failover.
- Live API credentials as a completion requirement.
- General-purpose agent tools or arbitrary native tool execution.

Subproject 5 owns persistence. Subproject 6 owns Workbench UI. Vision SP3
consumes BAC after this subproject is complete.

## 3. Architecture

### 3.1 Crate and dependency direction

```text
rollshot-app / future Preset Workbench
        |
        | AuthorizedModelInput + configured ToolRegistry
        v
rollshot-agent
        +-- session/run domain
        +-- bounded driver
        +-- tool registry
        +-- provider adapters (private Rig integration)
        |
        +--> rollshot-automation
        +--> rollshot-edit-proposal
        +--> rollshot-vision public interfaces
        +--> rig-core = 0.39.0
```

`rollshot-agent` is the only new crate. Rig remains private to provider/driver
modules. The public API uses Rollshot-owned request, response, event, budget,
error, and terminal-state types.

The automation and proposal crates remain framework-neutral:

- `rollshot-automation` does not gain provider, session, or model concerns.
- `rollshot-edit-proposal` remains the typed review boundary.
- `rollshot-vision` remains deterministic and agent-independent.
- `rollshot-app` supplies authorization and later renders events/results.

If provider integration later becomes large enough to justify a separate
adapter crate, it may be split without changing the public BAC model.

### 3.2 Suggested module boundaries

```text
crates/rollshot-agent/src/
  lib.rs
  domain.rs          # session/run IDs, messages, draft, terminal outputs
  driver.rs          # manual AgentRun state machine
  event.rs           # append-only product-facing run events
  budget.rs          # limits, usage, charging
  cancellation.rs    # cooperative cancellation boundary
  model.rs           # RollshotModel and normalized stream events
  provider/
    mod.rs
    anthropic.rs
    openai.rs
  tool/
    mod.rs            # registry and common tool contracts
    inspection.rs
    automation.rs
  error.rs
```

Each module has one primary responsibility. Provider modules may depend on Rig;
domain, budget, event, and tool contracts must not.

## 4. Domain Model

### 4.1 Sessions and runs

BAC stores session state in memory:

```rust
pub struct AgentSession {
    pub id: AgentSessionId,
    pub messages: Vec<SessionMessage>,
    pub runs: Vec<AgentRunSummary>,
}

pub struct AgentRun {
    pub id: AgentRunId,
    pub session_id: AgentSessionId,
    pub authorized_input_manifest: AuthorizedInputManifest,
    pub status: AgentRunStatus,
    pub budget_usage: RunBudgetUsage,
    pub tool_events: Vec<ToolEvent>,
    pub draft: DraftState,
}
```

These are representative shapes, not permission to persist sensitive runtime
payloads. Domain records should support later serialization where safe, but BAC
does not provide a repository or write them to disk.

`SessionMessage` contains completed user and assistant messages suitable for
continued in-memory history. Transient attachment bytes, provider wire frames,
partial assistant text, and sensitive tool payloads are not session messages.

### 4.2 Authorized model input

The caller completes upload disclosure and authorization before starting BAC:

```rust
pub struct AuthorizedModelInput {
    pub provider: ProviderId,
    pub model: ModelId,
    pub manifest: AuthorizedInputManifest,
    pub attachments: Vec<TransientAttachment>,
}
```

The manifest records the authorized text/attachment descriptors and the fixed
provider/model. Attachment bytes are transient run inputs.

BAC must not:

- read additional image/document state;
- enlarge the authorized region or payload;
- add an attachment not present in the manifest;
- change provider or model during the run; or
- place attachment bytes in session records, events, errors, or tracing.

### 4.3 Draft state and generations

One run owns one mutable full-source draft:

```rust
pub struct DraftState {
    pub generation: u64,
    pub source: Option<String>,
    pub validation: Option<GenerationEvidence<ValidatedAutomation>>,
    pub dry_run: Option<GenerationEvidence<DryRunEvidence>>,
}
```

`replace_automation_source` replaces the entire source and increments
`generation`. Replacement invalidates every validation, policy, dry-run,
proposal, and submission result from older generations.

`validate_automation`, `dry_run_automation`, and `submit_for_review` only act on
the current generation. Evidence includes its source generation so stale
results cannot be accepted accidentally.

### 4.4 Ready-for-review output

Successful submission produces an immutable, in-memory handoff:

```rust
pub struct DraftAutomation {
    pub source: String,
    pub validated: ValidatedAutomation,
    pub validation_summary: ValidationSummary,
    pub dry_run: DryRunEvidence,
}

pub struct ReadyForReview {
    pub automation: DraftAutomation,
    pub proposal: EditProposal,
    pub budget_usage: RunBudgetUsage,
}
```

BAC does not assign a persistent automation revision ID or activate a preset.
Subproject 5 turns a reviewed draft into an immutable stored revision.

### 4.5 Terminal states

```rust
pub enum RunTerminalState {
    ReadyForReview(ReadyForReview),
    NeedsUserInput(UserInputRequest),
    BudgetExhausted(BudgetExhaustedReport),
    ProviderFailure(ProviderFailureReport),
    AgentProtocolFailure(AgentProtocolFailureReport),
    SourceValidationFailure(SourceValidationFailureReport),
    RuntimeFailure(RuntimeFailureReport),
    UserCancelled(CancellationReport),
}
```

A terminal state is assigned once. No model or tool work proceeds afterward.

`ReadyForReview` requires:

- explicit successful `submit_for_review`;
- current-generation source;
- current-generation successful frontend validation and static policy checks;
- current-generation successful sandbox dry-run;
- valid candidate output and proposal policy;
- no exhausted run budget; and
- no pending or observed cancellation.

## 5. Event Model

BAC emits append-only events for a future UI subscriber:

```text
RunStarted
TextDelta
AssistantMessageCompleted
ToolCallStarted
ToolCallCompleted
ToolCallFailed
DraftReplaced
ValidationCompleted
DryRunCompleted
BudgetUpdated
Terminal
```

Events contain stable IDs, ordering sequence numbers, timestamps/durations where
needed, and privacy-safe summaries. They do not contain attachment bytes,
provider credentials, complete prompts, raw provider frames, or sensitive tool
payloads.

`TextDelta` is display-only transient state. BAC appends an assistant message to
the session only after the provider turn completes successfully. If a request
is cancelled or fails, partial text may remain visible for that live run but is
not committed to session history and is not replay input.

Tool calls are not emitted as executable events until their complete name and
arguments have been assembled and validated.

## 6. Provider and Streaming Model

### 6.1 Public model facade

The Rollshot-owned facade streams normalized events:

```rust
pub trait RollshotModel: Send + Sync {
    fn provider(&self) -> ProviderId;
    fn model(&self) -> ModelId;

    async fn stream(
        &self,
        request: ModelRequest,
        cancellation: CancellationFlag,
    ) -> Result<ModelEventStream, ModelError>;
}
```

The exact async trait and stream types are implementation details, but public
types remain independent of Rig and provider SDK types.

### 6.2 Normalized stream

Provider adapters normalize wire events into:

```rust
pub enum ModelStreamEvent {
    TextDelta(String),
    ToolCallDelta(ToolCallFragment),
    UsageDelta(ModelUsage),
    Completed(ModelCompletion),
}
```

Data flow:

```text
Anthropic/OpenAI stream
    |
    v
provider-specific parser
    |
    v
ModelStreamEvent
    +-- TextDelta -----> RunEvent::TextDelta
    +-- ToolCallDelta -> private call assembler
    +-- UsageDelta ---> budget charge + BudgetUpdated
    +-- Completed ----> complete-message/tool validation
                              |
                              v
                         Rig ModelTurn
                              |
                              v
                    AgentRun::model_response(...)
```

BAC does not stream partial tool calls to the registry. Tool name, call ID, and
arguments must be fully assembled. Arguments must decode as JSON and pass the
registered tool schema before execution.

Malformed, duplicate, incomplete, or unsupported tool calls end in
`AgentProtocolFailure`. A provider stream ending without its required completion
signal is incomplete and fails rather than being treated as a valid turn.

### 6.3 Anthropic and OpenAI adapters

Both adapters are production code and must:

- map the same Rollshot model request into provider-specific request bodies;
- encode registered tool definitions correctly;
- parse text deltas;
- assemble tool calls whose arguments span multiple stream frames;
- normalize stop/completion reasons;
- map usage accounting;
- distinguish transport, rate-limit, authentication, provider rejection, and
  malformed-response failures; and
- respond to cancellation without leaking partial payloads into durable state.

Recorded fixtures are the completion gate. Live API tests are optional because
they require credentials and transmit data.

### 6.4 Retry policy

BAC performs no automatic provider retry or provider failover.

Any provider or transport failure terminates as `ProviderFailure`, including a
failure before the first delta. This prevents duplicate cost, ambiguous partial
messages, and replay of tools with side effects. A future UI may offer an
explicit user retry that starts a new run.

## 7. Manual Agent Driver

BAC drives Rig one step at a time:

```text
AgentRun::next_step()
    |
    +-- CallModel -> BAC streams one provider response
    |                  -> assemble ModelTurn
    |                  -> AgentRun::model_response(...)
    |
    +-- CallTools -> BAC validates and executes calls serially
    |                  -> AgentRun::tool_results(...)
    |
    +-- Done -> requires a BAC terminal tool/result
```

The driver, not Rig, owns:

- provider invocation;
- event emission;
- budget charging;
- cancellation;
- tool availability and execution;
- draft/evidence state;
- terminal-state validation; and
- product error classification.

The driver must not accept an ordinary text response as `ReadyForReview` or
`NeedsUserInput`. Domain terminal states require the corresponding typed tool.

## 8. Tool Registry

### 8.1 Tool contract

Every tool has:

- a stable name and version;
- a JSON input schema exposed to providers;
- typed request/response decoding;
- an availability state;
- a privacy classification;
- a per-run call limit;
- timeout and cancellation behavior; and
- privacy-safe event summaries.

Unknown tools and schema-invalid arguments are protocol failures. Known but
unavailable tools return a stable typed unavailable result to the agent.

### 8.2 First-release tools

Inspection:

- `inspect_context_summary`
- `inspect_ocr`
- `inspect_layout`
- `inspect_region_features`

Automation authoring:

- `replace_automation_source`
- `validate_automation`
- `dry_run_automation`
- `submit_for_review`
- `request_user_input`

The complete protocol is defined now even when a production adapter is absent.
OCR/layout tools may be registered as unavailable until vision SP4/SP5 supply
their implementations. An unavailable inspection tool must not return an empty
success, because that would mislead the model into interpreting missing
capability as negative evidence.

### 8.3 Serial execution

If one provider turn contains multiple tool calls, BAC executes them in provider
response order, serially.

Before each call it rechecks:

- cancellation;
- terminal state;
- wall-clock deadline;
- tool availability;
- tool call budget; and
- tool-specific preconditions.

This ordering makes mutable draft operations deterministic. A successful
terminal tool (`request_user_input` or `submit_for_review`) stops execution of
remaining calls in the same turn.

### 8.4 Automation tool rules

`replace_automation_source`

- accepts one complete source string;
- enforces source-byte limits;
- increments generation;
- invalidates prior evidence.

`validate_automation`

- requires current source;
- runs `rollshot_automation::validate_source`;
- performs configured static policy checks;
- records source diagnostics and current-generation evidence.

`dry_run_automation`

- requires successful current-generation validation;
- uses the caller-configured `AutomationExecutor`, `AutomationHost`,
  `ExecutionPolicy`, proposal context, and cancellation;
- records execution metrics and the validated `EditProposal`;
- never applies proposal operations to `ImageDocument`.

`submit_for_review`

- requires complete current-generation evidence;
- validates proposal policy one final time;
- creates `ReadyForReview`;
- is terminal.

### 8.5 Typed user clarification

`request_user_input` accepts a bounded structure:

```rust
pub struct UserInputRequest {
    pub question: String,
    pub reason: String,
    pub choices: Vec<UserInputChoice>,
    pub visual_selection: Option<VisualSelectionRequest>,
}
```

The driver bounds string lengths, choice count, and selection request type.
Once valid, the tool immediately terminates the run as `NeedsUserInput`.

Assistant prose or a marker embedded in text cannot produce this state.

## 9. Budgets and Resource Control

BAC owns `RunBudget` and `RunBudgetUsage`. Limits include:

- model call count;
- wall-clock deadline;
- input tokens;
- output tokens;
- estimated provider cost;
- per-tool and aggregate tool calls;
- source bytes;
- validation attempts;
- dry-run attempts;
- automation capability calls;
- candidate output count; and
- total candidate area.

Budget checks occur before and after provider/tool work where usage becomes
known. Usage updates emit `BudgetUpdated`.

If a provider reports cumulative usage, BAC computes and charges the positive
delta exactly once. Missing provider usage is not treated as zero: the adapter
must either provide a documented conservative estimate or terminate with a
typed accounting failure according to configured policy.

Exhaustion produces `BudgetExhausted`. The report may reference the last valid
draft evidence for diagnostics, but exhausted runs cannot become
`ReadyForReview`.

## 10. Cancellation

One run cancellation signal covers:

- an in-flight provider stream;
- pending asynchronous tools; and
- sandbox dry-run execution.

Cancellation is cooperative. Before each state transition, model call, tool
call, and terminal result, BAC checks cancellation.

Some synchronous Rust host callbacks cannot be pre-empted while executing.
Cancellation takes effect immediately after such a callback returns. This
limitation is recorded in diagnostics and must not be presented as immediate
pre-emption.

Cancellation produces `UserCancelled`. It does not commit partial assistant
text, tool results that did not complete, or a review-ready draft.

## 11. Error Model

Errors remain separated so future UI can offer the correct recovery:

| Class | Examples | Terminal behavior |
|---|---|---|
| `ProviderFailure` | transport, auth, rate limit, provider rejection | terminate, no retry |
| `AgentProtocolFailure` | malformed stream, invalid tool JSON, unknown tool, incomplete call | terminate |
| `SourceValidationFailure` | parse, restricted subset, static policy | terminal only when the run cannot or does not repair it |
| `RuntimeFailure` | executor, host, output, proposal-policy failure | terminal only when the run cannot or does not repair it |
| `BudgetExhausted` | model/tool/token/cost/deadline/candidate limit | terminate |
| `UserCancelled` | explicit cancellation | terminate |

Validation and dry-run tools normally return typed failure results to the
agent so it can repair the current draft within remaining budgets. The driver
uses `SourceValidationFailure` or `RuntimeFailure` when the run ends without a
repair path, the model violates the protocol, or the relevant attempt budget is
exhausted.

A known unavailable inspection capability returns a typed tool result rather
than terminating the run.

## 12. Privacy and Diagnostics

BAC may keep completed assistant messages in memory for the current session.
This is necessary for multi-turn context. It must not treat tracing, error
reports, or product events as a second conversation store.

The following must not appear in tracing or privacy-safe records:

- provider API keys or authorization headers;
- attachment bytes;
- complete prompts or provider responses;
- raw OCR text or raw visual inspection output;
- complete automation source;
- sensitive tool arguments/results; or
- raw provider stream frames.

Tracing uses stable explicit `rollshot::agent::*` targets and structured
metadata such as provider ID, model ID, run ID, event kind, durations, counts,
usage, error class, and source generation.

Product events use bounded privacy-safe summaries. Tool implementations declare
their privacy classification so event construction cannot accidentally include
raw sensitive payloads.

Transient attachment bytes and partial provider state are dropped when the run
ends. Subproject 5 later decides which domain summaries are persisted.

## 13. Testing

### 13.1 Domain and state tests

- Source replacement increments generation.
- Replacement invalidates validation/dry-run evidence.
- Stale evidence cannot submit.
- Terminal state can be assigned once.
- Event sequence numbers are ordered.
- Partial text commits only after successful completion.
- A terminal tool prevents subsequent same-turn calls.

### 13.2 Scripted driver tests

Use a scripted model to drive:

```text
inspect_context_summary
replace_automation_source
validate_automation
dry_run_automation
submit_for_review
```

Assert a valid `ReadyForReview` containing a validated automation and transient
proposal. Additional scripted runs cover repair after validation failure,
repair after dry-run failure, and `request_user_input`.

### 13.3 Tool tests

- Response-order serial execution.
- Stable unavailable results for OCR/layout.
- Tool schema and typed decoding.
- Per-tool limits.
- Cancellation and timeout.
- Draft precondition enforcement.
- Submission guard enforcement.

### 13.4 Provider recorded-fixture tests

Anthropic and OpenAI each require fixtures for:

- request body and tool-definition schema;
- plain streamed text;
- interleaved text and tool call;
- tool arguments split across multiple frames;
- multiple ordered tool calls;
- usage mapping;
- normal completion;
- malformed tool JSON;
- incomplete stream;
- provider error and rate-limit response; and
- cancellation during streaming.

Fixtures contain synthetic non-sensitive content. Tests must not require
network access or API keys.

### 13.5 Budget and failure tests

- Model-call limit.
- Wall-clock deadline.
- Input/output token limit.
- Cost limit.
- Tool and attempt limits.
- Candidate count/area limits.
- Provider usage charged once.
- Provider failure never retries automatically.
- Cancellation does not commit partial text.

### 13.6 Privacy tests

- Tracing captures no prompt, response, attachment, automation source, or
  sensitive tool payload.
- Events contain only allowed summaries.
- Session history receives completed messages only.
- Terminal reports contain IDs/counts/classes, not raw provider payloads.

### 13.7 Existing-system integration

Integration tests use:

- existing `rollshot-automation` frontend;
- existing replaceable executor contract;
- `FakeAutomationHost` and at least one prepared `RealAutomationHost` path;
- `rollshot-edit-proposal` validation; and
- deterministic synthetic image/tool data.

No integration test applies operations to an `ImageDocument`.

Optional live Anthropic/OpenAI smoke tests may be run manually with explicit
authorization and synthetic inputs. They are evidence supplements, not gates.

## 14. Delivery Phases

The implementation plan should keep phases independently testable:

1. Domain, events, draft generations, budgets, and terminal states.
2. Typed tool registry and automation authoring tools.
3. Manual scripted-model driver and complete author-loop acceptance.
4. Streaming model facade and normalized stream assembly.
5. Anthropic adapter plus recorded fixtures.
6. OpenAI adapter plus recorded fixtures.
7. Cancellation, privacy, resource, and cross-crate integration hardening.

Each phase appends a handoff note under `docs/superpowers/handoffs/`.

## 15. Success Criteria

BAC is complete when:

1. A scripted provider completes the full author loop and returns
   `ReadyForReview`.
2. `request_user_input` returns a validated `NeedsUserInput`.
3. Anthropic and OpenAI recorded fixtures prove streaming text and structured
   tool normalization.
4. Text deltas are observable before turn completion, while tool calls execute
   only after complete schema-valid assembly.
5. Tool execution and run events are deterministic.
6. Every budget and cancellation path terminates with the correct typed state.
7. Provider failures do not retry automatically.
8. OCR/layout absence is represented as typed unavailability.
9. No BAC code persists session/run state or mutates `ImageDocument`.
10. No public API exposes Rig types.
11. Tracing and privacy-safe records contain no raw sensitive payloads.
12. The crate tests, workspace formatting, and applicable clippy checks pass.

After BAC lands, vision SP3 may design and implement template acquisition using
the bounded session, inspection, draft, budget, and `NeedsUserInput` contracts.
