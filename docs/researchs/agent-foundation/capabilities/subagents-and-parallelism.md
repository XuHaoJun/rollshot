# Subagents and parallelism comparison

**Research date:** 2026-07-22 (Asia/Taipei)
**Status:** Reviewed
**Umbrella revision:** 1
**Current Rollshot revision:** `3211433e2ba3d0153160d993573c6011f8176502`
**Reference revisions:** Pi `dd6bea41efa8caa7a10fe5a6401676dc5699f83f`;
oh-my-pi `7b141199d524b859c357fc89654f10b62b9f3df1`; Codex
`4a443994bd12f49f2f08b21a2f224d9d42b9e734`; Claude Code source
`2ca5ddabfed5f220812ea11f029eda03b21bc4c1`; Hyperframes
`807078c7cde9d5c8403588722d1cd9397c513a0d`.
**Evidence mode:** static source and test-source inspection. No child agent,
provider, worktree, remote session, cancellation race, process restart, or
Hyperframes build was executed.

This document compares child execution and parallel coordination. It does
**not** select a final Rollshot architecture.

## 1. Rollshot problem and workload traces

Rollshot currently has one bounded, serial Agent Run. One `AgentRunner`
invocation owns a fresh Rig run, finite 16-dimensional budget, cancellation,
one run-local `ToolContext`, and a typed terminal. A returned tool batch is
executed in model order; the first successful terminal tool stops the rest.
A child-agent registry, scoped child context, child scheduler, concurrency cap,
or artifact-based child completion was **not found in the investigated scope**
[A:R]. This is a valid baseline, not a deficient parallel runtime. [E:R1]

The three workload traces create different pressure:

| Workload | Current or referenced trace | What the trace does and does not establish |
|---|---|---|
| **Smart Redaction** | One provider/tool loop progresses source generation → validation → dry run → typed proposal. The app owns consent, budget, cancellation and review. [W1] | It establishes bounded specialist work and deterministic validation. It does **not** establish a need for a child, fan-out, or DAG. Inline serial execution avoids context duplication and keeps one authority/budget owner. |
| **Action Guide** | Durable project revisions surround independent caption and visual-annotation proposal calls. Visual annotation binds a `run_id`, reviewed step, `document_state_id`, image, fresh cancellation and finite turn configuration. [W2] | Future per-step fan-out is plausible only when several independent suggestions are product-approved. Current code proves revision-bound bounded proposals and stale-result rejection, not a parallel requirement. |
| **Deferred brag + Hyperframes** | Plan/check, scene build, assembly, render, poster and share-copy stages have explicit prerequisites. Optional scene workers consume packets; audio or generation may overlap independent work. [W3-W5] | If adopted, this trace establishes bounded fan-out/fan-in, Artifact validation and checkpoints. H1 supplies missing-Artifact re-dispatch; broader selective retry would be a Rollshot policy. It does not require Rollshot to ship video, use a general Workflow engine, or keep one long coordinator context. |

## 2. Terms and non-equivalent execution forms

### 2.1 Child-run isolation

A **child run** is isolated only when the parent can name and verify all of
these boundaries independently:

1. **Identity and lifecycle:** stable child/attempt identity, parent relation,
   status, terminal and cleanup owner.
2. **Context:** an explicit packet, fresh prompt, full/partial history fork, or
   restored child transcript—not an assumption that parent conversation,
   Memory, invoked Skills or queued input is visible.
3. **State:** separate mutable model/session state. Shared root progress or a
   Task registry is an explicit channel, not accidental AppState mutation.
4. **Scope:** authorized inputs, artifact/document revisions, filesystem root
   or worktree, output namespace and allowed coordination channels.
5. **Capability and authority:** separately resolved Tools, Skills/resources,
   provider/model, credentials, permission profile and approval behavior.
   Availability is not authority.
6. **Governance:** child budget/limits, cancellation propagation, concurrency
   admission and retry owner.
7. **Completion:** a typed result or validated expected Artifact, not merely a
   process exit, final prose, notification, or path string.

Process separation alone is therefore insufficient. A subprocess sharing the
same working directory and ambient credentials has a fresh context but not
filesystem or authority isolation. Conversely, an in-process child can isolate
its mutable context while intentionally sharing a root Task registry.

### 2.2 Execution vocabulary

| Form | Meaning in this comparison | Non-equivalence rule |
|---|---|---|
| **Spawn** | Create a fresh child Session/loop from an explicit task plus reconstructed configuration. | Fresh context is not automatically a fresh filesystem, credential set, provider, or permission grant. |
| **Fork** | Spawn with selected parent history/system/tool state copied or projected into the child. | A fork is a context/cache choice, not durable Workflow branching or snapshot isolation. |
| **Teammate** | Addressable, longer-lived peer with its own loop, mailbox and idle/active lifecycle. | A teammate is not one-shot fan-out, and a work-ledger Task is not the teammate itself. |
| **Remote agent** | Child work hosted by an authoritative remote service and observed through a remote identity/status API. | It is not merely remote process execution and cannot be inferred from a local sidecar alone. |
| **Worker** | Bounded executor whose dispatch packet and expected Artifact contract are sufficient to perform and validate one or more units. | “Worker” does not imply an LLM, a conversation, or a completion notification. |
| **Inline** | The coordinator executes the same unit in its own current context, usually serially. | Inline is not a fallback failure; it can be cheaper, more cache-efficient and easier to govern. |

Pi's repository `subagent` is example-only; oh-my-pi's built-in unit is Task
plus optional Async Job; Codex V1 and V2 are materially different systems; and
Claude local agent, fork, teammate and remote agent have different gates. They
must not be flattened into one “supports subagents” cell.

## 3. Hyperframes artifact-worker contract

Hyperframes supplies the clearest artifact-worker requirements, but only for
the deferred workload. [E:H1, E:H2]

### 3.1 Two source layers, precedence and dispatch requirements

Hyperframes has two related but non-identical instruction layers:

