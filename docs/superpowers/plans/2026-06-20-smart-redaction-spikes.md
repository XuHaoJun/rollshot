# Smart Redaction Agent Workbench — Technical Spikes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Every spike task ALSO follows the project `rollshot-run-spike` skill: isolated crate, `FINDINGS.md`, highest-risk gate first, evidence levels, stop on failed hard gate.

**Goal:** Run the four required technical spikes from the Smart Redaction Agent Workbench design (`docs/superpowers/specs/2026-06-20-smart-redaction-agent-workbench-design.md` §13) and produce written GO/NO-GO decisions — sandbox runtime, JavaScript parser, Rig integration, visual diff — plus the cross-cutting workspace MSRV resolution, so subsequent product subprojects can lock their dependency stack.

**Architecture:** Each spike is a **throwaway, isolated** Rust crate under `spikes/<topic>/` (standalone, empty `[workspace]` table, NOT a root-workspace member). Each produces a `FINDINGS.md` with empirical evidence and a recommendation; it does not produce production code. A temporary `workflow_dispatch` CI workflow bridges macOS verification (the dev host is headless Linux; macOS build/test runs only in CI, triggered manually by the user). A final consolidation task records the joint decisions and the MSRV resolution.

**Tech Stack:** Rust, `rquickjs` 0.12.x (sandbox candidate), JS-parser candidates (`oxc`, `swc`, `tree-sitter`, `boa`), `rig` 0.39.x (agent/provider candidate), `iced` 0.14 (visual diff prototype), `criterion` (headless benchmarks), GitHub Actions (`ubuntu-24.04` + `macos-14`).

## Global Constraints

These apply to **every** task. Values copied verbatim from the design spec, the execution environment, and facts verified during plan authoring.

