# Tools and scheduling comparison

**Research date:** 2026-07-22 (Asia/Taipei)
**Status:** In Progress (Round 3 capability comparison)
**Umbrella revision:** 1
**Current Rollshot revision:** `70b5a4ce17a1d2cd4d7ed9731678834bad1e12bf`
**Reference revisions:** Pi `dd6bea41efa8caa7a10fe5a6401676dc5699f83f`;
oh-my-pi `7b141199d524b859c357fc89654f10b62b9f3df1`; Codex
`4a443994bd12f49f2f08b21a2f224d9d42b9e734`; Claude Code source
`2ca5ddabfed5f220812ea11f029eda03b21bc4c1`.
**Evidence mode:** static source and test-source inspection. Rollshot's focused
tests were run for this task; the external test suites, provider calls,
permission dialogs, cancellation races, compaction, and process-crash behavior
were not executed.

This document compares Tool definition, exposure, authorization, scheduling,
and result lifecycle. It does **not** select a final Rollshot architecture.

## 1. Rollshot problem and three workload traces

Rollshot currently has a strong bounded baseline: a product assembles a typed
Tool registry for one Agent Run; the driver advertises that registry, charges a
16-dimensional finite budget, executes a returned batch serially, and stops on
the first successful terminal Tool. That design is directly useful for Smart
Redaction. It does not by itself establish that all future Tools should be
serial, nor does another system's parallel execution prove that Rollshot needs
it. [E:R0, E:R1]

| Workload | Observed trace | Scheduling pressure actually established |
|---|---|---|
| **Smart Redaction** | Source generation → validation → dry run → `submit_for_review`, or `request_user_input`. Generation-bound evidence and a typed terminal prevent stale submission. [W1] | Requires deterministic ordering, bounded result/context size, cancellation, and terminal stop-after-success. Mutating authoring Tools have real dependencies. This trace does **not** establish parallel Tool calls, dynamic discovery, or retry of ambiguous effects. |
| **Action Guide** | Durable project revisions surround independent caption and visual-annotation proposals; stale `document_state_id` results are rejected. [W2] | Requires availability and authority to be resolved from current document/capture state, and outputs tied to a revision. Independent read/inspection calls could be candidates for overlap, but current code does **not** establish that parallel scheduling is needed [A:R-PARALLEL]. |
| **Deferred brag + Hyperframes** | Project inspection feeds plan/check, optional scene workers, assembly, render, verification, poster, and share-copy Artifacts. Some stages are independent; others wait on explicit Artifact prerequisites. [W3, E:S1, E:J1] | If adopted, requires dependency-aware waves, expected-Artifact gates, background Job separation, attempt identity, and selective retry. A flat parallel Tool batch cannot represent those dependencies. It does not mandate video generation or a general Workflow engine in Rollshot [W3, E:S1]. |

The workload ladder therefore keeps three levels distinct: one serial agent
Tool batch, one safely overlappable batch, and a durable dependency-aware
Product Workflow. Calling all three “parallel tools” would erase ownership and
recovery boundaries.

## 2. Terms and non-equivalent stages

### 2.1 Tool exposure and execution pipeline

The minimum useful pipeline is:

```text
implementation registered
        -> enabled/available for this Run
        -> described or discoverable
        -> advertised in this model Step
        -> selected by the model (call ID + arguments)
        -> schema/value validated
        -> authorized for this invocation
        -> admitted by the scheduler
        -> executed (attempt ID)
        -> result correlated by call ID
        -> retained inline / spilled / promoted to Product Artifact
```

Each arrow is a separate policy boundary:

| Term | Meaning here | Must not be inferred from it |
|---|---|---|
| **Registration** | The host knows an implementation and canonical name. | That the Tool is enabled, safe, authorized, or visible to a model. |
| **Discovery** | The host or model can find metadata for a Tool not already visible. | That its full schema has been advertised or its code is trusted. |
| **Schema** | Machine-readable input, and optionally output, shape. | Semantic validity, side-effect safety, idempotency, or authority. |
| **Description** | Model/user-facing purpose and use constraints. | Enforced policy. A prompt sentence such as “do not run in parallel” is weaker than scheduler admission. |
| **Availability** | The Tool can be selected in this Run/Step after configuration, provider, feature, resource, and health checks. | Filesystem, network, capture, credential, or publishing authority. **Availability is not authority.** |
| **Authorization** | Current product/user/policy grant permits this concrete invocation and inputs. | Successful execution, future authorization, or authorization after resume. |
| **Selection** | The model emitted a call against an advertised/discovered name. | Valid arguments, authorization, or execution. |
| **Admission** | Scheduler has granted a serial/exclusive/shared/dependency slot. | That the Tool will finish or that its external effect is retry-safe. |

### 2.2 Typed and dynamic Tools

A **typed Tool** has a host implementation whose argument/result types and
policy metadata are known in source. A **dynamic Tool** is registered at
runtime from a schema and an external responder, extension, MCP server, client,
or selected environment. Both eventually cross a runtime JSON/provider
boundary. Compile-time Rust or TypeScript types reduce implementation mistakes;
they do not authenticate the model response or grant authority. Conversely, a
dynamic Tool is not inherently unsafe if the host validates its schema,
identity, provenance, availability, authority, limits, and cancellation.

Typed output is also separate from a typed Tool. Pi's and OMP's generic result
details, Codex's `ToolOutput`, and Claude's optional `outputSchema` have
different guarantees. A JSON value, transcript block, or path is not
automatically a Rollshot Product Artifact.

### 2.3 Scheduling vocabulary

| Form | Exact meaning | Important consequence |
|---|---|---|
| **Serial** | Start call *n+1* only after *n* reaches a result/error boundary. | Deterministic effect order; slow independent reads do not overlap. |
| **Ordered parallel** | Admit independent calls concurrently, but publish final call results and context mutations in model source order. | Latency may improve without changing provider-visible ordering; completion/progress can still be live. |
| **Unordered completion** | Publish final results as executions finish, correlated by call ID. | Protocol pairing can remain correct, but the next context, hooks, shared state, and user-visible order may vary. |
| **Dependency-aware** | Admit only nodes whose declared prerequisites and gates are satisfied, often in bounded waves. | Belongs to Product Task/Workflow ownership when dependencies outlive one model response. |
| **Terminal Tool** | A successful call ends the Agent Run or current child contract without another model sample. | It needs an explicit batch rule: first terminal wins, whole batch completes, siblings cancel, or terminal calls are isolated. |

None of the pinned core Tool schedulers is a durable dependency-aware Workflow
engine [A:ALL-DAG]. Claude's file-backed work ledger has dependency edges, but
it does not automatically schedule Tool calls; Hyperframes' Artifact stages are
workload/reference evidence rather than a core system implementation. [E:L0,
E:S1]

## 3. Current Rollshot behavior

### 3.1 Registration, availability, description, and authority

`Tool` exposes `name`, `json_schema`, and `call`. `ToolRegistry::register`
rejects duplicate names and stores `Arc<dyn Tool>` implementations. The
workbench constructs one registry for the Smart Redaction Run and registers its
authoring/inspection Tools; the visual-annotation path constructs a different
single-Tool registry. In current production paths, registry membership is both
registration and Run availability. There is no separate enabled/deferred Tool
catalog in the six-file agent boundary [A:R-DYNAMIC]. [E:R1]

`tool_definitions()` advertises the registered name and input JSON Schema, but
sets every description to an empty string. The product system prompt describes
the workflow and Tool names instead. Result Rust structs are serialized by Tool
implementations, but no output schema is advertised in `ToolDefinition`.
Therefore Rollshot is typed and schema-driven at implementation/input decode,
while model selection guidance is prompt-owned rather than definition-owned.
[E:R1]

