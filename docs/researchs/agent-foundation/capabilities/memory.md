# Agent memory boundaries

Status: Round 2 capability comparison; candidate space only

Research date: 2026-07-22 (Asia/Taipei)

Rollshot revision inspected: `9e333035e450cc8df4aeacde2be086457e97ec08`

Pinned comparison revisions: Pi
`dd6bea41efa8caa7a10fe5a6401676dc5699f83f`; oh-my-pi
`7b141199d524b859c357fc89654f10b62b9f3df1`; Codex
`4a443994bd12f49f2f08b21a2f224d9d42b9e734`; Claude Code
`2ca5ddabfed5f220812ea11f029eda03b21bc4c1`.

This document compares memory boundaries. It does **not** choose a Rollshot
architecture. The data classes below are deliberately separated from task
state, transcript persistence, context compaction, and image/project artifacts.

## 1. Question and workload boundary

The relevant product question is not simply “should Rollshot have memory?” It
is which facts may survive which boundary, who may write and retrieve them, and
how a user can inspect, correct, expire, redact, or delete them.

The baseline workloads impose three different demands:

- **Smart Redaction** is a bounded proposal-and-review run over sensitive
  screenshot content. It needs run-local facts and durable review state, but
  does not itself require semantic memory. [E:R1, E:W1]
- **Action Guide** has durable project frames, editable steps, revisions, and
  proposal provenance. Those are project artifacts and workflow state, not
  memories inferred from a conversation. [E:W2]
- Deferred **brag / Hyperframes** work depends on named artifacts and explicit
  checkpoints. Recovery must not depend on a model recalling prior prose.
  [E:W3]

## 2. Terms and hard data-class boundaries

“Session memory” is overloaded in the investigated systems. This comparison
uses the following narrower vocabulary.

| Data class | Definition | Correct home | Why it is not another class |
| --- | --- | --- | --- |
| **Run memory** | Ephemeral scratch facts used by one bounded agent run: current intent, tool results, candidate edits, and model/tool threading. | In-memory run state; discard at terminal state. | It is not a transcript merely because some messages are involved, and it is not reusable memory. |
| **Session transcript** | The canonical ordered conversation/tool-event record used to resume or audit a session. | Transcript/session store with its own retention controls. | Persistence does not turn every utterance or screenshot into a reusable fact. |
| **Project memory** | Curated, reusable knowledge scoped to one project and retrieved in later sessions. | A separately governed memory store. | It is not authoritative project state and must yield to current code/artifacts. |
| **User memory** | A user preference or fact intended to apply across projects. | User-scoped store with explicit control and stricter privacy rules. | Copying a project fact to a global directory is not a typed user scope. |
| **Team memory** | Shared organizational knowledge with membership, synchronization, and conflict semantics. | Team-scoped service/store. | A shared filesystem path is not sufficient authority or deletion semantics. |
| **Consolidation memory** | A derived index or synthesis over eligible source memories/transcripts. | Derived memory plus source lineage. | It cannot replace its sources or silently become authoritative. |
| **Compacted context** | A model-visible projection used to fit an active conversation within a context budget. | Transcript-owned derived projection. | It is not a durable truth store; Task 8 establishes this boundary. [E:K1] |
| **Workflow state** | Task phase, approval/gate, revision, budget, cancellation, proposal status, and authority. | Typed workflow/state store. | Relevance retrieval cannot safely reconstruct gates or authorization. |
| **Artifact** | Screenshot, frame, rendered result, proposal payload, guide, or other named product output. | Artifact/project storage with content policy. | An image or document remains an artifact even when a memory points to it. |

A memory record may refer to a transcript entry or artifact, but the reference
does not absorb the source bytes. A safe minimum lineage shape would name the
scope, writer, source reference, creation/update time, sensitivity class,
schema version, and expiry/deletion state. That is a candidate criterion, not
an assertion that Rollshot already implements it.

## 3. Capability status at a glance

Labels distinguish product defaults from optional or gated source. “Source”
means a defining implementation/configuration was inspected; “callsite/test”
means wiring or behavioral coverage was also inspected. Negative findings are
bounded by the audits in Section 14.