- **Spike isolation (rollshot-run-spike skill):** Every spike lives in `spikes/<topic>/` as a standalone crate whose `Cargo.toml` contains an empty `[workspace]` table. NEVER add a spike crate to the root `Cargo.toml` `members`. Production crates stay unchanged; any temporary production edit must be recorded in `FINDINGS.md` and reverted before commit.
- **Spike output is evidence, not code:** primary deliverable is `spikes/<topic>/FINDINGS.md` (from `.claude/skills/rollshot-run-spike/references/findings-template.md`). Record exact environment + command, evidence level (`compile` / `automated` / `runtime` / `hardware`), and result (`PASS` / `FAIL` / `MITIGATED` / `UNTESTED`) per milestone. **Never promote compile success into a runtime claim.**
- **Highest-risk gate first.** Stop at a failed **hard** gate and record the fallback instead of building on an invalid assumption.
- **Platform / CI policy (this environment — VERIFIED 2026-06-20):** The dev host is a **headless Ubuntu remote server**; `gh` CLI is **not installed** (`curl` is). All Linux build/test runs locally here. **macOS and any real-display GUI verification run only in CI.** Mechanism: an open PR (**#60**, `feat/smart-redaction-agent-workbench` → `main`) exists, so `spike-ci.yml` triggers on `pull_request` (paths-filtered to `spikes/**`). **`workflow_dispatch` alone does NOT work on a feature branch** — GitHub only exposes dispatchable workflows from the default branch — which is why the `pull_request` trigger is required. Flow per the user's instruction: the **controller** (not the implementer subagent) pushes at each macOS gate, then asks the user to paste the macOS job results back (the user approves/triggers the run on their side — "權限問題, 人工處理"). Implementer subagents do only the **Linux-local** steps and STOP before any "notify the user / wait for CI / wait for API key / wait for display" step. Mark platform/hardware checks that cannot be obtained as `UNTESTED`.
- **MSRV reality (VERIFIED 2026-06-20 — supersedes the spec's 1.85 assumption):** The workspace `Cargo.toml:23` declares `rust-version = "1.85"`, **but this is already stale.** The resolved `Cargo.lock` pins `iced 0.14.0` (declares `rust-version = "1.88"`, `edition = "2024"`) and `image 0.25.10` (declares `rust-version = "1.88.0"`). So the workspace's **true current floor is ≥1.88** and it does **not** build on 1.85 today. CI hides this by running unpinned `dtolnay/rust-toolchain@stable`. Consequences for this plan:
  - `rquickjs` 0.12.x (declares `rust-version` 1.87, to be confirmed by the probe) is **below** the existing 1.88 floor → adopting it adds **zero** MSRV cost. The spec's "1.85 vs 1.87" worry is moot.
  - The real MSRV questions are: (a) confirm the **true** current floor (baseline experiment); (b) confirm every chosen dependency (rquickjs, the picked parser, rig) builds at that floor; (c) flag any candidate whose latest version pushes the floor **above 1.88** (e.g. latest `oxc` requires ~1.94) as a real cost to weigh; (d) recommend **correcting the stale declared `rust-version`** to the true max of pinned deps.
  - This cross-cut is the **joint** output of Task 2 (sandbox) + Task 3 (parser) + Task 4 (rig). No executor/automation-frontend implementation may start before it is resolved.
- **Parser/runtime not pre-locked (spec §13):** Implementation planning must not lock parser/runtime details before these spikes. `rquickjs` and `rig` are candidates, not commitments. `rig` itself (`edition = "2024"`, version 0.39.0; dev `rust-toolchain.toml` pins 1.91 — that is its CI toolchain, not necessarily its consumer `rust-version`) must have its real consumer MSRV measured in Task 4.
- **Diagnostics (AGENTS.md §7):** runtime instrumentation in spike code uses `tracing` with stable `rollshot::*` targets; no `println!`/`dbg!`. `eprintln!` allowed only for spike UX before a subscriber exists.
- **Shell prefix (AGENTS.md §6):** prefix all shell commands with `rtk`.
- **Lifecycle:** on close, set each `FINDINGS.md` lifecycle to `retained-reference`. Do not delete spikes unless the user asks.

---

## File Structure

Created by this plan (all throwaway except the consolidation doc):

- `spikes/README.md` — explains the spikes directory is isolated, retained-reference, not part of the build.
- `.gitignore` (modify) — ignore `spikes/*/target/`. Spike `Cargo.lock` files ARE committed as frozen, decision-time evidence and must **not** be re-resolved after the spike flips to `retained-reference`.
- `.github/workflows/spike-ci.yml` — **temporary** `workflow_dispatch` workflow that builds/tests every `spikes/*/` crate on `ubuntu-24.04` + `macos-14` (stable), plus a floor-check job on Rust 1.88 (the workspace's real floor). Removed in Task 6.
- `spikes/sandbox-executor/` — Task 2 (13.2, highest risk).
- `spikes/js-frontend/` — Task 3 (13.1).
- `spikes/rig-agent/` — Task 4 (13.3).
- `spikes/visual-diff/` — Task 5 (13.4).
- `spikes/<topic>/FINDINGS.md` — one per spike.
- `docs/superpowers/spikes/2026-06-20-spike-decisions.md` — Task 6 consolidation (the one retained, product-facing artifact).

**Not in this plan (parallel-eligible):** Delivery subproject 2 (typed `EditOperation`/`EditProposal` + `ImageDocument` batch-transaction / one-step undo) is spike-INDEPENDENT — `rollshot-image-document` already has the snapshot/commit undo machinery and the grouped-undo precedent in `delete_annotation`. It should be specced and built in parallel on this branch, separately. Do not implement it here.

---

## Task 1: Spike harness + macOS/MSRV CI bridge

Sets up the isolated `spikes/` convention and the manual-dispatch CI workflow every later task relies on for macOS results. No spike crate yet — this is pure scaffolding plus the CI bridge.

**Files:**
- Create: `spikes/README.md`
- Create: `.github/workflows/spike-ci.yml`
- Modify: `.gitignore` (append spike target ignore)

**Interfaces:**
- Consumes: nothing.
- Produces: the `spikes/*/` layout contract (standalone crates, empty `[workspace]`), and a manually-dispatchable CI workflow named **`Spike CI`** with two jobs — **`Spikes`** (stable, builds all targets + runs tests, both OSes) and **`Floor check (Rust 1.88)`** (builds all spike crates on the workspace's real floor, both OSes). Later tasks rely on these exact names.

- [ ] **Step 1: Create the spikes README**

Create `spikes/README.md`:

```markdown
# Spikes

Throwaway feasibility experiments for the Smart Redaction Agent Workbench
(see `docs/superpowers/plans/2026-06-20-smart-redaction-spikes.md`).

- Each subdirectory is a **standalone** Rust crate with an empty `[workspace]`
  table. None are members of the root workspace and none are built by `cargo`
  from the repo root.
- Each crate's primary output is its `FINDINGS.md`.
- After a decision is consumed these become `retained-reference`: historical
  evidence only. Do not import them from production code, keep them synced, or
  delete them without an explicit request. Committed `Cargo.lock` files are
  frozen decision-time evidence — do not re-resolve them later.
```

- [ ] **Step 2: Ignore spike build artifacts**

Append to `.gitignore`:

```gitignore

# Spike build artifacts (isolated crates under spikes/). Cargo.lock is kept as
# frozen decision-time evidence; only target/ is ignored.
spikes/*/target/
```

- [ ] **Step 3: Create the manual macOS/MSRV CI workflow**

Create `.github/workflows/spike-ci.yml`. It is `workflow_dispatch`-only (never auto-runs). The `Spikes` job builds all targets (so benches compile) but runs only tests (so criterion `harness = false` benches do NOT execute under `cargo test`). The `Floor check` job builds every spike crate on Rust 1.88 — the workspace's real floor — to surface any candidate that requires more. Linux deps mirror the main CI's iced requirements (`libxkbcommon`, wayland); `libclang` is intentionally NOT installed (rquickjs 0.12 default features ship pre-generated bindings and need only a C compiler, preinstalled on both runners).

```yaml
name: Spike CI

on:
  # pull_request runs on the open PR (#60) when spike files change, so macOS
  # results are available on the feature branch. workflow_dispatch alone only
  # works once the workflow is on the default branch. Push to branch -> CI runs.
  pull_request:
    paths:
      - 'spikes/**'
      - '.github/workflows/spike-ci.yml'
  workflow_dispatch:

jobs:
  spikes:
    name: Spikes (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-24.04, macos-14]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Install Linux deps (iced needs xkbcommon/wayland; no libclang needed)
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y pkg-config libxkbcommon-dev libwayland-dev
      - name: Build all targets + run tests for this plan's spike crates
        shell: bash
        run: |
          # Only THIS plan's spikes — NOT spikes/layershell-feasibility (an unrelated
          # pre-existing spike needing Wayland/glib system libs, impossible on macOS).
          # No `set -e`: attempt every crate, fail at the end if any did, so one crate
          # cannot hide the others.
          set -uxo pipefail
          fail=0
          for name in sandbox-executor js-frontend rig-agent visual-diff; do
            d="spikes/$name"
            [ -d "$d" ] || { echo "skip $d (not present yet)"; continue; }
            echo "::group::$d"
            if (cd "$d" && cargo build --all-targets && cargo test); then echo "$d: OK"; else echo "$d: FAILED"; fail=1; fi
            echo "::endgroup::"
          done
          exit $fail

  floor-check:
    name: Floor check (Rust 1.89, ${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-24.04, macos-14]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.89.0
      - name: Install Linux deps
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y pkg-config libxkbcommon-dev libwayland-dev
      - name: Build this plan's spike crates on the workspace floor (1.89)
        shell: bash
        run: |
          # 1.89 = verified true workspace floor (wide@1.4.0 needs 1.89). layershell excluded.
          set -uxo pipefail
          for name in sandbox-executor js-frontend rig-agent visual-diff; do
            d="spikes/$name"
            [ -d "$d" ] || { echo "skip $d (not present yet)"; continue; }
            echo "::group::$d (1.89)"
            # step-level: a build FAILURE here is EVIDENCE (candidate needs > floor), not a pipeline failure.
            (cd "$d" && cargo +1.89.0 build) || echo "FLOOR-CHECK-FAILED: $d (this is evidence, read log above)"
            echo "::endgroup::"
          done
```

- [ ] **Step 4: Validate and commit**

YAML lint catches syntax but not Actions-schema errors. `gh` is not installed here, and `workflow_dispatch` would not list from a feature branch anyway — instead the `pull_request` trigger runs `Spike CI` on PR #60 when `spikes/**` changes, so the first real run happens when a spike crate is pushed (Task 2 onward). Validation here is YAML lint + commit; the controller confirms the run on the PR at the first macOS gate.

Run: `rtk python3 -c "import yaml; yaml.safe_load(open('.github/workflows/spike-ci.yml')); print('yaml ok')"`
Expected: `yaml ok`

```bash
rtk git add spikes/README.md .gitignore .github/workflows/spike-ci.yml
rtk git commit -m "chore(spike): add isolated spikes harness and PR-triggered macOS/floor CI bridge"
rtk git push -u origin feat/smart-redaction-agent-workbench
```

---

## Task 2: Sandbox executor spike (13.2) — HIGHEST RISK

**Decision:** GO/NO-GO on `rquickjs` 0.12.x as the validated-source sandbox runtime. It must execute restricted JS in a fresh, locked-down context with enforced memory/stack/time limits and safe host callbacks, and must build on both Linux and macOS. The MSRV angle is now secondary (rquickjs's 1.87 floor is below the workspace's existing 1.88), but this task establishes the **true workspace floor** as the baseline for the joint MSRV decision. Sequenced first because runtime choice cascades into the parser decision (Task 3) and the executor interface.

**Files:**
- Create: `spikes/sandbox-executor/Cargo.toml`
- Create: `spikes/sandbox-executor/FINDINGS.md`
- Create: `spikes/sandbox-executor/src/main.rs` (driver that runs each runtime experiment and prints structured results)
- Create: `spikes/sandbox-executor/tests/lockdown.rs` (automated lockdown assertions)

**Interfaces:**
- Consumes: the `Spike CI` workflow from Task 1.
- Produces: `spikes/sandbox-executor/FINDINGS.md` with a GO/NO-GO on rquickjs, the verified **true workspace floor**, and rquickjs's own required Rust version — all read by Task 3, Task 4, and Task 6.

- [ ] **Step 1: Scaffold the isolated crate**

Create `spikes/sandbox-executor/Cargo.toml` (empty `[workspace]` makes it standalone):

```toml
[package]
name = "spike-sandbox-executor"
version = "0.0.0"
edition = "2021"
publish = false

[workspace]

[dependencies]
rquickjs = "0.12"          # default features only: pre-generated bindings, no bindgen/libclang; needs only a C compiler
tracing = "0.1"
tracing-subscriber = "0.3"
```

Create `spikes/sandbox-executor/FINDINGS.md` by copying `.claude/skills/rollshot-run-spike/references/findings-template.md` and filling the header: Topic = "rquickjs sandbox executor", Decision = "Is rquickjs 0.12.x a safe, bounded sandbox for validated redaction automation, and does it build at the workspace's real MSRV floor on Linux + macOS?".

- [ ] **Step 2: Establish the TRUE workspace floor + rquickjs's own floor (compile evidence, Linux)**

First confirm the verified reality, so the MSRV decision rests on data, not the stale `Cargo.toml:23` declaration. Modern cargo enforces a dependency's declared `rust-version` at unit-graph build (before heavy compilation) and prints the exact required version.

Run:
```bash
rtk rustup toolchain install 1.85.0   # idempotent (already present)
rtk rustup toolchain install 1.88.0
# (a) Does the REAL workspace build on the stale-declared 1.85? Expected: FAIL, naming iced/image needing 1.88.
rtk bash -c "cd /home/noah/rollshot && cargo +1.85.0 check --workspace 2>&1 | tail -15"
# (b) Does it build on 1.88 (hypothesised true floor)? Expected: PASS.
rtk bash -c "cd /home/noah/rollshot && cargo +1.88.0 check --workspace 2>&1 | tail -5"
# (c) rquickjs's own floor: build the spike crate on 1.85. Expected: cargo RESOLVER error naming rquickjs's required rustc.
rtk bash -c "cd spikes/sandbox-executor && cargo +1.85.0 build 2>&1 | tail -15"
# (d) Build the spike crate on the workspace floor. Expected: PASS (rquickjs 1.87 <= 1.88).
rtk bash -c "cd spikes/sandbox-executor && cargo +1.88.0 build 2>&1 | tail -5"
```

Expected shapes (record verbatim):
- (a) `error: package ... requires rustc 1.88 ...` (iced 0.14.0 / image 0.25.10) → confirms the declared 1.85 is fictional; **true floor ≥1.88**.
- (b) PASS → confirms 1.88 is the real floor.
- (c) a cargo **rust-version resolution** error of the form `error: rustc 1.85.0 is not supported by the following package: rquickjs@0.12.x (requires rustc 1.87)`. **This IS the evidence — do NOT edit any manifest to make it build.** It proves declared MSRV, not intrinsic code MSRV; label it as such. The resolver already names the version, so no 1.86→1.87 probing loop is needed.
- (d) PASS → confirms **rquickjs adds zero MSRV cost** (1.87 ≤ the existing 1.88 floor).

Record four `compile`-evidence rows. This step has no hard gate of its own; it feeds Task 6's MSRV resolution (and the recommendation to correct `Cargo.toml:23`).

- [ ] **Step 3: HARD GATE — macOS C build feasibility (CI; notify the user, then WAIT)**

The quickjs C build must compile on macOS, the platform Rollshot ships. The headless Linux host cannot verify this. This is the genuinely fatal gate, so **gather it before relying on rquickjs**.

Commit + push so the crate is dispatchable:
```bash
rtk git add spikes/sandbox-executor
rtk git commit -m "spike(sandbox): rquickjs crate + workspace-floor and rquickjs-floor probes"
rtk git push
```

**Notify the user with this exact message, then STOP and wait for the result:**
> macOS CI result needed for the sandbox spike. Please open repo **Actions → "Spike CI" → Run workflow** on branch `feat/smart-redaction-agent-workbench`, then report: (1) did the **`Spikes (macos-14)`** job's build+test step pass for `spikes/sandbox-executor`? (2) what did the **`Floor check (Rust 1.88, macos-14)`** step print for `spikes/sandbox-executor` (paste the log tail — the job shows GREEN regardless, so report the STEP result, not the job dot)?

Gate semantics: if rquickjs **fails to build on macOS** and cannot be mitigated → **rquickjs is NO-GO**; record the fallback (see Step 9) and skip to Step 9. Steps 4–7 below are Linux **runtime** evidence worth gathering **while waiting** for the macOS result, but they are provisional — if Step 3 returns FAIL, they are discarded and the spike closes NO-GO.

- [ ] **Step 4: Lockdown — enumerate and deny ambient capabilities (automated, Linux)**

Prove a fresh minimal context exposes no network/fs/timer/async/dynamic-eval/reflection capability. Create `spikes/sandbox-executor/tests/lockdown.rs`.

**Important API note (resolve the eval-to-test-eval circularity):** in rquickjs 0.12, `eval`/`Function` come from the `Eval` intrinsic and `Promise`/`Proxy` are their own removable intrinsics. Build the context with `Context::custom`/`Context::builder()` selecting only what redaction needs (e.g. `Json`), NOT `Context::full`. But then `ctx.eval(...)` itself requires the `Eval` intrinsic — so you **cannot use `ctx.eval` to prove `eval` is absent**. Test globals under the chosen intrinsic set, and verify `eval`/`Function` absence via the context's intrinsic configuration (and/or compiled `Module`/bytecode evaluation), not via `eval`. Confirm exact constructors against rquickjs 0.12 docs.rs; the experiment is what matters.

JS probes to assert (runtime-stable regardless of binding API). Each forbidden global must be `undefined` (or unreachable) under the chosen intrinsics:

```text
FORBIDDEN_GLOBALS = [
  "setTimeout", "setInterval", "queueMicrotask",
  "Promise", "fetch", "XMLHttpRequest", "WebSocket",
  "require", "process", "global", "globalThis",
  "import", "Function", "eval",
  "Proxy", "Reflect", "WeakRef", "FinalizationRegistry",
  "Worker", "Deno", "Bun", "document", "window",
]
Assertions:
- for g in FORBIDDEN_GLOBALS  => `typeof <g>` is "undefined" (evaluated without depending on the Eval intrinsic)
- `import("x")`               => never resolves a module (parse/throw)
- prototype mutation          => a fresh per-run context means any mutation cannot leak across runs (assert by mutating in run A, re-creating context, asserting clean in run B)
```

Run: `rtk bash -c "cd spikes/sandbox-executor && cargo test --test lockdown -- --nocapture"`
Expected: all assertions PASS. Any forbidden global reachable and not removable by intrinsic configuration → record `FAIL` (hard), with the mitigation attempt. Evidence `automated`.

- [ ] **Step 5: Resource limits — memory, stack, time/loop interruption (runtime, Linux)**

In `src/main.rs`, three guarded runs (confirm exact API: `Runtime::set_memory_limit`, `Runtime::set_max_stack_size`, `Runtime::set_interrupt_handler`).

Exact JS payloads:
- Infinite loop: `while (true) {}` with an interrupt handler tripping after a step/wall-clock budget → must return a controlled interruption error, not hang.
- Memory bomb: `const a = []; for (;;) { a.push(new Array(100000).fill(0)); }` under a small `set_memory_limit` → must return an out-of-memory error, not abort the process.
- Deep recursion: `function f(){ return f(); } f();` under `set_max_stack_size` → must return a stack error, not segfault.

Run: `rtk bash -c "cd spikes/sandbox-executor && timeout 30 cargo run 2>&1 | tail -40"`
Expected: each payload returns a typed error; the process exits cleanly (no SIGSEGV/SIGABRT, no hang). Three rows, Gate `hard`, Evidence `runtime`. A crash or hang = `FAIL`.

- [ ] **Step 6: Host callback safety + cancellation (runtime, Linux)**

Register a Rust host function (a stand-in `rollshot.ocr()` capability). Test:
- Call it from JS and marshal an array-of-rects return value back to Rust.
- Have the host fn return `Err` → JS observes a controlled exception; Rust does not panic/UB.
- Trip the interrupt handler **while inside** a long host callback → verify clean teardown.

Run: `rtk bash -c "cd spikes/sandbox-executor && cargo run 2>&1 | tail -20"` (extend the driver).
Expected: host error surfaces as a JS exception; cancellation during a callback tears down cleanly. Gate `hard`, Evidence `runtime`.

- [ ] **Step 7: Fresh-context cost + binary footprint (automated/compile, Linux)**

In `main.rs`, time creating + dropping N fresh contexts running a trivial detector; record µs/context. Then measure release binary size.

Run:
```bash
rtk bash -c "cd spikes/sandbox-executor && cargo run --release 2>&1 | tail -10"
rtk bash -c "cd spikes/sandbox-executor && ls -l target/release/spike-sandbox-executor"
rtk bash -c "cd spikes/sandbox-executor && cargo tree | grep -i bindgen || echo 'no bindgen in default graph'"
```
Expected: per-context cost recorded (target: well under a per-run budget, e.g. < 5 ms); binary delta recorded; `no bindgen in default graph` (confirms the no-libclang rationale). Gate `soft`, Evidence `automated`/`compile`.

- [ ] **Step 8: macOS CPU-runtime parity — lockdown + resource limits only (CI; notify the user)**

The lockdown + resource-limit tests are CPU-only and run in CI on macOS via the `Spikes (macos-14)` job's `cargo test`. This proves **CPU-only** sandbox behavior on macOS, not any GPU/display path.

```bash
rtk git add spikes/sandbox-executor
rtk git commit -m "spike(sandbox): lockdown, resource-limit, host-callback, cost experiments"
rtk git push
```
**Notify the user:** re-run **"Spike CI"** workflow_dispatch on the branch; report whether **`Spikes (macos-14)`** passed `tests/lockdown.rs` and the driver. Record Result `PASS`/`FAIL`/`UNTESTED` with the CI run URL; evidence `runtime`, caveat "CPU-only sandbox tests; no GPU/display".

- [ ] **Step 9: Close the spike — GO/NO-GO + MSRV inputs**

Fill `FINDINGS.md` Final Recommendation:
- **Go/no-go on rquickjs 0.12.x** from the Step 3/5/6/8 hard gates (any hard FAIL = NO-GO).
- **MSRV inputs for Task 6:** the verified true workspace floor (≥1.88, from Step 2a/2b) and rquickjs's required version (Step 2c). State that rquickjs is MSRV-free relative to the existing floor, and recommend correcting the stale `Cargo.toml:23` `rust-version`.
- **Rejected alternatives & fallback triggers:** Boa (pure-Rust, gives an AST too — synergy with Task 3 — but **note: latest `boa_engine` declares `rust-version` 1.88, i.e. it sits AT the workspace floor, not below it; "Boa to keep 1.85" is not a real option since 1.85 is already off the table**; evaluate Boa's sandbox/interrupt maturity); deno_core / v8 (heavy, large binary). Fallback trigger = rquickjs hard-gate FAIL on macOS build, lockdown, or resource limits.
- **Remaining risks** (e.g. interrupt granularity, memory-limit accounting accuracy).

```bash
rtk git add spikes/sandbox-executor/FINDINGS.md
rtk git commit -m "spike(sandbox): record findings and rquickjs go/no-go"
```

---

## Task 3: JavaScript frontend / parser spike (13.1)

**Decision:** Which Rust parser produces the restricted-subset validator + Workflow IR normalizer with accurate source spans, acceptable maintenance/license/binary cost, and an acceptable MSRV impact relative to the workspace's ≥1.88 floor. The execution runtime (Task 2) and the **validation parser** are separate concerns (spec §5.1). **Synergy note:** if Task 2 selected Boa as runtime, `boa_parser`/`boa_ast` can serve double duty (parse for validation AND execute), collapsing two decisions — evaluate it as a first-class candidate.

**Files:**
- Create: `spikes/js-frontend/Cargo.toml` (candidate parsers behind cargo features)
- Create: `spikes/js-frontend/FINDINGS.md`
- Create: `spikes/js-frontend/src/main.rs` (runs the experiments per enabled candidate)
- Create: `spikes/js-frontend/fixtures/valid_detector.js` plus one fixture per high-value §5.2 rejection (below)

**Interfaces:**
- Consumes: Task 2's runtime decision (Boa synergy) and the verified workspace floor.
- Produces: `spikes/js-frontend/FINDINGS.md` with a parser shortlist + a decision matrix; the final pick is made **jointly with the MSRV resolution in Task 6**, not presumed here.

- [ ] **Step 1: Scaffold crate with candidate parsers behind features (PINNED versions)**

Create `spikes/js-frontend/Cargo.toml`. **Do NOT use `version = "0.*"` wildcards** — they resolve to the latest release, which for the pure-Rust parsers busts low toolchains for *version* reasons (e.g. latest `oxc` requires Rust ~1.94, `boa` ~1.88, `swc` deps ~1.86–1.88) and tells you nothing about the parser's intrinsic fitness. Pin each candidate to a concrete current release and **record the minimum Rust that version imposes** as the real MSRV column. Confirm exact latest versions on crates.io at execution time; the example pins below are representative.

```toml
[package]
name = "spike-js-frontend"
version = "0.0.0"
edition = "2021"
publish = false

[workspace]

[features]
default = []
oxc = ["dep:oxc_allocator", "dep:oxc_parser", "dep:oxc_span", "dep:oxc_ast"]
swc = ["dep:swc_common", "dep:swc_ecma_parser", "dep:swc_ecma_ast"]
treesitter = ["dep:tree-sitter", "dep:tree-sitter-javascript"]
boa = ["dep:boa_engine"]

[dependencies]
# Pin to a single matching oxc release (the oxc_* crates must share a version).
oxc_allocator = { version = "0.137", optional = true }
oxc_parser    = { version = "0.137", optional = true }
oxc_span      = { version = "0.137", optional = true }
oxc_ast       = { version = "0.137", optional = true }
# swc crates version-match is fiddly; pin a known-compatible set at execution.
swc_common    = { version = "5", optional = true }
swc_ecma_parser = { version = "13", optional = true }
swc_ecma_ast  = { version = "5", optional = true }
tree-sitter   = { version = "0.25", optional = true }
tree-sitter-javascript = { version = "0.23", optional = true }
boa_engine    = { version = "0.20", optional = true }
```

Create `FINDINGS.md`; Decision = "Which parser backs the restricted-subset validator + Workflow IR normalizer, and what MSRV does it impose?".

- [ ] **Step 2: Author the fixtures (valid + high-value §5.2 rejections)**

`fixtures/valid_detector.js` — uses only allowed constructs (spec §5.2):

```js
const matches = rollshot.ocr({ region: "full" })
  .filter((m) => m.confidence > 0.8)
  .map((m) => ({ x: m.x, y: m.y, w: m.w, h: m.h }));
return { candidates: matches };
```

Create one fixture per high-value §5.2 **rejected** construct (the actual security boundary), each a single representative violation:

- `fixtures/reject_var.js`: `var leaked = 1; return { candidates: [] };`
- `fixtures/reject_while.js`: `const o = []; while (true) { o.push(1); } return { candidates: o };`
- `fixtures/reject_dynamic_access.js`: `const k = "ocr"; return { candidates: rollshot[k]({ region: "full" }) };` (dynamic/computed property access)
- `fixtures/reject_reflect.js`: `return { candidates: Reflect.ownKeys(rollshot) };` (reflection; also covers Proxy family)
- `fixtures/reject_recursion.js`: `function f(n){ return n <= 0 ? [] : f(n - 1); } return { candidates: f(3) };`
- `fixtures/reject_class.js`: `class D {} return { candidates: [new D()] };` (class declaration + construction)
- `fixtures/reject_escaping_closure.js`: `const sink = []; [1,2].map((x) => sink.push(x)); return { candidates: sink };` (closure escaping its approved collection call)
- `fixtures/reject_generator.js`: `function* g(){ yield 1; } return { candidates: [...g()] };` (generator + unbounded iteration)

- [ ] **Step 3: Experiment A — parse + source-span quality (per candidate)**

In `src/main.rs`, for each enabled feature, parse `valid_detector.js` (expect accept) and each `reject_*.js`. For every reject fixture, extract the precise span (line, column, byte range) of the offending construct.

Run per candidate:
```bash
rtk bash -c "cd spikes/js-frontend && cargo run --features oxc 2>&1 | tail -40"
rtk bash -c "cd spikes/js-frontend && cargo run --features swc 2>&1 | tail -40"
rtk bash -c "cd spikes/js-frontend && cargo run --features treesitter 2>&1 | tail -40"
rtk bash -c "cd spikes/js-frontend && cargo run --features boa 2>&1 | tail -40"
```
Expected: each prints the AST root + a precise span per offending node. Record span accuracy (byte-exact / line-only / poor) per candidate. Evidence `automated`.

- [ ] **Step 4: Experiment B — subset-validation traversal across the §5.2 set (per candidate)**

For each candidate, implement a minimal walker that **accepts** `valid_detector.js` and **rejects** every `reject_*.js` with a span. This demonstrates the parser can actually enforce the spec's restricted subset, not just a token denylist. Record per-construct accept/reject + ergonomics (arena lifetimes for `oxc`, source maps for `swc`, CST cursor for `tree-sitter`, owned AST for `boa`).

Run: same commands as Step 3 (the walker runs inside `main.rs`).
Expected: `accept` for valid; `reject{construct, span}` for all eight reject fixtures. Any construct the walker cannot detect on a candidate = a gap; record it.

Note: this spike validates **traversal capability** against representative violations. The full, versioned allow/deny contract (spec §5.2's complete enumeration) is produced in the automation-frontend subproject, not here — this is an explicit scope boundary.

- [ ] **Step 5: Experiment C — Workflow IR extraction feasibility (per candidate)**

From `valid_detector.js`'s AST, extract the ordered `rollshot.*` capability calls (`ocr`), the collection operators (`filter`, `map`), and the final returned candidate shape — proving the AST yields what Workflow IR normalization (spec §5.3) needs. Print the extracted sequence.

Run: same commands.
Expected: prints `[ocr, filter, map] -> return.candidates`. Record feasibility per candidate.

- [ ] **Step 6: Experiment D — MSRV / license / maintenance / binary cost matrix (per candidate)**

```bash
# Real MSRV column = min Rust the PINNED version imposes. Probe by building on the workspace floor:
rtk bash -c "cd spikes/js-frontend && cargo +1.88.0 build --features oxc 2>&1 | tail -6"
rtk bash -c "cd spikes/js-frontend && cargo +1.88.0 build --features swc 2>&1 | tail -6"
rtk bash -c "cd spikes/js-frontend && cargo +1.88.0 build --features treesitter 2>&1 | tail -6"
rtk bash -c "cd spikes/js-frontend && cargo +1.88.0 build --features boa 2>&1 | tail -6"
# binary + dep-tree size per feature:
rtk bash -c "cd spikes/js-frontend && cargo build --release --features oxc && ls -l target/release/spike-js-frontend && cargo tree --features oxc | wc -l"
# (repeat --release sizing + dep-count per feature)
```
Record per candidate:
- **MSRV imposed:** does the pinned version build on 1.88 (≤ floor, free) or require more (e.g. oxc may demand ~1.94 → raises the floor — a real cost)? If a `cargo +1.88.0 build` fails with a `rust-version` resolver error, that named version IS the candidate's MSRV.
- **License:** oxc=MIT, swc=Apache-2.0, tree-sitter=MIT(+C), boa=Unlicense/MIT.
- **Maintenance activity:** latest crates.io release date + a quick repo-activity signal (last commit / release cadence) per candidate. `gh` is not installed; use `curl` against the crates.io API, e.g. `rtk curl -s https://crates.io/api/v1/crates/oxc_parser | rtk python3 -c "import json,sys; d=json.load(sys.stdin)['crate']; print(d['updated_at'], d['max_stable_version'])"`. This is §13.1's "Maintenance activity" dimension.
- **Binary + dep-tree size.**

Note: "pure Rust" and "low MSRV" are **orthogonal** — at latest versions, the C-based `tree-sitter` (MSRV ~1.77) is the lowest-MSRV candidate, while the pure-Rust ones impose ≥1.88 (oxc the highest). Do not conflate them.

- [ ] **Step 7: macOS build parity (CI; notify the user) — only if a C-using finalist (tree-sitter)**

Pure-Rust parsers (oxc/swc/boa) need no macOS-specific check. If `tree-sitter` is a finalist, confirm its C build on macOS.

**Notify the user only if tree-sitter is a finalist:** re-run **"Spike CI"** and report whether **`Spikes (macos-14)`** built `spikes/js-frontend` with the `treesitter` feature. Otherwise record macOS parity as `UNTESTED — pure Rust, low risk`.

```bash
rtk git add spikes/js-frontend
rtk git commit -m "spike(js-frontend): parser candidate comparison (span/traversal/IR/MSRV/maintenance/cost)"
rtk git push
```

- [ ] **Step 8: Close — shortlist + defer final pick to the joint MSRV decision**

Fill `FINDINGS.md` Final Recommendation: a ranked shortlist by span quality + §5.2 traversal coverage + maintenance + license + binary cost, **with the per-candidate MSRV recorded as evidence but the final pick deferred to Task 6** — because if Task 2 or Task 4 already pushes the floor (or a candidate like oxc raises it to ~1.94), the MSRV weighting changes. State explicitly: do not presume 1.85 or even 1.88 as the target floor until Task 6 resolves it. Correct any "Boa = 1.85-compatible" framing (latest Boa = 1.88). Note tree-sitter is the only sub-1.88-MSRV option at latest versions. Rejected alternatives + fallback triggers.

```bash
rtk git add spikes/js-frontend/FINDINGS.md
rtk git commit -m "spike(js-frontend): record parser shortlist and MSRV evidence"
```

---

## Task 4: Rig integration spike (13.3)

**Decision:** GO/NO-GO on `rig` 0.39.x for **manual** `AgentRun` driving behind a Rollshot-owned provider facade (spec §4.2 forbids `agent.prompt()` as the control plane); else hand-roll a provider trait. Also measures rig's **real consumer MSRV** (it uses `edition = "2024"`; its dev `rust-toolchain.toml` pins 1.91, but that is not necessarily its published `rust-version`) — rig is part of the MSRV cross-cut. Most verification uses a deterministic driver/fake model — no network, runs on the headless host. A live vision call is OPTIONAL/manual.

**Files:**
- Create: `spikes/rig-agent/Cargo.toml`
- Create: `spikes/rig-agent/FINDINGS.md`
- Create: `spikes/rig-agent/src/main.rs` (manual driving loop + facade)
- Create: `spikes/rig-agent/tests/driver.rs` (deterministic tool-call sequence + cancellation)

**Interfaces:**
- Consumes: the verified workspace floor (Task 2 Step 2).
- Produces: `spikes/rig-agent/FINDINGS.md` with GO/NO-GO on rig, the facade shape, and rig's required Rust version — **feeds the joint MSRV cross-cut** in Task 6.

- [ ] **Step 1: Scaffold crate**

Create `spikes/rig-agent/Cargo.toml` (rig is vendored at `learn-projects/rig` for reference; confirm the exact crate/feature names there). The multimodal `UserContent::Image` message content is **not** feature-gated — the `image` feature only gates image-generation clients — so no extra feature is needed for building image messages.

```toml
[package]
name = "spike-rig-agent"
version = "0.0.0"
edition = "2021"
publish = false

[workspace]

[dependencies]
rig-core = "0.39"
tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros", "sync", "time"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"

[dev-dependencies]
tokio = { version = "1", features = ["rt", "macros", "test-util", "sync", "time"] }
```

Create `FINDINGS.md`; Decision = "Can Rollshot drive Rig 0.39.x as a sans-I/O AgentRun behind its own facade with budgets/cancellation/usage, at the workspace MSRV floor, or must we hand-roll a provider trait?".

- [ ] **Step 2: rig MSRV probe (compile evidence, Linux) — feeds the cross-cut**

Before runtime work, measure rig's MSRV the same way as Task 2.

Run:
```bash
rtk bash -c "cd spikes/rig-agent && cargo +1.85.0 build 2>&1 | tail -12"
rtk bash -c "cd spikes/rig-agent && cargo +1.88.0 build 2>&1 | tail -6"
```
Expected: 1.85 likely FAILS (rig is `edition = "2024"`; expect a resolver `rust-version` error naming rig's or a transitive dep's required rustc). Record what 1.88 does: PASS → rig sits at/below the floor (free); FAIL naming a version >1.88 → **rig pushes the floor up** (a real cost, like oxc). Record as a `compile` row feeding Task 6.

- [ ] **Step 3: HARD GATE — manual multi-turn driving (automated)**

The core architectural requirement: drive the loop turn-by-turn ourselves, NOT via `agent.prompt()`. rig 0.39 exposes a sans-I/O `AgentRun` (in vendored rig: `agent/run/mod.rs`, `next_step() -> AgentRunStep::{CallModel, CallTools, Done}`, advanced by feeding `model_response` / `tool_results`). The **driver** owns model invocation. So the cleanest test feeds a scripted `ModelTurn` directly into `AgentRun` per step — a fake `CompletionModel` is **optional** (needed only if you exercise rig's own internal loop; if implemented it must provide `completion` + `stream` methods and the associated `Response`/`StreamingResponse` types). Confirm exact types in `learn-projects/rig`.

In `tests/driver.rs`, drive a scripted sequence: step 1 → tool call `inspect_ocr`; step 2 (after injecting its result) → tool call `replace_automation_source`; step 3 → `Done`/submit.

Run: `rtk bash -c "cd spikes/rig-agent && cargo test --test driver -- --nocapture"`
Expected: the manual loop advances through all three steps; each tool call observed and each tool result injected without surrendering control to a high-level prompt loop. Gate `hard`, Evidence `automated`. If rig forces `agent.prompt()` with no per-step driving → `FAIL`, record the hand-roll fallback.

- [ ] **Step 4: Tool schema + structured tool-call normalization (automated)**

Define a tool with a JSON schema (`inspect_ocr{region, max_results}`); verify rig surfaces the model's tool call as a normalized typed call with parsed arguments (via the scripted turn).

Run: same test command.
Expected: tool call arrives with `region`/`max_results` parsed. Evidence `automated`.

- [ ] **Step 5: Cancellation (automated)**

Drive a run that stalls (a pending future), then cancel via `tokio_util::sync::CancellationToken` or by dropping the future under `tokio::time::timeout`; assert clean teardown, no panic.

Run: same test command.
Expected: prompt stop, no panic, partial state observable. Gate `hard`, Evidence `automated`.

- [ ] **Step 6: Usage accounting + multimodal message construction (automated/compile)**

- Build a multimodal user message (image bytes + text) using rig's `UserContent::Image` + text types and assert it constructs/serializes (compile + automated; do NOT claim provider acceptance — no `image` feature needed).
- Verify rig exposes per-response usage (input/output tokens); inject a usage value via the scripted turn and read it back.

Run: same test command.
Expected: multimodal message builds; usage readable. Evidence `automated`(usage)/`compile`(image message). Provider-specific image encoding stays `UNTESTED` here (Step 8).

- [ ] **Step 7: Privacy-safe tracing + Rollshot facade (automated/compile)**

- Wrap rig behind a `RollshotModel` trait (spec §4.1) and implement it for two scripted providers; swap at runtime to prove provider selection lives behind the facade.
- Inspect whether rig emits prompts/responses via `tracing`; verify a `tracing_subscriber` filter can suppress/redirect those targets so no prompt/response text leaks. Record rig's default logging.

Run: `rtk bash -c "cd spikes/rig-agent && cargo run 2>&1 | tail -30"`
Expected: facade swaps providers; default rig logging characterized + suppression strategy identified. Evidence `automated`/`compile`.

- [ ] **Step 8: OPTIONAL live vision round-trip (manual; notify the user) — runs on the headless host (network only, no display/macOS)**

Provider-specific image + tool behavior can only be fully proven against a real API. Optional; sends data.

**Notify the user:** "Optional: to verify a live multimodal + tool round-trip I need an API key (e.g. `ANTHROPIC_API_KEY`) in the spike env and your OK to send a test image to the provider. This runs here on the headless host (network only). Skip → recorded as `UNTESTED (optional/manual)`." If a key is provided, drive one turn against the real provider with a tiny image (use the latest Claude model id — see the `claude-api` skill). Otherwise record `UNTESTED`. As a non-live alternative matching spec §11.6, capture one **recorded** provider response fixture and assert rig normalizes its tool calls.

- [ ] **Step 9: macOS parity (CI; notify the user) + close — GO/NO-GO**

```bash
rtk git add spikes/rig-agent
rtk git commit -m "spike(rig): MSRV probe, manual AgentRun driving, tools, cancellation, usage, facade"
rtk git push
```
**Notify the user:** re-run **"Spike CI"**; report whether **`Spikes (macos-14)`** built/tested `spikes/rig-agent`, and what the **`Floor check (Rust 1.88, macos-14)`** step printed for it. Record `PASS`/`FAIL`/`UNTESTED`.

Fill `FINDINGS.md` Final Recommendation: GO/NO-GO from the Step 3 + Step 5 hard gates; the facade shape; tracing-privacy strategy; rig's measured MSRV (Step 2) as a cross-cut input. **If Step 8 is skipped, the GO is explicitly conditional** — provider-specific structured tool behavior remains `UNTESTED`; record it under "Remaining risks" so the downstream agent-core subproject must close it (recorded fixture or live test). Rejected alternative (hand-rolled provider trait + raw HTTP) + its cost; fallback trigger (rig undrivable manually, leaks prompts unsuppressibly, or pushes MSRV unacceptably). Note the design (§12) permits not using rig.

```bash
rtk git add spikes/rig-agent/FINDINGS.md
rtk git commit -m "spike(rig): record rig integration decision"
```

---

## Task 5: Visual diff spike (13.4)

**Decision:** rendering approach for candidate overlays + before/after + **source diff and Workflow IR semantic summary** in the iced Result Workspace; and whether overlay rendering scales to many candidates on tall stitched images. **Environment reality:** the dev host is headless and macOS is CI-only — true interactive GPU latency is NOT obtainable here. This task measures what is obtainable (CPU-side culling/hit-testing/diff cost via headless benchmarks + iced compile) and explicitly flags GPU/interaction latency for a real-display run.

**Files:**
- Create: `spikes/visual-diff/Cargo.toml`
- Create: `spikes/visual-diff/FINDINGS.md`
- Create: `spikes/visual-diff/benches/overlay_cull.rs` (criterion; headless, CPU-side)
- Create: `spikes/visual-diff/src/main.rs` (iced 0.14 prototype: proposed vs accepted overlays + before/after toggle + source-diff pane + IR semantic summary)

**Interfaces:**
- Consumes: nothing from other spikes (independent; can run in parallel). References (does not import) the existing culling/viewport logic in `crates/rollshot-app/src/result_workspace/canvas.rs` and `viewport.rs`, and crop visual tokens in `crates/rollshot-overlay-core`.
- Produces: `spikes/visual-diff/FINDINGS.md` with the proposed-annotation model recommendation (labeled as a design recommendation, not spike-measured) + CPU-side latency numbers + the GPU-latency gap flagged for manual verification.

- [ ] **Step 1: Scaffold crate (invoke the `iced-rs` skill before any iced code)**

Create `spikes/visual-diff/Cargo.toml`:

```toml
[package]
name = "spike-visual-diff"
version = "0.0.0"
edition = "2021"
publish = false

[workspace]

[dependencies]
iced = { version = "0.14", features = ["canvas", "image", "tokio"] }
image = { version = "0.25", default-features = false, features = ["png"] }
similar = "2"              # text diff for the source-diff pane
tracing = "0.1"

[dev-dependencies]
criterion = { version = "0.5", default-features = false }

[[bench]]
name = "overlay_cull"
harness = false
```

Create `FINDINGS.md`; Decision = "How are proposed redaction candidates rendered/diffed in the Result Workspace, and does it scale to many candidates on tall stitched images?". (Note: `iced 0.14` itself sets the workspace floor at 1.88 — no new MSRV cost here.)

- [ ] **Step 2: HARD-MEASURABLE — CPU-side overlay cost, ordinary AND tall images (automated, headless)**

This is the part the headless host CAN measure. In `benches/overlay_cull.rs`, benchmark, at 100 / 500 / 1000 candidates, on **two image sizes** (ordinary `1920×1080` and tall stitched `4000×12000` — §13.4 names both): (a) frustum culling against a viewport rect (the `visible_image_rect` + per-annotation bounds-intersection pattern from `result_workspace/canvas.rs`), (b) hit-testing a point against all candidates, (c) computing a before/after candidate-set diff.

Run: `rtk bash -c "cd spikes/visual-diff && cargo bench 2>&1 | tail -50"`
Expected: per-pass timings at each candidate count × image size. Soft gate: cull pass < ~2 ms at 1000 candidates (well within a 16 ms frame). Record numbers. Evidence `automated`. (Ordinary is expected strictly easier than tall; record both so the §13.4 input range is covered.)

- [ ] **Step 3: Build the iced prototype incl. source diff + IR semantic summary (compile, Linux)**

In `src/main.rs` (per the `iced-rs` skill), build an iced 0.14 app rendering, over a tall test image in a `scrollable` + `Canvas`:
- accepted annotations (existing style) and proposed candidates in a **visually distinct** style (spec §8.2: distinct outline/opacity + low-confidence treatment), with a **before/after** toggle (proposed hidden vs shown);
- a `similar`-based **source-diff** text pane (old vs new automation JS);
- a **Workflow IR semantic summary** pane (spec §8.3 / §13.4): render a hand-authored IR value for `valid_detector.js` as a human-readable summary — capability list (`ocr`), thresholds (`confidence > 0.8`), and current-image candidate-count delta. This proves the IR-summary review surface, distinct from the source diff.

Run: `rtk bash -c "cd spikes/visual-diff && cargo build 2>&1 | tail -10"`
Expected: compiles. Evidence `compile`. (Do NOT claim it renders correctly — that needs a display.)

- [ ] **Step 4: Attempt headless GUI run, else flag for real-display verification**

Best-effort software-rendered run on the headless host:
```bash
rtk bash -c "command -v xvfb-run && echo have-xvfb || echo no-xvfb"
rtk bash -c "cd spikes/visual-diff && WGPU_BACKEND=gl LIBGL_ALWAYS_SOFTWARE=1 xvfb-run -a cargo run 2>&1 | tail -30 || true"
```
Expected: either a software-rendered smoke run (record `MITIGATED — software renderer, not representative of GPU latency`) or it cannot run headless (`UNTESTED`).

Then **notify the user** for representative interactive latency:
> Visual-diff interactive/GPU latency needs a real display. Options: (a) you run `spikes/visual-diff` on a machine with a GPU/display and report frame/interaction latency at 100/500/1000 candidates while scrolling a tall image; or (b) accept the CPU-side numbers from Step 2 + the compile result and mark GPU latency `UNTESTED`. Which?

Record the outcome. Do not promote compile/CPU evidence into a GPU-latency claim.

- [ ] **Step 5: macOS compile parity (CI; notify the user)**

```bash
rtk git add spikes/visual-diff
rtk git commit -m "spike(visual-diff): overlay cull bench + iced proposed/accepted + before/after + source/IR diff prototype"
rtk git push
```
**Notify the user:** re-run **"Spike CI"**; report whether **`Spikes (macos-14)`** built `spikes/visual-diff` (compile parity for the iced path). GUI latency on CI is also headless → `UNTESTED` there. Record result.

- [ ] **Step 6: Close — recommend the model (separate evidence from design judgment)**

Fill `FINDINGS.md` Final Recommendation, keeping the two cleanly separated:
- **Spike-measured:** CPU-side culling/hit-test/diff latency verdict (Step 2); compile feasibility of the proposed-vs-accepted overlay, before/after toggle, source diff, and IR summary (Step 3); GPU/interaction latency = `UNTESTED`/`MITIGATED` with how to close it.
- **Design recommendation (NOT spike-tested):** proposed annotations as a **transient review wrapper** converted to a committed `Annotation` on accept (keeps `rollshot-image-document` free of agent concerns) vs a first-class `Annotation` variant — explicitly labeled as design reasoning, because the bench/compile steps did not exercise the data-model choice. Rendering approach (reuse existing culling/downscaling), before/after as toggle vs side-by-side.
- Rejected alternatives + fallback triggers.

```bash
rtk git add spikes/visual-diff/FINDINGS.md
rtk git commit -m "spike(visual-diff): record rendering/model decision"
```

---

## Task 6: Consolidate decisions + resolve MSRV + handoff

Aggregate the four `FINDINGS.md` into one product-facing decisions document, resolve the workspace MSRV (the joint output of Tasks 2 + 3 + 4), state what each downstream subproject can lock, and retire the temporary CI workflow.

**Files:**
- Create: `docs/superpowers/spikes/2026-06-20-spike-decisions.md`
- Modify: all four `spikes/*/FINDINGS.md` (set lifecycle `retained-reference`)
- Delete: `.github/workflows/spike-ci.yml`

**Interfaces:**
- Consumes: all four `FINDINGS.md`.
- Produces: `docs/superpowers/spikes/2026-06-20-spike-decisions.md` — the retained decision record the next plans cite.

- [ ] **Step 1: Write the consolidated decision record**

Create `docs/superpowers/spikes/2026-06-20-spike-decisions.md` with sections:
- **Sandbox runtime:** chosen runtime + GO/NO-GO (Task 2).
- **Parser:** chosen parser (Task 3), incl. Boa double-duty note if applicable.
- **Agent/provider:** rig GO/NO-GO + facade (Task 4).
- **Visual diff:** proposed-annotation model (design recommendation) + rendering approach + the open GPU-latency item (Task 5).
- **MSRV resolution (the joint decision):** state the verified true current floor (≥1.88 from iced 0.14.0 + image 0.25.10), and the **max required Rust across all chosen deps** (rquickjs ≤1.88; the picked parser — tree-sitter ~1.77 / boa ~1.88 / swc ~1.88 / oxc ~1.94; rig's measured version). The resolved workspace MSRV = that max. **Action item:** correct the stale `Cargo.toml:23` `rust-version` to the resolved value (a hygiene fix the spikes surfaced). If a candidate (e.g. oxc) would push the floor above 1.88, record the explicit choice: accept the higher floor, or pin an older candidate version. Confirm `iced 0.14` etc. already build at the resolved floor (they declare it).
- **Downstream locks:** for delivery subprojects 3 (automation frontend/runtime) and 4 (bounded agent core), state exactly which dependency/interface choices are now fixed, and which risks carry forward (e.g. rig provider-specific tool behavior if `UNTESTED`).
- **Parallel track:** restate subproject 2 (`ImageDocument` batch transaction + typed edit ops) is unblocked and proceeds as its own spec.

- [ ] **Step 2: Mark all spikes retained-reference + freeze lockfiles**

In each `spikes/*/FINDINGS.md`, set `Lifecycle: retained-reference` and `Last updated:` to the execution date. Confirm each `spikes/*/Cargo.lock` is committed and add a one-line note in the decisions doc that these lockfiles are frozen-as-of-decision evidence (do not re-resolve).

- [ ] **Step 3: Retire the temporary CI bridge**

```bash
rtk git rm .github/workflows/spike-ci.yml
```
(The spike crates remain as retained evidence; only the temporary workflow is removed. If the user wants ongoing spike CI, keep it instead — confirm with the user.)

- [ ] **Step 4: Commit the consolidation**

```bash
rtk git add docs/superpowers/spikes/2026-06-20-spike-decisions.md spikes/*/FINDINGS.md
rtk git commit -m "docs(spike): consolidate smart-redaction spike decisions and MSRV resolution"
```

- [ ] **Step 5: Verify nothing leaked into the root workspace**

Run: `rtk cargo metadata --format-version 1 --no-deps | rtk python3 -c "import json,sys; m=json.load(sys.stdin); names=[p['name'] for p in m['packages']]; assert not any(n.startswith('spike-') for n in names), names; print('clean: no spike crates in workspace')"`
Expected: `clean: no spike crates in workspace`

Run: `rtk cargo build --workspace`
Expected: PASS (production build unaffected by the spikes).

---

## Self-Review

- **Spec §13 coverage:** §13.1 → Task 3 (span/traversal/IR/MSRV/license/**maintenance**/binary, all eight dimensions); §13.2 → Task 2 (fresh-context, mem/stack/time interrupt, frozen host API/lockdown, disabling imports/timers/async/ambient, host-callback safety, cancellation, footprint, cross-platform build, MSRV); §13.3 → Task 4 (multimodal+tools, manual AgentRun, provider-structured tools [live/fixture], cancellation, usage, privacy-safe tracing, facade provider selection, MSRV); §13.4 → Task 5 (overlays on **ordinary + tall** screenshots, before/after, **source diff**, **Workflow IR semantic summary**, latency with many candidates). Covered.
- **Other spec refs:** §4.2 MSRV cross-cut → Global Constraints + Task 2 Step 2 + Task 4 Step 2 + Task 6 Step 1 (joint, corrected to true ≥1.88 floor); §5.2 restricted subset → Task 3 fixtures cover var/while/dynamic-access/reflection/recursion/class/escaping-closure/generator + Task 2 lockdown denies Proxy/Reflect/eval/Function/timers/async/network (with explicit scope note that the full versioned contract is the automation-frontend subproject); §9.6 privacy-safe tracing → Task 4 Step 7; §12 "first plan = spikes, not product" → whole plan; subproject-2 parallel note → File Structure + Task 6 Step 1.
- **Verified-fact corrections applied:** iced 0.14.0 = `rust-version 1.88`/edition 2024 and image 0.25.10 = `rust-version 1.88.0` (both in current Cargo.lock) → real floor ≥1.88, declared 1.85 is stale; rquickjs (1.87) is MSRV-free; parser version pins (no `0.*`) with MSRV-imposed column; Boa-at-latest = 1.88 (not a 1.85 escape); rig folded into the MSRV cross-cut.
- **Platform/CI policy:** every macOS / real-display dependency STOPs and notifies the user with the exact workflow + which STEP/log to report (Task 2 Steps 3/8, Task 3 Step 7, Task 4 Steps 8/9, Task 5 Steps 4/5); floor-check + MSRV evidence is read from step logs, not the green dot; spike-ci installs iced's Linux deps and builds-but-doesn't-run benches. Covered.
- **Spike discipline:** isolated crates + `FINDINGS.md` + highest-risk gate first (macOS C-build / manual-driving / lockdown) + honest evidence levels (no compile→runtime/GPU promotion; design-vs-measured separated in Task 5); frozen lockfiles. Covered.
- **Note on code blocks:** because a spike's purpose is API discovery, some Rust binding snippets are representative with an explicit "confirm against 0.x docs" instruction; all commands, JS payloads/fixtures, pass/fail thresholds, expected error shapes, and FINDINGS rows are exact. Intentional for a spike plan, not a placeholder.
