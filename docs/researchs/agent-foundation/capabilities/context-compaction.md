# Context compaction and continuity comparison

**Research date:** 2026-07-22 (Asia/Taipei)  
**Status:** Reviewed  
**Umbrella revision:** 1  
**Current Rollshot revision:** `edaf0abe0dd9140e3b22f0fcf73c0ff79e4a2dc6`  
**Reference revisions:** Pi `dd6bea41efa8caa7a10fe5a6401676dc5699f83f`;
oh-my-pi `7b141199d524b859c357fc89654f10b62b9f3df1`; Codex
`4a443994bd12f49f2f08b21a2f224d9d42b9e734`; Claude Code source
`2ca5ddabfed5f220812ea11f029eda03b21bc4c1`.  
**Evidence mode:** static source and test-source inspection. No provider call,
cache experiment, compact/resume run, UI session, or process restart was
executed.

This document compares context-reduction behavior rather than feature names.
It does not select a final Rollshot architecture.

## 1. Rollshot problem statement and workload evidence

Rollshot currently starts a fresh bounded Rig `AgentRun` for each invocation.
Within that run Rig preserves assistant tool calls and correlated results, but
the app neither projects prior `AgentSession` exchanges into the new run nor
persists a compact boundary. A compaction, mini-compaction, Memory service, or
context projection was **not found in the investigated scope** [A:R]. That is
not yet a defect for Smart Redaction: its normal successful outcome is a typed
proposal after a short finite run. [E:R1, W1]

The three workloads create different continuity pressure:

| Workload | Observed boundary | Context requirement this comparison may infer |
|---|---|---|
| Smart Redaction | One finite author/validate/dry-run/submit run, serial tools, typed review terminal, app-owned consent and cancellation. [W1] | Compaction should normally not fire. If it does, loss of the current source generation, validation/dry-run evidence, terminal-tool rule, consent boundary, or pending review handoff is unacceptable. A durable Workflow is not established as necessary. |
| Action Guide | Durable project revision, steps, frames and annotations surround independent bounded caption/visual-proposal calls. [W2] | Model context may be disposable; project revision, selected step/keyframe, proposal and stale-result gate are not. Compaction cannot substitute for project persistence. |
| Deferred brag + Hyperframes | Project inspection, plan/check gates, background render/audio work, optional workers, expected files and explicit pre-render approval. [W3-W6] | A long coordinator may need context reduction, but stage readiness, approval decisions, job handles and expected artifacts must remain durable outside the summary. Artifact-based recovery may be safer than repeatedly summarizing a long transcript. |

## 2. Terminology and non-equivalent concepts

The following labels describe mechanisms, not a maturity ladder:

| Term | Meaning in this comparison | Non-equivalence rule |
|---|---|---|
| **Full compaction** | Replaces a broad model-visible history region with a summary or provider-produced replacement, optionally retaining a tail and continuity attachments. | A persisted old transcript does not mean the old prefix remains model-visible. |
| **Automatic compaction** | Host trigger policy based on estimated/observed tokens or a maintenance point. | `auto` names the trigger, not the summary algorithm. |
| **Reactive compaction** | Recovery after a provider rejects the prompt/media or reports overflow. | Pi's overflow compact-and-retry and Claude's hidden reactive module have different evidence and must not share an inferred algorithm. |
| **Remote compaction** | A remote endpoint or provider performs the reduction. | A generic remote summarizer, `/responses/compact`, and Responses-stream compaction are distinct wire and state contracts. |
| **Mini-/microcompact** | Umbrella research label for a smaller reduction that avoids replacing the whole conversational view. | The label does not make cached edits, time-based tool-result clearing, pruning, snipping or shake equivalent. |
| **Snip** | A shortened model projection while a fuller local/display history may remain. | Claude's implementation is absent at the pinned revision; visible callsites establish ordering and view separation, not selection logic. |
| **Pruning/elision** | Replaces selected tool-result content with placeholders while retaining message structure. | It is not a summary and need not create a compact boundary. |
| **Shake** | oh-my-pi's surgical replacement of selected tool results or large fenced/XML blocks, optionally with recovery artifacts. | It is source-named behavior, not a synonym for microcompact. |
| **Context collapse/projection** | Read-time model view reconstructed from committed/staged reductions while fuller REPL history remains. | Claude's callsites and persistence records are visible, but the algorithm modules are absent. |
| **Provider-native tool clearing** | Request-side provider context-management edits for tool use/result or thinking content. | It may leave local history unchanged and is coupled to provider semantics. |
| **Snapcompact** | oh-my-pi's deterministic image archive of older serialized context plus text edges. | It is neither semantic summarization nor Memory; continuation requires image capability. |
| **Handoff** | Generate a continuation document and start another session. | oh-my-pi handoff does not append `CompactionEntry`; it is session transition, not in-place compaction. |

Three storage concepts remain strictly separate:

1. **Compaction** controls the model-visible projection for a continuation.
2. **Persistence/checkpoint/resume** stores enough canonical state to rebuild a
   session, product task, workflow, job or artifact after a boundary.
3. **Memory** retrieves durable cross-turn or cross-session knowledge selected
   for future relevance.

A compact summary can be persisted without being durable Workflow state. A
JSONL transcript can survive restart without resuming a provider stream or
approval. A Memory file can be injected after compaction without becoming the
compaction record.

## 3. Evidence levels and status labels

- **L3 — source + focused test source:** implementation and assertions were
  inspected, but tests were not executed here.
- **L2 — source:** implementation is visible; runtime behavior is unobserved.
- **L1 — callsite/type only:** a gate, import, call, record shape or event is
  visible, but the implementation module is missing. No algorithm is inferred.
- **L0 — bounded absence:** the mechanism was **not found in the investigated
  scope** defined by an exact root and regex in Section 14.

Status is reported independently as **default**, **gated/default-off**,
**internal-only**, **hidden/missing implementation**, **example-only**, or
**not found**. “Built-in” does not imply every product surface installs it.

## 4. Current Rollshot behavior

Rollshot's model context exists inside one memory-only Rig state machine. Rig
threads messages and enforces a complete, non-empty correlated result set for
pending tool calls before the next model turn. Rollshot owns finite budgets,
cancellation, serial tool execution, source generations, validation/dry-run
evidence and typed terminals. None of that state currently crosses a compact
boundary because no such boundary exists. [E:R1]

Action Guide already demonstrates the required separation: its project
manifest and typed proposals persist independently from model history. That is
product/artifact persistence, not context compaction or Memory. [W2]

Security consequence: adding a transcript summary would create a new durable
derived copy of potentially sensitive prompt, screenshot-derived or tool
content. It must not silently inherit artifact retention merely because it is
useful for model continuity.

## 5. Per-system behavior

### 5.1 Pi: one active full-summary design

Pi's low-level loop only supplies `transformContext(messages)` before provider
conversion. The active coding-agent product owns the actual compact policy.
[E:P1]

