# Persistence, checkpoint, and resume comparison

**Research date:** 2026-07-22 (Asia/Taipei)
**Status:** In Progress (Round 2 capability comparison)
**Umbrella revision:** 1
**Current Rollshot revision:** `ebaf37a42b8212465ef184fccf3336e1d3dd0d5f`
**Reference revisions:** Pi `dd6bea41efa8caa7a10fe5a6401676dc5699f83f`;
oh-my-pi `7b141199d524b859c357fc89654f10b62b9f3df1`; Codex
`4a443994bd12f49f2f08b21a2f224d9d42b9e734`; Claude Code source
`2ca5ddabfed5f220812ea11f029eda03b21bc4c1`; Rig consumed by Rollshot
`0.39.0`.
**Evidence mode:** static source and test-source inspection. No process crash,
power loss, provider reconnect, child revival, external-job reattachment,
schema upgrade, or product UI resume was executed.

This document compares persistence and recovery mechanisms. It does **not**
select a final Rollshot architecture.

## 1. Rollshot problem and crash scenarios

The product question is not whether an agent can reopen some history. It is
which decisions and effects become durable before a crash, how ambiguous
partial work is reconciled, and which deterministic action is safe next.
Conversation history is useful evidence but cannot recreate authority,
idempotency, or an external process controller.

### 1.1 Smart Redaction: bounded run and review handoff

The current path is an in-memory model/tool loop ending in a typed proposal.
Its expected common case does not require a long-running Workflow, but these
crash points expose distinct recovery questions. [E:R1, W1]

| Crash point | Durable fact required | Safe Resume route |
|---|---|---|
| After authorized input is prepared, before the first provider request | Task identity, input/artifact revision and consent **reference**, provider/model/config fingerprint; raw pixels remain under artifact policy. | Revalidate consent and current artifact; either start attempt 1 or cancel. Never infer permission from a Transcript. |
| After provider output, before its tool call is durably accepted | Provider response/call ID is optional conversation evidence; there is no proven side effect [E:R3]. | Re-run the model turn or stop for user confirmation according to cost policy. Do not synthesize a successful tool result. |
| After a side-effecting tool starts, before completion is committed | Attempt ID, tool/idempotency key, input source generation, and an `unknown` effect state. | Reconcile the tool-specific effect. Retry only when declared idempotent; otherwise ask or compensate. Rig serialization alone cannot answer whether external I/O happened. [E:R3] |
| After validation/dry-run evidence for generation N, before proposal submit | Current source generation, evidence kind/hash/time, remaining budget, and artifact revision. | Reconstruct from typed evidence; never accept prose saying validation passed. |
| After `ReadyForReview`, before the app records the handoff | Proposal ID/payload hash, base revision, terminal and a handoff receipt or one atomic task/proposal commit. | Re-deliver the same proposal idempotently; do not start a new model run merely because the UI missed the event. |
| After user review, before document apply/persist | Durable review decision tied to proposal and base document revision. | Apply once with compare-and-swap, or report stale/conflict. Never replay a cached approval against a newer document. |

Current Rollshot does not persist the `AgentSession`, Rig run, budget, tool
context, proposal handoff, or review decision in the investigated agent path;
the exact durable-session audit returned zero hits [A:R0]. Consequently the
table is a recovery contract to evaluate, not implemented behavior.

### 1.2 Action Guide: durable project around bounded proposals

Action Guide already provides stronger artifact persistence: immutable assets,
a schema-versioned manifest, revision checks, validation on load, and an atomic
manifest commit. [E:R2, W2]

| Crash point | What current storage establishes | Recovery consequence |
|---|---|---|
| During first Save / Save As before directory publish | Work is built in a sibling temporary directory guarded for cleanup; final directory publish is `renameat_with(NOREPLACE)`. | The old destination is not overwritten. A process-killed temp directory may remain because RAII cleanup does not run; stale-temp scavenging policy is still needed. [E:R2] |
| While adding immutable frame assets to an existing project, before manifest commit | Assets may be materialized first; `project.json` still names the prior committed set/revision. | Load follows the old manifest. Unreferenced immutable assets are possible and need bounded garbage collection, not rollback of the valid project. |
| During manifest replacement | JSON is written to a sibling temp file, the file is `sync_all`'d, renamed, then the directory is synced. | Readers see the old or new manifest, subject to platform/filesystem guarantees. Focused tests cover conflicts/corrupt assets, but no injected power-loss run was performed [G:R1]. |
| A proposal finishes against revision R while the user edits/saves R+1 | Proposal input carries a base/document revision and current product paths reject stale results. | Mark stale; do not retry as if provider failure and do not mutate R+1. [W2] |
| Manifest commit succeeds but UI acknowledgement is lost | Revision and manifest are authoritative. | Reload and acknowledge the committed revision; do not create a second revision merely to reproduce the acknowledgement. |
| Load sees malformed JSON, unsupported schema, missing asset, wrong hash, or symlinked asset | The loader returns typed failure and validates referenced assets. V1 is read and converted to V2; unsupported versions fail. | Fail closed and offer repair/import UX. Memory or Transcript text cannot substitute for the rejected artifact. [E:R2] |

Action Guide product recovery is not Agent Run recovery. Its manifest can
reconstruct the guide while the model history is absent, which is a useful
artifact-driven baseline rather than proof that every future task belongs in
the same manifest.

### 1.3 Deferred brag / Hyperframes: checkpoints, Jobs, and artifacts

This workload is evidence for multi-stage recovery only if the deferred product
is adopted. It does not mandate video generation or a general Workflow engine.
[W3, W4, W5, W6]

| Crash point | Required durable boundary | Deterministic next-step rule |
|---|---|---|
| Storyboard/plan file written, before `check` result | Plan artifact identity/revision plus validation result remains absent. | Run `check`; file existence alone does not unlock production. |
| User approves plan/sketch/render, before scheduler observes it | Append or atomically store a decision with gate ID, artifact revision/hash, actor and time. | Project the gate from the durable decision; never ask the model to remember whether approval occurred. |
| Local/cloud Job starts, before its handle is committed | This is an ambiguity window: the external effect may exist without a local durable ID [W3, W4]. | Use a client idempotency key and provider query-by-key, or mark `unknown` and ask/reconcile. Blind restart can duplicate cost/effects. |
| Job handle committed, process dies while it runs | Stable provider/host identity, Job kind, start key, expected outputs and last observation cursor. | Reattach/query authoritative Job status. Conversation Resume does not recreate the process. |
| Worker notification arrives before its expected scene artifact is published | Notification is transient evidence only. | Keep Work Item incomplete until the artifact validates; one clean re-dispatch is allowed only after confirming absence. [W5] |
| Scene/render writer dies mid-file | Stage in a temporary path and publish by atomic rename after validation/hash/fsync policy. | Ignore/quarantine incomplete temp output; never satisfy readiness from a partial filename. |
| Render finishes externally before completion event commits | External status plus MP4/hash is authoritative evidence; local attempt may be `unknown` [W3, W4]. | Reconcile, validate and append one completion; do not render twice. |
| App restarts after some scenes but before assembly | Workflow decisions, predecessor state, expected artifact contracts and current artifact hashes must survive independently of children. | Recompute readiness; resume only missing/invalid stages. A fresh coordinator context is valid. |

## 2. Terms and non-equivalent Resume boundaries

