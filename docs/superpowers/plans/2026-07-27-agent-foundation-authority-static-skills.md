# Agent Foundation Authority and Static Skills Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add immutable run authority, independently enforced tool grants, one bounded static host skill catalog, and durable Smart Redaction skill provenance without changing Smart Redaction product behavior or UI.

**Architecture:** Product code resolves the bundled Smart Redaction package into an immutable `SkillUse`, constructs a privacy-safe `AuthoritySnapshot`, and persists their receipt on the active Product Task attempt before any provider or tool work. The provider-neutral runner composes a Rollshot-owned system envelope with the pinned skill body; `ToolRegistry` checks each tool's declared `RunOperation` requirements before entering the existing executor. Artifact promotion copies the same authority/skill receipt and binds it into a V2 run-config digest.

**Tech Stack:** Rust 2021 workspace, serde/serde_json, `toml` 1.1, SHA-256 (`sha2`), descriptor-relative Unix filesystem APIs (`rustix` 1.1), tokio, iced task integration, existing exact-CAS `TaskStore`.

## Global Constraints

- Governing spec: `docs/superpowers/specs/2026-07-27-agent-foundation-authority-static-skills-design.md`.
- Keep Smart Redaction author/improve behavior, provider facade, budgets, cancellation, validation, dry-run, proposal review, artifact staleness, and apply/reject semantics unchanged.
- No user-visible iced UI, visual-baseline change, marketplace, project discovery, model skill routing, remote provider, script, shell expansion, extension, hook, new disclosure, general sandbox, broker, job, retry, context, or audit platform.
- Catalog limits: 1,000 entries, 2 files/package, one directory level, 4 KiB manifest, 16 KiB body, 128 KiB accepted metadata, one main resource.
- Manifest V1 accepts only `schema_version`, `package_id`, `name`, `description`, optional `declared_version`, and `main = "SKILL.md"`; unknown fields fail.
- Durable provenance contains identifiers, versions, digests, operation labels, and bounded categories only—never skill bodies, ambient paths, pixels, OCR text, user messages, credentials, provider-native state, or unrestricted tool data.
- Product Task store and artifact/run-config schemas move to V2; V1 files remain readable and are not rewritten on startup.
- Every new runtime diagnostic uses stable explicit `rollshot::*` targets and structured privacy-safe fields.
- Do not alter the historical Slice 2 spec, plan, or Gate G1 decision record.
- Use `rtk` for every shell command.
- Stop rather than weaken the design if descriptor-relative no-follow loading cannot be implemented safely on both Linux and macOS, or if V1 pending reviews cannot remain restorable.

---

## File Structure

### New files

- `crates/rollshot-agent/src/authority.rs` — immutable authority values, canonical receipt/digest, input ceiling checks, and unit contracts.
- `crates/rollshot-agent/src/skills.rs` — manifest parser, bounded bundled/host catalog loader, descriptor-relative host reads, digesting, explicit invocation, redacted `Debug`, and tests.
- `crates/rollshot-agent/skills/smart-redaction/skill.toml` — bundled V1 package metadata.
- `crates/rollshot-agent/skills/smart-redaction/SKILL.md` — current reusable JavaScript guide, inspection loop, authoring loop, improve guidance, and examples.
- `docs/superpowers/spikes/2026-07-27-authority-static-skills-decision.md` — created only after implementation verification; Gate G2 evidence and residual risks.

### Modified files

- `Cargo.lock` — record the existing workspace dependencies as direct `rollshot-agent` dependencies.
- `crates/rollshot-agent/Cargo.toml` — add `toml`, `rustix`, and test-only `tempfile`.
- `crates/rollshot-agent/src/lib.rs` — export `authority` and `skills` modules.
- `crates/rollshot-agent/src/tools.rs` — declare per-tool requirements, enforce authority before dispatch, add typed denial, and retain the visual-annotation-only internal execution path.
- `crates/rollshot-agent/src/driver.rs` — accept authority/skill values, validate the disclosure ceiling, compose the pinned prompt, route authorized Smart Redaction calls, and retain visual annotation behavior.
- `crates/rollshot-agent/src/product_task.rs` — V2 run-contract receipt, attempt binding reducer, skill/authority artifact provenance, V2 config fingerprint, V1 compatibility, canonical/privacy tests.
- `crates/rollshot-app/src/result_workspace/workbench/task_store.rs` — accept schemas 1 and 2, preserve no-rewrite reconciliation, and test V1/V2 persistence.
- `crates/rollshot-app/src/result_workspace/workbench/run.rs` — bundled catalog invocation, capability-derived authority construction, persistence-before-execution, authorized registry composition, artifact V2 promotion, and integration tests.
- `crates/rollshot-app/src/result_workspace/workbench/eval/layer1.rs` — pass deterministic authority/skill fixtures to the provider runner.
- `crates/rollshot-app/src/result_workspace/workbench/eval/record.rs` — pass deterministic authority/skill fixtures to the cassette recorder.
- `crates/rollshot-app/src/result_workspace/mod.rs` — update Product Artifact fixture construction for required V2 receipts.

## Engineering Review Lock

This plan received one `plan-eng-review` pass in auto mode before implementation.

### Auto decisions

#### Auto decision D1 — Make the host root descriptor-relative too

Context: `O_NOFOLLOW` protects only the final pathname component; an ambient multi-component root open would leave an intermediate-component race.

ELI10: Checking only the package file is like locking a room while leaving the hallway doors movable. Walking every root component and keeping directory handles means every step is opened as a real directory, not a symlink.

Stakes if we pick wrong: a host package lookup could escape the allowlisted root during a rename/symlink race.

Recommendation: **D1A** because explicit descriptor ownership is the portable Linux/macOS design and matches the spec's fail-closed boundary.

Completeness: D1A=10/10, D1B=5/10.

Pros / cons:

- **D1A — component-wise no-follow root walk (recommended):** ✅ closes intermediate and final symlink substitution; ❌ adds a small Unix path walker and platform tests.
- **D1B — canonicalize then open:** ✅ less code; ❌ reopens by ambient path and leaves a time-of-check/time-of-use gap.

Net: spend a small amount of local filesystem code to preserve the central containment invariant.

#### Auto decision D2 — Keep wall-clock timing out of deterministic unit assertions

Context: the 1,000-entry test proves bounds, but a one-second CI assertion would mix correctness with host load.

ELI10: The test should prove the loader stops at the right size every time. A busy CI machine should not turn correct code red because another process used the CPU.

Stakes if we pick wrong: flaky timing failures obscure real containment and ordering regressions.

Recommendation: **D2A** because deterministic resource ceilings are the contract; observed duration belongs in Gate evidence.

Completeness: D2A=10/10, D2B=7/10.

Pros / cons:

- **D2A — deterministic scale test + recorded duration (recommended):** ✅ stable CI and measurable Gate evidence; ❌ no hard performance tripwire in the unit test.
- **D2B — assert under one second:** ✅ catches severe slowdown automatically; ❌ fails nondeterministically under contention.

Net: retain complete bounded-scale coverage without a flaky scheduler-dependent assertion.

#### Auto decision D3 — Model one invoked skill, not a speculative vector

Context: Gate G2 requires exactly one bundled Smart Redaction skill.

