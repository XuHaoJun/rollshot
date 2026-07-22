# oh-my-pi system profile

Status: In Progress (Round 1 system profile)

Research date: 2026-07-22 (Asia/Taipei)

oh-my-pi revision: `7b141199d524b859c357fc89654f10b62b9f3df1`

Release/package baseline: tag `v17.0.7`; `@oh-my-pi/pi-ai`,
`@oh-my-pi/pi-agent-core`, and `@oh-my-pi/pi-coding-agent` `17.0.7`

Upstream Pi sync marker recorded by oh-my-pi:
`b21b42d032919de2f2e6920a76fa9a37c3920c0a` (2026-03-22)

## 1. Scope and reproducibility baseline

This profile statically inspects the clean local oh-my-pi checkout at the
revision above. The repository identifies itself as a Pi fork; its porting
document records the upstream sync marker above. That establishes lineage, not
behavioral equivalence. This profile therefore separates:

- **Pi-lineage shape**: the provider-neutral message/tool loop, stateful
  `Agent`, JSONL conversation tree, and extension/skill form recognizable in
  both systems; this label does not claim that a particular implementation was
  introduced before or after the fork;
- **oh-my-pi built-in, origin unverified**: behavior implemented and wired by
  this revision whose fork provenance was not established by a direct upstream
  comparison in this pass, including Task/subagent infrastructure, background
  jobs, multiple compaction modes, memory backends, managed skills, ACP
  integration, and broader provider routing;
- **repository-documented fork-added (bounded)**: the porting guide explicitly
  says these exist in the fork but not upstream: `StatusLineComponent`,
  multi-credential auth with session affinity, Capability-based discovery,
  MCP/Exa/SSH integrations, LSP writethrough for format-on-save, Bash
  interception, and fuzzy path suggestions in the read tool. This exact
  seven-entry list is the only fork-added classification made here; it is
  documentation evidence, not an independently reproduced upstream diff. [D4];
- **example only**: repository examples that are not loaded by default;
- **feature-gated or experimental**: source exists, but activation requires a
  setting, optional backend, explicit hook/extension, or experimental flag;
- **roadmap/documentation claim**: described by repository prose but not
  promoted to built-in behavior without source support; and
- **not found in the investigated scope**: the exact bounded searches are
  recorded in Section 17. Every bounded-absence conclusion uses the exact
  phrase “not found in the investigated scope.”

The strongest claims below use source plus focused tests. Official repository
documentation is used for user-visible policy and cross-checked against code.
Tests were inspected but **not executed**; no provider call, terminal UI run,
ACP editor session, process restart, crash, or filesystem race was performed.
Static inspection is not runtime observation. [O1, O2, T1-T8]

**Overall confidence:** high for positive source claims cross-checked by focused
tests, medium for documentation-backed product policy and bounded absence
claims, and low for source-derived security inferences or behavior that needs a
live provider, client, restart, or race experiment. Each inference and absence
is labeled at its use site and bounded in Section 17.

The repository knowledge graph contained zero nodes for this checkout, so the
required graph-first exploration yielded no structural evidence and the audit
fell back to bounded source, test, and repository-document searches.

## 2. Architecture and ownership boundaries

At the pinned revision, oh-my-pi implements this layered product architecture:

```text
CLI / TUI / print / RPC / ACP host
  |
  v
coding-agent AgentSession
  |-- SessionManager + injectable SessionStorage + artifacts
  |-- capability/resource/extension/hook/skill aggregation
  |-- approval and ACP client bridge
  |-- compaction, memory, goal, todo, retry, settings
  |-- TaskTool --> child AgentSession(s) --> registry/lifecycle manager
  |                 `--> AsyncJobManager for detached task/bash work
  v
pi-agent-core Agent --> runAgentLoop
  |                     |-- provider stream
  |                     |-- shared/exclusive tool scheduler
  |                     `-- steering/follow-up/asides/pause/deadline
  v
pi-ai model/API boundary --> 14 built-in API adapters or custom adapter
```

`pi-ai` owns provider/model/message types and streaming adapters.
`pi-agent-core` owns the provider-neutral in-memory `Agent`, the loop, tool
scheduling, compaction primitives, telemetry, and event contracts.
`pi-coding-agent` owns product policy: persistence, context construction,
capability discovery, tools, subagents, approval, ACP, memory, and UI-facing
events. [O3-O8]

The capability registry is an **availability and normalization boundary**, not
an authority boundary. It loads registered providers concurrently, keeps them
in priority order, attaches source metadata, filters disabled providers and
extensions, validates results, and deduplicates by capability-specific key with
first/highest-priority wins. It does not grant filesystem, network, process, or
tool authority. A Capability-level permission, authorization, grant, sandbox,
or approval contract was not found in the investigated scope; the exact terms
and capability roots are recorded in [A3]. Approval and client permission are
separate layers. [O9]

## 3. Conversation, session, and run lifecycle

| Term | Meaning at this revision |
|---|---|
| Conversation | Ordered provider-neutral `AgentMessage[]`; assistant messages retain provider/model/API and provider session details needed for continuation. |
| Session | A long-lived `AgentSession` plus `SessionManager`, usually backed by a JSONL conversation tree and session-scoped artifacts. |
| Run | One core prompt/continue lifecycle from `agent_start` to `agent_end`; only one active run per `Agent`. |
| Turn | One streamed assistant response and the tool calls/results that follow it, bounded by `turn_start`/`turn_end`. |
| Task | A `TaskTool` invocation that creates one or a batch of child agent runs; not a general workflow record. |
| Job | An in-process `AsyncJobManager` record for detached bash or Task execution. |
| Workflow | A durable dependency-graph workflow abstraction was not found in the investigated scope; the bounded Task/async/todo/goal search is recorded in [A1]. |