- **H1 core generic contract:** one scene per dispatch. The child's prompt is
  the full role plus verbatim dispatch context; files and prompt are its entire
  world. Submit all scenes when the harness queues internally. With a hard cap,
  dispatch cap-sized waves until every scene has been attempted. The cap never
  changes scope and must not cause scenes to be dropped or merged. [E:H1]
- **H2 `general-video` specialization:** dispatch only past its measured
  threshold, give each worker **two to three scene packets**, and start **all
  workers in one wave**. Each scene packet inlines its storyboard block,
  blueprint and cited rules; workers read only their packets and design truth.
  Expected outputs are each scene's HTML and motion sidecar. [E:H2]

H2 is the more specific rule for `general-video` worker granularity, so its
two-to-three-scene grouping supersedes H1's generic one-scene grouping in that
workflow. H2's single-wave instruction and H1's mandatory hard-cap waves
conflict when the planned worker count exceeds a harness hard cap; neither
source defines a precedence rule for that case. That is an execution-planning
gap, not permission to merge more scenes, omit work, or claim a single wave.

Both layers require workers to be independent except for the project
filesystem. Shared-file mutation, implicit ordering and hidden predecessor
reads invalidate parallel fan-out. Native delegation is optional; the fallback
ladder is headless CLI workers, then serial inline execution from the same
packet. [E:H1, E:H2]

### 3.2 Completion, missing-Artifact re-dispatch and retry gaps

H1 defines `WAIT` by the expected Artifact existing on disk, never merely by
the harness completion notification. If the one expected scene Artifact is
missing, it re-dispatches one fresh child once with the same prompt plus the
gate failure. H1 says nothing about retrying an Artifact that exists but fails
content/schema validation. The stronger statement that a notification may be
lost, duplicated or early is a general distributed failure-model inference,
not a quoted Hyperframes guarantee. [E:H1]

H2 waits for every grouped scene's HTML and motion sidecar and later runs its
validation gates, but it does not define retry granularity when one worker's
two-to-three scenes are partially published or one published sibling is
invalid. In particular, the sources do **not** guarantee that successful
siblings can be retained while only one missing/invalid scene is retried. A
Rollshot design may adopt that as an explicit idempotent candidate policy, but
must not attribute it to the current Hyperframes contract. [E:H2, A:H]

This yields a two-channel model:

```text
transient channel: child started / progress / notification / exit
                         |
                         v
H1 WAIT gate: expected artifact exists on disk
                         |
              exists ------+------ missing
                 |                    |
                 v                    v
       workflow validation     same prompt once (H1)
                 |
        valid ---+--- invalid
          |               |
          v               v
       fan-in       source retry rule absent
```

The contract does not specify hierarchical budgets, durable cancellation or
fair scheduling. Those remain foundation questions rather than inferred
Hyperframes behavior. Grouped-worker partial retry, invalid-Artifact retry and
hard-cap/single-wave reconciliation are also source-bound gaps [A:H].

## 4. Per-system behavior

### 4.1 Pi: extension-example subprocesses, not a built-in lifecycle

Pi's core/coding-agent built-in boundary contains no child-agent lifecycle in
the investigated scope; the extension guide only points to an uninstalled
example [A:P0]. The example registers a `subagent` tool with single, parallel
and sequential-chain modes. Every child is a fresh `pi --mode json -p
--no-session` subprocess given an agent Markdown system prompt, task, optional
model/tool list and cwd. It does not copy parent messages. [E:P1]

Parallel mode accepts at most eight items and runs four subprocesses at once
through a source-order worker pool. It returns results in input order, caps
model-visible output to 50 KB per item, and retains detailed results in tool
details. One shared abort signal calls `proc.kill("SIGTERM")` for each active
subprocess, then after five seconds conditionally calls
`proc.kill("SIGKILL")` only when `!proc.killed`. Runtime signal delivery and
process-cleanup reliability were not exercised. Chain mode stops at the first
failed child and injects the preceding final text through `{previous}`. [E:P1]

This is useful subprocess evidence, but its model/tools/caps/cancellation are
extension-local. The one tool-call `AbortSignal` is passed to every active
`runSingleAgent`; each subprocess closure reacts to that shared signal. The
result array has agent labels and task indices, but the extension exposes no
child process ID/controller or cancel-by-child address, so addressed
single-child cancellation was **not found in the exact cancellation audit**
[A:P2]. A finite token/cost/wall-time budget, Skill/provider inheritance,
permission profile, spawn fairness/backpressure, expected Artifact contract
and selective retry were **not found in the example scope** [A:P1].

### 4.2 oh-my-pi: Task fan-out and process-local Jobs

oh-my-pi's built-in Task creates a separate child `AgentSession` from a
discovered agent definition. The effective policy selects the child model,
tools, Skills, spawn policy, output schema and optional isolation. Batch items
receive shared context plus per-item assignments; they have no predecessor
edges. Optional worktree isolation returns patch/worktree metadata, while
ordinary children share the parent filesystem and artifact ID space. [E:O1]

The per-`TaskTool`/session semaphore defaults to 32, uses a FIFO waiter array,
and is resized before acquire/release; increasing admits queued waiters,
lowering lets holders drain. `0` means unbounded. When async is wired,
non-blocking agents become process-local `AsyncJobManager` Jobs; blocking agents
and hosts without a manager wait inline. The Job manager defaults to 15 running
Jobs and five-minute retention. Caller-gated queued Jobs consume no running
slot until `markRunning`; direct registration at capacity fails. This is local
backpressure. FIFO is established only for one `TaskTool`/session semaphore;
the Job `queued` flag is not a manager-owned admission queue, and the exact
roots do not establish shared cross-session or durable fairness [E:O1, E:O2,
A:OQ].

Task prefers explicit `yield`: after the initial run it sends at most three
reminders and can force a final named-tool choice. A clean no-yield child may
still succeed through schema-valid JSON or nonempty raw output; strict schema
mode makes invalid output `schema_violation`. Notification delivery retries in
memory. None of these requires an expected product Artifact [E:O3, A:O].