Manual `/compact` and default-enabled automatic maintenance use the same
structured summary path. The default threshold is
`contextTokens > contextWindow - 16_384`; cut-point selection walks backward
until approximately 20,000 recent tokens are retained. It never cuts at a
`toolResult`; a split oversized turn gets a separate turn-prefix summary. The
prompt explicitly asks for goal, constraints, progress, blocked work, key
decisions, next steps and critical context, then appends cumulative read/edited
file lists. [E:P2]

Overflow is reactive but bounded: a same-model overflow error is removed from
the active projection, a compact boundary is appended, and the turn is retried
at most once. A successful answer whose reported usage is over the configured
window compacts without retry. Threshold maintenance compacts without
automatically repeating a completed turn. Transient summarizer failures use the
configured retry helper; cancellation, extension cancellation, absent model or
auth, invalid/missing boundary and exhausted retry return a visible compaction
failure rather than a fabricated summary. [E:P3]

`CompactionEntry` persists `summary`, `firstKeptEntryId`, `tokensBefore`,
optional usage/details and an extension marker in the JSONL tree. Older entries
remain stored; active context becomes summary plus kept messages. The newer
generic harness can store a materialized `retainedTail`, but that harness is not
wired into the current coding-agent path and must not be combined with its
active behavior. [E:P2, E:P4]

Named mini/microcompact, snip, pruning, shake, context-collapse and
provider-native tool clearing were **not found in the investigated scope**
[A:P]. Pi reports compaction-call cache read/write usage, but an explicit cache
preservation/invalidation policy for the replacement was **not found in the
investigated scope** [A:P-CACHE].

### 5.2 oh-my-pi: several deliberately different reducers

oh-my-pi's current branch stores `CompactionEntry` as a session-tree boundary;
old entries remain in JSONL while the model sees the latest summary/archive,
kept tail and later entries. The default strategy is `snapcompact`, with
`context-full`, `handoff`, `shake` and `off` alternatives. [E:O1]

| Mechanism | Trigger and algorithm evidence | Persistence/cache/failure | Status |
|---|---|---|---|
| `context-full` | Manual, overflow, incomplete output, post-turn threshold, optional mid-turn and idle maintenance. Local structured summary plus retained tail; cut never lands on tool result; split turns receive two summaries. **L3.** | Appends boundary; cumulative file list. Provider session closes after history rewrite. Model-candidate retry/fallback; errors distinguish overflow, incomplete and maintenance. | Built-in; compaction enabled by default, strategy available but not default. |
| Provider/native remote | Configured generic summarizer or OpenAI-compatible chat endpoint; OpenAI/Codex may use provider-native `/responses/compact` or streaming V2 and retain opaque provider state in `preserveData`. **L2/L3.** | Provider replacement state is persisted; native failure falls back according to policy; raw remote call has a 180-second ceiling. | Enabled by settings by default but endpoint/model capability dependent. |
| `snapcompact` | Deterministically serializes discarded history, truncates noisy tool data, rasterizes bounded source into model-aware PNG frames, and retains plain-text chronological edges. **L2.** | Source text and frames live in `preserveData.snapcompact`; later compacts re-render source. No summary-model/cache call. Falls back to `context-full` when the continuation model lacks image input. | Built-in and configured default; vision-model dependent. |
| `handoff` | Model-authored handoff with live system prompt/tools/history; starts a new session. Mid-turn/overflow paths fall back instead of racing or reusing an overflowing input. **L2.** | No `CompactionEntry`; new session gets visible handoff custom message. New provider/session cache. | Built-in strategy, not default. |
| pruning/elision | Replaces old, superseded or `useless` tool-result content with exact placeholders. Default age-based policy protects 40k recent tool-output tokens and requires 20k savings; small results and skill/active-plan reads are protected. **L3.** | Rewrites session entries but retains tool-result block/call ID. A warm-prefix suffix guard avoids expensive cache rewrite; message caches are invalidated on mutation. | Built-in maintenance; useless-result drop default-on where wired. |
| `shake` | Selects whole tool results and large fenced/XML regions; auto config protects 16k recent tokens and requires 4k savings, manual mode is aggressive. Skills are protected. **L3.** | Original regions are saved to `artifact://` when persistence succeeds; placeholders retain recovery link. Rewrites JSONL, invalidates caches and closes provider sessions; without persistence/write success it degrades to an unrecoverable placeholder. | Built-in manual/strategy path, not the default full mechanism. |

`mini-compact`, `microcompact` and cached microcompaction by those names were
**not found in the investigated scope** [A:O]. Rebranding shake, prune or
snapcompact as microcompact would erase their different recovery, cache and
fidelity contracts.

Todo snapshots and checkpoint/rewind markers are reconstructed from session
entries separately. Child transcripts and artifact spill may persist; live
`AsyncJobManager` state, controllers and approval cache do not become compact
state. Optional local/Hindsight/Mnemopi Memory remains a separate backend.
[E:O2]

### 5.3 Codex: persisted local or provider replacement history

Codex compaction is a core non-feature-gated operation. Pre-turn and mid-turn
auto triggers compare scoped/current context usage to the configured/model
limit; manual `Op::Compact` uses the same lifecycle hooks. Eligible OpenAI or
Azure Responses providers use remote compaction; other providers use the local
path. Remote V2 is stable/default-on, while selection remains provider
constrained. [E:C1, E:C2]

Local compaction sends existing history plus a checkpoint prompt through one
Responses client session, retries stream failures, and when the compact request
itself overflows removes oldest history items until something fits. The
replacement retains up to 20,000 tokens of recent real user messages and
appends the model summary as a user message. Mid-turn compaction re-injects
canonical initial context before the last real user message; standalone/pre-turn
replacement defers canonical reinjection to the next regular turn. [E:C1]

Remote V1 uses `/responses/compact`. Remote V2 uses an ordinary Responses
stream, requires exactly one compaction output item, admits at most two
transport retries, retains recent user/developer/system messages under a 64k
text budget (images are carried with retained messages), then appends the
provider compaction item. A previous-model attempt can fall back to the current
model only for classified retryable cases. [E:C2]

Every successful path installs and persists `RolloutItem::Compacted` with
replacement history and context-window lineage. Original rollout items remain
canonical history; reconstruction chooses the latest applicable replacement.
That is a conversational checkpoint, not resumption of pending approvals, tool
futures, provider streams, background processes, Goals or Workflow. [E:C3]

A dedicated invoked-skill/task/Todo/plan/permission/approval/artifact/recent-file
continuity attachment was **not found in the investigated compact sources**
[A:C-CONT]. The summary prompt requests decisions, constraints, progress and
next steps; canonical initial context is re-injected as described above.
Session-level grants may remain live in the same process, and Goal is separately
database-backed, but neither is encoded into the compact checkpoint. Named
mini/micro/cached compaction, snip, pruning, shake and context collapse were
**not found in the investigated scope** [A:C].