The Pi-lineage loop appends prompt messages, streams an assistant message,
executes tool calls, appends correlated tool results, and continues until the
assistant stops and queued follow-up work is exhausted. The pinned oh-my-pi
implementation includes an
absolute deadline, a pause gate, interruptible signals and polling, asynchronous
asides, soft tool requirements, dialect/provider transforms, and optional
OpenTelemetry/run coverage around that loop. Steering enters after a completed
tool batch; follow-ups enter when the run would otherwise settle. [O3, O4, T1]

Tool concurrency is explicit per tool or per call: `shared` calls can overlap,
while an `exclusive` call waits for every earlier shared call and the preceding
exclusive call; later shared calls wait behind it. Execution uses
`Promise.allSettled`, and tool events/results are emitted in completion order.
The inspected tests cover parallel shared calls and function-valued
shared/exclusive selection. They were not run. [O3, T1]

Abort propagates through `AbortSignal`; a configured deadline and explicit
`Agent.abort()` can end the run. Provider errors and aborts remain terminal
assistant stop reasons. The low-level loop is not itself durable: its active
promise, pause gate, deadline timer, and queued messages are in memory. [O3,
O4]

## 4. Task, todo, workflow, and background-job model

oh-my-pi has three distinct host-owned concepts:

1. **Todo** is a phased reminder list with `pending`, `in_progress`,
   `completed`, and `abandoned` items. Mutations are recovered from tool-result
   details or a user-edit custom session entry. It is branch-aware conversation
   state, not an executor: it has no dependency edges, leases, attempts, or
   output contract. With no session file it is memory-only. [O10]
2. **Goal mode** owns one objective with `active`, `paused`, `budget-limited`,
   `complete`, or `dropped` status, optional token budget, usage, and elapsed
   time. Mode-change entries persist active/paused state and the runtime can
   steer continuation. It is a bounded autonomous-session mode, not a DAG.
   [O11]
3. **Task** delegates one item or a batch to child agent sessions. Each item may
   choose an agent definition, stable name, isolation, output schema, and
   strict/permissive schema enforcement. Settled results carry usage, output
   paths, patches, validation metadata, and extracted tool data. [O12]

Batch Task is fan-out, not dependency scheduling: every item receives common
context and independently acquires a per-parent-session semaphore. A
`dependsOn` field, dependency-edge or DAG-node contract, workflow identifier,
or deterministic next-ready-node abstraction was not found in the investigated
scope; the bounded Task/async/todo/goal search is recorded in [A1].

`AsyncJobManager` supplies process-local background records for bash and Task:
ID, kind, label, owner, optional child-agent ID, running/completed/failed/
cancelled status, abort controller, result/error text, progress details, and an
optional queued flag. Queued items do not consume the running-job cap until
started. Completion delivery retries in memory with exponential backoff and
jitter; completed records are evicted after a retention interval. Jobs can be
listed, polled, watched, or cancelled through the hub tooling. [O13]

This is managed background work across model turns, but not across application
processes. The manager, delivery queue, abort controllers, timers, and results
are maps in one process. Durable job serialization or rehydration
was not found in the investigated scope; the `src/async` terms and roots are recorded
in [A2]. Child session transcripts may be cold-revived separately, but that
does not restore the detached job that originally drove them. [O13]

## 5. Subagents and parallel execution

Task is implemented in oh-my-pi above the small Pi-lineage loop; this profile
does not assign its historical origin without a direct upstream comparison.
Agent definitions are discovered from bundled, user, and project sources and
can constrain model, tools, spawn policy, thinking level, skills, output
schema, and whether the parent blocks. Each child gets its own `AgentSession`
and transcript. It reconstructs a scoped system prompt and selected
capabilities rather than sharing the parent's mutable message array. [O12,
O14]

Parallelism has two caps:

- the per-TaskTool/session semaphore (`task.maxConcurrency`) gates child
  spawns, including fan-out from parallel Task calls; and
- the process `AsyncJobManager` caps actually running detached jobs while
  allowing caller-gated queued jobs.

The source resizes the Task semaphore before acquire and release, so a live
settings change affects new and already queued work. One sentence in
`docs/tools/task.md` says later changes do not resize it; source and focused
semaphore tests contradict that sentence, so this profile follows code. [O12,
T4]

Non-blocking agent definitions become background jobs when async execution and
a job manager are available. Blocking definitions run inline. Children have
async disabled to avoid recursively detached job trees; nested Task remains
available only below `task.maxRecursionDepth`. Parent abort reaches spawn
waiting and child execution. A soft request budget first asks a child to wrap
up, later forces a final `yield`, and finally aborts after a grace allowance.
Wall-time and MCP-call timeout guards also exist. These are child-run controls,
not hierarchical allocation from a global multidimensional budget. [O12, O14]

