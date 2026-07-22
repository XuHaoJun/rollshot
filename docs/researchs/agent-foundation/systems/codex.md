# Codex system profile

Status: In Progress (Round 1 system profile)

Research date: 2026-07-22 (Asia/Taipei)

Codex revision: `4a443994bd12f49f2f08b21a2f224d9d42b9e734`

Revision date: `2026-07-22T01:23:44Z`

## 1. Scope and reproducibility baseline

This profile is a static inspection of the pinned local checkout at
`learn-projects/codex`. The repository knowledge graph was queried first, as
required by the Rollshot workflow, but returned zero nodes and zero edges for
the ignored `learn-projects/codex` tree. Source, tests, and repository-owned
documentation were therefore inspected directly. [C1, A0]

Status labels in this profile mean:

- **built-in, default-on**: compiled and active under default feature settings;
- **built-in, default-off**: compiled but gated off by default;
- **experimental/under development**: explicitly classified that way in the
  feature registry;
- **app-server integration**: wired by `codex-app-server`, not necessarily by
  every Codex surface;
- **test-only evidence**: asserted in tests but not independently exercised in
  this research; and
- **not found in the investigated scope**: no matching domain abstraction was
  found in the bounded source searches recorded in Section 17.

The highest-confidence claims below come from Rust source plus tests. Repository
README files are secondary evidence: the exec-server README's statement that a
closed websocket immediately terminates managed processes conflicts with the
pinned `SessionRegistry` source and recovery tests, so source wins and the
conflict is recorded in Section 17. No model request, crash/restart test, remote
exec-server connection, or platform sandbox runtime test was performed. The
revision is a fast-moving same-day snapshot; feature stage and default values
must not be generalized to later versions. [C1, C9, D1, D4]

## 2. Architecture and ownership boundaries

Codex separates conversation orchestration, durable history, execution, and
extensions rather than placing them behind one monolithic agent object:

```text
CLI / TUI / app-server
        |
        v
ThreadManager --> CodexThread --> Session submission loop
                       |              |
                       |              +-- one active SessionTask
                       |              +-- run_turn model/tool loop
                       |              +-- approvals + turn state
                       |              `-- Event stream
                       |
                       +--> ThreadStore / LiveThread --> JSONL + SQLite metadata
                       +--> AgentControl -----------> child Threads / Sessions
                       +--> Environment -----------> local or exec-server process/fs
                       `--> ExtensionRegistry -----> goals, memory, skills, MCP, tools
```

`CodexThread` is the public bidirectional conduit formerly called a
conversation. It wraps an `Arc<Session>`, a bounded submission channel plus
event and watched-status channels, configuration/session metadata, and
persistence access. `Session`
owns the live runtime, services, conversation history, and at most one active
internal `SessionTask`. `ThreadManager` creates, resumes, forks, archives, and
tracks threads. [C2]

`ThreadStore` is the persistence boundary. Its local implementation keeps
canonical rollout history in JSONL and uses SQLite for queryable metadata when
available; an in-memory implementation exists, and the trait permits other
stores. `LiveThread` mediates appends and flushes so core orchestration does not
write raw rollout files directly. [C8, D2]

Execution is a separate boundary. A selected `Environment` supplies process,
filesystem, shell, working-directory, and workspace-root capability. Local
execution uses the host implementation; remote execution uses
`codex-exec-server`, an exec-specific JSON-RPC protocol for process and
filesystem operations. This is an execution transport, not a model provider or
agent scheduler. [C9]

## 3. Conversation, session, and run lifecycle

The important terms at this revision are:

| Term | Observed meaning |
|---|---|
| Thread | Persisted conversation/control unit with a `ThreadId`, rollout history, metadata, and zero or one live `Session`. |
| Session | Live runtime for a Thread: submission loop, services, state, configuration, channels, and active task. It may span many Turns. |
| Turn | User-visible unit beginning with `TurnStarted` and ending in `TurnComplete` or `TurnAborted`; only one is active per Session. |
| Step | One model sampling/tool-advertisement snapshot within a Turn. A Turn can make several Responses requests and execute several tool batches. |
| Internal task | `SessionTask`/`TaskKind::{Regular, Review, Compact}`: Tokio execution machinery, not a user task ledger. |
| Agent Run | A real `codex-agent-extension::AgentRun` containing child `thread_id`, initial `turn_id`, and `CodexThread`; app-server uses it for detached review. It is a launched child invocation, not a durable workflow record. |
| Task | A standalone user-domain `Task` model was **not found in the investigated scope**; protocol v1 aliases call Turn events `task_started`/`task_complete`, and internal Rust tasks are implementation details. [A1] |
| Todo | `update_plan` emits a flat todo/checklist `PlanUpdate`; it is not a workflow executor. |
| Workflow | A durable dependency graph, workflow instance, or workflow scheduler was **not found in the investigated scope**. [A1] |
| Job | A durable background-job record was **not found in the investigated scope**; live background terminal handles are narrower. [A1] |
| Compact | A history-projection operation producing a persisted compaction checkpoint and replacement model context. |
| Memory | A separate, default-off, cross-thread extraction/consolidation subsystem; not the compacted context. |
| Artifact | A general core artifact record was **not found in the investigated scope**; image generation has an extension-owned persisted file path. |

