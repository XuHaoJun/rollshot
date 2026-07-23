# Rollshot Agent Foundation Recommendation

**Research date:** 2026-07-23 (Asia/Taipei)  
**Status:** Reviewed  
**Umbrella revision:** 1  
**Research round:** 6 (Synthesis)  
**Systems/capabilities:** All 14 umbrella research areas, 4 system profiles, 13 capability comparisons, Rig boundary  
**Evidence baseline:** All reviewed artifacts at their pinned revisions; checkpoint 2 audit report `task-20-checkpoint-report.md`; decision-matrix commit `0f0594c`  
**Checkout commit:** `0f0594c4dc71da0350412eb1da76260ec5f9d320`  
**Evidence mode:** Static synthesis of reviewed artifacts. No new source inspection, provider request, or runtime experiment was performed for this document.

This document translates the decision matrix into staged, independently
verifiable implementation slices. Every recommendation traces to a
decision-matrix row. The recommendation is actionable slices, not a new design.

## 1. Boundaries and non-goals

### Boundaries (what the foundation owns)

The agent foundation governs these boundaries for Smart Redaction and Action
Guide workloads:

- **Run boundary:** one bounded invocation with provider-neutral streaming,
  typed tool execution, finite budgets, cancellation, and a typed terminal
  outcome (matrix §2.1, retain).
- **Task identity:** a durable Product Task envelope that binds revision,
  attempts, review artifacts, and terminal status around the run (matrix §2.2,
  adopt).
- **Artifact lifecycle:** a typed promotion contract from tool output to product
  artifact, plus the existing proposal envelope (matrix §2.12, combine).
- **Authority boundary:** an immutable snapshot of consent, OS permissions, and
  tool availability at run start, with the existing QuickJS sandbox as an inner
  enforcement layer (matrix §2.10, combine).
- **Skill boundary:** a static host-owned instruction catalog that never grants
  execution authority (matrix §2.9, adopt).
- **Job boundary:** a process-local registry of live operations with typed
  lifecycle, cancellation, and cleanup (matrix §2.6, adopt).
- **Context strategy:** artifact re-projection at product boundaries as the
  primary strategy, with typed manifest compaction as an emergency safety valve
  within a single run (matrix §2.4, combine).
- **Event boundary:** transient display events plus durable audit events for
  material transitions (matrix §2.13, retain + adopt).
- **Provider boundary:** the existing provider-neutral facade with Rig retained
  as a private implementation detail (matrix §2.14, retain).

### Non-goals

The following are explicitly outside the foundation's scope for the initial
slices:

- **Cross-run transcript persistence.** No workload establishes the need.
  Restart condition: a workload demonstrates that model history across
  invocations is required and cannot be satisfied by artifact re-projection
  (matrix §2.1, deferred).
- **Child agents, fan-out, and parallel execution.** Inline serial execution
  satisfies all active workloads. Restart condition: measured dispatch economics
  exceed inline cost, or Action Guide demonstrates real multi-step batch demand
  (matrix §2.3, deferred).
- **Semantic memory.** All three workloads recover correctly without it.
  Restart condition: explicit project state and curated records are
  insufficient, or repeated-instruction rate measurably degrades productivity
  (matrix §2.5, deferred).
- **Durable job recovery across process restart.** Process-local registry is
  sufficient. Restart condition: a tool starts remote/chargeable work, or the
  Hyperframes workload is adopted (matrix §2.6, deferred).
- **Conversation resume, workflow journal, and child sidecars.** Restart
  condition: artifact re-projection is insufficient for recovery (matrix §2.7,
  deferred).
- **Parallel tool scheduling and dynamic tool discovery.** Serial execution is
  sufficient. Restart condition: measured parallel dispatch economics exceed
  serial cost, or Hyperframes requires dependency-aware tool waves (matrix
  §2.8, deferred).
- **Authority-bound provider catalog and compiled extensions.** Restart
  condition: Hyperframes needs authority-preserving package handoff, or
  MCP-delivered skills materially affect the decision (matrix §2.9, deferred).
- **Live capability broker and durable job authority leases.** Restart
  condition: durable/remote jobs need authority reattachment (matrix §2.10,
  deferred).
