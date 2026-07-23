# Agent Foundation Decision Matrix

**Research date:** 2026-07-23 (Asia/Taipei)  
**Status:** Reviewed  
**Umbrella revision:** 1  
**Research round:** 6 (Synthesis)  
**Systems/capabilities:** All 14 umbrella research areas, 4 system profiles, 13 capability comparisons, Rig boundary  
**Evidence baseline:** All reviewed artifacts at their pinned revisions; checkpoint 2 audit report `task-20-checkpoint-report.md`

This document synthesizes every reviewed artifact into one traceable
disposition per research area. It is the single-page reference for the final
recommendation (Task 22). Every claim traces to a reviewed artifact; every
disposition uses exactly one of five approved verbs.

## 1. Disposition key

| Disposition | Meaning |
|---|---|
| **retain** | Keep existing Rollshot behavior as-is |
| **adopt** | Pick one external pattern and adapt it |
| **combine** | Name every source pattern AND the boundary between them |
| **spike** | Need executable evidence before deciding |
| **defer** | Out of current scope with explicit restart condition |

---

## 2. Decision matrix

### 2.1 Conversation, session, and run model

| Field | Value |
|---|---|
| **Rollshot need** | Provider-neutral streaming with tool-call/result continuity; bounded run lifecycle with typed terminals; session bookkeeping for one invocation. Smart Redaction workbench owns consent, budget, cancellation, and review handoff (`00-rollshot-baseline-workloads.md` §Current architecture). |
| **Workload evidence** | Smart Redaction: one fresh Rig `AgentRun` per invocation, serial tools, typed `RunTerminalState`. Action Guide: independent bounded proposal calls with `run_id` and `document_state_id`. brag+Hyperframes: multi-stage work would need durable run identity, not demonstrated by current code. |
| **Candidate patterns** | A: current fresh-run-per-invocation (Rollshot baseline). B: durable cross-run session with transcript persistence (Pi JSONL, oh-my-pi, Codex, Claude). C: artifact-driven re-projection from durable product state (Action Guide manifest, Hyperframes expected-artifact). |
| **Selected disposition** | **retain** |
| **Rationale** | No workload establishes cross-run transcript continuity. Smart Redaction's session is moved by value into the run and not returned; Action Guide persists product artifacts independently. The provider-neutral facade already keeps Rig types private. Adding cross-run session persistence before a workload requires it would add retention/privacy surface without user value. |
| **Authority owner** | App workbench owns consent, provider config, session value, budget, cancellation, and review state. `AgentRunner` owns one run's model/tool loop. |
| **Persistence owner** | Agent state is memory-only. Action Guide project manifest is durable product artifact persistence, separate from agent state. |
| **Failure model** | Run terminal is typed and authoritative (`ReadyForReview`, `NeedsUserInput`, `Cancelled`, `BudgetExhausted`, etc.). Transient `RunEvent`s may drop; terminal reconciliation is authoritative. |
| **Smallest verifiable slice** | Current Smart Redaction flow already demonstrates the pattern. No new work needed. |
| **Deferred portion** | Cross-run transcript continuity and durable session persistence. Restart condition: a workload demonstrates that model history across invocations is required and cannot be satisfied by artifact re-projection. |
| **Evidence links** | `00-rollshot-baseline-workloads.md` [R1-R8, W1-W2]; `task-todo-workflow-state.md` §3; `context-compaction.md` §4; `persistence-checkpoint-resume.md` §4.1 |
| **Confidence** | High — current code is directly inspected; no workload contradicts the retention. |

### 2.2 Task, todo, and workflow state

| Field | Value |
|---|---|
| **Rollshot need** | Bounded product work identity with revision-bound inputs, attempt tracking, typed terminal, and review artifact reference. Smart Redaction needs this around one run; Action Guide needs revision-bound proposal identity; Hyperframes (deferred) would need dependency-aware workflow state. |
| **Workload evidence** | Smart Redaction: validation/dry-run attempts are run-budget counters, not durable task records. Action Guide: `ProjectManifestV2` stores revision, frames, steps, annotations; visual-annotation binds `run_id`, `document_state_id`. Hyperframes: dependency stages, checkpoint gates, expected-artifact completion. |
| **Candidate patterns** | A: bounded Product Task envelope around existing run (`task-todo-workflow-state.md` §11 Pattern A). B: durable workflow ledger with dependency graph, attempts, and jobs (Pattern B). C: product-owned staged ledger without general scheduler (Pattern C). |
| **Selected disposition** | **adopt** — Pattern A (bounded Product Task envelope) |
| **Rationale** | Smart Redaction and Action Guide both benefit from explicit task identity, revision binding, attempt records, and review artifact references. Pattern A adds the minimum durable record without introducing dependency graphs, cycle detection, or scheduler complexity. The deferred workload's dependency/checkpoint needs are explicitly deferred. Claude's work-ledger dependency edges and oh-my-pi's fan-out Task are useful references but their graph/scheduler surfaces exceed current workload evidence. |
| **Authority owner** | App/product owns the Product Task record. `AgentRunner` owns the live execution within one task attempt. |
| **Persistence owner** | Product Task snapshot is durable; live run state remains memory-only within one attempt. |
| **Failure model** | Typed terminal maps to task status. `running` attempts at crash are reconciled as `unknown`; stale proposals against changed document revisions are rejected. |
| **Smallest verifiable slice** | Add a `ProductTask` struct around one Smart Redaction run: task ID, type, authorized input references, document/project revision, status, attempt summaries, terminal, proposal artifact reference, timestamps. Persist atomically with the review handoff. |
| **Deferred portion** | Dependency graph, workflow scheduler, checkpoint gates, and job handles. Restart condition: the deferred brag/Hyperframes workload is activated (product decision P3 from checkpoint 2). |
| **Evidence links** | `task-todo-workflow-state.md` §11-12; `00-rollshot-baseline-workloads.md` [W1-W6]; `persistence-checkpoint-resume.md` §10 Pattern A |
| **Confidence** | High for the adopt decision; medium for the exact schema shape (needs spike on Smart Redaction handoff crash behavior). |

