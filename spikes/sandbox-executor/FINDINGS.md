# rquickjs Sandbox Executor Feasibility Spike - Findings

## Status

- Lifecycle: active
- Decision owner: Task 2
- Started: 2026-06-20
- Last updated: 2026-06-20

## Decision

Is rquickjs 0.12.x a safe, bounded sandbox for validated redaction automation, and does it build at the workspace's real MSRV floor on Linux + macOS?

## Environment

- OS: Ubuntu Linux (headless)
- Rust toolchains: 1.85.0, 1.88.0, 1.89.0
- rquickjs: 0.12.0 (default features, pre-generated bindings, no bindgen/libclang required — confirmed)
- Repo: /home/noah/rollshot, branch feat/smart-redaction-agent-workbench

## Risk Results

| Risk | Gate | Evidence | Result | Notes / artifacts |
|---|---|---|---|---|
| (2a) Workspace builds on declared 1.85 | soft | compile | FAIL (expected) | wgpu 27.0.1 needs 1.88; wide 1.4.0 needs 1.89; true floor is 1.89 |
| (2b) Workspace builds on 1.88 (hypothesis) | soft | compile | FAIL | wide@1.4.0 requires rustc 1.89 — true floor is ≥1.89 |
| (2c) rquickjs spike builds on 1.85 | soft | compile | FAIL | rquickjs@0.12.0 requires rustc 1.87 |
| (2d) rquickjs spike builds on 1.89 (true floor) | soft | compile | PASS | `cargo +1.89.0 build` clean in 9.22s — rquickjs 1.87 ≤ workspace floor 1.89 |
| (3) macOS C build feasibility | hard | compile | PASS | Spike CI `Spikes (macos-14)` on commit 2b9e9ac (PR #60): `cargo build --all-targets` for spikes/sandbox-executor succeeded — quickjs C build via cc works on aarch64-apple-darwin |
| (4a) Lockdown: no ambient capabilities | hard | automated | PASS | 15/15 tests pass; see lockdown finding below |
| (4b) dynamic import does not resolve external module | hard | automated | PASS | Module::declare without loader fails; no external module loaded |
| (4c) prototype mutation isolated across runtimes | hard | automated | PASS | Mutation in runtime A invisible in fresh runtime B |
| (4d) prototype mutation across contexts (same runtime) | hard | automated | PASS (better than expected) | Each JS_NewContext gets its own prototype chain — no cross-context prototype leakage |
| (5a) Infinite-loop interruption | hard | runtime | PASS | set_interrupt_handler fires; loop killed after ~1000 steps |
| (5b) Memory-bomb OOM | hard | runtime | PASS | set_memory_limit(4MB) triggers Exception before OOM |
| (5c) Deep-recursion stack limit | hard | runtime | PASS | set_max_stack_size(256KB) triggers Exception on deep recursion |
| (6a) Host callback marshal + return | hard | runtime | PASS | Function::new closure returns String to JS; called correctly |
| (6b) Host callback Err → JS exception | hard | runtime | PASS | Exception::throw_message surfaced as catchable JS error |
| (6c) Cancellation inside host callback | hard | runtime | MITIGATED | Interrupt fires at JS opcode boundaries only; blocking Rust host call is NOT pre-empted. Demonstrated: host_completed=true, interrupt_fired=true (fires in JS loop after host returns). Risk: long-running host calls. Mitigation: keep host fns <1ms; use Rust-side timeout for longer ones. |
| (7a) Fresh-context cost (µs/ctx) | soft | runtime | PASS | 72–91 µs/ctx (debug, restricted context); well under 5ms |
| (7b) Binary footprint | soft | compile | PASS | 1.7 MB stripped release binary |
| (7c) No bindgen in default dep graph | soft | compile | PASS | `cargo tree | grep -i bindgen` → empty (default features only) |
| (8) macOS lockdown + resource limits | hard | runtime | PASS | Spike CI `Spikes (macos-14)` on commit 2b9e9ac (PR #60): `cargo test` (incl. lockdown 15/15) passed on macOS — CPU-only sandbox behavior parity confirmed; `Floor check (Rust 1.89, macos-14)` also PASS |

## Observations

### MSRV floor discovery (Step 2)

The workspace's declared MSRV is 1.85, but the actual floor is higher:
- `cargo +1.85.0 check --workspace` fails: `wgpu@27.0.1 requires rustc 1.88`, `wide@1.4.0 requires rustc 1.89`
- `cargo +1.88.0 check --workspace` fails: `wide@1.4.0 requires rustc 1.89`
- True workspace MSRV floor is ≥1.89

rquickjs@0.12.0 declares `rust-version = "1.87"`:
- Fails on 1.85, passes on 1.89 (and 1.88)
- rquickjs floor (1.87) is below the workspace floor (1.89) — no conflict
- Row (2d) updated from 1.88 to 1.89: `cargo +1.89.0 build` clean in 9.22s

### Default features only

The spike uses `rquickjs = "0.12"` (default features). The original `features = ["full"]` was removed. The `intrinsic::Eval` type (used for spike driver experiments) lives at `rquickjs::context::intrinsic::Eval` and is available with default features. `cargo tree | grep -i bindgen` returns empty.

### Lockdown finding: JS_AddIntrinsicBaseObjects always-present globals (Step 4)

Even `Context::base()` (the most restricted rquickjs context) includes these via `JS_AddIntrinsicBaseObjects`:

| Global | Type | Risk level | Override-verified |
|---|---|---|---|
| `eval` | Function | HIGH — can execute arbitrary JS strings | YES — set(Undefined) + re-read asserts undefined |
| `Function` | Constructor | HIGH — can construct arbitrary functions | YES — set(Undefined) + re-read asserts undefined |
| `queueMicrotask` | Function | MEDIUM — microtask scheduling | YES — set(Undefined) + re-read asserts undefined |
| `globalThis` | Object | LOW — just a reference to global scope | YES — set(Undefined) + re-read asserts undefined |
| `Reflect` | Object | MEDIUM — introspection, apply, defineProperty | YES — set(Undefined) + re-read asserts undefined |

**Mitigation confirmed for all five:** Each of the five globals now has a test that (a) asserts the global IS present, then (b) sets it to Undefined and re-reads to confirm it is now undefined. Previously only `eval` and `Function` had the full set-then-assert-undefined pattern. All five are now fully verified overridable.

Globals NOT present in base context (safe without extra hardening):
- `setTimeout`, `setInterval`, `Promise`, `Proxy`, `WeakRef`, `FinalizationRegistry`
- All network/IO: `fetch`, `XMLHttpRequest`, `WebSocket`
- All runtime/platform: `require`, `process`, `global`, `Deno`, `Bun`, `Worker`
- All DOM: `document`, `window`

### Dynamic import lockdown (Step 4b)

`import('x')` without a registered module loader fails at the module resolution step. Tested via `Module::declare()` at the Rust API level: compiling a module that imports from 'x' fails (or evaluation fails) when no loader is registered via `Runtime::set_loader()`. The production sandbox must NOT call `Runtime::set_loader()`.

Note: calling `import('x')` via `ctx.eval()` in script mode creates GC objects that cannot be cleanly freed without draining pending jobs. The `Module::declare` Rust API is used for the test instead, which is also the more production-relevant test surface (avoids the GC lifecycle pitfall).

### Prototype mutation isolation (Step 4c/4d)

**Cross-runtime isolation (4c):** Mutating `Object.prototype.__poisoned = true` in runtime A is invisible in a fresh runtime B. PASS — each runtime has its own GC heap.

**Cross-context, same runtime (4d) — BETTER THAN EXPECTED:** Mutating `Array.prototype.evil = 1` in context A (within runtime R), then creating a fresh context B within the same runtime — the mutation is NOT visible in context B. QuickJS creates a new global object and prototype chain per `JS_NewContext()` call, even within the same runtime. Each context is isolated at the prototype level even when sharing the same GC heap.

**Implication:** A fresh Context per user script (within the same Runtime) is sufficient for prototype isolation. A fresh Runtime provides stronger GC isolation (completely separate heap) but is not required for prototype safety alone.

### Resource limits (Step 5)

All three hard gates pass on restricted `Context::base() + intrinsic::Eval` (the production context footprint — NOT `Context::full`):
- **Infinite loop**: `set_interrupt_handler` fires a callback on every N interpreter steps. Returning `true` raises an uncatchable QuickJS exception.
- **Memory bomb**: `set_memory_limit(4MB)` caused OOM exception before JS array filled memory.
- **Deep recursion**: `set_max_stack_size(256KB)` caused stack overflow exception.

Note: all resource-limit experiments previously used `Context::full`. They now use `Context::builder().with::<intrinsic::Eval>().build(&rt)` — the minimal context needed for `ctx.eval()`. The `Eval` intrinsic is required only for the spike's Rust-side script execution. A production sandbox using pre-compiled bytecode can omit Eval from the runtime context entirely.

### Host callbacks (Step 6)

- `Function::new(ctx, closure)` correctly wraps Rust closures as JS functions
- Return values marshal cleanly (String → JS string)
- `Exception::throw_message(&ctx, "msg")` converts Rust error to a catchable JS `Error` with `.message` property
- Closures capture external state (Arc, etc.) correctly
- `rquickjs::prelude::Opt<T>` provides optional argument support (NOT `rquickjs::Opt<T>` — path requires `prelude`)

### 6c: Interrupt during host callback — MITIGATED (not PASS)

**Finding:** The QuickJS interrupt handler fires at JS opcode boundaries only. A Rust host function runs with no JS opcodes executing, so the interrupt counter cannot advance during the host call.

**Demonstrated:** JS evaluates `hostFn(); while(true){}`. The interrupt handler requires >1000 JS steps to fire. `hostFn()` (2ms sleep, sets `host_completed=true`) completes before any JS opcodes for the while loop. Result: `host_completed=true`, `interrupt_fired=true` (fires in the subsequent while loop).

**Risk:** A host function that blocks for a long time cannot be pre-empted by the QuickJS interrupt.

**Mitigation:** Keep host functions short (<1ms). For longer operations, use a Rust-side `tokio::time::timeout` or watchdog thread. Remaining risk, not a blocker.

Row (6c) previously claimed "PASS (via 5a)" which overstated. It is now correctly labeled MITIGATED with an honest note.

### Performance (Step 7a)

- Debug: 72–91 µs/context (100 restricted-context benchmark)
- Release: ~84 µs/context (original measurement; release benchmarks use restricted context)
- Budget: <5ms — PASS by ~55x margin

### Binary footprint (Step 7b)

Release binary: **1.7 MB** (stripped). No bindgen in dep graph — rquickjs uses pre-generated sys bindings.

### API notes for production implementation

- `rquickjs::context::intrinsic::Eval` to add Eval intrinsic to a custom context
- `rquickjs::prelude::Opt<T>` for optional function args (not `rquickjs::Opt`)
- `ctx.eval::<(), _>(src)` when you only care about errors (avoids Value lifetime issues)
- `ctx.eval::<String, _>(src)` for string results — avoids lifetime constraints of `Value<'js>`
- `Context::base()` = `Context::custom::<intrinsic::None>()` internally
- `Context::builder().with::<intrinsic::Json>().build(&rt)` to add Json without Eval/Promise
- `Runtime::set_interrupt_handler`, `set_memory_limit`, `set_max_stack_size` all on `Runtime` (not `Context`)
- Interrupt handler is per-Runtime, not per-Context
- Do NOT call `Runtime::set_loader()` in the production sandbox (prevents external module loading)
- Each `Context::base()` / `Context::builder().build()` call creates a fresh prototype chain, even within the same Runtime

### macOS verification (Step 3 + Step 8) — CI on commit 2b9e9ac (PR #60)

The corrected Spike CI (which scopes to this plan's spikes and excludes the unrelated `spikes/layershell-feasibility`) ran green on both runners:
- `Spikes (macos-14)`: `spikes/sandbox-executor` `cargo build --all-targets` + `cargo test` (lockdown 15/15) PASS → macOS quickjs C build (gate 3) and macOS CPU-runtime parity for lockdown + resource limits (gate 8) both confirmed.
- `Spikes (ubuntu-24.04)`: PASS.
- `Floor check (Rust 1.89, macos-14 + ubuntu-24.04)`: PASS — builds at the true workspace floor on both platforms.

(The earlier red run was the globbed loop hitting `layershell-feasibility`'s Wayland/glib system-lib needs, unrelated to this spike.)

## Final Recommendation

- **Go / no-go: GO (Linux + macOS confirmed)**
- Supporting evidence:
  - All hard gates pass on Linux AND macOS: lockdown (with required post-construction stripping), resource limits (loop/OOM/stack), host callbacks, macOS C-build + CPU-runtime parity (gates 3 & 8, CI 2b9e9ac)
  - Import lockdown: no module loader registered → no external module can be loaded (tested via Module::declare)
  - Prototype isolation: per-context prototype chains confirmed (better than expected)
  - All five base-intrinsic globals (eval, Function, queueMicrotask, globalThis, Reflect) confirmed overridable with set-then-assert pattern
  - Performance: ~84 µs/context in release — well under any reasonable budget
  - Binary: 1.7 MB, no bindgen, no libclang dependency
  - Fits on workspace MSRV floor (rquickjs needs 1.87, workspace needs 1.89) — confirmed at 1.89.0
  - Default features only: no `features = ["full"]` required
- Required production hardening (not a blocker, but mandatory before shipping):
  - After `Context::base()` or `Context::custom()`, explicitly set `eval`, `Function`, `queueMicrotask`, `globalThis`, `Reflect` to `Undefined` in globals
  - Use `Context::base()` or builder pattern — never `Context::full()` in production
  - Do NOT call `Runtime::set_loader()` — ensures import() cannot resolve external modules
- Rejected alternatives:
  - **Boa**: declares `rust-version = "1.88"` — same floor, not below; sandbox/interrupt maturity weaker than QuickJS
  - **deno_core / v8**: heavy, large binary, complex build
- Fallback triggers (none tripped): would have been rquickjs hard-gate FAIL on macOS build (gate 3) or macOS lockdown/resource limits (gate 8) — both PASSED on CI 2b9e9ac.
- Remaining risks (carry forward to the automation-frontend / executor subproject):
  - **Interrupt granularity**: handler fires at interpreter steps only; a blocking host call is NOT pre-empted (MITIGATED — keep host fns <1ms; Rust-side watchdog/timeout for longer ones)
  - **Memory-limit accounting**: `set_memory_limit` does not count Rust-side allocations made by host callbacks — host capabilities must bound their own allocations
  - **Production hardening is mandatory**: strip the 5 base-intrinsic globals; never `Context::full()`; never `Runtime::set_loader()`
- Product handoff: GO confirmed. The executor subproject should adopt rquickjs 0.12.x with the required hardening above; MSRV is a non-issue (1.87 ≤ workspace floor 1.89). Recommend correcting the stale `Cargo.toml` `rust-version = "1.85"` → `1.89` (tracked in Task 6 MSRV resolution).

When the decision has been consumed, set `Lifecycle` to `retained-reference`.
Retained spikes are historical evidence, not source of truth or production dependencies.