`AuthorizedModelInput` encapsulates and bounds a provider payload that upstream
product code has already selected and authorized; its constructor does not make
that authorization decision. `payload_mode` upstream chooses whether screenshot
bytes are included, while `AuthorizedModelInput::new` validates descriptor and
attachment counts, nonzero dimensions, declared versus actual byte counts, and
per-attachment/total limits. It is not a per-Tool filesystem/network/capture/
credential grant. The product's decision to construct a narrow registry is a
valuable availability boundary, but a separate invocation authority object,
approval hook, dynamic Tool registry, or sandbox contract was **not found in
the investigated agent scope** [A:R-AUTH, A:R-DYNAMIC]. Capability status
returned by image/OCR inspection indicates whether an inspection capability is
available/partial/unavailable; it likewise does not grant authority.

### 3.2 Serial batch, terminal Tools, and budgets

The driver receives Rig `PendingToolCall`s in model order, charges the whole
returned batch against Tool/validation/dry-run budgets, and calls
`ToolRegistry::execute_calls`. The registry executes serially. An unknown Tool,
hard `ToolError`, argument/result byte overflow, per-Tool call-limit failure, or
cancellation stops the batch. An argument-decode error is lowered to a
recoverable Tool result so the model may correct the call. [E:R1, E:R2]

`submit_for_review` and `request_user_input` are terminal. The first successful
terminal Tool stops later calls in the same batch and immediately returns a
typed `RunTerminalState`; the driver does not sample the model again. That
means later calls never run, and the successful terminal result is not threaded
into a subsequent provider request. For a nonterminal batch, the driver builds
one Rig result per pending call and Rig enforces complete call/result pairing
before the next model request. [E:R1, E:R3]

The registry enforces per-call argument/result byte limits and a per-Tool call
count. `BudgetTracker` separately accounts Tool calls, argument/result bytes,
validation/dry-run/capability attempts, model usage, wall time and other
dimensions. These counters identify budget consumption, not execution attempts
for replay or deduplication. `cumulative_usage_deduplicates_within_turn` is a
budget-accounting invariant, not a Tool-effect idempotency contract
[A:R-IDEMP].

### 3.3 Result lifecycle and gaps

Nonterminal results are serialized to strings, correlated to provider call IDs,
kept in the in-memory Rig history, and bounded by registry, driver, and Run
budgets. A large result fails; it is not spilled. No Tool-result store,
retention policy, compaction projection, artifact promotion record, pre/post
hook, generic retry ledger, scheduler dependency, or runtime dynamic Tool was
found in the investigated scope [A:R-RESULT, A:R-DYNAMIC, A:R-IDEMP]. The typed
`ReadyForReview` handoff is stronger: successful validation/dry-run evidence is
lowered into a Product proposal and terminal, not merely left as a Tool-result
path. [E:R0, E:R1]

This is a bounded limitation, not a defect for the current workload. Serial
execution plus generation checks make Smart Redaction's mutating chain easy to
reason about. OpenAI requests also explicitly set `parallel_tool_calls: false`;
host serial policy remains authoritative even for providers that emit a batch.
[E:R1]

## 4. Per-system factual behavior and status

### 4.1 Pi: active Tool snapshot with source-ordered parallel results

At the pinned **Reviewed** profile revision, Pi's low-level `AgentTool` includes
name, description, TypeBox input parameters, execute callback, optional
`prepareArguments`, execution mode, updates, usage, and terminal hint.
Coding-agent has a larger built-in/extension registry and selects an active
Tool array; that array is the next provider request's availability snapshot.
Extensions can register or replace implementations. There is no built-in
deferred ToolSearch stage in the inspected Pi path. [E:P0, E:P1]

Pi defaults the batch to parallel. It performs start/preflight sequentially,
validates arguments, and lets `beforeToolCall` block. Allowed calls then run
concurrently. `tool_execution_end` follows completion order, while final
Tool-result messages are emitted in assistant source order. Global sequential
mode—or one Tool marked `executionMode: "sequential"`—makes the whole batch
serial. [E:P1, T:P1]

`afterToolCall` may replace content/details/error/usage/terminate. The batch
stops without another model call only when **every** finalized result has
`terminate: true`; one terminal result does not cancel or skip siblings.
Unknown Tools, validation failures, blocks, aborts, and execution failures are
paired error results. Extension `tool_call` mutations are not revalidated in
the inspected coding-agent hook path. [E:P1, T:P1]

Built-in edit/write Tools join a canonical per-file mutation queue, but that
queue does not infer multi-file dependencies or arbitrary side effects. A
built-in authority/grant/sandbox or generic Tool idempotency/attempt contract
was **not found in the exact focused scope** [A:P-AUTH, A:P-IDEMP]. Pi sessions
retain Tool results/details in JSONL; compaction summarizes older context and
retains a recent tail, but Pi has no generic typed Artifact promotion contract
in the investigated scope. [E:P0, E:C1]

### 4.2 oh-my-pi: discoverable Tools and shared/exclusive completion order

At the pinned **Reviewed** revision, OMP Tools may come from built-ins,
extensions/hooks, custom modules, MCP, capability providers, ACP/client mounts,
or SDK injection. An enabled Tool can be `essential` (full top-level schema) or
`discoverable` (mounted under `xd://` or found with Tool search). Its contract
includes schema, description/summary, approval tier, intent, load mode,
interruptibility, update/rendering, and shared/exclusive concurrency metadata.
The capability registry aggregates availability and source priority; it is
explicitly not an authority boundary. [E:O0, E:O1]

The core scheduler resolves concurrency per call from raw pre-validation
arguments. Shared calls overlap; an exclusive call waits for the preceding
exclusive and all earlier shared calls, and later shared calls wait behind it.
A throwing resolver falls back to exclusive. Execution uses
`Promise.allSettled`, but final Tool results/events are emitted in **completion
order**, so provider-visible result order can differ from assistant source
order. Tests assert both parallel overlap and completion ordering. [E:O1,
T:O1]

Approval policy runs before extension `tool_call` interception in the inspected
extension wrapper. The hook can block; `tool_result` can replace the returned
content/details/error state. Interruptible wait-like Tools may be aborted to
deliver steering; completed effects retain their real result rather than being
misreported as skipped. A completed post-Tool hook can use the special
`TERMINAL_TOOL_RESULT_ABORT_REASON` to persist the batch and stop before the
next provider call; Task uses this for terminal `yield`. That is a host/hook
protocol, not a generic `terminate` field on every Tool result. [E:O1, E:O2]

Large textual outputs can spill to a session-scoped file and leave an
`artifact://<numeric-id>` reference. Resume scans existing IDs, and full inline
resolution is capped at 8 MiB. These are immutable-addressed overflow logs, not
typed Product Artifacts with revision/provenance/acceptance
[A:O-ARTIFACT]. OMP pruning can elide contextually useless or older Tool
material from the model projection. [E:O3, E:C1]

OMP has approval tiers and concurrency metadata, but a common side-effect
class, dependency scheduler, effect idempotency key, or durable attempt ledger
was **not found in the focused Tool/runtime scope** [A:O-SIDE,
A:O-IDEMP, A:ALL-DAG].

### 4.3 Codex: typed/dynamic router with ordered read/write admission

At the pinned **Reviewed** revision, each model Step builds a `ToolRouter` from
typed core/extension handlers and runtime Tool specifications. It also supports
dynamic per-Thread Tools and MCP Tools. Exposure is direct, deferred, or hidden;
ToolSearch can return deferred specs. Dynamic calls are correlated with the
model call ID and wait on a Turn-local oneshot response. Availability remains
separate from approval, `PermissionProfile`, sandbox, environment, and
session/turn grants. [E:C0, E:C2]

`ToolCallRuntime` asks the handler whether parallel calls are supported. An
opt-in handler acquires a shared `RwLock` read guard; other handlers acquire the
write guard and exclude the batch. MCP server opt-in or a read-only annotation
can enable parallel calls. The model capability controls whether parallel
calls are requested, but host admission still applies. Final futures are held
in `FuturesOrdered`, so results are recorded in source order even when handler
completion overlaps. [E:C1, T:C1]

