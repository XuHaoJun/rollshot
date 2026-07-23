# Claude Code system profile

Status: Reviewed (Round 1 system profile)

Research date: 2026-07-22 (Asia/Taipei)

Claude Code source revision: `2ca5ddabfed5f220812ea11f029eda03b21bc4c1`

Revision date: `2026-04-01T09:47:37+08:00`

## 1. Scope and reproducibility baseline

This profile is a static inspection of the pinned local checkout at
`learn-projects/claude-code-source-code`. Rollshot's code-review graph was
queried first, but returned zero nodes and zero edges across zero files for this
ignored learn-project. The investigation therefore fell back to bounded direct
source inspection with `rg`, `sed`, and `git ls-tree`. [C1, A0]

Status labels are deliberately strict:

- **implemented, default**: present and active on the ordinary external path;
- **implemented, feature-gated**: source exists, but activation requires an
  experiment, environment, flag, account, or build-flavor gate;
- **hidden/unavailable source**: a compiled callsite or type reference exists,
  but the referenced implementation is absent from the pinned tree;
- **disabled**: source explicitly selects an off/default-false path; and
- **roadmap-only**: a source comment explicitly describes future integration.

The strongest claims below come from TypeScript source. Source-map payloads,
README prose, and third-party reverse-engineering commentary were not used as
proof. No provider request, crash/restart exercise, remote CCR session, tmux or
iTerm team, permission dialog, or context-overflow run was executed. Defaults
controlled by GrowthBook can change server-side, and this external source tree
does not contain every ant/internal or bundle-gated module. In particular, the
context-collapse, reactive-compact, cached-microcompact, history-snip, and MCP
skill implementations referenced by visible callsites are absent. Those
limitations make feature availability and hidden algorithms lower confidence
than the visible ownership and data-model claims. [C1, A2]

## 2. Architecture and ownership boundaries

Claude Code's orchestration is a composition of session, query, task, tool,
skill, compact, and persistence services rather than one durable Agent object:

```text
REPL / SDK / bridge
        |
        v
AppState or QueryEngine ---- conversation messages ----> query() model loop
        |                            |                         |
        |                            |                         +-- tools + hooks
        |                            +-- compact/microcompact  +-- permissions
        |                                                      `-- provider API
        +-- root Task registry
        |     +-- local shell / local agent
        |     +-- remote agent / teammate
        |     `-- task output files + notifications
        +-- Task/Todo presentation state
        `-- MCP, skills, settings, memory attachments