### 5.4 Claude Code: layered full, micro and projected paths

Traditional manual/automatic full compaction is implemented on the ordinary
path. The effective window reserves at most 20,000 summary-output tokens and
auto compaction begins 13,000 tokens below that effective window. Settings and
`DISABLE_COMPACT`/`DISABLE_AUTO_COMPACT` can disable it. Three consecutive
automatic failures trip a session circuit breaker. [E:L1]

Full compaction asks a tool-denied single-turn compact agent for a detailed
summary. The post-boundary order is summary, optional retained segment,
attachments and hook messages. The host explicitly re-injects up to five recent
files under token caps, plan file and plan-mode instructions, invoked skills
under per-skill/aggregate caps, async-agent status/output paths, deferred tool
and agent discovery deltas, MCP instructions and session-start hooks. A
prompt-too-long compact request drops oldest complete API-round groups up to
three times; pairing normalization handles resulting orphans. [E:L2]

The cache behavior is first-class. The default-enabled compact fork attempts to
reuse the main prompt-cache prefix and falls back to a normal streaming path.
After success the host marks the expected cache break, clears read/memory,
system-prompt, microcompact, classifier-approval and speculative-check caches,
but deliberately retains invoked-skill content for the next compact. Subagent
cleanup avoids clobbering main-thread module state. [E:L2, E:L3]

The remaining mechanisms are not one feature:

| Mechanism | Evidence and semantics | Status |
|---|---|---|
| Session-memory compact | Uses a persistent session-memory summary plus a recent tail (10k minimum, five text-bearing messages, 40k maximum defaults), adjusts the boundary for tool-use/result and shared-thinking-message invariants, and falls back to traditional compact when invalid. | Implemented, GrowthBook/default-false gated; **L2**. It uses Memory as input but still emits a compact boundary. |
| Time-based microcompact | After a main-thread cache gap (default 60 minutes), replaces all but five recent compactable tool results with `[Old tool result content cleared]`; keeps blocks/IDs and resets cached-MC state because the prefix is cold. | Implemented but default `enabled: false`; **L2**. No compact boundary or disk rewrite is evidenced in this path. |
| Cached microcompact | Visible path queues provider `cache_edits`, leaves local messages unchanged, pins edits by user-message position, then emits a boundary after actual deleted-token usage is observed. | `CACHED_MICROCOMPACT`; **hidden/missing implementation, L1** because `cachedMicrocompact.ts` is absent. Selection/deletion algorithm is not established. |
| Reactive compact | Visible loop withholds prompt-too-long/media errors, permits one recovery transition and uses the returned full-compaction shape. | `REACTIVE_COMPACT`; **hidden/missing implementation, L1** because `reactiveCompact.ts` is absent. Trigger callsites are evidence; compact algorithm is not. |
| History snip | Runs before microcompact, projects a shortened query view, yields a boundary and subtracts estimated freed tokens from auto-compact pressure; headless `QueryEngine` may truncate its local view while REPL scrollback remains fuller. | `HISTORY_SNIP`; **hidden/missing implementation, L1** because both snip modules are absent. Selection/persistence algorithm is not established. |
| Context collapse/projection | Runs before auto compact, projects a model view, can drain staged collapses on a withheld 413, and has append-only commit plus last-wins staged-snapshot record shapes restored on resume. | `CONTEXT_COLLAPSE`; **hidden/missing implementation, L1** because `index.ts` and `persist.ts` are absent. The 90/95-percent comments and call sequence do not prove the hidden algorithm. |
| API provider-native tool/thinking clearing | Emits conditional Anthropic context-management edits. Thinking is preserved by default for the call or reduced to one turn after a cold gap. Tool result/use clearing uses visible 180k trigger/40k target defaults. | Thinking strategy implemented conditionally. Tool clearing is **internal-only and explicit-env gated** (`USER_TYPE=ant` plus `USE_API_CLEAR_TOOL_*`); **L2**. |

Full compaction boundaries and content replacements are persisted in JSONL.
The hidden collapse record shapes are also visible, but their reducer is not.
Auto Memory is independent Markdown/topic-file storage; clearing its attachment
cache after compaction does not merge Memory with the compact summary. [E:L4]

## 6. Trigger, boundary, cache and failure comparison

| System | Trigger policy | Installed model boundary | Cache consequence | Failure path |
|---|---|---|---|---|
| Rollshot | None in scoped agent foundation. | None. | None. | Typed run terminal, not compaction recovery. |
| Pi | Manual; post-turn threshold; same-model overflow. Auto enabled by default. | Persisted summary + `firstKeptEntryId` + kept tail; old JSONL remains. | Explicit cache replacement policy was **not found in the investigated scope** [A:P-CACHE]; summary call reports cache usage. | Extension may cancel/replace; transient summarizer retry; one overflow compact-and-retry; failure event and no fabricated boundary. |
| oh-my-pi | Manual, overflow, incomplete, post-turn, optional mid-turn, idle; strategy-specific dispatch. | Summary/tail, provider replacement, snap archive, or new handoff session; prune/shake mutate selected entries. | Cache-aware pruning avoids warm prefix; mutation invalidates memoized message conversions; full/shake rewrites close provider sessions; native remote may preserve opaque state. | Context promotion and model/remote/local fallback; abort; typed notices; shake artifact-write failure degrades recovery link; snap needs image capability. |
| Codex | Manual; pre-turn and mid-turn scoped/full-window limit. | Persisted replacement history and window lineage; local summary or provider compaction item. | Local retry reuses one client session; token analytics record cache input/write. Replacement begins a new context window; no separate mini-cache editor. | Hooks may stop; bounded stream retry; local overflow drops oldest items; remote V2 requires exactly one compact item and two transport retries; errors end the compact task. |
| Claude Code | Manual, proactive threshold, optional session-memory; hidden reactive/snip/collapse callsites; time-gap micro; internal API policies. | Full boundary + summary/retained segment/attachments; time micro mutates request view; cached/API paths may edit provider cache only; collapse shape unresolved. | Full compact first tries cache-sharing fork then marks break/clears caches. Time micro waits for cold cache; cached micro is designed for warm-cache edits. | Three-failure auto circuit breaker; up to three PTL head truncations; streaming fallback; hidden reactive one-attempt guard; user abort and invalid summary surface. |

## 7. Preservation matrix

“Preserved” below distinguishes **explicit model-visible continuity** from
**separate durable/live state**. Persisting an old transcript is not enough to
claim that the post-compact model sees a fact.

