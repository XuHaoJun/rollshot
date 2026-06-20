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
- rquickjs: 0.12.x (pre-generated bindings, no bindgen/libclang required)
- Repo: /home/noah/rollshot, branch feat/smart-redaction-agent-workbench

## Risk Results

| Risk | Gate | Evidence | Result | Notes / artifacts |
|---|---|---|---|---|
| (2a) Workspace builds on declared 1.85 | soft | compile | UNTESTED | pending |
| (2b) Workspace builds on 1.88 (true floor) | soft | compile | UNTESTED | pending |
| (2c) rquickjs spike builds on 1.85 | soft | compile | UNTESTED | pending |
| (2d) rquickjs spike builds on 1.88 | soft | compile | UNTESTED | pending |
| (3) macOS C build feasibility | hard | compile | UNTESTED — pending controller CI | |
| (4) Lockdown: no ambient capabilities | hard | automated | UNTESTED | pending |
| (5a) Infinite-loop interruption | hard | runtime | UNTESTED | pending |
| (5b) Memory-bomb OOM | hard | runtime | UNTESTED | pending |
| (5c) Deep-recursion stack limit | hard | runtime | UNTESTED | pending |
| (6a) Host callback marshal + return | hard | runtime | UNTESTED | pending |
| (6b) Host callback Err → JS exception | hard | runtime | UNTESTED | pending |
| (6c) Cancellation inside host callback | hard | runtime | UNTESTED | pending |
| (7a) Fresh-context cost (µs/ctx) | soft | runtime | UNTESTED | pending |
| (7b) Binary footprint | soft | compile | UNTESTED | pending |
| (7c) No bindgen in default dep graph | soft | compile | UNTESTED | pending |
| (8) macOS lockdown + resource limits | hard | runtime | UNTESTED — pending controller CI | |

## Observations

(to be filled as experiments run)

## Final Recommendation

- Go / no-go: PENDING
- Supporting evidence: pending Steps 3-8
- Rejected alternatives: Boa (pure-Rust, gives AST synergy with Task 3, but `boa_engine` declares rust-version 1.88 — same floor, not below; "Boa to keep 1.85" is not a real option since 1.85 is already off the table given workspace's true floor >=1.88; also evaluate sandbox/interrupt maturity); deno_core / v8 (heavy, large binary)
- Fallback triggers: rquickjs hard-gate FAIL on macOS build, lockdown, or resource limits
- Remaining risks: interrupt granularity, memory-limit accounting accuracy
- Product handoff: pending close-out (Step 9, controller)

When the decision has been consumed, set Lifecycle to retained-reference.
Retained spikes are historical evidence, not source of truth or production dependencies.