`Session::spawn` starts a Tokio submission loop over a bounded operation
channel. `Op::UserInput` builds a `TurnContext`, first attempts to steer an
eligible active turn, and otherwise spawns a regular task. Spawning a new task
replaces an existing one with an explicit abort reason. The task creates a
cancellation subtree, emits lifecycle events, flushes rollout state, and clears
the active turn on completion or abort. [C2]

`run_turn` creates one `ModelClientSession` for the Turn and may make multiple
Responses requests through it. Each sampling step snapshots the advertised
tools and settings, streams response items, dispatches tool calls, appends tool
results, drains steering/mailbox input, and continues until no follow-up work
remains. Auto-compaction can occur before sampling or mid-turn. Retryable
provider errors stay inside the Turn; non-retryable errors become events and
terminate that execution without corrupting the reusable Thread. [C3, C13]

Turn-only pending approvals, permission prompts, user-input requests, dynamic
tool responses, cancellation tokens, and tool counters live in `TurnState`.
Session-level context, token/rate-limit data, previous settings, and
session-scoped permission grants live in `SessionState`. This split is material
to resume semantics: persisted transcript state is broader than in-flight
continuation state. [C2, C11]

## 4. Task, todo, workflow, and background-job model

`update_plan` is a **core built-in, unconditionally registered tool**: the core
utility tool plan adds `PlanHandler` without a feature check. It is
intentionally described in source as a todo/checklist tool. Its model is a flat
list of `Pending`, `InProgress`, or `Completed` items plus optional explanation.
The handler emits `EventMsg::PlanUpdate`; no owner, dependency edge, lease,
retry policy, or executor is attached. Plan mode is a different concept: a
model-authored Plan turn item, and the handler rejects `update_plan` there.
[C4]

Thread goals are a distinct **built-in, default-on stable** feature. The
app-server installs goal tools when a state database is available. A Thread may
have one unfinished goal with objective, status, optional token budget, and
usage accounting. `create_goal`, `get_goal`, and `update_goal` persist through
the state database; model-side update is limited to `Complete` or `Blocked`,
while pause/resume and limit states are system/user controlled. A Goal can
govern persistence across turns, but it still is not a Task graph or Workflow.
[C4, C12]

`SessionTask` is internal runtime machinery for regular chat, review, and
compaction. The v1 protocol's legacy serialization names
`task_started`/`task_complete` alias Turn events. Neither should be interpreted
as a user-domain Task abstraction. [C2, C14]

Long-running unified-exec commands can outlive a tool-call response and remain
listed as `BackgroundTerminalInfo` with item/process IDs, command, and cwd.
Thread/app-server APIs list, terminate, and clean these handles. They are live
process-manager entries; durable background-job recovery, scheduling, and
reattachment were **not found in the investigated scope**. The bounded search
covered core, protocol, app-server, and extensions for Task/Todo/Workflow/Job
types and workflow/DAG/background-job terminology. [C9, A1]

## 5. Subagents and parallel execution

Multi-agent v1 is **stable and default-on** under feature key `multi_agent`.
Multi-agent v2 is **stable but default-off** under `multi_agent_v2`. Resolution
selects v2 when enabled, otherwise v1 for compatible/new sessions; old or
resumed histories can retain their recorded version. [C5, C12]

In v1, `spawn_agent` creates a separate child Thread/Session, optionally forks
filtered parent history, applies role/model/reasoning overrides, inherits the
live approval policy, permission profile, environment snapshot, cwd, and
conditional exec policy, and watches completion so the parent receives a
notification. The default v1 limit is six spawned agent threads per user
Session/agent tree, not a process-global limit: the `AgentRegistry` that counts
them is shared by all agents in that Session. The default v1 maximum depth is
one. [C5, C12]