| System | Run memory | Session / transcript | Project memory | User memory | Team memory | Consolidation | Evidence character |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **Rollshot** | Default, in-memory `AgentRun` plus current `AgentSession`. | In-memory `AgentSession` exchanges; no serialization or product return path observed. | Missing — **not found in the investigated scope**. [A:R0] | Missing — **not found in the investigated scope**. [A:R0] | Missing — **not found in the investigated scope**. [A:R0] | Missing — **not found in the investigated scope**. [A:R0] | Source plus runner/workbench callsites and tests. [E:R1, E:R2] |
| **Pi coding-agent** | Default in-memory agent state. | Default JSONL session tree; `--no-session` disables persistence; picker can resume and delete. | Built-in semantic project memory is **not found in the investigated scope**. [A:P0] | **Not found in the investigated scope**. [A:P0] | **Not found in the investigated scope**. [A:P0] | **Not found in the investigated scope**. [A:P0] | Source/docs plus session tests in the reviewed profile. [E:P1, E:P2] |
| **oh-my-pi** | Default Pi-lineage in-memory run state. | Default persisted JSONL session; unpersisted sessions are excluded from memory extraction. | Optional local backend, default off; path is encoded from `cwd`. | A typed user scope is **not found in the investigated scope**. [A:O0] | A typed team/shared scope is **not found in the investigated scope**. [A:O0] | Optional local two-phase extraction/consolidation, default off with memory. | Source, configuration, commands, and focused storage/consolidation tests. [E:O1, E:O2, E:O3, E:O4] |
| **Codex** | Default thread/turn runtime state. | Default canonical rollout JSONL plus SQLite metadata where available. | Memory feature is stable but default off; the inspected store is Codex-home/global with `cwd`-aware inputs, not a declared typed project store. | Typed user memory is **not found in the investigated scope**. [A:C0] | Typed team memory is **not found in the investigated scope**. [A:C0] | Default-off two-phase global consolidation; read/use and generation can be separately gated. | Source, app-server wiring, state queries, prompts, and focused tests. [E:C1, E:C2, E:C3, E:C4, E:C5] |
| **Claude Code** | Default live query/app state. | Persistent session history is separate from auto memory. | Auto memory is implemented and default on for eligible ordinary sessions; project/git-root keyed. | Implemented typed `user` memory inside the project memory corpus; this is a semantic category, not proven cross-project user scope. [E:L1] | Implemented but feature/server/OAuth gated team memory and sync. [E:L4] | Direct main-agent memory writes are default; background extraction and nightly/dream-style consolidation are separately gated. | Source and visible callsites; server-controlled gates and missing internal modules limit runtime certainty. [E:L1, E:L2, E:L3, E:L4] |

Two status nuances matter. First, Codex’s `MemoriesConfig` defaults inside the
feature do not make the feature default-on: the `memories` feature registry is
stable but disabled by default. Second, Claude Code’s `user` type describes
what a memory is about; its inspected path is still project keyed, so this is
not evidence of a cross-project user database.

## 4. Current Rollshot boundary

### 4.1 What exists

`AgentSession` owns a `session_id`, completed text exchanges, and a pending user
message. It derives `Clone`/`Debug`, not serialization. `run_with_provider`
pushes the current user message and starts a fresh Rig `AgentRun`; it does not
replay earlier `AgentSession::exchanges()` into that new run. The workbench
moves the session into a spawned task and emits a terminal result without
returning or persisting the session. This is an in-memory bookkeeping boundary,
not durable session memory. [E:R1]

`AuthorizedModelInput` bounds attachment count, dimensions, and encoded bytes;
its `Debug` output redacts user text and attachment bytes. Visual-annotation
runs may take authorized screenshot attachments. The Smart Redaction request
currently supplies `attachments: vec![]`, even when the workbench authorizes a
`FullScreenshot` payload. This is a useful minimization property, but not a
general memory/privacy policy. [E:R2]

### 4.2 What is absent

No reusable memory record, scoped store, retrieval policy, expiry policy,
consolidator, or user/team memory boundary was found in the focused agent
sources: **not found in the investigated scope**. [A:R0]

That absence is not a defect for the current workloads. The dangerous move
would be to reuse `AgentSession`, compaction text, or project artifacts as an
implicit memory layer without adding explicit lifecycle and privacy semantics.

## 5. Per-system lifecycle comparison

### 5.1 Pi: transcript persistence without built-in semantic memory

Pi’s coding agent auto-saves a JSONL session tree under its agent sessions
directory unless `--no-session` is used. The picker supports resume and delete
(trash where available). Entries include messages, images encoded in message
content, tool output/details, compaction records, and custom entries; branch
selection reconstructs active context. [E:P1, E:P2]