Bare “resume” is prohibited in this comparison. These six boundaries differ in
state owner, authority, reconstruction and failure behavior.

| Boundary | What resumes | What it must not imply |
|---|---|---|
| **Conversation Resume** | A selected Transcript branch and a model-visible projection, possibly including model/thinking metadata and compaction boundaries. | No provider stream, tool future, approval, Workflow, Job, or external side effect is recreated. |
| **Child-context Resume** | A child transcript/identity plus enough scoped prompt, tool, model and parent metadata to launch another child run. | It is not instruction-pointer continuation or proof that the original child completed. Current capability and permission policy must be revalidated. |
| **Agent Run recovery** | A bounded run state at a declared durable step, including pending model/tool protocol and attempt accounting. | A serializable sans-I/O machine does not prove a pending side effect is safe to repeat. |
| **Workflow recovery** | Durable Product Tasks/Work Items, dependencies, checkpoints, attempts, artifacts and next-ready projection. | Reopening a Session, Todo, Goal or child tree is insufficient without a transition owner. |
| **Job/process reattachment** | A still-live or remotely durable execution handle, observation cursor and lifecycle operations. | A PID alone is unsafe after restart; transport reconnect alone does not establish process identity. |
| **Transport Resume** | Protocol sequence/session state for a connection such as relay, websocket, or remote-control bridge. | It does not reconstruct Conversation, Agent Run, Workflow, approval, or artifact state unless a higher layer explicitly does so. |

Other terms remain separate:

- A **Checkpoint** in this document is a durable user/system decision or typed
  recovery boundary. Pi labels, compaction entries, and oh-my-pi
  checkpoint/rewind are bookmarks/context boundaries, not automatically
  product approval gates.
- An **Artifact** is a named product completion output with whatever identity,
  validation and revision contract the workload requires. An ambient path,
  tool result, task log or child notification is not enough.
- A **Snapshot** materializes current state. An **Event log** records ordered
  facts from which state may be projected. A **Transcript** is a conversation
  record. None is a synonym for the others.
- A persistence call being software-crash safe does not establish power-loss
  durability. `write`/`rename` without the required file/directory sync policy
  can still lose acknowledged data after power failure.

## 3. Persistence model comparison

No model is selected. Each can be correct for a narrower boundary.

| Model | Canonical record and next-step routing | Partial-write / corruption behavior required | Strengths | Costs and failure risks |
|---|---|---|---|---|
| **Event log** | Append immutable, sequenced facts such as attempt started, checkpoint decided, artifact accepted and Job observed; rebuild a projection and choose the first ready transition. | Per-record framing/checksum/schema; reject interior corruption; tolerate only a proven incomplete tail; atomic sequence allocation; snapshot/replay version rules. Side effects require intent/effect/commit reconciliation and idempotency keys. | Complete audit, selective retry, deterministic projections, explains how state changed. | Replay/migration complexity, duplicate/out-of-order events, privacy retention, projection bugs, log growth. Codex rollout is event-like for conversation but is not a generic Workflow event API [E:C1]. |
| **Snapshot** | Atomically replace one typed state image containing IDs, versions, decisions, attempts and next-step inputs; route from explicit status and predecessor fields. | Temp write, validation, file sync, same-directory rename, directory sync or transactional DB; compare-and-swap revision; retain/verify a previous snapshot. | Simple bounded-task recovery and fast reads; natural fit for one Smart Redaction Task envelope or product manifest. | Loses history unless supplemented; large rewrites; concurrent writers and crash during migration need policy; stale snapshot can hide external effects. Action Guide is positive artifact-snapshot evidence [E:R2]. |
| **Transcript** | Append messages/tool results and rebuild the active branch/model context; the model proposes what to do next. | JSONL tail policy, parent-link validation, compaction boundary compatibility, migration and retention. Silent malformed-line skipping can turn corruption into missing context. | Excellent Conversation Resume, branching and audit; already demonstrated by Pi-class and Claude systems. | Weak authority and execution semantics; cannot reliably distinguish attempted from completed side effects or reconstruct Jobs/checkpoints. Sensitive content retention is broad. |
| **Artifact-driven** | Treat validated manifests/files/hashes and explicit gate records as authoritative; recompute which output is absent or stale. | Stage then publish, validate schema/hash/revision, make marker-last commits, quarantine temps/orphans, use compare-and-swap for mutable manifests. | Strong product truth, provider-independent restart, expected-artifact completion, avoids summary drift. | Does not by itself preserve intent, attempts, denial/cancellation, Job handles or conversational rationale. Ambient file existence is too weak. |
| **Hybrid** | Combine a small typed Event log or Task/Workflow snapshot with Artifact references; keep Transcript optional for Conversation Resume and diagnostics; periodically materialize projections. | Define cross-store commit order and reconciliation. A checkpoint/event must never point to an unpublished artifact; an externally started Job without a local handle stays `unknown` until queried [E:C3, E:L3]. | Can assign each state class to its proper owner; supports bounded task and long Workflow recovery without making Transcript authoritative. | Highest schema/migration/testing surface; cross-store orphaning, privacy/deletion fan-out and operational complexity. “Hybrid” is not automatically safest. |

## 4. Current Rollshot and Rig boundary

### 4.1 Durable and live state

`AgentSession` derives only `Debug, Clone`; it owns completed text pairs and a
pending user message in memory. `run_with_provider` pushes the new user message
then constructs a fresh `rig_core::agent::run::AgentRun::new`; prior exchanges
are not passed through `with_history`. Run budget, cancellation, draft
generation/evidence, registry counters and live `RunEvent`s are also process
state. The focused persistence audit found no `AgentSession` serializer/store,
checkpoint, resume or reattachment contract in the exact roots [A:R0, E:R1].

Rollshot's declared `AuditEvent` vocabulary is serializable, but focused
construction hits were confined to `runtime.rs` tests; production emission was
**not found in the investigated scope** [A:R1]. It is not a durable Event log.
Transient workbench events may drop, and terminal reconciliation is live UI
behavior rather than reconnect reconstruction.

Action Guide is the positive exception at the product boundary. Its V2 manifest
and immutable assets are durable, revisions reject stale writers, JSON/asset
validation fails closed, and the commit path has explicit file/directory sync.
That should remain classified as artifact/project persistence [E:R2].

### 4.2 Rig's serializable machine is necessary but insufficient evidence

Pinned Rig 0.39 derives `Serialize`/`Deserialize` for `AgentRun`, including its
protocol phase. A serialized `ExecutingTools` state carries pending calls so a
new process can re-obtain them; focused tests round-trip streamed state while
tools pend and then continue with correlated results. [E:R3]

The source also explicitly states that the serialization contains the full
conversation and has no cross-version stability guarantee: resume with the
same Rig version. Rollshot does not currently persist it. Even if it did, the
machine knows that a tool result is pending, not whether a filesystem, cloud,
or product side effect happened before the crash. A Rollshot recovery contract
would still own:

- storage atomicity, encryption/retention and schema/version envelope;
- provider/model/tool/skill/config fingerprints and compatibility decisions;
- side-effect idempotency/reconciliation;
- durable budget/attempt/cancellation accounting;
- proposal/review/artifact and Job linkage; and
- next-step routing and user-visible recovery terminals.

## 5. Per-system persistence and recovery trace

Every negative cell includes an audit ID with exact roots/regex/hits in
Section 12. “Runtime gap” means static source/test evidence did not execute the
claimed failure path.