- **Hierarchical reservation ledger and child/job budgets.** Restart condition:
  child or job budgets become necessary, or Hyperframes is adopted (matrix
  §2.11, deferred).
- **Expected-output completion contracts and artifact budgets.** Restart
  condition: Hyperframes is adopted (matrix §2.12, deferred).
- **Full event-sourcing journal and reconnectable event replay.** Restart
  condition: Hyperframes needs durable workflow event receipts (matrix §2.13,
  deferred).
- **Provider-native compaction, capability negotiation, and third-provider
  support.** Restart condition: provider-native compaction becomes a candidate,
  or a workload requires capability negotiation (matrix §2.14, deferred).
- **Video generation, general workflow platform, or one upstream agent ported
  wholesale.** These are non-goals per the umbrella (README §2.4).

---

## 2. Staged slices

Six slices, ordered by dependency and workload value.

| Slice | Name | Primary workload | Dispositions | Depends on |
|:-----:|------|-----------------|--------------|------------|
| 1 | Provider boundary spike | Smart Redaction | retain + spike (§2.11, §2.14) | — |
| 2 | Product Task and artifact promotion | Smart Redaction | adopt (§2.2), combine (§2.12) | Slice 1 |
| 3 | Authority and skill foundation | Smart Redaction + Action Guide | combine (§2.10), adopt (§2.9) | Slice 2 |
| 4 | Live job registry | Action Guide | adopt (§2.6) | Slice 3 |
| 5 | Context strategy | Action Guide | combine (§2.4) | Slice 2 |
| 6 | Audit observability | All workloads | retain + adopt (§2.13) | Slice 2 |

Dependency rationale:

- **Slice 1 → all.** The provider-stream cancellation spike and Rig effort
  measurement verify that the current translation boundary is sound. Every
  later slice builds on the provider facade; if the spike reveals a boundary
  failure, the Rig disposition changes and subsequent slices adjust.
- **Slice 2 → Slices 3, 5, 6.** The Product Task envelope provides task
  identity, revision binding, and artifact references used by authority
  snapshots (Slice 3), context re-projection (Slice 5), and audit events
  (Slice 6).
- **Slice 3 → Slice 4.** The authority boundary gates job execution; the job
  registry uses authority snapshots for operation admission.
- **Slice 2 → Slice 4.** Job identity references the Product Task.
- **Slice 2 → Slice 5.** Context re-projection starts from durable task/artifact
  state.
- **Slice 2 → Slice 6.** Audit events log task/artifact transitions.

---

## 3. Per-slice specification

### Slice 1: Provider boundary spike

| Field | Value |
|-------|-------|
| **Problem statement** | The Rig translation boundary in `driver.rs`, `model.rs`, and `provider.rs` has never been exercised under provider stream edge cases (establishment stall, established-item stall, cancel/deadline race). The code/test/security surface of a Rig 0.39→0.40 upgrade is unmeasured. Without executable evidence, the retain disposition rests on static inspection alone. |
| **Workload** | Smart Redaction's serial bounded run with typed terminals. Provider-stream cancellation is a current deficiency regardless of future architecture. |
| **Adopted patterns** | Decision matrix §2.11 (retain + spike on provider-stream cancellation) and §2.14 (retain + spike on Rig effort measurement). §3.1 (Rig retain) with §3.2 (fork/vendor) as fallback. |
| **State/authority owner** | `AgentRunner` owns the live run's model/tool loop. Rollshot owns the public model facade. Rig is a private implementation detail. |
| **Failure/cancel/resume behavior** | The spike tests four failure modes: (1) provider stream stalls during establishment → `ProviderFailure` terminal within deadline; (2) provider stream stalls mid-item → partial content discarded, terminal within deadline; (3) cancel races with deadline → whichever fires first produces the honest terminal; (4) provider error mid-stream → `ProviderFailure` terminal. Each mode must produce a `RunTerminalState` variant that is honest — no silent success after partial failure. |
| **Acceptance evidence** | (1) Fake provider test suite covering all four edge cases, each asserting the correct `RunTerminalState` variant and latency bound. (2) `stream_to_model_events` cancellation gap fixed or documented with a targeted follow-up. (3) Rig 0.39→0.40 effort measurement report: code surface diff, test surface diff, breaking changes, security surface delta. (4) `rtk cargo test -p rollshot-agent` passes. |
| **Deferred scope** | If the spike reveals the translation boundary cannot accommodate a needed behavior change, fork/vendor becomes the next option. Provider-native compaction and capability negotiation are deferred per matrix §2.14. |