This is strong transcript persistence and weak evidence for semantic memory:
the bounded memory audit returned zero hits. Pi’s session deletion is therefore
not proof of memory-level selective deletion, expiry, consolidation, or
relevance retrieval. [A:P0]

### 5.2 oh-my-pi: optional project-local derived memory

The local memory backend is disabled by default. When selected, it encodes the
current working directory into a project memory root, injects bounded guidance,
and exposes `/memory view`, `stats`, `diagnose`, `clear`, and `enqueue` /
`rebuild`. The backend is explicitly `searchable: false`: normal recall is
bounded file/index injection, not a structured semantic search service.
[E:O1, E:O3]

Startup Phase 1 claims eligible prior persisted sessions, excluding the current
thread, subagents, and unpersisted sessions. It keeps selected user/assistant
and bounded tool-result content, extracts raw memory/rollout summary/slug, and
redacts recognized secret patterns. Phase 2 serializes per-project
consolidation, uses leases/heartbeats, writes `MEMORY.md`, summaries, and
generated skills, and prunes unretained generated derivatives. Defaults include
a 30-day source-age window, 12-hour idle threshold, 64 startup rollouts, and a
5,000-token injection cap. These are eligibility/selection bounds, not a
documented end-to-end deletion guarantee for every derivative. [E:O1, E:O2]

Raw and rollout-summary material retains `thread_id`/update metadata; a
consolidated statement can lose one-to-one attribution unless the model emits
it. The read-path prompt says current repository/user instructions win and
memory alone is not proof. A manual `learn` tool appends bounded, deduplicated
lessons after secret redaction and delimiter neutralization; the read path
neutralizes again. This mitigation is narrower than general prompt-injection,
PII, or screenshot-content filtering. [E:O2, E:O4]

### 5.3 Codex: default-off global consolidation with citations

The stable `memories` feature is default off. App-server wiring installs the
extension but enables use only when both the feature and configuration permit
it. Within the feature, generation/use default true, while dedicated memory
tools remain optional. [E:C1]

Phase 1 is root-session-only, requires a state database, claims eligible
interactive rollouts under age/idle/lease limits, extracts structured memory,
and redacts recognized secrets. Phase 2 holds a global lock, selects sources by
usage/freshness, creates a git-backed workspace, invokes an internal
consolidation agent, and replaces/prunes derived files. The internal agent
disables memory, apps, MCP, plugins, collaboration, and network in the managed
path; a broader preselected external/disabled sandbox profile is preserved, so
“always sandboxed” would overstate the source. [E:C2, E:C4]

Read guidance searches `MEMORY.md`, summaries, skills, and optional ad-hoc
notes; it warns that memory may be stale and requires source citations when it
changes the plan. Citation parsing and usage attribution preserve rollout
references. An explicit clear operation empties both memory roots and rejects a
symlinked root. Dedicated tools support bounded list/read/search and adding an
ad-hoc note; a selective per-derived-fact delete operation is **not found in
the investigated scope**. [E:C3, E:C5, A:C0]

`disable_on_external_context` can mark threads polluted after external web/MCP
activity; Phase 1 claims only enabled rows. This is a meaningful poisoning
gate, but it is optional and does not validate factual truth. Usage and
`max_unused_days` influence selection/pruning; they are not proof that all
copies, citations, source transcripts, or already derived statements expire
together. [E:C4]

### 5.4 Claude Code: default project auto memory plus gated sharing

Eligible ordinary sessions load project-keyed auto memory by default. Trusted
settings select a path based on the canonical git/project root; project-local
settings cannot redirect the store, and worktrees share a canonical project
memory root. `MEMORY.md` is a bounded index (200 lines / 25 KiB) with topic
files. Types are `user`, `feedback`, `project`, and `reference`; prompts
explicitly exclude current-task state, derivable code, git history, and
ephemeral conversation. [E:L1]

The main agent can write memory directly. A separate, feature-gated background
extractor may scan the session and write with a restricted memory-only tool
set; it skips when the main agent already wrote memory. Retrieval scans bounded
file headers, selects up to five relevant memories, avoids already surfaced
items, and tells the model to verify stale facts against current sources. A
one-day freshness warning does not delete or expire content. [E:L2, E:L3]

The user can explicitly ask to remember, ignore, update, or forget. Automatic
expiry/retention declarations are **not found in the investigated scope**.
[A:L0] Team memory has traversal/symlink containment and a high-confidence
secret guard before sync, but requires a compile/feature path, server gate, and
OAuth-backed synchronization. The equivalent team secret guard was not found
on the private auto-memory write path; that is a bounded code observation, not
proof that no upstream guard exists. [E:L4]