| Continuity item | Rollshot | Pi | oh-my-pi | Codex | Claude Code |
|---|---|---|---|---|---|
| Invoked skills | No compact boundary exists, so skill continuity across a compact boundary is not established [A:R, A:R-CONT]. | A dedicated invoked-skill compaction attachment was **not found in the investigated scope** [A:P-CONT]; continuity is summary/retained messages only. | Skill results and `skill://` reads are protected from prune/shake; a dedicated invoked-skill full-compaction attachment was **not found in the investigated scope** [A:O-CONT]. Skill files remain external resources. | A dedicated invoked-skill compact attachment was **not found in the investigated scope** [A:C-CONT]; canonical initial context is re-injected mid-turn or next turn. | Explicit agent-scoped `invoked_skills` attachment, recent-first, capped; invoked-skill map intentionally survives cleanup. |
| Tasks / Todos / Workflow state | Run-local draft/evidence exists, but no Product Task/Workflow compact attachment is established [E:R1, A:R-CONT]. | A dedicated task/Todo/job compact attachment was **not found in the investigated scope** [A:P-CONT]. Example Todo can reconstruct from full JSONL branch, separately from model projection. | Todo phases reconstruct from session entries; child transcript may persist; live Task/Job controller remains separate [E:O2]. A host-owned Workflow/DAG contract was **not found in the investigated scope** [A:O-WORK]. | A dedicated task/Todo/plan compact attachment was **not found in the investigated scope** [A:C-CONT]; `update_plan` is an event/checklist and Goal is separate DB state. | Summary prompt asks for pending tasks; async local-agent status/output path is attached. Work-ledger JSON and legacy Todo persist separately; a general `Workflow` declaration was **not found in the investigated scope** [A:L-DOM]. |
| User constraints / decisions | They exist only in the live run messages; no compact restoration is established [E:R1, A:R-CONT]. | Explicit summary sections for constraints and key decisions. | Structured summary/handoff prompts; snapcompact preserves bounded source rather than interpreting it. | Summary prompt explicitly asks for decisions, constraints and preferences. | Detailed summary prompt explicitly requests decisions, user requests and pending/current work. |
| Permissions / approvals | App-owned consent and tool availability are live; no permission/approval compact attachment is established [E:R1, A:R-CONT]. | Built-in grant/cache was **not found in the investigated scope** [A:P]. | Live approval cache/policy is outside compact entries, and ACP “allow always” is process-local [E:O2]. | A dedicated permission/approval compact attachment was **not found in the investigated scope** [A:C-CONT]; session grants can remain live. | A dedicated permission/approval compact attachment was **not found in the investigated scope** [A:L-CONT]; classifier approvals and speculative checks are cleared after full compaction [E:L3]. |
| Product artifacts / proposals | Typed proposals and Action Guide projects remain app-owned; no artifact/proposal compact attachment is established [W1, W2, A:R-CONT]. | A dedicated artifact compact attachment was **not found in the investigated scope** [A:P-CONT]; files/tool details remain stored history or ambient files. | Artifact spill is separate; shake explicitly writes recovery artifacts when possible [E:O2]. | A dedicated artifact compact attachment was **not found in the investigated scope** [A:C-CONT]; original rollout and Goal/image extensions remain separate. | Async task output paths may be attached; a general typed `Artifact` declaration was **not found in the investigated scope** [A:L-DOM]. |
| Pending gates / checkpoints | `NeedsUserInput` ends a run and review remains app-owned; no gate/checkpoint compact attachment is established [E:R1, A:R-CONT]. | A dedicated gate compact attachment was **not found in the investigated scope** [A:P-CONT]; “Blocked” is summary prose. | Todo/checkpoint state reconstructs separately [E:O2]; handoff/summary may mention it [E:O1]. | Pending user-input/approval futures are not reconstructed by Thread resume [E:C3]. | Plan file/mode are explicit; a general permission/approval/gate compact attachment was **not found in the investigated scope** [A:L-CONT]. |
| Tool-call/result pairing | Rig requires a complete result set before advancing [E:R1]. | Cut never begins at tool result; split-turn logic retains the paired assistant side [E:P2]. | Cut never begins at tool result; prune/shake retain the tool-result block and call ID; useless serialization may omit the whole pair from discarded summary input [E:O1]. | Local replacement selects recent user text plus summary, so prior call/result structures are omitted as part of that replacement [E:C1]; remote replacement is provider-owned [E:C2]. | Session-memory adjusts tail start for pairs; PTL retry groups API rounds and normalizes orphans; time micro clears result content but keeps the paired block/ID [E:L2, E:L3, E:L4]. Selection algorithms in the missing reducers remain unknown [A:L]. |
| Recent files / plan | Tool context has current source/evidence, but no recent-file/plan compact restoration is established [E:R1, A:R-CONT]. | Summary appends cumulative read/modified file lists; a dedicated plan/recent-file attachment was **not found in the investigated scope** [A:P-CONT]. | Summary appends cumulative file tree; active plan reads are protected from prune/shake. | Summary may mention files; a dedicated recent-file/plan compact attachment was **not found in the investigated scope** [A:C-CONT]. | Explicit recent file re-read (up to five plus caps), plan file, plan-mode and discovered-tool attachments. |
| Child/background status | No child/job compact restoration is established [A:R-CONT]. | A dedicated job compact attachment was **not found in the investigated scope** [A:P-CONT]. | Live Task/Job state remains outside compaction; child transcript/artifacts may persist [E:O2]. | Child topology is Thread persistence, not compact payload; background terminals remain live process state [E:C3]. | Explicit local async-agent status/error/output attachment; Work-ledger/runtime state remains separately owned. |
| Provider-specific continuation | Private adapters and Rig history exist only inside the current run; no provider-continuation compact attachment is established [E:R1, A:R-CONT]. | Assistant messages retain provider IDs/signatures in kept history; summary is provider-neutral prose. | Remote compaction may persist opaque provider replacement state; history rewrites close provider sessions. | Remote compaction item is provider-native; local path is Responses-shaped and persists replacement history. | Cached/API edits are Anthropic-specific; full summary reconstructs Claude message context and resets cache baselines. |

No summary provides a cryptographic or schema-level guarantee that a model
mentioned every required item. Explicit attachments improve visibility but
still do not replace authoritative product stores.

## 8. Compaction versus persistence and Memory

| System | Compaction projection | Persistence boundary | Memory boundary |
|---|---|---|---|
| Rollshot | None. | Agent state memory-only; typed review/project artifacts owned by app/Action Guide. | Service was **not found in the investigated scope** [A:R]. |
| Pi | Summary + kept tail. | Append-only JSONL keeps original entries and boundary; resume rebuilds conversation, not interrupted work. | Built-in cross-session semantic Memory was **not found in the investigated scope** [A:P]; extensions may add one. |
| oh-my-pi | Summary/provider replacement/snap archive or selective mutation. | JSONL boundary/rewrites and artifacts; live Jobs/approvals are not recovered. | Optional local/Hindsight/Mnemopi backends; local default off. Memory context may inform summary but remains independently stored. |
| Codex | Local or remote replacement history. | Append-only rollout + `CompactedItem`; resume reconstructs Thread/model view, not in-flight Turn objects. | Stable-labelled/default-off extraction/consolidation extension, separate from compact items. |
| Claude Code | Boundary + summary/tail/attachments; optional cache/projection layers. | JSONL transcript and feature-specific records; live approvals/tools are not instruction-pointer resume. | Default-on-when-eligible Auto Memory Markdown/topic files plus gated session/team paths; attachment caches may reset on compact. |