Subagent completion uses `yield` as its enforced/preferred protocol. The driver
sends up to three reminders and, where the model supports a named tool choice,
requests `yield` on the final reminder.
Compatibility fallback still permits success without `yield`: a cleanly
settled child with an output schema may return JSON that validates against that
schema, while a cleanly settled child without a schema may return nonempty raw
output. Missing/invalid fallback remains a failure where the finalizer requires
structured or nonempty output. Strict schema mode can turn an invalid result
into `schema_violation`. This is stronger than plain notification, but it does
not require an expected product artifact or a separate reviewer decision.
[O14, T4]

Finished non-isolated children may be adopted by `AgentLifecycleManager`:
`idle` children park after a TTL, dispose their live session, retain registry
metadata/session path, and revive on demand. Persisted registry references can
cold-revive after a later process loads them if a top-level reviver factory can
faithfully reconstruct the child. Isolated children have no reviver and are
disposed after their result/patch handoff. The transcript remains readable by
`history://<agent>`. [O15, T5]

Isolation is optional and platform-backed. Isolated Task items work in a
separate workspace/worktree and return patch metadata instead of silently
merging. Ordinary children can share the parent's filesystem and artifact ID
space, so concurrent writes still require task design or isolation; the Task
scheduler does not infer file conflicts. [O12, O14]

## 6. Compaction, context continuity, and memory

Compaction is a first-class session-tree boundary, not deletion. A
`CompactionEntry` stores summary/replacement state, the first kept entry,
tokens, optional details and `preserveData`; context reconstruction walks the
active leaf path and projects from the latest boundary while older JSONL
entries remain available. [O6, O16]

Triggers include manual compaction, provider overflow, incomplete output due to
length, post-turn threshold maintenance, optional mid-run threshold checks, and
idle maintenance. Source names the following strategies:

| Strategy or command | Semantics |
|---|---|
| `context-full` / `/compact soft` | Local model-generated structured summary plus retained tail. |
| `/compact remote` | Provider-native OpenAI Responses compaction or configured remote endpoint, with preserved opaque provider state where supported and local fallback policy. |
| `snapcompact` | Deterministic local bitmap archive of discarded text, reintroduced as image context; no summarizing model/network call, but requires a vision-capable continuation model. |
| `handoff` | Generates a handoff document and starts a new session; it does **not** append a `CompactionEntry`. Mid-run handoff falls back to in-place compaction. |
| `shake` | Surgical context reduction of selected text regions, distinct from full summary compaction. |
| pruning/elision | Removes superseded or uneventful tool-result material from the provider projection while preserving protected results such as skill reads and active plan references. |

A mechanism named `mini-compact`, `microcompact`, or cached microcompaction was
not found in the investigated scope; the compaction roots and exact terms are
recorded in [A5]. `shake`, pruning, and snapcompact should not be renamed to
force cross-system equivalence. [O16]

`session_before_compact` hooks may cancel or supply a custom compaction,
including `preserveData`; failures fall back to normal compaction. A
`session_compact` event fires after persistence. The repository's
`examples/hooks/custom-compaction.ts` demonstrates a Gemini-based replacement,
but it is example-only and loads only when explicitly requested. Inspected hook
tests cover emission, cancellation, replacement, ordering, saved-entry
visibility, and hook failure fallback; they were not run. [O17, X1, T2]

oh-my-pi also has optional cross-session memory, separate from compaction:

- `memory.backend: local` (off by default) runs a startup background pipeline,
  extracts durable signal from eligible persisted main sessions, consolidates
  it into `MEMORY.md`, a compact injected summary, and generated skill
  playbooks, and stores its queue/index in SQLite.
- It skips subagents and non-persisted sessions, uses a lease/heartbeat for
  consolidation, redacts common secret patterns before writing, and exposes
  `memory://` plus view/stats/diagnose/clear/enqueue commands.
- Repository source also contains optional Hindsight and Mnemopi session state
  paths. Their availability depends on the configured backend and is not a
  property of the core loop.

Memory guidance is explicitly heuristic and must be checked against current
repository evidence. It is not executable workflow state, an approval record,
or a replacement for artifacts. [O18]

## 7. Persistence, checkpoints, and resume

The default CLI persistence is an append-only version-3 JSONL tree under
`~/.omp/agent/sessions/<encoded-cwd>/`. A header identifies the session; every
entry has an ID and parent ID, and a mutable leaf selects the active branch.
Entry kinds include messages, model/thinking/service-tier changes, compaction,
branch summaries, custom/custom-message state, labels, title, TTSR injection,
session initialization, and mode changes. `buildSessionContext` walks only the
root-to-leaf path and restores the latest relevant state. [O6]

Default file appends are handed to the OS synchronously, but are never
`fsync`'d: the source describes software-crash safety, not power-loss
durability. Whole-file rewrites use temp-write plus atomic rename through the
storage abstraction. A disk failure is latched. Conversation persistence is
deferred until the session becomes substantive rather than materializing every
empty draft. [O6, O19, T6]

`SessionStorage` is injectable. `FileSessionStorage` is the product default;
`MemorySessionStorage` supports ephemeral/SDK use. `IndexedSessionStorage`,
`SqlSessionStorage` (Postgres, MySQL, SQLite adapters), and
`RedisSessionStorage` have source, exports, and focused tests, but searches of
CLI/product construction sites found the file backend as the default and the
other backends primarily in tests/embedding APIs. They should be called
embeddable alternatives, not default distributed session persistence. [O19,
T6]