Parent abort reaches semaphore waiting and child execution. Task also limits
recursion (default 2), runtime (default unlimited), output bytes/lines and a
soft request budget/forced-yield grace. These are configured child limits, not
allocation and reconciliation from one parent multidimensional budget; a
hierarchical budget was **not found in the investigated scope** [A:O]. The Job
map, controllers and delivery queue are process-local; serialization,
rehydration or reattachment was **not found in the Job-manager scope** [A:O].

### 4.3 Codex: V1 child Threads versus V2 collaboration paths

Codex `multi_agent` V1 is Stable/default-on; `multi_agent_v2` is
Stable/default-off at the pinned revision. A recorded/resumed session can keep
its chosen version. [E:C1]

V1 `spawn_agent` creates a child Thread/Session: `fork_context=false` is fresh,
while `true` uses a full filtered fork and rejects an agent-type override. V1
and V2 both build the child from the live turn's effective config:
provider/model, reasoning, approval policy, permission profile, cwd,
environment snapshot and conditional exec policy. A requested model or role
layer is then resolved at spawn time; role config can also change the child's
available Skill configuration. These are config/resolution semantics, not a
copy of the parent's already-built Tool registry or invoked-Skill state
[E:C2, A:C4]. The default V1 cap is six spawned threads across the Session's
shared registry and maximum depth is one. Admission is atomic and returns
`AgentLimitReached`; it is not a queued spawn scheduler. [E:C2]

V2 is path/mailbox based: spawn, send/follow-up, interrupt, list and wait.
`fork_turns=none` starts without parent history or selected capability-root
extension state. `all` (the default) and positive last-N both use the fork
path: selected capability roots are copied explicitly from parent SessionMeta
before truncation, while rollout filtering keeps suitable
system/developer/user/final-assistant context and drops prior Tool calls and
outputs. Full history alone preserves the reference-context/cache item and
parent agent type; last-N rebuilds context and may apply a new role. Thus
available Skills from role/config, selected capability roots, historical Tool
invocations, and durable invoked-Skill state are four different things. The
focused roots do not establish a durable invoked-Skill ledger/version snapshot
[E:C2, A:C4]. Spawn edges persist, and idle children may be cold-loaded or
LRU-unloaded for residency.

Each child builds a fresh per-turn Tool router/registry from its own
`TurnContext`, config and shared runtime services. Both fresh and forked thread
constructors pass an empty `dynamic_tools` vector, so parent dynamic Tool specs
are not propagated by these spawn paths. Core/configured Tool availability may
be re-resolved in the child; it must not be described as inheritance of the
parent's current model-visible Tool set [A:C4].

V2 defaults to four concurrent Threads per Session **including the root**, so
effective child capacity is three. It first unloads an eligible idle resident;
if no slot can be freed, spawn fails. Mailbox messages have a queue, but a
spawn-admission queue, fairness or backpressure contract was **not found in the
investigated V2 roots** [A:C1]. The configured V1 depth is explicitly ignored
by V2, and a V2 enforcement call was **not found in the investigated spawn
roots** [A:C2].

Completion queues a parent notification/status; it does not validate a typed
Artifact [A:C3]. A child is interrupted explicitly. Legacy control separately
supports recursive live-tree shutdown, so interrupting a parent Turn must not
be described as automatic durable cancellation of all descendants. Rollout
and optional tree-budget features are default-off and do not establish a
mandatory hierarchical child budget. [E:C2]

### 4.4 Claude Code: local agent, gated fork, gated teammate and gated remote

Claude's ordinary local subagent is implemented/default. A child gets a
separate message loop, sidechain transcript, agent definition, resolved model,
tool pool, optional preloaded Skills and additive MCP servers. Regular children
start with only prompt messages unless an explicit fork context is passed.
Mutable `ToolUseContext` state is cloned/fresh or no-op by default while root
Task registration remains shared for visibility. Sync agents normally share
the parent abort controller; async agents use an unlinked controller so they
can outlive the spawning turn and are stopped through their Runtime Task.
[E:L1]

Regular async children avoid UI permission prompts. Agent permission mode and
allowed-tool rules are reconstructed/scoped; SDK CLI allow rules remain in
force. A worktree is optional. Child model aliases can override the agent
definition, but the visible model/provider stack remains Claude-specific.
[E:L1]

The separate fork path is compiled behind `FORK_SUBAGENT`, interactive-only
and mutually exclusive with coordinator mode. It passes the parent's exact
rendered system prompt, full fork prefix, exact Tool pool, model and thinking
configuration to maximize prompt-cache reuse; permission mode `bubble` routes
questions to the parent. All fork children are async and recursive fork is
rejected. This gated fork must not be described as the ordinary local-agent
default. [E:L2]

Teammates are longer-lived peers with their own loops, permission modes,
controllers, full disk transcripts, file-backed mailboxes and a capped
50-message UI mirror. They can idle, receive leader messages, claim unblocked
work-ledger items, compact independently and shut down cooperatively before
forced kill. External availability requires
`CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` or `--agent-teams` **and** the
GrowthBook kill switch; ant users bypass those gates. The roster is flat:
nested teammate creation is rejected, though a teammate may spawn synchronous
subagents. [E:L3]

Remote-agent Task source persists a local identity sidecar, polls authoritative
Claude.ai status and can restore polling on resume; kill archives the remote
session. However, the external build statically excludes AgentTool's ant-only
remote launch branch, so source semantics do not establish external
availability. [E:L4]

A visible global/per-team agent concurrency cap, spawn queue/fairness policy,
hierarchical parent/child budget, or expected Artifact completion contract was
**not found in the investigated external-source roots** [A:L]. Tool-batch
concurrency has a separate default cap of ten and must not be reused as an
agent cap.

## 5. Cross-system isolation, context and authority matrix

