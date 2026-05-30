## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

## 5. Branching

- **superpowers spec commit:** when a spec has been reviewed and approved by the user and you are about to commit that spec, branch-first-if-on-`main`: if currently on `main` (the default branch), create a new branch (`git checkout -b <name>`) FIRST, then commit the spec — never commit it directly on `main`. If already on a non-`main` branch, do NOT create a new branch; commit on the current branch. (This is the one moment to enforce; other commits follow the normal harness rules.)
- New branches: use `git checkout -b <name>` in place.
- Never set up git worktrees unless explicitly asked.

## 6. Shell Commands

- Always prefix shell commands with `rtk`, unless invoking MCP tools or another non-shell tool.

## 7. Verification

For Rust changes, prefer:
- `rtk cargo test`
- `rtk cargo fmt --check`
- `rtk cargo clippy --workspace --all-targets -- -D warnings` when risk justifies it

For frontend changes under `crates/rollshot-app`, prefer:
- `rtk pnpm --dir crates/rollshot-app run typecheck`
- `rtk pnpm --dir crates/rollshot-app test`
- `rtk pnpm --dir crates/rollshot-app run build`

When already in `crates/rollshot-app/` (check with `pwd`):
- `rtk pnpm run typecheck`
- `rtk pnpm test`
- `rtk pnpm run build`

### Performance verification

For changes touching `rollshot-core` stitching paths (matcher, canvas,
verifier, stitcher), also capture before/after numbers:

- `rtk cargo bench -p rollshot-core --bench stitch_sequences -- --out bench-results/runs/<scope>/after.jsonl`
- `rtk python3 scripts/bench/compare.py bench-results/runs/<scope>/before.jsonl bench-results/runs/<scope>/after.jsonl`

See `docs/bench.md` for the full workflow and metric reference.

@RTK.md

## 8. Project Map

Use this as orientation only; verify current symbols, command flags, and
behavior against code before relying on them.

- `crates/rollshot-core`: platform-independent stitching. Matcher, canvas,
  overlap, verifier, metrics, and `Stitcher` live here. Changes on matcher,
  canvas, verifier, or stitcher paths usually need core tests and the benchmark
  checks listed above.
- `crates/rollshot-capture`: capture traits, frame metadata, fixture capture,
  Linux portal/PipeWire capture, and feature-gated macOS ScreenCaptureKit via
  `scap`.
- `crates/rollshot-cli`: command-line entry points. `src/args.rs` is the source
  of truth for subcommands and flags; `cmd_*` modules hold behavior.
- `crates/rollshot-app`: Tauri v2 interactive capture app. Frontend code lives
  under `crates/rollshot-app/src`; Rust/Tauri commands live under
  `crates/rollshot-app/src-tauri/src`.
- `scripts/bench`: benchmark JSONL summarization and before/after comparison.
- `README.md`: user-facing setup and manual testing notes. Treat command
  examples as documentation to verify against code, not as implementation
  source of truth.

## 9. learn-projects

The `learn-projects/` directory contains cloned reference repositories for
learning and cross-referencing. They are **not** part of rollshot's build
and are excluded from search tools by `.ignore` and `.rgignore`.

**Searching**: use `--no-ignore` (ripgrep) or similar flags to include
learn-projects results when needed.

| Project | Remote | Relationship to rollshot |
|---------|--------|--------------------------|
| `obs-studio` | obsproject/obs-studio | Reference for streaming and capture layer (OBS capture architecture, PipeWire, ScreenCaptureKit patterns). Directly relevant to `rollshot-capture`. |
| `rust-cv` | rust-cv/cv | Computer Vision library in Rust. Reference for image stitching, feature detection, and geometric transforms used in `rollshot-core`. |
| `scap` | zed-industries/scap | Screen capture library by Zed Industries. `rollshot-capture` is built as a scap-compatible crate; the macOS backend uses scap. Directly relevant to `rollshot-capture`. |
| `snow-shot` | mg-chao/snow-shot | Same category: screenshot/long-screenshot software. Reference for screenshot workflows, UI patterns. |
| `tauri-template` | dannysmith/tauri-template | Tauri v2 app template. Reference for Tauri app structure and patterns used in `rollshot-app`. |
| `wayscrollshot` | jswysnemc/wayscrollshot | Same category: Wayland scrolling screenshot tool. Reference for screenshot/capture workflows, especially Linux/Wayland portal integration. |

## 10. docs/ — Snapshots, Not Source of Truth

**Code is the source of truth. `docs/` contains snapshots, not current spec.**

Files in `docs/` fall into two categories, both of which can drift from code:

- **Historical (frozen)**: `docs/superpowers/plans/` and `docs/superpowers/specs/`
  are snapshots produced by the superpowers workflow. They capture design
  intent at the time of writing and are **not** updated after implementation
  or subsequent iteration. Treat as archive.
- **Forward-looking (research)**: other docs (e.g. `docs/stitching-*.md`,
  `docs/rollshot_mvp_design.md`) may describe analysis or ideas for upcoming
  work — they reflect intent, not necessarily what's in code today.

Rules:

- When `docs/` conflicts with code, **code wins**. Always.
- Do not assume any filename, function, module, flag, or behavior named in
  `docs/` still exists (or exists yet) — verify against code (use
  code-review-graph MCP tools below) before relying on it.
- Do not retroactively edit historical plans/specs in `docs/superpowers/` to
  "fix" drift; that defeats the purpose of a snapshot. New iteration → new
  plan/spec, or update the code directly.
- Do not delete or move files in `docs/` unless explicitly asked.

**Exception — active superpowers workflows.** A plan/spec is **live** (not
historical) while a superpowers skill is actively driving work against it:

- `superpowers:writing-plans` — the plan being authored is live.
- `superpowers:executing-plans` / `subagent-driven-development` — the plan
  being executed is the spec; the executor's job is to make code match plan.
- `superpowers:verification-before-completion` /
  `finishing-a-development-branch` — the plan defines success criteria.
- **User explicitly directs work against a specific plan/spec** (e.g.
  "implement `docs/superpowers/plans/2026-05-23-foo.md`", "verify against
  `docs/superpowers/specs/bar.md`") — that file is live for the scope of the
  request, even without a formal skill invocation.

In these contexts the "code wins on conflict" rule above does **not** apply
to the plan being worked on — that plan is the source of truth for the
duration of the workflow. The plan becomes a frozen snapshot only after the
workflow completes and the branch lands.

## 11. Spec/Plan Process — Default Flow and Lightweight Escape

The default for creative/implementation work is the full superpowers flow:
brainstorm → spec → approval → writing-plans → execute. The native superpowers
skills require this even for tasks that look simple (their HARD-GATE and the
"This Is Too Simple To Need A Design" anti-pattern), allowing only a *short*
design — never skipping it.

**Lightweight escape (user-initiated).** This instruction overrides that gate
(superpowers' own Instruction Priority puts user instructions above skills).
When the user explicitly says to skip the spec/plan and implement directly
(e.g. "just do it", "skip the spec", "no plan needed", "直接做"), go straight to
implementation — no design doc, no plan file.

- You may NOT decide on your own that a task is "too simple" to spec — the skip
  is the user's call.
- When you judge a task trivial (e.g. a single-file change with no design
  choices), you MAY proactively *suggest* skipping the spec/plan and wait for
  the user's go-ahead — but default to the normal flow until they agree.
- Skipping the spec/plan does NOT skip engineering discipline: still apply §1
  (surface assumptions, ask when unclear), §4 (goal-driven / TDD), §5
  (branching), and §7 (verification).

<!-- code-review-graph MCP tools -->
## MCP Tools: code-review-graph

**IMPORTANT: This project has a knowledge graph. ALWAYS use the
code-review-graph MCP tools BEFORE using Grep/Glob/Read to explore
the codebase.** The graph is faster, cheaper (fewer tokens), and gives
you structural context (callers, dependents, test coverage) that file
scanning cannot.

### When to use graph tools FIRST

- **Exploring code**: `semantic_search_nodes` or `query_graph` instead of Grep
- **Understanding impact**: `get_impact_radius` instead of manually tracing imports
- **Code review**: `detect_changes` + `get_review_context` instead of reading entire files
- **Finding relationships**: `query_graph` with callers_of/callees_of/imports_of/tests_for
- **Architecture questions**: `get_architecture_overview` + `list_communities`

Fall back to Grep/Glob/Read **only** when the graph doesn't cover what you need.

### Key Tools

| Tool | Use when |
| ------ | ---------- |
| `detect_changes` | Reviewing code changes — gives risk-scored analysis |
| `get_review_context` | Need source snippets for review — token-efficient |
| `get_impact_radius` | Understanding blast radius of a change |
| `get_affected_flows` | Finding which execution paths are impacted |
| `query_graph` | Tracing callers, callees, imports, tests, dependencies |
| `semantic_search_nodes` | Finding functions/classes by name or keyword |
| `get_architecture_overview` | Understanding high-level codebase structure |
| `refactor_tool` | Planning renames, finding dead code |

### Workflow

1. The graph auto-updates on file changes (via hooks).
2. Use `detect_changes` for code review.
3. Use `get_affected_flows` to understand impact.
4. Use `query_graph` pattern="tests_for" to check coverage.