Resume restores the conversation branch, model/mode state, compaction boundary,
and session artifacts. Fork creates another JSONL session containing the
selected path and records parent-session metadata. It does not resume an
interrupted provider stream, tool promise, approval prompt, queued aside,
running background job, or open terminal. [O6, O13, A2]

The built-in `checkpoint`/`rewind` pair is a real context-cost control for
top-level sessions: it records the message count and session entry at the
checkpoint, allows exploratory tool work, then replaces the intermediate active
context with a concise model-authored report. Checkpoint and completed-rewind
state is reconstructed from successful tool-result entries after resume or tree
navigation. It does not revert filesystem changes despite an outdated tool
summary calling it “git-based,” and it is not available to subagents. This is a
useful recoverable conversation save point, but not a general workflow
checkpoint with dependency readiness, external-job attachment, or idempotency
keys. Goal and todo state likewise survive through session entries, and Task
child transcripts can be revived. [O10, O11, O15, O31, A1]

## 8. Tools and scheduling

Tools are schema-described `AgentTool`s with execution, optional update,
approval, rendering, intent, and concurrency metadata. They can come from
built-ins, extensions, MCP, capability providers, ACP/client mounting, or SDK
injection. The tool registry/description surface, approval decision, and
model-selected tool call are separate stages. [O3, O8, O20]

The core shared/exclusive scheduler prevents a declared exclusive operation
from overlapping other calls in its batch, while permitting shared calls to
run concurrently. Exclusivity is advisory metadata supplied by the tool; it is
not inferred from filesystem paths or side effects. Tool errors become
correlated tool results unless the run is aborted, preserving the provider's
call/result protocol. [O3, T1]

Large textual tool results can spill into session-scoped files and leave an
`artifact://<numeric-id>` recovery reference in the truncated result. Parent
and child sessions share one artifact manager and ID space; existing IDs are
scanned on resume. The artifact protocol caps whole-resource inline resolution
at 8 MiB and directs callers to selectors/path-based access for larger data.
This is useful output retention, but artifacts are untyped log files without a
built-in revision, provenance graph, acceptance decision, or workflow status.
[O21, A4]

Extensions and hooks can observe or intercept tool calls/results. MCP calls
proxied into subagents have a fixed timeout and abort propagation. These hooks
and dynamic tools increase integration breadth, but they also mean the static
tool registry is not a complete security boundary. [O14, O20, T4]

## 9. Skills and extensions

Skills are metadata plus local files: name, description, `SKILL.md` path,
base directory, source, and optional hidden/model-invocation flag. Discovery
aggregates capability providers and custom paths, realpath-deduplicates skill
files, applies enable/disable/include/ignore settings, and resolves name
collisions by priority. Sources include native oh-my-pi, plugin, Claude/
Claude-plugin, Codex, OpenCode, GitHub, and managed-skill providers. Discovery
is non-recursive at each configured root (`*/SKILL.md`). [O9, O22]

Metadata is injected into the prompt for progressive disclosure; the model
reads instructions/resources on demand. A hidden or
`disable-model-invocation` skill is omitted from the model listing but remains
explicitly reachable by its slash command or `skill://` if active. Child Task
sessions receive discovered skills. An opaque per-run skill-package authority/
version pin was not found in the investigated scope; the relevant skill roots
and exact terms are recorded in [A6]. [O22]

`skill://<name>/<path>` resolves the name against the caller's active skill list
or a process-global active snapshot, then maps to a local base directory. It
rejects absolute paths and lexical `..` traversal and checks that
`path.resolve(target)` remains under `path.resolve(base)`. It does not attach an
authority/package/version identity, and the handler does not `realpath` the
target before reading. Therefore, as a source-based inference, its containment
check does not itself prevent an in-tree symlink from resolving outside the
skill directory. This should be verified dynamically before assigning security
severity. Skill reads also bypass normal result truncation, which favors exact
instructions but can consume large context. [O23, A6]

The broader internal-resource router also serves `agent://`, `artifact://`,
`history://`, `memory://`, `rule://`, SSH and other schemes. These are convenient
local locators, not a cryptographic or opaque resource-authority system. [O24]

Extensions and hooks are executable TypeScript loaded into the same process.
Project roots, user roots, plugins, and explicit CLI paths participate in
loading; the official loading document states that extensions are not
sandboxed. Approval applies when wrapped tools execute, not to arbitrary code
already running inside a trusted extension. [D2, O20]

### Managed and auto-learned skills

Managed skills are an **experimental, off-by-default authoring path**. The
provider root is `~/.omp/agent/managed-skills`; its priority is deliberately
last so authored skills with the same name win. Discovery can surface existing
managed skills whenever skills are enabled, while automatic creation/nudging
requires `autolearn.enabled` (default false). [O25]

The controller runs only for top-level substantive turns, skips aborted, plan,
and goal cases, and requires a minimum tool-call count (default five). With
`autoContinue` it can run a private capture turn; otherwise it provides
standing guidance. Managed skills persist across sessions. This is learning by
generating/editing instruction packages, not model-weight learning. [O25]

The writer constrains names to lowercase letters/digits/hyphens (maximum 64),
caps content at 64,000 UTF-8 bytes, sanitizes descriptions, refuses symlinked
roots/directories, and uses no-follow/single-link checks for file updates.
Same-name mutations are serialized within one process; cross-process races are
not covered by that lock. The `manage_skill` tool requires autolearn; `learn`
also depends on a supported memory backend. [O25]

