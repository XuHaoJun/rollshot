# Subagents and parallelism comparison

**Research date:** 2026-07-22 (Asia/Taipei)
**Status:** In Progress (Round 3 capability comparison)
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
| **Deferred brag + Hyperframes** | Plan/check, scene build, assembly, render, poster and share-copy stages have explicit prerequisites. Optional scene workers consume packets; audio or generation may overlap independent work. [W3-W5] | If adopted, this trace establishes bounded fan-out/fan-in, artifact validation, checkpoints and selective retry. It does not require Rollshot to ship video, use a general Workflow engine, or keep one long coordinator context. |

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

### 3.1 Dispatch and isolation requirements

- The child's prompt is the full worker role plus exact dispatch context. The
  packet and files on disk are its entire world; it must not rely on seeing the
  parent's conversation, Memory or Skills.
- General-video's packet builder inlines each scene's storyboard block,
  blueprint and cited rules. Workers read only their assigned packets and the
  design truth file, not the shared storyboard or skill documents.
- One worker receives two to three scenes only when dispatch pays for itself.
  The expected outputs are each scene's
  `compositions/<frame-id>.html` and `.motion.json`.
- Workers are independent except for the project filesystem. Independence is
  a design precondition: shared-file mutation, implicit ordering and hidden
  predecessor reads invalidate parallel fan-out.
- A harness cap reduces active parallelism, never scope. If the harness queues,
  submit all items; if it hard-caps, dispatch cap-sized waves until every
  artifact exists. Do not drop or merge work merely to fit the cap.
- Native delegation is optional. The fallback ladder is headless CLI workers,
  then serial inline execution from the same packet.

### 3.2 Completion and selective retry

`WAIT` completes on expected Artifacts existing and passing their applicable
validation, never on the harness notification. A notification can be lost,
duplicated, early, or detached from a failed publication. A missing Artifact
means that item failed; Hyperframes allows one fresh re-dispatch with the same
packet plus the concrete gate failure. Successful siblings are retained. [E:H1]

This yields a two-channel model:

```text
transient channel: child started / progress / notification / exit
                         |
                         v
durable gate: expected artifact published -> validate schema/content/hash
                         |
               valid ----+---- invalid or missing
                 |                    |
                 v                    v
          item complete       selective retry once
                 |                    |
                 +------ fan-in ------+
                            |
                  successors become ready
```

The contract does not specify hierarchical budgets, durable cancellation or
fair scheduling. Those remain foundation questions rather than inferred
Hyperframes behavior [A:H].

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
details. One shared abort signal sends SIGTERM, then attempts SIGKILL after five
seconds. Chain mode stops at the first failed child and injects the preceding
final text through `{previous}`. [E:P1]

This is useful subprocess evidence, but its model/tools/caps/cancellation are
extension-local. A finite token/cost/wall-time budget, Skill/provider
inheritance, permission profile, spawn fairness/backpressure, expected Artifact
contract and selective retry were **not found in the example scope** [A:P1].

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
backpressure, not a durable or cross-session fair scheduler. [E:O1, E:O2]

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

V1 `spawn_agent` creates a child Thread/Session and can fork filtered parent
history. Spawn configuration inherits the live model provider/model,
reasoning, approval policy, permission profile, cwd, environment snapshot and
conditional exec policy; a role or model/reasoning override can then narrow or
replace selected values. The default V1 cap is six spawned threads across the
Session's shared registry and maximum depth is one. Admission is atomic and
returns `AgentLimitReached`; it is not a queued spawn scheduler. [E:C2]

