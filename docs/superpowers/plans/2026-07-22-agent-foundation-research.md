# Agent Foundation Research Execution Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> `superpowers:subagent-driven-development` (recommended) or
> `superpowers:executing-plans` to execute this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Produce the evidence-backed system profiles, capability comparisons,
decision matrix, and staged Rollshot recommendation required by the approved
Agent Foundation Umbrella Research Specification.

**Architecture:** Execute the approved hybrid two-pass program. Round 0 fixes
Rollshot and workload requirements; Round 1 builds independently reviewable
system profiles; Rounds 2–4 compare capabilities using those profiles plus
direct source evidence; Round 5 admits only named evidence gaps; Round 6
synthesizes decisions without implementing agent code.

**Tech Stack:** Markdown research artifacts, local reference repositories,
code-review-graph MCP for Rollshot exploration, `rtk`-prefixed Git and ripgrep
commands, authoritative upstream documentation only when local source cannot
answer a question.

## Global Constraints

- Source of truth: `docs/researchs/agent-foundation/README.md` at umbrella
  revision 1, plus later logged amendments.
- Research outputs live under `docs/researchs/agent-foundation/`; this plan is a
  live execution document and becomes historical after the research lands.
- Use code-review-graph before shell search for Rollshot. The learn-project
  graphs currently contain zero nodes, so shell fallback is allowed there.
- Prefix every shell command with `rtk`.
- Record checkout commit, research date, timezone, evidence type, confidence,
  and limitations in every research document.
- Distinguish source evidence, tests, official documentation, runtime
  observation, and inference.
- Treat “not found in investigated scope” as a bounded result, never proof of
  nonexistence.
- Do not silently equate Task, Todo, Workflow, Job, Agent Run, Session, Memory,
  Compact, or Artifact.
- `mini-compact` remains an umbrella label until each source mechanism is
  identified; do not equate it automatically with cached microcompact,
  snipping, projection, or pruning.
- Rig may be retained, forked/vendored, replaced, or removed. Upstream
  compatibility and avoidance of a fork are not decision criteria.
- Do not modify agent implementation code during this plan.
- Do not add a gap-driven reference without first amending the umbrella log.
- Each capability document must compare at least two materially different
  designs or explicitly defer through the approved gap process.
- Commit each independently reviewable research artifact or tightly coupled
  checkpoint update separately with a `docs(agent): ...` message.

## Shared research artifact contract

Every new research document begins with this metadata shape, filled with actual
values rather than placeholders:

```markdown
# <Document title>

**Research date:** <ISO date, Asia/Taipei>
**Status:** In Progress | Reviewed | Superseded
**Umbrella revision:** <number>
**Research round:** <number>
**Systems/capabilities:** <explicit list>
**Evidence baseline:** <repository hashes and/or official-doc dates>
```

Before committing any research artifact, run:

```bash
rtk git diff --check
rtk rg -n "TB[D]|TO[D]O|FIXM[E]|implement[ ]later|fill[ ]in" <artifact>
```

The `rg` command must return no matches. Then verify that every substantive
claim has a nearby path, symbol, test, authoritative link, or explicit
`Inference:` label.

---

### Task 1: Round 0 Rollshot baseline and workload requirements

**Files:**
- Create: `docs/researchs/agent-foundation/00-rollshot-baseline-workloads.md`
- Read: `docs/researchs/agent-foundation/README.md`
- Read: `crates/rollshot-agent/src/domain.rs`
- Read: `crates/rollshot-agent/src/driver.rs`
- Read: `crates/rollshot-agent/src/model.rs`
- Read: `crates/rollshot-agent/src/provider.rs`
- Read: `crates/rollshot-agent/src/runtime.rs`
- Read: `crates/rollshot-agent/src/tools.rs`
- Read: `learn-projects/rig/crates/rig-core/src/agent/`
- Read: `learn-projects/brag/skills/brag/SKILL.md`
- Read: `learn-projects/hyperframes/skills/hyperframes-core/references/production-loop.md`
- Read: `learn-projects/hyperframes/skills/hyperframes-core/references/review-loop.md`
- Read: `learn-projects/hyperframes/skills/hyperframes-core/references/subagent-dispatch.md`

**Produces:** The canonical current-state and workload vocabulary consumed by
all later capability documents.

- [ ] **Step 1: Capture the reproducibility baseline**