## 10. Permissions, sandboxing, and trust

General tool policy uses three declared tiers (`read`, `write`, `exec`) and
three modes (`always-ask`, `write`, `yolo`). Per-tool `allow`, `deny`, or
`prompt` policy overrides the mode in every case. A tool with no declaration is
treated as `exec`. Object-form decisions can request an override prompt for a
specific dangerous operation. [O26]

This is an approval gate, not a general OS sandbox. Normal tools execute with
the host process authority; `yolo` approves all tiers unless a user policy
overrides it. Bash marks critical destructive patterns for an override, but
the source policy intentionally auto-approves them in yolo. Task itself is an
`exec`-tier tool; subagent sessions are headless and use yolo internally, while
the parent Task invocation forms the main approval boundary and explicit
per-tool policies continue to apply. Optional Task isolation separates a
workspace, not all process/network/credential authority. [O12, O26, D2]

ACP adds a second client-mediated gate. Its `ClientBridge` can route reads and
writes to unsaved editor buffers, terminals to the host, and permission
requests to `session/request_permission`. In default-config ACP sessions,
`bash`, delete/move, and destructive edit intents ask the editor even when the
schema default mode is yolo; an explicitly selected yolo/auto-approve mode can
skip that ACP gate unless per-tool policy still says prompt. Ordinary edit,
write, and AST edit are not in the special ACP destructive gate. [O27, T3]

“Allow always” decisions are cached only in the live `AgentSession`, keyed by
tool/intent, and cleared when the client bridge changes. They are not JSONL
entries and do not survive resume. Disconnect/abort races reject permission
requests rather than granting them. Inspected ACP tests cover forwarding,
default versus explicit yolo, user prompt overrides, destructive tools,
always/once choices, and abort behavior; they were not run. [O27, T3]

The capability registry, skill discovery, and internal URL router should not be
called permission systems. Project extensions are unsandboxed executable code,
and `skill://` is a local path mapping rather than a grant token. Rollshot would
need a stronger separation of discovered, enabled, authorized, and invoked
capabilities. [O9, O20, O23]

## 11. Budgets, cancellation, retry, and failures

oh-my-pi exposes several local controls rather than one hierarchical budget:

- core run deadline and abort signal;
- provider usage/token accounting and retry/fallback chains;
- Goal token budget and elapsed usage;
- Task maximum concurrency, recursion depth, wall-time, output byte/line caps,
  soft request budget, forced-yield grace, and MCP call timeout;
- AsyncJobManager running-job cap and completion-retention period; and
- memory/compaction-specific token and scan limits. [O3, O11-O14, O18]

Cancellation is strongest inside one live process: parent tool abort reaches
semaphore waits and child sessions; job cancel aborts its registered runner;
agent abort reaches provider and tool signals; lifecycle dispose cancels owned
resources. Process death makes these controllers unavailable. Durable
cancellation-intent reapplication after resume
was not found in the investigated scope; the related async persistence roots and terms are
recorded in [A2]. [O3, O13-O15]

Retry is similarly layered. Provider/model fallback chains can retry model
requests; subagent runs install scoped fallback roles; tool failures return to
the model; async completion delivery retries its callback; compaction has
remote/local fallback; invalid structured child output can be retried before
strict failure. These mechanisms do not share a durable attempt ledger or one
idempotency policy. [O13, O14, O16]

A single run-owned budget object covering tokens, cost, turns, tools, children,
jobs, artifacts, wall time, and cancellation
was not found in the investigated scope; the searched roots and terms are recorded in [A7].
Consequently, a child budget is configured rather than explicitly allocated
from and reconciled to a parent budget.

## 12. Artifacts, events, and observability

The core loop emits typed agent, turn, message, and tool lifecycle events.
`AgentSession` adds persistence, compaction, approval, Task progress/lifecycle,
goal, session-switch, and extension/hook events. Task publishes raw child
events plus aggregated progress on an event bus, and the registry exposes child
status/activity for hub and UI surfaces. [O3, O8, O12, O15, O20]

Optional OpenTelemetry follows GenAI semantic conventions and adds oh-my-pi
attributes. A run collector returns aggregate telemetry and coverage on
`agent_end`; one-shot compaction, handoff, and branch-summary model calls can be
instrumented too. When telemetry is unset the loop avoids tracer lookup; with
no SDK the OpenTelemetry API is a no-op. This is operational observability, not
durable workflow event sourcing. [O28]

JSONL entries form a durable conversation audit trail, while tool-output
artifacts retain overflow bytes and Task outputs retain reports/patches. A
generic typed product-artifact revision, lineage/provenance record,
expected-artifact contract, review decision, or approval-to-immutable-artifact
transaction was not found in the investigated scope; the bounded
session/artifact/Task roots and terms are recorded in [A4]. [O6, O12, O21]

## 13. Provider boundary

`pi-ai` remains provider-neutral at the loop boundary: a `Model<Api>`, context,
options, and stream function produce normalized assistant stream events. The
normalized types deliberately retain provider, API, model, request IDs,
thinking signatures, usage, and provider-session state where continuation
requires them. “Provider-neutral” therefore means one host contract, not loss
of provider-specific continuity. [O4, O29]

