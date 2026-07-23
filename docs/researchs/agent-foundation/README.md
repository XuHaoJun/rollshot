# Rollshot Agent Foundation Umbrella Research Specification

**Date:** 2026-07-22  
**Status:** Frozen  
**Umbrella revision:** 1  
**Area:** Agent foundation  
**Output root:** `docs/researchs/agent-foundation/`

## 1. Purpose

This umbrella governs a multi-round research program for the next iteration of
Rollshot's agent foundation. It exists to prevent one reference implementation,
one dependency, or one attractive feature from prematurely determining the
architecture.

The research will compare how mature coding-agent systems model state,
delegation, context continuity, execution, extensibility, and safety. It will
then make capability-by-capability recommendations for Rollshot. The result is
research and an architectural recommendation, not an implementation plan.

The motivating workload is broader than the current bounded authoring loop:

- Smart Redaction is a short, bounded, review-producing agent run.
- Action Guide introduces longer-lived capture artifacts and editable workflow
  state.
- The deferred brag plus Hyperframes idea exercises project inspection,
  multi-stage creative work, checkpoints, background processes, rendering,
  optional parallel scene workers, and artifact-based recovery.

Brag and Hyperframes are workload evidence, not a mandate to implement video
generation or copy their architecture. The deferred product idea remains
documented in
[`../../ideas/2026-07-22-agent-skills-action-guide-launch-video.md`](../../ideas/2026-07-22-agent-skills-action-guide-launch-video.md).

## 2. Scope

### 2.1 Core systems

The first two research passes use a fixed core comparison set:

| System | Role in the comparison | Baseline checkout |
|---|---|---|
| Rollshot | Current bounded-agent baseline and product constraints | repository HEAD at each research round |
| Pi | Small provider-neutral agent loop, extensions, and Agent Skills | `dd6bea41efa8caa7a10fe5a6401676dc5699f83f` |
| oh-my-pi | Broader Pi-derived capability, resource, and managed-skill ecosystem | `7b141199d524b859c357fc89654f10b62b9f3df1` |
| Codex | Production Rust agent, tools, approvals, sandboxing, skills, and lifecycle | `4a443994bd12f49f2f08b21a2f224d9d42b9e734` |
| Claude Code source | Tasks, local and remote agents, teams, resume, and layered compaction | `2ca5ddabfed5f220812ea11f029eda03b21bc4c1` |

The checkout hashes make the initial evidence reproducible. A research document
that refreshes a checkout must record its own hash and date; it must not silently
reuse this table as proof of a later investigation.

### 2.2 Supporting references

These projects answer narrower questions and are not ranked as complete coding
agents:

| Reference | Use | Baseline checkout |
|---|---|---|
| Rig | Audit the state-machine invariants Rollshot currently consumes and the boundary of retain, fork, vendor, replace, or remove decisions | `2f37dfcd0156bdceab3eabe6f0a953f9202e2d77` |
| brag | Short launch-video workflow and cross-skill handoff workload | `357a805e76a93a528ac6cccac28c8da3e893272b` |
| Hyperframes | Artifact-driven video workflow, checkpoints, background work, and bounded worker dispatch workload | `807078c7cde9d5c8403588722d1cd9397c513a0d` |

Rig is not an architectural constraint. Rollshot does not need to preserve
upstream compatibility or avoid a fork. Research must still report the code,
test, security, and maintenance surface Rollshot would own under each option,
but reluctance to diverge from upstream is not a decision criterion.

### 2.3 Gap-driven additions

Additional systems may be researched only when the fixed core set leaves a
specific question unanswered. Examples include a durable-workflow reference
such as Temporal or a checkpointed agent-graph reference such as LangGraph.

Every addition must first be recorded in the amendment log with:

- the unresolved question;
- why the core systems do not answer it;
- the narrow evidence sought;
- the research documents affected; and
- whether earlier conclusions require review.

This rule permits discovery without turning the program into an unrestricted
market survey.

### 2.4 Non-goals

This research does not:

- implement agent-foundation code;
- design or implement the launch-video product;
- choose one upstream agent to port wholesale;
- require all capabilities to come from one reference system;
- preserve Rig merely because Rollshot currently uses it;
- treat a larger feature set as evidence of better Rollshot fit;
- build a general-purpose coding-agent platform without Rollshot workload
  evidence; or