ELI10: A list suggests the system already supports combining many skills, ordering them, and resolving conflicts. It does not. A single field tells the truth and is easier to validate.

Stakes if we pick wrong: later code may assume unreviewed multi-skill precedence and provenance semantics.

Recommendation: **D3A** because YAGNI and explicit contracts beat a premature collection.

Completeness: D3A=10/10 for Slice 3, D3B=10/10 but broader than required.

Pros / cons:

- **D3A — one `SkillUseReceiptV1` (recommended):** ✅ exact current invariant and simpler canonical digest; ❌ a future multi-skill design needs a schema revision.
- **D3B — `Vec<SkillUseReceiptV1>` now:** ✅ avoids a future field migration; ❌ implies unsupported ordering/combination behavior.

Net: accept an honest future schema change instead of claiming capability early.

#### Auto decision D4 — Use validated identity newtypes at the skill boundary

Context: source authority, package, and resource identifiers cross catalog, invocation, persistence, and prompt composition.

ELI10: Plain strings make it easy to swap a package name and a source name by accident. Separate validated types let the compiler reject those mix-ups.

Stakes if we pick wrong: identity confusion can bind a digest or receipt to the wrong authority/package tuple.

Recommendation: **D4A** because these IDs are security boundaries, not display strings.

Completeness: D4A=10/10, D4B=7/10.

Pros / cons:

- **D4A — validated newtypes (recommended):** ✅ compiler-enforced identity and centralized limits; ❌ modest constructor/serde code.
- **D4B — raw `String` fields:** ✅ fewer types; ❌ repeated validation and cross-field mix-up risk.

Net: use boring newtypes where identity errors would undermine provenance.

#### Auto decision D5 — Preserve buildable commits, then perform one clean Product cutover

Context: changing exported runner/constructor signatures in early core tasks would break `rollshot-app` until later tasks.

ELI10: Foundation pieces can be added and tested without switching the running app immediately. Once every piece exists, one commit updates all callers and removes the old path, so no final shim remains.

Stakes if we pick wrong: intermediate commits do not compile, or temporary unguarded APIs accidentally ship.

Recommendation: **D5A** because each commit stays reviewable while the completed branch has a clean cutover.

Completeness: D5A=10/10, D5B=8/10.

Pros / cons:

- **D5A — additive core tasks + atomic Task 7 cutover (recommended):** ✅ buildable commits and no final alias; ❌ brief duplicate prompt/V1 constructors inside the branch.
- **D5B — break callers across several commits:** ✅ less temporary duplication; ❌ non-buildable revisions and harder review/bisect.

Net: temporary branch-local compatibility is cheaper than broken commits; Task 7 must delete it.

### What already exists

| Existing contract | Reuse decision |
|---|---|
| `ToolRegistry` typed registration, schema advertisement, serial execution, limits, and cancellation | Extend only at pre-dispatch admission; retain execution order and counters. |
| `AuthorizedModelInput` validated descriptors/bytes and redacted `Debug` | Inspect the existing public manifest/accessor; do not duplicate payload storage. |
| `RunCancellation`, `RunBudget`, provider-neutral `AgentRunner`, and typed terminals | Pass through unchanged; authority denial becomes one additional honest terminal path. |
| Restricted QuickJS plus capability manifest/host bridge | Keep as inner execution enforcement; do not build a new sandbox. |
| Smart Redaction validate → dry-run → submit-for-review flow | Move instructions only; keep tool and proposal behavior unchanged. |
| Slice 2 Product Task, exact-CAS TaskStore, artifact promotion, restore, and stale guards | Add one receipt/reducer and V2 schema; preserve CAS and review semantics. |
| Existing workbench payload consent and prepared vision capability state | Use as Product inputs to the immutable snapshot; do not add permission UI. |
| Workspace `toml`, `rustix`, `sha2`, serde, tokio | Reuse pinned workspace dependencies; add only direct crate declarations/test `tempfile`. |

### NOT in scope

- Multiple invoked skills, ordering, or conflict semantics — one bundled proof is sufficient.
- Model-visible list/read/search or implicit routing — host selection is exact and explicit.
- Project/user discovery and package installation — only bundled plus caller-allowlisted host roots.
- Remote/provider catalogs and environment handoff — no current workload requires them.
- Scripts, extensions, hooks, or skill-defined tools/grants — instruction bytes never execute.
- General filesystem/network/process/credential/capture/publish authority — the closed operation enum covers current Smart Redaction only.
- Live revocation, approval cache, broker, sandbox, or durable lease — the bounded foreground run uses an immutable snapshot.
- Attachment-delivery behavior changes — `FullScreenshot` remains a ceiling, not a new upload.
- UI controls or visual baselines — the same author/improve product path selects the bundled skill.
- Jobs, retries, context continuity, durable audit, launch video, and Phase 3 slices — each remains behind its umbrella gate.

### Test coverage map
```text
pure contracts          filesystem adversaries          Product integration
      |                           |                              |
      v                           v                              v
authority/tool tests --> catalog no-follow tests --> running receipt committed
      |                           |                              |
      `------------- agent crate green ------------------------> provider starts
                                                                  |
                                                                  v
                                                        tool admission enforced
                                                                  |
                                                                  v
                                                        V2 artifact promoted
                                                                  |
                                                                  v
                                                     stale/review regressions green