The API registry reserves 14 built-in adapter identifiers at this revision:
OpenAI completions/responses/Codex responses, OpenRouter, Azure responses,
Anthropic, Bedrock, Google Generative AI/Gemini CLI/Vertex, Ollama, Cursor,
GitLab Duo Agent, and Devin Agent. Extensions can register additional streaming
functions under non-reserved API names. `CATALOG_PROVIDERS` contains 61
top-level provider IDs in the pinned source, and `KnownProvider` is derived from
that array; actual availability still depends on disablement, credentials,
keyless-local rules, and dynamic discovery. [O29, O30]

The coding-agent model registry merges bundled models, `models.yml` custom
providers/models, runtime-discovered local engines, and extension
registrations. Local Ollama, llama.cpp, and LM Studio may be keyless.
Credentials and OAuth are provider-scoped. This breadth is an integration
strength but also expands compatibility, retry, privacy, and test surface.
[D3, O30]

## 14. Strengths for Rollshot

- The Pi-lineage loop stays understandable while exposing explicit
  shared/exclusive tool scheduling, cancellation, deadlines, and optional
  telemetry.
- JSONL tree sessions make branch and compaction boundaries inspectable, and
  the injectable storage contract demonstrates how conversation persistence
  can be separated from the session manager.
- Task combines scoped child sessions, bounded fan-out, live progress, explicit
  yield with defined compatibility fallback, caller-defined output schemas,
  optional isolation, and revivable child transcripts. Those are useful
  patterns for bounded specialist work.
- Capability loading cleanly aggregates heterogeneous discovery sources with
  source metadata, deterministic priority, validation, disablement, and
  collision policy.
- `skill://` and other internal resources provide a compact progressive-
  disclosure UX; managed-skill write hardening shows attention to generated
  content and filesystem races.
- Compaction distinguishes summarization, provider-native state, deterministic
  image archive, handoff, and surgical reduction rather than treating every
  context-saving mechanism as one operation.
- Approval, ACP mediation, per-tool policy, structured child output, artifact
  spill, and optional memory are concrete product integrations absent from a
  minimal loop.

## 15. Mismatches and risks

- Task is a child-run facility, not a durable workflow engine. Fan-out batches,
  todos, goals, jobs, and session trees do not supply dependency readiness,
  leases, attempts, idempotent recovery, or artifact-gated completion.
- Async jobs do not survive process restart. Rollshot render, capture, or cloud
  jobs may outlive a turn or application process and need durable handles plus
  reattachment.
- Conversation resume is rich, but active approvals, job controllers, provider
  streams, and external process state are intentionally not reconstructed.
- Tool tiers and ACP prompts are useful policy, but normal execution and
  project extensions are not sandboxed. Headless children rely heavily on the
  parent Task boundary and explicit policy.
- Capability availability is easy to confuse with authority. Rollshot should
  keep capability declaration, user grant, runtime authorization, and tool
  selection distinct in types and persistence.
- `skill://` uses a process-global fallback snapshot and local lexical path
  mapping. An opaque package authority/version pin
  was not found in the investigated scope
  [A6], and the lexical resolver has a possible symlink
  containment gap inferred from source. A durable Rollshot run should record
  exactly which skill package revision it used.
- Memory and managed skills deliberately synthesize durable text. For screenshot
  content, Rollshot would need stronger provenance, retention, deletion,
  sensitivity, and opt-in boundaries than generic project memory.
- Tool spill artifacts and Task output paths are not the typed, versioned,
  review-producing artifacts needed by Smart Redaction or Action Guide.
- The provider catalog and multiple compaction paths are broad but carry a
  large compatibility surface; Rollshot should adopt contracts selectively,
  not copy feature count.

### Feature-gated, example, and roadmap boundary

The following must not be presented as default core behavior: local autonomous
memory (`memory.backend` defaults off), Hindsight/Mnemopi backends, autolearn
(experimental and disabled), custom compaction hooks, example Gemini
compaction, optional Task isolation, async Task (setting/host dependent), ACP
editor mediation (host dependent), remote compaction (model/endpoint dependent),
snapcompact continuation (vision-model dependent), and non-file session-storage
adapters (embedding choice). Repository examples demonstrate extension points;
they are not evidence that the default CLI invoked them. [O16-O19, O25, O27,
X1]

## 16. Unresolved questions

1. Does Task cold revival behave correctly after a real process restart when
   plugins, skills, models, MCP tools, or working directories changed?
2. What filesystem escape is possible through a symlink nested under an active
   skill directory, and does any outer read-tool check close it?
3. How do ACP clients differ in permission-option support, unsaved-buffer
   routing, disconnect behavior, and terminal lifetime?
4. How well do strict child schemas recover from malformed streaming output,
   and can expected artifact existence be incorporated without model-only
   claims?
5. What information is lost across each compaction strategy for active Task,
   todo, goal, approvals, and provider session state?
6. How do SQLite/Redis/SQL session backends behave under multi-process writers,
   crash faults, and schema upgrades outside their focused tests?
7. Can optional memory meet image/document privacy requirements, including
   auditable deletion and source-level provenance?

## 17. Evidence index

### Source evidence

- **O1 — checkout identity and Pi lineage:** repository `README.md`,
  `docs/porting-from-pi-mono.md`, root/package manifests, and git metadata at
  `7b141199d524b859c357fc89654f10b62b9f3df1`.
- **O2 — public package boundaries:** `packages/ai/src/index.ts`,
  `packages/agent/src/index.ts`, `packages/coding-agent/src/index.ts`.
