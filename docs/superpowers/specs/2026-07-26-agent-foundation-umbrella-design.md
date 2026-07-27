# Rollshot Agent Foundation Umbrella Design

**Date:** 2026-07-26  
**Status:** Approved umbrella design  
**Area:** Agent foundation  
**Source research:**
[`docs/researchs/agent-foundation/`](../../researchs/agent-foundation/)
**Motivating deferred idea:**
[`docs/ideas/2026-07-22-agent-skills-action-guide-launch-video.md`](../../ideas/2026-07-22-agent-skills-action-guide-launch-video.md)

## 1. Purpose

This umbrella turns the reviewed agent-foundation research into a governed,
staged implementation program. Its scope is to strengthen Rollshot's agent
capabilities without implementing the deferred launch-video idea or turning
Rollshot into a general-purpose coding-agent platform.

The umbrella defines:

- six independently verifiable foundation slices;
- phase ordering, dependencies, and acceptance gates;
- cross-slice ownership, authority, failure, privacy, and testing invariants;
- the required contents of each future child design spec and implementation
  plan; and
- restart conditions for deliberately deferred capabilities.

It does not prescribe implementation-plan-level files, symbols, storage
formats, or Rust APIs. Each child spec must re-examine the current code before
selecting those details.

## 2. Source-of-truth policy

The frozen research documents are evidence and architectural input. They do not
override current code. At the start of every child-spec workflow:

1. inspect the current implementation and tests;
2. verify that the research gap still exists;
3. record material drift from the research baseline;
4. preserve this umbrella's approved boundaries unless new evidence requires an
   explicit amendment; and
5. design against the current product path rather than historical names or
   assumptions.

This umbrella remains the live governing spec until Gate G3 is completed. A
finished child spec or plan becomes a historical snapshot and must not be
retroactively edited to hide implementation drift.

## 3. Goals

The program establishes these Rollshot-owned capabilities:

1. a provider-neutral run boundary with honest cancellation and failure
   terminals;
2. durable Product Task identity and typed artifact promotion;
3. immutable authority snapshots and a bounded static skill catalog;
4. a process-local lifecycle for live jobs;
5. artifact-first context continuity with a typed emergency safety valve; and
6. durable, privacy-safe audit evidence for material transitions.

The first product-path proof for skills is a bundled Smart Redaction instruction
skill that reuses the existing author/improve workflow, tools, budgets,
cancellation, validation, dry-run, and proposal review. It must not add a new
product feature or require new user-facing UI.

## 4. Non-goals

The program does not include:

- launch-video product design or implementation;
- brag or Hyperframes integration;
- video authoring, rendering, or export;
- child agents, fan-out, parallel tool scheduling, or a workflow DAG;
- semantic memory or cross-run transcript persistence;
- durable remote-job recovery or process adoption after restart;
- provider-native compaction or provider capability negotiation;
- a marketplace, package publishing, dependency solving, or automatic updates;
- automatic skill learning or self-modifying skills;
- arbitrary JavaScript, WebAssembly, native plugin, or Markdown-script
  execution;
- skill-defined permission grants or policy overrides;
- general event sourcing or reconnectable event replay; or
- a port or fork of an upstream agent framework without a separate approved
  decision.

These exclusions remain in force unless their stated restart conditions are
met and a new design workflow approves the added scope.

## 5. Program architecture

```text
Phase 0 — Boundary Evidence
└── Slice 1: Provider Boundary Reliability
        │
        ▼ Gate G0: retain the current private boundary or approve a change

Phase 1 — Durable Product Contracts
└── Slice 2: Product Task and Artifact Promotion
        │
        ▼ Gate G1: durable revision-bound review artifact proven

Phase 2 — Safe Skill Capability
└── Slice 3: Authority and Static Skills
        │
        ▼ Gate G2: bundled Smart Redaction skill proven end to end

Phase 3 — Operational Maturity
├── Slice 4: Live Job Registry ───────── depends on Slices 2 and 3
├── Slice 5: Context Continuity ──────── depends on Slice 2
└── Slice 6: Audit Observability ─────── depends on Slice 2
        │
        ▼ Gate G3: umbrella complete
```

Phases 0 through 2 are strictly sequential. Slice 5 and Slice 6 are technically
eligible after Gate G1, but their default execution point is after Gate G2 to
avoid concurrent changes to shared agent contracts. Slice 4 starts only after
Gate G2 because job admission depends on the authority boundary.