In v2, the collaboration surface is path/mailbox based: `spawn_agent`,
`send_message`, `followup_task`, `interrupt_agent`, `list_agents`, and
`wait_agent`. `fork_turns` defaults to all, accepts none or a bounded count, and
filters a full fork to system/developer/user content plus final assistant
answers rather than copying every tool result. Spawn edges are persisted in an
agent graph store; the root can restore metadata, cold-load persisted children,
and evict/reload idle residents. The pinned v2 default is four concurrent
threads per Session **including the root**, so
`effective_agent_max_threads(V2)` subtracts the root and exposes capacity for
three spawned threads. A v2 maximum-depth check was **not found in the
investigated scope** defined in [A2]. [C5, C12, A2]

Interruption is explicit per child. Legacy control also has a separate
`shutdown_agent_tree` operation; parent Turn interruption is not equivalent to
an automatic durable cancellation tree. Completion communicates status and
messages, not a typed artifact contract. The parent and child may see the same
filesystem, so filesystem writes are coordination side effects rather than
returned artifacts. [C5]

Tool calls have separate parallelism. `ToolCallRuntime` uses a read/write gate:
tools declaring parallel support acquire a shared read lock, while a
non-parallel tool acquires the write lock and serializes against the batch.
Cancellation aborts or allows runtime-specific teardown, then returns a
model-visible aborted result. This is tool scheduling inside a Turn, not
subagent scheduling. [C3]

## 6. Compaction, context continuity, and memory

Local compaction is a **core built-in, non-feature-gated implementation**. With
the default-off `token_budget` feature disabled, manual and automatic
compaction select it by default for providers that are not eligible for remote
compaction; recognized OpenAI/Azure Responses providers instead take the remote
route. Local compaction sends the current history and a summary prompt through
the Responses path, retries stream failures, and on context overflow removes
the oldest history item until the request fits. The resulting model context
retains bounded user messages and appends a summary as a user message. Mid-turn
compaction reinjects canonical context before the last user input;
standalone/pre-turn compaction resets the reference point for the next Turn.
[C6, C12]

The replacement is persisted as `RolloutItem::Compacted` with replacement
history and context-window lineage. Original rollout history remains available;
reconstruction selects the compacted projection. Compaction is therefore a
checkpoint of model-visible history, not durable Task progress, semantic
Memory, or an agent handoff artifact. [C6, C8]

Remote compaction is provider constrained. It is used only for recognized
OpenAI or Azure Responses providers. `remote_compaction_v2` is **stable and
default-on** and performs compaction through the normal Responses stream; the
older remote path calls `/responses/compact`. V2 has a 64k retained-message
budget and two transport retries. [C6, C12, C13]

Named mini-compaction, micro-compaction, and cached-compaction mechanisms were
**not found in the investigated scope**. The exact bounded search covered
`core/src`, `protocol/src`, and `thread-store/src` for those names and cache /
compact combinations. [A3]

Memory is separate and **stable but default-off** under feature key
`memories`. When enabled in app-server and permitted by config, root,
non-ephemeral sessions can run a two-phase pipeline: recent eligible rollouts
are claimed and summarized into redacted raw memories; then a locked
consolidation agent selects and writes retained memory resources under the
Codex home. Memory tools list/read/search/add notes, and read results retain
source citations. Subagents do not initiate the root extraction pipeline.
[C7, C12, D3]

## 7. Persistence, checkpoints, and resume

The local `ThreadStore` writes append-only JSONL rollout items as canonical
history and maintains SQLite-backed searchable metadata where available.
Session metadata, response items, user/event records, Turn context, world
state, compaction checkpoints, token usage, goal updates, and inter-agent
communication can therefore participate in reconstruction. Store and live
thread traits also permit in-memory or externally supplied implementations.
[C8, D2]

Resume loads `InitialHistory::Resumed`, reconstructs the latest model-visible
history, compaction window lineage, previous Turn settings, world-state
baseline, token information, and multi-agent version. Incomplete persisted
turns are recognized as mid-turn for fork/interrupt boundaries. V2 agent graph
metadata supports restoring child topology and cold-loading persisted child
Threads. [C5, C8]

Codex has three distinct recovery layers that must not be collapsed:

1. **Relay-frame resume** reconnects a rendezvous/Noise relay stream and its
   transport sequencing. It is below exec-server JSON-RPC session identity and
   does not reconstruct a Codex Thread or Turn. [C9]