PreToolUse runs after routing/kind checks and before handler execution; it can
block or update input through a handler-specific reparse path. PermissionRequest
hooks participate in approval. PostToolUse runs only for a successful Tool
output in the inspected registry path; a post hook may replace/block the
model-visible result, but cannot undo the completed effect. A named
`PostToolUseFailure` or equivalent generic error hook was **not found in that
path**; failure lifecycle telemetry/events are not an error hook
[A:C-ERROR-HOOK]. Cancellation either aborts the handler future or waits for
runtime teardown according to the handler contract, then produces one
correlated aborted result. [E:C1, E:C3]

Codex truncates generic model-visible Tool output and preserves the correlated
result in rollout history, but a generic spill-to-Artifact promotion, terminal
Tool flag, dependency-aware Tool scheduler, or effect idempotency/attempt
ledger was **not found in the focused Tool paths** [A:C-LIFECYCLE,
A:ALL-DAG]. Compaction changes the model projection while original rollout
history remains; its narrow image-generation Artifact is not a generic Tool
result lifecycle. [E:C0, E:C1, E:C4]

### 4.4 Claude Code source: rich metadata, deferred search, dual scheduling paths

At the pinned **Reviewed external-source** revision, `Tool` includes Zod/JSON
input schema, optional output schema, dynamic description, enablement,
read-only/concurrency-safe/destructive/open-world metadata, permission checks,
interrupt behavior, deferred/always-load flags, result-size threshold, hooks,
progress, and rendering. MCP adds runtime Tools; deferred Tools require
ToolSearch before their schema is considered present. Four availability stages
are visible: assembled pool, `isEnabled`, direct/deferred advertisement, then
model selection. [E:L0, E:L1]

Both schedulers conservatively treat invalid inputs or throwing concurrency
classifiers as unsafe, but their mechanics differ. The **nonstreaming** path
partitions consecutive safe calls and feeds their generators to `all()`, whose
`Promise.race` loop yields results in completion order; only this path has
`CLAUDE_CODE_MAX_TOOL_USE_CONCURRENCY`, default 10. It queues concurrent
context modifiers by Tool ID and applies them in source order after the batch.
The **streaming** executor has no visible numeric concurrency cap: safe calls
start while all executing calls are safe, and a completed safe call can be
yielded ahead of an earlier still-running safe call. An executing unsafe call
is a barrier that blocks later admission/result emission; unsafe context
modifiers apply immediately, while concurrent-safe context modifiers are
explicitly unsupported in the inspected code. Progress remains live. A Bash
error in the streaming path aborts running siblings and yields paired synthetic
errors; other Tool errors do not automatically cancel unrelated siblings.
[A:L-ORDER, E:L1, T:L1]

Input schema/value validation precedes PreToolUse. The pre hook can add context,
stop, update input, or influence permission; general/tool-specific permission
resolution follows. PostToolUse observes a successful result, while a distinct
PostToolUseFailure path observes failure. Cancellation uses per-Tool child
controllers and Tool-specific `interruptBehavior` (`cancel` or conservative
`block`). [E:L1]

Large textual results spill under the session's `tool-results/` directory by
`tool_use_id`; create-new (`wx`) prevents rewriting the same ID on later
microcompact replay. The model receives a preview and file path. A separate
per-message result budget persists selected fresh results and records
replacement state for resumable main/agent sources. Compaction may project or
replace Tool results, but output files remain heterogeneous path resources, not
a common Product Artifact contract. [E:L2, E:C1]

The source declares `isDestructive` for irreversible delete/overwrite/send
operations and `isOpenWorld` for external interaction, but a generic effect
idempotency key, attempt ledger, dependency-aware Tool scheduler, or common
terminal-Tool flag was **not found in the exact focused roots**
[A:L-LIFECYCLE, A:ALL-DAG]. Some context-reduction modules remain
hidden/unavailable in this external source, so their Tool-result interaction is
lower-confidence [E:L0].

## 5. Registration, discovery, Availability, Authorization, and selection

| System | Registration and Tool type | Discovery, schema, and description | Availability | Authorization | Selection |
|---|---|---|---|---|---|
| **Rollshot** | Host registers typed Rust `Tool` objects; duplicate names rejected. No runtime dynamic registry found [A:R-DYNAMIC]. | Full registered input schemas advertised every Step; description is currently empty; no deferred discovery [A:R-DYNAMIC]. | Product-constructed per-Run registry. Capability result status is separate. | `payload_mode` upstream selects authorized attachment inclusion; `AuthorizedModelInput::new` only validates payload shape/limits. Per-Tool authority/approval/sandbox not found [A:R-AUTH]. | Provider call must match registered name; Rig/driver validate call structure and Tool decodes values. |
| **Pi** | Typed `AgentTool`; built-ins plus executable extension registration/replacement. | Active Tools' TypeBox schemas/descriptions are advertised; no built-in deferred ToolSearch found in focused scope [A:P-AUTH]. | Coding-agent active Tool snapshot, refreshable between turns. | Optional extension block hook; built-in grant/sandbox/approval cache not found [A:P-AUTH]. | Model selects an active name; prepare/schema validation then pre-hook. |
| **OMP** | Typed `AgentTool` adapters over built-ins, extensions, MCP, capability/ACP/client/SDK sources. | Essential full schemas or discoverable metadata via `xd://`/search. | Enabled sources, capability priority, provider/resource health and selection; capability availability is not authority. | Tier/mode/per-Tool approval plus ACP/client mediation; extensions remain unsandboxed code. | Model selects direct or discovered Tool; approval and hooks run per invocation. |
| **Codex** | Typed core/extension handlers plus runtime dynamic and MCP specs. | Direct/deferred/hidden exposure; ToolSearch supplies deferred specs. | Router rebuilt per Step from current Turn/environment/extensions. | Approval policy, permission profile, environment/sandbox, grants, and permission hooks are separate. | Model-visible call ID maps to handler; payload kind/input reparsed and admitted. |
| **Claude** | Typed TypeScript `Tool` pool plus MCP Tools. | Zod/JSON schema, dynamic description; deferred ToolSearch and always-load escape. | Pool → `isEnabled` → direct/deferred advertisement. | Tool/general rules, mode, PreToolUse, permission handler and local trust checks. | Model call is validated, hooked, authorized, then scheduled. |

The portable lesson is not “dynamic is better” or “typed is safer.” Rollshot
needs explicit identities for **implementation**, **schema revision**,
**availability snapshot**, **authority decision**, **model call**, and
**execution attempt** if it later supports discovery, resume, or retries.

## 6. Scheduling and terminal execution comparison

| System | Admission unit | Concurrency | Final result order | Terminal behavior | Dependency behavior |
|---|---|---|---|---|---|
| **Rollshot** | Whole model batch, then calls one by one. | Serial only; OpenAI request also disables provider parallel calls. | Source order. | First successful named terminal stops remaining calls and ends Run. | No Tool dependency graph found [A:R-DYNAMIC]. Dependencies live in Tool preconditions/generation evidence. |
| **Pi** | Whole batch; one sequential Tool makes whole batch serial. | Default parallel after sequential preflight. | Source order; end events completion order. | Stop only when every finalized result says terminate; siblings complete. | No dependency scheduler found [A:ALL-DAG]; edit/write only serialize same canonical path. |
| **OMP** | Each call resolves shared/exclusive; barriers preserve declared order. | Shared overlap; exclusive waits on prior shared/exclusive. | Completion order. | Special completed-hook abort reason stops before next sample; Task `yield` supplies policy. | No Tool dependency graph found [A:ALL-DAG]. |
| **Codex** | Per-handler read/write gate. | Explicit shared opt-in; otherwise exclusive. | Source order via `FuturesOrdered`. | Common terminal Tool flag not found [A:C-LIFECYCLE]. Some Tools affect Turn/run lifecycle through their own handlers. | No Tool dependency graph found [A:ALL-DAG]. |
| **Claude** | Nonstreaming partitions consecutive safe calls/unsafe barriers; streaming admits incrementally. | Nonstreaming safe batch cap defaults to 10; streaming has no visible numeric cap. Executing unsafe is a barrier [A:L-ORDER]. | Safe-call results are completion-order in both paths. Nonstreaming concurrent context modifiers apply later in source order; streaming safe modifiers are unsupported [A:L-ORDER]. | Common terminal Tool flag not found [A:L-LIFECYCLE]; hooks can prevent continuation in specific flows. | No Tool dependency scheduler found [A:ALL-DAG]; work-ledger dependencies are separate. |