## 6. Owner, writer, reader, retention, deletion, retrieval, provenance

| System / store | Owner and writers | Readers / retrieval | Persistence and retention | Deletion / user control | Provenance |
| --- | --- | --- | --- | --- | --- |
| Rollshot run/session | `AgentRunner` and workbench task write; model/tool loop reads current run. | Direct in-memory access; no relevance retrieval. | Process/task lifetime only in inspected path. | Terminal drop; no surfaced session control. | Session/run IDs and exchanges, but no durable source lineage. [E:R1] |
| Pi JSONL session | Coding-agent `AgentSession` / `SessionManager`; extensions may add custom entries. | Resume, branch reconstruction, active-context projection. | Default append-only JSONL; optional `--no-session`. | Picker delete/trash; user can avoid persistence. | Entry IDs/parents and typed records preserve conversation-tree lineage. [E:P1, E:P2] |
| oh-my-pi local memory | Phase 1 extractor, Phase 2 consolidator, and explicit `learn` writer. | Bounded index/files plus startup guidance; local backend reports non-searchable. | Project root; age/idle eligibility and derivative pruning, but full derivative expiry is **not found in the investigated scope**. [A:O1] | `/memory clear`; enqueue/rebuild; backend can remain off. | Raw/summary artifacts name thread/update; consolidated claim-level lineage may be lossy. [E:O1, E:O2, E:O3, E:O4] |
| Codex memories | Phase 1/2 internal agents; explicit ad-hoc note tool. | Prompt-guided index/search/read; optional dedicated tools; citations feed usage. | Codex-home roots plus state DB; selection by source age/idle/use and stale derivative pruning. | Feature generation/use can be disabled; clear empties roots; selective fact delete is **not found in the investigated scope**. [A:C0] | Rollout summaries, citations, source paths, usage counts. [E:C2, E:C3, E:C4, E:C5] |
| Claude project auto memory | Main agent by default; gated extractor; user instructions guide edits. | Bounded scan + model selection of up to five; stale/current-source rules. | Project/git-root files; no automatic expiry declaration: **not found in the investigated scope**. [A:L0] | Explicit remember/ignore/update/forget; settings/env can disable. | File/topic descriptions; no uniform source-transcript citation contract found in inspected auto-memory path. [E:L1, E:L2, E:L3] |
| Claude team memory | Gated team writer/sync service and authorized members. | Team-memory attachment/sync path when gates and OAuth permit. | Shared synchronized project/team store; server behavior not runtime-tested. | Forget/delete can flow through files/sync; complete deletion SLA is **not found in the investigated scope**. [A:L1] | Repo/team path and sync metadata; exact remote lineage semantics remain gated/partially external. [E:L4] |

## 7. Privacy, poisoning, expiry, and redaction

### Privacy and screenshot-sensitive risk

Session stores in Pi-class systems can contain image bytes and tool output.
Memory extractors in oh-my-pi and Codex consume transcript-derived content.
Secret regexes reduce credential leakage but do not identify faces, health or
financial information, arbitrary PII, confidential UI text, or the intent
behind a screenshot. Claude Code’s team path adds a high-confidence secret
scanner, but its private auto-memory path does not establish a screenshot-safe
policy. [E:P2, E:O2, E:C2, E:L4]

For Rollshot, the safe baseline is therefore:

- raw screenshot pixels, crops, OCR text, thumbnails, and model attachments are
  artifacts or run inputs, never memory by default;
- a memory candidate derived from an image would require an explicit purpose,
  typed sensitivity, source reference, preview, and user-controlled acceptance;
- redaction outputs and “this region is sensitive” proposals remain document
  operations/workflow state; they must not silently teach cross-session memory;
- deleting an artifact must not leave an untracked semantic paraphrase that
  reveals its content.

### Poisoning and authority

All derived memory is untrusted input. oh-my-pi neutralizes delimiters for the
manual learned-lessons path and says repository/user instructions win. Codex
can exclude externally polluted threads and requires citations. Claude prompts
say current sources win and bounded retrieval avoids indiscriminate injection.
None makes memory authoritative workflow state. [E:O4, E:C3, E:C4, E:L2]

Rollshot memory, if pursued, must be unable to grant tool authority, approve an
edit, advance a gate, change an artifact revision, enlarge a budget, or bypass
current policy. Retrieved text must be labeled and provenance visible to both
policy code and the model.

### Expiry, redaction, and deletion

