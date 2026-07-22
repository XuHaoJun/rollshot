# Budgets, cancellation, retries, and failures comparison

**Research date:** 2026-07-22 (Asia/Taipei)  
**Status:** In Progress (Round 3 capability comparison)  
**Umbrella revision:** 1  
**Current Rollshot revision:** `709d4ca7fd3c83ee388f2b4a8798f9ea13d34924`  
**Reference revisions:** Pi `dd6bea41efa8caa7a10fe5a6401676dc5699f83f`;
oh-my-pi `7b141199d524b859c357fc89654f10b62b9f3df1`; Codex
`4a443994bd12f49f2f08b21a2f224d9d42b9e734`; Claude Code source
`2ca5ddabfed5f220812ea11f029eda03b21bc4c1`.  
**Evidence mode:** static source, test-source, and repository-document
inspection. No provider request, cancellation race, child tree, background Job,
process kill, crash/Resume, retry storm, or ambiguous external effect was
executed.

This document compares resource governance and actionable failure semantics. It
does **not** select a final Rollshot architecture.

## 1. Rollshot problem and workload evidence

A budget is useful only if the owner can say what is being bounded, when work is
admitted, who is charged, what happens after cancellation, and whether a retry
is a replay or a new effect. One integer named `max_tokens` cannot govern a Tool
batch, child tree, local process, remote render, and accepted Product Artifact.

| Workload | Concrete trace | Governance pressure actually established |
|---|---|---|
| **Smart Redaction** | One bounded model/Tool loop owns a finite 16-dimensional `RunBudget`, one cancellation source, serial Tools, generation-bound validation/dry-run evidence, and a typed review/failure terminal. [W1] | Requires hard local ceilings, cancellation into provider/automation work, recoverable validation feedback, and actionable typed terminals. It does **not** establish Child Agent, Job, Workflow, Artifact-count, retry, or parallelism budgets [A:R-HIER]. |
| **Action Guide** | Durable project revisions surround independent caption and visual-annotation calls. Visual annotation has a fresh bounded Run; proposal results are rejected when their document revision is stale. The separate video-import path has live operation identity, cancellation, process reaping, staged output, and cleanup. [W2] | Requires revision-bound outcomes and stale-result classification rather than provider retry. If several proposals or media operations overlap later, resource ownership must remain product-visible. Current source does **not** establish a foundation parent/child or durable Job budget [A:R-HIER, A:R-DURABLE]. |
| **Deferred brag + Hyperframes** | Optional workers build expected scene Artifacts; preview, media generation, local render, and remote render may outlive a model turn; checkpoints and Artifact prerequisites govern readiness. [W3] | If adopted, requires hierarchical reservations, concurrency and Job ceilings, durable cancellation intent, retry-safe attempts, external idempotency, partial-Artifact quarantine, and recovery accounting. It does not mandate video generation or a general coding-agent platform. |

The ladder therefore has three materially different scopes: one Agent Run,
one parent with bounded children, and a Product Workflow with Jobs and
Artifacts. A single shared counter would conflate their lifetimes and failure
owners.

## 2. Terms and non-equivalent controls

### 2.1 Budget vocabulary

| Term | Meaning in this comparison | Non-equivalence rule |
|---|---|---|
| **Limit** | A fixed threshold on one dimension, such as four model calls or 8 MiB of Tool output. | A concurrency cap or provider context window is a limit, but not automatically spend allocation. |
| **Budget** | A limit plus scope, owner, accounting rule, exhaustion outcome, and retry/Resume policy. | Recording usage without an exhaustion policy is telemetry, not a budget. |
| **Reservation** | Capacity removed from an owner's available balance before admitting work whose final charge is not yet known. | A child slot reservation prevents oversubscription of threads; it is not a token/cost reservation unless those dimensions are also debited. |
| **Allocation** | The maximum vector a parent grants to a child/Job/attempt. | Copying the parent's full configured limits into every child permits aggregate overcommit. |
| **Charge** | Observed usage assigned to exactly one operation/attempt and rolled up to its owners. | Provider cumulative usage must be delta-normalized before charging; a retry is not free. |
| **Reclaim** | Return the unused part of a completed/cancelled reservation after its final charge is known. | Unknown external effects/cost remain reserved or enter reconciliation; they are not optimistically refunded [E:J1, E:PERSIST]. |
| **Overcommit** | Simultaneous reservations exceed an owner's remaining capacity. | Actual provider usage exceeding an estimate is an overrun, which needs a typed policy; it does not justify admitting more work. |
| **Soft limit** | Warning, steering, graceful-yield, or admission-pressure threshold; work may continue under explicit policy. | A reminder is not a hard stop and must not be presented as guaranteed spend control. |
| **Hard limit** | Host-enforced refusal, cancellation, or typed exhaustion boundary. | Post-response token enforcement is still a hard terminal policy, but cannot undo already consumed provider tokens. |

### 2.2 Failure, cancellation, and retry vocabulary

- A **failure class** identifies the owner and safe next action: Provider,
  Protocol, Validation, Runtime/Tool, Authority, Blocked, Needs Input,
  Cancelled, Exhausted, or Reconciliation.
- A **terminal** is the host-owned end state of one scoped attempt/Run/Job. A
  provider stop reason, process exit, Tool result, final prose, or child
  notification is not automatically a Product terminal.
- **Cancellation requested** is intent. **Cancellation confirmed** means the
  authoritative owner has stopped or was already terminal. Cancellation may be
  unknown for remote work or a process tree after a lost acknowledgement.
- A **retry** is a new attempt under an explicit owner and remaining budget. A
  replay returns an already-recorded result without re-executing the effect.
- A **provider retry** repeats transport/model sampling. A **protocol retry**
  repairs a dropped stream or malformed exchange. A **validation retry** asks
  for corrected content. A **Tool/Job/Workflow retry** may repeat effects and
  therefore needs stronger idempotency and reconciliation.
- A **logical operation ID** names the Product intent, an **attempt ID** names
  one execution and charge, a **model call ID** pairs Tool use/result, and a
  **provider idempotency key** deduplicates an external start. They are not
  interchangeable.

## 3. Current Rollshot behavior

### 3.1 The 16-dimensional bounded Run

`RunBudget`, `UsageSnapshot`, and `BudgetDimension` define the current
Rollshot-owned envelope [E:R1]:

| Requested control area | Current dimension / behavior | Enforcement character |
|---|---|---|
| Token | `InputTokens`, `OutputTokens` | Usage becomes known during/after the provider stream; an overage returns dimensioned `BudgetExhausted`, but already-consumed tokens cannot be undone. |
| Cost | `Cost` field and comparison | The driver never charges provider prices, so production usage remains zero and the configured ceiling is ineffective. Token/model-call ceilings are the current spend proxy [E:R1, A:R-COST]. |
| Wall time | `WallTime` | Checked before and between model/Tool work. Stream conversion selects between `stream.next()` and the deadline, but observes cancellation only before starting each item poll. Initial establishment and an established stream with one permanently pending item therefore have distinct cancellation gaps [A:R-CANCEL]. |
| Tool calls | `ToolCalls`, `PerToolCalls` | Batch counts are charged before Tool execution; the registry separately enforces per-Tool counts. |
| Tool bytes | `ArgumentBytes`, `ResultBytes`, `SourceBytes`, `Attachments` | Argument deltas and assistant/source bytes are bounded while streaming; results and attachments are bounded at their boundaries. `SourceBytes` also backs the aggregate assistant-text cap in the driver. |
| Validation/evaluation | `ValidationAttempts`, `DryRunAttempts`, `CapabilityCalls`, `CandidateCount`, `AffectedArea` | Attempt counts are precharged where known; result-derived capability/candidate/area usage is charged after execution. |
| Child Agent | No current dimension or child owner was found [A:R-HIER]. | Not applicable to the current serial Run. |
| Job/process | No foundation Job lifetime/budget was found [A:R-HIER, A:R-DURABLE]. | Product video import has live resource/cancellation behavior, not this Run budget. |
| Artifact | No generic Artifact count/byte/retention budget was found [A:R-HIER]. | `ReadyForReview` promotes one task-specific typed proposal; ordinary files/results are not budgeted Artifacts. |
| Retry | No retry-attempt/token/cost budget or durable attempt ledger was found [A:R-HIER, A:R-RETRY]. | Model correction consumes ordinary model/Tool/validation dimensions. |
| Parallelism | No Tool/child/Job parallelism budget exists in the agent scope [A:R-HIER]. | Tools execute serially; OpenAI requests also set `parallel_tool_calls: false`. |

The tracker stages usage in a current-Turn snapshot and rolls it into accumulated
usage through `apply_turn()`. `check_accumulated()` detects overage after a
commit boundary; some resource consumption is necessarily post-observation.
The scheme is run-local and in memory. It has no durable operation/attempt ID,
reservation ledger, or parent roll-up [E:R1, A:R-DURABLE].

### 3.2 Hard versus soft behavior

Current Rollshot has hard ceilings and no budget-warning/yield protocol in the
investigated agent scope [A:R-HIER]:

- wall time, model-call protocol depth, assistant/argument/result bytes,
  attachments, Tool counts, validation/dry-run/capability/candidate/area and
  token dimensions terminate or reject work;
- Tool and attachment limits can reject before an effect, while provider token
  and result-derived dimensions can only terminate after cost/resource use is
  observed;
- `max_turns` is additionally enforced by Rig's protocol state machine and is
  mapped to the applicable terminal path; and
