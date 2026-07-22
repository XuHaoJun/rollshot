# Pi system profile

Status: In Progress (Round 1 system profile)

Research date: 2026-07-22 (Asia/Taipei)

Pi revision: `dd6bea41efa8caa7a10fe5a6401676dc5699f83f`

Package versions: `@earendil-works/pi-ai`, `@earendil-works/pi-agent-core`, and `@earendil-works/pi-coding-agent` `0.81.1`

## 1. Scope and reproducibility baseline

This profile statically inspects the local Pi checkout at the revision above. It
separates three layers that are easy to conflate:

1. `pi-ai` owns Pi's unified message/model/provider contracts and provider
   implementations.
2. `pi-agent-core` owns the small model/tool loop and stateful `Agent`. At this
   revision it also exports a newer, separately tested `AgentHarness`, session,
   compaction, and repository layer.
3. `pi-coding-agent` is the current CLI product. It still composes the core
   `Agent` through its own `AgentSession` and `SessionManager`; no
   `AgentHarness` reference was found in `packages/coding-agent/src/core` or its
   public index. [P1, P2, P5, P8]

Status labels in this document mean:

- **core built-in**: implemented by `pi-ai` or `pi-agent-core`;
- **coding-agent built-in**: implemented and wired by the CLI product;
- **harness implemented**: source and tests exist in `pi-agent-core`, but the
  coding agent has not migrated to it in the investigated revision;
- **extension-provided**: possible through an installable TypeScript extension,
  not a built-in Pi lifecycle semantic;
- **planned**: described as unfinished in Pi's own harness document; and
- **not found in investigated scope**: no matching domain abstraction was found
  in the bounded searches recorded in Section 17.

The high-confidence claims below come from source and tests. Documentation is
used for user-facing behavior and security policy, and is labeled separately.
No live provider request or crash/restart experiment was performed, so static
inspection is not runtime proof. The checkout also contains ongoing harness
work whose own document calls lifecycle semantics provisional. [P8]

## 2. Architecture and ownership boundaries

The current coding-agent path is a thin core loop surrounded by a much larger
product/session layer:

```text
pi-coding-agent UI / RPC / print mode
  owns project trust, resources, extensions, built-in tools, settings,
       JSONL SessionManager, retries, compaction and session switching
                     |
                     v
AgentSession  -->  pi-agent-core Agent  -->  runAgentLoop
                     |                       |
                     |                       +-- stream assistant
                     |                       +-- execute tool batch
                     |                       `-- steer/follow-up until stop
                     v
                pi-ai Models / Provider  --> provider API implementation