V2 is path/mailbox based: spawn, send/follow-up, interrupt, list and wait.
`fork_turns` defaults to `all` and accepts `none`, `all`, or a positive last-N
turn count. Full history is filtered to suitable rollout content rather than
blindly cloning every transient result; selected capability roots are carried
through the fork boundary. Spawn edges persist, and idle children may be
cold-loaded or LRU-unloaded for residency. [E:C2]

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
| **Codex V1/V2** | Fresh or full/last-N filtered fork; V2 separate mailbox/path. | Same selected environment/workspace unless environment policy differs; files are ambient coordination. | Provider/model/reasoning inherited; role/model overrides; full fork carries selected capability roots. General invoked-Skill inheritance for fresh children was **not found in the focused compact/persistence evidence** [A:C4]. | Approval policy, permission profile, environment and exec policy inherited as snapshots and then enforced in separate child Thread. |
| **Claude local/fork** | Regular prompt-only child or explicit context; fork keeps byte-exact parent prefix. Mutable state isolated; root Runtime Task registry shared. | Shared cwd, optional worktree; sidechain transcript/output path. | Regular agent resolves tools/Skills/MCP/model; fork uses exact tools/model/thinking. Provider remains Claude-specific. | Async normally avoids prompts; scoped rules/agent mode apply. Fork uses parent-bubbling permission mode. |
| **Claude teammate** | Independent persistent loop and mailbox; capped UI mirror. | Shared/team-selected workspace plus transcript/mailbox files. | Own scoped Tool/model context; configured agent Skills preload through the shared `runAgent` path; can spawn only allowed synchronous children [E:L1, E:L3]. | Independent permission mode; permission can be mediated through leader/mailbox. External feature gates apply. |
| **Claude remote** | Remote service owns live context; local identity/status sidecar. | Remote environment/session URL and logs. | Claude remote stack; general provider choice was **not found** [A:L]. | Remote eligibility/account/build gates; kill archives remote session. External launch is unavailable at the pinned external build [E:L4]. |
| **Hyperframes worker** | Complete role + packet; no inherited conversation/Memory/Skills. | Shared project but disjoint expected scene paths. | Harness-selected worker; packet inlines required recipes. Provider/model/permission policy is unspecified [A:H]. | Harness grants must be explicit; artifact contract is not a sandbox. |

## 6. Scheduling, cancellation and completion matrix

Every negative or unknown cell cites an exact audit in Section 13.

| Design | Admission, queue, fairness and backpressure | Cancellation | Completion and retry |
|---|---|---|---|
| **Rollshot** | One Run and serial tool batch; child admission does not exist [A:R]. | One cancellation source reaches provider and automation. | Typed Run terminal/proposal; no child completion [A:R]. |
| **Pi example** | Max 8 per call, 4 active; source-order local pool. Cross-call/global fairness or backpressure was **not found** [A:P1]. | Shared signal sends TERM then KILL to each subprocess. No addressed single-child cancel was found [A:P1]. | Process/assistant result; chain stops on failure. Expected Artifact validation/selective retry was **not found** [A:P1]. |
| **OMP Task + Job** | FIFO session semaphore default 32, dynamically resized; Job active cap default 15, parked items excluded, direct over-cap registration errors. No durable/cross-session fairness policy [A:O]. | Abortable semaphore waits, child run and owner-scoped Job cancellation. Process death loses controllers [A:O]. | Yield/schema/raw fallback; delivery retry is in memory. Expected Artifact gate and durable attempt ledger were **not found** [A:O]. |
| **Codex V1** | Shared registry cap 6, depth 1; hard error, no spawn queue [E:C2, A:C1]. | Explicit interrupt; separate legacy tree shutdown. | Completion watcher notification/status. Typed Artifact validation/retry was **not found** [A:C3]. |
| **Codex V2** | 4 total including root; LRU idle unload then hard error. Mailbox queue is not admission queue; fairness unknown [A:C1]. | Explicit per-path interrupt; persistent topology is not cancellation intent. | Parent mailbox/status; no expected Artifact contract [A:C3]. |
| **Claude local/fork** | Global child cap/queue/fairness was **not found** [A:L]. Fork shares cache prefix; regular child is colder. | Sync linked to parent; async Task-owned and intentionally unlinked from spawning turn. | Runtime notification/final text; no generic expected Artifact/selective retry [A:L]. |
| **Claude teammate** | Ready ledger items may be claimed, but visible global/per-team concurrency/fairness cap was **not found** [A:L]. | Cooperative shutdown, then forced kill; independent current-work controller. | Mailbox/task status. Ledger completion does not validate a product Artifact [A:L]. |
| **Claude remote** | Remote service scheduling/cap is unknown in the external tree [A:L]. | Kill archives remote session. | Authoritative remote status can restore polling; generic Artifact completion was **not found** [A:L]. |
| **Hyperframes** | Harness queues all or coordinator dispatches cap-sized waves; fairness is unspecified [A:H]. Cap never changes scope. | A portable cancellation contract is unspecified [A:H]. | Validate expected files; re-dispatch only missing/invalid items once with gate failure [E:H1]. |