Unordered completion is therefore real in OMP and Claude safe-call batches,
but it is not a maturity level above ordered completion. It optimizes
latency-to-result visibility at the cost
of nondeterministic transcript/context order. Call IDs preserve protocol
pairing; they do not make shared-state effects deterministic. Pi and Codex
overlap execution while retaining source-order final results; Claude instead
shows why result order and later context-modifier order must be stated
separately [A:L-ORDER].

Terminal Tools need stricter admission than ordinary read-only Tools. Running a
terminal submission alongside mutation, external publication, or another
terminal can create a winner after siblings have already caused effects.
Rollshot's serial first-winner rule avoids that today. In a future parallel
model, a terminal Tool should be an exclusive barrier or the only call in its
admission group unless a workload proves a different, fully specified rule.

## 7. Side effects, idempotency, replay, and attempts

### 7.1 Side-effect classes

| Class | Examples | Safe default admission | Retry/replay requirement |
|---|---|---|---|
| **Read-only** | Read immutable project metadata, current source snapshot, cached region features. | Parallel only when the read snapshot/revision and underlying provider are concurrency-safe. | May retry with a new attempt if reads are stable and budgets/cost permit; still preserve call/attempt distinction. |
| **Mutating** | Edit/replace automation source, write a project file, update a Task. | Serial/exclusive by target or explicit dependency; optimistic revision check before commit. | Retry only with operation ID/precondition or after proving no effect. A repeated model call is a new attempt, not dedup by equal JSON. |
| **External** | Network/API query, spawn process, remote render, send MCP call. | Explicit capacity/rate/cost/credential admission; read-only external calls may still have quotas. | Use provider idempotency key or query-by-key for chargeable starts; ambiguous acknowledgement becomes `start_unknown`, not blind retry. [E:J1] |
| **Irreversible** | Delete/overwrite without recovery, publish/share, send message, apply accepted proposal. | Exclusive, current authorization/approval, explicit Product checkpoint; normally terminal or deterministic handoff. | Never automatic replay without durable deduplication and user-visible prior outcome. |

Rollshot currently has no common side-effect metadata [A:R-DYNAMIC]. Pi has
parallel/sequential and a file-mutation queue, OMP shared/exclusive plus
approval tier/intent, Codex per-handler parallel support and MCP read-only
annotations, and Claude exposes the richest direct metadata
(`isReadOnly`/`isDestructive`/`isOpenWorld`/`isConcurrencySafe`). None proves
effect idempotency. Scheduler classification should be host-owned and
conservative; a Tool or external server's declaration is evidence to validate,
not authority to broaden access.

### 7.2 Identity and retry rules

Four identifiers solve different problems:

1. **Model call ID** pairs assistant Tool use with the result sent back.
2. **Logical operation ID** names the Product intent across attempts, such as
   “render revision 7” or “propose annotations for document state 12.”
3. **Attempt ID** names one admitted execution, budget charge, hook/audit span,
   and terminal outcome.
4. **Provider idempotency key** lets an external authority deduplicate repeated
   start requests for the same logical operation.

Reusing a model call ID for spill-file naming, as Claude does, deduplicates
retention writes during context replay; it does not prove the Tool effect was
executed exactly once. Likewise, retaining a Tool result in Pi/OMP/Codex/
Claude transcripts does not authorize re-execution after resume.

A generic Tool effect idempotency/replay/attempt contract was not found in the
focused runtimes [A:R-IDEMP, A:P-IDEMP, A:O-IDEMP, A:C-LIFECYCLE,
A:L-LIFECYCLE]. Therefore all retry statements in this comparison are
candidate Rollshot policy, not descriptions of a shared reference guarantee.
At minimum:

- validation/unknown-Tool errors may be corrected with a new model call;
- a known pre-execution cancellation may be retried as a new attempt;
- a completed read may be replayed from its retained result when valid for the
  same snapshot;
- a mutating/external/irreversible attempt with ambiguous completion must be
  reconciled by operation/provider identity before retry; and
- duplicate completion notifications/results must converge on one retained
  attempt terminal and one Product Artifact publication.

## 8. Hooks, approval, cancellation, result, and error flow

| System | Pre/approval order | Post/result/error behavior | Cancellation |
|---|---|---|---|
| **Rollshot** | Decode/limits inside Tool; no generic pre-hook or per-call approval layer found [A:R-AUTH, A:R-DYNAMIC]. | Tool returns Success/Recoverable or hard error; driver emits start/end and typed Run terminal. No generic post/error hook found [A:R-DYNAMIC]. | Shared Run cancellation checked before calls; automation receives the paired flag. A Tool must observe its own flag during work. |
| **Pi** | Argument preparation + schema validation → `beforeToolCall`; coding-agent extension `tool_call` can block/mutate (mutation not revalidated). Built-in authority gate not found [A:P-AUTH]. | `afterToolCall` can rewrite content/details/error/usage/terminate; failures become correlated results. | One AbortSignal reaches providers and Tools; sequential batch stops after observed abort, parallel calls share cancellation. |
| **OMP** | In extension wrapper: approval policy/prompt → extension `tool_call` → execution. Hook wrappers can separately block. | Extension/hook `tool_result` can rewrite success or error; completed effects remain real. Terminal hook reason stops after batch persistence. | Per-call signals plus special interruptible waits; parent/steering abort behavior is Tool metadata. |
| **Codex** | Route/kind check → PreToolUse (block/update) → handler; permission hooks/approval/sandbox occur in the handler's execution path. | PostToolUse only for successful output in focused registry; it may block/replace model-visible result after effect. Lifecycle events still record failure/abort, but named `PostToolUseFailure` or equivalent generic error hook was not found [A:C-ERROR-HOOK]. | Cancellation either aborts the future or awaits declared teardown, then returns one paired abort output. |
| **Claude** | Schema/value validation → PreToolUse/update/stop → permission resolution → call. | PostToolUse for success; explicit PostToolUseFailure for errors; output can add context/stop signals. | Child AbortControllers; Tool chooses interrupt `cancel` or conservative `block`; Bash failure may cancel siblings. |

Hooks run inside a security- and privacy-sensitive path. They need bounded
inputs/outputs, timeouts, deterministic ordering, provenance, cancellation, and
an explicit failure policy. A pre-hook that changes arguments must cause schema,
side-effect, authority, and scheduling re-evaluation; otherwise it can move an
authorized read into an unauthorized mutation. A post-hook can change what the
model sees, but cannot roll back an effect and must not relabel an executed
mutation as “skipped.”

## 9. Tool-call/result pairing, retention, spill, Artifact promotion, and compaction

