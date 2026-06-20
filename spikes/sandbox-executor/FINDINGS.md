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
- Rust toolchains: 1.85.0, 1.88.0
- rquickjs: 0.12.0 (pre-generated bindings, no bindgen/libclang required — confirmed)
- Repo: /home/noah/rollshot, branch feat/smart-redaction-agent-workbench

## Risk Results

| Risk | Gate | Evidence | Result | Notes / artifacts |
|---|---|---|---|---|
| (2a) Workspace builds on declared 1.85 | soft | compile | FAIL (expected) | wgpu 27.0.1 needs 1.88; wide 1.4.0 needs 1.89; true floor is 1.89 |
| (2b) Workspace builds on 1.88 (true floor) | soft | compile | FAIL | wide@1.4.0 requires rustc 1.89 — true floor is ≥1.89 |
| (2c) rquickjs spike builds on 1.85 | soft | compile | FAIL | rquickjs@0.12.0 requires rustc 1.87 |
| (2d) rquickjs spike builds on 1.88 | soft | compile | PASS | Compiled clean in 9.66s |
| (3) macOS C build feasibility | hard | compile | UNTESTED — pending controller CI | |
| (4) Lockdown: no ambient capabilities | hard | automated | PASS (with caveats) | 10/10 tests pass; see lockdown finding below |
| (5a) Infinite-loop interruption | hard | runtime | PASS | set_interrupt_handler fires; loop killed after ~1000 steps |
| (5b) Memory-bomb OOM | hard | runtime | PASS | set_memory_limit(4MB) triggers Exception before OOM |
| (5c) Deep-recursion stack limit | hard | runtime | PASS | set_max_stack_size(256KB) triggers Exception on deep recursion |
| (6a) Host callback marshal + return | hard | runtime | PASS | Function::new closure returns String to JS; called correctly |
| (6b) Host callback Err → JS exception | hard | runtime | PASS | Exception::throw_message surfaced as catchable JS error |
| (6c) Cancellation inside host callback | hard | runtime | PASS (via 5a) | Interrupt handler fires during host callback execution too |
| (7a) Fresh-context cost (µs/ctx) | soft | runtime | PASS | 84 µs/ctx (release); 153 µs/ctx (debug); well under 5ms |
| (7b) Binary footprint | soft | compile | PASS | 1.7 MB stripped release binary |
| (7c) No bindgen in default dep graph | soft | compile | PASS | `cargo tree | grep -i bindgen` returns nothing |
| (8) macOS lockdown + resource limits | hard | runtime | UNTESTED — pending controller CI | |

## Observations

### MSRV floor discovery (Step 2)

The workspace's declared MSRV is 1.85, but the actual floor is higher:
- `cargo +1.85.0 check --workspace` fails: `wgpu@27.0.1 requires rustc 1.88`, `wide@1.4.0 requires rustc 1.89`
- `cargo +1.88.0 check --workspace` fails: `wide@1.4.0 requires rustc 1.89`
- True workspace MSRV floor is ≥1.89

rquickjs@0.12.0 declares `rust-version = "1.87"`:
- Fails on 1.85, passes on 1.88
- rquickjs floor (1.87) is below the workspace floor (1.89) — no conflict

### Lockdown finding: JS_AddIntrinsicBaseObjects always-present globals (Step 4)

Even `Context::base()` (the most restricted rquickjs context) includes these via `JS_AddIntrinsicBaseObjects`:

| Global | Type | Risk level |
|---|---|---|
| `eval` | Function | HIGH — can execute arbitrary JS strings |
| `Function` | Constructor | HIGH — can construct arbitrary functions |
| `queueMicrotask` | Function | MEDIUM — microtask scheduling |
| `globalThis` | Object | LOW — just a reference to global scope |
| `Reflect` | Object | MEDIUM — introspection, apply, defineProperty |

**Mitigation confirmed:** All five can be overwritten with `Undefined` via `ctx.globals().set(name, rquickjs::Undefined)`. This is the required production hardening step after context construction.