- no warning threshold, parent pressure signal, graceful child yield, or
  reservation-estimate policy was found [A:R-HIER].

Cost is the important false-hard edge: the type looks enforceable, and unit
tests can charge synthetic cost, but the production driver documents that no
pricing function supplies charges. It must be reported as **declared but not
operationally enforced**, not as a functioning spend cap [E:R1, A:R-COST].

### 3.3 Cancellation propagation and cleanup

One `RunCancellation::cancel()` fans into a Tokio `CancellationToken` and the
automation executor's `CancellationFlag`. The driver checks the token before
and between model/Tool phases; `ToolRegistry` checks before each serial call;
and dry-run automation receives the paired flag [E:R1, E:R2]. Provider stream
cancellation is weaker: `stream_to_model_events` checks cancellation before
starting each item poll, then its `select!` waits only for the absolute deadline
or `stream.next()` [A:R-CANCEL].

The concrete Anthropic/OpenAI adapters await
`model.stream(completion_request)` before wrapping the returned stream in
`stream_to_model_events(bounds)`, while the driver also awaits
`provider.stream(request, bounds)` directly. No `select!` around this initial
stream-establishment future was found. Static inspection therefore does not
establish that cancellation or the Rollshot deadline interrupts a concrete
provider request that stalls before yielding the stream [A:R-CANCEL]. After
establishment, the deadline is selected concurrently with `stream.next()`, but
cancellation is not. If one item poll remains pending indefinitely, cancellation
does not wake that poll; observation may be delayed until the item resolves or
the deadline fires. Establishment-stall, established-item-stall, and
cancel/deadline-race behavior remain runtime gaps, including which terminal is
reported and at what latency [A:R-CANCEL].

A Child Agent, Job/process registry, hook lifecycle, cancellation-intent store,
or generic cleanup/compensation protocol was not found in the six-file agent
scope [A:R-HIER, A:R-DURABLE]. That bounded absence does not erase the separate
Action Guide import process-reaping and scratch-cleanup evidence [W2].

### 3.4 Retry ownership and current terminals

The Rollshot facade/driver has no automatic provider/protocol/Tool retry loop,
backoff, jitter, idempotency key, or attempt ledger in the investigated scope
[A:R-RETRY]. A provider/stream error maps to `ProviderFailure`; malformed
Tool-call/Rig state maps to `AgentProtocolFailure`. Recoverable Tool argument,
validation, and dry-run failures are returned to the model so a later model
call may correct them, spending the same Run budget. This is bounded model-led
correction, not transparent replay.

The main Smart Redaction terminal taxonomy is already unusually explicit:

| Terminal | Meaning and next owner |
|---|---|
| `ReadyForReview` | Typed proposal/evidence handoff; product review owns the next decision. |
| `NeedsUserInput` | The Run cannot continue without a concrete user answer; not a generic dependency-blocked state. |
| `Cancelled` | Current live Run observed cancellation. It does not prove a nonexistent child/Job tree was cleaned [A:R-HIER]. |
| `BudgetExhausted { dimension }` | One named Run dimension ended work. Scope is the current Run only. |
| `SourceValidationFailure` | Model/tool correction ended without valid source. |
| `RuntimeFailure` | Dry-run or handoff/runtime failure. |
| `AgentProtocolFailure { message }` | Rig/Tool-call/result or driver invariant failure. |
| `ProviderFailure { message }` | Provider creation/request/stream failure after sanitization/mapping. |

The visual-annotation path has its own typed `Suggested`, `NoSuggestion`,
`Cancelled`, dimensioned `BudgetExhausted`, `ProviderFailure`, and
`ProtocolFailure` terminal set. This is intentionally narrower and omits raw
provider/prompt/attachment content [E:R2].

## 4. Cross-system budget comparison

Every negative or unknown cell cites an exact audit in Section 13. Similar
names in this first inventory are not treated as equivalent enforcement; the
companion matrix in Section 4.1 makes owner/scope, charge timing, force,
outcome, and retry/Resume accounting explicit.

| Control | Rollshot | Pi | oh-my-pi | Codex | Claude Code source |
|---|---|---|---|---|---|
| **Token** | Hard input/output Run dimensions; post-observation terminal [E:R1]. | Usage/statistics exist, but a finite product Run token budget was not found [A:P-BUD]. | Goal has an optional post-usage token budget and persisted used amount; Task request/yield controls are separate. No unified parent tree budget was found [E:O1, A:O-HIER]. | Default-off `token_budget` provides context guidance; default-off tree-shared `rollout_budget` gives reminders and, after recorded usage reaches its limit, `SessionBudgetExceeded` [E:C1]. | Query has `maxTurns`, `maxBudgetUsd`, and feature-gated task/token budget paths; no unified hierarchy was found [E:L1, A:L-HIER]. |
| **Cost** | Field exists, but production driver does not charge it [E:R1, A:R-COST]. | Provider/session cost is recorded; finite cost governance was not found [A:P-BUD]. | Provider usage/fallback accounting exists; unified hard cost ceiling was not found [A:O-HIER]. | Rate/usage telemetry exists; mandatory hard cost budget was not found [A:C-HIER]. | Query `maxBudgetUsd` checks already accumulated cost after a yielded message and returns `error_max_budget_usd`; it is a post-consumption stop, not pre-admission reservation [A:L-HIER]. Parent/child reconciliation was not found [E:L1, A:L-HIER]. |
| **Wall time** | Hard Run deadline at observed boundaries; establishment and established-item cancellation gaps remain [A:R-CANCEL]. | Provider timeout is configurable; finite whole-Run wall budget was not found [A:P-BUD]. | Core deadline and Task `maxRuntimeMs` are hard local aborts; the request/yield budget instead begins as soft steering [E:O1]. | Stream idle timeout defaults 300 s; Turn/tool/process timeouts are component-owned. Unified Run/Workflow wall budget was not found [A:C-HIER]. | Task/tool/remote-component timeouts exist; one hierarchical wall budget was not found [A:L-HIER]. |
| **Tool calls / bytes** | Tool/per-Tool calls and argument/result/source/attachment bytes are explicit [E:R1]. | Tool/session usage exists; finite Tool-call/byte governance was not found [A:P-BUD]. | Task request/yield first steers, while returned-output byte/line caps hard-truncate after capture; one run-wide Tool-call/byte budget was not found [E:O1, A:O-HIER]. | Tool counters/output truncation exist; hard tree Tool-call/byte budget was not found [A:C-HIER]. | Result spilling/size thresholds exist; unified Tool-call/byte budget was not found [A:L-HIER]. |
| **Child** | Child budget/allocation not found [A:R-HIER]. | Built-in child lifecycle and child budget not found; subprocess example policy is extension-local [A:P-BUD]. | Task request/runtime/output/recursion limits are configured per child; allocation/reclaim from a parent vector was not found [E:O1, A:O-HIER]. | Tree-shared rollout usage/reminders exist, but per-child allocation/reserve/reclaim was not found [E:C1, A:C-HIER]. | `maxTurns`/agent settings can cap children; a visible hierarchical child-budget policy was not found [A:L-HIER]. |
| **Job** | Foundation Job budget not found [A:R-HIER]. | Built-in Job budget/lifecycle not found [A:P-BUD]. | Direct non-queued Job registration hard-rejects at the manager cap; caller-parked queued Jobs bypass that count until caller `markRunning()`, which does not recheck it. Token/cost/resource allocation from parent was not found [E:O2, A:O-ADMISSION, A:O-HIER]. | Background terminal/exec limits exist; a durable Job resource budget was not found [A:C-HIER]. | Runtime Task limits exist; a common Job budget was not found [A:L-HIER]. |
| **Artifact** | Generic Artifact count/bytes/retention budget not found [A:R-HIER]. | Generic Artifact budget was not found [A:P-BUD]. | Spill files have size controls, but Product Artifact budget was not found [A:O-HIER]. | Generic Artifact budget/type was not found [A:C-HIER]. | Heterogeneous output paths exist; a common Artifact budget was not found [A:L-HIER]. |
| **Retry** | Ordinary dimensions charge correction, but retry budget/attempt ledger not found [A:R-RETRY]. | Agent retry count is a local retry policy, not a total resource reservation [E:P1, A:P-HIER]. | Provider/Task/schema/Job delivery layers have local retry budgets; no shared durable attempt ledger was found [E:O2, A:O-HIER]. | Request/stream retry maxima exist; tree resource reservation for retries was not found [E:C2, A:C-HIER]. | Component retry limits/circuit breakers exist; a durable workflow retry budget was not found [E:L2, A:L-HIER]. |
| **Parallelism** | Serial Tool loop; no parallelism budget [A:R-HIER]. | Core Tool parallelism exists; no child/Job budget in built-in scope [A:P-BUD]. | The per-session Task semaphore waits for a spawn slot. Direct `AsyncJobManager` registration instead throws at its cap. A Job registered `queued: true` is parked by its caller and later promoted by caller `markRunning()`; the manager supplies no admission queue, fairness policy, or cap reacquire [E:O1, E:O2, A:O-ADMISSION]. | V1/V2 agent caps and Tool read/write gate are positive; a spend allocation tied to those slots was not found [A:C-HIER]. | Tool safe-call cap is path-specific and agent/team cap is not visible in the external roots [A:L-HIER]. |

### 4.1 Enforcement companion matrix