Record Rollshot HEAD and the Rig, brag, and Hyperframes checkout hashes. State
that code wins over historical Rollshot docs.

- [ ] **Step 2: Map the current bounded agent**

Use code-review-graph to locate the agent runner, session, provider facade,
tool registry, budgets, cancellation, events, and terminal states. Read the
focused source and describe ownership, persistence, serial execution, and
current in-memory boundaries.

- [ ] **Step 3: Audit the Rig boundary**

Trace exactly which Rig types and invariants Rollshot consumes. Separate public
Rollshot contracts from crate-internal Rig state-machine use. Describe retain,
fork/vendor, replace, and remove as available outcomes without selecting one.

- [ ] **Step 4: Define the workload ladder**

Document Smart Redaction, Action Guide, and brag/Hyperframes as three pressure
levels. For every required capability, cite the code or workflow step that
creates the need; do not infer a general platform requirement from an unused
upstream feature.

- [ ] **Step 5: Write the baseline artifact**

Use sections for reproducibility, terminology, current architecture, Rig
boundary, workload profiles, proven gaps, current strengths, unknowns, and
evidence index. Mark static-only conclusions explicitly.

- [ ] **Step 6: Verify and commit**

Run the shared checks plus:

```bash
rtk rg -n "Smart Redaction|Action Guide|brag|Hyperframes|Rig|Evidence index" docs/researchs/agent-foundation/00-rollshot-baseline-workloads.md
rtk git add docs/researchs/agent-foundation/00-rollshot-baseline-workloads.md
rtk git commit -m "docs(agent): establish foundation research baseline"
```

Expected: every workload and the Rig boundary appears, `git diff --check`
passes, and the commit contains only the baseline artifact.

### Task 2: Round 1 Pi system profile

**Files:**
- Create: `docs/researchs/agent-foundation/systems/pi.md`
- Read: `learn-projects/pi/packages/agent/src/`
- Read: `learn-projects/pi/packages/agent/test/agent-loop.test.ts`
- Read: `learn-projects/pi/packages/agent/docs/agent-harness.md`
- Read: `learn-projects/pi/packages/ai/src/`
- Read: `learn-projects/pi/packages/coding-agent/docs/sessions.md`
- Read: `learn-projects/pi/packages/coding-agent/docs/session-format.md`
- Read: `learn-projects/pi/packages/coding-agent/docs/skills.md`
- Read: `learn-projects/pi/packages/coding-agent/docs/extensions.md`
- Read: `learn-projects/pi/packages/coding-agent/src/core/skills.ts`

**Produces:** A source-backed profile of Pi's small loop, provider boundary,
sessions, tools, extensions, skills, and deliberately absent semantics.

- [ ] **Step 1: Record Pi's hash and package boundaries**
- [ ] **Step 2: Trace one complete model/tool loop through source and tests**
- [ ] **Step 3: Trace session persistence, steering/follow-up, compaction hooks, skills, and extensions**
- [ ] **Step 4: Search explicitly for task/todo, child-agent, parallel, job, checkpoint, and resume semantics; bound every absence claim**
- [ ] **Step 5: Write all 17 system-profile sections from the umbrella template**
- [ ] **Step 6: Verify that implemented, optional, extension-provided, and absent behavior are distinct**

```bash
rtk rg -n "Architecture|Task|Subagent|Compaction|Persistence|Skills|Permissions|Evidence index" docs/researchs/agent-foundation/systems/pi.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/systems/pi.md
rtk git commit -m "docs(agent): profile pi architecture"
```

### Task 3: Round 1 oh-my-pi system profile

**Files:**
- Create: `docs/researchs/agent-foundation/systems/oh-my-pi.md`
- Read: `learn-projects/oh-my-pi/packages/agent/src/`
- Read: `learn-projects/oh-my-pi/packages/ai/src/`
- Read: `learn-projects/oh-my-pi/packages/coding-agent/src/extensibility/skills.ts`
- Read: `learn-projects/oh-my-pi/packages/coding-agent/src/capability/`
- Read: `learn-projects/oh-my-pi/packages/coding-agent/src/internal-urls/skill-protocol.ts`
- Read: `learn-projects/oh-my-pi/packages/coding-agent/src/session/`
- Read: `learn-projects/oh-my-pi/packages/coding-agent/examples/hooks/custom-compaction.ts`

**Produces:** A profile that isolates inherited Pi behavior from oh-my-pi's own
capabilities, internal resource protocols, session variants, and added
extensibility.