2. **Exec-server session and recoverable-process recovery** is a live execution
   capability. `initialize` accepts `resume_session_id` and returns a stable
   `session_id`. On connection shutdown, `SessionRegistry` detaches the session
   but retains the same `ProcessHandler` for 30 seconds (200 ms in tests), with
   notifications temporarily disabled. Reattach within that TTL installs the
   new notification sender and preserves managed processes. The client retries
   recoverable connection/registry errors for at most 25 seconds, leaving
   margin inside the server TTL. After reattach it calls
   `process/read(after_seq = last_published_seq)` for each acknowledged,
   recoverable process to replay missed output/exit/close events. [C9, T5]
3. **Codex Thread/Turn resume** loads persisted rollout history and reconstructs
   model-visible context as described above. Pending approval/user-input
   oneshots, active tool futures, cancellation tokens, provider streams, and
   process handles are not recreated by `ThreadStore` reconstruction;
   restoration of those objects was **not found in the investigated scope**
   defined in [A4]. [C8, A4]

Exec-server recovery is bounded rather than transparent continuation. Only
processes whose start response completed are marked recoverable; a transport
failure during an unacknowledged start takes the cleanup path. HTTP body streams
are failed before reconnect. Replay depends on retained process events/output
(bounded to 1 MiB or 50,000 chunks per process); an unrecoverable sequence gap
fails and terminates that client process session. Missing reconnect strategy,
stdio transport, TTL expiry, or server shutdown cannot reattach; TTL expiry
shuts down the retained `ProcessHandler` and its processes. These conclusions
are source- and test-backed but were not runtime-tested in this research. [C9,
T5]

## 8. Tools and scheduling

Each model Step builds a `ToolRouter` from core and extension registries and
advertises tool specifications through the Responses request. Tool calls become
typed invocations keyed by model-visible call ID and a runtime call ID. The
runtime records call counts, lifecycle events, terminal outcomes, diffs, and
traces. Unknown or incompatible calls are converted to model-visible failures
unless they are fatal to the Turn. [C3]

Parallel admission is opt-in per tool handler and constrained by the
read/write gate described in Section 5. MCP tools may opt in; read-only MCP
annotations can permit parallel calls. Tools such as plugin installation
explicitly prohibit parallel invocation. The model's
`supports_parallel_tool_calls` capability controls the Responses request flag,
but host-side handler admission still applies. [C3]

Unified exec supports foreground completion and yielded background terminal
sessions. Exec-server environments implement process start/read/write/
terminate plus filesystem RPCs. A selected Turn environment binds environment
ID, cwd, workspace roots, shell, and filesystem/process implementation; tool
arguments can target an environment. [C9]

A generic durable scheduler, dependency-aware Workflow engine, cron/queue Job
model, or artifact-gated executor was **not found in the investigated scope**.
[A1]

## 9. Skills and extensions

Host skill discovery is a **core built-in, unconditionally initialized and
warmed service**, not a feature-gated subsystem. Session initialization creates
or reuses `SkillsService` and snapshots configured roots. Skill instruction
inclusion is separately configurable and defaults to true; bundled-skill root
availability is also separately configurable. `SkillsService` builds an
immutable `HostSkillsSnapshot`, caches by cwd/effective config, and exposes
metadata before selected main-resource content. Main prompt injection and
descriptions have explicit byte/token caps. Explicit mentions can select
enabled skills; `disable-model-invocation` removes prompt visibility without
turning the package into a nonexistent resource. [C10]

The extension contract preserves ownership through
`SkillAuthority { kind, id }`, opaque package IDs, and opaque resource IDs.
Authority kinds are Host, Executor, Orchestrator, and Custom. `list` produces
authority-bound entries; `read` revalidates catalog enablement and routes only
to the matching provider. Host reads use the discovering host filesystem,
executor reads use the selected execution environment filesystem, and
orchestrator reads use the orchestrator/MCP resource owner. Authority is a
routing/ownership boundary, not a user permission grant. [C10]

App-server wires host, executor, and orchestrator providers. Orchestrator
skills are exposed through bounded `codex_apps` MCP resources; executor skill
catalogs are bound to selected roots/environments. Other Codex surfaces need
not have the same provider set. [C10]

Feature `skill_search` is **stable and default-on**, but at this revision it
enables shadow selection/metrics rather than live prompt retrieval. The
provider trait has a `search` method, but host, executor, and orchestrator
implementations return empty results, and a model-facing `skills.search` tool
was **not found in the investigated scope**; only list/read tool sources were
present. The bounded audit covered `core-skills/src` and `ext/skills/src` for
`SkillSearch`, provider `search`, and tool registrations. [C10, C12, A5]

Extensions also register tools, prompt fragments, lifecycle hooks, token-usage
contributors, and event sinks. Hooks are **stable and default-on** and include
pre/post tool, permission request, pre/post compact, session, prompt, subagent,
and stop events. [C10, C12, C14]