| System / control | Owner and scope | Admission / accounting point | Enforcement character | Exhaustion / limit outcome | Retry and Resume accounting |
|---|---|---|---|---|---|
| **Rollshot — model/Tool/validation calls and request bytes** | One live `AgentRunner` Run; per-Tool count is also registry-local [E:R1]. | Known call counts, arguments, attachments, and attempt counts are charged or rejected before their governed effect; accumulated checks also occur at Turn commit [E:R1]. | Hard pre-admission or pre-effect ceiling. | Dimensioned `BudgetExhausted` or boundary rejection; the named dimension owns the stop [E:R1]. | Model correction is a new ordinarily charged call. Tracker/attempt reconstruction on Resume was not found [A:R-RETRY, A:R-DURABLE]. |
| **Rollshot — provider tokens and result-derived usage** | One live Run; input/output tokens, result/source bytes, capability calls, candidates, and affected area [E:R1]. | Observed while streaming or after Tool/result completion; consumption can precede the check [E:R1]. | Hard **post-observation** terminal, not admission reservation [A:R-HIER]. | Dimensioned `BudgetExhausted`; already consumed provider/Tool resources are not undone [A:R-HIER]. | Corrections charge the same Run. Retry reservation and durable usage reconstruction were not found [A:R-RETRY, A:R-DURABLE]. |
| **Rollshot — declared cost** | `RunBudget.cost` / `UsageSnapshot.cost` in one live Run [E:R1]. | Synthetic charge paths exist, but the production driver supplies no provider-price charge [A:R-COST]. | Declared but inactive: neither enforceable nor meaningful cost telemetry in the current product path. | No production cost exhaustion can be relied upon while usage remains zero [A:R-COST]. | No retry cost allocation or durable cost reconciliation was found [A:R-HIER, A:R-DURABLE]. |
| **Rollshot — wall time / stream** | One live Run deadline plus provider adapter boundaries [E:R1, E:R2]. | Tracker checks happen between phases; established stream conversion selects deadline versus next item, while cancellation is checked only before the item poll [A:R-CANCEL]. | Hard when a boundary/deadline is observed; neither establishment nor a pending item poll selects cancellation. | Tracker boundary can name `WallTime`; in-stream deadline follows the provider-stream error path. Exact cancel/deadline race terminal and latency are runtime gaps [A:R-CANCEL]. | Backoff is absent [A:R-RETRY]; live elapsed time is not reconstructed on Resume [A:R-DURABLE]. |
| **Pi — token/cost/Tool usage** | Coding-agent Session/usage statistics [E:P1]. | Recorded after provider/Tool activity. | Telemetry; a finite product Run ceiling was not found [A:P-BUD]. | A product budget-exhaustion terminal was not found [A:P-BUD, A:P-TERMINAL]. | Transient retries consume real usage, but no shared retry allocation or durable budget ledger was found [A:P-HIER]. |
| **Pi — timeout and transient retry** | Provider request / current agent turn, configured locally [E:P1]. | Timeout races live work; agent transient retry occurs after classified failure. No parent reservation was found [A:P-HIER]. | Timeout is hard for that request; retry count is a local hard maximum, not a spend budget [A:P-BUD, A:P-HIER]. | Request error after timeout/retry exhaustion; no common typed Product terminal was found [A:P-TERMINAL]. | Agent retry defaults to three with 2/4/8 s exponential delays; provider retry defaults to zero. Attempt/usage Resume ledger was not found [E:P1, A:P-HIER]. |
| **oh-my-pi — Goal token budget** | One Goal owns `tokenBudget`, `tokensUsed`, and elapsed usage [E:O1]. | Usage is accumulated after provider responses and compared to the optional Goal budget [E:O1]. | Hard Goal-state boundary after observed use, not child pre-allocation [A:O-HIER]. | Goal becomes `budget-limited` [E:O1]. | Budget/used/time persist; Thread Resume pauses active Goal by default and resets the live timing baseline. Child-tree accounting remains absent [E:O1, A:O-HIER]. |
| **oh-my-pi — Task request/yield budget** | One child Task run [E:O1]. | Request count is observed after calls: first a wrap-up notice, then forced final `yield`, then abort after a non-cooperative grace overrun [E:O1]. | Soft steering followed by a hard forced-yield/grace boundary; not equivalent to a token budget [A:O-HIER]. | Graceful yielded result when cooperative; otherwise abort/error identifying the exceeded soft request budget [E:O1]. | Scoped schema/provider correction may retry, but no durable parent attempt/charge ledger was found [A:O-HIER]. |
| **oh-my-pi — Task runtime and returned output** | One child Task; `maxRuntimeMs` and byte/line output caps [E:O1]. | Runtime timer races live execution; output is captured and then hard-truncated to byte/line maxima [E:O1]. | Hard runtime abort; hard post-capture returned-output cap. The latter limits the returned payload, not upstream generation spend [A:O-HIER]. | Timeout abort/error for runtime; truncated result plus metadata for output cap [E:O1]. | Child controls are configured, not reserved/reconciled from the parent; live promises/controllers are not a durable Resume ledger [A:O-HIER]. |
| **oh-my-pi — Task spawn semaphore** | One `TaskTool` instance, therefore one Session's Task spawns [E:O1, A:O-ADMISSION]. | Each sync or async Task body calls the abortable semaphore `acquire()` and waits until a per-session spawn slot is released [A:O-ADMISSION]. | Hard local concurrency cap with caller-side waiting; this is the Task admission gate. | The Task starts after acquiring a slot or exits its wait on abort; no Product budget-exhaustion terminal is created [A:O-ADMISSION, A:O-TERMINAL]. | The live semaphore is resized in place, but its waiters/permits are not reconstructed after process death [A:O-HIER]. |
| **oh-my-pi — direct `AsyncJobManager` registration** | Process-local manager and its `maxRunningJobs`; it counts `status === running && !queued` [E:O2, A:O-ADMISSION]. | `register()` counts active non-queued Jobs before inserting the new Job [A:O-ADMISSION]. | Hard synchronous admission error; the manager does not wait or enqueue the rejected registration [A:O-ADMISSION]. | Throws `Background job limit reached (<cap>). Wait for running jobs to finish or cancel one.` Retained completed records later expire [E:O2, A:O-ADMISSION]. | A caller may retry registration later, but the manager owns no admission queue/fairness/reacquire protocol and its state is not reconstructed after process death [A:O-ADMISSION, A:O-HIER]. |
| **oh-my-pi — caller-parked queued Job** | Caller-created Job registered with `queued: true`; the caller owns the actual gate [E:O2, A:O-ADMISSION]. | The manager excludes it from `maxRunningJobs`. In the Task path, the Job body waits on the per-session Task semaphore, then calls `markRunning()` after acquiring that permit [A:O-ADMISSION]. | `queued` is a counting flag, not a manager admission queue. `markRunning()` only clears the flag; it performs no manager-cap reacquire or fairness check [A:O-ADMISSION]. | Caller cancellation can end the parked body; otherwise caller admission starts it. The manager emits no capacity outcome at promotion [A:O-ADMISSION]. | Caller policy owns any retry/wait order. Manager delivery retry applies after completion, not to this admission gate; neither caller queue nor manager state is durable [E:O2, A:O-ADMISSION, A:O-HIER]. |
| **Codex — default-off `token_budget`** | Current Session/Thread context policy [E:C1]. | Context usage drives reminder/compaction guidance [E:C1]. | Soft guidance/reminder; under-development and default-off. | Guidance/compaction rather than a mandatory spend terminal [E:C1]. | It is not a durable tree-spend allocation; Resume reconstruction was not found [A:C-HIER]. |
| **Codex — default-off tree `rollout_budget`** | One weighted counter shared through `AgentControl` across the root Session tree [E:C1]. | Usage is recorded post-response; pending per-Thread/window reminders are injected before later work [E:C1]. | Soft reminders before a hard post-observation shared limit. | `SessionBudgetExceeded`, mapped to the client protocol error of the same name [E:C1]. | Retries spend shared usage, but per-child reservations/reclaim and persisted usage reconstruction were not found [A:C-HIER]. |
| **Codex — provider retry/idle controls** | Provider request or response stream [E:C2]. | Retry occurs after request/stream failure; idle timer races the stream. | Hard local maxima/timeouts, not aggregate Run/Workflow spend allocation [A:C-HIER]. | Request/stream error after configured attempts; no shared retry-budget exhaustion dimension was found [A:C-HIER]. | Defaults are four request retries, five stream reconnects, and 300 s idle; attempts are not a durable effect ledger [E:C2, A:C-HIER]. |
| **Claude — Query `maxTurns`** | One Query [E:L1]. | Turn count is checked in the Query loop after progress has occurred [E:L1]. | Hard Query-local count, not hierarchical allocation [A:L-HIER]. | Query returns its local maximum-turn error/result path [E:L1]. | Component retries can add work inside a turn; common durable attempt/Resume accounting was not found [A:L-HIER, A:L-RETRY]. |
| **Claude — Query `maxBudgetUsd`** | One Query's accumulated API cost [E:L1]. | `getTotalCost() >= maxBudgetUsd` is checked after yielding each processed message [E:L1]. | Hard **post-consumption** stop; not preflight price reservation [A:L-HIER]. | SDK result `subtype: error_max_budget_usd` includes accumulated cost/usage [E:L1]. | Already incurred cost is retained. Parent/child reservation and durable retry/Resume cost reconciliation were not found [A:L-HIER, A:L-RETRY]. |
| **Claude — feature-gated task/token budget** | Task/agent-local configuration path [E:L1]. | Exact admission versus post-observation behavior is not established in the visible external roots [A:L-HIER]. | Feature-gated; a portable hard/soft classification was not established [A:L-HIER]. | A common provider-neutral exhaustion outcome was not found [A:L-TERMINAL]. | Durable hierarchical retry/Resume accounting was not found [A:L-HIER, A:L-RETRY]. |