- [ ] **Step 1: Record the hash and identify fork lineage versus original additions**
- [ ] **Step 2: Trace agent loop, session tree/storage, tools, and providers**
- [ ] **Step 3: Trace capability discovery, `skill://` resolution, managed skills, and compaction hooks**
- [ ] **Step 4: Bound task/subagent/job/parallel claims through source and tests**
- [ ] **Step 5: Write all 17 profile sections, labeling inherited and oh-my-pi-specific behavior**
- [ ] **Step 6: Verify and commit**

```bash
rtk rg -n "Pi lineage|Capability|skill://|Compaction|Task|Subagent|Evidence index" docs/researchs/agent-foundation/systems/oh-my-pi.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/systems/oh-my-pi.md
rtk git commit -m "docs(agent): profile oh-my-pi architecture"
```

### Task 4: Round 1 Codex system profile

**Files:**
- Create: `docs/researchs/agent-foundation/systems/codex.md`
- Read: `learn-projects/codex/codex-rs/core/src/session/`
- Read: `learn-projects/codex/codex-rs/core/src/compact.rs`
- Read: `learn-projects/codex/codex-rs/core/src/tools/`
- Read: `learn-projects/codex/codex-rs/core/src/codex_thread.rs`
- Read: `learn-projects/codex/codex-rs/ext/skills/src/`
- Read: `learn-projects/codex/codex-rs/core-skills/src/`
- Read: `learn-projects/codex/codex-rs/model-provider-info/src/lib.rs`
- Read: `learn-projects/codex/codex-rs/protocol/src/protocol.rs`
- Read: `learn-projects/codex/codex-rs/exec-server/`

**Produces:** A profile of Codex's Rust lifecycle, skills authority, sandbox and
approval policies, delegation, context management, and Responses-oriented
provider boundary.

- [ ] **Step 1: Record hash, crate boundaries, and enabled versus feature-gated paths**
- [ ] **Step 2: Trace thread/session/run and tool execution lifecycles**
- [ ] **Step 3: Trace compaction, delegation, approvals, permission profiles, sandboxing, and execution server ownership**
- [ ] **Step 4: Trace skills catalog, authority-preserving list/read/search, context budget, and invocation selection**
- [ ] **Step 5: Trace model-provider configuration and distinguish endpoint configurability from wire-protocol neutrality**
- [ ] **Step 6: Write all 17 profile sections with platform and experimental limitations**
- [ ] **Step 7: Verify and commit**

```bash
rtk rg -n "Thread|Delegation|Compaction|SkillAuthority|Sandbox|Approval|Responses|Evidence index" docs/researchs/agent-foundation/systems/codex.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/systems/codex.md
rtk git commit -m "docs(agent): profile codex architecture"
```

### Task 5: Round 1 Claude Code system profile

**Files:**
- Create: `docs/researchs/agent-foundation/systems/claude-code.md`
- Read: `learn-projects/claude-code-source-code/src/Task.ts`
- Read: `learn-projects/claude-code-source-code/src/tasks/`
- Read: `learn-projects/claude-code-source-code/src/QueryEngine.ts`
- Read: `learn-projects/claude-code-source-code/src/services/compact/`
- Read: `learn-projects/claude-code-source-code/src/services/tools/`
- Read: `learn-projects/claude-code-source-code/src/skills/`
- Read: `learn-projects/claude-code-source-code/src/bootstrap/state.ts`
- Read: `learn-projects/claude-code-source-code/src/bridge/`
- Read: `learn-projects/claude-code-source-code/src/memdir/`
- Read: `learn-projects/claude-code-source-code/src/Tool.ts`

**Produces:** A profile that separates task infrastructure, todos, local and
remote agents, in-process teammates, session resume, memory, full compaction,
history snipping, and cached microcompact.

- [ ] **Step 1: Record hash and label implemented, feature-gated, hidden, disabled, and roadmap-only code**
- [ ] **Step 2: Trace Task types, statuses, handles, disk output, kill paths, and root-store ownership**
- [ ] **Step 3: Trace local/background/remote agents, teammates, context construction, permissions, and resume reconstruction**
- [ ] **Step 4: Trace auto/reactive compaction, compact boundaries, snip projection, cached microcompact, and post-compact preservation**
- [ ] **Step 5: Trace session persistence, bridge resume, memory directories, skills, and tool orchestration**
- [ ] **Step 6: Write all 17 profile sections without treating README reverse-engineering claims as source proof**
- [ ] **Step 7: Verify and commit**