- **O3 — core lifecycle and scheduler:** `packages/agent/src/agent-loop.ts`.
- **O4 — stateful Agent and provider-facing context:**
  `packages/agent/src/agent.ts`, `packages/agent/src/types.ts`,
  `packages/ai/src/types.ts`.
- **O5 — coding-agent orchestration:**
  `packages/coding-agent/src/session/agent-session.ts`.
- **O6 — JSONL tree/session projection:**
  `packages/coding-agent/src/session/session-manager.ts`,
  `packages/coding-agent/src/session/session-entries.ts`.
- **O7 — SDK composition:** `packages/coding-agent/src/sdk.ts`.
- **O8 — tool/session contracts and event bus:**
  `packages/coding-agent/src/tools/index.ts`,
  `packages/coding-agent/src/utils/event-bus.ts`.
- **O9 — Capability registry:** `packages/coding-agent/src/capability/index.ts`,
  `packages/coding-agent/src/capability/types.ts`, and capability definitions
  under `packages/coding-agent/src/capability/`.
- **O10 — todo state:** `packages/coding-agent/src/tools/todo.ts`.
- **O11 — goal mode:** `packages/coding-agent/src/goals/state.ts`,
  `packages/coding-agent/src/goals/runtime.ts`,
  `packages/coding-agent/src/goals/tools/goal-tool.ts`.
- **O12 — Task schema/tool:** `packages/coding-agent/src/task/index.ts`,
  `packages/coding-agent/src/task/types.ts`.
- **O13 — detached jobs:** `packages/coding-agent/src/async/job-manager.ts`,
  `packages/coding-agent/src/tools/hub/`.
- **O14 — child execution/policy/isolation:**
  `packages/coding-agent/src/task/executor.ts` (yield reminder driver at
  lines 1654-1821 and no-yield/schema fallback finalizer at lines 541-671),
  `packages/coding-agent/src/task/spawn-policy.ts`,
  `packages/coding-agent/src/task/worktree.ts`.
- **O15 — child registry and revival:**
  `packages/coding-agent/src/registry/agent-registry.ts`,
  `packages/coding-agent/src/registry/agent-lifecycle.ts`,
  `packages/coding-agent/src/task/persisted-revive.ts`.
- **O16 — compaction:** `packages/agent/src/compaction/`,
  `packages/coding-agent/src/session/compact-modes.ts`, and the compaction paths
  in `packages/coding-agent/src/session/agent-session.ts`.
- **O17 — compaction extension events:**
  `packages/coding-agent/src/extensibility/shared-events.ts`,
  `packages/coding-agent/src/extensibility/extensions/`,
  `packages/coding-agent/src/extensibility/hooks/`.
- **O18 — memory:** `packages/coding-agent/src/memories/`,
  `packages/coding-agent/src/memory-backend/`,
  `packages/coding-agent/src/internal-urls/memory-protocol.ts`, plus Hindsight
  and Mnemopi session state used by `task/executor.ts`.
- **O19 — storage implementations:**
  `packages/coding-agent/src/session/session-storage.ts`,
  `indexed-session-storage.ts`, `sql-session-storage.ts`, and
  `redis-session-storage.ts`.
- **O20 — extensions/hooks and tool wrapping:**
  `packages/coding-agent/src/extensibility/extensions/`,
  `packages/coding-agent/src/extensibility/hooks/`, and
  `packages/coding-agent/src/extensibility/custom-tools/`.
- **O21 — output artifacts:** `packages/coding-agent/src/session/artifacts.ts`,
  `packages/coding-agent/src/internal-urls/artifact-protocol.ts`,
  `packages/coding-agent/src/tools/output-meta.ts`.
- **O22 — skill discovery:** `packages/coding-agent/src/capability/skill.ts`,
  `packages/coding-agent/src/extensibility/skills.ts`.
- **O23 — `skill://` path resolution:**
  `packages/coding-agent/src/internal-urls/skill-protocol.ts` and read-tool
  internal-URL handling.
- **O24 — internal resource router:**
  `packages/coding-agent/src/internal-urls/`.
- **O25 — managed/autolearn skills:**
  `packages/coding-agent/src/autolearn/managed-skills.ts`,
  `packages/coding-agent/src/autolearn/controller.ts`, settings schema, and
  `packages/coding-agent/src/tools/manage-skill.ts`.
- **O26 — general approval:** `packages/coding-agent/src/tools/approval.ts`,
  approval metadata on built-in tools, and extension wrapper policy.
- **O27 — ACP bridge and permission cache:**
  `packages/coding-agent/src/session/client-bridge.ts`,
  `packages/coding-agent/src/modes/acp/acp-client-bridge.ts`, and ACP paths in
  `agent-session.ts`.
- **O28 — telemetry:** `packages/agent/src/telemetry.ts`,
  `packages/agent/src/run-collector.ts`, and instrumented loop/compaction paths.
- **O29 — API boundary:** `packages/ai/src/api-registry.ts`,
  `packages/ai/src/stream.ts`, `packages/ai/src/types.ts`.
- **O30 — model/provider catalog:** `CATALOG_PROVIDERS` in
  `packages/catalog/src/provider-models/descriptors.ts:62-487`, its
  `KnownProvider` derivation at lines 489-490, and
  `packages/coding-agent/src/config/model-registry.ts`. Reproduction command
  `rtk awk 'NR>=62 && NR<=487 && /^[[:space:]]*id:/ {count++} END {print count}' packages/catalog/src/provider-models/descriptors.ts`
  printed `61` at the pinned revision.