Each Phase 3 slice receives its own child spec and plan. Parallel implementation
is not authorized by this umbrella; the relevant child plans must reassess
shared-state and code-coupling risks first.

## 6. Gate policy

A gate passes only when:

1. the child design spec was approved;
2. its implementation plan was completed;
3. the plan's tests and acceptance evidence pass;
4. the implementation received independent code review;
5. migrations, residual risks, and deferred scope are recorded; and
6. the next child spec can use the resulting contracts without relying on an
   unresolved failure from the previous slice.

A failed gate stops progression. The current child spec must be revised, a
bounded spike must be run, or this umbrella must be amended. Later slices must
not silently absorb unresolved work.

## 7. Child document contract

Every slice follows this lifecycle:

```text
current-code exploration
→ child design discussion
→ child spec approval
→ child spec commit
→ implementation plan
→ execution
→ verification and independent review
→ gate decision
```

### 7.1 Umbrella responsibilities

This umbrella fixes:

- the problem and user value;
- ownership and authority boundaries;
- slice dependencies;
- mandatory acceptance evidence;
- non-goals and restart conditions; and
- questions that the child spec must answer.

### 7.2 Child-spec responsibilities

Each child spec fixes, against then-current code:

- concrete architecture and public contracts;
- state ownership and data flow;
- migration and compatibility behavior;
- failure, cancellation, staleness, and privacy semantics;
- test strategy and measurable acceptance criteria; and
- slice-specific exclusions.

### 7.3 Implementation-plan responsibilities

Each plan specifies:

- exact files and symbols;
- test-driven task order;
- minimal changes for each task;
- verification commands;
- migration order;
- review checkpoints; and
- rollback or stop points.

The umbrella deliberately does not contain these implementation details.

### 7.4 Document names

The umbrella is stored at:

`docs/superpowers/specs/2026-07-26-agent-foundation-umbrella-design.md`.

Child documents use the naming conventions:

- `docs/superpowers/specs/YYYY-MM-DD-agent-foundation-SLICE-SLUG-design.md`;
- `docs/superpowers/plans/YYYY-MM-DD-agent-foundation-SLICE-SLUG.md`.

The six slice slugs are `provider-boundary`, `product-task-artifact`,
`authority-static-skills`, `live-job-registry`, `context-continuity`, and
`audit-observability`. `YYYY-MM-DD` is the child document's actual creation
date.

Slice 1 additionally produces a decision record containing its provider/Rig
boundary disposition.

## 8. Cross-slice ownership and data flow

```text
Product-authorized input
        │
        ▼
Product Task and attempt
        │
        ├── immutable Authority Snapshot
        ├── immutable SkillUse identity and digest
        ▼
bounded Agent Run
        │
        ▼
typed Tool Registry and policy enforcement
        │
        ▼
validated candidate output
        │
        ▼
Product Artifact → explicit review → product commit or rejection
        │
        └── durable material audit events
```

Ownership remains explicit:

- the product owns consent, authority, Product Tasks, artifacts, review
  decisions, and publication truth;
- `AgentRunner` owns one bounded run, not a durable workflow;
- skills provide instructions and resources, never authority;
- tool executors own concrete side effects and independently enforce grants;
- provider and Rig state are private execution details, not product truth;
- context continuity re-projects product-owned artifacts rather than trusting
  transcript prose;
- the job registry owns live operation lifecycle, not product approval; and
- audit events are evidence of material transitions, not the product state
  itself.

## 9. Cross-slice failure invariants

All child designs must preserve these rules:

- boundary failures are typed and fail closed;
- an optional skill failure cannot expand remaining permissions;
- cancellation does not automatically retry side effects;
- stale task revisions, artifact revisions, skill digests, or document
  revisions are not silently substituted;
- partial provider, tool, or job results cannot be promoted as successful
  artifacts;
- transient display events may be dropped, but terminal, task, and artifact
  state must repair visible state;
- authority, consent, and approvals cannot be reconstructed from model prose;
- provider-native errors do not leak provider-specific types into public
  Rollshot contracts; and
- each child spec defines error ownership at its own boundary instead of
  creating a speculative global error abstraction.

## 10. Cross-slice privacy invariants

Durable provenance may contain identifiers, schema versions, content digests,
bounded error categories, and accepted user decisions. It does not retain by
default:

