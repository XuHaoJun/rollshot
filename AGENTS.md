# Rollshot

Rollshot is a screenshot and scrolling-capture project with a Rust stitching core, capture backends, a CLI, and an iced desktop app. Use this file for agent workflow rules and repo orientation; use `README.md` for user-facing setup, and verify implementation details against code.

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

- **Branch naming:** use conventional prefixes — `feat/<description>` (new feature), `fix/<description>` (bug fix), `hotfix/<description>` (urgent fix), `perf/<description>` (performance optimization), or `docs/<description>` (pure investigation/research with no code changes). Never use `spec/` or `specs/` — specs are committed to the feature branch, not a separate branch. The branch name should describe the nature of the work (e.g. `feat/scroll-capture`, `perf/matcher-overlap`, `docs/wayland-portal-research`).
- **superpowers spec commit:** when a spec has been reviewed and approved by the user and you are about to commit that spec, branch-first-if-on-`main`: if currently on `main` (the default branch), create a new branch (`git checkout -b <prefix>/<name>`) FIRST, then commit the spec — never commit it directly on `main`. If already on a non-`main` branch, do NOT create a new branch; commit on the current branch. (This is the one moment to enforce; other commits follow the normal harness rules.)
- New branches: use `git checkout -b <prefix>/<name>` in place.
- Never set up git worktrees unless explicitly asked.

## 6. Shell Commands

- Always prefix shell commands with `rtk`, unless invoking MCP tools or another non-shell tool.

## 7. Rust Diagnostics

- Use `tracing` for all runtime diagnostics in active product paths, including
  temporary debugging instrumentation.
- Every tracing event must use a stable explicit `rollshot::*` target and
  structured fields. Use `trace` for high-volume or per-frame details.
- Do not use `println!`, `eprintln!`, or `dbg!` as temporary diagnostics.
- `eprintln!` is allowed only for intentional user-facing stderr output,
  test/benchmark/spike UX, or failures before the tracing subscriber can be
  initialized.
- Before committing temporary instrumentation, either remove it or confirm it
  is privacy-safe, appropriately leveled, and useful as retained diagnostics.

## 8. Verification

For Rust changes, prefer:
- `rtk cargo test`
- `rtk cargo fmt --check`
- `rtk cargo clippy --workspace --all-targets -- -D warnings` when risk justifies it

### Performance verification

For changes touching `rollshot-core` stitching paths (matcher, canvas,
verifier, stitcher), also capture before/after numbers:

- `rtk cargo bench -p rollshot-core --bench stitch_sequences -- --out bench-results/runs/<scope>/after.jsonl`
- `rtk python3 scripts/bench/compare.py bench-results/runs/<scope>/before.jsonl bench-results/runs/<scope>/after.jsonl`

See `docs/bench.md` for the full workflow and metric reference.

### Platform-split capture UI changes

Linux and macOS capture UI paths are intentionally different:

- Linux uses the native iced Wayland layer-shell overlay
  (`rollshot-iced-overlay`).
- macOS uses the iced overlay path through `rollshot-app` /
  `rollshot-iced-overlay` with ScreenCaptureKit (`macos-sck`) as the
  platform-default capture backend.

For any change touching capture UI/UX, check both platform paths before editing.
This includes overlay behavior, crop selection, coordinate mapping, input
passthrough, focus/window visibility, scroll/Esc controls, stitching live
preview, final preview, save handoff, capture launch options, backend/region
semantics, and shared overlay visuals.

Prefer shared code when behavior must match both paths:

- `crates/rollshot-overlay-core` for preview viewport logic and crop visual
  tokens.
- `crates/rollshot-iced-overlay` for the active Linux and macOS iced overlay
  runners and UI.
- `crates/rollshot-app/src/main.rs` for the active iced product app launch and
  save handoff.
If a change intentionally applies to only one platform, state that explicitly in
the plan and final response, including the reason, the unchecked counterpart
path, and any remaining runtime-verification risk.

@RTK.md

## 9. Project Map

Use this as orientation only; verify current symbols, command flags, and
behavior against code before relying on them.

- `crates/rollshot-core`: platform-independent stitching. Matcher, canvas,
  overlap, verifier, metrics, and `Stitcher` live here. Changes on matcher,
  canvas, verifier, or stitcher paths usually need core tests and the benchmark
  checks listed above.
- `crates/rollshot-capture`: capture traits, frame metadata, fixture capture,
  Linux portal/PipeWire and KWin-native capture (`linux-portal`, `linux-kwin`),
  and the macOS ScreenCaptureKit backend (`macos-sck`) — `scap` for streaming,
  `rollshot-macos-oneshot` for one-shot screenshots. `KNOWN_BACKEND_NAMES` in
  `src/backend.rs` is the source of truth for backend flags.
- `crates/rollshot-macos-oneshot`: unsafe-isolation crate for macOS
  ScreenCaptureKit one-shot capture (Objective-C FFI via `objc2`). Public API
  is safe; the rest of the workspace keeps `unsafe_code = "forbid"`. Used by
  `rollshot-capture`.
- `crates/rollshot-linux-desktop`: small Linux desktop integration helper
  shared by the daemon and Action Guide SNI paths (StatusNotifierItem host
  detection).