| Design | Context / mutable state | Filesystem and outputs | Tools, Skills, provider/model | Permissions and authority |
|---|---|---|---|---|
| **Rollshot inline** | One fresh Rig run; one run-local ToolContext. | App/product owns screenshot, proposal and Action Guide artifacts. | Registered Rollshot tools; Rollshot provider facade/model config. | App consent + explicit Tool registry, finite budget and one cancellation owner. |
| **Pi example spawn** | Fresh no-session subprocess, only task + agent system prompt. | Selected cwd is shared; result is JSON stream/final text. | Frontmatter can select Tool list and model. Skill/provider transfer was **not found** [A:P1]. | Project-agent confirmation is optional UI policy; a child permission/sandbox profile was **not found** [A:P1]. |
| **OMP Task** | Fresh child AgentSession with shared context + assignment; optional transcript revival. | Shared workspace/artifact IDs or optional worktree/patch. | Agent definition/scoped policy selects model, Tools, Skills and fallback. | Parent Task is exec-tier approval boundary; headless child/per-tool policy applies. Isolation is workspace isolation, not credential/network sandboxing. |
| **Codex V1/V2** | V1 is fresh or full filtered fork; V2 `none` is fresh and `all`/last-N are filtered forks with mailbox/path. | Same selected environment/workspace unless environment policy differs; files are ambient coordination. | Live config seeds provider/model and configured Tool/Skill availability; role/model layers resolve per spawn. Child Tool routers rebuild per turn; spawn paths pass no parent dynamic Tool specs. Full/last-N forks explicitly copy selected capability roots, but filtered history drops Tool calls; durable invoked-Skill state is a documented gap [A:C4]. | Approval policy, permission profile, environment and exec policy are inherited as snapshots and then enforced in a separate child Thread. |
| **Claude local/fork** | Regular prompt-only child or explicit context; fork keeps byte-exact parent prefix. Mutable state isolated; root Runtime Task registry shared. | Shared cwd, optional worktree; sidechain transcript/output path. | Regular agent resolves tools/Skills/MCP/model; fork uses exact tools/model/thinking. Provider remains Claude-specific. | Async normally avoids prompts; scoped rules/agent mode apply. Fork uses parent-bubbling permission mode. |
| **Claude teammate** | Independent persistent loop and mailbox; capped UI mirror. | Shared/team-selected workspace plus transcript/mailbox files. | Own scoped Tool/model context; configured agent Skills preload through the shared `runAgent` path; can spawn only allowed synchronous children [E:L1, E:L3]. | Independent permission mode; permission can be mediated through leader/mailbox. External feature gates apply. |
| **Claude remote** | Remote service owns live context; local identity/status sidecar. | Remote environment/session URL and logs. | Claude remote stack; general provider choice was **not found** [A:L]. | Remote eligibility/account/build gates; kill archives remote session. External launch is unavailable at the pinned external build [E:L4]. |
| **Hyperframes worker** | Complete role + packet; no inherited conversation/Memory/Skills. H1 dispatches one scene; H2 groups two to three for `general-video`. | Shared project but disjoint expected scene paths. | Harness-selected worker; packet inlines required recipes. Provider/model/permission policy is unspecified [A:H]. | Harness grants must be explicit; Artifact completion is not a sandbox. |

## 6. Scheduling, cancellation and completion matrix

Every negative or unknown cell cites an exact audit in Section 13.

| Design | Admission, queue, fairness and backpressure | Cancellation | Completion and retry |
|---|---|---|---|
| **Rollshot** | One Run and serial tool batch; child admission does not exist [A:R]. | One cancellation source reaches provider and automation. | Typed Run terminal/proposal; no child completion [A:R]. |
| **Pi example** | Max 8 per call, 4 active; source-order local pool. Cross-call/global fairness or backpressure was **not found** [A:P1]. | One tool-call signal calls `proc.kill("SIGTERM")` for every active subprocess, then after five seconds conditionally calls `proc.kill("SIGKILL")` only when `!proc.killed`; runtime termination reliability is unverified. No exposed child ID/controller or addressed single-child cancel was found [A:P2]. | Process/assistant result; chain stops on failure. Expected Artifact validation/selective retry was **not found** [A:P1]. |
| **OMP Task + Job** | FIFO only inside one per-session semaphore; default 32 and dynamically resized. Job active cap defaults to 15; parked items are excluded and direct over-cap registration errors. Durable/cross-session fairness is a source-bound gap [A:OQ]. | Abortable semaphore waits, child run and owner-scoped Job cancellation. Process death loses controllers [A:O]. | Yield/schema/raw fallback; delivery retry is in memory. Expected Artifact gate and durable attempt ledger were **not found** [A:O]. |
| **Codex V1** | Shared registry cap 6, depth 1; hard error, no spawn queue [E:C2, A:C1]. | Explicit interrupt; separate legacy tree shutdown. | Completion watcher notification/status. Typed Artifact validation/retry was **not found** [A:C3]. |
| **Codex V2** | 4 total including root; LRU idle unload then hard error. Mailbox queue is not admission queue; fairness unknown [A:C1]. | Explicit per-path interrupt; persistent topology is not cancellation intent. | Parent mailbox/status; no expected Artifact contract [A:C3]. |
| **Claude local/fork** | Global child cap/queue/fairness was **not found** [A:L]. Fork shares cache prefix; regular child is colder. | Sync linked to parent; async Task-owned and intentionally unlinked from spawning turn. | Runtime notification/final text; no generic expected Artifact/selective retry [A:L]. |
| **Claude teammate** | Ready ledger items may be claimed, but visible global/per-team concurrency/fairness cap was **not found** [A:L]. | Cooperative shutdown, then forced kill; independent current-work controller. | Mailbox/task status. Ledger completion does not validate a product Artifact [A:L]. |
| **Claude remote** | Remote service scheduling/cap is unknown in the external tree [A:L]. | Kill archives remote session. | Authoritative remote status can restore polling; generic Artifact completion was **not found** [A:L]. |
| **Hyperframes H1/H2** | H1 submits all to an internal queue or uses hard-cap waves, one scene/dispatch. H2 groups two-to-three scenes and asks for one worker wave; hard-cap reconciliation is unspecified [E:H1, E:H2, A:H]. | A portable cancellation contract is unspecified [A:H]. | H1 re-dispatches a missing one-scene Artifact once. H2 validates grouped outputs, but partial-sibling and invalid-Artifact retry semantics are unspecified [E:H1, E:H2, A:H]. |

## 7. Fan-out, fan-in and dependency readiness