- screenshot or image pixels;
- raw Action Guide semantic input;
- provider credentials;
- complete skill bodies;
- unrestricted project file contents; or
- provider-native conversation internals.

Every new serialization, debug, tracing, event, and persistence path must have
privacy-focused tests or bounded inspection evidence. Runtime diagnostics in
product paths use privacy-safe structured `tracing` events with stable
`rollshot::*` targets.

## 11. Slice 1 — Provider Boundary Reliability

### 11.1 Problem

The current provider translation boundary needs executable evidence for stream
establishment stalls, established-stream stalls, provider errors, and
cancel/deadline races. The code, test, and security cost of changing the pinned
Rig boundary is also unmeasured.

### 11.2 Child spec must answer

- Who terminates establishment stalls, mid-stream stalls, provider errors, and
  cancel/deadline races?
- How are partial text and partial tool arguments discarded or represented?
- Which `RunTerminalState` honestly represents each failure?
- What latency bounds apply to cancellation and deadlines?
- What is the Rig 0.39-to-0.40 code, test, and security adaptation surface?
- Which evidence causes retain, fork/vendor, or boundary redesign?

### 11.3 Plan boundary

The plan must:

1. create deterministic fake-provider failure fixtures;
2. add tests for all four edge-case classes before production fixes;
3. fix the known cancellation gap without rewriting the provider stack;
4. measure the Rig upgrade surface; and
5. produce a decision record.

The child workflow should use a bounded technical-spike process because the
outcome is evidence, not a predetermined implementation decision.

### 11.4 Gate G0

Gate G0 requires:

- executable coverage for all four edge-case classes;
- a measured and passing cancel/deadline latency bound;
- no successful terminal after partial failure;
- no Rig type leakage through the provider-neutral public API;
- passing `rollshot-agent` provider contracts; and
- a written retain, fork/vendor, or redesign disposition.

## 12. Slice 2 — Product Task and Artifact Promotion

### 12.1 Problem

One bounded agent run lacks durable product task identity around attempts,
revisions, review artifacts, and terminal status. The existing
`ReadyForReview` path is not a generic typed promotion boundary for product
artifacts.

### 12.2 Child spec must answer

- What identities distinguish Product Task, attempt, agent run, proposal, and
  artifact?
- Who owns each state transition and timestamp?
- How are schema version, digest, source binding, validation receipt, and
  provenance represented?
- How is the review handoff persisted atomically?
- How is a `running` attempt reconciled after a crash?
- How are stale document and artifact revisions rejected?
- What retention and privacy policies apply?

### 12.3 Plan boundary

The plan must:

1. test state transitions, staleness, and crash reconciliation first;
2. add the minimum Product Task contract;
3. add a typed artifact-promotion contract;
4. make the existing `ReadyForReview` proposal the first concrete artifact;
   and
5. integrate the existing review handoff without introducing a workflow DAG.

### 12.4 Gate G1

Gate G1 requires:

- traceability from proposal to task, attempt, run, and document revision;
- a review decision bound to an artifact revision;
- deterministic stale-proposal rejection;
- passing persistence and reconciliation tests; and
- no regression in the existing Smart Redaction workflow.

## 13. Slice 3 — Authority and Static Skills

### 13.1 Problem

Rollshot needs an explicit immutable authority boundary for a run and a
host-owned instruction catalog whose contents cannot grant execution authority.
Skills must extend the agent without changing the provider-neutral facade or
bypassing tools, budgets, cancellation, validation, and review.

### 13.2 Child spec must answer

- How do consent, OS permissions, tool availability, and document revision form
  an immutable `AuthoritySnapshot`?
- How does each tool declare and enforce required authority?
- What are the skill manifest, metadata limits, content limits, digest, and
  deterministic precedence rules?
- How are path containment, symlinks, special files, and size limits handled?
- How do catalog availability, invocation, grants, and execution remain
  distinct?
- How are skill invocation and provenance bound to Product Task and artifact?
- How does a bundled Smart Redaction instruction skill enter the existing
  author/improve path without adding UI or product behavior?

### 13.3 Plan boundary

The plan must:

1. test fail-closed authority enforcement first;
2. connect the immutable snapshot to `ToolRegistry` execution;
3. implement a bounded run-local static host catalog;
4. implement an explicit host invocation contract;
5. add one bundled Smart Redaction instruction skill; and
6. prove that skill content cannot grant filesystem, network, process, image,
   or document-mutation authority.