### 5.1 Summary matrix

| System | Durable decisions / checkpoints | Reconstruction and next-step routing | Partial writes, corruption and migration | Live state not reconstructed / Stale risk |
|---|---|---|---|---|
| **Rollshot** | Action Guide manifest revisions and typed product artifacts are durable [E:R2]. A durable agent decision/checkpoint/run record was **not found**: [A:R0], 0 hits. | Guide load validates current manifest/assets. Agent Run/workflow routing after restart was **not found**: [A:R0] and Task 7 audit [A:R-WORK]. | Action manifest uses temp + file sync + rename + directory sync, V1→V2 read migration and revision CAS. Power-loss semantics were not runtime-tested [G:R1]. | Entire agent run, approval/review handoff, provider stream and tool future are live rather than reconstructed [E:R1, A:R0]. Persisted skill revision/authority was **not found** in scoped run/session roots [A:SKILL], 0 hits. |
| **Pi coding-agent** | Append-only JSONL messages, model/thinking changes, compaction, custom state and labels; “checkpoint” label/compaction is conversational, not a product gate [E:P1]. | Load active parent-linked branch; restore model/thinking/context; the model continues from conversation. Durable pending tool/provider/approval/Job fields were **not found** [A:P-LIVE], 0 hits, so there is no Agent Run/Workflow next-step reducer. | Active loader skips malformed lines; V1→V3 migration mutates then rewrites. Rewrite opens the target with `"w"`; append/rewrite sync/atomic primitives were **not found** [A:P-ATOM]: one hit, only a field-name “rename” comment. Crash consistency remains a runtime gap [G:P1]. | Provider stream, in-flight tool, queues/retry timers and extension resources are live rather than reconstructed [A:P-LIVE], 0 hits. Harness recovery is explicitly Planned and warns unfinished tools need idempotency declarations [E:P2]. Durable invoked-skill revision was **not found** [A:SKILL], 0 hits. |
| **oh-my-pi** | JSONL branch, model/mode, Todo, Goal and checkpoint/rewind tool results persist. Checkpoint/rewind rehydrates a context save point/report, not filesystem rollback or Workflow approval [E:O1, E:O2]. | Conversation builds from branch; Todo/Goal/checkpoint state reduce from entries. Persisted child `session_init` can rebuild a child contract and transcript; Workflow dependency/next-ready routing was **not found** in Task 7 [A:O-WORK]. | Synchronous appends are documented software-crash safe but never fsync; full rewrites use a fenced temp/rename path and disk failures latch. Migration tests include idempotency; crash/power-loss and alternate storage backends remain runtime gaps [E:O1, G:O1]. | ACP allow-always cache, provider stream, Task promises/controllers and `AsyncJobManager` are live rather than reconstructed [E:O4, A:O-LIVE], 0 hits. Job serialization/rehydration/reattachment was **not found** [A:O-JOB], 0 hits. Cold child revival uses current auth/model/settings and therefore must handle stale capability sources [E:O3]. |
| **Codex** | Canonical rollout JSONL stores selected response/event/context/world-state/compaction/goal/inter-agent records; SQLite is a rebuildable projection [E:C1]. | Three owners remain separate: relay transport claims a fresh stream and keeps sequencing live [E:C4, A:C-RELAY]; exec-server can reattach a retained session/process within its TTL [E:C3]; Thread Resume replays history/settings/window/world state and can restore child topology [E:C1]. Pending Turn objects are absent from ThreadStore reconstruction [A:C-LIVE], 0 hits; the next model turn is fresh. | Every local append flushes canonical JSONL before best-effort SQLite projection. Exact corrupt-tail/schema-upgrade behavior was not runtime-tested in this research [G:C1]; serde defaults and compatibility paths are not a general Workflow migration contract. | Pending approvals/permissions/user input/tool futures/provider stream/background handles are live rather than ThreadStore-reconstructed [A:C-LIVE], 0 hits. Relay sequence/reorder state and detached exec sessions are also live, bounded transport/process mechanisms [E:C3, E:C4, A:C-RELAY]. Resuming under a different model emits a warning [E:C1]. Durable invoked-skill version/authority was **not found** in ThreadStore/reconstruction [A:SKILL], 0 hits. |
| **Claude Code** | Main/subagent JSONL, Task ledger JSON, output files, sidecars, Memory and bridge pointers are separate stores. Local agent and remote identity have explicit narrow resume paths [E:L1, E:L2, E:L3]. | Conversation loader builds a branch and skips/repairs certain orphan fragments. Local agent Resume rebuilds tool/model/permission context; remote sidecar fetches current CCR status; bridge reconnect is gated Transport Resume. Generic Runtime Task recovery was **not found** [A:L-RUNTIME]. | Transcript writes are delayed, batched append calls. Parser skips malformed lines. `fsync`/atomic transcript publish primitives were **not found** [A:L-ATOM], 0 hits; generic transcript schema migration was **not found** [A:L-MIG], 0 hits. Corrupt/stale bridge pointers are deleted. | Local shell/in-process teammate handles are live and generic reconstruction was **not found** [A:L-RUNTIME]. Agent metadata may be missing/stale and falls back; current definitions/tools are rebuilt [E:L2]. Relevant focused test files were **not found** in the pinned external tree [A:L-TEST], 0 hits, so crash behavior remains a source/runtime gap. |

### 5.2 Pi: Transcript continuity, lenient tail parsing

The active `SessionManager` appends one JSON object per line and reconstructs
the selected leaf. Its loader skips blank and malformed lines. This can recover
past an incomplete tail, but it also means an interior corrupt decision can
disappear silently and sever a parent chain. A Rollshot Transcript design
should distinguish “one truncated final record” from interior corruption and
surface repair evidence rather than generally skip malformed records.

Pi's migrations generate V2 tree IDs/parent links and rename a message role for
V3, then `_rewriteFile` writes the whole target directly. The newer generic
Harness instead rejects malformed entry lines in its own storage tests, but is
not the coding-agent integration. Its design explicitly leaves semi-durable
provider/tool recovery Planned. Combining the Harness's stricter test with the
active product's loader would create a fictional implementation [E:P1, E:P2].

Next-step routing after Conversation Resume remains model-owned. A label named
`checkpoint-1` or a compaction `retainedTail` can help context reconstruction;
neither identifies a durable review decision, external Job, or idempotent tool
attempt.

### 5.3 oh-my-pi: richer journal, conversation checkpoint, child cold revival

oh-my-pi makes its file guarantee unusually explicit: synchronous appends have
reached the OS when append returns, but no `fsync` is issued. Atomic rewrites
use a staged body, guard-then-rename, a Windows EPERM backup/rollback path and
fences against concurrent appends or superseding rewrites. Focused tests cover
guard cleanup, EPERM rollback and idempotent session migration; they were
inspected, not executed [E:O1].

Checkpoint/rewind stores message count, entry ID and time after the checkpoint
tool result. On rewind, it retains a model-authored report and replaces the
intermediate active context. `#rehydrateCheckpointRewindState` scans the current
branch to restore an unfinished checkpoint or a completed rewind after Resume
or tree navigation. This is genuine durable **conversation checkpoint**
behavior, but it neither reverts filesystem changes nor carries dependency
readiness, approval authority, Job identity or idempotency [E:O2].