| System | Pairing/order | Retention and spill | Compaction interaction | Artifact promotion |
|---|---|---|---|---|
| **Rollshot** | Rig requires one result per pending nonterminal call; driver preserves source order. Terminal Run exits before another provider request. | In-memory result string, registry/driver/Run byte caps; oversize fails. No result store/retention/spill contract found [A:R-RESULT]. | No Rollshot compaction layer found in current agent boundary [A:R-RESULT]. | `ReadyForReview` is explicit Product handoff after deterministic evidence; no generic ordinary-result promotion record was found [A:R-RESULT]. |
| **Pi** | Source-order result messages after parallel execution; call ID pairing. | JSONL transcript/details; built-in bash may persist truncated full output path, but no common typed Artifact store found [A:P-ARTIFACT]. | Full summary + recent tail; old transcript remains. Tool details may reconstruct extension state. | Extension-specific only; generic promotion contract not found [A:P-ARTIFACT]. |
| **OMP** | Completion-order results, paired by call ID. | Session `artifact://` spill files with resumed numeric ID scan and 8 MiB inline cap. | Full/remote/snap/shake/prune paths can summarize or elide model-visible result content; stored session/artifact data remains separate. | Spill files are untyped overflow logs; Product Artifact validation/acceptance/promotion not found [A:O-ARTIFACT]. |
| **Codex** | `FuturesOrdered` source-order recording and call ID pairing. | Model-visible output truncation plus rollout records; generic spill Artifact not found [A:C-LIFECYCLE]. | Persisted compaction checkpoint replaces projection; original rollout remains. | Narrow extension-specific image path only; no generic promotion in focused Tool paths [A:C-LIFECYCLE]. |
| **Claude** | Safe-call results are completion-order in nonstreaming and streaming paths; unsafe barriers constrain streaming emission. Paired by `tool_use_id` [A:L-ORDER]. | Per-Tool and aggregate thresholds spill text by ID to session files; preview/path retained; content-replacement decisions can persist for resume. | Tool-result budget runs before microcompact; traditional/micro/hidden projection mechanisms may replace results while retained files/transcript records remain separate. Hidden algorithms limit confidence [E:L0]. | Paths/attachments remain heterogeneous; common typed Product Artifact contract not found [A:L-LIFECYCLE]. |

Spill and Artifact promotion solve different problems:

- **Spill** preserves bytes omitted from model context. It needs access control,
  retention/deletion, integrity, privacy classification, and a stable locator.
- **Compaction** changes the model-visible projection. It must preserve the
  call/result protocol and references needed to retrieve promoted evidence.
- **Promotion** validates an output against a Product contract, assigns Product
  identity/revision/provenance, and records acceptance/review state. A spill
  path becomes an Artifact only through this explicit transition.

Rollshot should not put raw screenshot/tool bytes into an unbounded transcript
or assume that a compact summary preserves executable evidence. The Product
Artifact should retain minimal provenance—operation/attempt, Tool/schema
revision, source document revision, validation/hash, sensitivity and retention
class—while the model projection may use a bounded summary/reference.

## 10. Candidate Rollshot scheduling patterns

These patterns are deliberately not a final selection. Each pattern satisfies
the Round 3 gate by specifying ownership, concurrency, completion,
cancellation, failure, retry, and Artifact behavior.

| Required semantic | **Pattern A — bounded serial transaction** | **Pattern B — classified ordered-parallel batch** | **Pattern C — dependency-aware Artifact waves** |
|---|---|---|---|
| **Ownership** | `AgentRunner` owns one Run/Tool batch; product owns current registry, authority, budget, typed terminal and proposal. | Run scheduler owns call attempts/order; product supplies immutable availability/authority snapshot and target revision; Tool declares validated class but cannot grant itself authority. | Product Workflow owns nodes/dependencies/checkpoints/Artifacts; Tool runner/Job/Child Agent adapters own individual attempts. Conversation is not the scheduler. |
| **Concurrency** | Exactly one call executes at a time in model order. Terminal Tool is an exclusive barrier and first success stops later calls. | Read-only, snapshot-bound, concurrency-safe calls share a bounded pool; mutating/external/irreversible/terminal or unknown calls are exclusive barriers. Final results/context modifications publish in source order. | Admit ready nodes in bounded waves after dependency and authority checks. Independent Artifact producers may overlap; checkpoints, irreversible actions and fan-in gates serialize. |
| **Completion** | Nonterminal call produces one correlated result; whole batch is complete only after every admitted call before a terminal/hard stop. Run terminal is typed. | Each model call ID and attempt produces one terminal result, including cancelled/skipped. Batch completes after source-order publication of every admitted result; terminal success ends only after earlier calls settle and later calls are never admitted. | Node completion requires typed output plus expected Artifact validation/publication. Notification/process/agent exit is progress, not completion. Workflow completes from durable terminal nodes and gates. |
| **Cancellation** | Run cancellation checked before/between calls and passed into Tool; current call cleans up, later calls never start. | Per-attempt child token plus batch token. Cancel queued calls immediately; running reads stop cooperatively; mutating/external calls report confirmed/unknown effect state before batch terminal. | Persist cancellation intent when recovery is promised; propagate to active Tool/Job/Child attempts, stop new admission, reconcile confirmed/unknown outcomes, and retain checkpoint/accepted Artifacts. |
| **Failure** | Validation/recoverable errors return to model; hard registry/protocol/budget/cancellation error ends batch/Run with typed state. | Every sibling gets a paired outcome. Read failure need not cancel other reads; exclusive/terminal failure opens a barrier before later admission. Hook, authority, scheduler, runtime and result-validation failures remain distinct. | Node/attempt failures, lost Jobs, stale revisions, checkpoint denial and Artifact validation failures are durable and distinct. Downstream nodes remain blocked; independent completed nodes remain valid. |
| **Retry** | No automatic retry of effects. Model correction emits a new call/attempt. Known pre-execution failures may retry within remaining budget; ambiguous effects require user/product reconciliation. | Host assigns attempt ID; only read-only/idempotent calls auto-retry under explicit limits. Same provider idempotency key resolves ambiguous external start; mutation retries require revision/precondition/dedup. | Workflow owns selective new attempts. Reuse immutable accepted inputs/Artifacts; retry only failed/missing nodes after concrete gate feedback. Provider starts reuse operation idempotency key; attempt history/cost is retained. |
| **Artifact** | Ordinary results stay bounded inline; successful terminal promotes the existing typed proposal/evidence. Oversize fails unless a separately designed spill contract is added. | Large results may spill to an access-controlled run resource, but only product validators promote typed Artifacts. Compaction retains call ID, result summary and locator. | Artifact identity, expected schema/hash/revision/provenance and acceptance are the fan-in contract. Publish atomically/marker-last; partial attempt output is quarantined or deleted. |

### 10.1 Trade-offs and preliminary fit

| Pattern | Smart Redaction | Action Guide | Deferred brag + Hyperframes |
|---|---|---|---|
| **A: bounded serial** | Exact current semantic fit; simplest terminal/evidence reasoning [E:R1, W1]. | Fits one revision-bound caption/annotation proposal [W2]. | Correct inline fallback but cannot overlap independent stages or survive as Workflow state [W3, E:S1]. |
| **B: ordered parallel** | Benefit unproven; inspection reads may be measurable candidates, while authoring chain remains exclusive [A:R-PARALLEL, W1]. | Could overlap independent revision-bound inspections/proposals if product demand appears [A:R-PARALLEL, W2]. | Useful inside one ready wave, but not sufficient for dependencies, Jobs, checkpoints or durable Artifacts [W3, E:S1]. |
| **C: Artifact waves** | Unjustified overhead for the current bounded Run [E:R1, W1]. | Plausible only if the product orchestrates multiple revision-bound units rather than today's independent calls [W2]. | Strong semantic match if the deferred workflow is adopted; still does not imply every node needs an Agent [W3, E:S1]. |

Pattern B is materially different from Pi's batch-wide sequential override and
from OMP/Claude completion-order publication: it combines conservative
classification with source-order final results. Claude also shows that a
streaming scheduler needs an explicit concurrency cap and context-modifier
policy rather than inheriting assumptions from its nonstreaming path
[A:L-ORDER]. Pattern C is materially different from any flat Tool scheduler
because durable Product state, not one model response, determines readiness and
completion.

## 11. Security, privacy, and authority consequences

1. Registry/discovery provenance must be auditable. Dynamic Tool schemas and
   descriptions can be attacker-controlled content and must not broaden
   credentials or sandbox policy.