## 10. Permissions, sandboxing, and trust

Approval and sandboxing are orthogonal. `AskForApproval` selects when a user or
reviewer is consulted: `UnlessTrusted`, default `OnRequest`, fine-grained
`Granular`, or `Never`. `PermissionProfile` selects managed filesystem/network
policy, disabled sandboxing, or an external sandbox. Built-in profiles include
read-only, workspace-write, and danger-full-access. [C11]

Managed filesystem policy is split into ordered read/write/deny entries plus
special paths; network is restricted or enabled. More-specific entries win,
with deny precedence on equally specific conflicts. Protected workspace
metadata such as `.git`, `.agents`, and `.codex` remains guarded unless
explicitly granted. Invalid deny globs fail closed in runtime matching. [C11]

Shell/apply-patch execution derives an approval requirement, consults exec
policy and cached session approvals, optionally requests a decision, computes
effective additional permissions, then selects and transforms a sandbox
attempt. Denied-read policy prevents unsandboxed escalation because bypassing
would silently broaden reads. Approval-for-session caches are live session
state; Turn and Session permission grants are recorded separately. [C11]

`exec_permission_approvals` and `request_permissions_tool` are both
**under-development and default-off**. When enabled and allowed by approval
policy, `request_permissions` emits a structured request, intersects the reply
with the requested profile, and records a Turn- or Session-scoped grant. Under
`Never`, or granular policy that disallows this flow, it returns an empty
grant. [C11, C12]

Platform enforcement differs: repository documentation says macOS uses
Seatbelt, Linux selects legacy Landlock only when semantics round-trip and
otherwise uses bubblewrap, and Windows uses elevated or restricted-token
backends with fail-closed constraints. Exec-server receives the native command
plus canonical permission context and enforces its side of process/filesystem
sandboxing; the orchestrator intentionally does not wrap a remote command in a
host sandbox. [C9, C11, D1]

Subagents inherit resolved approval and permission settings at spawn time, but
remain separate Threads/Sessions. Skill authority does not bypass sandbox or
approval policy, and shared filesystem visibility is not an authorization
grant. [C5, C10, C11]

## 11. Budgets, cancellation, retry, and failures

Token usage and provider rate-limit snapshots are tracked in Session state and
emitted as events. Context-window limits drive compaction. Provider metadata
defaults to four HTTP request retries, five stream reconnection attempts, and a
300-second stream idle timeout, with configurable bounded maxima. Responses
WebSocket failures can fall back to HTTP for the remainder of the Session.
[C12, C13]

`token_budget` and `rollout_budget` are separate **under-development,
default-off** features. Token budget controls context-window reminders and
compaction guidance. Rollout budget is shared through `AgentControl` across a
root agent tree, records usage, and injects reminders against a configured
limit. Neither default supplies a mandatory durable Workflow budget. [C12]

Turn cancellation uses hierarchical Tokio `CancellationToken`s. Interrupting
or replacing a Session task cancels tool/model work, clears pending
elicitations, records a model-visible interrupted marker when configured,
flushes the rollout, and emits `TurnAborted`. Tool runtimes either abort
immediately or await their teardown contract. Individual subagents are
interrupted explicitly; legacy tree shutdown is a distinct operation. [C2,
C3, C5]

Failures are differentiated among model-visible tool errors, fatal Turn errors,
retryable provider stream failures, sandbox denial, approval denial/abort,
budget errors, environment disconnects, and internal agent death. Events carry
warnings/errors and stream-retry progress so frontends need not infer all
states from final text. [C3, C9, C14]

## 12. Artifacts, events, and observability

`Event { id, msg }` is the core outward stream. `EventMsg` covers Turn and item
lifecycle, raw Responses items, messages/reasoning deltas, tool begin/end and
output, approval/permission/user-input requests, plan and goal updates,
compaction, token counts, environment connection state, patches/diffs,
subagents, hooks, warnings, errors, and retries. App-server maps these into its
v2 protocol and watches Thread state. [C14]

Rollout persistence is event-sourced enough for transcript reconstruction but
is not a generic event-log API: `RolloutItem` selectively stores session meta,
response items, event messages, Turn context, world state, compaction, and
inter-agent communication. OpenTelemetry/tracing, session telemetry, tool-call
timing, provider traces, and analytics supplement user-facing events. [C8,
C14]