## 7. Fan-out, fan-in and dependency readiness

Parallelism is safe only after readiness is decided outside the child:

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
Task launch. Hyperframes demonstrates artifact-gated fan-in. None alone is a
complete durable Workflow scheduler.

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
scene/work items. Workers receive disjoint packets and output paths. The
coordinator validates expected Artifacts, selectively retries once, then
unlocks assembly. A coordinator restart rebuilds readiness from durable packets,
checkpoint decisions and artifacts rather than child transcripts.

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
  `examples/extensions/subagent/{index,agents}.ts` and `README.md`. Regex:
  `token.?budget|cost.?budget|wall.?time|max.?turn|max.?token|permission.?profile|sandbox|skill|provider|expected.?artifact|artifact.?completion|retry|fair|backpressure|queue`.
  Hits were README prose mentioning providers and a temporary-file mutation
  queue; none defined the named child budget, provider/Skill inheritance,
  permission profile, spawn fairness/backpressure or Artifact/retry contract.
- **[A:O] oh-my-pi Workflow, budget, Artifact and Job durability.** Roots:
  `packages/coding-agent/src/task` and `src/async/job-manager.ts`. Regex:
  `dependsOn|depends_on|blockedBy|blocked_by|workflowId|workflow_id|next.?ready|readiness|expected.?artifact|artifact.?completion|parent.?budget|child.?budget|hierarch.{0,20}budget|serialize|deserialize|rehydrate|reattach`.
  Hits were JSON/schema serialization and an in-process git mutation comment;
  the named Workflow readiness, Artifact completion, hierarchical budget and
  Job restart contract were **not found in the investigated scope**.
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
- **[A:C4] Codex invoked-Skill inheritance gap.** Prior focused persistence
  audit searched ThreadStore, rollout reconstruction and compact/fork sources
  for `invoked.?skill|skill.?version|skill.?snapshot|skill.?authority|skill.?package.?id|skill.?revision`.
  Durable general invoked-Skill inheritance was **not found**; positive full
  fork evidence is limited to `selected_capability_roots`.
- **[A:L] Claude agent economics/completion.** Roots:
  `src/tools/AgentTool`, `src/tasks/{LocalAgentTask,InProcessTeammateTask,RemoteAgentTask}`,
  `src/utils/swarm`, and `src/utils/agentSwarmsEnabled.ts`. Regex:
  `expected.?artifact|artifact.?completion|artifact.?contract|max.{0,20}(agent|teammate|swarm)|agent.{0,20}max|teammate.{0,20}max|swarm.{0,20}max|semaphore|queue.{0,20}(spawn|agent|teammate)|fair|backpressure|provider.?override|provider.?model`.
  Hits exposed Agent `maxTurns` and local notification queues, not a visible
  global/team concurrency cap, admission fairness/backpressure, generic
  provider override or expected Artifact contract. Those concepts were **not
  found in the investigated external-source scope**; hidden service policy may
  exist.
- **[A:H] Hyperframes unspecified governance.** Literal sources:
  `hyperframes-core/references/subagent-dispatch.md` and
  `general-video/SKILL.md`. Search/complete reading established dispatch,
  cap/wave, packet, completion and retry rules. A portable child token/cost
  budget, provider/model/permission policy, cancellation tree or fairness
  algorithm is not specified in those sources; this is a source-bound gap, not
  an assertion about every supported harness.

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
  output accounting and TERM/KILL behavior. No runtime execution.

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
  V1/V2 handlers and their control/spawn/residency tests: inheritance,
  forks/mailbox, caps, LRU residency, interrupt and completion watcher. Tests
  were inspected, not executed.

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
  prompt, filesystem-only assumption, cap/waves, Artifact WAIT, one fresh
  re-dispatch and fallback ladder.
- **[E:H2] Workflow source:** `skills/general-video/SKILL.md` §5: measured
  inline/packet economics, two-to-three scenes per worker, one-wave fan-out,
  packet builder, expected HTML/motion sidecars and validation gates.

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