```

`Agent` owns an in-memory transcript, tool list, one active run, abort
controller, steering/follow-up queues, lifecycle reduction, and listener
settlement. The low-level loop works over `AgentMessage`, transforms it to
`pi-ai` `Message[]` only at the provider call boundary, and receives its stream
function as a dependency. [P2]

`AgentSession` owns coding-product policy around that loop: message persistence,
extension event mediation, tool registry refresh, user-input routing,
auto-retry, compaction, model/thinking settings, and the stronger
`agent_settled` boundary. `SessionManager` owns the current append-only JSONL
conversation tree. [P5, P6]

The exported `AgentHarness` is a second orchestration layer inside
`pi-agent-core`. It owns a generic `Session`, turn snapshots, pending session
writes, structural-operation phases, save points, resources, and harness hooks.
Its code and harness tests implement substantial behavior, but its own design
document records remaining lifecycle hardening, hook work, automatic
compaction, model registry design, and semi-durable recovery as unfinished.
It must not be treated as the coding agent's active integration or as a finished
durable runtime. [P8, T2]

## 3. Conversation, session, and run lifecycle

Pi's terms at this revision are:

| Term | Observed meaning |
|---|---|
| Conversation/transcript | Ordered `AgentMessage[]`: user, assistant, tool-result, and application-defined messages. Coding-agent adds custom, bash, branch-summary, and compaction-summary roles. |
| Session | A coding-agent JSONL file and active tree leaf, or the generic harness `Session`; it can span many low-level runs. |
| Run | One `Agent.prompt()` or `Agent.continue()` lifecycle from `agent_start` to `agent_end`. Only one may be active per `Agent`. |
| Turn | One assistant response plus its complete tool batch and tool-result messages, from `turn_start` to `turn_end`. |
| Settled | Coding-agent state after the run and any automatic retry, compaction/retry, or queued continuation have ended. |
| Workflow | No built-in record identified; a session, run, or compaction summary is not itself a workflow. |

For a normal prompt, `runAgentLoop` copies prior context plus prompt messages,
emits start/message events, requests a streamed assistant message, appends it to
context, executes any tool calls, appends correlated tool results, and asks the
model again while tool work or queued steering remains. When the model stops
without more work, the outer loop drains follow-up messages before emitting
`agent_end`. Provider failures encoded as `stopReason: "error"` and aborts end
that low-level run. [P2, T1]

`Agent.prompt()` rejects if another run is active. `steer()` queues input to be
injected only after the current assistant turn and all of its tool calls finish;
`followUp()` waits until the agent would otherwise stop. Each queue can drain
all messages or one oldest message at a time. Coding-agent persists delivered
messages on `message_end` and exposes queue state to its UI. Its
`agent_settled` event is deliberately later than core `agent_end`. [P2, P5,
T1]

Conversation state is provider-neutral at the Pi type level but not
provider-opaque: assistant messages retain `api`, `provider`, `model`, response
identifiers, thinking signatures, and provider-specific thought signatures
needed for replay. [P3]

## 4. Task, todo, workflow, and background-job model

**Built-in task/workflow state: not found in the investigated scope.** The
bounded search covered `packages/agent/src`, the required core loop test and
harness document, `packages/coding-agent/src/core` excluding vendored renderer
code, and the named coding-agent docs. Hits for “task” or “todo” were ordinary
Promise variable names, skill descriptions, compaction-summary prose, and
extension examples—not a host-owned task identifier/status/dependency model.
[A1]

Pi's structured compaction prompt includes progress headings and Markdown
checkboxes, but that output is a model-authored continuity summary. It is not a
durable task ledger or scheduler. [P7]

The repository's `todo.ts` example demonstrates an **extension-provided**
conversational todo tool. It stores list snapshots in tool-result `details` and
reconstructs state by scanning the active session branch. This gives branch-
correct reminders but no built-in workflow ownership, dependency graph,
execution lease, retry record, or artifact completion contract. [X1]

**Managed background-job state: not found in the investigated scope.** The
exact bounded search for job/process-handle terms found only a comment about
long-running synchronous tool execution. Extensions may start processes,
sockets, watchers, or timers after `session_start`, and the extension guide
requires them to clean those resources up at `session_shutdown`; Pi does not
thereby supply a job ID, durable lifecycle, poll/subscribe API, or reattachment
contract. [A3, P9]

## 5. Subagents and parallel execution

**Built-in child-agent or subagent lifecycle: not found in the investigated
scope.** The source/docs boundary in [A2] contains no core spawn/fork-child API,
parent-child registry, inherited budget, child cancellation tree, or
artifact-based completion semantic.

Pi does have **core built-in parallel tool execution**. `Agent` defaults
`toolExecution` to `"parallel"`. Tool calls are preflighted sequentially; allowed
calls then execute concurrently. `tool_execution_end` follows completion order,
while final tool-result messages are emitted in the assistant's source order.
Global sequential mode, or any tool in the batch declaring
`executionMode: "sequential"`, makes the whole batch sequential. Steering input
is not injected until the complete batch finishes. [P2, T1]

The repository also ships an **uninstalled extension example** named
`subagent`. It registers a tool that spawns a separate `pi` process per child,
uses JSON mode for output, supports one child, a sequential chain, or up to
eight parallel tasks with four concurrent subprocesses, and propagates abort
using `SIGTERM` followed by `SIGKILL`. Each child receives an isolated prompt
and configured tool/model set; parallel model-visible output is capped per
task. These are extension-local policies, not guarantees of the core loop or
coding-agent session manager. [X2]

## 6. Compaction, context continuity, and memory

The low-level loop offers only an optional `transformContext(messages)` hook
before every provider call. The core test proves that transformed messages are
then passed to `convertToLlm`; the loop itself does not choose a threshold or
summary strategy. [P2, T1]

Coding-agent implements manual and automatic compaction. It triggers when
estimated context exceeds `contextWindow - reserveTokens`, or when a provider
reports overflow. It preserves recent turns, asks a model for a structured
summary of older context, persists a `compaction` entry, rebuilds the active
context, and performs at most one compact-and-retry recovery for an overflowing
turn. `/tree` can separately summarize an abandoned branch. Extensions can
cancel or replace either generated summary. [P5, P7, P9]

The active coding-agent `SessionManager.appendCompaction` persists a summary,
`firstKeptEntryId`, token count, optional details/usage, and extension marker.
The newer generic harness additionally supports a materialized `retainedTail`,
making that compaction entry a self-contained context boundary; old entries
remain in storage. The mechanisms compress provider context but do not erase
the transcript or create executable workflow state. [P6, P8, T2]

Memory is principally the active transcript/session tree plus extension-owned
custom entries. Custom entries are excluded from model context unless an
application projector maps them; custom messages do enter context. **A built-in
cross-session user/project semantic-memory service, retrieval policy, expiry,
or deletion API was not found in the same source/docs boundary.** This is a
bounded static conclusion, not a claim that extensions cannot build one. [P6,
A1]

## 7. Persistence, checkpoints, and resume

Coding-agent sessions are append-only JSONL trees. Entries include messages,
model/thinking changes, compactions, branch summaries, custom extension state,
labels, and session metadata. Each non-header entry has `id`/`parentId`; the
active branch is reconstructed from the current leaf. `/resume`, `pi -r`, and
`--session` reopen a saved conversation, while `/tree`, `/fork`, and `/clone`
create or select alternate conversation branches. Session format versions 1–3
are migrated on load. [P6, D1]

This is **conversation/session resume**, not interrupted-run resume. No durable
active model request, in-flight tool call, steering/follow-up queue, retry
timer, extension process handle, or managed job record was found in the active
coding-agent session schema. Labels named “checkpoint” are user bookmarks;
compaction entries are context reconstruction boundaries. Neither is a typed
approval/review checkpoint that gates later workflow execution. [D1, A3]

The generic harness persists leaf changes and queued session writes at save
points, and tests reopening its JSONL storage. However, its own document labels
semi-durable harness/session recovery as a planned spike, explicitly noting
that provider streams are not resumable and that unfinished tool calls are
unsafe to retry without idempotency declarations. [P8, T2]

## 8. Tools and scheduling

An `AgentTool` has a name, description, TypeBox parameter schema, label,
execution callback, optional argument compatibility transform, optional
per-tool execution mode, update callback, optional usage, and a `terminate`
hint. Arguments are schema-validated after `prepareArguments`. Unknown tools,
validation failures, blocked pre-hooks, aborts, and thrown tool errors become
error tool results instead of escaping the batch. [P2]

Tool availability is the `AgentContext.tools` snapshot for a provider request.
Coding-agent maintains a larger registry and active-tool selection, builds its
system prompt from active tools, and refreshes model/thinking/tools between
turns. Its built-in tools are `read`, `bash`, `edit`, `write`, `grep`, `find`,
and `ls`; extensions can register or replace tools. [P5, P10]

Hooks separate several concerns but not with a product permission type:
`tool_call` can mutate or block validated arguments, `tool_result` can replace
result fields, and context/provider hooks can rewrite outgoing context,
headers, or payloads. Mutated arguments are not revalidated. [P9, T1]

Parallel tools create real write-race risk. Built-in `edit` and `write` use a
per-file mutation queue, and extension documentation tells custom mutating
tools to join it. That queue serializes the same canonical target path, not all
side effects or arbitrary cross-file dependencies. [P9, P10]

## 9. Skills and extensions

Skills are instruction/resource packages, not executable runtime plugins.
Coding-agent discovers global, trusted-project, package, settings, and explicit
CLI paths; parses `SKILL.md` frontmatter; puts only name, description, and path
in the system prompt; and relies on the model's `read` tool for full on-demand
loading. `/skill:name` explicitly expands a skill body into the user prompt.
`disable-model-invocation` hides a skill from discovery text but keeps explicit
invocation. Missing descriptions reject a skill; most other validation failures
warn and load. [P11, D2]

The `allowed-tools` skill field is documented as experimental. The inspected
skill loader records no corresponding authorization field on its `Skill`
result, so it must not be described here as enforced permission. [P11, D2]

Extensions are executable TypeScript modules loaded through `jiti`. They can
register tools, commands, providers, resources, renderers, flags and UI; observe
or mutate lifecycle events; persist custom session entries; customize
compaction; and start external resources. Project-local extensions load only
after project trust, but global, explicit CLI, and trusted extension code runs
with the Pi process's full authority. [P9, P12]

The generic `AgentHarness` has typed event/result hooks and generic resources,
skills, prompt templates, and tools, but its own documentation says a generic
hook/provenance system and safe session facade still have unfinished design and
implementation work. [P8]

## 10. Permissions, sandboxing, and trust

Pi's documented built-in security boundary is project-resource trust, not tool
authorization. The closest saved directory decision controls whether project
settings, extensions, skills, prompts, themes, and packages load; noninteractive
modes use the configured default or CLI trust override. `AGENTS.md` and
`CLAUDE.md` context can still load regardless of project trust unless context
loading is disabled. [D3]

Pi explicitly has no built-in sandbox. Built-in tools and extensions run with
the operating-system permissions of the user who launched Pi. The security
guide recommends a container, VM, micro-VM, remote sandbox, or other OS policy
boundary for untrusted or unattended work. [D3]

An extension can implement a confirmation gate by blocking `tool_call`, and
the example subagent extension confirms before using project-local agent
prompts. Those are optional extension policies. **A core approval cache,
capability grant, filesystem/network authority object, or fail-closed reconnect
policy was not found in the investigated scope.** This observation is not a
recommendation that Rollshot should copy Pi's trust model; it is input to the
later permissions comparison. [P9, X2, A3]

## 11. Budgets, cancellation, retry, and failures

Pi records provider and nested-tool token usage and cost and can report session
statistics, but **a finite run budget for tokens, cost, turns, tool calls, wall
time, child agents, jobs, or artifacts was not found in the investigated
core/coding-agent boundary**. The core loop has no maximum-turn parameter; it
continues while tool calls or queued input require another turn. [P2, P5, A1]

Cancellation is an `AbortSignal` shared with provider streaming and tool
execution. `Agent.abort()` signals the active run. Coding-agent separately
tracks abort controllers for retries, compaction, branch summaries, and user
bash; its `abort()` cancels retry plus the agent and waits for settlement.
Tools and extension hooks must honor the passed signal themselves. [P2, P5,
P9]

Provider request options expose timeout, provider/SDK retry count, and maximum
server-requested retry delay. Coding-agent defaults agent-level transient-error
retry to three attempts with exponential backoff and defaults provider retries
to zero. Context overflow goes through compaction rather than ordinary retry,
with only one compact-and-retry recovery attempt. Compaction and branch-summary
model calls have their own observable retry callbacks. [P3, P5, D4]

Failure is represented mainly through assistant `stopReason`/`errorMessage`,
error tool results, thrown high-level mutation errors, and lifecycle events.
There is no common typed terminal taxonomy comparable to Rollshot's current
run terminal states. [P2, P3, P8]

## 12. Artifacts, events, and observability

Core events cover agent, turn, message, and tool-execution start/update/end.
Coding-agent adds queue updates, settled state, compaction, auto-retry,
summarization retry, model/thinking changes, session switches, extension
errors, and provider request/response hooks. Event listeners are awaited by
core `Agent`; `agent_end` is the final loop event but idle settlement waits for
its listeners. [P2, P5, P9]

Session entries and tool-result `details` provide an extensible event/state
record, and session statistics aggregate messages, tool calls, usage, and cost.
Extensions can add renderers and custom entries; separate extension/resource
metadata can carry source information. The system does not provide a built-in
typed artifact registry, expected-artifact completion contract, review
decision, revision graph, or artifact provenance record in the investigated
scope. Ordinary files produced by tools remain ambient filesystem outputs.
[P5, P6, P9, A4]

The subagent and todo examples show how extensions can place structured results
inside tool details, but those shapes and completion rules belong to the
extension. They do not become Pi-wide artifact or task semantics. [X1, X2]

## 13. Provider boundary

`pi-ai` defines the provider-facing contracts: `Context`, `Message`,
`AssistantMessage`, `ToolResultMessage`, `Usage`, `Model`, stream events and
options. A `Provider` owns ID/name, authentication, model discovery, optional
dynamic refresh, and both typed API streaming and simplified streaming.
`Models` resolves credentials/headers/environment, delegates to the provider,
and returns a lazy stream whose failures are encoded as terminal assistant
messages. [P3, P4]

Provider factories can use one API implementation or dispatch by `model.api`.
Pi ships many provider factories while keeping the agent loop dependent only on
a `StreamFn`. Coding-agent extensions can register additional providers and
intercept the final headers/payload/response. [P4, P9]

The boundary is unified and application-owned, but not fully provider-erased:
message history retains provider/API identifiers and opaque thinking/response
signatures for continuity. Model changes are persisted in the session tree;
coding-agent restores the active model and thinking level on session resume.
[P3, P6]

## 14. Strengths for Rollshot

These are preliminary inferences from the cited evidence for later comparison,
not selections or recommendations:

- A genuinely small loop with an injected stream function keeps provider
  dispatch outside loop control flow. [P2, P4]
- Tool-call/result correlation and source-order persistence remain deterministic
  even when execution completion is parallel. [P2, T1]
- Steering and follow-up are separate, precisely timed input queues. [P2, T1]
- JSONL conversation trees preserve alternate histories without rewriting old
  entries, while compaction changes context projection rather than deleting
  stored history. [P6, D1]
- Skill metadata-first disclosure is simple, inspectable, and explicit about
  project trust. [P11, D2]
- The distinction between `agent_end` and product-level settled state prevents
  integrations from declaring completion before automatic continuation ends.
  [P5, P9]
- Extension examples make optional policies visible as examples instead of
  pretending that todo or subagent semantics are inherent in the core. [X1,
  X2]

## 15. Mismatches and risks

These are preliminary fit/risk inferences from the cited evidence:

- Pi's trust boundary does not supply the product-owned authority and approval
  model a privacy-sensitive screenshot application would require. This is an
  observed mismatch, not yet a Rollshot design recommendation. [D3]
- The core loop has no finite multidimensional run budget or typed terminal
  outcome, and the session schema does not durably represent active work. [P2,
  P6]
- Conversation resume can be mistaken for workflow recovery; provider streams,
  in-flight tools, queues, and external processes are not reconstructed. [P8,
  A3]
- Parallel tools are efficient but make side-effect safety a tool/extension
  responsibility. The per-file queue cannot express general dependencies or
  transactional multi-file work. [P2, P9]
- Extensions are maximally powerful executable code. Skill instructions can
  also induce arbitrary tool use, while `allowed-tools` is not evidenced as an
  enforced grant in the inspected loader. [D2, D3, P11]
- The generic `AgentHarness` and current coding-agent `AgentSession` duplicate
  orchestration/session concerns during an incomplete migration, so a consumer
  must choose evidence from the actually integrated path rather than combine
  their strongest features into a fictional whole. [P5, P8]
- Subagent/todo examples are useful demonstrations but their state, security,
  output caps, concurrency and recovery policies have no core compatibility
  guarantee. [X1, X2]

## 16. Unresolved questions

1. Will `pi-coding-agent` migrate to `AgentHarness`, and which existing
   `AgentSession` retry, compaction, extension, and settlement semantics will
   survive that migration?
2. What crash consistency guarantees are intended for partially appended JSONL
   entries, and will the generic storage layer replace the current synchronous
   `SessionManager` path?
3. Which generic harness hook, provenance, and session-facade semantics will be
   implemented rather than remaining design notes?
4. Should a future Pi task/job layer exist at all, or are todos, subagents and
   background resources deliberately left to extensions?
5. How should provider-specific response/thinking state be handled if a durable
   session resumes under a different provider or model?
6. Which tool side effects, if any, will declare idempotency or retry safety for
   future interrupted-operation recovery?
7. Runtime verification is still needed for process crashes during persistence,
   abort races, extension hook failure, and subagent cleanup; this profile makes
   no runtime claims about those cases.

## 17. Evidence index

### Source, tests, and documentation inspected

| ID | Type | Status | Evidence |
|---|---|---|---|
| P1 | Source/metadata | Implemented | `learn-projects/pi/packages/{ai,agent,coding-agent}/package.json`; Pi Git revision and package boundaries. |
| P2 | Source | Core built-in | `packages/agent/src/agent-loop.ts` (`runLoop`, `streamAssistantResponse`, `executeToolCalls*`), `agent.ts` (`Agent`, queues/lifecycle), `types.ts` (`AgentLoopConfig`, `AgentTool`, events), `stream-fn.ts`. |
| T1 | Tests | Core built-in | `packages/agent/test/agent-loop.test.ts`: context transform, tool-call continuity, truncated arguments, parallel completion/source ordering, steering timing, sequential override, next-turn refresh, termination, continuation. |
| P3 | Source | Core built-in | `packages/ai/src/types.ts`: message/content/tool/result/usage/stream contracts and provider-bearing assistant state. |
| P4 | Source | Core built-in | `packages/ai/src/models.ts`: `Provider`, `Models`, `ModelsImpl`, `createProvider`, auth application, lazy streaming and dynamic model refresh. |
| P5 | Source | Coding-agent built-in | `packages/coding-agent/src/core/agent-session.ts`: `AgentSession`, persistence event handling, prompt/queue routing, settled lifecycle, compaction, retry, cancellation and statistics. |
| P6 | Source | Coding-agent built-in | `packages/coding-agent/src/core/session-manager.ts`: `SessionManager`, `_persist`, `_appendEntry`, context construction, branching, compaction/custom entries, open/continue/fork. |
| D1 | Official repository docs | Coding-agent built-in | `packages/coding-agent/docs/sessions.md` and `session-format.md`: user-facing storage, tree, resume, format versions and entry semantics. |
| P7 | Source/docs | Coding-agent built-in | `packages/coding-agent/src/core/compaction/{compaction.ts,branch-summarization.ts,utils.ts}` and `docs/compaction.md`: triggers, cut points, summaries, branch continuity and overflow recovery. |
| P8 | Source/docs | Harness implemented plus planned items | `packages/agent/src/harness/agent-harness.ts`, `harness/session/{session.ts,jsonl-storage.ts,jsonl-repo.ts}`, and `packages/agent/docs/agent-harness.md`. The document explicitly separates implemented behavior from planned recovery/hooks/lifecycle work. |
| T2 | Tests | Harness implemented | `packages/agent/test/harness/agent-harness.test.ts`, `session.test.ts`, `compaction.test.ts`, `storage.test.ts`, and `repo.test.ts`: queues, abort, save points, persistence ordering, compaction, branching and reopen behavior. |
| P9 | Source/docs | Coding-agent built-in extension surface | `packages/coding-agent/docs/extensions.md`, `src/core/extensions/{types.ts,runner.ts,loader.ts}`: registration, hooks, session entries, trust, resource lifecycle and error behavior. |
| P10 | Source | Coding-agent built-in | `packages/coding-agent/src/core/tools/{index.ts,file-mutation-queue.ts,read.ts,bash.ts,edit.ts,write.ts,grep.ts,find.ts,ls.ts}`: built-in tools and file-mutation scheduling. |
| P11 | Source | Coding-agent built-in | `packages/coding-agent/src/core/skills.ts`: discovery, validation, collision handling and XML prompt formatting. |
| D2 | Official repository docs | Coding-agent built-in/experimental | `packages/coding-agent/docs/skills.md`: locations, progressive disclosure, commands, trust warning, experimental `allowed-tools`. |
| P12 | Source | Coding-agent built-in trust gate | `packages/coding-agent/src/core/{project-trust.ts,trust-manager.ts,resource-loader.ts}`. |
| D3 | Official repository docs | Policy | `packages/coding-agent/docs/security.md`: project trust boundary, no built-in sandbox, external isolation guidance. |
| D4 | Official repository docs | Coding-agent built-in | `packages/coding-agent/docs/settings.md`, retry settings and defaults. |
| X1 | Example source | Extension-provided, not installed by default | `packages/coding-agent/examples/extensions/todo.ts`: branch-reconstructed todo tool state in tool-result details. |
| X2 | Example source/docs | Extension-provided, not installed by default | `packages/coding-agent/examples/extensions/subagent/{index.ts,README.md,agents.ts}`: subprocess isolation, single/chain/parallel modes, caps, cancellation and project-agent confirmation. |

### Bounded absence searches

| ID | Search boundary and terms | Result and interpretation |
|---|---|---|
| A1 | Case-insensitive `task|tasks|todo|todos` across `packages/agent/src`, required loop test/harness doc, `packages/coding-agent/src/core` (excluding vendored renderer code), and named session/skill/extension/compaction docs. | Hits were natural-language task references, compaction-summary checkboxes, Promise variable names, or extension examples. No built-in task/todo/workflow domain record was found in this scope. |
| A2 | `sub.?agent|child.?agent|spawn.?agent|fork.?agent|agent.?spawn|delegate` across the same built-in boundary, then separately under coding-agent extension examples. | No built-in child-agent lifecycle was found. The separate extension-example search found `examples/extensions/subagent`, which is reported only as extension-provided behavior. |
| A3 | `job|jobs|background job|background process|process handle|job id|job_id`, plus `checkpoint|approval|resume|recovery|reopen|reattach`, across the same boundary with vendored renderers excluded. | No managed-job abstraction was found. Resume hits were session switching; checkpoint hits were compaction reconstruction, labels, or extension examples; unfinished-operation recovery appears only in the harness's planned work. |
| A4 | `artifact|artifacts|provenance|review decision|approval checkpoint` across the same boundary. | Hits were generic message “artifacts,” harness hook/source provenance notes, or extension source metadata. No built-in typed product artifact/review contract was found. |
| A5 | `parallel|parallelism|concurrent|concurrency|Promise.all|worker|queue|queued` across the same boundary. | Implemented hits cover parallel tool calls, per-file mutation queues, model/package refresh, and user-message queues. No built-in parallel task or agent scheduler was found; the separate subagent example supplies its own scheduler. |

All absence statements are limited to these paths and terms. They do not prove
that another Pi package, an uninspected extension, or future revision cannot
provide the capability.