Age windows often govern *eligibility* or *retrieval*, not erasure. File
pruning can remove a derivative while its source transcript survives; clearing
memory can leave artifacts or transcripts; deleting a source can leave a
consolidated paraphrase. These operations require separate, testable semantics.

At minimum, a future design would have to specify:

1. source retention independently for transcript and artifact;
2. memory-record expiry and retrieval suppression;
3. consolidation invalidation when a source is redacted/deleted;
4. cascade behavior for citations, indexes, embeddings, caches, backups, and
   synced team copies;
5. user-visible list, inspect, correct, forget, clear-scope, and disable controls;
6. an auditable tombstone or deletion receipt that does not preserve the
   sensitive content itself.

## 8. Explicit mapping for Rollshot data

| Rollshot datum | Classification | Persistence expectation | Memory treatment |
| --- | --- | --- | --- |
| Current request, active model/tool messages, transient OCR/model result | Run memory | Discard at terminal state unless separately promoted. | Never auto-promote whole payload. |
| `AgentSession` exchanges | Session transcript candidate | Currently in memory only; a future transcript store needs its own policy. | Source may be eligible only under explicit filtering; not memory itself. |
| Run status, terminal reason, budgets, cancellation | Workflow state | Durable only where recovery/audit requires it. | Never reconstructed from memory. |
| Proposal status, review decision, base revision, stale-proposal rejection | Workflow state | Durable with document/project revision. | Memory may explain, never authorize or mutate. |
| Screenshot/frame/crop/thumbnail/OCR boxes | Artifact | Project/artifact policy, potentially encrypted and short-lived. | Pixels/text excluded by default. |
| Image-document operations and flattened render | Artifact/document history | Durable when the user saves/project flow requires it. | Not semantic memory. |
| Action Guide manifest, frames, steps, ordering, revision | Artifact plus workflow state | Durable project state. | A memory may link to the current manifest, never replace it. |
| Compact summary / retained turns / reconstruction metadata | Compacted context | Derived from canonical transcript; validity is session-specific. | Never imported as durable memory solely because it is concise. [E:K1] |
| Explicit reusable preference (“always export guides as …”) | Candidate user/project memory | Only after scope and consent are resolved. | Typed record with lineage, correction, expiry, and deletion. |
| Reusable verified project convention | Candidate project memory | Opt-in/accepted and source-linked. | Current repository/project state overrides stale memory. |
| Deferred brag / Hyperframes completion evidence | Named artifact/checkpoint | Durable workflow/artifact storage. | Memory can point to evidence; completion cannot depend on recall. |

## 9. Workload traces

### Smart Redaction

1. Screenshot/crop is an authorized run input/artifact.
2. The bounded agent proposes typed redaction operations.
3. Proposal, base revision, validation, and review decision are workflow state.
4. Accepted operations become image-document history/artifact state.
5. Run transcript follows transcript policy.
6. No screenshot pixels, OCR text, sensitive-region description, or proposal is
   written to cross-session memory by default.

Measurable safety target for any candidate: zero raw screenshot bytes and zero
OCR spans in memory under the default Smart Redaction flow; deterministic
invalidation after source deletion; zero retrieved memories capable of changing
review authority.

### Action Guide

1. Frames and semantic events enter the project artifact/engine boundary.
2. The guide model owns deterministic steps and revision.
3. Agent suggestions are proposals tied to that revision.
4. User/project preferences may become explicit memory only through a separate
   promotion action.

Measurable target: restart recovery reconstructs the exact guide from manifest,
frames, and revisions with memory disabled; stale proposals are rejected
identically whether memory is on or off.

### Deferred brag / Hyperframes

1. A named artifact records the deliverable.
2. A checkpoint records dependencies, gates, and completion evidence.
3. Session compaction may project recent discussion.
4. Memory may retrieve a preference or link, but cannot prove completion.

Measurable target: a clean process with transcript/memory unavailable can
determine checkpoint status from authoritative artifacts/workflow records.

## 10. Alternatives

No alternative is selected in this comparison.

### Alternative A — no reusable semantic memory

Keep run state ephemeral; add transcript persistence only if product recovery
requires it; keep project/workflow/artifact state authoritative. Users re-state
preferences when needed.

Benefits: smallest privacy and poisoning surface; deletion boundaries are
clearer; current workloads remain supportable. Costs: repeated instructions,
no cross-session personalization, and no learned project convention retrieval.