2. Availability and authority must be separate typed decisions. Resume or
   compaction may restore that a Tool existed, but current permission, capture
   consent, network/credential availability, document revision, and sandbox
   policy must be revalidated before admission.
3. Parallel reads can still leak information or exhaust rate/cost limits.
   “Read-only” describes mutation, not privacy or expense.
4. Hook inputs/results may contain screenshot text, file paths, commands, MCP
   arguments or credentials. Default audit events should record identities,
   sizes, decision classes and hashes—not raw sensitive content.
5. Spill resources need unguessable or authority-bound identity, containment,
   no-follow behavior where filesystem-backed, bounded reads, retention and
   deletion. OMP's numeric locator and Claude's local path are useful UX
   evidence, not sufficient Rollshot authority designs.
6. Post hooks cannot reverse effects. A rejected/rewritten result must retain an
   audit fact that the handler executed and whether its effect is known.

## 12. Non-goals and measurable evaluation criteria

### 12.1 Non-goals

This comparison does not:

- enable parallel Tool calls in current Smart Redaction;
- add Tools, hooks, MCP, dynamic extensions, Workflow or Job code;
- treat a Tool description, Skill `allowed-tools`, capability status, MCP
  annotation, or concurrency flag as an authority grant;
- promote every Tool result or ambient file into a Product Artifact;
- promise exactly-once side effects without provider/product deduplication;
- choose source-order versus completion-order UI progress for every surface;
- make a compacted transcript an execution ledger;
- require durable Workflow recovery for Action Guide's current independent
  proposal calls;
- select a pattern, concurrency cap, retry count, spill store, retention period,
  or final architecture; or
- copy one upstream Tool runtime wholesale.

### 12.2 Measurable criteria

| Dimension | Required measure / pass criterion |
|---|---|
| **Exposure separation** | Tests independently vary registered, enabled, advertised/deferred, selected and authorized states. Zero unavailable or unauthorized invocation reaches a Tool handler. |
| **Schema/description** | 100% advertised Tools have nonempty bounded descriptions and valid input schemas; dynamic schema/provenance failures fail closed. Hook-updated input is revalidated before execution. |
| **Pairing** | For batches of 1–32 calls with unknown, invalid, denied, cancelled and failed members, every provider-visible call receives exactly one matching result unless a typed Run terminal intentionally ends the conversation. Zero orphan/duplicate call IDs. |
| **Ordering** | Randomize completion timing 10,000 times. Serial/ordered modes produce byte-stable final result and context order; unordered mode, if retained, exposes and tests its explicit completion-order contract. |
| **Admission safety** | Exhaustively test read-only/mutating/external/irreversible/unknown classes. Zero exclusive/terminal overlap; peak shared count never exceeds cap; admission queue/fairness and backpressure are observable. |
| **Terminal Tools** | In every batch position and with two terminal calls, prove the declared winner, sibling start/cancel rule, budget charge, result pairing and typed terminal. Zero effect after the winning barrier. |
| **Cancellation** | Cancel before admission, during hook/approval, during handler, after effect/before result, and during spill. Each attempt reaches one confirmed/unknown terminal; no leaked Tool/Job/process/resource. |
| **Retry/idempotency** | Duplicate start/result/notification delivery and crash at acknowledgement boundaries. Zero duplicate Product apply/publish/remote charge; attempt history distinguishes replayed result from new execution. |
| **Result limits** | Sweep 0 B to 2× each inline/aggregate/spill limit with text, structured, image and malformed results. No silent truncation; locator integrity and access policy hold; model always knows when content is incomplete. |
| **Compaction/resume** | Compact before/after a batch, spill and terminal. Reconstructed context preserves pairing, invoked Tool/schema identity, accepted Artifact reference and pending authority recheck; it never re-executes an effect. |
| **Artifact promotion** | 100% promoted outputs validate schema/hash/source revision/provenance and publish atomically. Raw spills and partial outputs never satisfy Product completion. |
| **Performance/cost** | Compare Pattern A with Pattern B at concurrency 1/2/4/8 for three independent reads, mixed barriers and provider latency distributions. Report p50/p95 wall time, tokens, provider cost, memory, result bytes and cancellation latency. |
| **Privacy** | Default logs/transcripts/spill metadata contain zero raw screenshot bytes, credentials or full sensitive Tool arguments. Retention expiry and explicit deletion remove/tombstone derived resources within the declared SLA. |

## 13. Evidence gaps, required spikes, and exact negative audits

### 13.1 Required bounded spikes before any scheduling change

1. Build a fake Tool harness with deterministic slow reads, revision-checked
   mutations, an ambiguous external start, terminal submission, hook mutation,
   cancellation and oversized output. Compare Pattern A/B ordering and budgets.
2. Inject cancellation at pre-hook, authority decision, scheduler wait,
   handler pre-effect/post-effect, result serialization and spill publication.
   Verify one attempt terminal and no false “skipped” after an effect.
3. If dynamic Tools remain plausible, register a malformed/colliding schema,
   change availability between Steps, revoke authority, resume, and prove
   discovery metadata cannot execute or widen authority.
4. Compact histories containing parallel successes, failures, spills and a
   terminal. Verify pairing and Artifact references with the original result
   bytes unavailable to the model projection.
5. If Pattern C remains plausible, use a three-node diamond with one stale
   Artifact, one missing Artifact and one ambiguous remote start. Crash/restart,
   then prove deterministic readiness and selective retry.
6. Runtime-test the specific external pattern only if Rollshot depends on it:
   Pi extension mutation/order, OMP completion order/terminal abort, Codex
   cancellation/read-write gate, or Claude spill/compaction/sibling abort.

### 13.2 Graph limitations

The Rollshot graph was queried first. It indexed `ToolRegistry`,
`execute_single`, `execute_calls`, `CapabilityStatus`, model call/result types,
and relevant tests in `tools.rs`/`model.rs`; direct source inspection then
bounded current behavior. Each ignored reference checkout returned **0
communities and 0 community pairs**, so pinned direct source/test inspection
was required [G0].

### 13.3 Exact bounded absence audits

- **[A:R-DYNAMIC] Rollshot dynamic/hook/scheduling audit.** Literal roots
  `crates/rollshot-agent/src/{domain,driver,model,provider,runtime,tools}.rs`;
  case-insensitive regex
  `dynamic.?tool|mcp|tool.?search|pre.?tool|post.?tool|hook|side.?effect|idempotenc|attempt.?id|dedup|dependency|depends.?on|dag|parallel|concurr`.
  Hits were one budget-usage deduplication test and OpenAI
  `parallel_tool_calls: false`. A runtime dynamic Tool/MCP/search registry,
  generic pre/post/error hook, common side-effect/idempotency/attempt metadata,
  dependency scheduler, or host-parallel executor was **not found in this
  scope**. Direct reading separately establishes the typed serial registry.
- **[A:R-AUTH] Rollshot payload-authority audit.** The same six agent files;
  regex `authority|authoriz|approval|permission|sandbox`. Hits were
  `AuthorizedInputManifest`/`AuthorizedModelInput` names and uses plus prompt
  wording. Direct inspection of `domain.rs::AuthorizedModelInput::new` and
  `rollshot-app/src/result_workspace/workbench/run.rs` established that
  `payload_mode` upstream selects whether image bytes are included, while the
  constructor validates counts, dimensions, declared/actual bytes and limits.
  A per-Tool authority object, approval mechanism or sandbox contract was
  **not found in the named agent scope**; the type name is not evidence that
  its constructor performs authorization.
- **[A:R-RESULT] Rollshot result-lifecycle audit.** The same six agent files;
  regex
  `result.?store|retention|spill|compaction|compact|artifact.?promot|artifact.?accept|review.?decision`.
  It returned no matches. A common Tool-result store, retention policy, spill,
  compaction projection or generic Product Artifact promotion/acceptance record
  was therefore **not found in this exact scope**. Direct source separately
  establishes bounded inline results and the task-specific `ReadyForReview`
  terminal [E:R1].