**Traceability:** matrix §2.11 budgets/cancellation (retain + spike), §2.14
provider boundaries (retain + spike), §3.1 Rig retain, §3.2 Rig fork/vendor.

---

### Slice 2: Product Task and artifact promotion

| Field | Value |
|-------|-------|
| **Problem statement** | Smart Redaction has no durable task identity around its run. Validation/dry-run attempts are run-budget counters, not durable records. The `ReadyForReview` proposal is the only path from tool output to product artifact — there is no generic promotion contract for tool or external results that need to become product artifacts. |
| **Workload** | Smart Redaction: one bounded run that needs revision binding, attempt tracking, and typed review artifact reference. Action Guide: revision-bound proposals that bind `run_id` and `document_state_id`. |
| **Adopted patterns** | Decision matrix §2.2 adopt Pattern A (bounded Product Task envelope). §2.12 combine Pattern A (proposal envelope, retained) + Pattern B (typed artifact promotion contract, adopted). |
| **State/authority owner** | App/product owns the Product Task record. `AgentRunner` owns the live execution within one task attempt. Product owns artifact truth, review decisions, and publication authority. |
| **Failure/cancel/resume behavior** | `running` attempts at crash are reconciled as `unknown`. Stale proposals against changed document revisions are rejected deterministically. Validation failure returns structured diagnostics. Review decision is durable and tied to artifact revision. |
| **Acceptance evidence** | (1) `ProductTask` struct: task ID, type, authorized input references, document/project revision, status, attempt summaries, terminal, proposal artifact reference, timestamps. (2) `ProductArtifact` trait: artifact ID, revision, kind, schema version, content digest, source binding, validation receipt, provenance, retention class. (3) Both implemented for the existing `ReadyForReview` proposal as the first concrete type. (4) Persisted atomically with the review handoff. (5) `rtk cargo test -p rollshot-agent` passes. |
| **Deferred scope** | Dependency graph, workflow scheduler, checkpoint gates, and job handles are deferred until the Hyperframes workload is activated (product decision P3). Expected-output completion contracts are deferred per matrix §2.12. |

**Traceability:** matrix §2.2 task/todo/workflow (adopt Pattern A), §2.12
artifacts/review/provenance (combine Pattern A+B), §2.7 persistence (adopt
Task checkpoint snapshot).

---

### Slice 3: Authority and skill foundation

