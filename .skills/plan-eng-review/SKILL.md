---
name: plan-eng-review
description: |
  Eng manager-mode plan review. Lock in the execution plan — architecture,
  data flow, diagrams, edge cases, test coverage, performance. Walks through
  issues interactively, or applies recommendations automatically when explicitly
  requested. Use when asked to
  "review the architecture", "engineering review", "tech review", or "lock in the plan".
  Proactively suggest when the user has a plan or design doc and is about to
  start coding — to catch architecture issues before implementation.
allowed-tools:
  - Read
  - Write
  - Edit
  - Grep
  - Glob
  - AskUserQuestion
  - Bash
  - WebSearch
---

# Plan Review Mode

Review this plan thoroughly before making any code changes. For every issue or recommendation, explain the concrete tradeoffs and give an opinionated recommendation. Ask for user input before assuming a direction unless auto mode is active.

## Review modes

Choose the mode from the user's current request:

- **Interactive mode (default):** Use the review section gates and ask the user to decide each issue.
- **Auto mode:** Activate only when the user explicitly asks for "auto mode", "automatic review", "自動審查", "自動修訂", or an equivalent unambiguous instruction. Do not infer auto mode from a request to review quickly or comprehensively.

### Auto mode behavior

Auto mode overrides every later instruction to call `AskUserQuestion`, pause, wait, or stop after an issue or review section. All review checks, required outputs, issue limits, and decision-brief quality requirements still apply.

1. Complete Step 0 and all four review sections before editing the plan.
2. For every issue, write the normal decision brief as `Auto decision D<N>`, choose the recommended option, and record the reasoning. Do not call `AskUserQuestion`.
3. If the complexity check triggers, adopt the recommended minimum viable scope. Move deferred work to `NOT in scope`; never silently delete requirements.
4. After the complete review, apply all recorded decisions to the plan in one editing pass.
5. Re-read the revised plan and verify task/file declarations, TDD order, Run/Expected pairs, commit boundaries, required outputs, and internal consistency.
6. In the completion summary, list every auto decision and summarize the edits made.

Stop and ask the user only when continuing would require guessing:

- no single review target can be identified;
- the plan cannot be parsed well enough to edit safely;
- an issue has no defensible recommended option;
- recommendations conflict and cannot be reconciled without changing the user's stated goal.

Auto mode edits only the reviewed plan. Do not implement product code, create commits, or change unrelated files.

## Priority hierarchy

If the system triggers context compaction: Step 0 > Test diagram > Opinionated recommendations > Everything else. Never skip Step 0 or the test diagram. Do not preemptively warn about context limits — the system handles compaction automatically.

## Engineering preferences (use these to guide recommendations)

* DRY is important — flag repetition aggressively.
* Well-tested code is non-negotiable; rather too many tests than too few.
* "Engineered enough" — not under-engineered (fragile, hacky) and not over-engineered (premature abstraction, unnecessary complexity).
* Err on the side of handling more edge cases, not fewer; thoughtfulness > speed.
* Bias toward explicit over clever.
* Right-sized diff: favor the smallest diff that cleanly expresses the change ... but don't compress a necessary rewrite into a minimal patch. If the existing foundation is broken, say "scrap it and do this instead."

## Cognitive Patterns — How Great Eng Managers Think

These are not additional checklist items. They are the instincts that experienced engineering leaders develop over years — the pattern recognition that separates "reviewed the code" from "caught the landmine." Apply them throughout your review.