- **[A:R-PARALLEL] Current Action Guide scheduling gap.** Reused Round 0 and
  subagent comparison evidence, then audited the literal files
  `rollshot-app/src/timeline_workspace/{visual_annotation_agent,caption_agent,update}.rs`.
  Exact scheduling regex
  `tokio::join!|join_all|select!|futuresunordered|parallel|concurr` returned
  only two unrelated bounded frame-decode concurrency comments in `update.rs`.
  Identity/result terms
  `caption_suggestions_running|visual_annotation_suggestion|caption_agent_run_id|visual_annotation_agent_run_id|document_state_id|stale`
  positively found separate Run state and stale/revision checks. A join or
  scheduler for parallel caption/visual proposal execution, or a measured
  requirement for it, was **not found in these files**.
- **[A:R-IDEMP] Rollshot replay identity audit.** Same six agent files and
  exact terms `idempotenc|attempt.?id|dedup|replay|retry`; the only relevant
  hit was `cumulative_usage_deduplicates_within_turn`, which deduplicates
  budget accounting. A Tool effect operation/attempt/idempotency/replay ledger
  was **not found in this scope**.
- **[A:P-AUTH] Pi authority/discovery audit.** Literal files
  `packages/agent/src/{agent-loop,types}.ts`,
  `packages/agent/test/agent-loop.test.ts`,
  `packages/coding-agent/src/core/{agent-session,session-manager}.ts`, and
  `core/extensions/{types,runner}.ts`; regex
  `authority|authorization|permission.?grant|approval.?cache|sandbox|tool.?search|defer.?load`.
  Positive extension hooks/active Tools remain [E:P1]. A built-in deferred
  ToolSearch, Tool authority grant, approval cache or sandbox contract was
  **not found in this scope**.
- **[A:P-IDEMP] Pi effect/attempt audit.** Same literal roots; regex
  `idempotenc|attempt.?id|dedup|dependency|depends.?on|dag|side.?effect|terminal.?tool`.
  The only unrelated hit was an Authorization-header comment. A generic
  effect-idempotency/attempt/dependency/side-effect-class contract was **not
  found in this scope**.
- **[A:P-ARTIFACT] Pi Tool-result promotion audit.** Reused Reviewed profile
  exact roots `agent-loop.ts`, coding-agent `agent-session.ts`,
  `session-manager.ts`, built-in Tools and extension contracts; terms
  `typed.?artifact|artifact.?registry|expected.?artifact|artifact.?completion|review.?decision|artifact.?provenance`.
  No built-in typed Product Artifact/promotion contract was found; Tool details
  and built-in output paths are retained results, not promotion.
- **[A:O-IDEMP] OMP Tool lifecycle audit.** Literal files
  `packages/agent/src/{agent-loop,types}.ts`, focused agent-loop tests,
  `coding-agent/src/extensibility/{shared-events,extensions/types,extensions/runner,extensions/wrapper,hooks/types,hooks/runner}.ts`,
  and `session/artifacts.ts`; regex
  `idempotenc|attempt.?id|dedup|dependency|depends.?on|dag|terminal.?tool|stop.?after.?success`.
  Positive hits were the special terminal abort reason and unrelated credential
  deduplication. A common Tool effect idempotency/attempt/dependency contract
  and generic stop-after-success field were **not found in this scope**.
- **[A:O-SIDE] OMP side-effect-class audit.** The same OMP literal roots;
  PCRE2 identifier regex
  `\b(?:sideEffect|isReadOnly|isDestructive|isOpenWorld|irreversible|effectClass)\b`.
  It returned no matches. OMP's positive `approval` tier, `intent`,
  `interruptible`, and shared/exclusive `concurrency` fields remain [E:O1], but
  a common Tool side-effect class equivalent to the audited identifiers was
  **not found in these roots**.
- **[A:O-ARTIFACT] OMP Product Artifact audit.** The same roots, including
  `session/artifacts.ts`; PCRE2 identifier regex
  `\b(?:artifactValidation|validateArtifact|artifactAcceptance|acceptArtifact|reviewDecision|expectedArtifact|productArtifact|promoteArtifact)\b`.
  It returned no matches. The positive session spill manager allocates and
  resolves overflow files [E:O3]; Product Artifact validation, review
  acceptance, expected-output gating or promotion was **not found in this
  exact scope**.
- **[A:C-LIFECYCLE] Codex Tool lifecycle audit.** Literal roots
  `codex-rs/core/src/{session/turn.rs,tools/parallel.rs,tools/router.rs,tools/registry.rs,hook_runtime.rs}`
  and `tools/handlers/dynamic.rs`; regex
  `idempotenc|attempt.?id|dedup|dependency|depends.?on|dag|terminal.?tool|stop.?after.?success|result.?store|retention|spill|artifact.?promot`.
  No matches. A generic Tool effect idempotency/attempt/dependency/terminal,
  result spill/store/retention, or Artifact-promotion contract was **not found
  in this focused scope**.
  Direct source separately establishes runtime call IDs, gate ordering,
  dynamic calls and hook behavior [E:C1-E:C3].
- **[A:C-ERROR-HOOK] Codex Tool-error-hook audit.** The same Codex literal
  roots; exact regex
  `PostToolUseFailure|post_tool_use_failure|post_tool_failure|run_post_tool_failure_hooks|run_tool_error_hooks|ToolErrorHook`.
  It returned no matches. Direct reading of `tools/registry.rs` and
  `hook_runtime.rs` shows `PostToolUse` is built only when Tool execution reports
  success; failed/aborted lifecycle events and telemetry remain positive
  evidence, but a named `PostToolUseFailure` or equivalent generic Tool-error
  hook was **not found in this path** [E:C3].
- **[A:L-ORDER] Claude scheduling order/cap audit.** Literal files
  `src/services/tools/{toolOrchestration,StreamingToolExecutor}.ts`,
  `src/utils/generators.ts`, and `src/query.ts`; regex
  `getMaxToolUseConcurrency|CLAUDE_CODE_MAX_TOOL_USE_CONCURRENCY|all\(|Promise\.race|runToolsConcurrently|queuedContextModifiers|contextModifiers|getCompletedResults|getRemainingResults|canExecuteTool|isConcurrencySafe|status === 'executing'|status === 'completed'`.
  The only numeric Tool concurrency cap is passed by nonstreaming
  `runToolsConcurrently` to `all()`, whose `Promise.race` yields completion
  order. The streaming executor has no visible numeric cap; its queue admits
  safe calls together, yields completed safe calls without waiting for earlier
  executing safe calls, and stops result traversal at an executing unsafe call.
  Nonstreaming concurrent context modifiers are replayed in source order;
  streaming concurrent-safe modifiers are explicitly unsupported in the
  inspected implementation.
- **[A:L-LIFECYCLE] Claude Tool lifecycle audit.** Literal files
  `src/{Tool.ts,query.ts}`,
  `src/services/tools/{toolOrchestration,StreamingToolExecutor,toolExecution,toolHooks}.ts`,
  and `src/utils/toolResultStorage.ts`; regex
  `idempotenc|attempt.?id|dedup|dependency|depends.?on|dag|terminal.?tool|stop.?after.?success|artifact.?promot`.
  Hits were a sibling-error comment about implicit command dependencies and
  unrelated memory deduplication. A generic Tool effect idempotency/attempt,
  dependency scheduler, terminal Tool, or Product Artifact promotion contract
  was **not found in this scope**. Spill-file create-new behavior is positive
  result-retention evidence, not effect deduplication [E:L2].
- **[A:ALL-DAG] Cross-system Tool dependency audit.** The exact
  dependency/DAG terms in [A:R-DYNAMIC], [A:P-IDEMP], [A:O-IDEMP],
  [A:C-LIFECYCLE], and [A:L-LIFECYCLE] found no Tool-call dependency scheduler.
  Claude work-ledger dependencies, OMP Task fan-out, and Hyperframes workflow
  instructions were inspected separately and are not silently reclassified as
  Tool-batch scheduling [E:L0, E:S1].