| Field | Value |
|-------|-------|
| **Problem statement** | There is no explicit authority boundary between consent/OS permissions and concrete executor operations. Screen capture, input monitoring, model credentials, local files, and publish destination have different disclosure, lifetime, and revocation rules, but no immutable snapshot governs a run. Skill instructions have no host-owned catalog with validated metadata, content digest, or containment guarantees. |
| **Workload** | Smart Redaction: product owns disclosure consent, payload mode, review-before-apply. Action Guide: capture backend + listen-only input + export destination. Both workloads need instruction packages for task-specific guidance (redaction policy, review instructions, detection/editing workflows). |
| **Adopted patterns** | Decision matrix §2.10 combine Pattern A (capability snapshot at Agent Run boundary) + Pattern C's inner enforcement layer (fresh-context QuickJS executor + manifest-bounded host bridge). §2.9 adopt Alternative A (static host instruction catalog). |
| **State/authority owner** | Product owns consent, accepted artifact truth, review decisions, and publish authority. The `AuthoritySnapshot` is immutable for the run duration. Rollshot owns the skill catalog as an availability boundary. Skill content never grants execution authority. Tool execution remains a separate existing registry path with its own policy evaluation. |
| **Failure/cancel/resume behavior** | Missing/invalid/expired/mismatched authority grant denies execution. Unavailable approver denies foreground-only requests. Capture/input failure returns typed denial/degradation. Skill failures: `UnavailableAuthority`, `UnknownPackage`, `UnknownResource`, `InvalidMetadata`, `CatalogLimitExceeded`, `ResourceTooLarge`, `ContainmentViolation`, `DigestMismatch`/`StaleRevision`. Failure to load an optional skill must not weaken policy. |
| **Acceptance evidence** | (1) `AuthoritySnapshot` struct with consent state, OS permission status, tool availability, and document revision. (2) Wired into the existing `ToolRegistry` execution path — each tool declares required authority and the executor checks the snapshot. (3) One host-owned skill root: parse `SKILL.md` frontmatter, validate metadata, canonicalize containment, compute digest, create run-local catalog. (4) One explicit invocation path (`/skill:name`). (5) Skill registered as a prompt-injection source with bounded metadata budget. (6) Spike: macOS Screen Recording/Input Monitoring prompt/revocation behavior documented. (7) `rtk cargo test -p rollshot-agent` passes. |
| **Deferred scope** | Live capability broker (Pattern B) for durable/remote jobs. Authority-bound provider catalog (Alternative B) for Hyperframes handoff. Compiled extensions (Alternative C). MCP-delivered skills (deferred Round 5 gap 2). `@anthropic-ai/sandbox-runtime` inspection (deferred Round 5 gap 3). Restart condition: durable/remote jobs need authority reattachment, or Hyperframes needs authority-preserving package handoff. |

**Traceability:** matrix §2.10 permissions/sandboxing (combine Pattern A+C),
§2.9 skills/extensions (adopt Alt A).

---

### Slice 4: Live job registry

| Field | Value |
|-------|-------|
| **Problem statement** | Action Guide's video import already demonstrates a live media-operation lifecycle (`ImportCoordinator` with `ImportOperationId`, pass progress, `VideoImportCancellation`, `CancellableChild`, scratch cleanup), but the pattern is not generalized. There is no reusable process-local job registry for operations whose lifecycle can outlast the model turn that started them. |
| **Workload** | Action Guide: video import with operation identity, progress, cancellation, process reaping, staged output, and cleanup. Hyperframes (deferred): preview server, local render, remote render with idempotency key and polling. |
| **Adopted patterns** | Decision matrix §2.6 adopt Pattern A (live host operation registry). |
| **State/authority owner** | App/product owns the job registry and operation identity. Product adapters own capture/input/export authority. |
| **Failure/cancel/resume behavior** | Typed operation status: starting, running, succeeded, failed, cancelled. Process death loses controllers; orphan detection and cleanup, not PID adoption. Terminal records retained for a short timer. No durable job serialization. |
| **Acceptance evidence** | (1) Reusable `JobRegistry` extracted from `ImportCoordinator` behavioral pattern: `JobId`, kind, owner, status, cancellation, structured progress, bounded log reference, child handles, terminal result, short retention timer. (2) Authority snapshot checked at job admission. (3) Cleanup tests for orphan detection. (4) `rtk cargo test -p rollshot-action` passes. |
| **Deferred scope** | Durable job recovery across process restart. Remote job receipts with idempotency keys. Provider cost accounting for jobs. Restart condition: a tool starts remote/chargeable work, or the Hyperframes workload is adopted. |

**Traceability:** matrix §2.6 long-running jobs (adopt Pattern A).

---

### Slice 5: Context strategy