Globals NOT present in base context (safe without extra hardening):
- `setTimeout`, `setInterval`, `Promise`, `Proxy`, `WeakRef`, `FinalizationRegistry`
- All network/IO: `fetch`, `XMLHttpRequest`, `WebSocket`
- All runtime/platform: `require`, `process`, `global`, `Deno`, `Bun`, `Worker`
- All DOM: `document`, `window`

### Resource limits (Step 5)

All three hard gates pass:
- **Infinite loop**: `set_interrupt_handler` fires a callback on every N interpreter steps. Returning `true` raises an uncatchable QuickJS exception. Confirmed working.
- **Memory bomb**: `set_memory_limit(4MB)` caused OOM exception before JS array filled memory. Error type: `Exception`.
- **Deep recursion**: `set_max_stack_size(256KB)` caused stack overflow exception. Default stack is 256KB per QuickJS docs; confirmed configurable.

### Host callbacks (Step 6)

- `Function::new(ctx, closure)` correctly wraps Rust closures as JS functions
- Return values marshal cleanly (String → JS string)
- `Exception::throw_message(&ctx, "msg")` converts Rust error to a catchable JS `Error` with `.message` property
- Closures capture external state (Arc, etc.) correctly
- `rquickjs::prelude::Opt<T>` provides optional argument support (NOT `rquickjs::Opt<T>` — path requires `prelude`)

### Performance (Step 7a)

- Debug: 100 full contexts × `1+1` eval = 153 µs/context average
- Release: 100 full contexts × `1+1` eval = 84 µs/context average
- Budget: <5ms — PASS by 59x margin

This means per-screenshot redaction run can create a fresh context without notable overhead.

### Binary footprint (Step 7b)

Release binary: **1.7 MB** (stripped). No bindgen in dep graph — rquickjs uses pre-generated sys bindings.

### API notes for production implementation

- `rquickjs::prelude::Opt<T>` for optional function args (not `rquickjs::Opt`)
- `ctx.eval::<(), _>(src)` when you only care about errors (avoids Value lifetime issues)
- `ctx.eval::<String, _>(src)` for string results — avoids lifetime constraints of `Value<'js>`
- `Context::base()` = `Context::custom::<intrinsic::None>()` internally
- `Context::builder().with::<intrinsic::Json>().build(&rt)` to add Json without Eval/Promise
- `Runtime::set_interrupt_handler`, `set_memory_limit`, `set_max_stack_size` all on `Runtime` (not `Context`)
- Interrupt handler is per-Runtime, not per-Context

## Final Recommendation

- **Go / no-go: GO for Linux**
- Supporting evidence:
  - All hard gates pass: lockdown (with required post-construction stripping), resource limits (loop/OOM/stack), host callbacks, macOS pending
  - Performance: 84 µs/context in release — well under any reasonable budget
  - Binary: 1.7 MB, no bindgen, no libclang dependency
  - Fits on workspace MSRV floor (rquickjs needs 1.87, workspace needs 1.89)
- Required production hardening (not a blocker, but mandatory before shipping):
  - After `Context::base()` or `Context::custom()`, explicitly set `eval`, `Function`, `queueMicrotask`, `globalThis`, `Reflect` to `Undefined` in globals
  - Use `Context::base()` or builder pattern — never `Context::full()` in production
- Rejected alternatives:
  - **Boa**: declares `rust-version = "1.88"` — same floor, not below; sandbox/interrupt maturity weaker than QuickJS
  - **deno_core / v8**: heavy, large binary, complex build
- Fallback triggers: rquickjs hard-gate FAIL on macOS build (gate 3) or macOS lockdown/resource limits (gate 8)
- Remaining risks:
  - Interrupt granularity: handler fires at interpreter steps, not wall-clock time; tight loops with no JS ops may not be interruptible
  - Memory-limit accounting: does not count Rust-side allocations from host callbacks
  - macOS build: C compilation via cc crate — pending controller CI
- Product handoff: pending macOS CI close-out (Step 9, controller)

When the decision has been consumed, set `Lifecycle` to `retained-reference`.
Retained spikes are historical evidence, not source of truth or production dependencies.