- produce an implementation plan before the research recommendation is
  reviewed.

## 3. Research model

The program uses a hybrid two-pass method.

### Pass A: system profiles

Each core system receives a compact architecture profile using the same
vocabulary and evidence template. This pass establishes what each system means
by task, todo, job, agent, session, compact, memory, skill, and resume before any
cross-system comparison is attempted.

### Pass B: capability deep dives

Each capability document compares materially different designs across the
systems. A missing concept is meaningful evidence: a simple sequential agent
must not be described as having DAG or parallel semantics merely to fill a
matrix cell.

### Synthesis

A decision matrix traces each Rollshot recommendation back to system-profile
and capability evidence. Rollshot may retain its own design, adopt one pattern,
combine patterns, request a technical spike, or explicitly defer a capability.

## 4. Known research areas

The following are first-class research areas at revision 1.

### 4.1 Conversation, session, and run model

- message and content-item representation;
- distinction between conversation, session, run, turn, and workflow;
- tool-call and tool-result continuity;
- steering, follow-up, interrupt, and queued user input;
- main-thread versus child-agent context; and
- provider-specific state versus provider-neutral state.

### 4.2 Task, todo, and workflow state

- whether todos are model-authored reminders, host-owned state, or durable
  execution records;
- task identifiers, ownership, status transitions, dependencies, and terminal
  states;
- flat lists versus dependency graphs or DAGs;
- sequential-only systems versus parallel-ready systems;
- task output, artifact, progress, and error representation;
- state visibility to the model and user; and
- recovery after process or model-turn failure.

Task, todo, workflow, external job, and agent run must remain distinct concepts
unless evidence demonstrates that a system intentionally unifies them.

### 4.3 Subagents and parallelism

- spawn, fork, teammate, remote-agent, and worker semantics;
- inherited, copied, reconstructed, or isolated context;
- scoped tools, resources, permissions, and budgets;
- parent-child lifecycle and cancellation propagation;
- concurrency caps, queues, waves, and scheduling;
- filesystem or artifact coordination and race prevention;
- completion based on agent notification versus expected artifact;
- selective retry and concrete failure feedback; and
- when inline sequential execution is cheaper or more reliable than dispatch.

Subagent count is not a maturity metric. The research must evaluate isolation,
state ownership, completion semantics, and failure handling.

### 4.4 Context compaction and continuity

- manual and automatic full compaction;
- reactive compaction thresholds and context-window policy;
- mini-, micro-, or cached micro-compaction;
- snipping, pruning, projection, and tool-result compression;
- summary generation, validation, and compact-boundary representation;
- prompt-cache effects and model/provider coupling;
- preservation of invoked skills, active tasks, user decisions, permissions,
  artifacts, and pending checkpoints;
- compaction in child-agent or sidechain contexts;
- observability and user control; and
- failure behavior when compaction cannot fit or loses required state.

Terminology must follow source evidence. `mini-compact` is an umbrella research
label until each system's actual mechanism is identified; it must not be assumed
equivalent to Claude Code's cached microcompact, history snipping, or another
system's pruning.

Compaction is not persistence. A summary that preserves conversational context
does not replace durable workflow state or artifact records.

### 4.5 Memory

- ephemeral run memory;
- conversation and session history;
- project and user memory;
- shared team or agent memory;
- background consolidation; and
- memory retrieval, provenance, expiry, privacy, and deletion.

The research must state which information belongs in memory, compacted context,
workflow state, or an artifact store and why.

### 4.6 Long-running jobs and processes

- foreground tool execution versus managed background jobs;
- process and remote-job handles;
- start, subscribe or poll, cancel, collect, and cleanup lifecycle;
- logs, structured progress, cost, and partial output;
- jobs that outlive a model turn, agent run, UI session, or application process;
- idempotency and reattachment after resume;
- local preview servers and interactive services;
- render, FFmpeg, analysis, media-generation, and cloud-job workloads; and
- separation of agent wall-time budgets from external job lifetime.

### 4.7 Persistence, checkpoint, and resume

- event log, snapshot, transcript, artifact-driven, and hybrid persistence;
- durable user decisions and approval checkpoints;
- crash consistency and partial-write handling;
- resume routing and reconstruction of active state;
- compatibility across model, provider, skill, or application upgrades;
- stale tool handles, permissions, and external jobs; and
- deterministic rules for choosing the next executable step.