This companion table prevents five misleading equivalences: telemetry is not a
budget; post-consumption cost/token stops are not admission control; soft
steering is not a hard ceiling; output truncation is not upstream spend
control; and a shared counter/reminder is not parent-to-child reservation.

### 4.2 Parent-to-child allocation, reservation, reclaim, and Resume

| System | Parent/child accounting semantics | Resume accounting |
|---|---|---|
| **Rollshot** | No child exists; no allocation, reservation, reclaim, or overcommit rule was found [A:R-HIER]. | Tracker is in memory; serialization/reconstruction of budget/attempt/cancellation state was not found [A:R-DURABLE]. |
| **Pi** | Built-in child lifecycle is absent in the investigated boundary; the example gives every subprocess local caps rather than allocating a parent vector [A:P-BUD]. | Conversation Resume retains usage evidence, but durable active-run/child budget reconstruction was not found [A:P-HIER]. |
| **oh-my-pi** | Task concurrency/request/output/runtime controls are configured. Job ownership scopes cancellation, but parent spend is not reserved/reconciled [E:O1, E:O2, A:O-HIER]. | Goal mode positively persists `tokenBudget`, `tokensUsed`, and `timeUsedSeconds`, pauses active work by default on Thread Resume, and resets live accounting baselines. This is one Goal's accounting, not child-tree allocation [E:O1]. Task promises/Job controllers and a shared attempt ledger remain live-only [A:O-HIER]. |
| **Codex** | Default-off `RolloutBudget` is one weighted counter shared by the root Session tree. It emits per-Thread reminders and reports exhaustion, but does not carve out child shares. Agent spawn slot reservation/release prevents Thread-cap races; it is concurrency admission, not token/cost reservation [E:C1, A:C-HIER]. | Persisted reconstruction of rollout budget usage/deliveries or per-child reservations was not found in ThreadStore/rollout roots [A:C-HIER]. |
| **Claude** | Query/agent-local caps exist. A named automatic parent allocation, reservation/reclaim, or overcommit policy was not found [A:L-HIER]. | Transcript/remote identity Resume exists, but a durable unified parent/child budget/attempt record was not found [A:L-HIER]. |

The main design distinction is **shared counter versus reservation**. A shared
counter can eventually stop a tree, but N children can begin expensive calls
before any usage arrives. Reservation denies admission earlier, bounds
aggregate exposure, and makes retry capacity explicit. It also introduces
estimation and stranded-capacity trade-offs.

## 5. Cancellation propagation, cleanup, and confirmation

| System | Provider / Tool | Child / Job / process | Hooks / cleanup / durable intent |
|---|---|---|---|
| **Rollshot** | Driver/Tool checks plus the automation flag are positive. Provider cancellation is boundary-observed: neither initial stream establishment nor an established pending item poll selects cancellation; the latter can wait until item or deadline [A:R-CANCEL]. | Child/Job/process propagation is not present in agent scope [A:R-HIER]. | Generic hooks, cleanup graph, and durable cancel intent were not found [A:R-DURABLE]. |
| **Pi** | One `AbortSignal` reaches provider and Tools; retry, compaction, summary, and user Bash controllers are separate [E:P1]. | Built-in Child/Job propagation not found [A:P-BUD]. The example sends `SIGTERM`, then conditionally `SIGKILL`; runtime cleanup reliability is unverified [G:P-CANCEL]. | Extensions own idempotent `session_shutdown`; a hard crash cannot run it. Durable cancel intent was not found [A:P-HIER]. |
| **oh-my-pi** | Agent abort reaches provider/Tool signals; core deadline can abort a run [E:O1]. | Parent Task abort reaches semaphore wait/child; owner-scoped Job cancel aborts its runner; dispose attempts bounded drain [E:O1, E:O2]. | Controllers/delivery queue are process-local, so cancellation confirmation after process death is not reconstructed [A:O-HIER]. |
| **Codex** | Hierarchical Tokio tokens cancel model/Tool work; Tool runtime aborts or awaits teardown [E:C2]. | Children are interrupted explicitly; legacy tree shutdown is separate. Exec process terminate and TTL cleanup are positive [E:C2]. Parent Turn cancel is not automatic durable tree cancel [A:C-HIER]. | Turn abort flushes rollout/events, but durable cancellation intent for arbitrary children/Jobs was not found [A:C-HIER]. |
| **Claude** | AbortController trees reach provider/tool paths; per-Tool interrupt behavior is `cancel` or `block` [E:L1]. | Sync child can link to parent; async work deliberately unlinks and uses Runtime Task kill. Remote kill archives; teammate shutdown is cooperative then forced [E:L1]. | Pre/Post/PostFailure hooks observe local paths. A generic durable cancellation/cleanup transaction was not found [A:L-HIER]. |

Portable cancellation must distinguish:

```text
requested -> propagated -> confirmed terminal -> cleanup complete
     |             |                |
     +-> unknown <-+----------------+
```

Stopping new admission is immediate. Existing provider calls, Tools, children,
Jobs, processes, hooks, and Artifact publishers each need an owner. Confirmed
cancel may still require cleanup; cleanup failure must not rewrite the original
terminal or publish partial output.

## 6. Retry ownership and idempotency comparison

### 6.1 Layered retry matrix

| Retry layer | Rollshot | Pi | oh-my-pi | Codex | Claude Code source |
|---|---|---|---|---|---|
| **Provider request** | Automatic request retry/backoff not found [A:R-RETRY]. | Coding-agent provider retry defaults to 0; timeout and max server delay are configurable [E:P1]. | Provider/model fallback and retry are local policy; no common attempt ledger [A:O-HIER]. | Request retries default 4, hard-config cap 100 [E:C2]. | Provider errors use local classification/retry paths, but a common durable attempt record was not found [A:L-HIER]. |
| **Stream/protocol** | Stream/protocol error terminates; no reconnect loop found [A:R-RETRY]. | Agent-level transient retry defaults to 3 with 2/4/8 s exponential delay; no jitter is documented in the cited default [E:P1, A:P-RETRY]. | Provider protocol paths have local retry/fallback, including bounded Harmony recovery; ownership remains component-local [E:O1]. | Stream reconnect defaults 5, hard cap 100; idle timeout defaults 300 s [E:C2]. | Compact/context paths have their own bounded recovery; general stream retry limits are not normalized in the external profile [A:L-RETRY]. |
| **Validation/model correction** | Recoverable argument/validation/dry-run feedback returns to model; each correction spends ordinary dimensions [E:R2]. | Validation failures become Tool results; a later model call may correct them [E:P1]. | Structured child schema/yield retry is bounded locally; strict mode can terminally reject [E:O1]. | Tool validation errors are model-visible; no generic automatic effect retry was found [A:C-HIER]. | Tool validation/permission/hook failures have local result paths; no general durable validation-attempt ledger [A:L-HIER]. |
| **Tool effect** | No automatic Tool retry/idempotency key [A:R-RETRY]. | Generic Tool idempotency/attempt policy not found [A:P-HIER]. | Generic Tool effect idempotency ledger not found [A:O-HIER]. | Generic Tool effect idempotency/attempt ledger not found [A:C-HIER]. | Generic Tool effect idempotency/attempt ledger not found [A:L-HIER]. |
| **Child** | Child layer absent [A:R-HIER]. | Built-in child layer absent; example chain stops on failure [A:P-BUD]. | Scoped model fallback/schema retries exist; no parent attempt journal [E:O1, A:O-HIER]. | Child completion/interruption exist; generic child retry policy was not found [A:C-HIER]. | Local/remote child restart/resume paths differ; a universal retry policy was not found [A:L-HIER]. |
| **Job delivery/execution** | Foundation Job layer absent [A:R-HIER]. | Built-in Job layer absent [A:P-BUD]. | Completion delivery uses exponential backoff capped at 30 s with up to 200 ms jitter; retry ownership ends at notification delivery [E:O2]. | Exec reconnect/replay is bounded recovery, not effect restart; generic Job retry policy not found [A:C-HIER]. | Remote polling tolerates transient failures; generic local/remote Job retry policy not found [A:L-HIER]. |
| **Workflow** | Workflow retry owner absent [A:R-DURABLE]. | Durable Workflow retry absent [A:P-HIER]. | Durable Workflow retry/attempt ledger absent [A:O-HIER]. | Durable Workflow retry owner absent [A:C-HIER]. | General durable Workflow retry policy absent [A:L-RETRY]. |

### 6.2 Safe retry rules

| Effect state | Safe action |
|---|---|
| Validation failed before admission | Correct arguments/content and create a new attempt under the remaining validation/model budget. |
| Cancelled before execution | A new attempt may be admitted; retain the cancelled attempt and its zero/known charge. |
| Read-only call failed before result | Retry only if the same immutable input revision is still current and rate/cost budget remains. Apply bounded exponential backoff and provider guidance; add jitter when concurrent clients could synchronize. |
| Mutating Tool acknowledgement lost | Reconcile by logical operation/precondition/effect receipt. Do not repeat merely because no Tool result reached the transcript. |
| Remote/chargeable Job start ambiguous | Reuse the provider idempotency key and query by key/handle. Route to `needs_reconciliation` if the provider cannot answer. |
| Artifact was published but terminal event was lost | Validate the immutable Artifact/receipt and record completion once. Never rerun solely to reproduce the event. |
| Partial Artifact exists | Quarantine/delete by attempt policy. It cannot satisfy completion; retry publishes to a unique attempt path. |
| Retry limit reached | Return the most specific typed terminal plus attempts, last safe evidence, remaining budget, and user actions. Do not collapse it to generic runtime failure. |