A general typed Artifact entity with producer, status, lineage, validation,
and completion contract was **not found in the investigated scope** of core,
protocol, app-server, exec-server, and extensions. Image generation is a
narrow exception: its extension persists generated bytes at an
extension-owned artifact path and returns context/output hints. Files changed by
tools, terminal output, plans, final messages, and subagent completion are not
automatically promoted into a common Artifact model. [C15, A6]

## 13. Provider boundary

Codex has a runtime `ModelProvider` trait owning provider info, auth/account
state, capability upper bounds, model catalog construction, preferred helper
models, API error mapping, and runtime base URL. Built-ins include a configured
provider and an Amazon Bedrock provider. `ModelProviderInfo` permits custom
base URL, environment/command/bearer/AWS auth, query parameters, headers,
retry limits, timeouts, and WebSocket support. [C13]

This is **endpoint-configurable but not wire-neutral**. `WireApi` has exactly
one accepted variant, `Responses`; `wire_api = "chat"` is rejected. The core
client is explicitly a Responses client, builds `/responses`,
`/responses/compact`, memory, and realtime calls, and retains Responses-specific
IDs, request items, streaming events, sticky Turn state, and optional WebSocket
incremental state. Amazon Bedrock is integrated through an OpenAI-compatible
Responses/Mantle boundary rather than a distinct message protocol. [C13]

Therefore a service exposing a compatible Responses endpoint is configurable,
but a native Anthropic Messages, Gemini, or arbitrary provider wire adapter is
**not found in the investigated scope**. Supporting one would require more than
supplying a URL or implementing auth; the request/response/tool/stream and
compaction contracts are Responses-shaped. [A7, C13]

Provider capability flags can bound namespace tools, image generation, and web
search, while model metadata controls context window, reasoning, parallel-tool
support, and other request details. These are useful seams, but they do not
erase the wire-level coupling. [C13]

## 14. Strengths for Rollshot

- Thread, Session, Turn, Step, internal task, Agent Run, compaction, Memory,
  Goal, and background terminal are implemented as distinct concepts rather
  than a single overloaded run record.
- Append-only rollout history plus compaction checkpoints and reconstruction
  provide strong conversational continuity while keeping Codex Thread resume
  separate from the bounded live-process recovery owned by exec-server.
- Approval policy, permission profile, platform sandboxing, exec policy,
  additional grants, environment authority, and skill authority form explicit
  trust boundaries.
- Multi-agent v1 and v2 offer real child Threads, bounded concurrency,
  history-fork choices, messaging, interruption, persisted topology, and
  observability.
- Tool dispatch has typed registrations, per-handler parallel admission,
  cancellation, lifecycle events, and model-visible failure semantics.
- The execution-environment/exec-server split supports local and remote
  process/filesystem ownership without mixing it into the model-provider layer.
- Skills preserve authority and opaque resource identity across host,
  executor, and orchestrator owners.

## 15. Mismatches and risks

- A durable Task/Workflow/Job/Artifact foundation was **not found in the
  investigated scope** defined in [A1] and [A6]. Plan checklist, Goal, internal
  task, Agent Run, and background terminal cannot substitute for one another.
- Provider configuration breadth can be mistaken for provider neutrality;
  the wire contract is Responses-only.
- Codex Thread resume is transcript reconstruction, not continuation of
  approvals, tool futures, provider streams, or process handles. Exec-server
  separately provides TTL-bounded live-session/process recovery; treating one
  as the other would overstate durability.
- Default-on v1 and default-off v2 have materially different addressing,
  mailbox, fork, persistence, and residency semantics; “Codex multi-agent” is
  not one stable shape.
- Memory is stable-labelled but default-off, app-server-oriented, state-DB and
  filesystem dependent, and driven by internal model calls. It should not be
  assumed available on all surfaces.
- Skill search's stable/default-on feature name overstates current behavior:
  inspected providers return empty search results and selection is shadow-only.
- Fine-grained exec permission approvals and the request-permissions tool are
  under development and default-off.
- Sandbox enforcement is platform- and environment-dependent; static policy
  equivalence does not replace runtime verification on macOS, Linux, Windows,
  and remote exec-server hosts.
- The graph had no coverage for this ignored checkout, increasing the chance
  that broad textual searches miss dynamically generated or unusually named
  integration points. [A0]

## 16. Unresolved questions

1. Which surfaces besides app-server install the same goal, memory, skills,
   image-generation, and executor-provider extensions in production builds?
2. What migration contract will apply when multi-agent v2 becomes default, in
   particular for v1 depth limits, history filtering, and persisted children?
3. Is v2's lack of a maximum-depth check in the inspected handler intentional,
   enforced elsewhere operationally, or temporary?