Success criteria: all three workload traces recover correctly without memory;
no sensitive screenshot-derived semantic residue; transcript retention and
deletion are independently usable.

### Alternative B — opt-in, project-scoped accepted memories

Add a small typed project store. Only explicit user acceptance or a clearly
marked “remember for this project” action writes it. Records contain scope,
writer, source reference, sensitivity, timestamps, schema version, and optional
expiry. Retrieval is bounded and citation-bearing; current project sources win.

Benefits: solves repeated project conventions without global profiling; easier
to inspect and clear than automated consolidation. Costs: user friction,
curation burden, stale facts, migration work, and a new privacy surface.

Success criteria: 100% of records have lineage; zero cross-project retrieval;
100% list/edit/forget coverage; deletion invalidates indexes/caches within a
defined SLA; poison tests cannot modify gates or permissions; retrieval p95 and
token overhead remain within an agreed budget.

### Alternative C — layered automatic consolidation

Build eligibility filtering over retained transcripts, extract candidate
project/user facts, consolidate them, and optionally add separately governed
team sync. User, project, and team scopes are distinct stores; consolidation is
derived and source-linked. Screenshot-derived inputs remain excluded unless an
explicit high-sensitivity flow is designed.

Benefits: lowest repeated-instruction burden and richer long-horizon recall.
Costs: largest privacy, poisoning, provenance, expiry, synchronization,
explainability, and operational surface; model-generated consolidation can
erase nuance or lineage. Team memory additionally needs identity, membership,
conflict, revocation, and remote deletion contracts.

Success criteria: extraction precision/recall on an approved corpus; zero
secret/PII/screenshot leakage in adversarial fixtures; bounded false-memory and
poison acceptance rates; complete provenance from consolidated claim to source;
scope-isolation tests; source-redaction cascade; measurable retrieval latency,
token cost, storage growth, and deletion SLA.

## 11. Shared non-goals

The alternatives do not authorize any of the following:

- using memory as the canonical task, approval, budget, revision, or checkpoint
  store;
- treating compacted context as long-term memory;
- saving screenshots, crops, OCR text, tool dumps, or complete transcripts as
  semantic memory by default;
- inferring team sharing from a common filesystem path;
- granting tools or permissions from retrieved text;
- claiming a deletion guarantee before transcript, artifact, derived index,
  cache, backup, and sync semantics are specified;
- adding a vector database or consolidation agent before a concrete workload
  demonstrates that explicit project state and curated records are insufficient;
- choosing among Alternatives A-C in this research round.

## 12. Decision dimensions and focused spikes

| Dimension | Measure before selection |
| --- | --- |
| User value | Repeated-instruction rate and task success with memory disabled versus explicit project records. |
| Recall quality | Precision/recall, stale-fact rate, source-verification rate, and “no relevant memory” calibration. |
| Privacy | Secret, PII, screenshot/OCR, and cross-project leakage on adversarial fixtures. |
| Poison resistance | Whether retrieved content can alter system policy, approvals, budgets, revisions, or tool authority; target zero. |
| Provenance | Percentage of retrieved/consolidated claims traceable to an inspectable source; target 100% for accepted records. |
| Expiry/deletion | Time to stop retrieval and remove every declared derivative; orphan-derivative count after source deletion. |
| User control | Discoverability and correctness of list, inspect, edit, forget, clear-scope, export, and disable. |
| Performance | Retrieval p50/p95 latency, prompt tokens, storage growth, consolidation provider cost, startup impact. |
| Recovery | Smart Redaction, Action Guide, and Hyperframes state recovers with memory disabled. |

Useful bounded spikes, if a later round authorizes them, are: classify real
Rollshot data without retaining image bytes; prototype an in-memory typed record
and deletion cascade; evaluate bounded lexical retrieval before semantic search;
and run a poisoning corpus against policy/gate isolation. None requires shipping
a memory architecture.

## 13. Confidence and limitations

Confidence is **high** for Rollshot’s current in-memory boundary and positive
memory mechanics directly cross-checked in source; **medium-high** for default
and gate classifications cross-checked against configuration/callsites/tests;
**medium** for bounded absence, provenance loss, and security implications; and
**low** for live provider quality, remote team synchronization, server-controlled
gates, crash behavior, and deletion across backups because those were not run.

The comparison is static. No provider request, memory extraction/consolidation,
restart, race, sync, or deletion cascade was executed. The knowledge graph had
Rollshot coverage but returned zero results for the initial broad memory/workload
queries and does not cover ignored `learn-projects`; bounded direct source
inspection was therefore required. Source wins over historical documents.