Cold child revival is narrower Child-context Resume. It reopens the transcript,
reads a persisted `session_init` contract, refuses revival when that contract
or cwd is absent, clamps tools to recorded names, and prevents restricted runs
from consulting process-global MCP state. Yet auth, model registry and settings
come from the new process; unknown/missing tool names are ignored. The child
must therefore be treated as a new attempt over prior context, with stale
capabilities surfaced rather than as transparent continuation [E:O3].

`AsyncJobManager` remains a process singleton holding promises, abort
controllers, delivery queues and results. Its `resumeDeliveries` only lifts
suppression inside the same live manager; it is not restart reattachment
[A:O-JOB].

### 5.4 Codex: three-layer transport, process and Thread recovery

`LocalThreadStore` treats JSONL as the durable replay format and SQLite as a
queryable, rebuildable view. Local writes flush the recorder before metadata
projection, so SQLite can lag after a failure but should not get ahead of
canonical history. `InitialHistory::Resumed` reconstructs model-visible
history, previous settings, compaction window lineage, world state and token
information. A different current model produces a warning rather than
pretending perfect compatibility [E:C1].

Codex has three recovery identities with different sequence owners and failure
ranges. They cannot be substituted for one another:

| Layer | Identity and sequence owner | Persisted/live state, Resume and failure range |
|---|---|---|
| **Relay-frame transport** | A harness creates a UUID `stream_id`. `RelayMessageFrame::resume` claims that route at rendezvous with `RelayResume.next_seq = 0`; inspected constructors also set `ack = 0` and `ack_bits = 0. Plain/Noise send counters begin at zero, while Noise's in-memory receiver releases ciphertext by its own contiguous sequence. [E:C4] | The inspected relay/noise roots expose no store/checkpoint/restore primitive [A:C-RELAY], 0 hits. Reordering is live and bounded to a 64-record distance and 1 MiB pending buffer; duplicates are ignored and exhaustion/oversized gaps fail the stream. Deferred reconnect fetches a fresh connection bundle and creates a fresh authenticated stream; it does not replay prior JSON-RPC frames from a durable relay cursor. This layer ends at websocket/Noise/virtual-stream failure and sits below exec-server JSON-RPC session identity. [E:C4] |
| **Exec-server session/process** | `session_id` identifies a live server registry entry; each acknowledged process owns its retained output/event sequence and the client tracks `last_published_seq`. [E:C3] | `resume_session_id` can reattach the retained `ProcessHandler` within 30 seconds and `process/read(after_seq)` replays retained process events. Missing strategy, unacknowledged start, TTL expiry, unrecoverable gap or exec-server death defeats this layer; it is not durable Thread reconstruction. [E:C3, G:C1] |
| **Thread reconstruction** | `ThreadId` and canonical rollout order belong to `ThreadStore`; compaction/window lineage controls the projected model history. [E:C1] | JSONL/metadata reconstruct conversation, settings, world state, tokens and child topology. Live Turn channels/futures, relay state and process handles are outside that projection [A:C-LIVE, A:C-RELAY]. This is Conversation Resume, not transport replay or Job/process reattachment. |

Turn state shows the missing boundary directly: approvals, permission requests,
user input, elicitations, dynamic tool responses and tool counters are held in
maps/channels under a live `TurnState`; those symbols do not occur in
ThreadStore or rollout reconstruction [E:C2, A:C-LIVE]. Thread Resume is
therefore Conversation Resume, not Agent Run recovery.

Exec-server recovery is therefore the middle live Job/process layer. A session detaches
but retains its `ProcessHandler` for 30 seconds (200 ms in tests); reattachment
uses `resume_session_id`, then reads recoverable acknowledged processes after
the last published sequence. Starts become recoverable only after acknowledgement.
Event/output buffers and sequence gaps are bounded, and expiry shuts down the
processes. Focused tests cover reconnect without killing a managed process, but
were not run here [E:C3]. This is not process recovery after exec-server death,
nor is relay sequencing a Thread Resume.

### 5.5 Claude Code: heterogeneous sidecars and current-policy reconstruction

Claude batches JSONL appends on a timer and parses by skipping malformed lines.
The inspected transcript path does not show file/directory sync, an atomic
rewrite contract, or an explicit transcript schema migration. Sidecar writes
for local/remote agents and bridge pointers are ordinary file writes, so a
crash can yield missing/corrupt metadata even when the main Transcript survives
[E:L1, A:L-ATOM, A:L-MIG].

Local Child-context Resume filters unresolved tool uses and orphaned
thinking-only messages, reconstructs content replacements, checks the old
worktree best-effort, and then starts a new async agent under current state.
The original spawn permission is not requested again, while subsequent worker
tools use the reconstructed/current permission context. Missing agent metadata
falls back to a general-purpose agent; a missing worktree falls back to parent
cwd. Those are explicit compatibility choices, not transparent identity
preservation [E:L2].

Remote-agent sidecars persist only identity. Resume fetches authoritative CCR
status: archived/404 entries are removed, transient auth/network failure keeps
the sidecar for later, and still-running sessions resume polling. This is a
useful small-record + external reconciliation pattern [E:L3]. Bridge pointers
have schema validation and a four-hour TTL; corrupt/stale pointers are cleared.
Bridge reconnect remains feature/account/server gated Transport Resume, not
local Runtime Task recovery.

## 6. Durable decision, checkpoint, write and routing contract

The following trace applies candidate persistence models to any Rollshot-owned
Task or Workflow. It is a comparison criterion, not a chosen schema.

```text
authorized intent + input revision
  -> durable attempt intent / idempotency key
  -> side effect or provider/Job invocation
  -> durable observed result
  -> artifact staged + validated + published
  -> durable completion/checkpoint decision
  -> projection computes exactly one next route
