# Task 6 Report: Typed Emergency Manifest and Model-Dispatch Budget

## Status: DONE

## Commit
`bb45a4e` — feat(agent): add emergency continuity manifest

## Changes

### continuity.rs
- Added `RunContinuityStageV1` enum with 4 variants: `Drafting`, `NeedsValidation`, `NeedsDryRun`, `ReadyToSubmit`
- Added `EvidenceContinuityV1` struct with `from_record()`, `is_validation()`, `is_dry_run()` helpers
- Added `BudgetContinuityV1` struct encoding Duration as `(secs, nanos)` and cost as IEEE-754 bits
- Added `ToolContinuitySnapshot` struct with privacy-safe tool context snapshot
- Added `RunContinuityManifestInputs` struct for build parameters
- Added `RunContinuityManifestV1` struct with `build()` method that:
  - Validates projection task/attempt/run/source/authority/skill references
  - Derives stage from current-generation evidence
  - Captures budget limits and committed usage
  - Rejects stale evidence (validation/dry-run without corresponding cache)
  - Rejects cancelled operations and non-finite cost
  - Caps canonical manifest at 64 KiB
  - Computes deterministic SHA-256 digest with domain separator
- Added `ContextRecoveryError` enum with 10 error variants
- Added `#[derive(Clone)]` to `ContinuityProjectionV1` for manifest ownership
- Added 12 new tests covering stage derivation, stale task/attempt/cancelled, evidence filtering, budget encoding, privacy redaction

### runtime.rs
- Added `Eq` and `serde::Serialize` derives to `EvidenceKind`
- Added `BudgetTracker::charge_model_dispatch()` method that:
  - Checks `used.model_calls + 1` against limit
  - Increments only committed `used.model_calls` (not turn accumulator)
  - Each provider dispatch consumes one budget unit
- Added 3 new tests: dispatch committed, turn isolation, limit enforcement

### tools.rs
- Added `ToolContext::continuity_state()` method that:
  - Locks draft, pending_review in fixed order
  - Snapshots current-generation evidence (filters stale)
  - Computes content_binding_digest from DocumentContentBinding
  - Returns privacy-safe `ToolContinuitySnapshot`
  - Contains no source, proposals, validated programs, metrics, capability handles, or review content

## Test Summary
- `rtk cargo test -p rollshot-agent continuity` — 42 passed
- `rtk cargo test -p rollshot-agent runtime` — 36 passed (3 new)
- `rtk cargo test -p rollshot-agent tools` — 59 passed
- `rtk cargo test -p rollshot-agent` — 448 passed (3 suites, 5.15s)
- Total new tests: 15 (12 continuity + 3 runtime)

## Verification Output
```
$ rtk cargo test -p rollshot-agent continuity
cargo test: 42 passed (2 suites, 406 filtered, 22 warnings, 0.00s)

$ rtk cargo test -p rollshot-agent runtime
cargo test: 36 passed, 412 filtered out (2 suites, 0.05s)

$ rtk cargo test -p rollshot-agent tools
cargo test: 59 passed, 389 filtered out (2 suites, 0.00s)

$ rtk cargo test -p rollshot-agent
cargo test: 448 passed (3 suites, 5.15s)
```

## Concerns
None. All existing tests pass. New types integrate cleanly with existing infrastructure.