| Field | Value |
|-------|-------|
| **Problem statement** | Smart Redaction's normal outcome should fire zero compactions, but there is no safety valve for context pressure within a single run. Action Guide's project state is artifact-driven, but there is no mechanism to end a coordinator context at a product boundary and start a fresh run from durable state. If context pressure occurs, authoritative product state (generation evidence, revision, consent) must survive — not just prose summaries. |
| **Workload** | Smart Redaction: one finite run where compaction should be near-zero, but a safety valve is needed. Action Guide: project manifest is authoritative; ending a context at a revision boundary and starting fresh from durable state is the primary strategy. |
| **Adopted patterns** | Decision matrix §2.4 combine Pattern C (artifact re-projection, primary) + Pattern A (typed manifest, emergency safety valve). Claude's explicit continuity inventory (recent files, plan, invoked skills, async-agent status) borrowed for the manifest's field list. |
| **State/authority owner** | Product owns authoritative project/task state. Compaction summary is untrusted model output that cannot recreate approvals or permissions. Original transcript and replacement projection stored separately when product retention permits. |
| **Failure/cancel/resume behavior** | Compaction failure returns typed terminal; overflow recursion bounded to one retry. Authority/consent/approval never reconstructed from summary prose. Stale proposals still fail deterministically after compaction or resume. |
| **Acceptance evidence** | (1) Artifact re-projection implemented for Action Guide: end a coordinator context at a project revision boundary, start a fresh run from durable manifest + step/keyframe + checkpoint decisions. (2) Typed manifest safety valve: define continuity fields (recent artifacts, active task, invoked skills, approval decisions) for a single-run emergency compaction. (3) Spike: in-memory fake store contract, crash at tool/evidence/terminal/review boundaries, measure how often Task snapshot adds value over clean restart. (4) `rtk cargo test -p rollshot-action` passes. |
| **Deferred scope** | Provider-native compaction/remote compact. Hidden/gated Claude compaction reducers (deferred Round 5 gap 1). Cache-aware selective reduction optimization. Conversation resume and workflow journal. Restart condition: a compaction pattern becomes a synthesis finalist, or artifact re-projection is insufficient for recovery. |

**Traceability:** matrix §2.4 context compaction (combine Pattern C+A),
§2.7 persistence (adopt Task checkpoint snapshot), §2.1 conversation/session
(retain — no cross-run transcript needed for re-projection).

---

### Slice 6: Audit observability

| Field | Value |
|-------|-------|
| **Problem statement** | The current dual-path pattern (transient display `RunEvent` + authoritative `RunTerminalState`) is correct for Smart Redaction's bounded run. However, `AuditEvent` is declared and test-covered but not exercised in production. Material transitions (task created, attempt started, proposal submitted, review decided, artifact published) have no durable audit evidence. |
| **Workload** | Smart Redaction: `RunEvent` stream with `try_send` and terminal reconciliation. Action Guide: operation/revision-correlated progress and publish events around durable project state. |
| **Adopted patterns** | Decision matrix §2.13 retain the dual-path pattern + adopt typed audit event production for material transitions. |
| **State/authority owner** | Terminal state is authoritative. Transient events are best-effort display projection. Audit events are durable evidence of material transitions. |
| **Failure/cancel/resume behavior** | Dropped transient events are disclosed; terminal/snapshot repairs visible state. Interior audit event loss is not acceptable after acknowledgment. Reconnect reconstructs from authoritative state, not event replay. |
| **Acceptance evidence** | (1) `AuditEvent` variants emitted for Smart Redaction material transitions: task created, attempt started, proposal submitted, review decided. (2) Serialized to the existing declared vocabulary. (3) Audit events stored in append-only log with retention policy. (4) `rtk cargo test -p rollshot-agent` passes. |
| **Deferred scope** | Full event-sourcing journal. Reconnectable event replay. Progress aggregation across parent/child runs. Checkpoint pause/resume events. Restart condition: the Hyperframes workload is adopted and needs durable workflow event receipts. |

**Traceability:** matrix §2.13 events/observability/steering (retain + adopt).

---

## 4. Risks identified separately

### Product decisions

| ID | Decision | Evidence | Status |
|----|----------|----------|--------|
| P1 | Whether Action Guide needs foundation-owned orchestration beyond independent caption/annotation tasks | Baseline unknown 2; matrix §2.2 deferred portion | Open — deferred until Slice 2 is implemented and tested |
| P2 | Whether the deferred brag/Hyperframes workflow should run inside Rollshot, through an external skill host, or remain deferred | Baseline unknown 3; matrix §2.3 deferred portion | Open — no workload pressure until Hyperframes is activated |
| P3 | Activate the Hyperframes workload (unlocks dependency graph, checkpoint gates, job handles, child agents) | Matrix §2.2, §2.3, §2.6, §2.7 deferred restart conditions | Open — product decision, not architecture |