```bash
rtk rg -n "TaskStatus|Background|Teammate|Resume|compact boundary|microcompact|snip|Memory|Evidence index" docs/researchs/agent-foundation/systems/claude-code.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/systems/claude-code.md
rtk git commit -m "docs(agent): profile claude code architecture"
```

### Task 6: Checkpoint 1 taxonomy review

**Files:**
- Modify if evidence requires: `docs/researchs/agent-foundation/README.md`
- Modify if corrections are required: `docs/researchs/agent-foundation/systems/*.md`
- Read: all Round 0 and Round 1 artifacts

**Produces:** User-approved vocabulary and false-equivalence audit before
capability synthesis.

- [ ] **Step 1: Build a cross-profile term table in the checkpoint report sent to the user**

Include Task, Todo, Workflow, Job, Agent Run, Session, Child Agent, Compact,
Microcompact, Memory, Artifact, and Resume. Show “absent” rather than forcing a
mapping.

- [ ] **Step 2: Audit contradictions and evidence levels across profiles**
- [ ] **Step 3: Present Checkpoint 1 and wait for user review**
- [ ] **Step 4: Apply approved corrections and log any umbrella amendment**
- [ ] **Step 5: Mark the four profiles `Reviewed` and commit the checkpoint update**

```bash
rtk rg -L "Status:.*Reviewed" docs/researchs/agent-foundation/systems/*.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/README.md docs/researchs/agent-foundation/systems
rtk git commit -m "docs(agent): approve agent system taxonomy"
```

Expected: `rg -L` prints nothing, or the task remains blocked at Checkpoint 1.

### Task 7: Round 2 Task, Todo, and Workflow state comparison

**Files:**
- Create: `docs/researchs/agent-foundation/capabilities/task-todo-workflow-state.md`
- Read: Round 0 baseline and all reviewed system profiles
- Re-read focused task/session source cited by those profiles

**Produces:** A state-machine and ownership comparison that preserves
sequential-only, flat-todo, durable-task, and dependency-aware distinctions.

- [ ] **Step 1: Define non-equivalent terms and Rollshot workload requirements**
- [ ] **Step 2: Draw each implemented state machine and identify its owner and persistence boundary**
- [ ] **Step 3: Compare IDs, dependencies, parallel readiness, outputs, errors, visibility, and recovery**
- [ ] **Step 4: Compare at least two candidate Rollshot patterns without final selection**
- [ ] **Step 5: Verify every matrix cell has evidence, bounded absence, or inference label**
- [ ] **Step 6: Commit**

```bash
rtk rg -n "Task|Todo|Workflow|State machine|Ownership|Persistence|Alternatives|Evidence index" docs/researchs/agent-foundation/capabilities/task-todo-workflow-state.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/capabilities/task-todo-workflow-state.md
rtk git commit -m "docs(agent): compare task and workflow state"
```

### Task 8: Round 2 Context compaction comparison

**Files:**
- Create: `docs/researchs/agent-foundation/capabilities/context-compaction.md`
- Read: reviewed profiles and their cited compaction source/tests

**Produces:** An evidence-backed taxonomy of full compact, auto/reactive
compact, mini/micro compact, snipping, pruning, projection, and tool-result
compression.

- [ ] **Step 1: Define context pressure and continuity requirements by workload**
- [ ] **Step 2: Trace triggers, algorithms, boundaries, cache effects, and failure paths per system**
- [ ] **Step 3: Inventory preservation of skills, tasks, decisions, permissions, artifacts, and pending gates**
- [ ] **Step 4: Separate compaction from persistence and memory**
- [ ] **Step 5: Compare at least two Rollshot patterns and list measurable evaluation criteria**
- [ ] **Step 6: Commit**

```bash
rtk rg -n "full compaction|reactive|mini-compact|microcompact|snip|projection|Persistence|Memory|Evidence index" docs/researchs/agent-foundation/capabilities/context-compaction.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/capabilities/context-compaction.md
rtk git commit -m "docs(agent): compare context compaction models"
```

### Task 9: Round 2 Memory comparison

**Files:**
- Create: `docs/researchs/agent-foundation/capabilities/memory.md`
- Read: reviewed profiles and cited memory/session source