The status “not found in the investigated scope” is not a universal claim about
a product or later revision. It means only that the exact roots and expressions
below did not expose the asserted abstraction.

## 14. Bounded absence and negative audits

All audits were case-insensitive `rg -n -i` unless noted. Exit 1 with empty
output means zero matches.

**[A:R0] Rollshot semantic-memory boundary.** Literal roots:
`crates/rollshot-agent/src/domain.rs`, `driver.rs`, `model.rs`, `provider.rs`,
`runtime.rs`, and `tools.rs`. Regex:
`semantic.?memory|memory.?service|memory.?store|memory.?record|memory.?retriev|retrieval.?policy|memory.?expir|memory.?delet|project.?memory|user.?memory|team.?memory|cross.?session.?memory|memory.?consolidat`.
Result: **0 hits**. Project/user/team/consolidation memory is **not found in the
investigated scope**.

**[A:P0] Pi semantic-memory boundary.** Literal roots:
`learn-projects/pi/packages/agent/src`,
`learn-projects/pi/packages/coding-agent/src/core`, and coding-agent docs
`sessions.md`, `session-format.md`, `extensions.md`, `compaction.md`,
`security.md`. Same regex as [A:R0]. Result: **0 hits**. Built-in semantic
project/user/team/consolidation memory is **not found in the investigated
scope**; JSONL sessions and compaction remain separately evidenced positives.

**[A:O0] oh-my-pi typed scope declarations.** Literal roots:
`learn-projects/oh-my-pi/packages/coding-agent/src/memories`,
`src/memory-backend`, `src/internal-urls/memory-protocol.ts`, and
`learn-projects/oh-my-pi/docs/memory.md`. Regex:
`user.?memory|team.?memory|shared.?memory|global.?memory|memory.?scope`.
Result: **0 hits**. Typed user/team/shared/global scopes are **not found in the
investigated scope**; project scope is positively established by `getMemoryRoot`
and its `cwd` encoding, not by this absence query.

**[A:O1] oh-my-pi automatic expiry declaration.** Same literal roots as
[A:O0]. Regex:
`expires?|expiry|expiration|retention|time.?to.?live|\bttl\b|automatic.?delet|auto.?delet|delete.?after`.
Result: **1 hit**, `docs/memory.md:40`, whose command table says enqueue/rebuild
forces “consolidation/retention work.” No TTL/expiry/deletion declaration
appeared in source: full derivative expiry is **not found in the investigated
scope**. Positive max-source-age and pruning behavior is cited separately.

**[A:C0] Codex typed scope/selective deletion.** Literal roots:
`learn-projects/codex/codex-rs/memories`,
`state/src/runtime/memories.rs`, `core/src/memory_usage.rs`, and
`codex-api/src/endpoint/memories.rs`. Regex:
`user.?memory|team.?memory|shared.?memory|project.?memory|memory.?scope|selective.?delet|delete.?memory|remove.?memory`.
Result: **3 hits**: `memories/README.md:157` calls Phase 2 global artifacts
“shared”; two `write/src/workspace.rs` hits concern removing the temporary
workspace diff and explicitly not deleting memory content. A typed user/team
scope and selective per-fact delete are **not found in the investigated scope**.
Global consolidation and whole-root clear are separately evidenced positives.

**[A:L0] Claude Code automatic expiry.** Literal roots:
`learn-projects/claude-code-source-code/src/memdir` and
`src/services/extractMemories`. Regex:
`expires?|expiry|expiration|retention|time.?to.?live|\bttl\b|automatic.?delet|auto.?delet|delete.?after|max.?age`.
Result: **0 hits**. Automatic expiry/retention is **not found in the investigated
scope**; `memoryAge.ts` freshness warning and explicit forget are positive but
not automatic deletion.

**[A:L1] Claude Code team deletion completeness.** Literal positive scope:
`src/memdir/teamMemPaths.ts`, team-memory prompt/tool files, and
`src/services/teamMemorySync`. The visible source establishes gated sync and
file operations, but a remote deletion SLA, backup semantics, and all-copy
receipt are **not found in the investigated scope**. This is an evidence gap,
not a zero-hit regex claim.

## 15. Evidence index

### Rollshot and workload evidence

- **[E:R1]** `crates/rollshot-agent/src/domain.rs` — `AgentSession`,
  `CompletedExchange`; `crates/rollshot-agent/src/driver.rs` —
  `run_with_provider`, fresh `AgentRun::new`, exchange tests;
  `crates/rollshot-app/src/result_workspace/workbench/run.rs` — session moved
  into the spawned run and terminal handoff.