The first increment contains no runtime extensions, package scripts, remote
skill providers, marketplace, or automatic skill mutation.

### 13.4 Gate G2

Gate G2 requires:

- explicit selection and bounded loading of the bundled Smart Redaction skill;
- a recorded content digest and invocation provenance;
- independent authority checks for every tool call;
- unchanged budget, cancellation, validation, dry-run, and review semantics;
- typed failures for staleness, containment violations, special files, and
  oversize content;
- privacy-safe provenance without durable full-body retention; and
- no executable extension or script shortcut.

Passing Gate G2 demonstrates the minimum trustworthy skills foundation. It does
not authorize launch-video implementation.

## 14. Slice 4 — Live Job Registry

### 14.1 Problem

Action Guide video import already has live-operation behavior, but there is no
reusable process-local registry for work whose lifecycle may outlast the model
turn that initiated it.

### 14.2 Child spec must answer

- What are `JobId`, kind, owner, Product Task reference, and lifecycle states?
- How is authority checked at admission?
- How are structured progress, bounded logs, terminal result, cancellation, and
  child-process ownership represented?
- How are process death, orphan cleanup, and terminal retention handled?
- How can the existing import coordinator behavior be reused without changing
  its cancellation and process-reaping semantics?

### 14.3 Plan boundary

The plan must:

1. lock down existing import cancellation and reaping behavior with tests;
2. add reusable lifecycle contract tests;
3. extract a process-local registry;
4. migrate one existing import path as the proof; and
5. verify cancellation, failure, orphan cleanup, and short terminal retention.

### 14.4 Slice gate

The slice gate requires:

- typed starting, running, succeeded, failed, and cancelled states;
- cancellation and cleanup independent of model-turn lifetime;
- fail-closed admission authority;
- no unsafe process-ID adoption after application restart; and
- no remote jobs, durable reattachment, or workflow scheduler.

## 15. Slice 5 — Context Continuity

### 15.1 Problem

Authoritative product state must survive context boundaries without treating a
model-generated summary as persistence. Artifact re-projection is the primary
strategy; typed manifest compaction is only an emergency safety valve within a
run.

### 15.2 Child spec must answer

- Which product boundaries end an old context and start a fresh projection?
- Which manifest fields are authoritative?
- Which typed continuity fields must an emergency compact preserve?
- How are skill use, review decisions, task references, and artifact references
  retained?
- Which state must never be reconstructed from prose?
- How are compaction failure, overflow retry, and stale results handled?

### 15.3 Plan boundary

The plan must:

1. add artifact re-projection tests first;
2. prove a fresh Action Guide run from a durable revision boundary;
3. add a typed emergency continuity manifest;
4. inject failures at tool, evidence, terminal, and review boundaries; and
5. measure clean restart against snapshot recovery.

### 15.4 Slice gate

The slice gate requires:

- recovery of necessary context from durable product state;
- no reconstruction of authority, consent, or approval from prose;
- at most one bounded overflow retry;
- deterministic stale-proposal rejection after re-projection; and
- no semantic memory, conversation resume, or provider-native compaction.

## 16. Slice 6 — Durable Audit Observability

### 16.1 Problem

Transient display events and authoritative terminals are appropriate for a
bounded run, but material task, skill, proposal, review, and publication
transitions require durable privacy-safe audit evidence.

### 16.2 Child spec must answer

- Which events are transient display events and which are durable audit events?
- Which task, attempt, skill-use, authority-denial, proposal, review, and
  publication transitions are material?
- What are event identity, correlation, schema version, retention, and privacy
  rules?
- What guarantees apply after an append is acknowledged?
- How is startup reconciliation performed?
- How does the UI reconstruct from authoritative state rather than audit replay?

### 16.3 Plan boundary

The plan must:

1. add audit contracts for material transitions first;
2. activate or migrate the existing `AuditEvent` vocabulary;
3. add an append-only persistence boundary;
4. connect Product Task, artifact, skill-use, and review transitions; and
5. test privacy, redaction, and transient-event loss.

### 16.4 Gate G3

Gate G3 requires:

- durable correlated evidence for every defined material transition;
- no silent interior loss after append acknowledgement;
- repair of transient display loss from authoritative state;
- no audit retention of image pixels, raw semantic input, credentials, or full
  skill bodies;
- no conversion into a full event-sourcing or replay platform; and
- successful gates for all six slices.