### 2.3 Subagents and parallelism

| Field | Value |
|---|---|
| **Rollshot need** | Sequential inline execution for current bounded work. No workload establishes a need for child agents, fan-out, or parallel tool execution. |
| **Workload evidence** | Smart Redaction: one serial model/tool loop. Action Guide: independent bounded proposals, no batch demand proven. Hyperframes: optional scene workers with expected-artifact completion, but only if the deferred workload is adopted and exceeds inline economics. |
| **Candidate patterns** | Keep inline serial (current). A: revision-bound proposal fan-out. B: artifact-inspection specialists. C: Hyperframes-style artifact workers. |
| **Selected disposition** | **retain** |
| **Rationale** | Inline serial execution satisfies all active workloads. Smart Redaction's mutating authoring chain has real dependencies. Action Guide's independent proposals have no proven batch demand. Hyperframes' measured economics show fan-out pays only beyond ~6 scenes; the serial fallback remains valid. Adding child-agent infrastructure before demand creates context duplication, privacy exposure multiplied by N, and coordination overhead without user value. |
| **Authority owner** | App owns one run's registry, budget, cancellation, and review. No child authority delegation exists. |
| **Persistence owner** | Run state is memory-only. No child transcripts or artifacts. |
| **Failure model** | Single cancellation source reaches provider and automation. Typed terminal ends the run. |
| **Smallest verifiable slice** | Current Smart Redaction flow. No new work needed. |
| **Deferred portion** | Child-agent lifecycle, fan-out/fan-in, artifact-based completion, concurrency caps. Restart condition: (1) a workload proves measured dispatch economics exceed inline cost (product decision P3), or (2) Action Guide demonstrates real multi-step batch demand. |
| **Evidence links** | `subagents-and-parallelism.md` §8-12; `00-rollshot-baseline-workloads.md` [W1-W5]; Hyperframes economics evidence [H2] |
| **Confidence** | High — all three workloads are inspected; inline is directly sufficient. |

### 2.4 Context compaction and continuity