```

At each arrow:

1. **Durable decisions:** user approval/denial, cancellation intent, retry
   authorization and artifact acceptance carry stable IDs and the exact input
   revision. They are never reconstructed from model prose.
2. **Checkpoints:** a checkpoint names its gate, Workflow/Task, prerequisite
   artifact revisions, decision, actor and schema version. Repeated delivery is
   idempotent; a later artifact revision makes the decision stale unless policy
   explicitly carries it forward.
3. **Partial writes:** one record is either absent or valid. Multi-record
   transitions have an explicit commit marker/projection rule. An incomplete
   tail may be truncated only after validating sequence/checksum; interior
   corruption fails closed.
4. **Reconstruction:** replay or load validates schema, references, parent
   links, artifact hashes, attempt monotonicity and authorization scope before
   making state runnable.
5. **Next-step routing:** route from typed state, not the latest message:
   `needs_reconciliation`, `needs_user_input`, `ready`, `waiting_job`,
   `ready_for_review`, `complete`, `cancelled`, or a typed incompatible/corrupt
   terminal. At most one scheduler owns a lease for a ready item.

## 7. Stale dependency and compatibility matrix

Resume crosses time as well as process boundaries. The stored record must
separate the version used originally from what is available now.

| Potentially stale dependency | Detection / persisted evidence | Safe policy choices and trade-off |
|---|---|---|
| **Skills/extensions** | Stable authority/package/resource ID plus content/version hash and invoked skill set. The focused persistence roots of all four systems contained no durable invoked-skill revision/authority field [A:SKILL], 0 hits; Codex's live `HostSkillsSnapshot` is discovery/cache state, not that record. | Rehydrate the exact trusted package when retained, or declare incompatible and restart from a safe artifact boundary. Silently loading a same-name newer skill can change tools/instructions. Storing full skill content expands retention and security surface. |
| **Provider/model/API** | Provider ID, model slug/version, wire protocol, context/feature assumptions and opaque continuation provenance. Pi-class transcripts retain provider/model/signatures; Codex warns on model change; Claude restores/falls back according to current definitions. | Continue only when compatibility rules pass; otherwise start a new attempt with canonical inputs. Opaque provider compact/cache state may be nonportable. Pinning indefinitely reduces drift but increases availability and migration cost. |
| **Application/config/schema** | App build, record schema, feature flags, task profile and deterministic migrator version. | Migrate a copy/transactionally, retain the old record, validate invariants, or refuse. Lenient unknown-field/default behavior is useful only when semantics are truly backward-compatible. |
| **Permissions/consent** | Persist the request/scope and decision provenance only where policy permits; current authority is independently resolvable. | Fail closed and revalidate at Resume and before each side effect. Never serialize a live approval channel or treat old “allow always” as universally current. Re-prompting is safer but can frustrate users; product policy decides which narrow grants may survive. |
| **Tool handles/calls** | Call ID, tool contract/version, input hash, side-effect class, idempotency key, attempt and observed result/receipt. | Pure/idempotent tools may retry; externally queryable tools reconcile; unknown non-idempotent effects require user/compensation. A pending Rig call or transcript tool-use is not enough [E:R3]. |
| **External Jobs/processes** | Provider/host authority, stable Job ID or idempotency key, observation cursor, start acknowledgement, expected artifacts and expiry/lease. | Reattach/query the authoritative service; if identity expired, distinguish `lost` from `failed` and ask/retry by policy. A PID is not portable; Codex's TTL handler and Claude remote sidecar show two narrower models [E:C3, E:L3]. |
| **Artifact revisions** | Typed Artifact ID, content hash/schema, project/document revision, producer attempt and validation receipt. | Compare-and-swap mutable manifests; accept immutable outputs by hash; stale/missing/revoked references block successors. Retaining every old artifact aids audit but raises storage/privacy deletion costs. |

## 8. Failure semantics, idempotency, atomicity and migration

### 8.1 Partial write and corruption policy

- **Append log:** length/frame or newline record plus sequence and checksum.
  A final incomplete frame can be quarantined/truncated with an audit event;
  malformed interior records, duplicate sequence numbers or broken hash chains
  make the projection incompatible/corrupt. General “skip malformed lines,” as
  seen in active Pi and Claude loaders, is insufficient for authority records.
- **Snapshot/manifest:** serialize and validate to a sibling temp, sync file,
  rename in the same directory, then sync the directory when filesystem
  durability is required. Store schema and monotonic revision; write through
  compare-and-swap. Action Guide is current positive source evidence [E:R2].
- **Artifacts:** write to a non-final path, validate/decode/hash, then publish.
  A completion event references only a published Artifact. Orphans are safe to
  collect after proving no committed record references them.
- **Cross-store transaction:** when no shared database transaction exists,
  choose an order and make reconciliation explicit. Starting an external Job
  before persisting its idempotency key is unsafe; persisting intent first lets
  Resume query/retry by that key.

### 8.2 Idempotency and exactly-once claims

Exactly-once external effects are not implied by exactly-once local event
insertion. Prefer:

- stable operation/idempotency keys generated before dispatch;
- at-least-once delivery plus deduplication at the side-effect owner;
- compare-and-swap for artifact/document apply;
- effect receipts and query-by-key for cloud Jobs;
- attempt numbers that never overwrite earlier evidence; and
- a terminal `unknown_effect`/`needs_reconciliation` rather than optimistic
  success after an ambiguous crash.

### 8.3 Schema migration

A candidate must define compatibility for Event, Snapshot, Transcript sidecar,
Artifact manifest and Job-handle schemas independently. Migration should be
idempotent, copy/transaction based, preserve unknown evidence where safe, and
run validation before publish. Pi's active V1→V3 and Action Guide V1→V2 paths
show concrete migrations; oh-my-pi adds an idempotency test. Rig explicitly has
no cross-version run-state guarantee. Claude's focused transcript roots exposed
no generic schema-version/migrator contract [A:L-MIG]. Codex contains multiple
serde compatibility paths, but a general Workflow schema and crash migration
were not established [G:C1].

## 9. Security, privacy and retention

- Transcript/Event logs can contain screenshot-derived text, prompts, tool
  arguments/results and provider metadata. Persist only what the selected
  recovery boundary needs; encrypt/restrict it under product policy.
- Durable decisions should store sanitized structured facts and opaque Artifact
  references rather than copying pixels/OCR/proposal prose into every store.
- A checkpoint or compact summary is untrusted content until matched to a host
  decision record. Text saying “approved” cannot grant capture, filesystem,
  network or publish authority.
- Retention/deletion must enumerate canonical log, snapshots/projections,
  Transcript, artifact blobs, temp/orphan files, indexes, backups, sidecars and
  remote Jobs. Deleting only the conversation does not delete derived artifacts;
  deleting only an Artifact can leave sensitive Transcript content.
- Corruption logs and tombstones must be useful without echoing private payloads.

## 10. Candidate Rollshot patterns and trade-offs

These patterns are inputs to later synthesis. No final selection is made.

### Pattern A — typed Task checkpoint Snapshot plus Artifact truth

Wrap one bounded Smart Redaction or Action Guide proposal in a Rollshot-owned
Task record. Atomically persist Task/attempt status, authorized input references,
provider/model/tool/config fingerprints, remaining/reconciled budget, source
generation or project revision, typed terminal, proposal Artifact reference and
review/handoff receipts. Artifact bytes and Action Guide state remain in their
product stores. No general Workflow/Event log or Job scheduler is added.

**Recovery:** load and validate the Snapshot; reconcile any `running` attempt as
`unknown`, check Artifact/document revisions, then route to retry, needs-user,
review redelivery, stale, complete or incompatible. Conversation Resume is an
optional separate feature.

**Advantages:** small conceptual surface, fast reconstruction, fits current
typed terminals/review and Action Guide's revision discipline, avoids making
Transcript canonical.

**Costs/trade-offs:** snapshot history/audit is limited unless attempts are
retained; atomic multi-store handoff and external side effects still need
receipts; does not represent Hyperframes dependencies/checkpoints/Jobs. Adding
unused graph fields would overcomplicate Smart Redaction.

### Pattern B — append-only Workflow journal + materialized Snapshot + Artifacts

For an adopted multi-stage workload, persist immutable Workflow events for Work
Item creation/readiness inputs, attempts, user checkpoint decisions, Job
intent/observations, Artifact publication and cancellation. Materialize a
versioned Snapshot for fast load; recompute readiness deterministically from
predecessors, checkpoint decisions and validated expected Artifacts. Store
Conversation transcripts separately and start fresh coordinator contexts from
the Workflow projection when useful.

**Recovery:** verify/replay from the last trusted Snapshot/log sequence,
reconcile every `running` attempt/Job with the side-effect owner, validate
Artifacts, then lease ready work serially or under a bounded cap.

**Advantages:** audit and selective retry, explicit gate authority, artifact-
based worker completion, supports long Jobs and fresh-context re-projection.

**Costs/trade-offs:** event/projection/schema design, cross-store reconciliation,
leases, migrations, log compaction, privacy/retention and larger adversarial
test matrix. This complexity is unjustified unless the deferred workload (or
another real product) proves it.

### Pattern C — Transcript/child sidecars as optional continuity only

Persist a branchable Transcript and small child identity/contract sidecars for
user-requested Conversation/Child-context Resume. Repair unresolved tool
fragments by marking them interrupted, rebuild under current policy, and bind
the child to the authoritative Task/Artifact revision. This can supplement A
or B but cannot advance Workflows or approvals by itself.

**Trade-offs:** preserves rationale and child productivity at the price of
sensitive retention, provider/model/skill drift and repair logic. It should be
omitted if workload evidence shows that artifact projection is sufficient.

### Preliminary fit without selection

| Pattern | Smart Redaction | Action Guide | Deferred brag + Hyperframes |
|---|---|---|---|
| A: Task Snapshot + Artifacts | Natural bounded recovery/handoff candidate; durable run recovery is not yet proven necessary, so value must be measured. | Fits independent revision-bound proposals and existing atomic project store. | Insufficient for dependencies, gates and long Jobs. |
| B: Workflow journal hybrid | Capable but substantially more state than this workload proves. | Useful only if multi-step orchestration becomes a real product need. | Strong semantic match if adopted, but highest implementation/operations cost. |
| C: Transcript/child sidecars | Optional user continuity; not proposal authority. | Optional rationale; project manifest remains canonical. | Helpful for coordinator/worker context, but artifacts/gates/Jobs remain authoritative. |

## 11. Measurable evaluation criteria and required spikes

| Dimension | Candidate measure |
|---|---|
| Crash coverage | Inject a crash before/after every durable boundary in Section 1; 100% reconstruction to one typed route, never optimistic completion from ambiguous state. |
| Decision safety | 0 approvals/permissions/checkpoints recreated from Transcript/summary; 100% side effects revalidate current authority and input revision. |
| Idempotency | Re-deliver every command/event at least twice; 0 duplicate document applies, renders, cloud Jobs or proposal handoffs. |
| Artifact integrity | 100% completed references resolve and validate; partial/missing/hash-mismatched outputs block successors and surface typed errors. |
| Revision safety | 100% stale Smart Redaction/Action Guide proposals and checkpoint decisions rejected after base Artifact revision changes. |
| Log/snapshot integrity | Detect every interior corruption, duplicate/out-of-order sequence and broken reference; recover only a deliberately injected incomplete tail. |
| Migration | Upgrade and downgrade fixtures across every supported schema; migrations are idempotent and a failed migration leaves the old record readable. |
| Job recovery | For acknowledged and ambiguous starts, 100% correct reattach/query/unknown routing; 0 blind duplicate starts. Measure reconnect success by outage duration and handle TTL. |
| Compatibility drift | Matrix of changed skill/provider/model/config/tool/permission sets; every case continues, restarts, or fails closed according to declared policy. |
| Performance | Resume p50/p95 latency, replay events, Snapshot size/write latency, fsync cost, storage growth and compaction frequency. |
| Privacy/deletion | Enumerate all derived stores; deletion/redaction reaches each declared derivative within SLA with 0 raw screenshot/OCR leakage to default logs. |
| Observability | UI state reconstructed from durable records after reconnect; transient event loss never changes terminal/progress truth. |

Required bounded spikes before synthesis selects a pattern:

1. **Smart Redaction handoff fault test:** implement only an in-memory fake store
   contract, crash at tool/evidence/terminal/review boundaries, and measure how
   often Snapshot A adds user value over clean restart.
2. **Action Guide power-loss/orphan audit:** fault the temp/rename/fsync and
   asset-before-manifest boundaries on supported filesystems; verify old-or-new
   manifest, orphan cleanup and stale revision behavior.
3. **External Job ambiguity spike:** fake start/query APIs with idempotency keys;
   crash before and after acknowledgement and prove no duplicate Job.
4. **Journal projection property test:** for Pattern B, permute duplicate events,
   truncate the tail and inject interior corruption; assert deterministic
   readiness/checkpoint/Artifact state.
5. Runtime-test reference recovery only if it becomes a dependency: Pi partial
   JSONL/migration, oh-my-pi power loss/child cold revival, Codex exec TTL/gaps,
   or Claude sidecar/Transcript corruption. Static profiles are insufficient.

## 12. Bounded negative audits and runtime gaps

All `rg` audits were run at the pinned revisions. “0 hits” means the named
pattern returned no output in the literal roots, not that another package or a
later revision cannot provide the capability.

**[A:R0] Rollshot durable agent Session/Run boundary.** Literal roots:
`crates/rollshot-agent/src/{domain,driver,runtime}.rs` and
`crates/rollshot-app/src/result_workspace/workbench/run.rs`. Case-insensitive
regex:
`impl\s+(Serialize|Deserialize)\s+for\s+AgentSession|serde.{0,40}AgentSession|AgentSession.{0,40}(save|persist|resume|recover)|Session(Store|Storage|Repository)|checkpoint|reattach|idempotenc`.
Result: **0 hits**. A durable `AgentSession`, checkpoint, Resume, recovery,
reattachment or idempotency contract was **not found in the investigated
scope**. Positive fresh-run source is [E:R1].

**[A:R1] Rollshot audit/Event log production path.** Literal roots:
`crates/rollshot-agent/src` and `crates/rollshot-app/src`. Regex:
`AuditEvent::|TurnComplete`. Result: declaration/test/UI mapping hits only:
`runtime.rs` declares `TurnComplete`, emits it in a test sink and constructs
`AuditEvent` variants in tests; the app maps `TurnComplete` to no activity.
Production `AuditEvent` construction/emission was **not found in the
investigated scope**. This is source classification, not a runtime event-loss
test.

**[A:R-WORK] Rollshot Task/Workflow recovery.** Task 7's exact roots were
`crates/rollshot-agent/src/{domain,driver,model,provider,runtime,tools}.rs`;
regexes targeted Task/Todo/Workflow/Job declarations and
`depends[_ -]?on|dependency|workflow[_ -]?id|job[_ -]?id|attempts?|retry|resume|checkpoint`.
No durable Product Task/Workflow/Job/recovery record was found; attempt hits
were current-run budget counters and retry prose/tests. This audit is reused,
not broadened to all Rollshot product code.

**[A:P-LIVE] Pi interrupted-operation state.** Literal roots:
`learn-projects/pi/packages/coding-agent/src/core/{session-manager,agent-session}.ts`
and docs `{sessions,session-format}.md`. Regex:
`pending.?tool|in.?flight.?tool|provider.?request|approval|permission|job.?id|process.?handle|reattach|idempotenc|retry.?safe|active.?run|run.?state`.
Result: **0 hits**. Durable active provider/tool/approval/Job state was **not
found in the investigated active coding-agent scope**. Harness planned work is
positive roadmap evidence [E:P2], not part of this absence.

**[A:P-ATOM] Pi active JSONL atomicity.** Literal file:
`learn-projects/pi/packages/coding-agent/src/core/session-manager.ts`. Regex:
`fsync|fdatasync|sync_all|atomic|rename`. Result: **1 hit**, the V2→V3 comment
“rename hookMessage role to custom”; no durability/atomic-write primitive hit.
Such a primitive was **not found in the investigated scope**. Source inspection
also finds `_rewriteFile` opening the target with `"w"`; no crash was injected.

**[A:O-JOB] oh-my-pi Job restart persistence.** Literal file:
`learn-projects/oh-my-pi/packages/coding-agent/src/async/job-manager.ts`.
Regex:
`serialize|deserialize|rehydrate|reattach|persist|session.?entry|from.?json|to.?json`.
Result: **0 hits**. Job serialization/rehydration/reattachment was **not found
in the investigated scope**. `resumeDeliveries` is positively inspected as a
same-manager delivery operation [E:O3].

**[A:O-LIVE] oh-my-pi live Session/Task state in persistence/revival.** Literal
roots: `packages/coding-agent/src/session/{session-manager,session-entries}.ts`
and `src/task/persisted-revive.ts`. Regex:
`acpPermissionDecisions|allow_always|reject_always|AbortController|retryPromise|pendingNextTurnMessages|provider.?stream|tool.?promise|task.?controller|activeEvalExecutions`.
Result: **0 hits**. Persistence/revival of the named live permission, stream,
promise/controller and execution fields was **not found in the investigated
scope**. Their live owners are positive source evidence [E:O4]; Job-specific
restart persistence remains the narrower [A:O-JOB] audit.

**[A:O-WORK] oh-my-pi Workflow routing.** Task 7's exact roots were
`packages/coding-agent/src/{task,async,goals}` plus `src/tools/todo.ts`; regex
targeted `dependsOn|depends_on|blockedBy|blocked_by|workflowId|workflow_id|DAG|next.?ready|readiness|scheduler`.
Only ordinary implementation-dependency prose appeared; a host-owned
Workflow/dependency/next-ready contract was **not found in the investigated
scope**.

**[A:C-LIVE] Codex Thread Resume live objects.** Literal roots:
`learn-projects/codex/codex-rs/thread-store/src` and
`codex-rs/core/src/session/rollout_reconstruction.rs`. Regex:
`pending_approvals|pending_request_permissions|pending_user_input|pending_elicitations|pending_dynamic_tools|ProcessHandler|process_id|background_terminal|CancellationToken|provider.?stream`.
Result: **0 hits**. Reconstruction of those Turn/process/provider live objects
by ThreadStore was **not found in the investigated scope**; separate positive
exec-server recovery is [E:C3].

**[A:C-RELAY] Codex relay durable recovery cursor.** Literal roots:
`learn-projects/codex/codex-rs/exec-server/src/relay.rs` and
`exec-server/src/noise_relay`. Regex:
`std::fs|tokio::fs|OpenOptions|File::|ThreadStore|SessionRegistry|checkpoint|snapshot|sqlite|database|persist|restore|rehydrate`.
Result: **0 hits**. A durable relay sequence/replay/checkpoint store was **not
found in the investigated scope**. Positive source inspection [E:C4] shows
fresh UUID stream IDs, zeroed Resume/ack cursor fields, live send counters and
the bounded in-memory Noise reorder buffer; this audit does not cover the
separate `SessionRegistry` process-recovery layer [E:C3].

**[A:L-ATOM] Claude Transcript atomicity.** Literal roots:
`learn-projects/claude-code-source-code/src/utils/{sessionStorage,json}.ts` and
`src/tools/AgentTool/resumeAgent.ts`. Regex:
`fsync|fdatasync|sync_all|atomic.?rename|temp.?rename|write.?temp`.
Result: **0 hits**. A file/directory sync or atomic Transcript publish contract
was **not found in the investigated scope**.

**[A:L-MIG] Claude Transcript migration.** Literal roots:
`src/utils/{sessionStorage,sessionRestore,json}.ts` and
`src/tools/AgentTool/resumeAgent.ts` under the pinned Claude checkout. Regex:
`schema.?version|session.?version|transcript.?version|migrat(?:e|ion).{0,40}(session|transcript|jsonl)|(session|transcript|jsonl).{0,40}migrat`.
Result: **0 hits**. A generic transcript schema-version/migration contract was
**not found in the investigated scope**. Optional legacy bridges and defaulted
fields are positive compatibility behavior but not that contract.

**[A:L-TEST] Claude focused recovery tests in external tree.** `git ls-tree`
at the pinned revision was filtered by exact regex
`(^|/)(sessionStorage|sessionRestore|resumeAgent|RemoteAgentTask|bridgePointer).*(test|spec)\.(ts|tsx)$`.
Result: **0 paths**. Focused test source for these recovery modules was **not
found in the investigated external tree**; visible source remains evidence.

**[A:L-RUNTIME] Claude generic Runtime Task recovery.** The Reviewed profile's
exact roots were `src/tasks`, `src/utils/task`, `src/tools/AgentTool`,
`src/utils/sessionRestore.ts`, and `src/utils/sessionStorage.ts`; regex
`(?:restore|resume)[A-Za-z]*(?:Task|Agent)|reattach|resurrect|sidecar`.
Hits showed local-agent Resume and remote-agent sidecars. A generic local shell,
teammate or arbitrary Runtime Task resurrection routine was **not found in the
investigated scope**.

**[A:SKILL] Durable invoked-skill version/authority.** Exact per-system roots:
Pi `core/{session-manager,skills}.ts`; oh-my-pi
`session/{session-manager,session-entries}.ts` plus
`task/persisted-revive.ts`; Codex `thread-store/src` plus
`core/src/session/rollout_reconstruction.rs`; Claude
`utils/{sessionStorage,sessionRestore}.ts` plus `AgentTool/resumeAgent.ts`.
Regex:
`invoked.?skill|skill.?version|skill.?snapshot|skill.?authority|skill.?package.?id|skill.?revision`.
Result: **0 hits in each system boundary**. A durable invoked-skill package
version/authority record was **not found in these persistence/recovery roots**.
This does not deny live skill discovery snapshots, compact attachments, or
skill files elsewhere.

Runtime/source gaps:

- **[G:R1]** Action Guide's atomic/revision/corrupt-asset tests were inspected,
  but no kill/power-loss/filesystem fault injection was run.
- **[G:P1]** Pi's focused coding-agent Session tests and Harness tests were not
  executed; active malformed-tail and direct rewrite crash behavior is static.
- **[G:O1]** oh-my-pi's file/SQL/Redis/memory storage, rewrite-race, checkpoint
  and child-revival tests were inspected selectively, not executed; multi-
  process, power-loss and backend schema-upgrade behavior remains unverified.
- **[G:C1]** Codex rollout/compaction reconstruction, relay/noise-relay and
  exec-server recovery tests were not executed. The focused audits did not
  establish corrupt-tail repair, non-local ThreadStore transactions,
  cross-version Workflow migration, durable relay replay or exec-server-process
  restart recovery.
- **[G:L1]** Claude server-side CCR/bridge behavior, GrowthBook/build gates and
  internal modules are unavailable or unexecuted in the pinned external tree.

## 13. Evidence index

Cross-capability provenance is fixed as follows: Task 7 is the Task/Todo/
Workflow comparison, Task 8 is context compaction, and Task 9 is memory. This
document reuses Task 7's Workflow audits [A:R-WORK, A:O-WORK] and Task 8's
compact-resume source route [E:C1]; it does not attribute Workflow evidence to
Task 9.

### Rollshot, Rig and workloads

- **[E:R1] Source + test source:**
  `crates/rollshot-agent/src/domain.rs` — `AgentSession`;
  `driver.rs` — `run_with_provider`, fresh `AgentRun::new`, terminals and
  provider/tool threading; `runtime.rs` — budget, draft evidence, events and
  cancellation; workbench run/state paths. Supports current live boundary and
  typed outcome. No provider/UI/restart run was performed.
- **[E:R2] Source + test source:**
  `crates/rollshot-action/src/project/{model,store,validate}.rs` —
  `ProjectManifestV1/V2`, `read_manifest`, `write_json_atomic`,
  `commit_noreplace`, save/load, revision/conflict, corrupt/missing/symlinked
  asset and migration tests. Supports Action Guide Artifact persistence; not
  agent history.
- **[E:R3] Pinned dependency source + tests:**
  `/home/noah/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rig-core-0.39.0/src/agent/run/{mod,streamed}.rs`
  — `AgentRun`, `RunState::ExecutingTools`, serde warning,
  `streamed_run_serde_round_trips_while_tools_pend`. Machine-local path is
  reproducible through the exact Cargo pin/checksum recorded in Round 0.
- **[W1]** Round 0 Smart Redaction workload and current workbench/agent source.
- **[W2]** Round 0 Action Guide workload and current project/proposal sources.
- **[W3]** Pinned brag `skills/brag/SKILL.md`: deferred product workflow and
  command surface. Deferred workload evidence, not Rollshot implementation.
- **[W4]** Pinned Hyperframes `production-loop.md`: stage/check/render and
  external render-loop boundaries. Deferred workload evidence, not Rollshot
  implementation.
- **[W5]** Pinned Hyperframes `subagent-dispatch.md`: expected-artifact worker
  completion and one clean re-dispatch. Deferred workload evidence, not
  Rollshot implementation.
- **[W6]** Pinned Hyperframes `review-loop.md`: review gates and artifact
  revision loops. Deferred workload evidence, not Rollshot implementation.

### Pi

- **[E:P1] Source/docs:** active coding-agent
  `packages/coding-agent/src/core/session-manager.ts` and
  `docs/{sessions,session-format}.md` — append/load/branch, malformed-line skip,
  V1→V3 migration, rewrite and checkpoint-label/compaction vocabulary.
- **[E:P2] Source/docs/test source, not active integration:**
  `packages/agent/src/harness/session/{jsonl-storage,jsonl-repo,session}.ts`,
  `packages/agent/docs/agent-harness.md`, and
  `packages/agent/test/harness/{storage,repo,session}.test.ts` — strict malformed
  entry tests and Planned semi-durable operation recovery.

### oh-my-pi

- **[E:O1] Source + test source:**
  `packages/coding-agent/src/session/{session-manager,session-storage,session-migrations}.ts`;
  tests under `test/session-manager/`, especially
  `rewrite-rename-eperm.test.ts` and `migration.test.ts`. Supports software-
  crash/no-fsync claim, fenced atomic rewrite, failure handling and migration.
- **[E:O2] Source + test source:**
  `src/tools/checkpoint.ts`, checkpoint/rewind handling and
  `#rehydrateCheckpointRewindState` in `src/session/agent-session.ts`, and
  `test/agent-session-checkpoint-rewind-branch.test.ts`. Supports conversation
  checkpoint recovery; tests were inspected, not executed.