Backoff delays consume the retry owner's wall-clock policy, while provider
tokens/cost, Tool calls, Job starts, and bytes consume their ordinary resource
dimensions. A retry count by itself is not a resource budget.

## 7. Normalized failure and terminal model

This is a comparison vocabulary for later synthesis, not a selected schema.

| Normalized class | Meaning / safe next action | Current Rollshot mapping | Reference-system evidence |
|---|---|---|---|
| **Succeeded / ready for review** | Required typed result and Product Artifact/evidence validate; advance to review/next gate. | `ReadyForReview`; visual `Suggested`/`NoSuggestion`. | External systems commonly expose final messages/Task completion, but a generic typed Product Artifact terminal was not found [A:P-TERMINAL, A:O-TERMINAL, A:C-TERMINAL, A:L-TERMINAL]. |
| **Needs user input** | One concrete question/decision can unblock the same logical operation. | `NeedsUserInput`. | Pi-class systems queue input; Codex has user-input requests; Claude permission/questions exist. Common Product terminal equivalence is not established [A:P-TERMINAL, A:O-TERMINAL, A:C-TERMINAL, A:L-TERMINAL]. |
| **Blocked** | Dependency, authority, resource, or external condition is unmet; record blocker and wake condition without pretending to fail. | A distinct `Blocked` Run terminal was not found [A:R-TERMINAL]. | OMP Goal and Codex Goal have blocked/budget-limited concepts; they are Goal states, not universal Run terminals. Pi/Claude common equivalents were not found [A:P-TERMINAL, A:L-TERMINAL]. |
| **Cancelled** | Cancellation was observed for this scope; report confirmation/unknown and cleanup separately. | `Cancelled`. | Pi abort, OMP cancelled Job, Codex `TurnAborted`, Claude killed Task are scope-specific, not one taxonomy [A:P-TERMINAL, A:O-TERMINAL, A:C-TERMINAL, A:L-TERMINAL]. |
| **Exhausted** | Name scope and dimension (`run.input_tokens`, `tree.cost`, `job.wall_time`, `artifact.bytes`, `retry.attempts`). User may raise/reallocate/restart. | `BudgetExhausted { dimension }`; cost caveat remains [E:R1]. | OMP Goal budget-limited and Codex budget-limited abort/reminders exist. Pi finite Run exhaustion and Claude unified dimension taxonomy were not found [A:P-BUD, A:L-TERMINAL]. |
| **Validation failure** | Input/output/schema/policy/Artifact did not validate; include bounded structured diagnostics and stale revision. Retry only corrected retry-safe work. | `SourceValidationFailure`; visual invalid terminal currently maps to `ProtocolFailure`. | OMP `schema_violation` is a child-specific positive; others usually return Tool/protocol errors rather than a universal terminal [A:P-TERMINAL, A:C-TERMINAL, A:L-TERMINAL]. |
| **Provider failure** | Authentication, quota/rate limit, unavailable endpoint, or non-recoverable model error. Include retryability/retry-after without secrets. | `ProviderFailure { message }`; visual redacts to unit variant. | All systems classify provider errors locally; exact categories differ. |
| **Protocol failure** | Malformed stream/Tool pairing/JSON/state-machine invariant; do not automatically repeat an ambiguous effect. | `AgentProtocolFailure { message }`; visual `ProtocolFailure`. | Pi stop/error fields, Codex Turn errors, and Claude tool/query errors are not common Product terminals [A:P-TERMINAL, A:C-TERMINAL, A:L-TERMINAL]. |
| **Runtime/Tool failure** | Host Tool/process/hook/internal execution failed. Preserve whether an effect occurred and cleanup result. | `RuntimeFailure`; hard Tool errors currently become `AgentProtocolFailure` in the main driver. | OMP Job `failed`, Codex model-visible Tool errors, and Claude Runtime Task `failed` are local shapes. |
| **Needs reconciliation / unknown effect** | Start/commit/cancel acknowledgement was lost; query authoritative owner or ask. Never optimistic retry. | Distinct terminal not found [A:R-TERMINAL]. | Common provider-neutral terminal was not found in the core-system scopes [A:P-TERMINAL, A:O-TERMINAL, A:C-TERMINAL, A:L-TERMINAL]. |
| **Lost / expired** | A previously acknowledged Job/remote identity can no longer be observed; distinguish from execution failure. | Job terminal absent [A:R-HIER]. | Codex exec TTL and Claude remote sidecars expose narrower recovery outcomes; a common Job terminal was not found [A:C-TERMINAL, A:L-TERMINAL]. |
| **Incompatible / corrupt** | Resume record, schema, Tool/Skill/provider version, or Artifact failed compatibility/integrity checks; fail closed. | Durable Resume terminal absent [A:R-DURABLE]. | Conversation stores have local corruption/migration behavior; no common Workflow terminal exists [A:P-TERMINAL, A:O-TERMINAL, A:C-TERMINAL, A:L-TERMINAL]. |

`NeedsUserInput` and `Blocked` should remain distinct. The former expects a
question/answer handoff; the latter may wait for a dependency, authority,
provider recovery, Job, or product edit. Likewise, `Cancelled` must not erase
whether an effect is unknown, and `Exhausted` must always name the scope and
dimension.

## 8. State, authority, security, and privacy

- The product owns consent, accepted Artifact truth, review decisions, and
  whether retry is allowed. The model may request actions but cannot allocate
  itself more budget or declare cancellation confirmed.
- The parent/scheduler owns admission and reservation. A child owns only its
  granted vector and cannot spend unreserved parent capacity.
- Provider usage is normalized into Rollshot dimensions; provider metadata
  does not become the canonical Product terminal.
- Current permission, capture consent, filesystem/network/credential authority,
  and input revision are revalidated on every attempt and Resume. A persisted
  budget or idempotency key is not an authority grant.
- Budget/audit records should store identities, numeric usage, hashes,
  sensitivity/retention class, and sanitized failure categories—not raw
  screenshots, prompts, Tool arguments/results, credentials, callback URLs, or
  full source paths by default.
- Cancellation and retries can amplify disclosure: N children and M attempts
  multiply uploaded pixels, transcript copies, and provider retention. Privacy
  bytes/attachments need aggregate accounting, not only per-child limits.
- Error messages are untrusted external content. Bound, sanitize, and classify
  them before persistence or display; preserve stable codes separately from
  prose.

## 9. Candidate Rollshot patterns without final selection

### Pattern A — retain the bounded single-Run envelope

Keep one product-owned 16-dimensional `RunBudget`, one serial Tool context,
one cancellation source, and the current typed terminals. Make the existing
contract explicit: pre-admission limits are hard, provider/result-derived
limits are post-observation hard terminals, and cost is unsupported until a
pricing function exists. Model correction remains a new call within the same
Run; automatic effect retry is absent.

**Non-goals:** no Child Agent hierarchy, Job manager, Workflow scheduler,
dynamic Artifact budget, durable Run Resume, or parallel Tool execution. This
pattern does not pretend that configured cost is enforced.

### Pattern B — hierarchical reservation ledger for bounded children

A parent Run/Task owns a multidimensional available vector. Before spawn it
atomically reserves an explicit child allocation plus one concurrency slot.
The child charges operation/attempt IDs against that reservation. On a known
terminal, the parent reclaims unused capacity and rolls actual charges upward;
on unknown provider/Tool effects it retains the reservation until
reconciliation. Admission never copies the full parent budget into every
child, and aggregate reservations cannot overcommit hard dimensions.

Persist only the Task/attempt/reservation/charge/cancellation facts when Resume
is promised. Reconstruction recomputes `available = limit - committed charges
- live/unknown reservations`, validates monotonic attempt identities, and does
not refund an unknown child. Retries get new attempt IDs and new reservations;
the same provider idempotency key may reconcile one logical external effect.

**Non-goals:** no arbitrary Workflow DAG, remote Job platform, team chat, child
Transcript as completion truth, or automatic parallelism merely because slots
exist.

### Pattern C — separate Run, Job, and Artifact/Workflow envelopes

A Product Workflow owns durable Work Items, checkpoints, cancellation intent,
retry policy, and expected Artifact contracts. Agent Runs receive bounded
token/Tool/context allocations; Job adapters receive provider/process
concurrency, wall-time, cost, log, and scratch reservations; the product owns
Artifact byte/count/retention quotas and acceptance. Job waiting does not spend
Agent Run wall time except for bounded observe calls.

Remote starts persist logical operation/idempotency intent before dispatch.
Resume queries authoritative handles, retains unknown spend, and routes to
running, cancelled, failed, lost/expired, or needs reconciliation. Workflow
retry selects only failed/missing retry-safe nodes, keeps validated siblings,
and publishes each Artifact atomically from a unique attempt path.

**Non-goals:** no universal distributed transaction, exactly-once claim without
provider/product deduplication, requirement that every Work Item use an Agent,
or mandate to ship the deferred media workload.