### Technical spikes

| ID | Spike | Slices | Evidence |
|----|-------|--------|----------|
| S1 | Provider-stream cancellation: fake providers with edge-case injection; verify every `RunTerminalState` is honest | Slice 1 | Matrix §2.11, §2.14 |
| S2 | Rig 0.39→0.40 effort measurement: code surface, test surface, breaking changes, security surface | Slice 1 | Matrix §2.14, §3.1-3.5 |
| S3 | macOS Screen Recording/Input Monitoring prompt/revocation behavior | Slice 3 | Matrix §2.10 |
| S4 | Authority snapshot value measurement: crash at tool/evidence/terminal/review boundaries, measure how often Task snapshot adds value over clean restart | Slice 5 | Matrix §2.7 |
| S5 | Skill metadata schema validation: frontmatter parsing, containment canonicalization, digest computation | Slice 3 | Matrix §2.9 |

### Migrations

| Migration | Slices | Risk |
|-----------|--------|------|
| Existing `ReadyForReview` proposal to `ProductArtifact` trait | Slice 2 | Low — additive trait; existing proposal becomes first concrete type |
| Existing `ImportCoordinator` to reusable `JobRegistry` | Slice 4 | Medium — behavioral extraction; must preserve existing import cancellation/reaping semantics |
| Existing `ToolRegistry` execution path to check `AuthoritySnapshot` | Slice 3 | Medium — adds a check layer; must fail closed and not weaken existing policy |
| `stream_to_model_events` cancellation gap fix | Slice 1 | Low-Medium — targeted fix in provider.rs; must not break existing provider contract tests |

### Compatibility risks

| Risk | Mitigation |
|------|------------|
| Rig 0.39→0.40 upgrade breaks translation boundary | Slice 1 spike measures before committing; fork/vendor is the fallback (matrix §3.2) |
| Authority snapshot adds latency to run startup | Snapshot is built once at run start from already-available state; profile in Slice 3 |
| Skill catalog metadata budget exceeds context window | Bounded metadata budget enforced at catalog creation; measure in Slice 3 |
| Artifact re-projection loses context that transcript compaction would preserve | Re-projection starts from authoritative product state, not conversation memory; the safety valve covers within-run edge cases |

---

## 5. Rig disposition

### Recommended option: Retain

Rig 0.39 is retained as a private implementation detail behind Rollshot's
provider-neutral facade. The translation boundary in `driver.rs`, `model.rs`,
and `provider.rs` already keeps Rig types out of public contracts. No workload
establishes a need to change the state machine's protocol phases, threading
invariants, or serialization contracts.

The exact consumed surface (state machine, turn/message assembly, provider
machinery, test harness) is documented in the baseline (§Rig boundary). Rollshot
does not delegate product budget, cancellation, authorization, tool
implementations, serial scheduling policy, terminals, or persistence to Rig.

### How it fits into the staged slices

Rig work is concentrated in **Slice 1** (provider boundary spike):

- **S1 (provider-stream cancellation spike):** Verify that the translation
  boundary handles all four stream edge cases correctly. Fix the
  `stream_to_model_events` cancellation gap.
- **S2 (Rig effort measurement):** Measure the code/test/security surface of
  a Rig 0.39→0.40 upgrade. Report breaking changes and the surface Rollshot
  would need to adapt.

If the spike reveals that the translation boundary cannot accommodate a needed
behavior change, **fork/vendor** (matrix §3.2) becomes the next option. The
fork surface is well-bounded: state machine, stream assembler, message types,
and provider clients — all already behind the private translation boundary.

**Remove** (matrix §3.4) is premature before workloads are fully scoped. Smart
Redaction's serial bounded run is simple enough for a bespoke loop, but Action
Guide's heterogeneous task profiles and the deferred Hyperframes workload may
benefit from a general state machine's protocol invariants.

**Replace** (matrix §3.3) was not triggered: no alternative library provides
Rust state machine + multi-provider streaming. Pi's `pi-ai` is TypeScript;
Codex is Responses-wire-only; Claude Code is Anthropic-specific.