sessionStorage ---- JSONL transcript + agent/remote sidecars
memdir ------------ persistent MEMORY.md and topic files
bridge ------------ remote-control environment/session transport
```

`QueryEngine` owns a reusable headless conversation store and submits turns to
`query()`. The interactive product supplies equivalent state through React
`AppState`. `ToolUseContext` is the dependency boundary passed into tools: it
carries messages, abort state, permission state, available tools/MCP clients,
agent definitions, caches, and state mutators. Nested agents receive an
isolated context, but `setAppStateForTasks` deliberately reaches the root task
registry so background infrastructure remains visible. [C2, C5]

Runtime Task implementations own process/agent-specific lifecycle and output.
The shared framework owns registration, updates, terminal eviction, and SDK
events. Transcript storage, memory files, Task-list JSON, team mailboxes, and
remote-control state are separate persistence domains; none is a universal
workflow or artifact store. [C3, C4, C10, C11]

## 3. Conversation, session, and run lifecycle

The important terms at this revision are:

| Term | Observed meaning |
|---|---|
| Conversation | Ordered messages owned by interactive AppState or a headless `QueryEngine`; model context is a projection of this history. |
| Session | A session ID plus persisted JSONL transcript and live application/query state; it can span many user turns. |
| Turn/query | One invocation of `query()` that may sample repeatedly and execute multiple tool batches until it yields a terminal result. |
| Task (runtime) | A root-registry entry for a shell, local agent, remote agent, teammate, or other backgroundable activity. |
| Task (work ledger) | A separate file-backed record with subject, owner, dependencies, and `pending`/`in_progress`/`completed` status. |
| Todo | A separate flat checklist held in session state and reconstructed from `TodoWrite` transcript calls on the applicable path. |
| Agent Run | A named durable `AgentRun` domain abstraction was **not found in the investigated scope**; local and remote agent execution is represented by Task state plus transcript/session data. [A1] |
| Compact | A context-replacement operation and boundary message; not a Task, Session, or Memory record. |
| Memory | Persistent auto-memory files, distinct from transcript history and compaction summaries. |
| Artifact | A general typed Artifact record was **not found in the investigated scope**; output paths and files are task/tool-specific resources. [A1] |

`QueryEngine` is reusable across submissions and keeps mutable messages, usage,
content-replacement state, and permission-denial state. It records user,
assistant, and compact-boundary messages to JSONL when persistence is enabled.
After a compact boundary, the headless in-memory store can discard the old
prefix for garbage collection while the persisted transcript retains the
boundary needed for reconstruction. The source calls future interactive-REPL
use of `QueryEngine` a future phase, so that particular unification is
**roadmap-only**, not the current REPL ownership model. [C2]

The provider loop streams assistant output, validates and schedules tools,
emits progress/results, and may rebuild messages after compaction or context
management. Turn-local permission requests and abort controllers are live
state; JSONL persistence does not imply that an interrupted tool call resumes
at its instruction pointer. [C2, C12]

## 4. Task, todo, workflow, and background-job model

Claude Code has three distinct task-like systems.

First, the runtime Task framework is **implemented, default**. `TaskType`
declares `local_bash`, `local_agent`, `remote_agent`,
`in_process_teammate`, `local_workflow`, `monitor_mcp`, and `dream`.
`TaskStatus` is `pending`, `running`, `completed`, `failed`, or `killed`;
terminal states are completed/failed/killed. `TaskHandle` carries the task ID
and optional cleanup callback. Implementations register in the root
`AppState.tasks` map and may expose `kill`. Background classification derives
from pending/running state unless a Task is explicitly foreground. Output is
append-only per-task disk data under a session-scoped project temp directory,
polled as deltas and capped at 5 GB. Terminal notification and eviction remove
live registry state, not necessarily the output file. [C3]

`local_workflow` and `monitor_mcp` are type and UI-label declarations whose
implementation modules were **not found in the investigated scope** at the
pinned tree. They are therefore not proof of a durable Workflow engine or
monitor lifecycle. The exact tree audit is in [A2].

Second, the file-backed Task work ledger is **implemented, default for
interactive sessions**. A record contains ID, subject, description, optional
active form/owner, status, dependency edges (`blocks`/`blockedBy`), and
metadata. One JSON file per task lives under `~/.claude/tasks/<taskListId>/`;
locks, filesystem watches, in-process signals, and a five-second fallback poll
coordinate updates. Task-list identity prefers explicit environment/team
identity and otherwise the session ID. The four tools create, get, list, and
update records. Dependency edges block claiming, while teammates can claim or
be assigned pending work. This is a coordination ledger, not an execution DAG:
no runtime Task is automatically spawned merely because a ledger item becomes
ready. [C4]

Third, `TodoWrite` is a flat checklist with content, active form, and status.
Interactive mode uses Tasks v2 by default and therefore does not use TodoWrite;
noninteractive mode enables Tasks v2 only through
`CLAUDE_CODE_ENABLE_TASKS`. On the legacy path, Todo state lives in AppState
and resume scans transcript tool calls for the latest list. [C4]

A general durable `Workflow`, `Job`, `AgentRun`, or `Artifact` type was **not
found in the investigated scope**. [A1] Runtime shell process handles and CCR
sessions are narrower task-specific resources. Cron/assistant scheduling was
outside the designated foundation roots and must not be inferred as a generic
Workflow engine from this profile.

## 5. Subagents and parallel execution

Local subagents are **implemented, default** through the Agent tool. A spawn
selects an agent definition, builds the worker tool pool using the worker's
permission mode, preloads declared skills and MCP servers, creates an isolated
`ToolUseContext`, and runs a separate message loop. Synchronous agents return a
result directly. Asynchronous agents register `LocalAgentTaskState`, write a
sidechain transcript and metadata, report progress/usage, and notify the parent
on completion. Foreground execution can transition to background; parent
context mutation remains isolated while root Task registration is shared. [C5]

Agent resume is explicit rather than transparent continuation. It loads a
sidechain transcript and metadata, repairs orphaned/unresolved tool-call
fragments, reconstructs content-replacement state, validates worktree metadata
best-effort, rebuilds the agent definition/tool pool, and re-registers the same
agent ID as an asynchronous task. The original spawn permission is not asked
again, but subsequent worker tool calls remain subject to the reconstructed
permission context. [C5]

Agent teams/teammates are **implemented, feature-gated** for external users:
activation requires `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` or
`--agent-teams`, plus a GrowthBook kill switch; ant users are always enabled.
Teammates may run in-process or in tmux/iTerm panes. In-process teammates have
independent message loops, permission modes, abort controllers, full disk
transcripts, capped 50-message UI mirrors, and file-backed mailboxes. They can
remain idle, accept leader messages, claim unblocked Task-list items, compact
their own context, and process cooperative shutdown before forced kill.
Nested teammate creation is rejected because the roster is flat; in-process
teammates may spawn synchronous subagents but not background agents. [C6]

Remote agent isolation has visible implementation but is **build-flavor
gated**: the pinned external Agent-tool branch statically excludes the ant-only
CCR launch block. The underlying remote Task polls a Claude.ai session, keeps a
session URL/log, persists an identity sidecar, and can restore polling after
`--resume` by fetching live remote status. Kill archives the remote session.
Those source semantics do not establish availability in this external build.
[C7]

Tool-call parallelism is separate from agent parallelism. Consecutive tools
whose implementations declare concurrency safety are grouped and run with a
default maximum concurrency of ten; unsafe tools serialize. Streaming execution
allows safe calls to overlap, buffers final results, applies context modifiers
in original tool order, and can cancel sibling calls after a shell failure.
[C12]

## 6. Compaction, context continuity, and memory

Traditional full compaction is **implemented, default** when automatic
compaction is enabled. The effective context window reserves up to 20,000
tokens for the summary; automatic compaction begins roughly 13,000 tokens below
that effective window. `DISABLE_COMPACT`, `DISABLE_AUTO_COMPACT`, or settings
can disable it. A circuit breaker stops repeated attempts after three
consecutive failures. Manual and automatic modes invoke a compact agent with
tools denied; prompt-too-long recovery can drop oldest API-round groups up to
three times. [C8]

The result is a compact boundary, model-authored summary, optional preserved
tail, attachments, hook output, and token metrics. Reconstruction order is
boundary, summary, retained messages, attachments, then hooks. Full compaction
can re-inject recent files, plan state, invoked skill contents, async-agent
status, deferred tool/agent/MCP discovery data, and session-start hooks. It
clears read and microcompact caches but deliberately retains invoked-skill
state. Partial compaction can preserve a prefix or suffix. [C8]

Several context mechanisms must not be conflated:

| Mechanism | Status and semantics |
|---|---|
| Traditional compact | Implemented; summary plus continuity attachments and compact boundary. |
| Session-memory compact | **Implemented, feature-gated/default-false** by `tengu_session_memory` and `tengu_sm_compact`; uses a persistent session-memory summary, preserves a recent tail, and falls back to traditional compact when invalid. |
| Time-based microcompact | Implemented but **disabled by default** (`enabled: false`); after a long cache gap it replaces older tool results before the request. |
| Cached microcompact | **Hidden/unavailable source** behind `CACHED_MICROCOMPACT`; callsites and types exist, but `cachedMicrocompact.ts` is absent. It edits API cache context while leaving local messages intact and later emits a boundary with observed deleted tokens. |
| Reactive compact | **Hidden/unavailable source** behind `REACTIVE_COMPACT`; visible callsites withhold context/media errors and request one reactive retry, but the algorithm module is absent. |
| History snip | **Hidden/unavailable source** behind `HISTORY_SNIP`; visible callsites project a shortened model view and preserve fuller REPL scrollback, but both snip implementation modules are absent. |
| Context collapse | **Hidden/unavailable source** behind `CONTEXT_COLLAPSE`; `query.ts` visibly calls `applyCollapsesIfNeeded` before autocompact to project a model view and possibly commit more collapses, and calls `recoverFromOverflow` on a withheld 413. `src/services/contextCollapse/{index,persist}.ts` are absent, so the projection, commit, staging, and overflow algorithms are not established. |
| API thinking management | Implemented call-level strategy, emitted only when the request has thinking and redact-thinking is inactive. It preserves all thinking by default for that call, or retains one thinking turn when `clearAllThinking` is requested. This is conditional request configuration, not a general external/default clearing capability. |
| API tool-result/use clearing | **Implemented, feature-gated/internal-only**: it returns early for non-ant users and requires explicit `USE_API_CLEAR_TOOL_RESULTS` and/or `USE_API_CLEAR_TOOL_USES` opt-in. The visible defaults are a 180,000-token trigger and 40,000-token target when enabled. |

Auto Memory is also separate. It is persistent Markdown under a validated
memory directory, default-enabled unless bare/remote/config conditions disable
it. `MEMORY.md` is a bounded index (200 lines/25 KB) into topic files organized
as user, feedback, project, or reference memory. A model may retrieve up to
five relevant topic files. Team memory is additionally feature-gated; nightly
dream and extraction paths have their own gates. Compaction may clear in-memory
attachment caches, but it does not turn these files into the compact summary.
[C9]

## 7. Persistence, checkpoints, and resume

The canonical local conversation record is JSONL at the project session path.
Parent links reconstruct a selected conversation leaf and allow branching;
records also carry summaries, content replacements, compact boundaries, and
feature-specific context metadata. Subagent transcripts live under the
session's `subagents/` directory with small metadata sidecars. Raw transcript
loading is bounded at 50 MB. Resume restores message history, file history,
attribution/feature state, applicable Todo state, main agent selection, model,
working directory/worktree, and permission mode when available. [C10]

When `CONTEXT_COLLAPSE` is compiled in, session storage appends ordered
collapse-commit records and last-wins staged-queue snapshots. Both interactive
and CLI resume call the missing `persist.restoreFromEntries` before the first
query so the visible callsite can reconstruct the commit log and staged
snapshot used by projection. This persistence contract is visible, but the
reconstruction and projection algorithms remain **hidden/unavailable source**.
[C2, C10, A2]

Persistence is selective:

- local agent transcripts can be explicitly resumed through the Agent path;
- remote Task identity sidecars are scanned on session resume, live CCR status
  is fetched, and polling is reconstructed for still-running sessions;
- recoverable remote auth/network failures preserve the sidecar for a later
  attempt, while archived/404 sessions remove it; and
- a generic restart-time resurrection routine for local shell Tasks,
  in-process teammate Tasks, or arbitrary Task types was **not found in the
  investigated scope**. The bounded restore audit is [A3].

Task-list JSON and Auto Memory files persist independently of transcript
resume. Task output directories are memoized so `/clear` in the same process
does not orphan currently running output, but that behavior is not evidence of
post-crash process reattachment. [C3, C4, C9]

Remote-control resume is another boundary and is **implemented,
feature-gated**. Availability requires the `BRIDGE_MODE` build feature, a
Claude.ai subscriber OAuth context, and GrowthBook gate `tengu_ccr_bridge`,
whose cached default is false. When entitled, a project
`bridge-pointer.json` stores session/environment IDs and source for four hours.
Perpetual REPL bridge mode can re-register the environment and reconnect a
matching server session; failure clears the pointer and creates a fresh
session. A clean single-session KAIROS shutdown can intentionally leave the
backend environment resumable; fatal or ordinary multi-session shutdown
follows different archive/deregister paths. This is CCR transport reattachment,
not generic local Task recovery. [C11]

## 8. Tools and scheduling

`Tool` is a typed contract with input/output schemas, validation, execution,
permission checking, enablement, concurrency/read-only/destructive/open-world
metadata, interruption behavior, rendering/events, result-size spilling,
deferred loading, hooks, and optional MCP identity. Conservative defaults mark
a tool non-concurrent, non-read-only, and non-destructive unless it opts in.
[C12]

Availability has four stages: a tool can exist in the assembled pool, pass its
`isEnabled` gate, be advertised immediately or deferred behind ToolSearch, and
then be selected by the model. Authorization happens per invocation after
schema validation, tool-specific/general permission evaluation, and applicable
PreToolUse hooks; PostToolUse or PostToolUseFailure hooks observe results.
Connected MCP servers add tools dynamically, and agent definitions may require
matching authenticated MCP tool namespaces. A connected server without tools
does not satisfy that requirement. [C5, C12]

Batch orchestration preserves model call order for context changes even when
safe execution overlaps. Interactive streaming can surface progress before
final ordered results. Abort handling uses child controllers; whether an
interrupt cancels, blocks, or leaves the parent alive is tool-specific. This is
an execution scheduler, not a durable Job scheduler. [C12]

## 9. Skills and extensions

Local/file, plugin, and bundled skills are **implemented, default** and
progressively disclosed. Discovery loads metadata from managed, user, project,
additional, plugin, and bundled sources; `SKILL.md` directories and legacy
commands are supported.
Frontmatter can constrain tools, model, effort, agent, forked context, hooks,
shell expansion, paths, and user/model invocation. Conditional path skills are
held until relevant files activate them. Bundled skills are lazily extracted
to an owner-only per-process directory. [C13]

Invoking an inline skill injects its content and optional context modifier.
Forked-context skills run through an isolated agent. Skill permission rules can
deny, allow, or ask. Plugins may add skills, commands, hooks, agents, and MCP
definitions. [C13]

MCP-provided skills are **hidden/unavailable source** behind the `MCP_SKILLS`
build feature. The gated MCP client callsite checks resource support, calls
`fetchMcpSkillsForClient`, merges returned commands into MCP commands, and the
Skill tool filters entries marked `loadedFrom === 'mcp'`; security callsites
also specify that remote MCP skill Markdown must not execute inline shell.
However, `src/skills/mcpSkills.ts` is absent, so discovery, resource decoding,
and exact availability semantics cannot be verified. MCP tools and ordinary
MCP prompts are separate and remain visible outside this missing module. [C13,
A2]

Invoked-skill state is an in-memory, agent-scoped map. It is retained and
re-injected across full compaction, but general durable persistence of that map
across process restart was **not found in the investigated scope**. Remote
canonical skill search is experimental/feature-gated and should not be treated
as the ordinary local discovery path. [C8, C13]

## 10. Permissions, sandboxing, and trust

Permission context contains the active mode, additional working directories,
allow/deny/ask rules grouped by source, availability of bypass/auto modes, and
flags for noninteractive prompt avoidance or awaited automated checks. Rules
and tool-specific checks return allow, deny, ask, or passthrough decisions with
structured reasons. Hooks and interactive/SDK permission handlers can further
mediate the call. [C14]

Async subagents cannot safely present parent UI prompts, so their isolated
context sets `shouldAvoidPermissionPrompts` and keeps local denial tracking.
Worker pools are assembled from the worker permission mode rather than simply
copying the parent's advertised subset. Agent-type permission syntax can deny
an agent before spawn; required MCP servers are verified against actually
available tools. Teammates independently carry a permission mode. [C5, C6]

Filesystem trust is enforced in several local boundaries rather than one
universal sandbox object: skill discovery defers invocation-time trust,
task-output creation uses no-follow/safe-open behavior, auto-memory path
overrides exclude project settings and validate containment, team-memory paths
reject traversal/symlink escape, and worktree isolation is explicit agent
configuration. This static inspection did not exercise OS sandbox profiles, so
it does not establish platform-specific containment strength. [C3, C9, C13]

## 11. Budgets, cancellation, retry, and failures

Query configuration supports `maxTurns`, `maxBudgetUsd`, and a feature-gated
task/token budget path. Agent definitions can also cap turns. The visible code
does not establish a single hierarchical budget automatically divided among a
parent, all children, tools, and remote sessions; such a unified budget was
**not found in the investigated scope**. [C2, A4]

Cancellation is based on `AbortController` trees plus Task-specific kill.
Foreground children can link to a parent abort; asynchronous background work
may deliberately use an unlinked controller so it survives the spawning turn.
Stopping a local agent aborts it and marks the Task killed; stopping a remote
agent also archives its cloud session; teammate shutdown distinguishes a
cooperative request from forced kill. [C3, C5, C6, C7]

Failure and retry policies are local: compact has bounded overflow retries and
a three-failure circuit breaker; remote polling continues through transient
errors and remote review has a 30-minute timeout; required MCP startup waits up
to 30 seconds; tools render validation, permission, hook, provider, or runtime
errors through their own paths. A general durable workflow retry policy was
**not found in the investigated scope**. [C7, C8, C12, A4]

## 12. Artifacts, events, and observability

The system emits SDK task-started, progress, notification, and termination
events; tool progress/results and hook progress; compact boundaries and compact
hook phases; bridge connectivity state; UI task/team updates; and extensive
structured analytics/telemetry events. Runtime Task notifications can include
result text, usage, worktree path, session URL, or output path depending on the
Task type. [C3, C5, C7, C8, C11, C12]

Durable outputs remain heterogeneous: conversation and subagent JSONL,
Task-list JSON, task log files, remote-agent sidecars, plan files, memory
Markdown, skill files, team mailboxes, and remote session records. A shared
typed Artifact identity, lineage, MIME/schema contract, retention policy, and
handoff API was **not found in the investigated scope**. [A1] For Rollshot,
those file paths are useful evidence channels but should not be mistaken for
artifact lifecycle semantics.

## 13. Provider boundary

The visible main loop and message/tool types are Claude/Anthropic-specific:
they import Anthropic SDK resources, preserve Claude thinking/cache metadata,
and use Claude model resolution and provider context-management strategies.
Subagents share that model stack, while memory relevance selection explicitly
uses a Claude model. This is not a provider-neutral model facade in the
investigated source. [C2, C5, C8, C9, C15]

MCP is a tool/resource extension boundary, not a model-provider abstraction.
Claude.ai CCR/teleport and bridge APIs are remote session/control transports,
not alternate inference providers. Support for a general third-party model
adapter was **not found in the investigated scope**; this claim is bounded to
the named orchestration/provider files, not every product integration. [C11,
C15]

## 14. Strengths for Rollshot

- The root Task registry plus isolated child contexts is a strong pattern for
  keeping background capture/analysis work observable without allowing a child
  to mutate unrelated UI state.
- Task output files, delta polling, explicit terminal notification, and output
  eviction separate high-volume progress from the conversation transcript.
- The distinction between a coordination ledger and runtime activity is useful:
  Rollshot can model review intent separately from execution handles.
- Compaction's continuity attachment inventory makes hidden state explicit:
  plans, invoked skills, async work, discovered tools, and recent files are
  deliberately considered at the context boundary.
- Sidecar-based remote recovery demonstrates a small durable identity record
  with live status fetched from the authoritative remote system.
- Tool metadata and conservative scheduling provide a practical route to safe
  overlap without declaring the whole agent runtime concurrent.

## 15. Mismatches and risks

- Claude Code has several meanings of Task and several context-reduction paths.
  Copying names without copying their boundaries would make Rollshot's model
  ambiguous.
- Most local runtime Task state is process memory. Output persistence and
  transcript persistence do not provide general crash-safe execution recovery.
- Agent teams depend on experimental opt-in and server-side gates; remote-agent
  isolation is unavailable in this external build flavor. Neither is a stable
  portability contract for Rollshot.
- Hidden context-collapse, compact/snip, and MCP-skill modules prevent
  independent verification of algorithms, thresholds, invariants, and defaults
  at the pinned revision.
- Bridge resume is not a baseline external capability: it requires a build
  feature, subscriber authentication, and a default-false server entitlement.
- Permission behavior is distributed across tool checks, rules, hooks, modes,
  UI/SDK handlers, trust checks, and platform mechanisms. A shallow port would
  likely omit a boundary.
- The provider path is Claude-specific, whereas Rollshot's existing agent crate
  owns a provider-neutral facade and explicit bounded-run contracts.
- There is no universal typed Artifact handoff. Rollshot needs stronger image,
  proposal, provenance, and review-decision records than ambient output paths.

## 16. Unresolved questions

1. What are the exact projection, commit, staging, recovery, and rollout
   semantics inside the absent context-collapse implementation?
2. Which runtime/build gate supplies `local_workflow` and `monitor_mcp`, and
   what persistence semantics do those implementations have?
3. Are local shell or in-process teammate Tasks intentionally nonrecoverable
   after process restart, or is recovery implemented outside the investigated
   roots?
4. What global and per-team concurrency limits are enforced by hidden service
   policy, beyond the visible tool-batch limit?
5. How do server-side GrowthBook assignments vary by account, product surface,
   and version for auto compact, microcompact, memory, and teams?
6. Which OS sandbox/profile implementations are selected on macOS, Linux, and
   Windows, and what guarantees are tested end to end?
7. Does internal/ant source expose a provider abstraction or typed artifact
   system absent from the external tree?
8. How does the absent MCP-skill loader authenticate, enumerate, validate, and
   cache remote skill resources, and which MCP capability versions qualify?
9. What are the exact algorithms and rollout defaults inside the absent
   reactive compact, cached microcompact, and history-snip modules?

## 17. Evidence index

Code evidence at revision
`2ca5ddabfed5f220812ea11f029eda03b21bc4c1`:

- **[C1] Baseline:** `src/Task.ts`; repository revision and commit timestamp;
  zero-node code-review-graph result for the learn-project checkout.
- **[C2] Conversation/query:** `src/QueryEngine.ts`, `src/query.ts`,
  `src/Tool.ts`, `src/utils/forkedAgent.ts`.
- **[C3] Runtime tasks:** `src/Task.ts`, `src/tasks/types.ts`,
  `src/utils/task/framework.ts`, `src/utils/task/diskOutput.ts`,
  `src/tasks/stopTask.ts`, `src/tasks/LocalShellTask/LocalShellTask.tsx`.
- **[C4] Task/Todo ledger:** `src/utils/tasks.ts`, `src/hooks/useTasksV2.ts`,
  `src/tools/TaskCreateTool`, `src/tools/TaskGetTool`,
  `src/tools/TaskListTool`, `src/tools/TaskUpdateTool`,
  `src/tools/TodoWriteTool`, `src/utils/todo/types.ts`.
- **[C5] Local agents:** `src/tasks/LocalAgentTask/LocalAgentTask.tsx`,
  `src/tasks/LocalMainSessionTask/LocalMainSessionTask.tsx`,
  `src/tools/AgentTool/runAgent.ts`, `src/tools/AgentTool/resumeAgent.ts`,
  `src/utils/agentContext.ts`, `src/utils/forkedAgent.ts`.
- **[C6] Teams:** `src/utils/agentSwarmsEnabled.ts`,
  `src/tasks/InProcessTeammateTask`, `src/utils/swarm/inProcessRunner.ts`,
  `src/utils/swarm/spawnInProcess.ts`, `src/utils/swarm/backends`,
  `src/utils/teammateMailbox.ts`.
- **[C7] Remote agents:** `src/tasks/RemoteAgentTask/RemoteAgentTask.tsx`,
  `src/tools/AgentTool/AgentTool.tsx`,
  `src/utils/background/remote/remoteSession.ts`,
  `src/utils/sessionStorage.ts`.
- **[C8] Compaction:** `src/services/compact/autoCompact.ts`,
  `src/services/compact/compact.ts`,
  `src/services/compact/sessionMemoryCompact.ts`,
  `src/services/compact/postCompactCleanup.ts`,
  `src/services/compact/microCompact.ts`,
  `src/services/compact/apiMicrocompact.ts`,
  `src/services/compact/timeBasedMCConfig.ts`, `src/query.ts`.
- **[C9] Memory:** `src/memdir/paths.ts`, `src/memdir/memdir.ts`,
  `src/memdir/findRelevantMemories.ts`, `src/memdir/teamMemPaths.ts`.
- **[C10] Session persistence:** `src/utils/sessionStorage.ts`,
  `src/utils/sessionRestore.ts`, `src/bootstrap/state.ts`; visible
  context-collapse commit/snapshot record and restore callsites are included.
- **[C11] Bridge:** `src/bridge/bridgeEnabled.ts`,
  `src/bridge/bridgePointer.ts`,
  `src/bridge/bridgeApi.ts`, `src/bridge/replBridge.ts`,
  `src/bridge/bridgeMain.ts`, `src/bridge/createSession.ts`.
- **[C12] Tools:** `src/Tool.ts`,
  `src/services/tools/toolOrchestration.ts`,
  `src/services/tools/StreamingToolExecutor.ts`,
  `src/services/tools/toolExecution.ts`, `src/services/tools/toolHooks.ts`,
  `src/utils/permissions`.
- **[C13] Skills:** `src/skills/loadSkillsDir.ts`,
  `src/skills/bundledSkills.ts`, `src/skills/mcpSkillBuilders.ts`,
  `src/services/mcp/client.ts`, `src/services/mcp/useManageMCPConnections.ts`,
  `src/tools/SkillTool/SkillTool.ts`, `src/bootstrap/state.ts`.
- **[C14] Permissions:** `src/Tool.ts`, `src/utils/permissions`,
  `src/hooks/toolPermission`, `src/utils/forkedAgent.ts`.
- **[C15] Provider boundary:** `src/query.ts`, `src/QueryEngine.ts`,
  `src/services/compact`, `src/tools/AgentTool`, `src/memdir`.

Bounded audit evidence:

- **[A0] Graph coverage:** `get_minimal_context` against
  `/home/noah/rollshot/learn-projects/claude-code-source-code` returned `0`
  nodes, `0` edges, and `0` files, so direct inspection was required.
- **[A1] Domain declaration absence audit:** `rg` used the exact regex
  `^(?:export\s+(?:default\s+)?|export\s+declare\s+|declare\s+)?(?:abstract\s+)?(?:type|interface|class)\s+(?:Workflow|Job|AgentRun|Artifact)\b`.
  This includes unexported, exported, default-exported, declared, and abstract
  TypeScript type/interface/class forms. The exact roots/files were
  `src/Task.ts`, `src/tasks`, `src/QueryEngine.ts`,
  `src/services/compact`, `src/services/tools`, `src/skills`,
  `src/bootstrap/state.ts`, `src/bridge`, `src/memdir`, `src/Tool.ts`,
  `src/utils/tasks.ts`, `src/hooks/useTasksV2.ts`,
  `src/tools/TaskCreateTool`, `src/tools/TaskGetTool`,
  `src/tools/TaskListTool`, `src/tools/TaskUpdateTool`,
  `src/tools/TodoWriteTool`, `src/tools/AgentTool`,
  `src/utils/swarm`, `src/utils/sessionStorage.ts`,
  `src/utils/sessionRestore.ts`, and `src/query.ts`. It returned no matches.
  The narrow conclusion is that declarations with those four names were **not
  found in the investigated scope**; differently named equivalents remain
  possible, and ordinary word occurrences were not treated as declarations.
- **[A2] Missing implementation audit:** `git ls-tree -r --name-only` at the
  pinned revision was restricted to the literal roots
  `src/services/compact`, `src/services/contextCollapse`, `src/tasks`, and
  `src/skills`. Its exact path regex was
  `^src/services/contextCollapse/(index|persist)\.ts$|^src/services/compact/(reactiveCompact|snipCompact|snipProjection|cachedMicrocompact)\.ts$|^src/tasks/(LocalWorkflowTask|MonitorMcpTask)(\.tsx?|/)|^src/skills/mcpSkills\.ts$`.
  It returned no matches. Visible gated imports, types, and labels were
  separately confirmed in `src/query.ts`, `src/QueryEngine.ts`,
  `src/services/compact/microCompact.ts`, `src/utils/sessionRestore.ts`,
  `src/services/mcp/client.ts`, `src/Task.ts`, and
  `src/tasks/pillLabel.ts`.
- **[A3] Restart recovery audit:** `rg` used the exact regex
  `(?:restore|resume)[A-Za-z]*(?:Task|Agent)|reattach|resurrect|sidecar` over
  the literal roots/files `src/tasks`, `src/utils/task`,
  `src/tools/AgentTool`, `src/utils/sessionRestore.ts`, and
  `src/utils/sessionStorage.ts`. Matches showed explicit local-agent resume and
  remote-agent sidecar restoration, but a generic local Task resurrection
  routine was **not found in the investigated scope**.
- **[A4] Budget/retry audit:** `rg` used the literal-alternation regex
  `maxTurns|maxBudgetUsd|taskBudget|TOKEN_BUDGET|retry|Retry|timeout|Timeout|consecutiveFailures|AbortController|\bkill\b`
  over `src/query.ts`, `src/QueryEngine.ts`, `src/Task.ts`, `src/tasks`,
  `src/tools/AgentTool`, `src/services/compact`, `src/services/tools`, and
  `src/utils/background/remote`. Matches established local turn/cost/task
  budgets and component retry/timeout/cancellation rules, but a named
  hierarchical child-budget policy and general durable workflow-retry policy
  were **not found in the investigated scope**.

Confidence is **high** for visible type distinctions, ownership, persistence
record shapes, and callsites; **medium** for default availability affected by
runtime configuration; and **low** for context-collapse, MCP-skill, compact,
and snip algorithms whose referenced modules are absent, as well as internal/ant
behavior. The evidence is static and revision-bounded.