- **[E:O3] Source:** `src/task/persisted-revive.ts`,
  `src/async/job-manager.ts`, and child registry/session-init contracts.
  Supports narrow child cold revival and process-local Job boundary.
- **[E:O4] Source:** `packages/coding-agent/src/session/agent-session.ts`,
  `packages/agent/src/{agent,agent-loop}.ts`, `src/task/index.ts`, and
  `src/async/job-manager.ts` — live ACP decision map, provider event stream,
  promises, abort controllers, queues and Job manager. The Reviewed profile's
  resume section explicitly excludes those live objects from Conversation
  Resume; [A:O-LIVE] bounds the persistence/revival absence.

### Codex

- **[E:C1] Source + test-source routes:** `codex-rs/thread-store/src`, notably
  `local/{mod,live_writer}.rs`; core
  `session/{mod,rollout_reconstruction}.rs`; compact-resume/fork tests cited by
  Task 8. Supports canonical JSONL, rebuildable SQLite and Thread reconstruction.
- **[E:C2] Source:** `codex-rs/core/src/state/{turn,session}.rs` — live pending
  channels/grants versus session history/settings.
- **[E:C3] Source + test source:**
  `exec-server/src/server/session_registry.rs`, `client_recovery.rs`,
  `client.rs`, protocol `resume_session_id`/`after_seq`, and
  `exec-server/tests/process.rs::exec_server_resumes_detached_session_without_killing_processes`.
  Supports TTL-bounded acknowledged-process recovery, not server restart.