### 4.8 Tools and scheduling

- typed versus dynamic tools;
- tool discovery, availability, and descriptions;
- serial and parallel tool calls;
- ordering, dependency, and stop-after-success semantics;
- side-effect classification and idempotency;
- tool-result size, retention, and compaction;
- hooks before and after execution; and
- separation between registered, available, authorized, and selected tools.

### 4.9 Skills and extensions

- discovery sources and project trust;
- metadata-first progressive disclosure;
- instruction and resource packages versus executable extensions;
- explicit and implicit invocation;
- source authority and opaque resource identifiers;
- per-run skill snapshots and versioning;
- skill context budgets and compaction continuity;
- declared capabilities versus granted authority; and
- installation, update, provenance, disablement, and revocation.

### 4.10 Permissions, sandboxing, and trust

- filesystem, process, network, credential, capture, and publishing authority;
- sandbox policies and platform boundaries;
- approval policy, cached approvals, and escalation;
- project, skill, extension, tool, and remote-provider trust;
- background-agent permission behavior;
- fail-closed behavior after disconnect or resume; and
- auditability without persisting private content unnecessarily.

### 4.11 Budgets, cancellation, retry, and failure

- token, cost, wall-time, tool, child-agent, job, and artifact budgets;
- hierarchical budget allocation;
- cancellation propagation and cleanup;
- retry ownership, retry limits, and idempotency;
- protocol, provider, validation, runtime, and user-blocked failure classes; and
- terminal states that remain actionable to the user.

### 4.12 Artifacts, review, and provenance

- typed artifacts versus ambient files;
- expected-artifact completion contracts;
- drafts, validation evidence, review decisions, and revisions;
- immutable versus mutable artifacts;
- source, skill, tool, model, and user-decision provenance;
- privacy boundaries and redaction; and
- handoff from agent judgment to deterministic execution.

### 4.13 Events, observability, and user interaction

- lifecycle event taxonomies;
- text, reasoning-safe status, tool, task, compact, job, and artifact events;
- progress aggregation across parent and child runs;
- UI reconstruction after reconnect;
- audit events versus transient display events;
- checkpoint questions and non-blocking progress updates; and
- diagnostics appropriate for privacy-sensitive product paths.

### 4.14 Provider and context boundaries

- provider-neutral request, response, usage, and tool-call models;
- model-specific context windows and compaction triggers;
- streaming and partial-tool-call semantics;
- capability negotiation and unsupported features;
- provider handoff within a session or workflow; and
- which state must remain owned by Rollshot.

## 5. Research rounds

### Round 0: Rollshot baseline and workload requirements

Document the current `rollshot-agent` model, its Rig usage, in-memory state,
typed tools, budgets, cancellation, terminal states, and product integration.
Describe the three workload classes without proposing a foundation design.

**Output:** `00-rollshot-baseline-workloads.md`

**Gate:** Every claimed gap is tied to current code and at least one workload.

### Round 1: Core system profiles

Produce one profile per core external system:

- `systems/pi.md`
- `systems/oh-my-pi.md`
- `systems/codex.md`
- `systems/claude-code.md`

**Gate:** The profiles use the common template, define system-specific terms,
and distinguish implemented behavior from documentation, experimental flags,
future-roadmap code, and inference.

**Checkpoint 1:** Review the taxonomy and correct false equivalences before
capability comparison begins.

### Round 2: State and continuity

Produce:

- `capabilities/task-todo-workflow-state.md`
- `capabilities/context-compaction.md`
- `capabilities/memory.md`
- `capabilities/persistence-checkpoint-resume.md`

**Gate:** Task/Todo/Workflow and compact/mini-compact each have independent,
evidence-backed comparisons and explicitly describe missing semantics.

### Round 3: Execution and delegation

Produce:

- `capabilities/subagents-and-parallelism.md`
- `capabilities/long-running-jobs.md`
- `capabilities/tools-and-scheduling.md`
- `capabilities/budgets-cancellation-retries.md`

**Gate:** Every candidate design specifies ownership, concurrency, completion,
cancellation, failure, retry, and artifact behavior.

### Round 4: Extensibility and safety

Produce:

- `capabilities/skills-and-extensions.md`
- `capabilities/permissions-and-sandboxing.md`
- `capabilities/artifacts-review-provenance.md`
- `capabilities/events-observability-steering.md`
- `capabilities/provider-and-context-boundaries.md`

**Gate:** Authority and availability are not conflated, and every extensibility
recommendation preserves explicit Rollshot ownership of product permissions.

### Round 5: Gap-driven investigations

Perform only investigations admitted through the amendment process. Each one
gets a narrowly named document under `capabilities/` or `systems/` and states
which unresolved matrix cells it exists to answer.

**Gate:** Each added reference closes a named gap or records that the question
remains unresolved; it does not broaden unrelated comparisons.

### Round 6: Synthesis

Produce:

- `decision-matrix.md`
- `rollshot-recommendation.md`

**Checkpoint 2:** Review capability evidence, unresolved gaps, and proposed
spikes before architectural selection.

**Gate:** Every recommendation maps to evidence and one of five dispositions:

1. retain a Rollshot design;
2. adopt a reference pattern;
3. combine explicitly named patterns;
4. run a bounded technical spike; or
5. defer with a stated restart condition.

**Checkpoint 3:** Review the decision matrix and Rollshot recommendation. Only
an approved recommendation may become input to a separate implementation spec.

## 6. Planned document inventory

| Document | Round | Initial status |
|---|---:|---|
| `README.md` | Umbrella | Active Research |
| `00-rollshot-baseline-workloads.md` | 0 | Planned |
| `systems/pi.md` | 1 | Planned |
| `systems/oh-my-pi.md` | 1 | Planned |
| `systems/codex.md` | 1 | Planned |
| `systems/claude-code.md` | 1 | Planned |
| `capabilities/task-todo-workflow-state.md` | 2 | Planned |
| `capabilities/context-compaction.md` | 2 | Planned |
| `capabilities/memory.md` | 2 | Planned |
| `capabilities/persistence-checkpoint-resume.md` | 2 | Planned |
| `capabilities/subagents-and-parallelism.md` | 3 | Planned |
| `capabilities/long-running-jobs.md` | 3 | Planned |
| `capabilities/tools-and-scheduling.md` | 3 | Planned |
| `capabilities/budgets-cancellation-retries.md` | 3 | Planned |
| `capabilities/skills-and-extensions.md` | 4 | Planned |
| `capabilities/permissions-and-sandboxing.md` | 4 | Planned |
| `capabilities/artifacts-review-provenance.md` | 4 | Planned |
| `capabilities/events-observability-steering.md` | 4 | Planned |
| `capabilities/provider-and-context-boundaries.md` | 4 | Planned |
| `decision-matrix.md` | 6 | Planned |
| `rollshot-recommendation.md` | 6 | Planned |

The inventory is revised when an admitted discovery adds, combines, splits, or
removes a research area. The amendment log must explain the change.

## 7. Evidence standard

Every system profile and capability document records:

- research date and timezone;
- repository commit or official-document version/date;
- exact source paths, symbols, tests, or authoritative links;
- evidence type: source, test, official documentation, runtime observation, or
  inference;
- confidence and known limitations;
- implemented, experimental, feature-gated, disabled, and roadmap-only status;
- state ownership and persistence boundary;
- concurrency and failure behavior where applicable;
- security and privacy consequences;
- pattern worth borrowing;
- pattern inappropriate for Rollshot;
- Rollshot gap or existing strength; and
- unanswered questions.

Static inspection must not be presented as runtime proof. A missing search
result is reported as “not found in the investigated scope,” not as proof that a
feature cannot exist.

Each capability comparison must include at least two materially different
designs. If the core systems expose only one, the document either admits a
gap-driven reference or defers the decision.

## 8. Common system-profile template

Each Round 1 profile uses these sections:

1. scope and reproducibility baseline;
2. architecture and ownership boundaries;
3. conversation, session, and run lifecycle;
4. task, todo, workflow, and background-job model;
5. subagents and parallel execution;
6. compaction, context continuity, and memory;
7. persistence, checkpoints, and resume;
8. tools and scheduling;
9. skills and extensions;
10. permissions, sandboxing, and trust;
11. budgets, cancellation, retry, and failures;
12. artifacts, events, and observability;
13. provider boundary;
14. strengths for Rollshot;
15. mismatches and risks;
16. unresolved questions; and
17. evidence index.