- **[E:R2]** `crates/rollshot-agent/src/domain.rs` — `AuthorizedModelInput`
  validation/redacted `Debug`; `driver.rs` — Smart Redaction empty attachments
  and visual-annotation `take_model_attachments`; workbench full-screenshot
  authorization callsite.
- **[E:W1]** `docs/researchs/agent-foundation/00-rollshot-baseline-workloads.md`
  — Smart Redaction bounded review workload; current driver/registry tests.
- **[E:W2]** Same baseline — Action Guide `ProjectManifestV2`, frame/step
  revision, provenance, and stale-proposal constraints; current action/app
  sources cited there.
- **[E:W3]** Same baseline and
  `docs/researchs/agent-foundation/README.md` — deferred brag/Hyperframes
  artifact and checkpoint requirements.
- **[E:K1]**
  `docs/researchs/agent-foundation/capabilities/context-compaction.md` —
  canonical transcript versus derived model-visible projection and Task 8
  Rollshot alternatives.

### Pi evidence

- **[E:P1]** `learn-projects/pi/packages/coding-agent/docs/sessions.md` and
  session-management source/tests cited by the reviewed Pi profile — default
  save, `--no-session`, resume/delete.
- **[E:P2]** `learn-projects/pi/packages/coding-agent/docs/session-format.md`
  and `packages/coding-agent/src/core/session-manager.ts` — JSONL tree entries,
  message/image/tool/compaction/custom records, branch reconstruction.

### oh-my-pi evidence

- **[E:O1]** `learn-projects/oh-my-pi/docs/memory.md` and
  `packages/coding-agent/src/memories/index.ts` — defaults, startup eligibility,
  extraction/consolidation, commands, bounds, generated files.
- **[E:O2]** `packages/coding-agent/src/memories/storage.ts` and `index.ts` —
  claims/leases, thread metadata, artifact sync/pruning, secret redaction.
- **[E:O3]** `packages/coding-agent/src/memory-backend/{types,runtime,resolve,off,local}.ts`
  and settings schema — default-off backend, project-local non-searchable
  behavior, clear and save.
- **[E:O4]** `packages/coding-agent/src/memories/index.ts` —
  `saveLearnedLesson`, `neutralizeInjection`, `readLearnedLessons`; memory
  read-path guidance cited by the reviewed oh-my-pi profile.

### Codex evidence

- **[E:C1]** `learn-projects/codex/codex-rs/features/src/lib.rs` and app-server
  extension wiring — stable default-off `memories` feature and use gate;
  `MemoriesConfig` defaults.
- **[E:C2]** `codex-rs/memories/README.md` and
  `memories/write/src/{start,phase1,phase2}.rs` — root-only startup, extraction,
  global consolidation, workspace and pruning.
- **[E:C3]** `memories/read/templates/memories/read_path.md`, read-tool source,
  and `memories/read/src/citations.rs` — bounded recall, staleness rule,
  citations and usage attribution.
- **[E:C4]** `state/src/runtime/memories.rs`, write guards/selection, external
  tool callsites that mark memory pollution, and phase-two sandbox tests — age,
  use, poisoning, and internal-agent boundary.
- **[E:C5]** `memories/write/src/control.rs` and
  `write/src/extensions/ad_hoc.rs` plus focused tests — whole-root clear,
  symlink refusal, explicit ad-hoc notes.

### Claude Code evidence

- **[E:L1]**
  `learn-projects/claude-code-source-code/src/memdir/{memdir,paths,memoryTypes}.ts`
  — default eligibility, project-root path, trusted-setting rules, memory
  categories, direct writer and explicit remember/forget guidance.
- **[E:L2]** `src/memdir/{memoryScan,findRelevantMemories,memoryAge}.ts` —
  bounded scan/selection, already-surfaced filtering, staleness warning.
- **[E:L3]** `src/services/extractMemories/extractMemories.ts` and extraction
  prompts — gated background writer and restricted tool set.
- **[E:L4]** `src/memdir/teamMemPaths.ts`, team-memory tools/prompts, secret
  guard/scanner, and `src/services/teamMemorySync` — gated team path,
  containment, credential/OAuth sync boundary.

The reviewed Round 1 profiles remain the cross-capability context for these
source references; this document re-checked the focused memory/session claims
at the pinned revisions rather than treating profile prose as the sole proof.