## 17. Verification policy

Every child plan includes, as applicable:

1. unit and state-machine tests;
2. provider, tool, persistence, or job boundary contract tests;
3. cancellation, race, staleness, and failure injection;
4. privacy-safe serialization, debug, and diagnostics tests;
5. regression coverage for the affected active workload;
6. `rtk cargo test` for affected crates;
7. `rtk cargo fmt --check`;
8. `rtk cargo clippy --workspace --all-targets -- -D warnings` when the risk
   justifies workspace-wide verification; and
9. independent code review before the gate decision.

Slice-specific verification additionally includes:

- a bounded-scale catalog test for Slice 3;
- process cleanup and orphan tests for Slice 4;
- boundary failure/recovery tests for Slice 5; and
- crash-consistency and acknowledged-append tests for Slice 6.

A child spec that changes user-visible iced UI must invoke the repository's
iced UI testing workflow before editing and satisfy its independent visual
review rules. This umbrella does not itself require a UI change.

## 18. Child workflow readiness

| Slice | Earliest start | Plan creation point |
|---|---|---|
| Slice 1 | After this umbrella is approved and committed | After the Slice 1 spike spec is approved |
| Slice 2 | After Gate G0 | After the Slice 2 child spec is approved |
| Slice 3 | After Gate G1 | After the Slice 3 child spec is approved |
| Slice 4 | After Gate G2 | After the Slice 4 child spec is approved |
| Slice 5 | After Gate G1; default after Gate G2 | After the Slice 5 child spec is approved |
| Slice 6 | After Gate G1; default after Gate G2 | After the Slice 6 child spec is approved |

This umbrella workflow creates no child spec or implementation plan. Those are
written just in time after their start conditions are satisfied.

## 19. Amendment policy

An umbrella amendment is required when new evidence changes:

- a cross-slice ownership or authority boundary;
- phase ordering or dependencies;
- a gate's required evidence;
- a program-level non-goal; or
- the definition of umbrella completion.

The amendment process is:

1. write a decision record describing the evidence and affected slices;
2. identify completed child documents that remain historical evidence;
3. present the umbrella change for user approval;
4. update only the live umbrella and future child requirements; and
5. do not rewrite completed child specs or plans.

A discovery contained within one slice belongs in that slice's child spec and
does not require an umbrella amendment.

## 20. Deferred capability restart conditions

| Deferred capability | Restart condition |
|---|---|
| Cross-run transcript persistence | A workload proves model history is required and artifact re-projection is insufficient |
| Child agents or fan-out | Measured dispatch economics or Action Guide batch demand exceeds inline serial execution |
| Semantic memory | Explicit project state and curated records are measurably insufficient |
| Durable or remote job recovery | A tool starts remote or chargeable work that must survive process restart |
| Parallel tool scheduling | Measured dependency-aware execution benefit exceeds serial cost and risk |
| Authority-bound remote skill providers | A real cross-environment handoff cannot be served by the host catalog |
| Compiled extensions | A trusted native integration requires startup registration beyond existing product composition |
| Hierarchical child/job budgets | Child or durable job execution is adopted |
| Expected-artifact workflow contracts | A multi-stage production workload is approved |
| Full event replay | A product workflow requires durable reconnectable event receipts |
| Provider-native compaction | A measured context workload makes it a finalist |
| Launch-video implementation | Separate product discovery and design approve it after the trustworthy skills prerequisite exists |

## 21. Launch-video boundary

Gate G2 satisfies only the deferred idea's prerequisite that Rollshot
demonstrate one smaller skill end to end with bounded tools, cancellation,
explicit authority, and reviewable output.

Restarting launch-video work still requires a separate workflow that:

1. rechecks current product and code state;
2. validates Action Guide evidence and user demand;
3. receives product/CEO review;
4. decides whether concierge validation is still required; and
5. creates a new launch-video design spec and implementation plan.

Neither Gate G2 nor Gate G3 automatically schedules or approves launch-video
work.

## 22. Umbrella completion

This umbrella completes only when:

- all six slice gates pass;
- all migrations and residual risks are recorded;
- the foundation remains useful independently of launch video;
- no deferred platform capability was added without its restart condition and a
  separate approved design; and
- the user approves the Gate G3 completion decision.

After completion, this umbrella becomes a historical snapshot. Future agent
foundation changes require a new dated design or a separately approved
iteration.