An absent capability receives an explicit “not found in investigated scope”
entry with the search boundary; sections are not silently omitted.

## 9. Capability comparison template

Each Round 2–4 capability document uses these sections:

1. Rollshot problem statement and workload evidence;
2. terminology and non-equivalent concepts;
3. current Rollshot behavior;
4. per-system behavior;
5. state and authority ownership;
6. lifecycle or state-machine comparison;
7. persistence and recovery;
8. parallelism and scheduling, when applicable;
9. failure, cancellation, and retry;
10. security and privacy;
11. alternatives and trade-offs;
12. preliminary Rollshot fit without final selection;
13. evidence gaps and required spikes; and
14. evidence index.

Capability documents compare behavior rather than feature names. They must not
choose a final Rollshot architecture before synthesis.

## 10. Governance

### 10.1 Document lifecycle

The umbrella follows:

```text
Draft -> Active Research -> Synthesis -> Reviewed -> Frozen
```

Research documents use `Planned`, `In Progress`, `Reviewed`, or `Superseded`.
The umbrella moves to `Synthesis` after Rounds 0–5 satisfy their gates, to
`Reviewed` after Checkpoint 3 approval, and to `Frozen` when the approved
recommendation is handed to a new implementation-spec workflow.

Frozen research is a historical snapshot. New evidence after freezing starts a
new dated iteration instead of rewriting the old conclusion.

### 10.2 Amendment rule

Research may uncover new foundation concerns. A concern becomes an umbrella
area when it crosses at least two existing capabilities, changes a foundation
boundary, or may change implementation order. A narrower discovery updates its
own capability document.

Each umbrella amendment records:

- date;
- triggering evidence;
- change;
- affected documents;
- earlier conclusions requiring review; and
- round or gate impact.

Amendments may strengthen or extend the research. They may not silently weaken
approved evidence standards or completion gates.

### 10.3 Checkpoint policy

Individual documents do not require a user pause. Formal review occurs at the
three checkpoints defined in Section 5. Research pauses earlier only for a
scope-changing ambiguity, unavailable evidence that blocks a round, authority
needed for an external action, or a product decision that evidence cannot make.

### 10.4 Decision discipline

The research must not:

- favor Rig because it is already present;
- favor Codex because it is Rust;
- copy Claude Code's product model because it is feature-rich;
- force sequential systems into a parallel model;
- treat conversational todos as durable workflow state;
- treat compaction summaries as persistence;
- measure orchestration maturity by subagent count; or
- add generality without a Rollshot workload.

Every final recommendation answers:

1. What Rollshot problem does this solve?
2. Which workload proves the need?
3. How do candidate systems model it?
4. Who owns state and authority?
5. How do failure, cancellation, and resume work?
6. What is the smallest independently verifiable slice?
7. Which capability is deliberately deferred?

## 11. Program completion criteria

This umbrella research is complete only when:

- the Rollshot baseline and three workload classes are documented;
- all four external core-system profiles pass Round 1's gate;
- every revision-1 capability document passes its round gate;
- Task/Todo/Workflow state has an independent comparison and disposition;
- subagents and parallelism have an independent comparison and disposition;
- compact and mini-compact mechanisms have an independent comparison and
  disposition;
- Rig retain, fork/vendor, replace, and remove boundaries are explicitly
  analyzed without upstream compatibility as a constraint;
- every admitted new area appears in the inventory and amendment log;
- the decision matrix traces claims to evidence;
- unresolved questions are classified as spike, defer, or product decision;
- the recommendation proposes staged, independently verifiable foundation
  slices instead of one platform-sized implementation; and
- the user approves the final recommendation before implementation design.

## 12. Amendment log

| Revision | Date | Trigger | Change | Affected documents | Review impact |
|---:|---|---|---|---|---|
| 1 | 2026-07-22 | Initial approved research design | Established the hybrid two-pass program, fixed core systems, revision-1 capability set, research rounds, evidence gates, and governance | Entire inventory | Begins Active Research |
| 2 | 2026-07-23 | Research program frozen | All 14 research areas dispositioned; 6 staged implementation slices recommended. Decision matrix and Rollshot recommendation reviewed and approved. 3 Round 5 gaps deferred with restart conditions. | `decision-matrix.md`, `rollshot-recommendation.md`, umbrella status | Program frozen |