1. **State diagnosis** — Teams exist in four states: falling behind, treading water, repaying debt, innovating. Each demands a different intervention (Larson, An Elegant Puzzle).
2. **Blast radius instinct** — Every decision evaluated through "what's the worst case and how many systems/people does it affect?"
3. **Boring by default** — "Every company gets about three innovation tokens." Everything else should be proven technology (McKinley, Choose Boring Technology).
4. **Incremental over revolutionary** — Strangler fig, not big bang. Canary, not global rollout. Refactor, not rewrite (Fowler).
5. **Systems over heroes** — Design for tired humans at 3am, not your best engineer on their best day.
6. **Reversibility preference** — Feature flags, A/B tests, incremental rollouts. Make the cost of being wrong low.
7. **Failure is information** — Blameless postmortems, error budgets, chaos engineering. Incidents are learning opportunities, not blame events (Allspaw, Google SRE).
8. **Org structure IS architecture** — Conway's Law in practice. Design both intentionally (Skelton/Pais, Team Topologies).
9. **DX is product quality** — Slow CI, bad local dev, painful deploys → worse software, higher attrition. Developer experience is a leading indicator.
10. **Essential vs accidental complexity** — Before adding anything: "Is this solving a real problem or one we created?" (Brooks, No Silver Bullet).
11. **Two-week smell test** — If a competent engineer can't ship a small feature in two weeks, you have an onboarding problem disguised as architecture.
12. **Glue work awareness** — Recognize invisible coordination work. Value it, but don't let people get stuck doing only glue (Reilly, The Staff Engineer's Path).
13. **Make the change easy, then make the easy change** — Refactor first, implement second. Never structural + behavioral changes simultaneously (Beck).
14. **Own your code in production** — No wall between dev and ops. "The DevOps movement is ending because there are only engineers who write code and own it in production" (Majors).
15. **Error budgets over uptime targets** — SLO of 99.9% = 0.1% downtime *budget to spend on shipping*. Reliability is resource allocation (Google SRE).

When evaluating architecture, think "boring by default." When reviewing tests, think "systems over heroes." When assessing complexity, ask Brooks's question. When a plan introduces new infrastructure, check whether it's spending an innovation token wisely.

## Documentation and diagrams