The minimum Rollshot rule implied by all three workloads is: authoritative
project revision, proposal, checkpoint decision, permission grant and external
job identity must never exist only in a compact summary or Memory document.

## 9. Failure, cancellation and privacy consequences

- **Loss detection:** a syntactically valid summary can omit a decision without
  raising an error. Continuity validation therefore needs workload assertions,
  not only “summary call succeeded.”
- **Pair safety:** retaining a result without its call is a provider protocol
  error; retaining neither is valid only if any important output is summarized
  or recoverable elsewhere. Pi/oh-my-pi/Claude visibly defend pair boundaries.
- **Overflow recursion:** compaction itself can exceed the window. Pi bounds
  recovery to one retry, Codex peels oldest items, Claude bounds PTL retries and
  automatic failure loops, and oh-my-pi can promote/fallback. Rollshot needs an
  explicit terminal rather than an unbounded compact loop.
- **Cache economics:** mutating a warm prefix can cost more than it saves.
  oh-my-pi's suffix guard and Claude's cold-gap/warm-edit split make cache state
  part of scheduling; provider-reported cache reads/writes should be measured,
  not inferred from reduced local token estimates.
- **Sensitive derivatives:** semantic summaries and Memory can concentrate
  names, paths and screenshot-derived facts. Snapcompact preserves even more
  source detail. Each derivative needs provenance, encryption/permissions,
  retention and deletion tied to its source artifact.
- **Authority:** compact text is untrusted model output. Statements such as
  “approved,” “safe to publish,” or “user allowed upload” cannot recreate an
  approval. The authoritative host record must be consulted after compaction
  and resume.
- **Recoverability:** shake with an artifact link and provider-native opaque
  replacement allow retrieval/replay that prose alone cannot. Those benefits
  also create storage lifecycle and compatibility obligations.

## 10. Workload continuity traces

### 10.1 Smart Redaction

```text
authorized input + task profile
  -> source generation N
  -> validation evidence(N)
  -> dry-run evidence(N)
  -> proposal submit
  -> app review
```

If context pressure occurs before submit, the host must re-inject authoritative
`generation=N`, current source/artifact reference, validation/dry-run status,
remaining budget, allowed/terminal tools and consent mode. A prose statement
that validation passed cannot replace generation-bound evidence. The expected
common case should record **zero compactions**; frequent compaction would signal
an oversized prompt/tool-result design. This is an **Inference** from [W1].

### 10.2 Action Guide

```text
ProjectManifest revision R + selected step/keyframe
  -> bounded annotation/caption request
  -> typed proposal bound to R
  -> user review
  -> apply only if current revision == R
```

Compaction may summarize conversational rationale, but the project store owns
R, step/frame identity and proposal. After compaction or process resume, a stale
proposal must still fail deterministically. Cross-step conversational Memory is
optional; durable artifacts already carry the stronger continuity. This is an
**Inference** from [W2].

### 10.3 Deferred brag + Hyperframes

```text
inspection artifacts -> plan/check gate -> asset/audio/frame work
 -> expected scene files -> assembly/verification
 -> explicit render approval -> MP4/poster/share copy
```

The coordinator can compact after an artifact/checkpoint boundary, but the
summary must be reconstructed from the workflow ledger: completed artifact
IDs/hashes, pending predecessors, job handles, one permitted re-dispatch and
approval state. Worker completion remains expected-artifact verification, not
a compacted notification. A fresh context generated from durable state is a
valid alternative to ever-longer summary chains. This is an **Inference** from
[W3-W6].

## 11. Candidate Rollshot patterns and trade-offs

These are patterns for later synthesis, not a final selection.

### Pattern A — host-owned full checkpoint with typed continuity manifest

At a manual/threshold boundary, generate a model summary but install it only
beside a Rollshot-authored manifest containing opaque references to current
task/run, product revision, current source generation, validation/dry-run
evidence, pending review/gate, invoked skill package IDs, remaining budget and
recent artifact/file references. Preserve a bounded recent tail at a
tool-call-safe boundary. Persist original transcript and replacement projection
separately when product retention permits.

**Advantages:** close to Pi/Codex full compaction while borrowing Claude's
explicit continuity inventory; validation can fail closed if required fields
are missing; works for bounded Smart Redaction and one Action Guide task.

**Costs/risks:** summary latency and privacy-sensitive derived text; cache
rewrite; manifest schema/versioning; explicit rules for which live grants are
revalidated rather than serialized. It still does not recover jobs or a
Workflow.

### Pattern B — projection-first, cache-aware selective reduction

Keep canonical transcript/artifact state unchanged and project the provider
view by eliding superseded/uneventful tool results, offloading large results to
typed artifacts and retaining recovery references. Protect skills, active plan,
current generation evidence and a recent tail. Only escalate to Pattern A when
the selective projection cannot create enough headroom. Cache-aware scheduling
may defer warm-prefix mutations or use a provider-native edit when supported.

**Advantages:** deterministic, inspectable loss; avoids model-summary drift for
many tool-heavy runs; measurable cache economics; shake/prune and time-based
microcompact provide materially different evidence from full summary.

**Costs/risks:** provider-specific cache contracts; artifact retention/read
authorization; must preserve call/result invariants; does little for long
natural-language discussions; a local placeholder is not recovery unless the
artifact is durable.

### Pattern C — artifact/workflow re-projection instead of transcript compaction

For Action Guide or deferred media work, end the coordinator context at safe
product boundaries and start a fresh bounded run from durable project/workflow
state, verified artifact inventory and checkpoint decisions. Conversation
summary is optional explanatory context, not the recovery source.

**Advantages:** strongest crash/review semantics and least summary-chain drift;
fits existing Action Guide persistence and Hyperframes' expected-artifact
completion.

**Costs/risks:** requires product/workflow projection code and cannot preserve
uncaptured conversational nuance. It is excessive for a single Smart
Redaction run unless that run already produces the required handoff artifact.

## 12. Preliminary fit without final selection

| Pattern | Smart Redaction | Action Guide | Deferred brag + Hyperframes |
|---|---|---|---|
| A: full + typed manifest | Emergency safety valve; expected trigger rate should be near zero. | Suitable for one bounded proposal if manifest binds project revision. | Useful coordinator continuity but insufficient without Workflow/Job persistence. |
| B: selective projection | Strong fit for large validation/read results when recovery artifacts exist. | Useful for repeated frame/OCR outputs; project stays authoritative. | Helpful within a stage; cannot encode dependency/gate readiness. |
| C: artifact/workflow re-projection | Usually unnecessary. | Natural fit because project already persists domain state. | Strong semantic fit if the deferred workload is adopted; implementation scope belongs to later rounds. |

