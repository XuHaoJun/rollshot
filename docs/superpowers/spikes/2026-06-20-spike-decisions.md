# Smart Redaction Agent Workbench — Spike Decision Record

**Date:** 2026-06-20
**Branch:** feat/smart-redaction-agent-workbench
**Status:** All four spikes GO. Decisions recorded below are locked for downstream subproject specs.

---

## 1. Sandbox Runtime

**Decision: GO — rquickjs 0.12.x**

All hard gates PASS on Linux and macOS (CI, commit 2b9e9ac, PR #60):

- Lockdown (15/15 tests): ambient-capability stripping confirmed for all five
  base-intrinsic globals present in `Context::base()`.
- Resource limits: `set_interrupt_handler` (infinite-loop kill), `set_memory_limit`
  (OOM before JS fills memory), `set_max_stack_size` (deep-recursion kill).
- Host callbacks: marshal/return and Err→JS exception both clean.
- Cancellation: interrupt fires; host-call pre-emption is MITIGATED (not full PASS).
- macOS C-build: quickjs C grammar builds on aarch64-apple-darwin.
- Binary: 1.7 MB stripped release, no bindgen, no libclang.
- Performance: ~84 µs/context release — well under any practical budget.

**Mandatory production hardening (must ship before release):**

After `Context::base()` or `Context::custom()`, immediately set these five globals to
`Undefined` and assert they read back as undefined:
- `eval` (HIGH risk — executes arbitrary JS strings)
- `Function` (HIGH risk — constructs arbitrary functions)
- `queueMicrotask` (MEDIUM)
- `globalThis` (LOW)
- `Reflect` (MEDIUM — introspection, apply, defineProperty)

Additional rules:
- Never use `Context::full()` in production.
- Never call `Runtime::set_loader()` — this prevents `import()` from resolving external modules.

**Remaining risks (carry forward to automation-frontend subproject):**

- Interrupt handler fires at JS opcode boundaries only; a blocking Rust host call is NOT
  pre-empted. Mitigation: keep host functions <1ms; use a Rust-side `tokio::time::timeout`
  or watchdog thread for longer ones.
- `set_memory_limit` does not account for Rust-side allocations made by host callbacks;
  host capabilities must bound their own heap use.

**Rejected alternatives:** Boa (same MSRV floor 1.88, weaker sandbox/interrupt maturity),
deno_core/v8 (heavy binary, complex build).

---

## 2. Parser

**Decision: RECOMMENDED — tree-sitter 0.26.x**

The final parser lock happens in the automation-frontend subproject spec. This section
records the decision reasoning and the coupled MSRV constraint.

### Shortlist (from spike)

| Candidate | MSRV imposed | Builds on 1.89 | §5.2 coverage | Span quality | Binary | Dep count |
|-----------|-------------|----------------|---------------|--------------|--------|-----------|
| oxc 0.137.0 | **1.94.0** | NO | YES (9/9) | Byte-exact | 1.5 MiB | ~130 |
| tree-sitter 0.26.9 | 1.77 | YES | YES (9/9) | Line:col (CST) | 1.0 MiB | ~30 |
| swc 41.1.1 | ~1.86 (empirical) | YES | YES (9/9) | Poor (global accum.) | 2.8 MiB | ~208 |
| boa 0.21.1 | 1.88.0 | YES | YES (9/9) | Partial (stmt=0:0) | 3.6 MiB | ~126 |

All four candidates passed all §5.2 fixtures (9/9) and IR extraction. The differentiators
are MSRV and span quality.

### Recommendation: tree-sitter

tree-sitter holds the 1.89 workspace floor (MSRV 1.77), is the smallest footprint
(~30 deps, 1.0 MiB), and all gates passed including macOS C-build (CI, commit dbf75fe,
PR #60, `Spikes (macos-14)` + `Floor check (1.89, macos-14)` both PASS). Traversal uses
CST `node.kind()` string matching rather than typed enum arms — more verbose than oxc but
functional and well-understood. Production should use `child_by_field_name("kind")` for
`var` detection rather than text-prefix matching.

oxc is the only candidate with byte-exact spans and typed arena AST; it is the better
ergonomic choice IF the workspace floor can rise to 1.94. This is the one remaining
coupled decision for the automation-frontend subproject: choose tree-sitter and the floor
stays at 1.89; choose oxc and the floor must be explicitly raised to 1.94.

### Fallback chain (if tree-sitter is rejected)

1. oxc — raise floor to 1.94 explicitly.
2. swc — floor stays 1.89; accept SourceMap overhead for user-facing spans.
3. boa — floor stays 1.89 (MSRV 1.88); accept 0:0 spans for statement nodes.

---

## 3. Agent / Provider

**Decision: GO — rig 0.39.x**

Hard gates PASS: manual `AgentRun` driving confirmed (3-turn sequence, no `agent.prompt()`
called), cancellation clean on both timeout-drop and `CancellationToken` paths.

Additional PASS: usage accounting, multimodal message construction, privacy-safe tracing
(AgentRun emits zero tracing events on the sans-IO scripted path), and `RollshotModel`
facade with runtime provider swap.

macOS parity: PASS (CI, commit b00c768, PR #60).

**Facade shape (product implementation target):**

```rust
trait RollshotModel: Send + Sync {
    fn name(&self) -> &'static str;
    async fn complete(
        &self,
        prompt: &Message,
        history: &[Message],
    ) -> Result<ModelTurn, RollshotError>;
}
```

**Dependency pin:** use `rig-core = "=0.39.0"` until MSRV stability is confirmed across
minor releases. rig-core has no published `rust-version` field; measured MSRV at 0.39.0
is 1.88 (below the workspace floor of 1.89 — no conflict).

**Remaining risk (OPEN — carry forward to agent-core subproject):**

Provider-specific structured tool behavior is UNTESTED. Whether Anthropic or OpenAI
providers correctly encode `ToolDefinition` JSON schemas and parse tool-call wire format
responses was not confirmed (user declined the optional API-key run during the spike).
The agent-core subproject must close this gap via a recorded fixture (spec §11.6) or a
live test.

**Rejected alternative:** hand-rolled provider trait + raw HTTP (~500–1500 LoC for request
serialisation, SSE streaming, retry logic, and error normalisation per provider). Wrapping
rig's `CompletionModel` behind `RollshotModel` costs ~50 LoC per provider and inherits
rig's normalisation for free.

---

## 4. Visual Diff

**Decision: GO — iced 0.14 overlay rendering approach**

CPU geometry cost is negligible at all tested candidate counts on both ordinary (1920×1080)
and tall (4000×12000) images:

| Operation | 1000 candidates | Gate |
|-----------|----------------|------|
| Frustum cull | 667–748 ns | <2 ms — PASS |
| Hit-test | 393–427 ns | <2 ms — PASS |
| Before/after diff | 1.76–1.97 µs | <2 ms — PASS |

iced 0.14 prototype compiles (proposed/accepted overlays, before/after toggle, `similar`-based
source-diff pane, Workflow IR semantic-summary pane). macOS compile PASS (CI, commit d57c8ce,
PR #60).

**Remaining risk:** GPU/interactive latency UNTESTED (headless host, no display). Risk is
assessed LOW given the CPU margin. Must be verified on a real display before shipping —
run the prototype at 500–1000 candidates, record GPU/interaction latency, and confirm <8 ms
scroll-frame overhead.

**Data model (design recommendation, not spike-tested):**

Use a transient `ProposedCandidate` wrapper held in session state, not a first-class
`Annotation` variant:

```rust
struct ProposedCandidate {
    bounds: Bounds,
    confidence: f32,
    label: String,
}
```

On accept, convert to `Annotation::OpaqueRedaction` via the existing commit path. This
keeps agent concerns out of `rollshot-image-document` (which is intentionally headless
and framework-neutral). Before/after toggle is a pure session-state flag — no document
mutation, no undo/redo participation.

---

## 5. MSRV Resolution

**Verified true workspace floor: 1.89**

The `Cargo.toml` declares `rust-version = "1.85"` (stale). The actual floor is 1.89,
driven by `wide@1.4.0` (requires 1.89) and `wgpu@27.0.1` (requires 1.88). This was
confirmed empirically by the sandbox-executor spike (Task 2):

- `cargo +1.85.0 check --workspace` → FAIL (`wgpu` needs 1.88, `wide` needs 1.89)
- `cargo +1.88.0 check --workspace` → FAIL (`wide` needs 1.89)
- `cargo +1.89.0 check --workspace` → PASS

**Action item:** Correct the stale `Cargo.toml` `rust-version` from `"1.85"` to `"1.89"`.
This is a hygiene fix that reflects the real floor regardless of the smart-redaction
feature. Tracked here as an explicit outcome of the spike MSRV cross-cut.

**New dep MSRV contributions vs. the 1.89 floor:**

| Dependency | Measured MSRV | Exceeds 1.89? |
|------------|--------------|---------------|
| rquickjs 0.12.x | 1.87 | NO — free |
| rig-core 0.39.0 | 1.88 | NO — free |
| tree-sitter 0.26.9 | 1.77 | NO — free |
| swc_ecma_parser 41.1.1 | ~1.86 (empirical) | NO — free |
| boa 0.21.1 | 1.88.0 | NO — free |
| **oxc 0.137.0** | **1.94.0** | **YES — would raise floor** |

**The parser choice is the one remaining MSRV-coupled decision:**

- tree-sitter (recommended) → workspace floor stays at 1.89.
- oxc → workspace floor must be explicitly raised to 1.94. This must be a stated
  engineering decision in the automation-frontend subproject spec, not a silent
  side-effect.

**Frozen lockfile evidence:** the committed `Cargo.lock` files in each spike crate
(`spikes/sandbox-executor/Cargo.lock`, `spikes/js-frontend/Cargo.lock`,
`spikes/rig-agent/Cargo.lock`, `spikes/visual-diff/Cargo.lock`) are frozen as of the
decision date (2026-06-20) and must not be re-resolved. They are the evidentiary record
of exactly which dep versions and MSRV constraints were measured. Do not run
`cargo update` in the spike crates.

---

## 6. Downstream Locks

### Subproject 3: Automation Frontend / Runtime

The following choices are now fixed for subproject 3's spec:

- **Sandbox runtime:** rquickjs 0.12.x with mandatory hardening (strip 5 base-intrinsic
  globals; never `Context::full()`; never `Runtime::set_loader()`).
- **Parser:** tree-sitter 0.26.x (recommended) — OR oxc 0.137.x if the subproject spec
  explicitly accepts raising the workspace floor to 1.94.
- **Hardening rules:** see §1 above; the full versioned contract (restricted JS subset,
  interrupt policy, memory-limit scope) is owned by subproject 3's spec.

Carry-forward risks:
- Host-call interrupt granularity (MITIGATED, not resolved — keep host fns <1ms).
- Host-side Rust alloc not counted by `set_memory_limit` (host capabilities must bound
  their own heap).

### Subproject 4: Bounded Agent Core

The following choices are now fixed for subproject 4's spec:

- **Agent framework:** rig 0.39.x behind the `RollshotModel` facade.
- **Dependency pin:** `rig-core = "=0.39.0"` until MSRV stability is confirmed.
- **Drive loop:** manual `AgentRun` (next_step/model_response/tool_results) — never
  `agent.prompt()`.

Carry-forward risk (OPEN):
- Provider-specific structured tool behavior UNTESTED (Anthropic / OpenAI wire format for
  `ToolDefinition` schemas and tool-call responses). Subproject 4 must close this via a
  recorded fixture or live test before the agent-core spec is marked complete.

---

## 7. Parallel Track

**Subproject 2: ImageDocument Batch Transaction + Typed EditProposal Ops**

Subproject 2 is spike-independent and proceeds as its own spec in parallel. It requires
no dependency decisions from the spikes — `rollshot-image-document` is headless and
framework-neutral. Work on subproject 2 does not block on the parser choice or MSRV
resolution above.
