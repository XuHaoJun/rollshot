# Automation Frontend and Runtime Handoff

**Completed subproject:** Parent §12, Subproject 3
**Next subproject:** Subproject 4 — Bounded Agent Core

## Delivered crates

- `rollshot-automation`
- `rollshot-automation-rquickjs`

## Locked dependencies

- oxc `=0.137.0`
- rquickjs `=0.12.0`

## Installed schema versions

- Language Schema: 1
- Workflow IR Schema: 1
- Capability API: 1
- Output Schema: 1

## Public integration sequence

1. Call `validate_source(source, validation_limits)`.
2. Present `workflow_ir`, `semantic_summary`, and `semantic_diff`.
3. Dry-run with `execute_to_proposal`.
4. Keep the returned `EditProposal` transient until human review.
5. Apply only reviewed operations through `rollshot_edit_proposal::lower` and `ImageDocument::apply_batch`.

## Subproject 4 handoff

The bounded agent core owns `replace_automation_source`, `validate_automation`,
`dry_run_automation`, and `submit_for_review`. It supplies `ProposalContext`,
`ExecutionPolicy`, cancellation, and a prepared `AutomationHost`; it must not
expose rquickjs types.

## Subproject 5 handoff

Persist canonical source, Workflow IR, all four schema versions, capability
manifest, static cost, validation limits, validation summary, and immutable
revision provenance.
Do not persist oxc ASTs, runtime contexts, raw OCR, or raw host results.

## Subproject 6 handoff

Add default feature `smart-redaction`; hide every related UI/dependency when
disabled. Render source diagnostics, semantic summary/diff, capability changes,
static costs, execution metrics, and proposal candidates.

## Deferred real capability adapters

Real OCR, layout, region-feature, and template-match implementations remain
unimplemented. Production adapters must prepare bounded results outside the
QuickJS callback and keep host callbacks below 1 ms.

## Remaining risks

- QuickJS interrupts do not pre-empt a blocking Rust callback.
- QuickJS memory limits exclude Rust allocations; host allocation limits are separate.
- Parser/runtime upgrades require all frontend and adversarial suites.
- Execution revalidates persisted source against its recorded limits and
  semantic artifact before creating a runtime.

## Verification evidence

**Platform:** Linux 6.8.0-124-generic x86_64
**Compiler:** rustc 1.96.0 (ac68fa20 2026-05-25)
**Base commit:** 830e00a
**Evidence state:** post-review working tree

### Crate-level tests

```
$ rtk cargo test -p rollshot-edit-proposal
cargo test: 14 passed (2 suites, 0.00s)

$ rtk cargo test -p rollshot-automation
cargo test: 37 passed (5 suites, 0.00s)

$ rtk cargo test -p rollshot-automation-rquickjs
cargo test: 24 passed (5 suites, 0.03s)
```

### Workspace formatting and lint

```
$ rtk cargo fmt --check
(no output — PASS)

$ rtk cargo clippy --workspace --all-targets -- -D warnings
cargo clippy: No issues found

$ rtk cargo test
cargo test: 1001 passed, 5 ignored (52 suites, 24.63s)
```

### Dependency pin verification

```
$ rtk cargo tree -p rollshot-automation
oxc_allocator v0.137.0, oxc_ast v0.137.0, oxc_parser v0.137.0 — all exact

$ rtk cargo tree -p rollshot-automation-rquickjs
rquickjs v0.12.0 — exact

$ rtk cargo tree -p rollshot-app --no-default-features
No rollshot-automation, oxc, rquickjs, or rig present — feature isolation confirmed
```

### macOS compile verification

Not executed on this run — Linux runner only. A macOS compile check
(`cargo test --no-run`) should be performed on a macOS runner before the
feature branch merges to main.