Synthesis must decide whether Rollshot needs any compact path before it has a
durable session boundary, and whether provider-native optimization is worth its
coupling. This document intentionally does not decide.

## 13. Measurable evaluation criteria and required spikes

Use scripted traces with injected canary facts, not subjective “continued
well” judgments:

1. **Required-state recall:** 100% exact recovery of task/run ID, project
   revision, source generation, current artifact IDs, pending gate and next
   deterministic action after each boundary.
2. **Authority safety:** 0 cases where a summary recreates consent, approval or
   permission absent from the host store; 100% revalidation before side effects.
3. **Tool protocol integrity:** 0 unmatched tool calls/results in every
   projected provider request; property-test arbitrary cut points and parallel
   result ordering.
4. **Artifact validity:** 100% recovery references resolve under authorized
   resume; missing/deleted artifacts produce typed failure, never silent prose
   substitution.
5. **Budget headroom:** resulting request remains below the provider limit by a
   configured reserve at p99; measure actual provider input, not only local
   estimates.
6. **Cache economics:** before/after cached-input, cache-write and uncached-input
   tokens plus monetary cost; compare warm and cold runs for full versus
   selective projection.
7. **Quality drift:** execute one, three and five successive boundaries on the
   same trace; score canary recall, wrong next action, duplicate work and stale
   decision rate.
8. **Latency/reliability:** p50/p95 compact latency, summarizer/provider failure,
   retries, fallback rate and circuit-breaker activation.
9. **Privacy/deletion:** enumerate every derived summary/archive/artifact;
   deletion of a source run removes or tombstones all derivatives within the
   product SLA and leaves an auditable record without private content.
10. **Workload-specific success:** Smart Redaction retains generation evidence;
    Action Guide rejects changed revision; Hyperframes resumes from last valid
    artifact and never bypasses render approval.

Required bounded spikes before selection:

- Compare Pattern A and B on a synthetic tool-heavy Smart Redaction trace at
  warm/cold cache states and at two provider windows.
- Crash after boundary persistence but before next request; verify atomic
  reconstruction of canonical history and projection.
- Delete or revoke an offloaded artifact and confirm fail-closed recovery.
- If considering provider-native clearing/remote compact, replay the same
  canonical trace through another provider and document the compatibility
  fallback.
- Runtime-test hidden/gated reference features only if they become candidate
  dependencies; static callsites are insufficient.

## 14. Evidence gaps and bounded absence audits

The code-review graph was queried first for Rollshot and each reference.
Rollshot's graph had 7,979 nodes/405 files but returned no compact-related
nodes for the focused semantic query. Each ignored learn-project returned zero
nodes, zero edges and zero files, so bounded source inspection followed.

- **[A:R] Rollshot.** Exact roots:
  `crates/rollshot-agent/src/{domain,driver,model,provider,runtime,tools}.rs`.
  Exact case-insensitive regex:
  `compaction|compact|microcompact|snip|prun|projection|context.?collapse|context.?management|memory`.
  It returned no matches. Therefore a compaction, projection or Memory
  mechanism was **not found in the investigated scope**.
- **[A:R-CONT] Rollshot compact continuity attachments.** Exact literal files:
  `crates/rollshot-agent/src/{domain,driver,model,provider,runtime,tools}.rs`.
  Exact case-insensitive regex:
  `compact.{0,60}(skill|task|todo|workflow|decision|constraint|permission|approval|artifact|proposal|gate|checkpoint|job|child|provider|recent.?file|plan)|(skill|task|todo|workflow|decision|constraint|permission|approval|artifact|proposal|gate|checkpoint|job|child|provider|recent.?file|plan).{0,60}compact`.
  It returned no matches. A compact attachment for those continuity categories
  was **not found in the investigated scope**; this does not deny their live-run
  or app-owned forms.
- **[A:P] Pi variants, grants and Memory.** Exact roots:
  `learn-projects/pi/packages/agent/src`,
  `learn-projects/pi/packages/coding-agent/src/core`, and
  `learn-projects/pi/packages/coding-agent/docs/{sessions,session-format,extensions,compaction,security}.md`. Exact
  case-insensitive regex:
  `mini.?compact|micro.?compact|cached.?compact|snip|prun|shake|context.?collapse|context.?projection|clear.?tool`.
  Hits were an illustrative `transformContext` comment using
  `pruneOldMessages`, unrelated prompt snippets/package-manager prune, and no
  active reducer with those names. A named mini/micro/cached compaction, snip,
  shake, context collapse/projection or provider-native tool-clearing
  mechanism was **not found in the investigated scope**. A second exact
  case-insensitive regex was
  `approval.?cache|cached.?approval|capability.?grant|permission.?grant|authority.?object|filesystem.?authority|network.?authority|permission.?cache|approval.?policy|semantic.?memory|memory.?service|memory.?store|memory.?record|memory.?retriev|retrieval.?policy|memory.?expir|memory.?delet|project.?memory|user.?memory|cross.?session.?memory`.
  It returned no matches in the same roots. A built-in approval/grant cache and
  cross-session semantic Memory service were **not found in the investigated
  scope**.
- **[A:P-CACHE] Pi compact cache contract.** Exact literal files:
  `learn-projects/pi/packages/coding-agent/src/core/compaction/compaction.ts`,
  `learn-projects/pi/packages/coding-agent/src/core/agent-session.ts`,
  `learn-projects/pi/packages/coding-agent/src/core/session-manager.ts`, and
  `learn-projects/pi/packages/coding-agent/docs/compaction.md`. Exact
  case-insensitive regex:
  `prompt.?cache|cache.?preserv|cache.?invalid|cache.?break|cache.?prefix|cache.?edit|cacheRead|cacheWrite`.
  The only hits were `cacheRead`/`cacheWrite` usage aggregation and reporting in
  `compaction.ts` and `agent-session.ts`; there were no policy hits. Therefore
  an explicit cache preservation/invalidation contract for installing a compact
  replacement was **not found in the investigated scope**.
- **[A:P-CONT] Pi compact continuity attachments.** Exact literal files:
  `learn-projects/pi/packages/coding-agent/src/core/compaction/compaction.ts`,
  `learn-projects/pi/packages/coding-agent/src/core/agent-session.ts`,
  `learn-projects/pi/packages/coding-agent/src/core/session-manager.ts`, and
  `learn-projects/pi/packages/coding-agent/src/core/skills.ts`.
  Exact case-insensitive regex:
  `invoked.?skill|skill.?attachment|re.?inject.{0,40}skill|post.?compact.{0,40}skill|plan.?attachment|recent.?file.?attachment|task.?attachment|todo.?attachment|job.?attachment|artifact.?attachment|gate.?attachment`.
  It returned no matches. A dedicated invoked-skill, task/Todo/job,
  plan/recent-file, artifact or gate compaction attachment was **not found in
  the investigated scope**; this does not deny continuity through the summary
  and kept messages.