4. What guarantees do non-local `ThreadStore` implementations make for
   atomicity, ordering, leases, and concurrent resume?
5. In deployed rendezvous services, which disconnect and registry-delay
   distributions fit inside the exec-server's 25-second client retry and
   30-second detached-session TTL, and how often do bounded output retention or
   sequence gaps make a nominally recoverable process unrecoverable?
6. Will skill provider search become a model-facing retrieval tool or alter
   prompt selection rather than remaining shadow metrics?
7. Is the default-off Memory pipeline intended to become a cross-surface
   contract, and what privacy/deletion guarantees will be normative?
8. Would a future provider interface accept non-Responses wire protocols, or
   is Responses compatibility an intentional permanent boundary?
9. Is a first-class Task/Workflow/Artifact model planned, or are external
   orchestration layers expected to own those concepts?

## 17. Evidence index

Primary source evidence:

- **[C1] Revision:** `git -C learn-projects/codex show -s` at
  `4a443994bd12f49f2f08b21a2f224d9d42b9e734`.
- **[C2] Core lifecycle:** `codex-rs/core/src/codex_thread.rs`,
  `session/session.rs`, `session/mod.rs`, `session/handlers.rs`,
  `tasks/mod.rs`, `state/session.rs`, and `state/turn.rs`.
- **[C3] Turn/tool loop:** `codex-rs/core/src/session/turn.rs`,
  `tools/parallel.rs`, `tools/router.rs`, and `tools/registry.rs`.
- **[C4] Plans and goals:** `codex-rs/core/src/tools/handlers/plan.rs`,
  `protocol/src/plan_tool.rs`, `ext/goal/src/tool.rs`, and
  `app-server/src/extensions.rs`.
- **[C5] Delegation:** `codex-rs/core/src/agent/control.rs`,
  `agent/control/spawn.rs`, and `tools/handlers/multi_agents*`; plus
  `ext/agent/src/lib.rs` for `AgentRun`.
- **[C6] Compaction:** `codex-rs/core/src/compact.rs`,
  `compact_remote.rs`, `compact_remote_v2.rs`, and
  `compact_remote_request.rs`.
- **[C7] Memory:** `codex-rs/memories/README.md`, `ext/memories/src`, and the
  app-server extension installation path.
- **[C8] Persistence:** `codex-rs/thread-store/src`,
  `thread-store/README.md`, `core/src/session/rollout_reconstruction.rs`, and
  `core/src/thread_manager.rs`.
- **[C9] Execution and recovery:**
  `codex-rs/exec-server-protocol/src/protocol.rs`,
  `exec-server/src/server/session_registry.rs`, `server/handler.rs`,
  `client_recovery.rs`, `client_transport.rs`, `client.rs`,
  `local_process.rs`, `relay.rs`, and `noise_relay`; plus
  `core/src/environment_selection.rs`, `session/turn_context.rs`, and
  `tools/sandboxing.rs`.
- **[C10] Skills:** `codex-rs/core-skills/src`, `ext/skills/src`, and
  `app-server/src/extensions.rs`.
- **[C11] Permissions/sandbox:** `codex-rs/protocol/src/permissions.rs`,
  `protocol/src/models.rs`, `protocol/src/protocol.rs`,
  `core/src/tools/sandboxing.rs`, `tools/handlers/request_permissions.rs`, and
  `core/src/session/mod.rs`.
- **[C12] Feature status:** `codex-rs/features/src/lib.rs` and
  `core/src/config/mod.rs`.
- **[C13] Provider boundary:** `codex-rs/model-provider/src/provider.rs`,
  `model-provider-info/src/lib.rs`, and `core/src/client.rs`.
- **[C14] Events/observability:** `codex-rs/protocol/src/protocol.rs`,
  `protocol/src/items.rs`, app-server event mapping, and tool lifecycle/trace
  sources.
- **[C15] Narrow artifact implementation:**
  `codex-rs/ext/image-generation/src/artifact.rs` and `tool.rs`.

Test evidence inspected but not executed:

- Core session/turn reconstruction and cancellation tests under
  `codex-rs/core/src/session/*_tests.rs` and `core/tests/suite`.
- Agent control, residency, spawn, wait, and v2 handler tests under
  `codex-rs/core/src/agent` and `tools/handlers/multi_agents*`.
- Compaction suites under `codex-rs/core/tests/suite/compact*.rs`.
- Permission, sandbox, guardian, and request-permissions tests under
  `codex-rs/core/src` and `core/tests`.