```


| Task / behavior | Unit | Integration | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| 1 / canonical authority, binding, disclosure, privacy | ✓ | — | — | no |
| 2 / declared requirements, denial-before-entry, serial stop | ✓ | — | — | no |
| 3 / manifest, bounds, precedence, no-follow, digest, immutable bytes | ✓ | filesystem fixture | — | no |
| 4 / bundled package, prompt split, examples | ✓ | provider-request regression | — | no |
| 5 / receipt reducer, V1/V2 canonical provenance | ✓ | — | — | no |
| 6 / exact-CAS V2 and V1 no-rewrite | ✓ | TaskStore fixture | — | no |
| 7 / persistence ordering, author/improve, provider/tool/artifact path | ✓ | synthetic provider + workbench | Smart Redaction test harness | no |
| 8 / affected crates, lint, privacy, independent review | ✓ | ✓ | focused full-flow suites | review verdict only |

All tests use fixed IDs/timestamps, synthetic providers, temporary directories, and existing fake vision/automation hosts. No real network, provider credential, GUI, screen capture, or sleep is required.

### Failure-mode matrix

| New path | Realistic failure | Test and handling | User-visible result |
|---|---|---|---|
| Authority construction/input ceiling | stale run/document or OCR-only input carries bytes | Task 1 mismatch tests; `AuthorityError` before stream/tool | bounded run failure, no provider/tool effect |
| Tool admission | advertised tool lacks one declared grant | Task 2 counter/no-mutation tests; `ToolError::AuthorityDenied` | honest fail-closed terminal |
| Host root open | symlink/rename/special file escapes package root | Task 3 descriptor/swap fixtures; typed containment/special error | required skill setup failure; optional package diagnostic |
| Catalog bounds | 1,001st package or aggregate metadata exceeds limit | Task 3 scale test; omission count/diagnostic | bundled required package remains exact or run fails |
| Invocation | expected digest differs from catalog snapshot | Task 3 mismatch test; `SkillError::DigestMismatch` | no substitution and no run |
| Run receipt CAS | competing writer or pre-commit I/O failure | Task 7 ordering/failpoint tests; existing CAS outcome mapping | store error, proposal suppressed |
| V1 load | old snapshot has no receipt | Task 5/6 literal V1 tests; `None` retained without rewrite | old pending review remains available under V1 semantics |
| V2 promotion | attempt and artifact receipts differ | Task 5/7 mismatch tests; promotion rejection | no ReadyForReview proposal |
| Prompt cutover | package missing/invalid or wrong authority | Task 4/7 exact bundled identity tests; no inline fallback | bounded setup failure |
| Privacy diagnostics | body/path/pixels leak through Debug/serde/error | Tasks 1/3/5/7 privacy scans; redacted Debug/bounded errors | no protected payload in durable/log surfaces |

No failure mode is untested, unhandled, and silent.

### Execution dependencies and parallelization

| Task | Modules touched | Depends on |
|---|---|---|
| 1 | `rollshot-agent/authority` | — |
| 2 | `rollshot-agent/tools` | 1 |
| 3 | `rollshot-agent/skills`, crate dependencies | 1 |
| 4 | `rollshot-agent/skills`, `rollshot-agent/driver` | 2, 3 |
| 5 | `rollshot-agent/product_task` | 1, 3 |
| 6 | `rollshot-app/result_workspace`, Product Task contracts | 5 |
| 7 | `rollshot-agent/driver`, `rollshot-app/result_workspace` | 1–6 |
| 8 | all affected modules and Gate evidence | 7 |

Sequential execution, no parallelization opportunity. Tasks 2–5 all touch the same `rollshot-agent` crate and Task 7 performs the required clean cross-crate cutover; parallel worktrees would increase merge and contract-drift risk.

### Review completion summary

- Step 0 Scope Challenge: accepted as one integrated slice; 5 new files, 11 modified files, 2 new modules, 8 tasks—below complexity triggers.
- Architecture Review: 2 issues resolved (root descriptor traversal; typed skill identities).
- Plan Structure + Code Quality: 2 issues resolved (buildable commit sequencing; singular skill receipt).
- Test Review: 1 gap resolved (removed flaky elapsed assertion while retaining measured Gate evidence).
- Performance Review: no blocking issue; accepted skill bodies are capped at 16,000 KiB across 1,000 maximum-size packages plus 128 KiB metadata, one run pins only one `Arc<str>`, prompt composition is once per bounded run, and no capture/stitch hot path changes.
- Unresolved decisions: 0.

Primary-source check: rustix `openat`/`OFlags::NOFOLLOW`, Linux `open(2)`/`openat2(2)`, and Apple `open(2)` confirm that no-follow applies to the final component. The plan therefore walks every root/package/file component descriptor-relatively rather than relying on a multi-component ambient open.

---

### Task 1: Immutable Authority Contract

**Files:**
- Create: `crates/rollshot-agent/src/authority.rs`
- Modify: `crates/rollshot-agent/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `authority.rs`

**Interfaces:**
- Consumes: `ProductTaskId`, `TaskAttemptId`, `RunId`, and `DocumentContentBinding` from `product_task`/`domain`; `AuthorizedModelInput` from `domain`.
- Produces:
  - `AuthoritySchemaVersion::V1`
  - `DisclosureCeiling::{OcrLayoutOnly, FullScreenshot}`
  - `PreparedCapability::{RegionFeatures, Ocr, Layout, TemplateMatch}`
  - `RunOperation::{ReadDraft, WriteDraft, InspectPreparedImage, ExecuteRestrictedAutomation, SubmitReviewCandidate, RequestUserInput}`
  - `AuthorityBinding::new(task_id, attempt_id, run_id, document_binding)`
  - `AuthoritySnapshot::new(binding, policy_revision, disclosure, existing_product_capture, prepared_capabilities, grants)`
  - `AuthoritySnapshot::authorize_tool(run_id, document_binding, required)`
  - `AuthoritySnapshot::validate_model_input(input)`
  - `AuthoritySnapshot::receipt(created_at_unix_ms)`
  - `AuthoritySnapshotReceiptV1` and `AuthorityError`

- [ ] **Step 1: Export the empty module and write compile-failing contract tests**

Add `pub mod authority;` to `lib.rs`. In `authority.rs`, write tests with fixed UUID-shaped IDs and a canonical empty document binding:

```rust
#[test]
fn snapshot_digest_is_canonical_and_order_independent() {
    let a = snapshot_with(
        [PreparedCapability::Ocr, PreparedCapability::RegionFeatures],
        [RunOperation::ReadDraft, RunOperation::WriteDraft],
    );
    let b = snapshot_with(
        [PreparedCapability::RegionFeatures, PreparedCapability::Ocr],
        [RunOperation::WriteDraft, RunOperation::ReadDraft],
    );
    assert_eq!(a.digest(), b.digest());
    assert_eq!(a.receipt(123).snapshot_digest, b.receipt(123).snapshot_digest);
}

#[test]
fn ocr_only_rejects_any_model_attachment() {
    let snapshot = snapshot_with_disclosure(DisclosureCeiling::OcrLayoutOnly);
    let input = png_input(vec![1, 2, 3, 4]);
    assert_eq!(
        snapshot.validate_model_input(&input),
        Err(AuthorityError::DisclosureExceeded {
            ceiling: DisclosureCeiling::OcrLayoutOnly,
            attachment_count: 1,
        })
    );
}

#[test]
fn full_screenshot_is_a_ceiling_not_a_requirement() {
    let snapshot = snapshot_with_disclosure(DisclosureCeiling::FullScreenshot);
    assert_eq!(snapshot.validate_model_input(&input_without_attachments()), Ok(()));
}

#[test]
fn authority_debug_and_receipt_exclude_private_content() {
    let snapshot = full_snapshot();
    let debug = format!("{snapshot:?}");
    let json = serde_json::to_string(&snapshot.receipt(123)).unwrap();
    for forbidden in ["api_key", "user_message", "skill body", "/home/"] {
        assert!(!debug.contains(forbidden));
        assert!(!json.contains(forbidden));
    }
}
```

Also cover invalid/empty policy revision, mismatched run ID, mismatched document binding, missing operation, unsupported schema deserialization, duplicate-free sorted receipt fields, and `existing_product_capture = false` rejecting `InspectPreparedImage` grants.

- [ ] **Step 2: Run the authority tests and verify the expected compile failure**

Run: `rtk cargo test -p rollshot-agent authority::tests --no-fail-fast`

Expected: FAIL because `authority` types and constructors do not exist.

- [ ] **Step 3: Implement closed authority values and canonical digesting**

Implement private fields and checked constructors. Use `BTreeSet` for capabilities/grants and a private canonical DTO; hash `b"rollshot-authority-v1\0"` followed by `serde_json::to_vec` of that DTO. Receipt timestamps are not part of `snapshot_digest`.

The core shape must be:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOperation {
    ReadDraft,
    WriteDraft,
    InspectPreparedImage,
    ExecuteRestrictedAutomation,
    SubmitReviewCandidate,
    RequestUserInput,
}