**Produces:** A boundary recommendation space for run, session, project, user,
team, and consolidation memory without conflating them with compacted context.

- [ ] **Step 1: Inventory memory scopes, writers, readers, retention, and deletion**
- [ ] **Step 2: Compare retrieval, provenance, privacy, poisoning, and expiry behavior**
- [ ] **Step 3: Map data classes to memory, workflow state, compacted context, or artifact storage**
- [ ] **Step 4: Compare candidate Rollshot patterns and explicit non-goals**
- [ ] **Step 5: Verify and commit**

```bash
rtk rg -n "Run memory|Session|Project|User|Team|Consolidation|Privacy|Alternatives|Evidence index" docs/researchs/agent-foundation/capabilities/memory.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/capabilities/memory.md
rtk git commit -m "docs(agent): compare agent memory boundaries"
```

### Task 10: Round 2 Persistence, checkpoint, and resume comparison

**Files:**
- Create: `docs/researchs/agent-foundation/capabilities/persistence-checkpoint-resume.md`
- Read: reviewed profiles and cited session/resume source

**Produces:** A comparison of event-log, snapshot, transcript,
artifact-driven, and hybrid recovery models.

- [ ] **Step 1: Define crash and resume scenarios for all three workloads**
- [ ] **Step 2: Compare durable decisions, checkpoints, partial writes, reconstruction, and next-step routing**
- [ ] **Step 3: Analyze stale skills, providers, permissions, tool handles, and external jobs**
- [ ] **Step 4: Compare at least two Rollshot persistence patterns**
- [ ] **Step 5: Verify and commit**

```bash
rtk rg -n "Event log|Snapshot|Transcript|Artifact|Checkpoint|Resume|Crash|Stale|Evidence index" docs/researchs/agent-foundation/capabilities/persistence-checkpoint-resume.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/capabilities/persistence-checkpoint-resume.md
rtk git commit -m "docs(agent): compare checkpoint and resume models"
```

### Task 11: Round 3 Subagents and parallelism comparison

**Files:**
- Create: `docs/researchs/agent-foundation/capabilities/subagents-and-parallelism.md`
- Read: reviewed profiles
- Read: `learn-projects/hyperframes/skills/hyperframes-core/references/subagent-dispatch.md`
- Read: `learn-projects/hyperframes/skills/general-video/SKILL.md`

**Produces:** A comparison of spawn, fork, teammate, remote-agent, worker, and
inline execution economics.

- [ ] **Step 1: Define child-run isolation and Hyperframes artifact-worker requirements**
- [ ] **Step 2: Compare context inheritance, scopes, budgets, cancellation, queues, and concurrency caps**
- [ ] **Step 3: Compare notification-based and artifact-based completion plus selective retry**
- [ ] **Step 4: Document when sequential inline execution is preferable**
- [ ] **Step 5: Compare candidate Rollshot child-run patterns and commit**

```bash
rtk rg -n "Spawn|Fork|Teammate|Worker|Isolation|Concurrency|Artifact|Retry|Inline|Evidence index" docs/researchs/agent-foundation/capabilities/subagents-and-parallelism.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/capabilities/subagents-and-parallelism.md
rtk git commit -m "docs(agent): compare subagent orchestration"
```

### Task 12: Round 3 Long-running jobs comparison

**Files:**
- Create: `docs/researchs/agent-foundation/capabilities/long-running-jobs.md`
- Read: reviewed profiles
- Read: Hyperframes render/progress references cited by the baseline
- Read: `crates/rollshot-app/src/action_guide_home/video_import.rs`
- Read: `crates/rollshot-app/src/managed_ffmpeg.rs`

**Produces:** A lifecycle comparison for local processes, preview servers,
media operations, and remote jobs that outlive model turns.

- [ ] **Step 1: Define start, observe, cancel, collect, cleanup, and reattach semantics**
- [ ] **Step 2: Compare process handles, remote handles, progress, logs, cost, and partial artifacts**
- [ ] **Step 3: Separate agent run wall time from external job lifetime**
- [ ] **Step 4: Compare candidate host-owned job models and Rollshot app coordinator reuse**
- [ ] **Step 5: Verify and commit**

```bash
rtk rg -n "Start|Progress|Cancel|Collect|Cleanup|Reattach|Wall time|Remote|Evidence index" docs/researchs/agent-foundation/capabilities/long-running-jobs.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/capabilities/long-running-jobs.md
rtk git commit -m "docs(agent): compare long-running job models"
```