- **O31 — context checkpoint/rewind:**
  `packages/coding-agent/src/tools/checkpoint.ts`, checkpoint state and rewind
  handling in `packages/coding-agent/src/session/agent-session.ts`, and
  `packages/coding-agent/src/prompts/tools/checkpoint.md` / `rewind.md`.

### Inspected tests (not executed)

- **T1 — core loop scheduling/lifecycle:**
  `packages/agent/test/agent-loop.test.ts`, especially shared parallelism,
  completion ordering, function-valued concurrency, abort, and steering cases.
- **T2 — compaction hooks and lifecycle:**
  `packages/coding-agent/test/compaction-hooks.test.ts`,
  `compaction-lifecycle.test.ts`, `compaction.test.ts`, and goal/mid-run and
  approved-plan compaction tests.
- **T3 — ACP:** `packages/coding-agent/test/agent-session-acp-permission.test.ts`,
  `acp-client-bridge.test.ts`, `acp-agent.test.ts`.
- **T4 — Task and limits:** Task source tests plus
  `packages/coding-agent/test/task-executor-mcp-timeout.test.ts` and dynamic
  semaphore tests in `issue-3464-ollama-cloud-task-backoff.test.ts`.
- **T5 — lifecycle persistence:** Task persisted-revive and agent-lifecycle
  tests under `packages/coding-agent/src/task/` and
  `packages/coding-agent/test/`.
- **T6 — session storage/durability:** session-storage, SQL, Redis,
  memory-storage, atomic-rewrite, close-race, and session-manager tests under
  `packages/coding-agent/test/`.
- **T7 — managed skills:** managed-skill/autolearn tests colocated with
  `packages/coding-agent/src/autolearn/` and tool tests.
- **T8 — skill/internal URL behavior:** skill discovery/protocol/read tests
  under `packages/coding-agent/test/` and colocated source tests.

### Official repository documentation

- **D1:** `docs/session.md`, `docs/sdk.md`, `docs/compaction.md`,
  `docs/tools/task.md`, `docs/memory.md`, `docs/skills.md`.
- **D2:** `docs/approval-mode.md`, `docs/extensions.md`, `docs/hooks.md`,
  `docs/extension-loading.md`.
- **D3:** `docs/providers.md`, `docs/adding-a-provider.md`.
- **D4 — bounded fork-added classification:**
  `docs/porting-from-pi-mono.md:377-387`, which labels exactly seven preserved
  features as existing in the fork but not upstream.

### Examples, inferences, and bounded absence searches

- **X1 — example only:** `examples/hooks/custom-compaction.ts`; it is not a
  default hook or compaction provider.
- **A1 — workflow/DAG absence:** exact case-insensitive search for
  `dependsOn|depends_on|dependency|DAG|workflowId|workflow_id` across
  `packages/coding-agent/src/task`, `src/async`, `src/tools/todo.ts`,
  `src/goals`, and relevant tests returned only ordinary code-dependency prose.
  Conclusion: a host-owned workflow dependency/DAG contract
  was not found in the investigated scope.
- **A2 — durable job absence:** search for
  `serialize|deserialize|recover|rehydrate|persist|session` across
  `packages/coding-agent/src/async`, `src/task/index.ts`, and the job manager
  returned in-memory job/delivery state and child-session persistence.
  Conclusion: durable job serialization, rehydration, or reattachment
  was not found in the investigated scope.
- **A3 — Capability authority absence:** search for
  `permission|authorize|authorization|grant|sandbox|approval` across
  `packages/coding-agent/src/capability` returned discovery/configuration
  concepts; approval lives under tools/session/ACP. Conclusion: a
  Capability-level authority grant or approval contract
  was not found in the investigated scope.
- **A4 — typed artifact/review absence:** search and source inspection across
  `src/session/artifacts.ts`, `src/internal-urls/artifact-protocol.ts`,
  `src/tools/output-meta.ts`, `src/task`, and session entry types returned spill
  logs and Task reports/patches. Conclusion: a generic typed artifact revision,
  provenance, expected-artifact, or review-decision model
  was not found in the investigated scope.
- **A5 — compaction terminology absence:** exact search for
  `mini-compact|mini compact|microcompact|micro-compact|cached micro` across
  `packages/agent/src/compaction`, coding-agent compaction paths/tests, and
  `docs/compaction.md` returned no matches. Conclusion: a compaction mechanism
  using those names was not found in the investigated scope.
- **A6 — skill authority/version absence:** search for
  `authority|version|pin|snapshot|realpath|symlink` in
  `src/internal-urls/skill-protocol.ts`, `src/extensibility/skills.ts`, and
  `src/capability/skill.ts` returned a process-global active snapshot and
  discovery realpath deduplication. Conclusion: an opaque per-run skill
  authority/version pin was not found in the investigated scope. The
  symlink-containment observation is an inference from the resolver's lexical
  checks, not a runtime exploit result.
- **A7 — unified budget absence:** source inspection and search across
  `packages/agent/src`, `src/goals`, `src/task`, `src/async`, and session policy
  returned multiple local limits. Conclusion: a single hierarchical or
  multidimensional run-budget contract was not found in the investigated scope.

No runtime observations were collected in this pass.