Parallelism is safe only after readiness is decided outside the child. The
following is a candidate Rollshot coordination policy, deliberately stronger
than the portable Hyperframes source contract:

1. **Project readiness:** freeze the relevant input/artifact/document revision,
   required approvals and predecessor set.
2. **Fan-out readiness:** select only mutually independent items. A shared
   context packet is immutable; each item has a unique output namespace and
   its own attempt ID.
3. **Admission:** apply one declared cap. Queue or wave all items without
   changing scope. Record queue delay separately from execution time.
4. **Execution:** children receive the minimum sufficient context, capability
   and authority snapshot. Progress is transient and may be dropped.
5. **Fan-in:** validate every expected typed output/Artifact against the frozen
   input revision. Source-order presentation does not imply source-order
   completion.
6. **Selective retry:** retry only missing/invalid retry-safe items, with the
   concrete validation failure and a new attempt ID. Keep successful siblings.
   Stale input revision, cancellation or non-idempotent effect routes to typed
   stop/reconciliation rather than retry.
7. **Successor readiness:** open assembly/apply/review only after all required
   artifacts validate and required checkpoint decisions remain current.

oh-my-pi Task demonstrates capacity fan-out without predecessor edges. Claude's
work ledger demonstrates dependency-aware claiming without automatic Runtime
Task launch. Hyperframes demonstrates Artifact-existence-gated fan-in. Its
source-backed retry guarantee stops at H1's missing one-scene Artifact; the
broader missing/invalid selective-retry rule above is Rollshot design guidance.
None alone is a complete durable Workflow scheduler.

## 8. When sequential inline execution is preferable

Inline is preferable when one or more of these are true:

- the workload is short enough that packet authoring and fresh-context warm-up
  exceed saved critical-path time;
- the next step depends on the immediately preceding judgment or tool result;
- several steps mutate the same document/files and isolation/merge would cost
  more than serialization;
- one provider prefix/cache is materially cheaper than duplicated child
  prompts;
- authority, consent or a review checkpoint should have one obvious owner;
- deterministic host validation is cheaper than explaining the validation
  contract to a child;
- failure probability or output variance makes fan-in/retry coordination more
  expensive than the work; or
- the host cannot enforce child budgets, cancellation or artifact validation.

Hyperframes provides measured workload evidence: up to roughly six short
scenes is faster inline in the current context; five short scenes measured
about 9 minutes inline versus about 21 minutes packetized. It recommends
fan-out only beyond that scale or for individually heavy scenes, then two to
three scenes per worker in one wave. These numbers are Hyperframes-specific,
not a universal Rollshot threshold. [E:H2]

Smart Redaction currently fits the inline case. Action Guide may still run
independent bounded proposals serially until real batch demand appears. A
future Hyperframes-like workload can switch execution policy without changing
its complete packet or expected-Artifact contract.

## 9. Candidate Rollshot child-run patterns

These are comparison candidates, not selections.

### Pattern A — revision-bound proposal fan-out

For an explicitly selected set of Action Guide steps, freeze the project and
document revision, then spawn at most N independent visual/caption specialist
runs. Each child gets only its step/keyframe and authorized image/reference,
scoped provider/Tool/Skill policy, child budget and cancellation. It returns a
typed proposal tied to the frozen revision; the app validates and presents all
proposals, and applies none automatically.

**Potential benefit:** lower batch latency while preserving current typed
review/stale-result behavior. **Risks:** repeated image/prompt tokens, provider
rate-limit bursts, privacy exposure multiplied by N, late proposals after edits,
UI review overload and no proven current product demand. Shared document edits
are excluded; stale children terminate rather than retry.

### Pattern B — artifact-inspection specialists, parent synthesis

A parent bounded Run snapshots one authorized artifact revision and fans out
read-only specialists such as OCR/layout/candidate-evidence inspection. Each
child returns a small typed observation with provenance; the parent or a
deterministic host combines them and retains existing validation/dry-run gates.

**Potential benefit:** isolate expensive context and overlap independent vision
calls. **Risks:** Smart Redaction currently does not prove that the overhead is
worthwhile; correlated model errors can look like consensus; a parent synthesis
turn adds tokens and latency. If the input is small or one capability dominates,
the same specialists should run inline.

### Pattern C — Hyperframes-style artifact workers

A product-owned coordinator writes immutable, versioned packets for ready
scene/work items. Workers receive disjoint packets and output paths. As an
explicit Rollshot candidate policy—not a claim about H1/H2—the coordinator
validates expected Artifacts, retains valid siblings and selectively retries
one missing/invalid retry-safe item once, then unlocks assembly. A coordinator
restart rebuilds readiness from durable packets, checkpoint decisions and
Artifacts rather than child transcripts.

**Potential benefit:** strongest match to deferred multi-stage creative work;
provider-neutral artifact recovery and partial success retention. **Risks:**
packet/build/validation code, filesystem publication and provenance, durable
Workflow/Job policy, worktree or disjoint-write enforcement, and significant
coordination overhead. This pattern is unjustified unless the deferred product
becomes real and exceeds inline economics.

## 10. Measurable execution economics

A child design must beat inline on measured end-to-end value, not agent count:

| Dimension | Required measurements | Failure signal |
|---|---|---|
| **Tokens/cost** | Parent input/output, packet bytes, duplicated system/Skill/tool schema tokens, child input/output, fan-in/synthesis, retry tokens and actual currency by provider. | Parallel total cost exceeds inline without a corresponding latency/quality gain. |
| **Time** | Packet preparation, spawn/warm-up, queue delay, execution, validation, retry, merge/fan-in and user review; p50/p95 critical path and total compute-seconds. | Queue/warm-up/coordination erases parallel critical-path savings. |
| **Cache** | Cache-read/write/uncached tokens for inline, fresh spawn, partial fork and full fork; warm/cold trials and prefix invalidation after Tool/Skill differences. | Fork copies so much history that cache savings do not offset duplicated context, or regular children miss all useful cache. |
| **Failure** | Spawn/provider/tool/permission/cancel/validation/artifact/stale-input rates; retry success and duplicate-side-effect count. | More than one retry, optimistic notification completion, duplicate effects, or failed cleanup. |
| **Coordination** | Packets authored, parent synthesis tokens, queue depth/wait, idle-slot time, output conflicts, merge failures, overwritten artifacts, duplicate work and review items per user decision. | Coordination failures or review burden rise faster than completed useful items. |