### Task 13: Round 3 Tools and scheduling comparison

**Files:**
- Create: `docs/researchs/agent-foundation/capabilities/tools-and-scheduling.md`
- Read: reviewed profiles and cited tool runtimes

**Produces:** A comparison of typed/dynamic tools, availability, authorization,
serial/parallel scheduling, hooks, and tool-result lifecycle.

- [ ] **Step 1: Compare registration, discovery, schema, description, availability, authorization, and selection**
- [ ] **Step 2: Compare serial, ordered parallel, dependency-aware, and terminal-tool execution**
- [ ] **Step 3: Compare idempotency, side-effect classes, hooks, result retention, and compaction**
- [ ] **Step 4: Compare candidate Rollshot scheduling models and commit**

```bash
rtk rg -n "Registration|Availability|Authorization|Serial|Parallel|Idempotency|Hooks|Result|Evidence index" docs/researchs/agent-foundation/capabilities/tools-and-scheduling.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/capabilities/tools-and-scheduling.md
rtk git commit -m "docs(agent): compare tool scheduling models"
```

### Task 14: Round 3 Budgets, cancellation, retries, and failures

**Files:**
- Create: `docs/researchs/agent-foundation/capabilities/budgets-cancellation-retries.md`
- Read: reviewed profiles and Rollshot runtime baseline

**Produces:** A comparison of hierarchical resource control and actionable
failure semantics.

- [ ] **Step 1: Compare token, cost, time, tool, child, job, and artifact budgets**
- [ ] **Step 2: Compare allocation from parent to child and accounting after resume**
- [ ] **Step 3: Compare cancellation propagation, cleanup, retry ownership, limits, and idempotency**
- [ ] **Step 4: Normalize provider, protocol, validation, runtime, blocked, and exhausted failures**
- [ ] **Step 5: Compare Rollshot patterns and commit**

```bash
rtk rg -n "Token|Cost|Wall time|Child|Cancellation|Retry|Idempotency|Terminal|Evidence index" docs/researchs/agent-foundation/capabilities/budgets-cancellation-retries.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/capabilities/budgets-cancellation-retries.md
rtk git commit -m "docs(agent): compare budgets and failure control"
```

### Task 15: Round 4 Skills and extensions comparison

**Files:**
- Create: `docs/researchs/agent-foundation/capabilities/skills-and-extensions.md`
- Read: reviewed profiles and cited skill/extension source

**Produces:** A comparison of instruction/resource skills, executable
extensions, discovery, snapshots, invocation, and authority boundaries.

- [ ] **Step 1: Compare source discovery, trust, metadata loading, and explicit/implicit invocation**
- [ ] **Step 2: Compare instruction-only packages, scripts, executable modules, and extension hooks**
- [ ] **Step 3: Compare opaque resources, source authority, context budgets, versions, and compaction preservation**
- [ ] **Step 4: Separate requested capabilities from granted permissions**
- [ ] **Step 5: Compare candidate Rollshot MVP boundaries and commit**

```bash
rtk rg -n "Discovery|Trust|Metadata|Invocation|Instruction|Extension|Authority|Snapshot|Evidence index" docs/researchs/agent-foundation/capabilities/skills-and-extensions.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/capabilities/skills-and-extensions.md
rtk git commit -m "docs(agent): compare skills and extension models"
```

### Task 16: Round 4 Permissions and sandboxing comparison

**Files:**
- Create: `docs/researchs/agent-foundation/capabilities/permissions-and-sandboxing.md`
- Read: reviewed profiles and cited permission/sandbox source

**Produces:** A comparison of authority, trust, approval, escalation, and
fail-closed behavior across foreground and background execution.

- [ ] **Step 1: Inventory filesystem, process, network, credential, capture, and publish authority**
- [ ] **Step 2: Compare sandbox policies, project trust, approvals, caching, and escalation**
- [ ] **Step 3: Compare child/background behavior, disconnect, resume, and revocation**
- [ ] **Step 4: Compare Rollshot patterns with privacy and audit consequences**
- [ ] **Step 5: Verify and commit**

```bash
rtk rg -n "Filesystem|Process|Network|Credential|Capture|Approval|Escalation|Fail.closed|Evidence index" docs/researchs/agent-foundation/capabilities/permissions-and-sandboxing.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/capabilities/permissions-and-sandboxing.md
rtk git commit -m "docs(agent): compare permission and sandbox models"
```