### 9.1 Round 3 semantics by pattern

| Required semantic | Pattern A — bounded single Run | Pattern B — child reservation ledger | Pattern C — Workflow/Job/Artifact envelopes |
|---|---|---|---|
| **Ownership** | Product supplies input/registry/budget/review; `AgentRunner` owns live accounting and serial execution. | Parent Task/scheduler owns limits/reservations; child owns its allocation and attempt; product owns review Artifact. | Product Workflow owns readiness/checkpoints/retries; Run and Job adapters own live execution; product store owns Artifact acceptance. |
| **Concurrency** | One Tool call at a time; terminal Tool is an exclusive first-success barrier [E:R2]. | Atomic reservation plus declared child cap; queued children hold either no reservation or an explicitly expiring reservation. No aggregate overcommit. | Per-class caps: Agent, provider request, local process/CPU/disk, remote Job/cost, and Artifact storage. Ready nodes run in bounded waves. |
| **Completion** | One typed Run terminal; `ReadyForReview` requires existing validation/dry-run handoff. | Child terminal plus typed result validation; notification/final prose alone is not parent completion. Parent completes only after required child results validate. | Job/provider terminal is insufficient; expected Artifact validates/publishes and durable node terminal commits before successors open [E:J1, E:PERSIST]. |
| **Cancellation** | Shared live token reaches driver/Tool/automation; later serial calls never start. Provider establishment stall, established pending-item stall, cancel/deadline race, terminal, and latency must be measured [A:R-CANCEL]. | Parent stops admission, marks intent, propagates child tokens, awaits confirmed/unknown terminals, reclaims only known unused reservations, and records cleanup. | Persist intent; propagate to Runs/Jobs/processes; query remote confirmation; quarantine partial outputs; cleanup has its own observable result. |
| **Failure** | Existing provider/protocol/validation/runtime/cancel/exhausted terminals; no durable unknown-effect state [A:R-DURABLE, A:R-TERMINAL]. | Child failures preserve scope/dimension/attempt and effect-known flag; parent can retain independent successes. Unknown spend/effect blocks reclaim [E:PERSIST]. | Durable provider/protocol/validation/runtime/blocked/cancelled/exhausted/lost/reconciliation states; downstream nodes remain blocked without discarding independent Artifacts. |
| **Retry** | No automatic effect retry; model correction is a new call consuming current budget [A:R-RETRY]. | Parent policy retries only retry-safe child attempts under a new reservation/attempt; backoff/jitter and max attempts are explicit; ambiguous effects reconcile first. | Workflow owns selective retry; provider idempotency key protects external start; cost/attempt history survives Resume; user checkpoint may be required. |
| **Artifact** | Ordinary results remain bounded in memory; one validated proposal handoff is task-specific. | Each child returns a revision-bound typed proposal/observation; product validates and accepts; partial output never counts. | Expected ID/schema/hash/source revision/provenance/retention are the fan-in contract; unique staging, atomic publish, quarantine/delete partial attempts. |

No pattern is selected. A later synthesis may retain A, adopt B only for a
measured child workload, adopt C only for an actual long-running product, or
defer both.

## 10. Preliminary workload fit

| Pattern | Smart Redaction | Action Guide | Deferred brag + Hyperframes |
|---|---|---|---|
| **A: single Run** | Exact current fit; cost enforcement and both provider-cancel gaps remain explicit [A:R-CANCEL]. | Fits each independent bounded proposal. | Valid inline fallback, but cannot govern long Jobs or dependency recovery [A:R-HIER, A:R-DURABLE]. |
| **B: child reservations** | More machinery than the current trace proves [W1, A:R-HIER]. | Candidate only if measured batches of independent revision-bound proposals justify fan-out. | Useful for worker Runs, but insufficient for Jobs/checkpoints/Artifact recovery by itself [E:J1, E:PERSIST]. |
| **C: separate envelopes** | Unjustified for the current bounded loop [W1, A:R-DURABLE]. | Candidate for product-owned media operations only if restart/remote value is proven. | Strong semantic match if the deferred workload becomes real; highest state and testing cost. |

## 11. Measurable evaluation criteria

| Dimension | Required measurement / pass criterion |
|---|---|
| **Budget dimensions** | Boundary tests for token, cost, wall time, Tool calls/bytes, child, Job, Artifact, retry and parallelism. Every rejection names scope/dimension/current/reserved/limit without sensitive payloads. |
| **Hard/soft truth** | Every configured control is labeled hard, post-observation hard, soft/reminder, or telemetry-only. No field is marketed as enforced when no production charge reaches it. |
| **Reservation conservation** | Property tests over spawn/finish/cancel/retry/crash show `committed + reserved + available = limit` per hard dimension, no negative balance, no double reclaim, and zero aggregate overcommit. |
| **Resume accounting** | Crash before/after reserve, external acknowledgement, charge, terminal and reclaim. Reconstruction produces the same committed usage; unknown work remains reserved until authoritative reconciliation [E:PERSIST]. |
| **Cost/tokens** | Compare inline and concurrency 1/2/4. Record parent/child provider input/output/cache tokens, actual currency, packet/schema duplication, retry charges and stranded reservations. |
| **Wall time** | Report Agent Run wall time separately from Job lifetime and queue delay; p50/p95 cancellation, cleanup, reservation wait and critical-path completion. |
| **Cancellation** | Inject cancel during provider establishment, an established pending item, and the cancel/deadline race, plus scheduler wait, Tool pre/post effect, child, Job/process, hook and Artifact publish. Record terminal and p50/p95 latency; every attempt reaches one confirmed/unknown terminal with no leaked process/resource [A:R-CANCEL, E:S1, E:J1]. |
| **Retry/idempotency** | Duplicate every request/event; inject lost acknowledgements. Zero duplicate document apply, external Job charge, accepted Artifact, completion notification, or budget charge. |
| **Backoff/fairness** | Test bounded exponential backoff with jitter under synchronized failures; retries cannot starve first attempts or exceed provider/global concurrency. Queue and retry wait are observable. |
| **Failure normalization** | Provider, protocol, validation, runtime, blocked, needs-input, cancel, every exhaustion dimension, unknown effect, lost/expired and corrupt Resume each map to one stable code and at least one safe user action [E:J1, E:PERSIST]. |
| **Artifact integrity** | 100% successful terminals reference existing schema/hash/revision-valid, atomically published Artifacts. Partial/stale outputs never unlock successors. |
| **Cleanup** | No leaked provider stream, Tool future, child, pipe, process tree, Job handle, hook task, scratch path, reservation, or cancellation waiter after terminal/retention expiry. |
| **Privacy** | Default budget/audit/failure records contain zero raw screenshot/video bytes, credentials, callback secrets, full Tool arguments/results, or unbounded provider messages. Deletion reaches all declared derivatives within SLA. |

Required bounded spikes before synthesis selects beyond Pattern A:

1. Use fake providers for (a) an initial `stream()` future that never resolves
   and (b) a returned stream whose next item never resolves. For each, inject
   cancellation before, after, and concurrently with deadline expiry; record
   the exact terminal plus p50/p95 cancellation latency. This defines rather
   than assumes establishment, established-item, and cancel/deadline-race
   behavior [A:R-CANCEL].
2. Property-test a vector reservation ledger with concurrent spawn, partial
   usage, cancellation, unknown cost, retry, crash, and reclaim.
3. Run the same revision-bound proposal inline and with child concurrency 1/2/4;
   measure total tokens/cost, latency, privacy bytes and review burden.
4. Use a fake remote Job with query-by-idempotency-key; crash before and after
   acknowledgement/cancel/Artifact collection and prove zero duplicate start or
   optimistic refund.
5. Inject every normalized failure into one Run/child/Job adapter and verify
   stable terminal codes, user actions, cleanup, and Artifact gates.
6. Runtime-test Pi, OMP, Codex, or Claude policies only if a later pattern
   depends on them; pinned static source is insufficient proof of deployed
   cancellation, retry timing, or Resume behavior.

## 12. Non-goals and unresolved questions

This comparison does not:

- enable retries, parallel Tools, Child Agents, Jobs, or durable Workflow code;
- turn provider rate limits, context windows, concurrency caps, or telemetry
  into one fictional budget;
- treat a Transcript, Goal, Todo, Runtime Task, child notification, output path,
  or process exit as Product completion;
- promise exactly-once external effects without authoritative deduplication;
- automatically refund unknown work or carry old permission/consent through
  Resume;
- require every Rollshot product to share one budget vector or retry policy;
- persist sensitive model/Tool/Artifact content merely to improve accounting;
- select a retry count, backoff formula, concurrency cap, store, retention
  period, provider, or candidate pattern; or
- choose a final architecture before Round 6 synthesis.

Open questions for synthesis are:

1. Is fixing current cost accounting and both provider-stream cancellation
   gaps sufficient for Smart Redaction, or does durable handoff value justify
   a Task envelope?
2. Does any Action Guide batch save enough latency to justify parent/child
   reservations after duplicated image/token/privacy cost?
3. Which dimensions are safely reservable estimates, and which must use
   conservative maxima or post-observation overrun policy?
4. Should soft-limit steering exist at all for bounded review Runs, or only for
   optional long child/workflow modes?
5. Which Product owner defines Artifact byte/count/retention quotas and remote
   Job cost authority if the deferred workload becomes active?
6. What user experience distinguishes retryable provider failure, blocked
   dependency, needs input, unknown effect, lost Job, and hard exhaustion?