### Independence from other slices

The Rig retain decision does not block Slices 2–6. The Product Task envelope
(Slice 2), authority snapshot (Slice 3), job registry (Slice 4), context
strategy (Slice 5), and audit events (Slice 6) are all built on top of the
existing provider-neutral facade, not on Rig's internal API. If the Slice 1
spike triggers a fork/vendor decision, the fork replaces the private internals
without changing any public contract or any slice's implementation.

### Verification criteria

- Slice 1 spike: all four provider-stream edge cases produce honest
  `RunTerminalState` variants.
- Slice 1 spike: Rig 0.39→0.40 effort measured with code/test/security surface
  report.
- If fork/vendor is triggered: fork boundary matches the existing private
  translation surface; no Rig types leak into public contracts.

---

## 6. Traceability matrix

Every recommendation traces to a decision-matrix row:

| Slice | Matrix row(s) | Disposition | Evidence artifact(s) |
|-------|---------------|-------------|---------------------|
| 1 | §2.11, §2.14, §3.1, §3.2 | retain + spike | `budgets-cancellation-retries.md`, `provider-and-context-boundaries.md`, baseline [R5-R6, G1-G2] |
| 2 | §2.2, §2.12, §2.7 | adopt + combine | `task-todo-workflow-state.md`, `artifacts-review-provenance.md`, `persistence-checkpoint-resume.md` |
| 3 | §2.10, §2.9 | combine + adopt | `permissions-and-sandboxing.md`, `skills-and-extensions.md`, baseline [R8, W1-W3] |
| 4 | §2.6 | adopt | `long-running-jobs.md`, baseline [W2, R2-R5] |
| 5 | §2.4, §2.7 | combine + adopt | `context-compaction.md`, `persistence-checkpoint-resume.md` |
| 6 | §2.13 | retain + adopt | `events-observability-steering.md`, baseline [R3, R5, R8] |
| Rig | §2.14, §3.1-3.5 | retain (spike in Slice 1) | `provider-and-context-boundaries.md`, baseline [G1, G2] |

Deferred capabilities with restart conditions:

| Capability | Matrix row | Restart condition |
|------------|-----------|-------------------|
| Cross-run transcript persistence | §2.1 | Workload demonstrates cross-run model history is required |
| Child agents/fan-out | §2.3 | Measured dispatch economics exceed inline cost |
| Semantic memory | §2.5 | Explicit project state is insufficient |
| Durable job recovery | §2.6 | Tool starts remote/chargeable work |
| Conversation resume | §2.7 | Artifact re-projection is insufficient |
| Parallel tool scheduling | §2.8 | Measured parallel economics exceed serial cost |
| Authority-bound skills | §2.9 | Hyperframes needs authority-preserving handoff |
| Live capability broker | §2.10 | Durable/remote jobs need authority reattachment |
| Hierarchical budgets | §2.11 | Child/job budgets become necessary |
| Expected-artifact contracts | §2.12 | Hyperframes is adopted |
| Event-sourcing journal | §2.13 | Hyperframes needs durable event receipts |
| Provider-native compaction | §2.14 | Compaction pattern becomes finalist |
| Claude hidden reducers | Round 5 gap 1 | Pattern B becomes finalist |
| MCP-delivered skills | Round 5 gap 2 | MCP skills materially affect decision |
| Sandbox runtime | Round 5 gap 3 | Pattern C remains finalist after spike |

---

## 7. Metadata

| Field | Value |
|-------|-------|
| Research date | 2026-07-23 (Asia/Taipei) |
| Status | Reviewed |
| Umbrella revision | 1 |
| Research round | 6 (Synthesis) |
| Systems/capabilities | All 14 umbrella areas, 4 system profiles, 13 capability comparisons, Rig boundary |
| Evidence baseline | All reviewed artifacts at their pinned revisions; checkpoint 2 audit report; decision-matrix commit `0f0594c` |
| Checkout commit | `0f0594c4dc71da0350412eb1da76260ec5f9d320` |
| Evidence mode | Static synthesis of reviewed artifacts. No new source inspection, provider request, or runtime experiment was performed for this document. |