#[derive(Clone, PartialEq, Eq)]
pub struct AuthoritySnapshot {
    binding: AuthorityBinding,
    policy_revision: String,
    disclosure: DisclosureCeiling,
    existing_product_capture: bool,
    prepared_capabilities: BTreeSet<PreparedCapability>,
    grants: BTreeSet<RunOperation>,
    digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoritySnapshotReceiptV1 {
    pub schema_version: u32,
    pub task_id: String,
    pub attempt_id: u32,
    pub run_id: String,
    pub policy_revision: String,
    pub disclosure_ceiling: DisclosureCeiling,
    pub existing_product_capture: bool,
    pub document_binding_digest: String,
    pub prepared_capabilities: Vec<PreparedCapability>,
    pub granted_operations: Vec<RunOperation>,
    pub snapshot_digest: String,
    pub created_at_unix_ms: i64,
}
```

Implement a custom `Debug` for `AuthoritySnapshot` that prints identifiers/digest/counts but never Product input bytes. `validate_model_input` reads only manifest descriptors and attachment count/bytes; do not consume attachments or add a second payload copy.

- [ ] **Step 4: Run focused authority tests**

Run: `rtk cargo test -p rollshot-agent authority::tests --no-fail-fast`

Expected: PASS, including canonical order, disclosure ceiling, binding mismatch, and privacy tests.

- [ ] **Step 5: Run the complete agent crate tests**

Run: `rtk cargo test -p rollshot-agent`

Expected: PASS with no behavior change outside the new module.

- [ ] **Step 6: Commit the authority contract**

```bash
rtk git add crates/rollshot-agent/src/authority.rs crates/rollshot-agent/src/lib.rs
rtk git commit -m "feat(agent): add immutable run authority contract"
```


---

### Task 2: Independent Tool Authority Enforcement

**Files:**
- Modify: `crates/rollshot-agent/src/tools.rs`
- Test: existing inline tests in `tools.rs`

**Interfaces:**
- Consumes: `AuthoritySnapshot`, `RunOperation`, `ToolContext.run_id`, and `ToolContext.content_binding` from Task 1.
- Produces:
  - `Tool::required_operations(&self) -> &'static [RunOperation]`
  - `ToolRegistry::execute_authorized_calls(..., authority: &AuthoritySnapshot, tool_ctx: &ToolContext, ...)`
  - `ToolError::AuthorityDenied { tool: String, operation: RunOperation }`

- [ ] **Step 1: Write failing denial and no-entry tests**

Add a test tool with an atomic call counter and `required_operations() == &[RunOperation::WriteDraft]`:

```rust
#[tokio::test]
async fn advertised_registered_tool_without_grant_never_enters_tool_body() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new(ToolRegistryLimits::permissive());
    registry.register(Arc::new(CountingTool::new(
        calls.clone(),
        &[RunOperation::WriteDraft],
    ))).unwrap();
    assert!(registry.tool_names().contains(&"counting"));

    let result = registry.execute_authorized_calls(
        &[ToolCall { name: "counting".into(), arguments_json: json!({}) }],
        &RunCancellation::new(),
        &BTreeSet::new(),
        &snapshot_granting([RunOperation::ReadDraft]),
        &test_context("source"),
    ).await;

    assert!(matches!(
        &result[0],
        Err(ToolError::AuthorityDenied { operation: RunOperation::WriteDraft, .. })
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
```

Add tests for one missing member of a multi-operation requirement, stale run/document binding, denial stopping later calls, cancellation winning before authority/tool entry, and denial leaving draft/evidence/review state unchanged.

- [ ] **Step 2: Run focused tests and verify failure**

Run: `rtk cargo test -p rollshot-agent tools::tests::advertised_registered_tool_without_grant_never_enters_tool_body -- --exact`

Expected: FAIL because the trait has no authority declaration and registry execution accepts no snapshot.

- [ ] **Step 3: Add declarations to every Smart Redaction tool**

Use static slices; do not allocate per call. Required mappings:

```text
GetContextSummaryTool      -> ReadDraft
ReadCurrentSourceTool      -> ReadDraft
ReplaceSourceTool          -> WriteDraft
EditSourceTool             -> ReadDraft + WriteDraft
ValidateSourceTool         -> ReadDraft
InspectImageContextTool    -> InspectPreparedImage
RegionFeaturesTool         -> InspectPreparedImage
OcrTool                    -> InspectPreparedImage
LayoutTool                 -> InspectPreparedImage
DryRunTool                 -> ReadDraft + InspectPreparedImage + ExecuteRestrictedAutomation
SubmitForReviewTool        -> ReadDraft + SubmitReviewCandidate
RequestUserInputTool       -> RequestUserInput
```

Test-only tools declare the narrow operation their test snapshot grants. The visual annotation stub remains on the explicitly named crate-private ephemeral path; it must not be exposed as a way for Smart Redaction callers to bypass authority.

- [ ] **Step 4: Enforce before counters, argument serialization, and tool entry**

In `execute_single`, verify `authority.authorize_tool(&tool_ctx.run_id, &tool_ctx.content_binding, tool.required_operations())` immediately after cancellation and before incrementing the per-tool counter. Map the first missing operation deterministically to `ToolError::AuthorityDenied`.

Add `execute_authorized_calls` as the only authority-bearing entry point and share the existing serial loop internally. Keep the current crate-internal driver call path unchanged until the atomic Product cutover in Task 7, so this commit does not break `rollshot-app`. Do not expose an unguarded public method to external callers.

- [ ] **Step 5: Run registry and full agent tests**

Run:

```bash
rtk cargo test -p rollshot-agent tools::tests --no-fail-fast
rtk cargo test -p rollshot-agent
```

Expected: PASS. Existing cancellation, malformed JSON, byte limits, call limits, terminal stopping, provider contracts, and visual annotation tests remain green.

- [ ] **Step 6: Commit tool enforcement**

```bash
rtk git add crates/rollshot-agent/src/tools.rs
rtk git commit -m "feat(agent): add authoring tool authority enforcement"
```

---

### Task 3: Bounded Static Host Skill Catalog

**Files:**
- Modify: `Cargo.lock`
- Modify: `crates/rollshot-agent/Cargo.toml`
- Create: `crates/rollshot-agent/src/skills.rs`
- Modify: `crates/rollshot-agent/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `skills.rs`

**Interfaces:**
- Consumes: no Product or provider types; only bounded byte sources and host-owned roots.
- Produces:
  - `SkillCatalogLimits::v1()` with exact global limits
  - validated `SkillAuthorityId`, `SkillPackageId`, and `SkillResourceId` newtypes
  - `SkillSource::{Bundled, HostRoot(HostSkillRoot)}` where `HostSkillRoot` owns an already validated directory descriptor
  - `HostSkillRoot::open(source_id, path)` that walks every root component descriptor-relatively with no-follow semantics
  - `StaticSkillCatalog::build(sources, limits)`
  - `StaticSkillCatalog::invoke(&SkillInvocationRequest, resolved_at_unix_ms)`
  - `SkillInvocationKind::HostExplicit`
  - immutable `SkillUse` plus privacy-safe `SkillUseReceiptV1`
  - typed `SkillError`, `CatalogDiagnostic`, and `CatalogBuildReport { catalog, omitted_count, diagnostics }`

- [ ] **Step 1: Add parser/digest tests that fail before implementation**

Write tests for a valid in-memory bundled package, unknown manifest fields, unsupported schema, invalid package IDs, missing/oversize descriptions, non-`SKILL.md` main path, invalid UTF-8, 4 KiB/16 KiB boundary values, a third unexpected package file, malformed optional-package omission without catalog/grant expansion, and canonical digest changes:

```rust
#[test]
fn manifest_and_body_changes_change_domain_separated_digest() {
    let base = bundled_package("smart-redaction", "1", "body");
    let body_changed = bundled_package("smart-redaction", "1", "body changed");
    let version_changed = bundled_package("smart-redaction", "2", "body");
    assert_ne!(resolve(base).digest(), resolve(body_changed).digest());
    assert_ne!(resolve(base).digest(), resolve(version_changed).digest());
}

#[test]
fn invocation_digest_mismatch_never_substitutes_current_body() {
    let report = catalog_with_bundled("smart-redaction", "current body");
    let error = report.catalog.invoke(&SkillInvocationRequest {
        source_authority: SkillAuthorityId::parse("rollshot.bundled").unwrap(),
        package_id: SkillPackageId::parse("smart-redaction").unwrap(),
        expected_digest: Some("00".repeat(32)),
        invocation_kind: SkillInvocationKind::HostExplicit,
    }, 123).unwrap_err();
    assert!(matches!(error, SkillError::DigestMismatch { .. }));
}
```

- [ ] **Step 2: Add Unix host-root adversarial tests**

Using `tempfile`, create immediate package directories and test:

- package-directory symlink;
- `skill.toml` symlink;
- `SKILL.md` symlink;
- FIFO or Unix socket as either file;
- absolute, slash, backslash, `.`, and `..` package components;
- oversize streaming reads;
- backing-file replacement after catalog construction; and
- a deterministic test hook that replaces a path between enumeration and descriptor-relative open.

The immutable snapshot assertion is:

```rust
let use_before = catalog.invoke(&request_without_expected_digest(), 10).unwrap();
std::fs::write(body_path, "replacement").unwrap();
let use_after = catalog.invoke(&request_with_digest(use_before.digest()), 11).unwrap();
assert_eq!(use_before.body(), use_after.body());
assert_eq!(use_before.digest(), use_after.digest());
```

- [ ] **Step 3: Add deterministic precedence and 1,000-entry tests**

Construct one bundled duplicate and two ordered host-root duplicates. Assert bundled wins, then earlier host root wins when no bundled entry exists, all shadowed entries emit bounded collision diagnostics, final ordering is source tier/index/package ID, and no collision is silent.

Build exactly 1,000 valid entries and 1 extra. Assert accepted count 1,000, omission count 1, metadata bytes at or below 128 KiB, and deterministic digest/order across two builds. Record the focused test duration in Gate G2 evidence instead of asserting wall-clock time in CI.

- [ ] **Step 4: Run catalog tests and verify failure**

Run: `rtk cargo test -p rollshot-agent skills::tests --no-fail-fast`

Expected: FAIL because the module and dependencies do not exist.

- [ ] **Step 5: Add dependencies and implement strict manifest parsing**

Add to `rollshot-agent`:

```toml
toml = { workspace = true }
rustix = { workspace = true, features = ["fs"] }

[dev-dependencies]
tempfile = "3"
```

If Cargo rejects feature unification because workspace `rustix` already fixes features, keep the workspace declaration unchanged and use `rustix = { workspace = true }` in the crate.

Use `#[serde(deny_unknown_fields)]` for the private manifest DTO. Validate before allocation growth, reject invalid identifiers, and hash:

```text
"rollshot-skill-v1\0"
+ canonical manifest security fields
+ u64 big-endian body length
+ exact SKILL.md bytes
```

`SkillUse` owns immutable `Arc<str>` body bytes and has a custom redacted `Debug`. `SkillUseReceiptV1` excludes body and paths.

- [ ] **Step 6: Implement descriptor-relative no-follow host loading**

On Unix, enumerate only immediate directory entry names, validate each as one component, then open package directories and fixed filenames relative to parent descriptors with `rustix::fs::openat` and `OFlags::NOFOLLOW`; require directory/regular-file metadata from the opened descriptors. Read with a `limit + 1` ceiling so oversized files never cause unbounded allocation.

`HostSkillRoot::open` must normalize the configured absolute or relative root, reject `..`, and walk every directory component from an opened `/` or current-directory descriptor using `DIRECTORY | NOFOLLOW`; it returns an owned root descriptor. Catalog loading then opens each validated single-component package and fixed file relative to held parent descriptors. Do not canonicalize and reopen by ambient path, recurse, or accept extra main names. Return `SkillError::UnsupportedPlatform` for host-root loading on unsupported targets; bundled sources remain platform-independent.

- [ ] **Step 7: Run catalog, crate, and formatting checks**

Run:

```bash
rtk cargo test -p rollshot-agent skills::tests --no-fail-fast
rtk cargo test -p rollshot-agent
rtk cargo fmt --check
```

Expected: PASS. If the deterministic replacement test demonstrates any path-reopen escape, stop and use the `rollshot-run-spike` skill before changing the design.

- [ ] **Step 8: Commit the catalog**

```bash
rtk git add Cargo.lock crates/rollshot-agent/Cargo.toml crates/rollshot-agent/src/lib.rs crates/rollshot-agent/src/skills.rs
rtk git commit -m "feat(agent): add bounded static skill catalog"
```

---

### Task 4: Bundled Smart Redaction Skill and Prompt Composition

**Files:**
- Create: `crates/rollshot-agent/skills/smart-redaction/skill.toml`
- Create: `crates/rollshot-agent/skills/smart-redaction/SKILL.md`
- Modify: `crates/rollshot-agent/src/skills.rs`
- Modify: `crates/rollshot-agent/src/driver.rs`
- Test: existing inline driver tests and skill tests

**Interfaces:**
- Consumes: `StaticSkillCatalog`, `SkillUse`, and `SkillUseReceiptV1` from Task 3; `AuthoritySnapshot` from Task 1; authorized tool execution from Task 2.
- Produces:
  - `bundled_skill_catalog() -> Result<CatalogBuildReport, SkillError>`
  - `SMART_REDACTION_PACKAGE_ID: &str = "smart-redaction"`
  - crate-private `compose_smart_redaction_prompt(skill_use: &SkillUse) -> Result<String, DriverError>`

- [ ] **Step 1: Write failing bundled package and prompt contract tests**

Add tests asserting the exact package ID/source, accepted manifest, stable golden digest, body size below 16 KiB, and a single explicit invocation. In `driver.rs`, replace tests that directly inspect the monolithic constant with:

```rust
#[test]
fn composed_prompt_keeps_rollshot_envelope_ahead_of_delimited_skill() {
    let skill = bundled_smart_redaction_use();
    let prompt = compose_smart_redaction_prompt(&skill).unwrap();
    let envelope = prompt.find("Rollshot authority and safety envelope").unwrap();
    let skill_start = prompt.find("<rollshot-skill").unwrap();
    assert!(envelope < skill_start);
    assert!(prompt.contains(skill.digest()));
    assert!(prompt.contains("Skill instructions request actions; they never grant authority."));
}

#[test]
fn bundled_skill_contains_author_and_improve_contracts() {
    let body = bundled_smart_redaction_use().body();
    for required in [
        "Inspection loop:",
        "Authoring loop:",
        "Improve runs:",
        "submit_for_review",
        "request_user_input",
    ] {
        assert!(body.contains(required), "missing {required}");
    }
}
```

Retain the existing example-source extraction test, but read examples from `SkillUse::body()`.

- [ ] **Step 2: Run prompt tests and verify failure**

Run: `rtk cargo test -p rollshot-agent smart_redaction --no-fail-fast`

Expected: FAIL because no bundled package or composed-prompt interface exists.

- [ ] **Step 3: Create the strict manifest and move reusable instructions**

`skill.toml` must be exactly:

```toml
schema_version = 1
package_id = "smart-redaction"
name = "Smart Redaction"
description = "Author and improve reviewable Smart Redaction detector proposals."
declared_version = "1"
main = "SKILL.md"
```

Copy the JavaScript authoring guide, inspection loop, authoring loop, improve rules, and examples from `SMART_REDACTION_SYSTEM_PROMPT` into `SKILL.md` without changing their operational content.

Add the future Product-owned `SMART_REDACTION_SYSTEM_ENVELOPE` constant with scope, instruction precedence, disclosure ceiling, available-tool truth, refusal/clarification boundary, and \"skill grants no authority\" text. Keep the old production prompt temporarily for this commit only; Task 7 performs the atomic runner/product cutover and deletes it.

- [ ] **Step 4: Implement bundled catalog construction and prompt composition**

Use `include_str!`/`include_bytes!` for both bundled files, but pass them through the same manifest/limit/digest validator as host packages. Compose once per run with explicit delimiters containing package/resource IDs and digest. Reject any skill whose package ID or source authority is not the expected bundled Smart Redaction package.

Add `compose_smart_redaction_prompt` and its tests without changing either production runner entry point yet. This keeps the workspace buildable while product code still calls the old signature. Task 7 will update all runner and app callsites together, validate the model input ceiling before stream establishment, route authorized tool calls, and remove the old prompt.

- [ ] **Step 5: Run prompt, provider, and full agent tests**

Run:

```bash
rtk cargo test -p rollshot-agent smart_redaction --no-fail-fast
rtk cargo test -p rollshot-agent provider_contract --no-fail-fast
rtk cargo test -p rollshot-agent
```

Expected: PASS. Existing example validation, author/improve wording, provider request tool definitions, provider failure, cancellation, and terminal behavior remain green.

- [ ] **Step 6: Commit the bundled skill package**

```bash
rtk git add crates/rollshot-agent/skills/smart-redaction/skill.toml crates/rollshot-agent/skills/smart-redaction/SKILL.md crates/rollshot-agent/src/skills.rs crates/rollshot-agent/src/driver.rs
rtk git commit -m "feat(agent): add bundled Smart Redaction skill"
```

The old production prompt remains only as a short-lived sequencing bridge until Task 7. It is not a fallback in the completed slice.

---

### Task 5: Product Task V2 Run-Contract and Artifact Provenance

**Files:**
- Modify: `crates/rollshot-agent/src/product_task.rs`
- Test: inline Product Task contract tests

**Interfaces:**
- Consumes: `AuthoritySnapshotReceiptV1` from Task 1 and `SkillUseReceiptV1` from Task 3.
- Produces:
  - `RunContractReceiptV1 { authority, skill_use, bound_at_unix_ms }`
  - `ProductTaskSnapshot::bind_run_contract(run_id, receipt, now)`
  - `TaskAttempt::run_contract() -> Option<&RunContractReceiptV1>`
  - `ProductArtifactMetadata::new_v2(..., run_contract: RunContractReceiptV1, ...)`
  - `RunConfigFingerprintV2` containing one authority digest and one exact skill-use receipt
  - explicit V2 constructors with V1-compatible deserialization

- [ ] **Step 1: Write failing reducer and migration tests**

Add tests for exact-once binding while `Running`, wrong run, stale timestamp, changed authority/task/document binding, second conflicting receipt, and identical idempotent retry behavior. Required assertions:

```rust
#[test]
fn run_contract_binds_once_to_active_attempt_before_promotion() {
    let running = running_task_fixture();
    let receipt = run_contract_fixture(&running);
    let bound = running.bind_run_contract(run_id_fixture(), receipt.clone(), 20).unwrap();
    assert_eq!(bound.attempts().last().unwrap().run_contract(), Some(&receipt));
    assert_eq!(bound.snapshot_revision(), running.snapshot_revision() + 1);

    let conflict = run_contract_with_skill_digest(&running, "ff".repeat(32));
    assert!(matches!(
        bound.bind_run_contract(run_id_fixture(), conflict, 21),
        Err(TaskContractError::RunContractConflict)
    ));
}

#[test]
fn promotion_requires_and_copies_exact_run_contract() {
    let bound = running_with_contract_fixture();
    let metadata = metadata_fixture(bound.active_run_contract().unwrap().clone());
    let ready = bound.record_ready_for_review(metadata, payload_fixture(), None, 30).unwrap();
    assert_eq!(
        ready.artifact_metadata().unwrap().run_contract(),
        bound.active_run_contract()
    );
}
```

Add raw V1 JSON without new fields and assert it deserializes with `run_contract == None`, retains schema version 1, and is not automatically relabeled. Add V2 canonical golden vectors and privacy scans.

- [ ] **Step 2: Run Product Task tests and verify failure**

Run: `rtk cargo test -p rollshot-agent product_task::tests --no-fail-fast`

Expected: FAIL because V2 receipt/reducer/fingerprint fields do not exist.

- [ ] **Step 3: Implement V1-compatible TaskAttempt receipt binding**

Add `#[serde(default)] run_contract: Option<RunContractReceiptV1>` to `TaskAttempt`. Keep `TaskAttempt::new` unchanged so existing Created/Running construction remains simple. Add private validation that receipt task/attempt/run/document/digest values match the snapshot and active attempt.

`bind_run_contract` rules:

- only `Running`;
- matching active run;
- monotonic timestamp;
- no terminal attempt;
- missing receipt binds and increments snapshot revision once;
- byte-for-byte identical receipt retry returns an unchanged clone;
- any different existing receipt is `RunContractConflict`.

- [ ] **Step 4: Require provenance for new V2 promotion**

Add explicit `ProductTaskSnapshot::new_v2` and `ProductArtifactMetadata::new_v2` constructors without changing existing app callsites in this commit. V2 promotion requires the active run contract; V1 deserialization and existing V1 constructors remain temporarily available until the atomic app cutover in Task 7. `record_ready_for_review` rejects missing/mismatched receipts for schema 2 while preserving historical schema 1 artifacts.

Introduce `RunConfigFingerprintV2` rather than silently changing V1 canonical bytes:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunConfigFingerprintV2 {
    pub provider: String,
    pub model: String,
    pub payload_mode: PayloadMode,
    pub run_kind: String,
    pub budget_dimensions: BTreeMap<String, u64>,
    pub authority_snapshot_digest: String,
    pub skill_use: SkillUseReceiptV1,
}
```

Include the single `skill_use` directly and domain-separate V2 digest bytes. Supporting multiple invoked skills is deferred until a real workload requires it. Retain V1 helpers only for historical V1 validation; do not use them for new runs.

- [ ] **Step 5: Run Product Task and full agent tests**

Run:

```bash
rtk cargo test -p rollshot-agent product_task::tests --no-fail-fast
rtk cargo test -p rollshot-agent
```

Expected: PASS. Existing transition, staleness, canonicalization, finite-value, size-bound, and privacy tests remain green.

- [ ] **Step 6: Commit V2 provenance**

```bash
rtk git add crates/rollshot-agent/src/product_task.rs
rtk git commit -m "feat(agent): bind authority and skill provenance to tasks"
```

---

### Task 6: Exact-CAS V2 Persistence and V1 Compatibility

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/task_store.rs`
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs` test fixtures
- Test: existing TaskStore and ResultWorkspace tests

**Interfaces:**
- Consumes: store schema V2 and run-contract reducer from Task 5.
- Produces:
  - TaskStore acceptance of schema versions 1 and 2 only
  - no-rewrite V1 reconciliation
  - unchanged exact-CAS, temp/fsync/rename, permission, retention, and failpoint semantics

- [ ] **Step 1: Write failing V1/V2 persistence tests**

Add tests that write literal V1 JSON, call `load`/reconciliation, compare bytes before and after, and assert no receipt was synthesized. Add V2 create/load/CAS round trip with a bound run contract. Extend unsupported-schema coverage to version 3.

```rust
#[test]
fn startup_reads_v1_without_rewriting_or_synthesizing_provenance() {
    let (store, _dir) = store();
    let path = write_literal_v1_running_snapshot(&store);
    let before = std::fs::read(&path).unwrap();
    let loaded = store.load(&task_id_fixture()).unwrap().unwrap();
    assert_eq!(loaded.store_schema_version(), 1);
    assert!(loaded.attempts()[0].run_contract().is_none());
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn schema_three_fails_closed() {
    let error = load_snapshot_with_schema(3).unwrap_err();
    assert!(matches!(error, TaskStoreError::UnsupportedSchema { version: 3 }));
}
```

- [ ] **Step 2: Run store tests and verify failure**

Run: `rtk cargo test -p rollshot-app task_store --no-fail-fast`

Expected: FAIL because TaskStore currently rejects schema >1 and fixtures lack V2 receipts.

- [ ] **Step 3: Accept V1/V2 while preserving storage mechanics**

Change the upper bound from 1 to 2. Do not rewrite on `load`, scan, prune, or reconciliation. Update fixture metadata constructors with a valid run-contract receipt. Keep the existing 4 MiB snapshot limit, 0700/0600 permissions, lock, exact serialized-byte CAS, sibling temp, file fsync, rename, parent fsync classification, and failpoints unchanged.

- [ ] **Step 4: Run focused persistence and workspace tests**

Run:

```bash
rtk cargo test -p rollshot-app task_store --no-fail-fast
rtk cargo test -p rollshot-app product_task --no-fail-fast
```

Expected: PASS with V1 no-rewrite, V2 round trip, concurrent-writer, failpoint, permissions, pruning, and source reconciliation coverage.

- [ ] **Step 5: Commit persistence compatibility**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/task_store.rs crates/rollshot-app/src/result_workspace/mod.rs
rtk git commit -m "feat(app): persist agent run authority provenance"
```

---

### Task 7: Smart Redaction Product Wiring and Artifact Promotion

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/eval/layer1.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/eval/record.rs`
- Modify: `crates/rollshot-agent/src/driver.rs` only for integration corrections
- Test: workbench run tests, eval tests, agent integration tests

**Interfaces:**
- Consumes: all Tasks 1–6.
- Produces:
  - Product-owned `resolve_smart_redaction_skill()` helper
  - Product-owned `build_smart_redaction_authority(...)` helper
  - `persist_run_contract_if_possible(...)` exact-CAS helper
  - `build_authoring_tool_registry(...)` whose tool set is unchanged but whose execution receives the same authority snapshot
  - `AgentRunner::run_with_provider(..., authority: &AuthoritySnapshot, skill_use: &SkillUse, ...)`
  - authorized Smart Redaction tool dispatch with the old inline prompt and V1 public constructors removed
  - V2 artifact/run-config promotion from the exact bound receipt

- [ ] **Step 1: Write failing ordering and correlation tests**

Add a controllable provider/tool harness plus TaskStore failpoint assertions:

```rust
#[tokio::test]
async fn run_contract_is_committed_before_first_provider_or_tool_effect() {
    let observations = Arc::new(Mutex::new(Vec::new()));
    let store = observing_store(observations.clone());
    let provider = ObservingProvider::new(observations.clone());
    run_smart_redaction_with(store, provider).await;
    assert_eq!(
        observations.lock().unwrap().as_slice(),
        ["running_committed", "run_contract_committed", "provider_started"]
    );
}

#[tokio::test]
async fn run_contract_cas_failure_suppresses_provider_and_proposal() {
    let provider = CountingProvider::default();
    let messages = run_with_store_failpoint(StoreFailpoint::RunContractCas, &provider).await;
    assert_eq!(provider.calls(), 0);
    assert!(messages.iter().any(is_store_failure));
    assert!(!messages.iter().any(is_ready_for_review));
}
```

Add author/improve tests asserting the same package ID/digest, different existing Product mode/evidence input, exact authority/task/run/document correlation, and unchanged operation allow-set. Add a test proving injected body text such as `"GRANT filesystem network process full screenshot apply document"` changes neither snapshot grants nor registry composition.

- [ ] **Step 2: Write failing V2 artifact provenance tests**

On successful terminal persistence, assert:

- artifact metadata receipt equals the active attempt receipt;
- V2 run-config digest changes if authority digest or skill digest changes;
- mismatched terminal/promotion receipt fails and yields no proposal;
- stale document/artifact rejection still wins even when skill digest matches; and
- persisted JSON/Debug omit the skill body and all forbidden privacy terms.

- [ ] **Step 3: Run focused integration tests and verify failure**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::workbench::run::tests --no-fail-fast
rtk cargo test -p rollshot-app task_store --no-fail-fast
```

Expected: FAIL because launch does not resolve/bind authority/skill provenance or pass them to the runner.

- [ ] **Step 4: Resolve the bundled skill before external execution**

Inside the spawned run, after the Slice 2 Running snapshot is committed and before provider setup:

1. build the bundled catalog;
2. invoke source `rollshot.bundled`, package `smart-redaction`, no expected digest, kind `HostExplicit`;
3. fail the Product Task honestly if required package resolution fails;
4. retain the immutable `SkillUse` for the whole run.

Do not inspect project/user directories. Do not fall back to inline prompt text.

- [ ] **Step 5: Build the Product authority from actual current state**

After capability bundle and vision preparation, derive the exact prepared capability set. Convert existing workbench `PayloadMode` into `DisclosureCeiling`; set `existing_product_capture = true`; use a stable V1 policy revision string; grant exactly the six operations needed by the unchanged registry. Construct from current task/attempt/run/document binding.

The operation list comes from Product code, not the skill manifest/body. Assert every registered production tool's declared requirements are a subset before starting the provider, while retaining registry pre-dispatch checks as the enforcement boundary.

- [ ] **Step 6: CAS-bind the receipt before constructing provider work**

Load the current Running snapshot, call `bind_run_contract`, and exact-CAS it. Treat `Committed` and existing commit-visible durability uncertainty exactly as Slice 2 does. On pre-commit failure or CAS mismatch, persist/emit a bounded failure and return before provider/tool execution.

Move no Product document mutation into this stage. The receipt contains no body/path.

- [ ] **Step 7: Pass immutable authority/skill through runner and eval callsites**

Update scripted/provider-backed Smart Redaction runner signatures and every rust-analyzer-reported product, layer1, recorder, and test callsite in the same commit. Validate `AuthorizedModelInput` against the snapshot before `session.push_user`, Rig state creation, or provider stream establishment. Route Smart Redaction through `execute_authorized_calls`; retain a clearly named crate-private ephemeral path only for the non-skill visual annotation stub.

Cut all new Product Tasks/artifacts over to the V2 constructors, then remove the temporary public V1 construction paths. Delete the old monolithic Smart Redaction prompt and use `compose_smart_redaction_prompt` on every provider turn. Eval/recorder construct deterministic bundled `SkillUse` and authority fixtures with the same operation set; they do not bypass registry enforcement. Keep registry membership/order and all UI messages unchanged.

- [ ] **Step 8: Promote V2 artifact from the bound receipt**

In `persist_terminal_outcome`, read the active attempt receipt rather than accepting a newly recomputed receipt. Build `RunConfigFingerprintV2`, its canonical digest, and `ProductArtifactMetadata` from that exact value. Reject missing or mismatched receipts before `record_ready_for_review` and before yielding the terminal proposal.

- [ ] **Step 9: Run focused product and regression suites**

Run:

```bash
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-app result_workspace::workbench --no-fail-fast
rtk cargo test -p rollshot-app task_store --no-fail-fast
rtk cargo test -p rollshot-app result_workspace::tests --no-fail-fast
```

Expected: PASS. Existing author/improve, event correlation, restore, stale apply, review delta, compensation, and no-proposal-on-store-failure tests remain green.

- [ ] **Step 10: Commit end-to-end product wiring**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/run.rs crates/rollshot-app/src/result_workspace/workbench/eval/layer1.rs crates/rollshot-app/src/result_workspace/workbench/eval/record.rs crates/rollshot-agent/src/driver.rs
rtk git commit -m "feat(app): launch Smart Redaction with pinned authority skill"
```

---

### Task 8: Gate G2 Verification, Independent Review, and Decision Record

**Files:**
- Modify only if verification finds a Slice 3 defect: files changed in Tasks 1–7
- Create after all checks pass: `docs/superpowers/spikes/2026-07-27-authority-static-skills-decision.md`
- Test: all affected suites and privacy/containment contracts

**Interfaces:**
- Consumes: completed Slice 3 implementation and spec acceptance criteria.
- Produces: reproducible Gate G2 evidence, independent review findings resolved or recorded, migration/residual-risk record, and a user-approval decision proposal.

- [ ] **Step 1: Run the focused security and migration matrix**

Run:

```bash
rtk cargo test -p rollshot-agent authority::tests --no-fail-fast
rtk cargo test -p rollshot-agent skills::tests --no-fail-fast
rtk cargo test -p rollshot-agent tools::tests --no-fail-fast
rtk cargo test -p rollshot-agent product_task::tests --no-fail-fast
rtk cargo test -p rollshot-app task_store --no-fail-fast
rtk cargo test -p rollshot-app result_workspace::workbench --no-fail-fast
```

Expected: PASS for denial-before-entry, catalog limits/order, containment/symlink/special/oversize/stale behavior, V1 no-rewrite, V2 receipt binding, author/improve, artifact correlation, and privacy.

- [ ] **Step 2: Run affected crate regression suites**

Run:

```bash
rtk cargo test -p rollshot-edit-proposal
rtk cargo test -p rollshot-automation
rtk cargo test -p rollshot-automation-rquickjs
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-vision --no-default-features
rtk cargo test -p rollshot-app
```

Expected: PASS. Record exact passed/ignored counts. This slice does not require a visual-baseline workflow because it changes no user-visible iced UI.

- [ ] **Step 3: Run formatting, lint, and privacy source checks**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --exclude rollshot-ocr --all-targets -- -D warnings
rtk git diff --check
```

Expected: PASS. If workspace clippy exposes a pre-existing warning, verify it against the Slice 3 base and record it without suppressing a new warning.

Inspect serialized/debug/tracing contract tests rather than relying only on source text. Confirm no `println!`, `eprintln!`, `dbg!`, full skill body, ambient catalog path, pixels, OCR text, credentials, or provider-native values enter new Product paths.

- [ ] **Step 4: Request independent code review**

Invoke the `requesting-code-review` skill against all Slice 3 implementation commits and the governing spec. Require explicit answers to:

1. Can provider or tool work begin before the run-contract CAS is commit-visible?
2. Can an advertised tool execute without every declared operation?
3. Can skill manifest/body/catalog membership mutate grants or registry membership?
4. Can host-root loading follow a symlink or reopen an ambient path after validation?
5. Can a digest mismatch substitute current bytes?
6. Can V1 pending artifacts still load/review without synthesized provenance?
7. Can V2 promotion use a receipt different from the active attempt?
8. Can author/improve diverge in package digest or bypass existing stale checks?
9. Can durable/debug/tracing output leak body/path/pixels/OCR/credentials?
10. Did the slice introduce any executable extension, script shortcut, routing, UI, job, retry, or deferred platform capability?

Fix every correctness/security finding, rerun its focused reproduction, then rerun Steps 1–3. Do not dismiss a finding without code/test evidence.

When review requires a code/test correction, commit it before rerunning the matrix:

```bash
rtk git add -u crates/rollshot-agent crates/rollshot-app
rtk git commit -m "fix(agent): resolve authority skills review findings"
```

If review finds no defect, do not create an empty commit.

- [ ] **Step 5: Write the Gate G2 decision record**

Create the decision only after Steps 1–4 pass. Include:

- status `Proposed for user approval`;
- branch/base/implementation commit range;
- selected architecture and exact non-goals;
- authority operation matrix per production tool;
- catalog source/limit/order/containment/digest evidence;
- Smart Redaction author/improve invocation evidence;
- persistence-before-execution and V1/V2 migration evidence;
- artifact task/attempt/run/document/authority/skill trace;
- privacy inspection;
- exact command counts and lint/format results;
- independent review questions/findings/resolution;
- residual risks from the spec; and
- explicit statement that Phase 3 and launch-video work remain unauthorized until their own gates/designs.

- [ ] **Step 6: Commit verification and Gate G2 evidence**

```bash
rtk git add docs/superpowers/spikes/2026-07-27-authority-static-skills-decision.md
rtk git commit -m "docs(agent): record authority static skills Gate G2 evidence"
```

Any defect fix from Step 4 must already have its own focused implementation commit before this documentation commit. Never stage unrelated work.

- [ ] **Step 7: Stop for Gate G2 user approval**

Present the decision record and current verification evidence. Do not begin Slice 4, 5, 6, launch-video design, or any deferred capability until the user explicitly approves Gate G2.