- **[T5] Exec-server recovery:**
  `codex-rs/exec-server/tests/process.rs` (including
  `exec_server_resumes_detached_session_without_killing_processes`),
  `src/server/handler/tests.rs`, `server/processor.rs` tests,
  `src/client_recovery_tests.rs`, and relay/noise-relay tests. These tests were
  inspected, not executed by this research.
- Other exec-server filesystem, transport, environment, and capability tests
  under `codex-rs/exec-server/tests` and `src/*_tests.rs`.
- Provider-info and model-provider tests under their respective crates.

Repository documentation evidence:

- **[D1]** `codex-rs/core/README.md` platform sandbox matrix.
- **[D2]** `codex-rs/thread-store/README.md` persistence boundary.
- **[D3]** `codex-rs/memories/README.md` pipeline overview.
- **[D4] Documentation conflict:** `codex-rs/exec-server/README.md` describes
  the wire API, but its statement that websocket close terminates remaining
  managed processes is stale at the pinned revision. `SessionRegistry` source
  and [T5] instead retain detached process state for the bounded TTL; source is
  authoritative for this profile.

Bounded negative/limitation audits:

- **[A0] Graph coverage:** code-review-graph minimal context for
  `learn-projects/codex` returned `0 nodes, 0 edges across 0 files`; direct
  source inspection was required.
- **[A1] Domain models:** searched `core/src`, `protocol/src`,
  `app-server/src`, and `ext` for declarations/names of Task, Todo, Workflow,
  Job, AgentRun, agent run, workflow, DAG, and background job. Hits were the
  internal `TaskKind`, plan checklist, `AgentRun`, comments/tests, and
  background terminals; a durable Workflow/Job domain model was **not found in
  the investigated scope**.
- **[A2] V2 depth:** exact roots were
  `codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs`,
  `codex-rs/core/src/tools/handlers/multi_agents_common.rs`,
  `codex-rs/core/src/agent/control/spawn.rs`, and
  `codex-rs/core/src/agent/registry.rs`. Searched symbols/literals were
  `next_thread_spawn_depth`, `thread_spawn_source`,
  `spawn_agent_with_communication`, `agent_max_depth`, `max_depth`, and
  `exceeds_thread_spawn_depth_limit`. V2 records depth, but a v2 enforcement
  call was **not found in the investigated scope**; config source explicitly
  says the v1 depth setting is ignored by v2.
- **[A3] Compaction variants:** exact roots were `codex-rs/core/src`,
  `codex-rs/protocol/src`, and `codex-rs/thread-store/src`; the case-insensitive
  literal patterns were `mini.?compact`, `micro.?compact`, `cached.?compact`,
  `compaction cache`, and `cache.*compact`. The search returned no matches, so
  those named variants were **not found in the investigated scope**.
- **[A4] Codex Thread/Turn in-flight resume:** inspected exact symbols/roots
  `Session::record_initial_history`, `session/rollout_reconstruction.rs`,
  `ThreadManager` resume/fork functions, `state/turn.rs`,
  `unified_exec/process_manager.rs`, and `thread-store/src`. Durable recreation
  by Thread resume of pending channels/futures/provider streams/process handles
  was **not found in the investigated scope**. This audit intentionally excludes
  the separate live exec-server session-recovery layer in [C9].
- **[A5] Skill search:** searched `core-skills/src` and `ext/skills/src` for
  `SkillSearch`, provider search implementations, and search tool registration;
  providers return empty results and only list/read model tools were found.
- **[A6] Artifacts:** exact roots were `codex-rs/core/src`,
  `codex-rs/protocol/src`, `codex-rs/app-server/src`,
  `codex-rs/exec-server/src`, and `codex-rs/ext`; searched
  declaration/literal terms were `(struct|enum|trait|type) Artifact`,
  `ArtifactId`, `artifact_id`, and the word `artifacts`. The only exact hit was
  unrelated test prose; a
  general Artifact type/ID/store was **not found in the investigated scope**.
  A broader singular `artifact` inspection separately found the narrow
  image-generation path recorded in [C15].
- **[A7] Provider wires:** inspected `WireApi`, provider creation, and the core
  model client; only Responses is accepted, and chat is explicitly rejected.

Confidence: high for source-defined architecture, feature defaults, the
exec-server recovery state machine, and the exact bounded searches at the
pinned revision; medium for behavior inferred jointly from source and tests;
low-to-medium for deployed relay timing, remote registry behavior,
cross-surface wiring, and platform sandbox behavior because none was executed.
The discovered exec-server README/source conflict lowers confidence in
repository prose unless independently matched to pinned source.