Every absence above is limited to the named roots, terms, and pinned revision.
It does not prove that an uninspected extension, internal module, later version,
or product layer cannot provide the capability.

## 14. Evidence index and limitations

### Rollshot and workload evidence

- **[E:R0] Round 0 source/test synthesis:**
  `00-rollshot-baseline-workloads.md` at its recorded Rollshot/Rig revisions.
  Supports the bounded Run, 16-dimensional budget, typed terminals, workload
  ladder and exact pinned Rig call/result invariants.
- **[E:R1] Rollshot source/test source:**
  `crates/rollshot-agent/src/tools.rs` (`Tool`, `ToolRegistry`, limits,
  `execute_calls`, Tool implementations), `driver.rs` (`AgentTaskProfile`,
  `run_with_provider`, `run_tool_turn`), `model.rs` (`ToolDefinition`,
  `ModelMessage`) and `runtime.rs` (`RunBudget`, cancellation/events).
- **[E:R2] Rollshot product construction:**
  `crates/rollshot-app/src/result_workspace/workbench/run.rs::build_authoring_tool_registry`
  plus visual-annotation registry construction in `driver.rs`. Supports
  product-selected per-Run availability; UI/provider not launched.
- **[E:R3] Pinned Rig 0.39 source/test source:** locally resolved
  `rig-core-0.39.0/src/agent/run/{mod,streamed}.rs`, as recorded by Round 0.
  Supports exhaustive steps and complete call/result pairing; local registry
  path is machine-specific but version/checksum are recorded in Round 0.
- **[W1] Smart Redaction workload:** Round 0 [R3, R5, R8], especially
  generation-bound validate/dry-run/submit and typed review terminal.
- **[W2] Action Guide workload:** Round 0 [A1-A3], especially durable Project
  revision, bounded caption/visual proposals and stale-result rejection.
- **[W3] brag/Hyperframes workload:** Round 0 [B1, H1-H3], specifically
  dependency stages, review gates, expected Artifacts and optional workers.
- **[E:S1] Adjacent capability evidence:** reviewed
  `task-todo-workflow-state.md`, `subagents-and-parallelism.md`, and
  `persistence-checkpoint-resume.md`. Supports separation of Tool batch,
  Product Workflow, Child Agent, attempt, checkpoint and Artifact completion.
- **[E:J1] Adjacent Job evidence:** `long-running-jobs.md`. Supports external
  start acknowledgement, provider idempotency key, confirmed/unknown cancel,
  Collect and Artifact publication semantics.
- **[E:C1] Adjacent compaction evidence:** `context-compaction.md`. Supports
  system-specific projection/retention behavior and the rule that compaction is
  not persistence or executable Workflow state.

### External source and test evidence

- **[E:P0] Pi Reviewed profile:** `systems/pi.md`; status and evidence labels
  remain authoritative for this comparison.
- **[E:P1] Pi source:** `packages/agent/src/{agent-loop,types}.ts` and
  coding-agent `core/{agent-session.ts,extensions/runner.ts,extensions/types.ts,tools}`.
  Supports active registration, schema/pre/post hooks, ordering and file queue.
- **[T:P1] Pi tests inspected, not run:**
  `packages/agent/test/agent-loop.test.ts`, especially sequential override,
  parallel overlap/source ordering, steering, failure and terminate cases.
- **[E:O0] OMP Reviewed profile:** `systems/oh-my-pi.md`; its feature/status
  and availability/authority distinctions are retained.
- **[E:O1] OMP source:** `packages/agent/src/{agent-loop,types}.ts`, coding-agent
  Tool/custom/capability registries and extension wrapper. Supports
  shared/exclusive scheduling, essential/discoverable metadata and approval.
- **[E:O2] OMP source:** terminal abort in `agent-loop.ts`, Task event handling
  in `task/executor.ts`, and `tools/yield.ts`. Supports Task-specific terminal
  `yield`; child run not executed.
- **[E:O3] OMP source:** `session/artifacts.ts`,
  `internal-urls/artifact-protocol.ts`, and Tool output metadata. Supports
  session spill IDs, resumed scan and inline cap.
- **[T:O1] OMP tests inspected, not run:**
  `packages/agent/test/agent-loop.test.ts`, shared overlap,
  completion-ordered results, function concurrency, interrupt/terminal cases.
- **[E:C0] Codex Reviewed profile:** `systems/codex.md`; feature statuses and
  authority boundaries remain authoritative.
- **[E:C1] Codex source:**
  `codex-rs/core/src/tools/{parallel,router,registry}.rs` and
  `session/turn.rs`. Supports read/write admission, cancellation and
  `FuturesOrdered` result recording.
- **[E:C2] Codex source:** `tools/handlers/{dynamic,tool_search,mcp}.rs` and
  Tool registry/exposure contracts. Supports dynamic/MCP/deferred Tools and MCP
  parallel annotations.
- **[E:C3] Codex source:** `hook_runtime.rs`, `tools/registry.rs`,
  `tools/approvals.rs` and handler-specific approval/sandboxing. Supports
  pre/post/permission hook phases; runtime dialogs not exercised.
- **[E:C4] Codex source:** rollout reconstruction/compaction and generic Tool
  output truncation cited by the Reviewed profile. Supports projection versus
  persisted rollout; no generic spill Artifact claim.
- **[T:C1] Codex tests inspected, not run:** tests in `tools/parallel.rs`,
  `tools/registry_tests.rs`, MCP parallel tests, context/result truncation and
  cancellation suites.
- **[E:L0] Claude Code Reviewed profile:** `systems/claude-code.md`; external
  build gates and hidden/unavailable modules remain limitations.
- **[E:L1] Claude source:** `src/Tool.ts`,
  `services/tools/{toolOrchestration,StreamingToolExecutor,toolExecution,toolHooks}.ts`,
  `src/utils/generators.ts`, and `src/query.ts`. Supports metadata, deferred
  selection, the distinct nonstreaming/streaming schedulers, completion-order
  safe-call results, nonstreaming-only cap/source-ordered context modifiers,
  hooks, permission, sibling error and cancellation [A:L-ORDER].
- **[E:L2] Claude source:** `src/utils/toolResultStorage.ts` and `query.ts`.
  Supports per-Tool/aggregate spill, call-ID create-new behavior, replacement
  persistence gate and ordering before microcompact.
- **[T:L1] Claude test/source confidence:** focused scheduling behavior was
  rechecked against the pinned source paths in [A:L-ORDER]; the external suite
  was not run in this task.
- **[G0] Graph evidence:** Rollshot file summaries plus four reference
  architecture queries. Reference roots returned zero communities/nodes; this
  is a coverage limitation, not source evidence.

### Limitations and open synthesis questions

Confidence is **high** for visible pinned types, call order, lock/barrier
algorithms, result pairing, status labels, and exact bounded audits; **medium**
for source plus focused tests that were not executed; and **low-to-medium** for
external permission UI, cancellation races, process/provider ambiguity,
hidden Claude context algorithms, deployment configuration, and compaction or
resume under failure.

Open questions for synthesis are:

1. Does any measured Rollshot workload save enough latency to justify Pattern B
   after classification, budget and cancellation complexity?
2. Which component owns Tool availability/authority snapshots if future MCP or
   dynamic providers are admitted, and how are schema revisions pinned?
3. Should descriptions move into typed Tool definitions even if discovery
   remains static?
4. Is spill needed for bounded inspection results, or should those Tools expose
   selectors/pagination and continue failing oversized results?
5. Which results merit Product Artifact promotion, and which retention/privacy
   policy applies to raw spills versus accepted proposals?
6. If Pattern C becomes necessary, can Product Workflow ownership remain
   outside the bounded agent loop and reuse Pattern A/B only within ready nodes?
7. What runtime evidence is required before trusting third-party read-only,
   concurrency, idempotency, or cancellation declarations?