- **[E:C4] Source + test source:** `exec-server/src/relay.rs`,
  `src/noise_relay/{harness,executor_stream,ordered_ciphertext}.rs`, their
  focused tests, and
  `exec-server/tests/relay.rs::deferred_noise_environment_connects_and_reconnects_with_fresh_bundle`.
  Supports stream-ID route claim, zero-based live sequencing, bounded
  duplicate/gap handling and fresh authenticated reconnect. It does not
  establish durable relay replay or exec-session/Thread reconstruction.

### Claude Code source

- **[E:L1] Source:** `src/utils/{sessionStorage,sessionRestore,json}.ts` —
  batched append, JSONL malformed-line behavior, branch/metadata restore and
  heterogeneous sidecars.
- **[E:L2] Source:** `src/tools/AgentTool/resumeAgent.ts` — fragment repair,
  metadata/worktree fallback, current agent/tool/model/permission reconstruction
  and new async child registration.
- **[E:L3] Source:**
  `src/tasks/RemoteAgentTask/RemoteAgentTask.tsx`, remote metadata helpers in
  `sessionStorage.ts`, and `src/bridge/{bridgePointer,replBridge}.ts` — identity
  sidecar, authoritative remote status query, pointer TTL/schema and gated
  Transport Resume.

## 14. Confidence and limitations