Run every candidate against the same trace at concurrency 1, 2, 4 and the
relevant provider cap. Compare serial inline, serial packet replay and bounded
fan-out. Dispatch pays only when measured critical-path savings plus output
quality exceed packet, cache, failure and coordination costs under a declared
margin; this document deliberately does not choose that margin.

## 11. Security, privacy and recovery consequences

- Child context must carry opaque authorized Artifact references and minimum
  necessary content. Duplicating screenshot bytes across N providers/runs
  multiplies disclosure, logs and retention.
- A copied permission profile is revalidated authority, not a transferable
  approval token. Background children that cannot ask must fail closed or use a
  parent-mediated request channel.
- A worktree prevents ordinary file collisions but does not isolate network,
  credentials, capture access or process authority.
- Shared output paths need no-follow/containment checks, unique attempt paths,
  staged publication and validation. A child must not overwrite an accepted
  sibling Artifact.
- Child transcripts and notifications may support diagnosis/resume but do not
  establish Workflow completion. After restart, reconcile typed attempts and
  Artifacts; do not infer success from final prose.
- Cancellation intent and cleanup ownership must be recorded separately from
  transient abort controllers if children or remote work can outlive a Turn or
  process.

## 12. Non-goals and preliminary fit without final selection

This comparison does not:

- replace Rollshot's current serial Smart Redaction path;
- require Action Guide to batch suggestions;
- commit Rollshot to brag, Hyperframes or video generation;
- select one upstream child-agent implementation or provider;
- design a general coding-agent team platform;
- treat a Todo, Goal, work ledger, child transcript or notification as Workflow
  state;
- specify a final Artifact store, scheduler, permission system or UI; or
- optimize for maximum child count.

| Candidate | Smart Redaction | Action Guide | Deferred brag + Hyperframes |
|---|---|---|---|
| **Keep inline serial** | Direct current fit; one budget/authority owner [W1]. | Fits independent tasks while batch demand is unproven [W2]. | Correct fallback; may be slow at sufficiently large/heavy scene counts [E:H2]. |
| **A: revision-bound proposals** | More machinery than current trace proves [W1]. | Plausible for explicit multi-step batches; requires product evidence [W2]. | Does not express staged artifact assembly by itself [W3-W5]. |
| **B: inspection specialists** | Plausible only after measurements show useful independent expensive calls [W1]. | Can produce read-only evidence for reviewed steps [W2]. | Useful for inspection, not scene publication or Workflow recovery [W3-W5]. |
| **C: artifact workers** | Excessive for one bounded proposal [W1]. | Useful only for a product-owned batch with typed outputs [W2]. | Strong semantic match if the deferred workload becomes real and exceeds inline economics [E:H1, E:H2]. |

No candidate is selected. Synthesis must decide whether any workload crosses
the measured dispatch threshold and whether orchestration belongs in the app,
a product Workflow service, or the agent foundation.

## 13. Evidence gaps and bounded audits

The code-review graph was queried first. Rollshot's graph contained 7,979
nodes across 405 files and located `AgentRunner`, `run_tool_turn`,
`ToolRegistry` and Action Guide visual-annotation paths. Each ignored reference
root—Pi, oh-my-pi, Codex, Claude Code and Hyperframes—returned zero nodes,
edges and files, so bounded source/test inspection followed.

All negative claims are limited to these exact audits:

- **[A:R] Rollshot child boundary.** Literal files:
  `crates/rollshot-agent/src/{domain,driver,model,provider,runtime,tools}.rs`.
  Case-insensitive regex:
  `sub.?agent|child.?agent|spawn.?agent|worker.?registry|agent.?queue|agent.?semaphore|agent.?concurr|artifact.?completion|expected.?artifact`.
  Result: **0 hits**. The named child lifecycle/scheduler/artifact completion
  concepts were **not found in the investigated scope**.
- **[A:P0] Pi built-in child boundary.** Roots:
  `packages/agent/src`, `packages/coding-agent/src/core`, and coding-agent
  `sessions.md`/`extensions.md`. Regex:
  `sub.?agent|child.?agent|spawn.?agent|fork.?agent|agent.?queue|agent.?semaphore|agent.?concurr|expected.?artifact|artifact.?completion`.
  The only hit was the extension-guide row linking to `examples/extensions/subagent`.
  A built-in child lifecycle was **not found in the investigated scope**.
- **[A:P1] Pi example governance/completion.** Literal files:
  `packages/coding-agent/examples/extensions/subagent/{index,agents}.ts` and
  `packages/coding-agent/examples/extensions/subagent/README.md`. Regex:
  `token.?budget|cost.?budget|wall.?time|max.?turn|max.?token|permission.?profile|sandbox|skill|provider|expected.?artifact|artifact.?completion|retry|fair|backpressure|queue`.
  Hits were README prose mentioning providers and a temporary-file mutation
  queue; none defined the named child budget, provider/Skill inheritance,
  permission profile, spawn fairness/backpressure or Artifact/retry contract.
- **[A:P2] Pi addressed-cancellation gap.** The same three literal files were
  searched with
  `cancel|abort|signal|kill|terminate|child.?id|task.?id|agent.?id|address|interrupt`.
  Hits were the tool-call `AbortSignal`, `wasAborted`, the direct
  `proc.kill("SIGTERM")` call, the five-second conditional
  `if (!proc.killed) proc.kill("SIGKILL")` call, README abort prose, result
  agent labels and a UI “Canceled” string. Direct source
  inspection showed the same `signal` passed to every parallel
  `runSingleAgent`; each process closure owns only its local `proc`. No child
  process/controller ID is returned or accepted by a cancel/interrupt API.
  Addressed single-child cancellation was therefore **not found in these exact
  roots**; this does not claim the host lacks whole-tool cancellation.