- `crates/rollshot-cli`: command-line entry points. `src/args.rs` is the source
  of truth for subcommands and flags; `cmd_*` modules hold behavior.
- `crates/rollshot-app`: Rust-only iced product app. It owns launch parsing,
  overlay selection, iced capture delegation, the macOS product flow, and the
  post-capture result workspace (annotation editing, storage, save handoff).
  The daemon now has a macOS adapter (`daemon/macos*`, winit + `tray-icon` +
  `global-hotkey`) alongside the Linux KDE adapter, both driving the shared
  `daemon/core.rs`.
- `crates/rollshot-image-document`: headless, framework-neutral,
  non-destructive image document and editing engine — annotation graph,
  history, geometry, hit-testing, and flattened rendering. No UI, windowing,
  clipboard, or capture code. Used by `rollshot-app` for annotations,
  redactions, and callouts.
- `crates/rollshot-iced-overlay`: iced overlay renderer. Both platform runners
  are active product paths: Linux uses the layer-shell runner
  (`linux_runner.rs`), macOS uses the capture-wired runner
  (`macos_capture.rs`).
- `crates/rollshot-overlay-core`: framework-neutral overlay logic shared by active
  overlay components, including preview viewport logic, capture-miss state, and
  crop visual tokens.
- **Action Guide crates** (built behind the non-default `action-guide` Cargo
  feature on `rollshot-cli` / `rollshot-app`):
  - `crates/rollshot-action`: platform-neutral Action Guide engine — frame
    ingestion, deterministic step detection, the editable guide model, and
    export. Owns no windows, permissions, native event APIs, or capture
    backend; driven by pushed frames plus privacy-filtered semantic events.
  - `crates/rollshot-linux-input` / `crates/rollshot-macos-input`: listen-only
    semantic-input sources (Linux evdev, macOS `CGEventTap`). Emit only
    privacy-filtered semantic actions — no raw key persistence or input
    injection.
- **iced UI work** (`rollshot-iced-overlay`, `rollshot-app`,
  `rollshot-overlay-core`): the workspace pins iced `0.14` (canvas, image,
  tokio). For building, modifying, or debugging any of these UIs — custom
  widgets, overlays, Canvas drawing, theming, subscriptions — invoke the
  `iced-rs` skill first; it carries the full 0.14 API reference and 0.14-correct
  upstream examples (0.13 signatures differ). The skill does **not** cover
  `iced_layershell` 0.18 (the Linux layer-shell layer) — for that, cross-ref
  `learn-projects/exwlshelleventloop` (§10).
- `scripts/bench`: benchmark JSONL summarization and before/after comparison.
- `README.md`: user-facing setup and manual testing notes. Treat command
  examples as documentation to verify against code, not as implementation
  source of truth.

## 10. learn-projects

The `learn-projects/` directory contains cloned reference repositories for
learning and cross-referencing. They are **not** part of rollshot's build
and are excluded from search tools by `.ignore` and `.rgignore`.

**Searching**: use `--no-ignore` (ripgrep) or similar flags to include
learn-projects results when needed.

| Project | Remote | Relationship to rollshot |
|---------|--------|--------------------------|
| `CrossMacro` | alper-han/CrossMacro | Cross-platform desktop automation app for recording/editing/replaying input macros. Reference for Action Guide input recording and step-editing workflows (`rollshot-action`, `rollshot-{linux,macos}-input`). |
| `exwlshelleventloop` | waycrate/exwlshelleventloop | Upstream repo of the `iced_layershell` crate, a direct dependency of `rollshot-iced-overlay`'s Linux layer-shell runner. |
| `flameshot` | flameshot-org/flameshot | Same category: screenshot tool with in-place annotation editor (C++/Qt). Reference for annotation/editing UX relevant to `rollshot-image-document` and `rollshot-app`. |
| `mark-shot` | jswysnemc/mark-shot | Same category: screenshot annotation/markup tool (Qt), by the wayscrollshot author. Reference for annotation workflows. |
| `obs-studio` | obsproject/obs-studio | Reference for streaming and capture layer (OBS capture architecture, PipeWire, ScreenCaptureKit patterns). Directly relevant to `rollshot-capture`. |
| `rust-cv` | rust-cv/cv | Computer Vision library in Rust. Reference for image stitching, feature detection, and geometric transforms used in `rollshot-core`. |
| `scap` | zed-industries/scap | Screen capture library by Zed Industries. `rollshot-capture` is built as a scap-compatible crate; the macOS backend uses scap. Directly relevant to `rollshot-capture`. |
| `snow-shot` | mg-chao/snow-shot | Same category: screenshot/long-screenshot software. Reference for screenshot workflows, UI patterns. |
| `spectacle` | KDE/spectacle | KDE screenshot utility. Reference for capture workflows and Linux/Wayland portal integration. |
| `wayscrollshot` | jswysnemc/wayscrollshot | Same category: Wayland scrolling screenshot tool. Reference for screenshot/capture workflows, especially Linux/Wayland portal integration. |

## 11. docs/ — Snapshots, Not Source of Truth

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

## 12. Spec/Plan Process — Default Flow and Lightweight Escape

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
  (surface assumptions, ask when unclear), §4 (goal-driven / TDD), and §8
  (verification).

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