Confidence is **high** for visible state ownership, persistence call order,
record fields, current default/gated distinctions, exact negative audits and
the separation of Resume boundaries. Confidence is **medium-high** for source
behavior backed by focused tests that were inspected but not run; **medium**
for cross-file crash consequences and candidate recovery inferences; and
**low** for power-loss durability, deployed remote services, non-local storage,
server-controlled gates, hidden Claude modules and provider/model compatibility
because they were not exercised.

The knowledge graph was queried first. It had no registered repositories in
the tool registry and returned zero nodes for focused Rollshot persistence,
project-save and stale-proposal queries; ignored `learn-projects` were likewise
uncovered in the Reviewed profiles. Bounded direct source/test inspection was
therefore required. Static inspection is not runtime proof, and every “not
found” statement is limited to its named roots and regex.

The comparison deliberately leaves these decisions open for synthesis:

1. whether Smart Redaction benefits enough from durable Task recovery to
   justify any persistence beyond proposal/project handoff;
2. whether Action Guide needs shared orchestration rather than its product-
   owned project plus independent bounded proposals;
3. whether the deferred media workload becomes real enough to justify a
   Workflow journal and durable Job layer;
4. whether Transcript/child continuity provides user value worth its privacy
   and compatibility surface; and
5. which store, retention and authority owner should hold any selected record.