* ASCII art diagrams are valued highly — for data flow, state machines, dependency graphs, processing pipelines, and decision trees. Use them liberally in plans and design docs.
* For particularly complex designs or behaviors, embed ASCII diagrams directly in code comments in the appropriate places: data types (relationships, state transitions), services (processing pipelines), and tests (what's being set up and why) when the test structure is non-obvious.
* **Diagram maintenance is part of the change.** When modifying code that has ASCII diagrams in comments nearby, review whether those diagrams are still accurate. Update them as part of the same commit. Stale diagrams are worse than no diagrams — they actively mislead. Flag any stale diagrams encountered during review even if outside the immediate scope of the change.

## BEFORE YOU START

### Locate the plan

The input to this skill is a **superpowers-style implementation plan** in `docs/superpowers/plans/`. These are TDD-format plans produced by `superpowers:writing-plans` and executed by `superpowers:executing-plans` or `superpowers:subagent-driven-development`.

Plan locations to check (in order):

```bash
ls -1t docs/superpowers/plans/*.md 2>/dev/null
ls -1t docs/superpowers/specs/*.md 2>/dev/null   # specs feed into plans; sometimes reviewed jointly
git diff --name-only main...HEAD 2>/dev/null | grep -iE 'plans?/.*\.md$'
```

If the user named a specific file, use that. If multiple plans exist and no specific one was named, ask which one. Read the plan fully before Step 0.

**The plan format** (recognize these markers so the review questions line up with the structure):

```
# <Plan Title>
> [optional] For agentic workers: REQUIRED SUB-SKILL ...

**Goal:** <one-line outcome>
**Architecture:** <crate / module layout>
**Tech Stack:** <languages, frameworks>

## File Structure
- Create: path/to/file  <short purpose>
- Modify: path/to/file  <short purpose>

## Task N: <Task name>
**Files:**
- Create / Modify: ...

- [ ] **Step 1: <Action>**
  <code block or command>
  Expected: PASS / FAIL <signal>

- [ ] **Step 2: ...**
...
```

A well-formed superpowers plan has these properties — the review should check each:

1. Tasks are independently shippable (could land as one PR each, in order).
2. Steps inside a task are small and individually verifiable (each has a Run/Expected pair).
3. Tests appear before the implementation that satisfies them (red → green → commit).
4. Each task ends with a commit step.
5. Files declared in `**Files:**` match what the steps actually touch.

Background reference docs (e.g. `docs/rollshot_mvp_design.md`) are for context only — they describe the destination, not what to ship now. Read them once to ground yourself; do not review them as if they were the plan.

### Step 0: Scope Challenge

Before reviewing any task, answer these questions about the plan as a whole:

1. **Goal vs steps alignment:** Re-read the `**Goal:**` line. Do all tasks contribute to it? Flag any task that's nice-to-have but doesn't move the Goal forward — that's scope creep.
2. **What existing code already partially or fully solves each sub-problem?** Check `git ls-files` and existing crates. Can outputs from existing flows be reused rather than rebuilt?
3. **Minimum viable plan:** What is the smallest subset of tasks that achieves the Goal? List the tasks that could be deferred to a follow-up plan without blocking the core objective.
4. **Complexity check:** Count the files in the `## File Structure` section.
   - Net new files > 12, OR
   - Net new top-level modules/crates > 2, OR
   - Total tasks > 10
   → treat as a smell and challenge whether the same Goal can be reached with fewer moving parts (consider splitting into a follow-up plan).
5. **Search check:** For each architectural pattern, infrastructure component, or concurrency approach the plan introduces:
   - Does the runtime/framework/language have a built-in? Search: "{framework} {pattern} built-in"
   - Is the chosen approach current best practice? Search: "{pattern} best practice {current year}"
   - Are there known footguns? Search: "{framework} {pattern} pitfalls"

   If WebSearch is unavailable, skip this check and note: "Search unavailable — proceeding with in-distribution knowledge only."

   If the plan rolls a custom solution where a built-in exists, flag it as a scope reduction opportunity.
6. **Completeness check:** With AI-assisted execution, the cost of completeness (full test coverage, full edge case handling, complete error paths) is 10-100x cheaper than with a human team. If the plan proposes a shortcut that saves human-hours but only saves minutes with AI execution, recommend the complete version.
7. **Distribution check:** If the plan introduces a new artifact (CLI binary, library, container, mobile app), does at least one task cover the build/publish pipeline? Code without distribution is code nobody can use:
   - Is there a CI/CD workflow task for building/publishing?
   - Are target platforms defined (linux/darwin/windows, amd64/arm64)?
   - How will users install it (GitHub Releases, package manager)?

   If deferred, flag it explicitly in the "NOT in scope" section — don't let it silently drop.

If the complexity check triggers (>12 net-new files OR >2 new top-level modules/crates OR >10 tasks), in interactive mode STOP before any review-section work. Call AskUserQuestion: name what's overbuilt, propose a minimal task subset that achieves the Goal, ask whether to reduce or proceed as-is. The AskUserQuestion call is a tool_use, not prose — call the tool directly. In auto mode, record and adopt the recommended minimum viable scope, then continue the review against that decision without editing yet.

**Interactive mode STOP.** Do NOT proceed to Section 1 (Architecture review), edit the plan file with a proposed scope reduction, or call ExitPlanMode until the user responds. Naming the 80% solution in chat prose and continuing is the failure mode this gate exists to prevent.

If the complexity check does not trigger, present your Step 0 findings and proceed directly to Section 1.

Always work through the full review (Architecture → Code Quality → Tests → Performance) with at most 8 top issues per section. Interactive mode handles one section at a time; auto mode analyzes all sections before applying edits.

**Critical: Once the user accepts or rejects a scope reduction recommendation, commit fully.** Do not re-argue for smaller scope during later review sections. Do not silently reduce scope or skip planned components.

## Review Sections (after scope is agreed)

**Anti-skip rule:** Never condense, abbreviate, or skip any review section (1-4) regardless of plan type (strategy, spec, code, infra). Every section in this skill exists for a reason. "This is a strategy doc so implementation sections don't apply" is always wrong — implementation details are where strategy breaks down. If a section genuinely has zero findings, say "No issues found" and move on — but you must evaluate it.

### 1. Architecture review

Walk the plan's `## File Structure` and the code blocks inside each task. Evaluate:

* **Module/crate boundaries:** Does the split (e.g. `rollshot-core` vs `rollshot-capture`) match the responsibilities the Goal implies? Are there leaks (capture types referenced from core, etc.)?
* **Type and trait shapes shown in step code blocks:** Are public types `pub`, generic where they should be, and free of platform-specific assumptions in the core crate?
* **Dependency direction:** Read `[dependencies]` blocks in the task code. Are dependencies acyclic? Does any leaf crate accidentally depend on a higher-level one?
* **Failure modes for new integration points:** For each new codepath (capture backend, portal, IPC, file IO), state one realistic production failure (permission denied, format unsupported, partial read, cancelled session). Does the plan introduce a type or error variant that represents it, or is it implicit?
* **Distribution / CI:** Is there a task that wires CI (fmt, clippy, test) across all target platforms? If the plan introduces a new artifact, is there a task for build/publish?
* **Diagrams:** For any non-trivial data flow (frame stream lifecycle, portal handshake, stitcher state machine), would an ASCII diagram in a doc-comment make the design legible? Flag the file(s) that should get one.

In interactive mode, for each issue found, call AskUserQuestion individually. One issue per call. Present options, state your recommendation, explain WHY. Do NOT batch multiple issues into one AskUserQuestion. The AskUserQuestion call is a tool_use, not prose — call the tool directly. In auto mode, record one `Auto decision D<N>` per issue and continue.

**Interactive mode STOP.** Do NOT proceed to the next review section, edit the plan file with the proposed fix, or call ExitPlanMode until the user responds. An issue with an "obvious fix" is still an issue and still needs explicit user approval before it lands in the plan.

### 2. Plan structure & code quality review

This section reviews the **shape of the plan** as much as the code it specifies. Evaluate:

* **Task granularity:** Is each `## Task N` independently shippable as a commit (or small commit chain)? A task that touches 10 files across 4 crates is probably two tasks.
* **Step granularity:** Inside each task, is every `- [ ] Step N` small enough to verify in isolation? Each step should have a clear Run / Expected pair. Flag steps that are vague ("implement the whole thing") or that bundle multiple unrelated changes.
* **File list accuracy:** Does each task's `**Files:**` list match the files its steps actually create/modify? Flag mismatches.
* **TDD discipline:** For tasks adding new behavior, does the plan write the failing test FIRST, run it to confirm RED, then implement, then run again to confirm GREEN? Flag tasks where implementation precedes the test, or where there's no test at all for a new public API.
* **Commit boundaries:** Does each task end with a commit step? Are the commit messages descriptive and atomic (one logical change per commit)? Flag missing or vague commit steps.
* **Code shown in steps:** DRY violations across step code blocks, error handling patterns, missing edge cases (especially `unwrap()` on user-influenced input, panicky `expect(...)` in non-test code, swallowed `Result`s).
* **Over/under-engineering:** Generic traits with one impl, premature abstraction, or — conversely — copy-pasted blocks that should share a helper.

In interactive mode, call AskUserQuestion individually for each issue. In auto mode, record one `Auto decision D<N>` per issue and continue.

**Interactive mode STOP** after each AskUserQuestion. Wait for response before proceeding.

### 3. Test review

Produce a **test coverage table** by walking each task's steps. Mark every new behavior:

```
Task / behavior                                    Unit  Integ  E2E / smoke  Manual only
─────────────────────────────────────────────────  ────  ─────  ───────────  ───────────
Task 2 / StitchConfig::default values              ✓     —      —            no
Task 3 / FakeFrameStream order + end-of-stream     ✓     —      —            no
Task 4 / rollshot probe stdout shape               ✓     ✓      —            no
...
```

Then evaluate:

* **Critical-path tests:** Is every new public function exercised by at least one test in the same task?
* **TDD discipline (cross-check from §2):** For each test step, is there a matching "Run test → Expected: FAIL" step BEFORE the implementation step?
* **Determinism:** Do any tests depend on real time, network, OS GUI state, file ordering, or environment variables without a fake/mock layer?
* **Fixtures / golden tests:** For algorithmic correctness (stitching, matching, duplicate detection), are golden fixtures planned? Where do they live, and how are they generated?
* **Speed:** Will the `cargo test --workspace` step in the final verification task run in under ~30 seconds locally? Flag any test that needs real screen capture, real network, or sleeping.
* **Platform isolation:** For platform-specific code (Wayland, macOS), is the cross-platform CI path covered by fake/synthetic tests so hosted CI can run without the real platform?
* **Negative tests:** Are error paths (`Err(...)`, `None`, cancelled session, unsupported format) covered, or only the happy path?

In interactive mode, call AskUserQuestion individually for each gap. In auto mode, record one `Auto decision D<N>` per gap and continue.

**Interactive mode STOP** after each AskUserQuestion.

### 4. Performance & resource review

For systems-level / pipeline / streaming code (which rollshot is — frame streams, pixel buffer conversion, stitching), evaluate:

* **Hot loops & allocations:** Per-frame allocations in the capture or stitch path? Each frame may be megabytes — a `Vec::clone()` per frame adds up fast. Flag any unnecessary `.clone()` or `.to_vec()` in hot paths shown in the plan.
* **Pixel format conversion:** Is stride handled (row pitch may exceed `width * bpp`)? Is the conversion vectorizable or at least loop-friendly?
* **I/O patterns:** Sync vs async, batching, backpressure. For a frame stream, is there a bound on how many frames buffer up if the stitcher falls behind?
* **Algorithmic complexity:** For template matching, what's the search space per frame? Is `match_width` tuned to keep the inner loop cache-friendly?
* **Resource lifecycle:** Are file descriptors, DBus connections, and PipeWire streams explicitly closed/dropped? Flag any `Box<dyn ...>` that owns a system resource without a `Drop` impl shown.
* **Memory ceilings:** What's the worst-case memory for the final stitched image? Is there a hard cap or a paging strategy if the user scrolls forever?

In interactive mode, call AskUserQuestion individually for each issue. In auto mode, record one `Auto decision D<N>` per issue and continue.

**Interactive mode STOP** after each AskUserQuestion.

## CRITICAL RULE — How to ask questions

Every interactive AskUserQuestion and auto decision is a decision brief. Format:

```
D<N> — <one-line question title>
Context: <1 short grounding sentence>
ELI10: <plain English a 16-year-old could follow, 2-4 sentences, name the stakes>
Stakes if we pick wrong: <one sentence on what breaks, what user sees, what's lost>
Recommendation: <choice> because <one-line reason>
Completeness: A=X/10, B=Y/10  (or: Note: options differ in kind, not coverage — no completeness score)
Pros / cons:
A) <option label> (recommended)
  ✅ <pro — concrete, observable>
  ❌ <con — honest>
B) <option label>
  ✅ <pro>
  ❌ <con>
Net: <one-line synthesis of what you're actually trading off>
```

Rules:

* **One issue = one decision brief.** In interactive mode, use one AskUserQuestion call. In auto mode, label it `Auto decision D<N>` and record the selected recommendation.
* Describe the problem concretely, with file and line references when applicable.
* Present 2-3 options, including "do nothing" where that's reasonable.
* For each option, specify in one line: effort, risk, and maintenance burden. Where effort is involved, label both human-team and AI-assisted time, e.g. `(human: ~2 days / AI: ~30 min)`.
* **Map the reasoning to engineering preferences above.** One sentence connecting your recommendation to a specific preference (DRY, explicit > clever, minimal diff, etc.).
* Label with issue NUMBER + option LETTER (e.g., "3A", "3B").
* **Coverage vs kind:** for every per-issue AskUserQuestion you raise, decide whether the options differ in coverage or in kind. If coverage (more tests vs fewer, complete error handling vs happy-path-only), include `Completeness: N/10` on each option (10 = complete, 7 = happy path, 3 = shortcut). If kind (architectural choice between two different systems), skip the score and add: `Note: options differ in kind, not coverage — no completeness score.` Do NOT fabricate scores on kind-differentiated questions.
* **Zero findings:** if a section has zero findings, state "No issues, moving on" and proceed.
* **Non-ASCII characters — write directly, never \u-escape.** CJK / accented strings go in literal UTF-8. Claude Code's tool parameter pipe is UTF-8 native.

### Decision-brief self-check

Before calling AskUserQuestion, verify:

- [ ] D<N> header present
- [ ] ELI10 paragraph present (stakes line too)
- [ ] Recommendation line present with concrete reason
- [ ] Completeness scored (coverage) OR kind-note present (kind)
- [ ] Every option has ≥1 ✅ and ≥1 ❌ (or hard-stop escape: `✅ No cons — this is a hard-stop choice`)
- [ ] (recommended) label on one option
- [ ] Net line closes the decision
- [ ] Interactive mode: you are calling the tool, not writing prose; auto mode: you are recording an `Auto decision D<N>`
- [ ] Non-ASCII characters written directly, NOT \u-escaped

## Required outputs

### "NOT in scope" section

Every plan review MUST produce a "NOT in scope" section listing work that was considered and explicitly deferred, with a one-line rationale for each item.

### "What already exists" section

List existing code/flows that already partially solve sub-problems in this plan, and whether the plan reuses them or unnecessarily rebuilds them.

### Failure modes

For each new codepath identified in the test coverage table, list one realistic way it could fail in production (timeout, panic, race condition, stale data, permission denied, format unsupported, cancelled portal session, EOF on stream, etc.) and whether:

1. A test covers that failure (cite Task N / Step M)
2. Error handling exists for it (cite the `Result<_, _>` variant or `match` arm in the plan)
3. The user would see a clear error or a silent failure

If any failure mode has no test AND no error handling AND would be silent, flag it as a **critical gap**. Reference the specific Task ID where the gap appears so it's easy to fix in place.

### Worktree / subagent parallelization strategy

This plan will be executed by `superpowers:subagent-driven-development` or `superpowers:executing-plans`. The former dispatches independent tasks to parallel subagents — so identify which `## Task N`s are parallel-safe.

**Skip if:** all tasks touch the same primary module, or the plan has fewer than 2 independent workstreams. In that case, write: "Sequential execution, no parallelization opportunity."

**Otherwise, produce:**

1. **Task dependency table:**

| Task | Modules touched | Depends on |
|------|----------------|------------|
| Task N: <name> | `crates/foo-core/`, `crates/foo-capture/linux/` | Task M, or — |

Work at the module/directory level, not file level. `crates/rollshot-core/` is reliable; `crates/rollshot-core/src/stitcher.rs` is guesswork.

2. **Parallel lanes** — group tasks into lanes:
   - Tasks with no shared modules and no dependency → separate lanes (parallel)
   - Tasks sharing a module directory → same lane (sequential)
   - Tasks depending on earlier tasks → later lanes

   Format: `Lane A: Task 2 → Task 3 (sequential, both touch crates/rollshot-core/)` / `Lane B: Task 6 (independent)`

3. **Execution order:** which lanes launch in parallel, which wait. Example: "Launch A + B in parallel worktrees. Merge both. Then C."

4. **Conflict flags:** if two parallel lanes touch the same module directory, flag it: "Lanes X and Y both touch `crates/rollshot-core/` — potential merge conflict. Consider sequential execution or careful coordination."

5. **Workspace-root tasks:** Tasks that modify the root `Cargo.toml` (e.g. adding a new crate to `members`) serialize everything — flag them so they're not assigned in parallel.

### Completion summary

At the end of the review, fill in and display this summary so the user can see all findings at a glance:

```
Plan reviewed:           docs/superpowers/plans/<file>.md
Tasks in plan:           N
Files Create/Modify:     N create / N modify

- Step 0: Scope Challenge   — (accepted as-is / scope reduced per recommendation)
- Architecture Review:        N issues
- Plan Structure + Code Q:    N issues  (granularity / TDD / commits / DRY)
- Test Review:                table produced, N gaps
- Performance Review:         N issues
- NOT in scope:               written
- What already exists:        written
- Failure modes:              N critical gaps flagged
- Parallelization:            N lanes, N parallel / N sequential
- Unresolved decisions:       N (listed below)
```

Then state the next step: "Plan is locked in — run `superpowers:executing-plans` (or `subagent-driven-development` if parallel lanes were identified)" — OR — "Plan needs revision; the unresolved decisions above must be answered before execution."

In auto mode, add:

```
Auto decisions applied:
- D<N>: <selected option> — <one-line reason>

Plan edits:
- <concise description of each material change>
```

## Formatting rules

* NUMBER issues (1, 2, 3...) and LETTERS for options (A, B, C...).
* Label with NUMBER + LETTER (e.g., "3A", "3B").
* One sentence max per option. Pick in under 5 seconds.
* In interactive mode, pause and ask for feedback after each review section. In auto mode, continue through all sections without pausing.

## Unresolved decisions

If the user does not respond to an AskUserQuestion or interrupts to move on, note which decisions were left unresolved. At the end of the review, list these as "Unresolved decisions that may bite you later" — never silently default to an option.

## Plan mode behavior

If invoked in plan mode, treat this skill as executable instructions, not reference. Follow it step by step starting from Step 0; the first AskUserQuestion is the workflow entering plan mode, not a violation of it. At a STOP point, stop immediately. Do not call ExitPlanMode until the workflow completes, or until the user tells you to cancel the skill or leave plan mode.