- **[A:O] oh-my-pi terminology.** Exact roots:
  `learn-projects/oh-my-pi/packages/agent/src/compaction`;
  literal files
  `learn-projects/oh-my-pi/packages/coding-agent/src/session/{agent-session,session-manager,compact-modes}.ts`;
  agent tests
  `learn-projects/oh-my-pi/packages/agent/test/{compaction-thinking-level,compaction-telemetry,compaction-reserve-provenance,compaction-file-ops,compaction-error-status,remote-compaction,snapcompact-frames,shake,supersede-prune}.test.ts`;
  coding-agent tests
  `learn-projects/oh-my-pi/packages/coding-agent/test/{agent-session-compaction,agent-session-auto-compaction-queue,agent-session-auto-compaction-progress-guard,agent-session-prune-persistence,agent-session-plan-reference-compaction,agent-session-plan-mode-convergence,agent-session-plan-compact-hook-instructions,agent-session-manual-snapcompact-fallback,agent-session-manual-retry}.test.ts`;
  and `learn-projects/oh-my-pi/docs/compaction.md`. Exact regex:
  `mini-compact|mini compact|microcompact|micro-compact|cached micro`.
  It returned no matches. A mechanism using those names was **not found in the
  investigated scope**; positive shake/prune/snapcompact evidence is reported
  under its source names.
- **[A:O-CONT] oh-my-pi compact continuity attachments.** Exact roots:
  `learn-projects/oh-my-pi/packages/agent/src/compaction` and literal files
  `learn-projects/oh-my-pi/packages/coding-agent/src/session/{agent-session,session-manager,compact-modes}.ts`.
  Exact case-insensitive regex:
  `invoked.?skill|skill.?attachment|re.?inject.{0,40}skill|post.?compact.{0,40}skill|recent.?file.?attachment`.
  The sole hit was an `agent-session.ts` tree-navigation comment classifying a
  user-invoked skill-prompt injection; it was not a compact attachment. A
  dedicated invoked-skill or recent-file full-compaction attachment was **not
  found in the investigated scope**. This does not negate the positively
  evidenced skill protections in prune and shake.
- **[A:O-WORK] oh-my-pi Workflow/DAG contract.** Exact roots/files:
  repository root `learn-projects/oh-my-pi`; literal paths relative to it:
  `packages/coding-agent/src/task`, `packages/coding-agent/src/async`,
  `packages/coding-agent/src/tools/todo.ts`, and
  `packages/coding-agent/src/goals`. Exact case-insensitive regex:
  `\bdependsOn\b|\bdepends_on\b|\bdependency\b|\bDAG\b|\bworkflowId\b|\bworkflow_id\b`.
  The only hits were comments about TypeScript/runtime dependency graphs in
  `task/yield-assembly.ts` and `task/renderer.ts`. A host-owned Workflow/DAG
  contract was **not found in the investigated scope**.
- **[A:C] Codex variants.** Exact roots:
  `learn-projects/codex/codex-rs/core/src`, `protocol/src`, and
  `thread-store/src`. Exact case-insensitive regexes:
  `mini.?compact`, `micro.?compact`, `cached.?compact`, `compaction cache`, and
  `cache.*compact`. They returned no matches. A named mini/micro/cached
  compaction was **not found in the investigated scope**. A second exact regex
  `snip|prun|shake|context.?collapse` over the same roots found unrelated Rust
  pruning terminology/tests but no context-reduction mechanism; those context
  mechanisms were **not found in the investigated scope**.
- **[A:C-CONT] Codex compact continuity attachments.** Exact literal files:
  `learn-projects/codex/codex-rs/core/src/compact.rs`,
  `learn-projects/codex/codex-rs/core/src/compact_remote.rs`,
  `learn-projects/codex/codex-rs/core/src/compact_remote_v2.rs`, and
  `learn-projects/codex/codex-rs/core/src/compact_remote_request.rs`. Exact
  case-insensitive regex:
  `invoked.?skill|skill.?attachment|task.?attachment|todo.?attachment|plan.?attachment|permission.?attachment|approval.?attachment|artifact.?attachment|recent.?file`.
  It returned no matches. A dedicated invoked-skill, task/Todo, plan,
  permission/approval, artifact or recent-file compact attachment was **not
  found in the investigated scope**; this conclusion is limited to those
  compact implementation files and does not deny canonical initial-context or
  summary-prompt continuity.
- **[A:L] Claude missing implementations.** `git ls-tree -r --name-only` at
  the pinned revision was restricted to literal roots
  `src/services/compact` and `src/services/contextCollapse`. Exact path regex:
  `^src/services/contextCollapse/(index|persist)\.ts$|^src/services/compact/(reactiveCompact|snipCompact|snipProjection|cachedMicrocompact)\.ts$`.
  It returned no matches. Those implementations were **not found in the
  investigated scope**. Visible imports/calls in `src/query.ts`,
  `src/QueryEngine.ts`, `src/services/compact/microCompact.ts`,
  `src/utils/sessionStorage.ts` and `src/utils/sessionRestore.ts` justify only
  L1 gate/order/record-shape claims.
- **[A:L-CONT] Claude compact permission/gate attachments.** Exact repository
  root `learn-projects/claude-code-source-code`; literal files relative to it:
  `src/services/compact/{compact,sessionMemoryCompact,postCompactCleanup,prompt}.ts`.
  Exact case-insensitive regex:
  `create(?:Permission|Approval|Workflow|Artifact|Gate|Checkpoint)Attachment|(?:permission|approval|workflow|artifact|gate|checkpoint).?attachment|attachment.{0,40}(?:permission|approval|workflow|artifact|gate|checkpoint)`.
  It returned no matches. A dedicated permission, approval, Workflow, Artifact,
  gate or checkpoint compact attachment was **not found in the investigated
  scope**; other explicitly generated file, plan, skill, async-agent and tool
  attachments remain positively evidenced in `compact.ts`.
- **[A:L-DOM] Claude generic Workflow/Artifact declarations.** Exact repository
  root `learn-projects/claude-code-source-code`; literal paths relative to it:
  `src/Task.ts`, `src/tasks`, `src/QueryEngine.ts`,
  `src/services/{compact,tools}`, `src/skills`,
  `src/bootstrap/state.ts`, `src/bridge`, `src/memdir`, `src/Tool.ts`,
  `src/utils/tasks.ts`, `src/hooks/useTasksV2.ts`,
  `src/tools/{TaskCreateTool,TaskGetTool,TaskListTool,TaskUpdateTool,TodoWriteTool,AgentTool}`,
  `src/utils/swarm`, `src/utils/{sessionStorage,sessionRestore}.ts`, and
  `src/query.ts`. Exact regex:
  `^(?:export\s+(?:default\s+)?|export\s+declare\s+|declare\s+)?(?:abstract\s+)?(?:type|interface|class)\s+(?:Workflow|Job|AgentRun|Artifact)\b`.
  It returned no matches. Declarations with those four exact names were **not
  found in the investigated scope**; differently named equivalents remain
  possible.