### Task 17: Round 4 Artifacts, review, and provenance comparison

**Files:**
- Create: `docs/researchs/agent-foundation/capabilities/artifacts-review-provenance.md`
- Read: reviewed profiles and workload artifacts

**Produces:** A comparison of typed artifacts, ambient files, validation
evidence, review decisions, revisions, and provenance.

- [ ] **Step 1: Compare artifact identity, mutability, storage, expected-output contracts, and completion**
- [ ] **Step 2: Compare draft, validation, approval, rejection, correction, and revision lifecycles**
- [ ] **Step 3: Compare skill/tool/model/source/user-decision provenance and privacy**
- [ ] **Step 4: Map judgment-to-deterministic-execution boundaries for Rollshot workloads**
- [ ] **Step 5: Verify and commit**

```bash
rtk rg -n "Typed artifact|Ambient file|Validation|Review|Revision|Provenance|Privacy|Evidence index" docs/researchs/agent-foundation/capabilities/artifacts-review-provenance.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/capabilities/artifacts-review-provenance.md
rtk git commit -m "docs(agent): compare artifact and review models"
```

### Task 18: Round 4 Events, observability, and steering comparison

**Files:**
- Create: `docs/researchs/agent-foundation/capabilities/events-observability-steering.md`
- Read: reviewed profiles and cited event/query/tool source

**Produces:** A lifecycle-event and interaction comparison suitable for UI
reconstruction, progress reporting, steering, and audit.

- [ ] **Step 1: Compare run, turn, message, tool, task, compact, job, artifact, and terminal events**
- [ ] **Step 2: Compare transient display events, durable audit events, and reconnect reconstruction**
- [ ] **Step 3: Compare steering, follow-up, interrupt, checkpoint, and queued input behavior**
- [ ] **Step 4: Compare privacy-safe Rollshot event models and commit**

```bash
rtk rg -n "Run|Turn|Tool|Task|Compact|Job|Artifact|Audit|Steering|Reconnect|Evidence index" docs/researchs/agent-foundation/capabilities/events-observability-steering.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/capabilities/events-observability-steering.md
rtk git commit -m "docs(agent): compare events and steering models"
```

### Task 19: Round 4 Provider and context boundaries comparison

**Files:**
- Create: `docs/researchs/agent-foundation/capabilities/provider-and-context-boundaries.md`
- Read: reviewed profiles and cited provider adapters

**Produces:** A comparison of provider-neutral ownership, streaming, tool-call
normalization, context windows, unsupported capabilities, and handoff.

- [ ] **Step 1: Compare request, message, streaming, usage, stop, and tool-call abstractions**
- [ ] **Step 2: Compare model context limits, compaction triggers, and capability negotiation**
- [ ] **Step 3: Compare provider switching/handoff and state that must remain Rollshot-owned**
- [ ] **Step 4: Reassess Rig's provider and state-machine boundary using accumulated evidence**
- [ ] **Step 5: Verify and commit**

```bash
rtk rg -n "Provider.neutral|Streaming|Tool call|Context window|Capability|Handoff|Rig|Evidence index" docs/researchs/agent-foundation/capabilities/provider-and-context-boundaries.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/capabilities/provider-and-context-boundaries.md
rtk git commit -m "docs(agent): compare provider and context boundaries"
```

### Task 20: Checkpoint 2 capability review and Round 5 admission

**Files:**
- Modify if required: `docs/researchs/agent-foundation/README.md`
- Modify if corrections are approved: `docs/researchs/agent-foundation/capabilities/*.md`
- Read: all Round 2–4 artifacts

**Produces:** Reviewed capability evidence and an explicit decision on every
remaining evidence gap.

- [ ] **Step 1: Audit every capability against its round gate and shared artifact contract**
- [ ] **Step 2: Classify each open question as answered, gap-driven research candidate, bounded spike, product decision, or defer**
- [ ] **Step 3: Present Checkpoint 2 with proposed additions and wait for user review**
- [ ] **Step 4: For each approved external reference, amend the umbrella before investigating it**
- [ ] **Step 5: Execute each admitted narrow investigation as its own reviewed document and commit**
- [ ] **Step 6: Mark completed capability documents `Reviewed` and commit checkpoint corrections**

```bash
rtk rg -L "Status:.*Reviewed" docs/researchs/agent-foundation/capabilities/*.md
rtk git diff --check
```