| Field | Value |
|---|---|
| **Rollshot need** | Smart Redaction's normal outcome should fire zero compactions. Action Guide's project state is already artifact-driven. If context pressure occurs, authoritative product state (generation evidence, revision, consent) must survive, not just prose summaries. |
| **Workload evidence** | Smart Redaction: one finite run; compaction should be near-zero. Action Guide: project manifest is authoritative; compaction cannot substitute. Hyperframes: stage readiness, approval decisions, job handles, and expected artifacts must remain durable outside any summary. |
| **Candidate patterns** | A: host-owned full checkpoint with typed continuity manifest (Pi/Codex-style + Claude's explicit attachment inventory). B: projection-first, cache-aware selective reduction (oh-my-pi shake/prune + Claude time-micro). C: artifact/workflow re-projection instead of transcript compaction. |
| **Selected disposition** | **combine** — Pattern C (artifact re-projection) as primary, Pattern A (typed manifest) as emergency safety valve |
| **Boundary** | Pattern C is the primary strategy: end coordinator context at safe product boundaries and start fresh bounded runs from durable project/workflow state. Pattern A fires only as an emergency safety valve when selective reduction cannot create enough headroom within a single run. The boundary is: artifact re-projection at product revision/checkpoint boundaries; typed manifest compaction only within a single run under context pressure. |
| **Rationale** | Artifact re-projection gives the strongest crash/review semantics and least summary-chain drift. It fits existing Action Guide persistence and Hyperframes' expected-artifact completion. The typed manifest safety valve covers the rare case where a single Smart Redaction run needs context reduction before submission. Claude's explicit continuity inventory (recent files, plan, invoked skills, async-agent status) is borrowed for the manifest's field list. oh-my-pi's cache-aware pruning is useful evidence but belongs to a later optimization pass. |
| **Authority owner** | Product owns authoritative project/task state. Compaction summary is untrusted model output that cannot recreate approvals or permissions. |
| **Persistence owner** | Original transcript and replacement projection stored separately when product retention permits. Authoritative project/task/artifact state lives in product stores, not in compaction summaries. |
| **Failure model** | Compaction failure returns typed terminal; overflow recursion bounded to one retry. Authority/consent/approval never reconstructed from summary prose. Stale proposals still fail deterministically after compaction or resume. |
| **Smallest verifiable slice** | Implement artifact re-projection for Action Guide: end a coordinator context at a project revision boundary, start a fresh run from durable manifest + step/keyframe + checkpoint decisions. No transcript summarization needed. |
| **Deferred portion** | Provider-native compaction/remote compact; hidden/gated Claude compaction reducers (deferred Round 5 gap 1); cache-aware selective reduction optimization. Restart condition: a compaction pattern becomes a synthesis finalist. |
| **Evidence links** | `context-compaction.md` §11-12; `persistence-checkpoint-resume.md` §10; `00-rollshot-baseline-workloads.md` [W1-W6]; `provider-and-context-boundaries.md` §5 |
| **Confidence** | Medium-high — the combine is well-supported by artifact evidence; the exact manifest schema needs spike validation. |

### 2.5 Memory

| Field | Value |
|---|---|
| **Rollshot need** | No workload establishes semantic memory as necessary. Smart Redaction needs run-local facts and durable review state. Action Guide has durable project artifacts. Hyperframes depends on named artifacts and checkpoints, not model recall. |
| **Workload evidence** | Smart Redaction: bounded proposal-and-review run; no cross-session knowledge required. Action Guide: project frames, steps, revisions are project artifacts, not memories. Hyperframes: recovery from authoritative artifacts/workflow records, not conversation recall. |
| **Candidate patterns** | A: no reusable semantic memory. B: opt-in, project-scoped accepted memories. C: layered automatic consolidation. |
| **Selected disposition** | **defer** |
| **Rationale** | All three workload traces recover correctly without memory. Adding memory before a workload demonstrates that explicit project state and curated records are insufficient would create privacy, poisoning, provenance, expiry, and deletion surface without user value. The safe baseline for screenshot content is: raw pixels, crops, OCR text, thumbnails, and model attachments are artifacts or run inputs, never memory by default. |
| **Authority owner** | N/A — no memory system exists. |
| **Persistence owner** | N/A. |
| **Failure model** | N/A — deferred. |
| **Smallest verifiable slice** | N/A — deferred. |
| **Deferred portion** | Entire memory capability. Restart condition: (1) a workload demonstrates that explicit project state and curated records are insufficient (product decision P1-style), or (2) repeated-instruction rate measurably degrades user productivity. |
| **Evidence links** | `memory.md` §1-12; `00-rollshot-baseline-workloads.md` [W1-W3] |
| **Confidence** | High — all three workloads are explicitly traced; absence is a bounded, evidence-backed conclusion. |

### 2.6 Long-running jobs and processes

| Field | Value |
|---|---|
| **Rollshot need** | Action Guide video import already demonstrates live media-operation lifecycle (operation identity, progress, cancellation, process reaping, staged output, cleanup). No workload currently establishes durable job recovery across process restart. |
| **Workload evidence** | Smart Redaction: bounded run, no detached job. Action Guide video import: `ImportCoordinator` with `ImportOperationId`, pass progress, `VideoImportCancellation`, `CancellableChild`, scratch cleanup. Hyperframes: preview server, local render, remote render with `render_id`, idempotency key, polling. |
| **Candidate patterns** | A: live host operation registry. B: durable external job receipt and reconciliation. C: product-owned media operation with artifact truth. |
| **Selected disposition** | **adopt** — Pattern A (live host operation registry) |
| **Rationale** | The `ImportCoordinator` and `CancellableChild` already demonstrate the behavioral shape needed: stable live operation identity, stale-event rejection, structured progress, cooperative signal plus forced reaping, bounded diagnostics, and cleanup tests. Pattern A extends this to a process-local registry without claiming restart recovery. Pattern B (durable remote receipt) is unnecessary unless a future tool starts remote/chargeable work. Pattern C is a domain-specific ownership choice that can use Pattern A internally. |
| **Authority owner** | App/product owns the job registry and operation identity. Product adapters own capture/input/export authority. |
| **Persistence owner** | Process-local registry; terminal records retained for a short timer. No durable job serialization. |
| **Failure model** | Typed operation status (starting, running, succeeded, failed, cancelled). Process death loses controllers; orphan detection and cleanup, not PID adoption. |
| **Smallest verifiable slice** | Extract the existing `ImportCoordinator` behavioral pattern into a reusable process-local job registry: `JobId`, kind, owner, status, cancellation, structured progress, bounded log reference, child handles, terminal result, short retention timer. |
| **Deferred portion** | Durable job recovery across process restart; remote job receipts with idempotency keys; provider cost accounting for jobs. Restart condition: (1) a tool starts remote/chargeable work, or (2) the deferred Hyperframes workload is adopted. |
| **Evidence links** | `long-running-jobs.md` §4-9; `00-rollshot-baseline-workloads.md` [W2, R2-R5]; `persistence-checkpoint-resume.md` §1.2 |
| **Confidence** | High — the existing import coordinator provides direct behavioral evidence; the adopt is narrowly scoped. |

### 2.7 Persistence, checkpoint, and resume

| Field | Value |
|---|---|
| **Rollshot need** | Action Guide already provides strong artifact persistence (immutable assets, schema-versioned manifest, revision checks, atomic commit). Agent run persistence is not proven necessary. Conversation resume across process restart is not established by any workload. |
| **Workload evidence** | Smart Redaction: in-memory run ending in typed proposal. Action Guide: `ProjectManifestV2` with temp+sync+rename+directory-sync commit, V1→V2 migration, revision CAS. Hyperframes: checkpoint decisions, expected-artifact contracts, job handles must survive independently of children. |
| **Candidate patterns** | A: typed Task checkpoint snapshot + artifact truth. B: append-only workflow journal + materialized snapshot + artifacts. C: transcript/child sidecars as optional continuity only. |
| **Selected disposition** | **retain** — Action Guide artifact persistence as-is; **adopt** Pattern A (Task checkpoint snapshot) when the Product Task envelope from §2.2 is implemented |
| **Rationale** | Action Guide's artifact-driven persistence is already the strongest recovery model for its workload. The Product Task snapshot (from §2.2) naturally becomes the persistence boundary for agent work: task/attempt status, authorized input references, provider/model fingerprints, remaining budget, typed terminal, proposal artifact reference, and review handoff receipts. Conversation transcript persistence and workflow journal are deferred. |
| **Authority owner** | Product owns artifact truth and review decisions. Task snapshot owns attempt-level recovery state. |
| **Persistence owner** | Action Guide manifest in product store. Task snapshot atomically persisted with review handoff. Transcript is optional and not authoritative. |
| **Failure model** | Crash before/after durable boundaries: load snapshot, reconcile `running` attempts as `unknown`, check artifact/document revisions, route to retry/needs-user/review-redelivery/stale/complete/incompatible. Never reconstruct approvals from transcript. |
| **Smallest verifiable slice** | Spike: implement in-memory fake store contract, crash at tool/evidence/terminal/review boundaries, measure how often Task snapshot adds value over clean restart. |
| **Deferred portion** | Conversation resume; workflow journal; child sidecars; durable run serialization. Restart condition: a workload demonstrates that artifact re-projection is insufficient for recovery. |
| **Evidence links** | `persistence-checkpoint-resume.md` §1-10; `00-rollshot-baseline-workloads.md` [W1-W6, R2]; `artifacts-review-provenance.md` §3-4 |
| **Confidence** | Medium-high — Action Guide evidence is strong; Task snapshot value needs spike measurement. |

### 2.8 Tools and scheduling

| Field | Value |
|---|---|
| **Rollshot need** | Deterministic serial execution with terminal stop-after-success for current workloads. Typed tool registry with schema-driven arguments, bounded results, and cancellation. No workload establishes parallel tool calls or dynamic discovery. |
| **Workload evidence** | Smart Redaction: serial authoring chain with real dependencies; `submit_for_review`/`request_user_input` are terminal. Action Guide: revision-bound proposals with stale-result rejection. Hyperframes: dependency-aware waves, but that belongs to workflow ownership (§2.2), not tool scheduling. |
| **Candidate patterns** | A: bounded serial transaction (current). B: classified ordered-parallel batch. C: dependency-aware artifact waves. |
| **Selected disposition** | **retain** |
| **Rationale** | Serial execution plus generation checks make Smart Redaction's mutating chain easy to reason about. OpenAI requests also explicitly set `parallel_tool_calls: false`. Pi's parallel default, oh-my-pi's shared/exclusive scheduler, and Claude's safe-call concurrency are useful references but none proves Rollshot needs parallel tools. The workload ladder keeps three levels distinct: one serial agent batch, one safely overlappable batch, and a durable dependency-aware workflow — the third belongs to Product Task/Workflow ownership (§2.2), not tool scheduling. |
| **Authority owner** | `AgentRunner` owns one run's tool batch. Product owns current registry and authority. |
| **Persistence owner** | Run-local tool context. No tool-result store or retention contract. |
| **Failure model** | Unknown tool, hard error, argument/result overflow, per-tool call-limit failure, or cancellation stops the batch. First successful terminal tool stops remaining calls. |
| **Smallest verifiable slice** | Current Smart Redaction flow. No new work needed. |
| **Deferred portion** | Parallel tool scheduling; dynamic tool discovery; tool-result spill/promotion; dependency-aware scheduling. Restart condition: (1) a workload proves measured parallel dispatch economics exceed serial cost, or (2) the deferred Hyperframes workload requires dependency-aware tool waves. |
| **Evidence links** | `tools-and-scheduling.md` §3-10; `00-rollshot-baseline-workloads.md` [W1-W3, R5] |
| **Confidence** | High — serial execution is directly inspected and sufficient for all active workloads. |

### 2.9 Skills and extensions

| Field | Value |
|---|---|
| **Rollshot need** | Instruction packages for task-specific guidance (redaction policy, review instructions, detection/editing workflows). No workload establishes executable extensions, remote package providers, or runtime plugin loading. |
| **Workload evidence** | Smart Redaction: skill could hold redaction policy and examples; consent/budget/validation remain Rollshot-owned. Action Guide: skill could describe task-specific workflows; guide/revision/privacy boundary remain product state. Hyperframes: needs versioned inputs and skill-to-skill handoff, but not a runtime extension marketplace. |
| **Candidate patterns** | A: static host instruction catalog. B: authority-bound provider catalog. C: trusted compiled extensions. |
| **Selected disposition** | **adopt** — Alternative A (static host instruction catalog) |
| **Rationale** | Alternative A is the smallest design for instruction reuse in the first two workloads. It validates metadata/resources, canonicalizes containment, computes a content digest, and creates a run-local immutable catalog. No runtime extension module, hook API, remote provider, autolearn, or package script execution shortcut. The safety invariant is strict: skill text/metadata/resources never authorize tools, filesystem, network, or product permissions. Alternative B (authority providers) is deferred for the Hyperframes workload's authority-preserving handoff. Alternative C (compiled extensions) answers a different executable-integration problem. Claude's `mcpSkills` loader is absent from the pinned source (deferred Round 5 gap 2). |
| **Authority owner** | Rollshot owns the skill catalog as an availability boundary. Skill content never grants execution authority. Tool execution remains a separate existing registry path with its own policy evaluation. |
| **Persistence owner** | Run-local immutable catalog snapshot. Content digest identifies observed bytes. |
| **Failure model** | `UnavailableAuthority`, `UnknownPackage`, `UnknownResource`, `InvalidMetadata`, `CatalogLimitExceeded`, `ResourceTooLarge`, `ContainmentViolation`, `DigestMismatch`/`StaleRevision`. Failure to load an optional skill must not weaken policy. |
| **Smallest verifiable slice** | Implement one host-owned skill root: parse `SKILL.md` frontmatter, validate metadata, canonicalize containment, compute digest, create run-local catalog. Register as a prompt-injection source with bounded metadata budget. One explicit invocation path (`/skill:name`). |
| **Deferred portion** | Authority-bound provider catalog (Alternative B); compiled extensions (Alternative C); MCP-delivered skills (deferred Round 5 gap 2); autolearn/managed skills; semantic search. Restart condition: (1) the Hyperframes workload needs authority-preserving package handoff, or (2) MCP-delivered skills materially affect the skills decision. |
| **Evidence links** | `skills-and-extensions.md` §12-13; `00-rollshot-baseline-workloads.md` [W1-W3] |
| **Confidence** | Medium-high — the static catalog design is well-supported; exact metadata schema needs spike validation. |

### 2.10 Permissions, sandboxing, and trust

| Field | Value |
|---|---|
| **Rollshot need** | Product-owned authority bridge between consent/OS permissions and concrete executor operations. Screen capture, input monitoring, model credentials, local files, and publish destination have different disclosure, lifetime, and revocation rules. The existing QuickJS sandbox is strong for restricted automation but is not a general authority broker. |
| **Workload evidence** | Smart Redaction: product owns disclosure consent, payload mode, review-before-apply. Action Guide: capture backend + listen-only input + export destination. Hyperframes: durable jobs must reattach current authority, use least-privilege credentials, require explicit publish authority. |
| **Candidate patterns** | A: product-owned capability snapshot + managed executor. B: live capability broker with short-lived operation tokens. C: product-specific gates + external sandbox boundary. |
| **Selected disposition** | **combine** — Pattern A (capability snapshot) for the Agent Run boundary + Pattern C's inner enforcement layer (fresh-context QuickJS executor + manifest-bounded host bridge) |
| **Boundary** | Pattern A owns the outer boundary: at Agent Run start, build an immutable `AuthoritySnapshot` from current consent, OS state, policy, document revision, environment, and narrow tool availability. Each tool declares required authority and the executor checks the snapshot or requests an additional grant. Pattern C's inner enforcement layer is retained: the existing fresh-context QuickJS runtime with manifest-bounded vision bridge continues to confine restricted automation. The boundary is: snapshot at the run/tool admission level; fresh-context sandbox at the automation execution level. Pattern B (live broker) is deferred for durable/remote jobs. |
| **Rationale** | Pattern A fits the current per-run registry and typed state. It keeps pixels/input/publish product-owned and can retain the existing QuickJS executor as an inner layer. Pattern C's external sandbox is useful but does not supply product disclosure, credential, capture/input, or publish grants by itself. The combination gives explicit authority decisions at the run boundary while preserving the already-proven narrow automation enforcement. |
| **Authority owner** | Product owns consent, accepted artifact truth, review decisions, and publish authority. The `AuthoritySnapshot` is immutable for the run duration. |
| **Persistence owner** | Grants are scoped to run/task/job lifetime. No durable grant persistence across resume. |
| **Failure model** | Missing/invalid/expired/mismatched grant denies execution. Unavailable approver denies foreground-only requests. Capture/input failure returns typed denial/degradation. Denied read cannot be recovered by requesting "run unsandboxed." |
| **Smallest verifiable slice** | Define `AuthoritySnapshot` struct with consent state, OS permission status, tool availability, and document revision. Wire it into the existing `ToolRegistry` execution path. Spike on macOS Screen Recording/Input Monitoring prompt/revocation behavior. |
| **Deferred portion** | Live capability broker (Pattern B); durable job authority leases; remote executor enforcement; `@anthropic-ai/sandbox-runtime` inspection (deferred Round 5 gap 3). Restart condition: (1) durable/remote jobs need authority reattachment, or (2) Pattern C's external sandbox boundary remains a finalist after spike. |
| **Evidence links** | `permissions-and-sandboxing.md` §4-12; `00-rollshot-baseline-workloads.md` [R8, W1-W3]; `rollshot-automation-rquickjs` execution/lockdown/bridge evidence |
| **Confidence** | Medium — the combination is well-supported by existing enforcement evidence; platform sandbox behavior needs runtime verification. |

### 2.11 Budgets, cancellation, retry, and failure

| Field | Value |
|---|---|
| **Rollshot need** | Hard local ceilings, cancellation into provider/automation work, recoverable validation feedback, and actionable typed terminals for one bounded run. Cost enforcement is declared but not operationally enforced (no pricing function). Provider-stream cancellation has establishment and established-item gaps. |
| **Workload evidence** | Smart Redaction: 16-dimensional `RunBudget`, serial tools, typed terminals. Action Guide: revision-bound outcomes and stale-result rejection; video import has live cancellation. Hyperframes: hierarchical reservations, concurrency/job ceilings, durable cancellation intent, retry-safe attempts. |
| **Candidate patterns** | A: retain bounded single-run envelope. B: hierarchical reservation ledger for bounded children. C: separate Run, Job, and Artifact/Workflow envelopes. |
| **Selected disposition** | **retain** Pattern A (bounded single-run envelope) + **spike** on provider-stream cancellation |
| **Rationale** | The 16-dimensional budget with typed terminals is directly sufficient for Smart Redaction. Cost is declared but not enforced — this must be reported honestly until a pricing function exists. The provider-stream cancellation gap (establishment stall, established-item stall, cancel/deadline race) is a current Rollshot deficiency that needs executable evidence regardless of future architecture. Pattern B (hierarchical reservation) is unnecessary until child/job budgets are needed. Pattern C (separate envelopes) belongs to the deferred workflow architecture. |
| **Authority owner** | Product supplies input/registry/budget/review. `AgentRunner` owns live accounting and serial execution. |
| **Persistence owner** | Budget state is run-local and in-memory. No durable attempt ledger. |
| **Failure model** | `BudgetExhausted { dimension }` for named dimensions. `Cancelled` for observed cancellation. `ProviderFailure` for provider errors. `AgentProtocolFailure` for Rig/tool-call invariant failures. `NeedsUserInput` for concrete user questions. |
| **Smallest verifiable slice** | Spike: fake providers with cancellation injection; test establishment-stall, established-item-stall, cancel/deadline race; measure latency and terminal accuracy. Fix the `stream_to_model_events` cancellation gap. |
| **Deferred portion** | Hierarchical reservation ledger; child/job budgets; durable attempt/retry ledger; soft-limit steering; failure-class UX. Restart condition: (1) child or job budgets become necessary, or (2) the deferred Hyperframes workload is adopted. |
| **Evidence links** | `budgets-cancellation-retries.md` §3-9; `00-rollshot-baseline-workloads.md` [R5-R6, W1-W3] |
| **Confidence** | High for retain; medium for the spike outcome (needs runtime evidence). |

### 2.12 Artifacts, review, and provenance

| Field | Value |
|---|---|
| **Rollshot need** | Typed product artifacts with identity, schema/version, lifecycle state, provenance, validation evidence, review decision, and storage/retention contract. Smart Redaction's `ReadyForReview` proposal is the existing strong pattern. Action Guide's project manifest with immutable assets and revision CAS is the durable artifact pattern. |
| **Workload evidence** | Smart Redaction: validate → dry-run → submit-for-review → user review → apply/reject. Action Guide: project revision, frame metadata, step annotations, caption/visual proposals bound to revision. Hyperframes: expected scene artifacts, validation gates, render/poster/share-copy. |
| **Candidate patterns** | A: proposal envelope (current Rollshot). B: artifact ledger with typed identity/revision/provenance. C: expected-output completion contract. |
| **Selected disposition** | **combine** — Pattern A (proposal envelope, retained) + Pattern B (typed artifact promotion contract, adopted) |
| **Boundary** | Pattern A is retained for the existing validate→dry-run→review→apply proposal flow. Pattern B adds a minimal typed artifact promotion contract: when a tool output or external result needs to become a product artifact, it passes through explicit validation, identity assignment, provenance recording, and acceptance. The boundary is: proposal envelope for agent-generated review artifacts; promotion contract for tool/external outputs becoming product artifacts. Pattern C (expected-output completion) is deferred for the Hyperframes workload. |
| **Rationale** | The existing proposal flow is strong for Smart Redaction. Action Guide's project manifest already demonstrates durable artifact persistence. The missing piece is a generic path from tool output to product artifact — currently only `ReadyForReview` does this, and only for one specific proposal type. A minimal promotion contract makes the transition explicit without introducing a general artifact registry. |
| **Authority owner** | Product owns artifact truth, review decisions, and publication authority. Validation evidence is deterministic and reviewable. |
| **Persistence owner** | Immutable artifact revisions with mutable head/status pointer. Product store retains authoritative bytes and metadata. |
| **Failure model** | Validation failure returns structured diagnostics. Stale proposals against changed revisions are rejected. Review decision is durable and tied to artifact revision. Publication is atomic/no-replace where supported. |
| **Smallest verifiable slice** | Define a minimal `ProductArtifact` trait: artifact ID, revision, kind, schema version, content digest, source binding, validation receipt, provenance, and retention class. Implement for the existing `ReadyForReview` proposal as the first concrete type. |
| **Deferred portion** | Expected-output completion contracts for workflow nodes; artifact byte/count/retention budgets; publish receipt for remote services. Restart condition: the deferred Hyperframes workload is adopted. |
| **Evidence links** | `artifacts-review-provenance.md` §1-11; `00-rollshot-baseline-workloads.md` [R5, W1-W6]; `persistence-checkpoint-resume.md` §1.2 |
| **Confidence** | Medium-high — the existing proposal pattern is strong; the promotion contract needs schema spike. |

### 2.13 Events, observability, and user interaction

| Field | Value |
|---|---|
| **Rollshot need** | Live display projection for Smart Redaction's bounded run plus authoritative typed terminal. Privacy-safe progress without persisting sensitive content. Defined dropped-update semantics. Steering boundaries (what arrives during an active turn). |
| **Workload evidence** | Smart Redaction: `RunEvent` stream (text chunk, tool start/end, source change) with `try_send` and terminal reconciliation. Action Guide: operation/revision-correlated progress and publish events around durable project state. Hyperframes: durable task/job/artifact receipts, checkpoint pause/resume, distinct steering planes. |
| **Candidate patterns** | A: snapshot+projection (current Rollshot dual-path). B: audit journal. C: dual stream (transient display + durable audit). |
| **Selected disposition** | **retain** the dual-path pattern + **adopt** typed audit event production for material transitions |
| **Rationale** | The current dual-path (transient display `RunEvent` + authoritative `RunTerminalState`) is correct for Smart Redaction. `AuditEvent` is declared and test-covered but not exercised in production — adopting its production emission for material transitions (task created, attempt started, proposal submitted, review decided, artifact published) gives durable audit evidence without requiring a full event-sourcing journal. Action Guide's operation-correlated events demonstrate the pattern. |
| **Authority owner** | Terminal state is authoritative. Transient events are best-effort display projection. Audit events are durable evidence of material transitions. |
| **Persistence owner** | Terminal in product state. Audit events in append-only log with retention policy. Transient events discarded after display. |
| **Failure model** | Dropped transient events are disclosed; terminal/snapshot repairs visible state. Interior audit event loss is not acceptable after acknowledgment. Reconnect reconstructs from authoritative state, not event replay. |
| **Smallest verifiable slice** | Emit `AuditEvent` variants for Smart Redaction material transitions: task created, attempt started, proposal submitted, review decided. Serialize to the existing declared vocabulary. |
| **Deferred portion** | Full event-sourcing journal; reconnectable event replay; progress aggregation across parent/child runs; checkpoint pause/resume events. Restart condition: the deferred Hyperframes workload is adopted and needs durable workflow event receipts. |
| **Evidence links** | `events-observability-steering.md` §1-9; `00-rollshot-baseline-workloads.md` [R3, R5, R8] |
| **Confidence** | High — the dual-path pattern is directly inspected; audit event adoption is low-risk. |

### 2.14 Provider and context boundaries

| Field | Value |
|---|---|
| **Rollshot need** | Provider-neutral facade with private Rig translation. Rollshot owns all public contracts (`ModelRequest`, `ModelMessage`, `ToolDefinition`, `ModelStreamEvent`, `ModelUsage`, `ModelCompletion`, `StopReason`, `ModelError`, `ProviderAdapter`, `StreamBounds`). No Rig types leak through the public API. |
| **Workload evidence** | Smart Redaction: Anthropic or OpenAI adapter, serial tools, typed terminal. Action Guide: same facade for visual-annotation and caption calls. Hyperframes: longer horizons pressure context-window policy and per-stage model/provider selection. |
| **Candidate patterns** | A: provider-erasure (strict neutrality). B: opaque payloads (continuity-preserving neutrality). C: capability facade (negotiation-aware neutrality). |
| **Selected disposition** | **retain** the provider-neutral facade + **spike** on provider-stream cancellation and Rig effort measurement |
| **Rationale** | The existing facade already achieves provider-erasure at the public boundary while retaining provider-specific state privately for continuity. This is the correct design for current workloads. Provider breadth beyond Anthropic/OpenAI is not established by any workload (answered question from checkpoint 2). The spike is needed to: (1) verify terminal honesty under provider stream edge cases, and (2) measure the code/test/security surface of the current Rig 0.39 pin versus a potential upgrade or replacement. |
| **Authority owner** | Rollshot owns the public model facade. Concrete adapters are `pub(crate)` with private Rig client fields. |
| **Persistence owner** | Provider state lives in the current run only. No durable provider session persistence. |
| **Failure model** | `ProviderFailure` for provider errors. `StopReason` describes one model call, not a run terminal. Budget/cancellation enforcement is Rollshot-owned. |
| **Smallest verifiable slice** | Spike: (1) fake providers with stream edge cases; verify every `RunTerminalState` is honest under establishment stall, item stall, cancel/deadline race, and provider error. (2) Measure Rig 0.39→0.40 upgrade effort: code surface, test surface, breaking changes. |
| **Deferred portion** | Provider-native compaction/remote compact; capability negotiation; provider handoff within a session; third-provider support. Restart condition: (1) provider-native compaction becomes a candidate, or (2) a workload requires capability negotiation. |
| **Evidence links** | `provider-and-context-boundaries.md` §3-8; `00-rollshot-baseline-workloads.md` [R4, R6, G1-G2] |
| **Confidence** | High for the retain decision; medium for spike outcomes (needs runtime evidence). |

---

## 3. Rig option analysis

Rig 0.39 is the pinned external crate behind Rollshot's private provider translation. The baseline §3 identifies four outcomes. Upstream compatibility and avoidance of divergence are **not** decision criteria (umbrella §2.2, plan Global Constraint).

### 3.1 Retain

| Field | Value |
|---|---|
| **What it means** | Keep `rig-core = "=0.39.0"` behind the current private translation boundary in `driver.rs`, `model.rs`, and `provider.rs`. |
| **Surface Rollshot continues to depend on** | State machine (`AgentRun`, `AgentRunStep`, `PendingToolCall`); turn/message assembly (`StreamedTurnAssembler`, `StreamedTurn`, `AssistantContent`, `ToolCall`, `UserContent`, `ToolResultContent`, `Message`, `OneOrMany`); provider machinery (`CompletionRequest`, `CompletionClient`, `CompletionModel`, `StreamingCompletionResponse`, concrete Anthropic/OpenAI clients); test harness (`MockResponse`). |
| **Evidence** | Baseline [R3, R4, R6, R7, G1]; provider-and-context-boundaries §3-4. |
| **Rationale** | The translation boundary already keeps Rig types private. Rollshot does not expose Rig through public contracts. The state machine invariants (exhaustive CallModel/CallTools/Done protocol, turn counting, message threading, streamed-call accounting, complete result-set requirement) are delegated and tested. Retaining avoids owning security fixes, provider protocol changes, and serialization/privacy behavior for the state machine while the public boundary remains clean. |
| **Costs** | Continued dependency on Rig's transitive updates and any upstream bugs. No cross-version stability guarantee for serialized state (though Rollshot does not serialize it). |
| **Implication for other decisions** | Compatible with all matrix dispositions. The state machine is an implementation detail behind the provider-neutral facade. |

### 3.2 Fork/vendor

| Field | Value |
|---|---|
| **What it means** | Copy or fork the consumed Rig code and evolve it for Rollshot without compatibility reluctance. |
| **Surface Rollshot would own** | Security fixes, provider protocol changes, serialization/privacy behavior, tests, and maintenance for the selected state-machine, stream, message, and/or provider portions. |
| **Evidence** | Baseline §Rig boundary [G1, G2]; provider-and-context-boundaries §8. |
| **Rationale** | Forking is justified when Rollshot needs to change the state machine's behavior (e.g., different protocol phases, different threading invariants, or different serialization contracts) and those changes cannot be made behind the translation boundary. Currently, no such change is established. The translation boundary already handles tool-argument assembly (Rig ingests stream items, Rollshot stores deltas, parses JSON, constructs final `ToolCall` values). |
| **Costs** | Full ownership of the forked code's security, compatibility, and maintenance surface. Provider protocol changes (new Anthropic/OpenAI features) must be implemented in the fork. |
| **Implication for other decisions** | Would be triggered if the spike (§2.14) reveals that the translation boundary cannot accommodate a needed behavior change, or if Rig 0.39→0.40 upgrade effort is prohibitive and the 0.40 changes are necessary. |

### 3.3 Replace

| Field | Value |
|---|---|
| **What it means** | Substitute another library or independently designed component under Rollshot's facade. |
| **Surface Rollshot would own** | Re-proving every delegated invariant (protocol state machine, turn counting, message threading, streamed-call accounting, complete result-set requirement) and adapting or replacing the concrete provider transports without leaking the replacement into product contracts. |
| **Evidence** | Baseline §Rig boundary; provider-and-context-boundaries §8. |
| **Rationale** | Replacement is justified when a different library provides materially better provider protocol support, smaller surface, or better maintenance characteristics. No such library was identified in the research. Pi's `pi-ai` is TypeScript; Codex is Responses-wire-only; Claude Code is Anthropic-specific. None is a drop-in replacement for Rig's Rust state machine + multi-provider streaming. |
| **Costs** | Must re-prove all delegated invariants. New library may have its own compatibility, security, and maintenance surface. |
| **Implication for other decisions** | Would be triggered if the Rig effort spike reveals that maintaining the translation boundary is more expensive than replacing the underlying state machine with a purpose-built component. |

### 3.4 Remove

| Field | Value |
|---|---|
| **What it means** | Eliminate Rig and implement only the Rollshot-specific loop/provider behavior still required, or narrow workloads so a general state machine is unnecessary. |
| **Surface Rollshot would own** | A smaller bespoke protocol, tool-result/history threading, stream assembly, provider transport, and adversarial tests. Would delete unused general Rig capabilities rather than recreating them speculatively. |
| **Evidence** | Baseline §Rig boundary; provider-and-context-boundaries §8. |
| **Rationale** | Removal is justified when Rollshot's workloads are narrow enough that a general state machine adds more surface than it saves. Smart Redaction's serial bounded run is simple enough for a bespoke loop. However, Action Guide's heterogeneous task profiles and the deferred Hyperframes workload may benefit from a general state machine's protocol invariants. Removing Rig before those workloads are fully scoped risks reimplementing the same invariants less rigorously. |
| **Costs** | Full ownership of protocol correctness, stream assembly, provider transport, and adversarial testing. Must implement turn counting, message threading, pending-call correlation, and error recovery from scratch. |
| **Implication for other decisions** | Would be triggered if: (1) the workloads are definitively scoped and all fit a bespoke serial loop, or (2) the fork/replace costs exceed the value of delegated invariants. |

### 3.5 Recommended Rig disposition

**Retain** — with the provider-stream cancellation spike (§2.11, §2.14) to verify that the translation boundary handles stream edge cases correctly, and a Rig 0.39→0.40 effort measurement spike to quantify the code/test/security surface of a potential upgrade. If the spike reveals that the translation boundary cannot accommodate needed behavior changes, fork/vendor becomes the next option. Remove is premature before workloads are fully scoped.

---

## 4. Deferred Round 5 gaps

Three gap-driven research candidates were deferred by user approval at Checkpoint 2. Each is carried as an explicit deferred limitation with its restart condition.

| # | Gap | Affected capability | Restart condition |
|---|---|---|---|
| 1 | Claude Code hidden compaction reducers (reactive, cached-microcompact, snip, collapse) | Context compaction §2.4 Pattern B (projection-first, cache-aware) | Pattern B becomes a synthesis finalist |
| 2 | Claude Code `mcpSkills` loader | Skills and extensions §2.9 | MCP-delivered skills materially affect the skills decision |
| 3 | `@anthropic-ai/sandbox-runtime` | Permissions and sandboxing §2.10 Pattern C (external sandbox boundary) | Pattern C remains a finalist after the authority snapshot spike |

---

## 5. Evidence traceability audit

### 5.1 Disposition-to-artifact mapping

Every disposition traces to at least one reviewed artifact:

| Research area | Disposition | Primary evidence artifacts |
|---|---|---|
| Conversation/session/run | retain | `00-rollshot-baseline-workloads.md` [R1-R8]; `task-todo-workflow-state.md` §3; `context-compaction.md` §4 |
| Task/todo/workflow | adopt (Pattern A) | `task-todo-workflow-state.md` §11-12; `00-rollshot-baseline-workloads.md` [W1-W6] |
| Subagents/parallelism | retain | `subagents-and-parallelism.md` §8-12; `00-rollshot-baseline-workloads.md` [W1-W5] |
| Context compaction | combine (C+A) | `context-compaction.md` §11-12; `persistence-checkpoint-resume.md` §10 |
| Memory | defer | `memory.md` §1-12; `00-rollshot-baseline-workloads.md` [W1-W3] |
| Long-running jobs | adopt (Pattern A) | `long-running-jobs.md` §4-9; `00-rollshot-baseline-workloads.md` [W2, R2-R5] |
| Persistence/checkpoint/resume | retain + adopt | `persistence-checkpoint-resume.md` §1-10; `00-rollshot-baseline-workloads.md` [W1-W6, R2] |
| Tools/scheduling | retain | `tools-and-scheduling.md` §3-10; `00-rollshot-baseline-workloads.md` [W1-W3, R5] |
| Skills/extensions | adopt (Alt A) | `skills-and-extensions.md` §12-13; `00-rollshot-baseline-workloads.md` [W1-W3] |
| Permissions/sandboxing | combine (A+C) | `permissions-and-sandboxing.md` §4-12; `00-rollshot-baseline-workloads.md` [R8, W1-W3] |
| Budgets/cancellation/retry | retain + spike | `budgets-cancellation-retries.md` §3-9; `00-rollshot-baseline-workloads.md` [R5-R6, W1-W3] |
| Artifacts/review/provenance | combine (A+B) | `artifacts-review-provenance.md` §1-11; `00-rollshot-baseline-workloads.md` [R5, W1-W6] |
| Events/observability/steering | retain + adopt | `events-observability-steering.md` §1-9; `00-rollshot-baseline-workloads.md` [R3, R5, R8] |
| Provider/context boundaries | retain + spike | `provider-and-context-boundaries.md` §3-8; `00-rollshot-baseline-workloads.md` [R4, R6, G1-G2] |

### 5.2 Contradictions between artifacts

No material contradictions were found between reviewed artifacts. Minor observations:

1. **oh-my-pi `task.maxConcurrency` resize behavior:** `docs/tools/task.md` says later changes do not resize the semaphore, but source and focused tests contradict that sentence. The oh-my-pi profile follows code over documentation. This does not affect the matrix dispositions.

2. **Codex exec-server README/source conflict:** The README states that a closed websocket immediately terminates managed processes, but `SessionRegistry` source and recovery tests retain detached process state for the bounded TTL. The Codex profile follows source. This does not affect the matrix dispositions.

3. **Claude Code hidden modules:** Several context-reduction modules are absent from the pinned source tree. The Claude Code profile records this as a bounded limitation. The matrix defers the corresponding gap (§5 gap 1) with an explicit restart condition.

All dispositions are backed by evidence from reviewed artifacts. No disposition depends on a claim that contradicts another reviewed artifact.

---

## 6. Metadata

| Field | Value |
|---|---|
| Research date | 2026-07-23 (Asia/Taipei) |
| Status | Reviewed |
| Umbrella revision | 1 |
| Research round | 6 (Synthesis) |
| Systems/capabilities | All 14 umbrella areas, 4 system profiles, 13 capability comparisons, Rig boundary |
| Evidence baseline | All reviewed artifacts at their pinned revisions; checkpoint 2 audit report |
| Evidence mode | Static synthesis of reviewed artifacts. No new source inspection, provider request, or runtime experiment was performed for this document. |