Open questions:

1. What exact correctness invariants and rollout defaults exist inside
   Claude's missing reactive, cached-microcompact, snip and collapse modules?
2. How does provider-native remote compaction behave across model upgrades,
   replay under another provider and server-side retention/deletion?
3. Does snapcompact's image recall remain reliable for code/path/error canaries
   across Rollshot's likely models and screenshot token budgets?
4. Which product state, if any, warrants compaction before Rollshot implements
   durable session/persistence policy?
5. Static inspection cannot establish deployed GrowthBook assignments, cache
   TTLs, remote service behavior or real summary quality.

## 15. Evidence index

### Rollshot and workloads

- **[E:R1] Source + tests:**
  `crates/rollshot-agent/src/{domain,driver,model,provider,runtime,tools}.rs`;
  current Rig run/message/tool-result boundary; graph and [A:R].
- **[W1] Source + tests:**
  `crates/rollshot-app/src/result_workspace/workbench/{run,state,mod}.rs` and
  Smart Redaction driver/tools; finite review-producing run.
- **[W2] Source + tests:** `crates/rollshot-action/src/project/{model,store}.rs`
  and timeline visual/caption proposal paths; durable revision-bound product
  state.
- **[W3-W6] Workload source:** brag `skills/brag/SKILL.md`; Hyperframes
  `production-loop.md`, `subagent-dispatch.md`, and `review-loop.md` at the
  pinned Round 0 revisions. These are deferred workload evidence, not Rollshot
  behavior.

### Pi

- **[E:P1] Source + test source:**
  `packages/agent/src/{agent-loop,agent,types}.ts`, especially
  `transformContext`; `agent-loop.test.ts`.
- **[E:P2] Source/docs:**
  `packages/coding-agent/src/core/compaction/{compaction,utils}.ts`,
  `session-manager.ts`, and `docs/compaction.md`: threshold, cut points,
  summary schema, file tracking and persisted boundary.
- **[E:P3] Source + test source:** `core/agent-session.ts` `_checkCompaction`
  and `_runAutoCompaction`; `test/suite/agent-session-compaction.test.ts`:
  threshold/overflow, extension, retry and failure events.
- **[E:P4] Source + tests, not active CLI integration:**
  `packages/agent/src/harness/{agent-harness,session/*,compaction/*}.ts` and
  `packages/agent/test/harness/{agent-harness,session,compaction,storage,repo}.test.ts`;
  materialized retained tail and provisional harness boundary.

### oh-my-pi

- **[E:O1] Source/docs/tests:** `packages/agent/src/compaction/`,
  `packages/snapcompact/src/snapcompact.ts`, coding-agent
  `session/{agent-session,session-manager,compact-modes}.ts`,
  `docs/compaction.md`; agent tests
  `{remote-compaction,snapcompact-frames,shake,supersede-prune,compaction-error-status}.test.ts`
  and coding-agent tests
  `{agent-session-compaction,agent-session-auto-compaction-queue,agent-session-prune-persistence,agent-session-plan-reference-compaction,agent-session-manual-snapcompact-fallback,agent-session-manual-retry}.test.ts`.
- **[E:O2] Exact source paths** under repository root
  `learn-projects/oh-my-pi`: Todo and checkpoint state in
  `packages/coding-agent/src/tools/{todo,checkpoint}.ts` plus rewind handling in
  `src/session/agent-session.ts`; Task/child state in
  `src/task/{index,types,persisted-revive}.ts`; live detached jobs in
  `src/async/job-manager.ts`; artifact spill in `src/session/artifacts.ts` and
  `src/internal-urls/artifact-protocol.ts`; approval state in
  `src/tools/approval.ts`, `src/session/client-bridge.ts`, and
  `src/modes/acp/acp-client-bridge.ts`; separate Memory backends under
  `src/{memories,memory-backend}`. These paths support the separation of compact
  projection from durable or process-live state. Representative symbols are
  `TodoPhase`, `CheckpointState`, `TaskTool`,
  `createPersistedSubagentReviverFactory`, `AsyncJobManager`, `ArtifactManager`,
  and `AgentSession.#acpPermissionDecisions`.

### Codex

- **[E:C1] Source + test source:** `codex-rs/core/src/compact.rs`,
  `session/{turn,context_window}.rs`, and `core/tests/suite/compact.rs`, including
  `manual_compact_uses_custom_prompt`, `auto_compact_runs_after_token_limit_hit`
  and `auto_compact_runs_after_resume_when_token_usage_is_over_limit`; local
  manual/pre-turn/mid-turn behavior.
- **[E:C2] Source + test source:** `compact_remote.rs`,
  `compact_remote_v2.rs`, `compact_remote_request.rs`, provider eligibility,
  `features/src/lib.rs`, and exact test files
  `core/tests/suite/{compact_remote,compact_remote_parity}.rs`, including
  `remote_compact_replaces_history_for_followups`,
  `remote_compaction_parity_manual_transcripts`,
  `remote_compaction_parity_manual_hooks`,
  `remote_compaction_parity_pre_turn_auto` and
  `remote_compaction_parity_mid_turn_auto`; remote V1/V2 status, retention and
  failure.
- **[E:C3] Source + tests:** `thread-store/src`,
  `session/rollout_reconstruction.rs` and exact test file
  `core/tests/suite/compact_resume_fork.rs`, including
  `compact_resume_and_fork_preserve_model_history_view`,
  `compact_resume_after_second_compaction_preserves_history` and
  `snapshot_rollback_past_compaction_replays_append_only_history`; persisted
  replacement projection and recovery limit.

### Claude Code source

- **[E:L1] Source:** `src/services/compact/autoCompact.ts`; thresholds,
  enablement, session-memory preference and three-failure circuit breaker.
- **[E:L2] Source:** `src/services/compact/{compact,prompt}.ts`; summary,
  attachments, partial/full boundaries, cache-sharing fork and PTL retry.
- **[E:L3] Source:** `microCompact.ts`, `timeBasedMCConfig.ts`,
  `apiMicrocompact.ts`, and `postCompactCleanup.ts`; visible micro/cache/context
  management and cleanup behavior.
- **[E:L4] Source:** `sessionMemoryCompact.ts`, `query.ts`, `QueryEngine.ts`,
  `sessionStorage.ts`, and `sessionRestore.ts`; retained-tail invariants and
  L1 missing-feature callsites/record shapes.

**Confidence:** high for pinned visible source fields, thresholds, ordering,
cut-point invariants, persistence shapes and exact negative audits; medium for
cross-file cache/persistence consequences and test-backed behavior not executed;
low for Claude's missing implementations, server-controlled defaults, remote
provider behavior, deployed cache economics and summary quality. The Reviewed
profiles were used for taxonomy and routing, while the focused compaction claims
above were re-checked against pinned source/tests.