## 13. Exact negative audits, graph coverage, and runtime gaps

### 13.1 Graph-first evidence

- **[G0] Graph coverage.** `get_minimal_context` for Rollshot returned 7,979
  nodes, 65,744 edges, and 405 files. Semantic/file queries located
  `BudgetTracker`, `RunCancellation`, `RunTerminalState`, budget terminal tests,
  provider-contract cancellation, Tool cancellation, and automation-executor
  cancellation. The same call against each literal reference root
  `learn-projects/{pi,oh-my-pi,codex,claude-code-source-code}` returned 0 nodes,
  0 edges, and 0 files. Reference claims therefore use the bounded shell/source
  audits below.

### 13.2 Rollshot audits

- **[A:R-HIER] Budget hierarchy/retry/parallelism audit.** Literal files:
  `crates/rollshot-agent/src/{domain,driver,model,provider,runtime,tools}.rs`.
  Case-insensitive regex:
  `parent.?budget|child.?budget|job.?budget|artifact.?budget|retry.?budget|parallelism.?budget|budget.{0,24}(reserv|reclaim|overcommit)|(?:reserv|reclaim|overcommit).{0,24}budget|resume.{0,30}(budget|usage)|(?:budget|usage).{0,30}resume|idempotenc|backoff|jitter`.
  Result: **0 hits**. Parent/child/Job/Artifact/retry/parallelism budgets,
  reservation/reclaim/overcommit, Resume accounting, idempotency, backoff and
  jitter were not found in this exact scope. Positive 16-dimensional source is
  [E:R1].
- **[A:R-COST] Production cost-charge audit.** Literal files `runtime.rs` and
  `driver.rs` were searched for `\bcost\b|Cost`; all matches were read. Runtime
  declares/checks/accumulates `cost` and has a synthetic unit test, but its
  field documentation explicitly says no provider/model pricing is wired and
  cost stays zero. No driver-side cost assignment was found. The comparison is
  therefore present but not operationally fed in the current product path.
- **[A:R-CANCEL] Provider-stream cancellation audit.** Literal files:
  `provider.rs` and `driver.rs`; direct control-flow reading of
  `ProviderAdapter::{stream}`, both concrete adapters,
  `stream_to_model_events`, and `AgentRunner::drive_streamed_turn`. Both
  concrete adapters first await `model.stream(completion_request)`, and the
  driver awaits `provider.stream(request, bounds)`, without a cancellation/
  deadline `select!` around those initial futures. Interruptibility before the
  stream is returned was therefore not established. For an established stream,
  source checks cancellation immediately before each item poll, but the
  following `tokio::select!` has branches only for `sleep_until(deadline)` and
  `stream.next()`. It has no cancellation branch. A cancellation request cannot
  by itself wake an indefinitely pending item poll; source only establishes
  observation when the item resolves or the deadline branch wins. No live
  establishment stall, established-item stall, or cancel/deadline race was run,
  so the resulting terminal and latency remain unverified.
- **[A:R-RETRY] Retry/effect audit.** The six [A:R-HIER] files were searched
  for `retry|backoff|jitter|idempotenc|attempt.?id|replay|dedup`. Relevant hits
  were prompt/test prose and cumulative usage deduplication; no facade-owned
  automatic provider/protocol/Tool retry, effect attempt/idempotency ledger,
  or backoff/jitter policy was found. Recoverable Tool/model correction is
  positive source [E:R2]. Dependency-internal HTTP behavior was not treated as
  a Rollshot contract.
- **[A:R-DURABLE] Durable accounting/cancellation audit.** Reused the exact
  persistence roots `crates/rollshot-agent/src/{domain,driver,runtime}.rs` and
  workbench run path with regex
  `serde.{0,40}(BudgetTracker|RunCancellation|UsageSnapshot)|(?:BudgetTracker|RunCancellation).{0,40}(save|persist|resume|recover)|Session(Store|Storage|Repository)|checkpoint|reattach|idempotenc`.
  A durable budget/attempt/cancellation/checkpoint/reattachment record was not
  found in the investigated scope. Current values are live memory [E:R1].
- **[A:R-TERMINAL] Terminal gap audit.** Literal `driver.rs` and
  `visual_annotation.rs`; declarations were read completely and exact terms
  `Blocked|Reconciliation|UnknownEffect|Lost|Expired|Incompatible|Corrupt`
  were searched. No main/visual terminal with those names was found. This does
  not deny that app layers can render messages for narrower errors.

### 13.3 Pi audits

- **[A:P-BUD] Finite budget/child/Job audit.** Exact Reviewed-profile boundary:
  `packages/agent/src`, `packages/agent/test/agent-loop.test.ts`,
  `packages/agent/docs/agent-harness.md`, `packages/coding-agent/src/core`, and
  coding-agent docs `sessions.md`, `session-format.md`, `skills.md`,
  `extensions.md`, `compaction.md`, `security.md`, `settings.md`, excluding
  `export-html/vendor`. Regex:
  `run.?budget|token.?budget|cost.?budget|turn.?budget|tool.?budget|wall.?time.?budget|child.?agent.?budget|job.?budget|artifact.?budget|max.?turns|max.?tool.?calls|budget.?exhaust|budget.?limit`.
  Hits were thinking/context/compaction budgets. A finite product Run or
  child/Job/Artifact governance budget was not found in this scope.
- **[A:P-RETRY] Retry-delay/jitter audit.** Literal files
  `packages/coding-agent/src/core/{agent-session,settings-manager,sdk}.ts` and
  `packages/coding-agent/docs/settings.md`; regex
  `maxRetries|baseDelay|retryDelay|maxRetryDelay|jitter`. The agent-session
  calculation is exactly `baseDelayMs * 2 ** (attempt - 1)` and the settings
  document lists the 2/4/8 s default; no jitter term was found in that cited
  agent-level retry path. Provider defaults remain a distinct SDK policy.
- **[A:P-HIER] Hierarchy/Resume/idempotency audit.** Same boundary; regex:
  `parent.?budget|child.?budget|job.?budget|artifact.?budget|retry.?budget|parallelism.?budget|budget.{0,24}(reserv|reclaim|overcommit)|(?:reserv|reclaim|overcommit).{0,24}budget|resume.{0,30}(budget|usage)|(?:budget|usage).{0,30}resume|durable.{0,20}(attempt|retry)|idempotenc`.
  Only branch-summary/compaction context-token reservation matched. A resource
  hierarchy, Resume charge ledger, or effect idempotency contract was not found
  in this scope.
- **[A:P-TERMINAL] Product terminal audit.** Same boundary; Reviewed-profile
  exact regex
  `terminal.?state|terminal.?status|terminal.?outcome|terminal.?taxonomy|run.?terminal|RunTerminal|stopReason|errorMessage|agent_end|agent_settled`.
  Hits expose provider stop/error and lifecycle/settlement events. A separate
  typed Product Run-terminal taxonomy was not found.
- **[G:P-CANCEL] Runtime gap.** The example subprocess's shared abort handler,
  `SIGTERM`, conditional delayed `SIGKILL`, and temporary-file `finally` path
  were statically inspected. No process-tree/signal-delivery runtime test was
  performed.

### 13.4 oh-my-pi audits

- **[A:O-HIER] Hierarchy/durability audit.** Literal roots:
  `packages/coding-agent/src/{goals,task,async,session}`. Regex:
  `parent.?budget|child.?budget|job.?budget|artifact.?budget|retry.?budget|parallelism.?budget|budget.{0,24}(reserv|reclaim|overcommit)|(?:reserv|reclaim|overcommit).{0,24}budget|resume.{0,30}(budget|usage)|(?:budget|usage).{0,30}resume|durable.{0,20}(attempt|retry)|idempotenc`.
  Hits were local schema/yield/provider retry-budget prose, context-compaction
  reserve, and a worktree patch idempotence check. No unified parent resource
  vector, allocation/reclaim/overcommit, or durable cross-layer attempt ledger
  was found. Positive Goal/Task/Job controls are [E:O1-E:O2].
- **[A:O-ADMISSION] Task/Job admission control-flow audit.** Literal files
  `packages/coding-agent/src/task/{index,parallel}.ts` and
  `packages/coding-agent/src/async/job-manager.ts`; terms
  `#spawnSemaphore|semaphore.acquire|queued|markRunning|maxRunningJobs|Background job limit reached|admission|fair|acquire|queue`
  were searched and the matching declarations/callers read. `TaskTool` owns one
  semaphore per Session and both sync spawns and async Task bodies wait on its
  abortable `acquire()`. Direct manager registration counts only running,
  non-queued Jobs and throws `Background job limit reached` at the cap. A
  caller may register a Job with `queued: true`; the manager excludes it from
  that count, while the caller performs the wait and later invokes
  `markRunning()`. The manager closure for `markRunning()` only clears the flag;
  manager-side queue matches beyond that flag belong to completion delivery,
  not admission. No manager-owned admission queue, fairness policy, or capacity
  reacquire was found. No runtime interleaving test was run.
- **[A:O-TERMINAL] Common terminal audit.** The same roots plus core
  `agent-loop.ts` were inspected for Task/Goal/Job status and failure unions.
  Positive local states include Goal `budget-limited`, Task schema outcomes,
  and Job `completed|failed|cancelled`. A common provider-neutral Product
  Run/Workflow/Artifact terminal union was not found in this scope.

### 13.5 Codex audits