- **[A:O] oh-my-pi Workflow, budget, Artifact and Job durability.** Roots:
  `packages/coding-agent/src/task` and
  `packages/coding-agent/src/async/job-manager.ts`. Regex:
  `dependsOn|depends_on|blockedBy|blocked_by|workflowId|workflow_id|next.?ready|readiness|expected.?artifact|artifact.?completion|parent.?budget|child.?budget|hierarch.{0,20}budget|serialize|deserialize|rehydrate|reattach`.
  Hits were JSON/schema serialization and an in-process git mutation comment;
  the named Workflow readiness, Artifact completion, hierarchical budget and
  Job restart contract were **not found in the investigated scope**.
- **[A:OQ] oh-my-pi admission/fairness scope.** Literal roots:
  `packages/coding-agent/src/task/{index,parallel}.ts` and
  `packages/coding-agent/src/async/job-manager.ts`. Regex:
  `cross.?session|global.?fair|durable.?fair|persist|rehydrate|reattach|restart|fair|admission|queue|waiter|session`.
  Hits establish one `#spawnSemaphore` per `TaskTool`/session, an in-memory
  waiter array admitted with `shift()` (FIFO), and Job `queued` flags that hold
  no execution slot; direct Job registration at capacity errors. They do not
  construct a shared cross-session admission queue or durable fairness state.
  Consequently FIFO fairness is supported only inside one live TaskTool
  semaphore; durable/cross-session fairness remains a source-bound gap.
- **[A:C1] Codex admission/fairness.** Roots: `core/src/agent`, V1
  `multi_agents.rs`, V2 handler directory and `core/src/config/mod.rs`. Search
  terms `queue|queued|fair|backpressure` found mailbox/input/persistence/resume
  queues, not a spawn-admission queue or fairness/backpressure policy. Direct
  source inspection found atomic V1 reservation and V2 LRU residency followed
  by `AgentLimitReached`.
- **[A:C2] Codex V2 depth.** Exact roots:
  `multi_agents_v2/spawn.rs`, `multi_agents_common.rs`,
  `agent/control/spawn.rs`, and `agent/registry.rs`; symbols
  `next_thread_spawn_depth|thread_spawn_source|spawn_agent_with_communication|agent_max_depth|max_depth|exceeds_thread_spawn_depth_limit`.
  V2 records depth, but a V2 enforcement call was **not found in the
  investigated scope**; config says the V1 setting is ignored by V2.
- **[A:C3] Codex Artifact completion.** The [A:C1] roots were searched for
  `expected.?artifact|artifact.?completion|artifact.?contract`; **0 hits**.
  Typed child Artifact validation/retry was **not found in the investigated
  scope**.
- **[A:C4] Codex Tool/Skill spawn boundary.** Exact implementation roots:
  `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs`,
  `codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs`,
  `codex-rs/core/src/tools/handlers/multi_agents_common.rs`,
  `codex-rs/core/src/agent/{control.rs,control/spawn.rs,role.rs}`,
  `codex-rs/core/src/thread_manager.rs`, and
  `codex-rs/core/src/tools/{router,spec_plan}.rs`; focused tests:
  `codex-rs/core/src/agent/{control,role}_tests.rs` and
  `codex-rs/core/src/tools/handlers/multi_agents_tests.rs`. Symbol/term audit:
  `build_agent_spawn_config|apply_spawn_agent_role|apply_role_to_config|build_tool_router|dynamic_tools|selected_capability_roots|UserInput::Skill|skills.config|FullHistory|LastNTurns|fork_turns`.
  Hits establish: live config is cloned then role/model/runtime layers resolve
  per spawn; role config can alter available Skill configuration; each child
  builds a Tool router from its own turn/config/runtime services; fresh and
  forked thread constructors both pass `Vec::new()` for dynamic Tools; the
  fork filter drops prior Tool calls/outputs; and `all`/last-N copy
  `selected_capability_roots` explicitly before truncation while `none` uses a
  fresh thread/extension state. A second exact-root regex,
  `invoked.?skill|skill.?version|skill.?snapshot|skill.?authority|skill.?package.?id|skill.?revision`,
  returned **0 hits**. Thus configured/available Skills and selected capability
  roots have positive source paths, but durable inheritance of an invoked-Skill
  ledger/version is **not established**. The audit does not claim every Tool
  service is absent: core/MCP/extension availability is re-resolved rather than
  copied from the parent's current model-visible registry.
- **[A:L] Claude agent economics/completion.** Roots:
  `src/tools/AgentTool`, `src/tasks/{LocalAgentTask,InProcessTeammateTask,RemoteAgentTask}`,
  `src/utils/swarm`, and `src/utils/agentSwarmsEnabled.ts`. Regex:
  `expected.?artifact|artifact.?completion|artifact.?contract|max.{0,20}(agent|teammate|swarm)|agent.{0,20}max|teammate.{0,20}max|swarm.{0,20}max|semaphore|queue.{0,20}(spawn|agent|teammate)|fair|backpressure|provider.?override|provider.?model`.
  Hits exposed Agent `maxTurns` and local notification queues, not a visible
  global/team concurrency cap, admission fairness/backpressure, generic
  provider override or expected Artifact contract. Those concepts were **not
  found in the investigated external-source scope**; hidden service policy may
  exist.
- **[A:H] Hyperframes layer conflicts and unspecified governance.** Literal sources:
  `hyperframes-core/references/subagent-dispatch.md` and
  `general-video/SKILL.md`. Complete reading establishes H1's one-scene
  dispatch, hard-cap waves, Artifact-existence WAIT and one missing-Artifact
  re-dispatch; H2 separately establishes two-to-three scene packets per worker,
  all workers in one wave, workload economics and validation gates. Neither
  source resolves hard-cap waves versus H2's single wave, grouped-worker
  partial publication/sibling retention, or retry of an existing but invalid
  Artifact. A portable child token/cost budget, provider/model/permission
  policy, cancellation tree or fairness algorithm is also unspecified. These
  are source-bound gaps, not assertions about every supported harness.

Required spikes before synthesis can select a pattern:

1. Run the same bounded Rollshot trace inline, packetized-serial and at child
   concurrency 2/4; collect the economics in Section 10.
2. Inject notification loss, early exit, missing/partial Artifact, stale input
   revision and cancellation at every fan-in boundary; verify no optimistic
   completion and only selective retry.
3. Exercise permission denial and parent-mediated approval for a background
   child; prove it fails closed when no user channel exists.
4. Crash a coordinator after child completion but before fan-in commit; rebuild
   readiness from typed attempts and Artifacts without child prose.
5. Runtime-test V2 depth/residency, OMP dynamic-cap fairness/Job loss, Claude
   gated team/remote availability, and Pi subprocess cleanup only if a pattern
   depends on them. Static source is insufficient.

## 14. Evidence index and limitations

### Rollshot and workloads

- **[E:R1] Source + test source:**
  `crates/rollshot-agent/src/driver.rs` — `AgentRunner::{run_with_provider,run_tool_turn}`;
  `runtime.rs` — `RunBudget`, `RunCancellation`; `tools.rs` —
  `ToolRegistry::execute_calls`; graph results for `AgentRunner`,
  `ToolRegistry`, and `visual_annotation_agent`. Supports serial baseline and
  live ownership; no provider/UI run.
- **[W1] Source + test source:** Smart Redaction workbench and agent driver/tool
  paths recorded in Round 0. Supports one bounded review-producing run.
- **[W2] Source + test source:**
  `rollshot-app/src/timeline_workspace/{visual_annotation_agent,caption_agent}.rs`
  and Action Guide proposal/project paths. Supports revision-bound independent
  proposals, not fan-out demand.
- **[W3-W5] Deferred workload source:** pinned brag `SKILL.md`; Hyperframes
  production/review loop and [E:H1-H2]. Not Rollshot implementation.

### Pi

- **[E:P1] Example source, not installed default:**
  `packages/coding-agent/examples/extensions/subagent/{index,agents}.ts` and
  `README.md`: subprocess invocation, prompt/model/Tool scope, chain, 8/4 caps,
  output accounting, the source-level SIGTERM call and conditional SIGKILL
  call. Runtime signal delivery and cleanup were not executed.

### oh-my-pi

- **[E:O1] Source + inspected test source:**
  `src/task/{index,types,executor,parallel,worktree,isolation-runner}.ts`,
  settings `task.maxConcurrency`, `maxRecursionDepth`, `maxRuntimeMs`, and
  dynamic semaphore tests cited by the Reviewed profile. Supports Task scope,
  optional isolation, cap and cancellation; tests not run.
- **[E:O2] Source:** `src/async/job-manager.ts`: default 15 active Jobs,
  queued flag, owner-scoped controller, progress/delivery/retention. Process
  memory only.
- **[E:O3] Source + inspected test source:**
  `task/executor.ts::driveSessionToYield` and `finalizeSubprocessOutput`:
  three reminders, schema/raw fallback and strict failure. No expected Artifact.

### Codex

- **[E:C1] Source:** `features/src/lib.rs` and `core/src/config/mod.rs`:
  V1/V2 stage/defaults and version resolution at the pinned revision.
- **[E:C2] Source + inspected tests:**
  `core/src/agent/{control,control/spawn,control/residency,registry}.rs`,
  V1/V2 handlers, role/config application, Tool router/spec planning,
  ThreadManager spawn paths and their focused tests: config inheritance,
  fork modes/capability roots, per-child Tool/Skill resolution, caps, LRU
  residency, interrupt and completion watcher. Tests were inspected, not
  executed. See [A:C4] for the exact Tool/Skill audit and bounded gap.

### Claude Code source

- **[E:L1] Source:** `AgentTool/runAgent.ts`, `utils/forkedAgent.ts`,
  `LocalAgentTask`: prompt/fork context, mutable-state isolation, root Task
  visibility, Tool/Skill/MCP/model/permission resolution and async lifecycle.
- **[E:L2] Source, feature-gated:** `AgentTool/forkSubagent.ts` and gated
  AgentTool callsites: full prefix/exact tools/model/thinking, async-only,
  bubble permissions and recursion guard.
- **[E:L3] Source, external feature-gated:** `agentSwarmsEnabled.ts`,
  `InProcessTeammateTask`, `utils/swarm/{spawnInProcess,inProcessRunner}.ts`,
  `teammateMailbox.ts`: env/flag + GrowthBook gate, peer loop/mailbox/idle/
  shutdown behavior. No team runtime run.
- **[E:L4] Source, external launch unavailable:** `RemoteAgentTask.tsx`,
  AgentTool's literal `"external" === 'ant'` launch guard, and remote sidecar
  helpers in `sessionStorage.ts`. Supports gated source semantics only.

### Hyperframes

- **[E:H1] Workflow source:**
  `skills/hyperframes-core/references/subagent-dispatch.md`: complete dispatch
  prompt, filesystem-only assumption, one scene per dispatch, hard-cap waves,
  Artifact-existence WAIT, one fresh re-dispatch for a missing Artifact and the
  fallback ladder.
- **[E:H2] Workflow source:** `skills/general-video/SKILL.md` §5: measured
  inline/packet economics, two-to-three scenes per worker, all workers in one
  wave, packet builder, expected HTML/motion sidecars and validation gates. It
  does not define grouped partial-success or invalid-Artifact retry.

**Limitations:** Confidence is high for visible pinned source fields, gates,
caps, context construction and exact negative audits; medium for behavior
backed by tests that were inspected but not run; and low for deployed provider
limits, server-side gates, fairness under contention, worktree/permission
containment, remote services, cancellation races, restart recovery and the
actual Rollshot economics because none was exercised. A missing search result
is never proof that another package, hidden build or later revision lacks the
feature. Hyperframes measurements describe its current workflow, not Rollshot.

Open questions for later synthesis are: whether any Rollshot workload crosses
the dispatch threshold; whether child authority/budgets belong to the app or a
foundation scheduler; whether Action Guide ever needs batch orchestration; and
whether the deferred workload becomes active enough to justify durable
Artifact/Workflow machinery.