Expected: no planned capability file is omitted; `rg -L` prints nothing after
Checkpoint 2 approval. If a gap is admitted, synthesis remains blocked until
the added document closes or explicitly defers that named gap.

### Task 21: Round 6 Decision matrix

**Files:**
- Create: `docs/researchs/agent-foundation/decision-matrix.md`
- Read: all reviewed baseline, system, capability, and admitted gap artifacts

**Produces:** A traceable disposition for every umbrella capability.

- [ ] **Step 1: Create one row per known and amended research area**

Each row contains Rollshot need, workload evidence, candidate patterns,
selected disposition, rationale, authority owner, persistence owner, failure
model, smallest verifiable slice, deferred portion, evidence links, and
confidence.

- [ ] **Step 2: Use only the five approved dispositions**

Use retain, adopt, combine, spike, or defer. A combine row must name every
source pattern and the boundary between them.

- [ ] **Step 3: Add explicit Rig option analysis**

Include retain, fork/vendor, replace, and remove. Do not score upstream
compatibility or avoidance of divergence as benefits.

- [ ] **Step 4: Audit evidence traceability and contradictions**
- [ ] **Step 5: Verify and commit**

```bash
rtk rg -n "retain|adopt|combine|spike|defer|Rig|Workload evidence|Smallest verifiable slice|Confidence" docs/researchs/agent-foundation/decision-matrix.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/decision-matrix.md
rtk git commit -m "docs(agent): synthesize foundation decision matrix"
```

### Task 22: Round 6 Rollshot recommendation

**Files:**
- Create: `docs/researchs/agent-foundation/rollshot-recommendation.md`
- Read: `docs/researchs/agent-foundation/decision-matrix.md`
- Read: approved umbrella

**Produces:** A staged, evidence-backed foundation recommendation that can later
seed separate implementation specs.

- [ ] **Step 1: State the recommended foundation boundaries and non-goals**
- [ ] **Step 2: Order independently verifiable slices by dependency and workload value**
- [ ] **Step 3: For each slice, state problem, workload, adopted patterns, state/authority owner, failure/cancel/resume behavior, acceptance evidence, and deferred scope**
- [ ] **Step 4: Identify product decisions, technical spikes, migrations, and compatibility risks separately**
- [ ] **Step 5: Explain the recommended Rig disposition without treating it as an architectural prerequisite**
- [ ] **Step 6: Check every recommendation against a decision-matrix row and commit**

```bash
rtk rg -n "Boundaries|Non-goals|Stage|Workload|State owner|Authority|Failure|Resume|Rig|Spike|Deferred|Evidence" docs/researchs/agent-foundation/rollshot-recommendation.md
rtk git diff --check
rtk git add docs/researchs/agent-foundation/rollshot-recommendation.md
rtk git commit -m "docs(agent): recommend staged agent foundation"
```

### Task 23: Checkpoint 3 approval and research freeze

**Files:**
- Modify: `docs/researchs/agent-foundation/README.md`
- Modify if approved corrections require it: `docs/researchs/agent-foundation/decision-matrix.md`
- Modify if approved corrections require it: `docs/researchs/agent-foundation/rollshot-recommendation.md`

**Produces:** The approved, frozen research snapshot and an explicit boundary
before implementation design.

- [ ] **Step 1: Present the matrix and recommendation at Checkpoint 3**
- [ ] **Step 2: Apply user corrections and re-run traceability checks**
- [ ] **Step 3: Mark matrix and recommendation `Reviewed`**
- [ ] **Step 4: Update umbrella status from `Synthesis` to `Reviewed`, then to `Frozen` when handed to a new implementation-spec workflow**
- [ ] **Step 5: Record the final amendment-log entry and verify program completion criteria line by line**
- [ ] **Step 6: Commit the approved snapshot**

```bash
rtk rg -n "Status:.*Frozen|Program completion criteria|Amendment log" docs/researchs/agent-foundation/README.md
rtk rg -n "Status:.*Reviewed" docs/researchs/agent-foundation/{decision-matrix.md,rollshot-recommendation.md}
rtk git diff --check
rtk git add docs/researchs/agent-foundation
rtk git commit -m "docs(agent): freeze foundation research"
```

Expected: all umbrella completion criteria are supported by reviewed artifacts;
no agent implementation code is present in the research commits. A separate,
user-approved implementation spec is the only allowed next design artifact.