- **[A:C-HIER] Hierarchical budget/Resume audit.** Literal roots:
  `codex-rs/core/src/{rollout_budget.rs,agent,session,config}`,
  `codex-rs/thread-store/src`, and `codex-rs/protocol/src`. Regex:
  `parent.?budget|child.?budget|job.?budget|artifact.?budget|retry.?budget|parallelism.?budget|budget.{0,24}(reserv|reclaim|overcommit)|(?:reserv|reclaim|overcommit).{0,24}budget|resume.{0,30}(budget|usage)|(?:budget|usage).{0,30}resume|durable.{0,20}(attempt|retry)|idempotenc`.
  Result: **0 hits**. Direct reading separately establishes a shared weighted
  `RolloutBudget` counter/reminders and spawn/residency slot reservations. A
  resource-vector allocation/reclaim or persisted tree-budget reconstruction
  was not found.
- **[A:C-TERMINAL] Common terminal/Artifact audit.** Exact Reviewed-profile
  roots `core/src`, `protocol/src`, `app-server/src`, `exec-server/src`, and
  `ext`; declaration/literal terms covered Product `Artifact`, Workflow/Job,
  terminal taxonomy, and retry attempts. Positive Turn abort, Tool errors,
  agent death, background terminals and the narrow image Artifact were found.
  A common Product Workflow/Job/Artifact terminal and effect-attempt ledger were
  not found in the investigated scope.

### 13.6 Claude Code audits

- **[A:L-HIER] Hierarchy/Resume/idempotency audit.** Literal roots/files:
  `src/query.ts`, `src/QueryEngine.ts`, `src/Task.ts`, `src/tasks`,
  `src/tools/AgentTool`, `src/services/compact`, `src/services/tools`,
  `src/utils/sessionStorage.ts`, `src/utils/sessionRestore.ts`, and
  `src/utils/background/remote`. Regex:
  `parent.?budget|child.?budget|job.?budget|artifact.?budget|retry.?budget|parallelism.?budget|budget.{0,24}(reserv|reclaim|overcommit)|(?:reserv|reclaim|overcommit).{0,24}budget|resume.{0,30}(budget|usage)|(?:budget|usage).{0,30}resume|durable.{0,20}(attempt|retry)|idempotenc`.
  The only matches were idempotency-test instructions inside a built-in
  verification-agent prompt. A named hierarchical resource budget,
  reservation/reclaim/overcommit policy, durable attempt ledger, or Resume
  accounting contract was not found in this external-source scope.
- **[A:L-RETRY] Budget/retry audit.** Exact roots:
  `src/query.ts`, `src/QueryEngine.ts`, `src/Task.ts`, `src/tasks`,
  `src/tools/AgentTool`, `src/services/compact`, `src/services/tools`, and
  `src/utils/background/remote`; regex
  `maxTurns|maxBudgetUsd|taskBudget|TOKEN_BUDGET|retry|Retry|timeout|Timeout|consecutiveFailures|AbortController|\bkill\b`.
  Hits establish query limits, AbortController/Task kill, component timeouts,
  compact retries/circuit breaker, remote polling, and hook retry guidance.
  A single durable Workflow retry policy was not found.
- **[A:L-TERMINAL] Common terminal audit.** Exact runtime Task, query, Tool,
  Agent, remote, and session roots above were inspected for status/failure
  declarations. Positive Task `completed|failed|killed`, provider/tool errors,
  permission/hook failures, and query limits were found. A common typed Product
  Run/Workflow/Job/Artifact terminal union, including reconciliation/lost/
  corrupt states, was not found in the investigated external-source scope.

All absence statements are revision- and path-bounded. They do not prove that
an uninspected extension, hidden/internal build, provider SDK, service policy,
or later revision lacks a capability.

## 14. Evidence index and limitations

### Rollshot and workload evidence

- **[E:R1] Source + test source:**
  `crates/rollshot-agent/src/runtime.rs` — `BudgetDimension`, `RunBudget`,
  `UsageSnapshot`, `BudgetTracker`, `RunCancellation`, per-dimension budget and
  cancellation tests; `driver.rs` — terminals, charging/commit paths and
  terminal budget tests. Graph evidence is [G0].
- **[E:R2] Source + test source:**
  `crates/rollshot-agent/src/driver.rs` — provider/Tool loop, terminal mapping,
  correction and visual-annotation runner; `provider.rs` — concrete adapters,
  bounds and error mapping; `tools.rs` — serial registry, cancellation,
  per-Tool limits and automation flag; `model.rs` — provider-neutral error/
  stop vocabulary. No live provider was used.
- **[E:R3] Test source:**
  `crates/rollshot-agent/tests/provider_contract.rs` and focused driver/runtime/
  Tool/visual tests for cancellation, 30-second product budget, terminal
  dimensions, provider/privacy contracts. Tests are source evidence here; fresh
  execution is recorded in the task verification report.
- **[W1]** Round 0 Smart Redaction trace and current agent/workbench source.
- **[W2]** Round 0 Action Guide trace; current proposal/revision paths; adjacent
  `long-running-jobs.md` video-import evidence. Product persistence/process
  behavior is not agent-foundation budget behavior.
- **[W3]** Pinned brag/Hyperframes production, worker, review, preview and remote
  render traces recorded by Round 0 and the adjacent Round 3 comparisons.
- **[E:S1] Adjacent capability:** `subagents-and-parallelism.md` — isolation,
  caps, cancellation, completion and selective-retry candidates.
- **[E:J1] Adjacent capability:** `long-running-jobs.md` — start ambiguity,
  cancellation confirmation, collection, cleanup, reattach and remote receipts.
- **[E:T1] Adjacent capability:** `tools-and-scheduling.md` — Tool exposure,
  side-effect classes, ordering, result limits, terminal barriers and attempts.
- **[E:PERSIST] Adjacent capability:** `persistence-checkpoint-resume.md` —
  durable decisions, unknown effects, idempotency, Resume boundaries and
  Artifact-driven recovery.

### Reference-system evidence

- **[E:P0] Reviewed profile:** `systems/pi.md`; status distinctions, active
  coding-agent path, built-in absences and exact audits remain authoritative.
- **[E:P1] Source/docs:** Pi `packages/agent/src/{agent,agent-loop,types}.ts`,
  coding-agent `core/agent-session.ts`, retry helpers, and `docs/settings.md`.
  Supports AbortSignal and the defaults: agent retry enabled, maximum three,
  2 s exponential base; provider retry zero; provider retry-delay cap 60 s.
- **[E:O0] Reviewed profile:** `systems/oh-my-pi.md`.
- **[E:O1] Source/test source:** OMP core Agent deadline/cancellation;
  `coding-agent/src/goals/{state,runtime}.ts`;
  `src/task/{types,index,executor,parallel}.ts`. Supports Goal accounting/Resume
  behavior, the per-Session Task spawn semaphore, Task soft request steering,
  forced-yield/grace abort, hard runtime timer, and post-capture byte/line
  truncation; tests were not run.
- **[E:O2] Source:** OMP `src/async/job-manager.ts`: default 15 running Jobs,
  direct-registration hard error at capacity, caller-owned `queued` counting
  flag/`markRunning()` behavior, five-minute retention, owner-scoped abort,
  completion-delivery retry base 500 ms, max 30 s, jitter 200 ms, and
  process-local state.
- **[E:C0] Reviewed profile:** `systems/codex.md`; feature status remains
  authoritative.
- **[E:C1] Source/test source:** Codex `core/src/rollout_budget.rs`,
  `core/src/session/rollout_budget.rs`, protocol error declarations,
  `agent/control.rs`, agent registry/residency, `features/src/lib.rs`, and config
  tests. Supports default-off token/rollout features, tree-shared weighted usage
  reminders, hard post-recording `SessionBudgetExceeded`, and separate
  spawn-slot reservation/release.
- **[E:C2] Source/test source:** Codex model-provider-info defaults (four
  request retries, five stream reconnects, 300-second idle timeout), core
  cancellation/Tool/agent paths, and exec-server recovery cited by the Reviewed
  profile. Tests were inspected, not executed.
- **[E:L0] Reviewed external-source profile:** `systems/claude-code.md`; build/
  feature gates and hidden modules remain limitations.
- **[E:L1] Source:** Claude `QueryEngine.ts`, `query.ts`, Runtime Task/Agent and
  Tool executor roots. Supports query-local limits, the post-message
  `maxBudgetUsd` accumulated-cost check and `error_max_budget_usd` result,
  AbortController trees, linked/unlinked child behavior, Task kill and
  hook/tool cancellation.
- **[E:L2] Source:** Claude compact paths: bounded prompt-too-long recovery and
  `MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES = 3`; remote polling/tool paths are
  component-local. No provider/service runtime was exercised.

### Limitations

Confidence is **high** for visible pinned types, constants, status labels,
charging/terminal control flow, and exact bounded audits; **medium** for source
plus tests that were inspected but not run; and **low-to-medium** for actual
provider retry behavior, pricing, cancellation during stream establishment or
an established pending item, cancel/deadline races and their terminal/latency,
process trees, remote services, server-controlled feature gates, crash/Resume,
and cleanup races because they were not exercised.

The reference graphs had zero coverage, and several systems delegate behavior
to provider SDKs or hidden/service-gated modules. Static source cannot prove
timely signal delivery, absence of duplicate external effects, provider cost
accuracy, fair admission under contention, or crash-safe accounting. Every
negative statement means only “not found in the named investigated scope.”
