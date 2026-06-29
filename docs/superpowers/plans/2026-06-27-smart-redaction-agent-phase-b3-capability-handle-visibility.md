# Smart Redaction Agent Phase B3 Capability Handle Visibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose existing `AutomationInput.capability_handles` to Smart Redaction inspection and dry-run without adding template persistence or template inspection.

**Architecture:** Make `ToolContext` the single source of run capability handles. `inspect_image_context` serializes bounded handle metadata and derives template-match availability from the same map that `dry_run` passes into `AutomationInput`. Keep `ToolContext::new` as the empty-handle default and add `ToolContext::new_with_capability_handles` for handle-aware runs so intermediate commits do not break existing callers.

**Tech Stack:** Rust, `BTreeMap`, `serde`, `rollshot-agent` authoring tools, `rollshot-app` workbench, `rollshot-automation::AutomationInput`, Tokio tests.

---

## File Structure

- Modify `crates/rollshot-agent/src/tools.rs`
  - Add `ToolContext.capability_handles`.
  - Add `ToolContext::new_with_capability_handles` while preserving `ToolContext::new` as an empty-handle default.
  - Add `CapabilityHandleSummary` and `ImageContextResult.capability_handles`.
  - Compute template-match availability from the handle map.
  - Pass handles into `DryRunTool`'s `AutomationInput`.
  - Add focused inspection and dry-run tests.
- Modify `crates/rollshot-agent/src/driver.rs`
  - Update the Smart Redaction prompt and prompt contract tests.
- Modify `crates/rollshot-app/src/result_workspace/workbench/run.rs`
  - Add an explicit empty product handle helper.
  - Use `ToolContext::new_with_capability_handles` for Smart Redaction workbench runs.
  - Keep current product behavior empty until Phase F.

No other files are required for this phase.

---

## Engineering Review Lock-In

### Reference Harness Notes

`learn-projects/claude-code-source-code/README.md` describes an agent loop where
tool-facing state is surfaced through tool results, tool calls are validated
before execution, and the loop appends structured `tool_result` values back to
the model. B3 follows that pattern by making capability-handle state visible via
`inspect_image_context` and by making dry-run execute against the same
run-scoped state.

### Data Flow

```text
product workbench / tests
        |
        v
ToolContext::new_with_capability_handles(...)
        |
        +--> inspect_image_context
        |       |
        |       v
        |   capability_handles[] + template_match status
        |
        +--> dry_run
                |
                v
        AutomationInput.capability_handles
                |
                v
        QuickJS input.capabilityHandles
```

### NOT in Scope

- Template asset creation: Phase F owns the lifecycle for creating handles.
- Template handle persistence: B3 only carries handles supplied by the current run.
- Template-match inspection: no `inspect_template_match` tool is added here.
- Product template preparation: `RealAutomationHost::prepare_template_match` remains deferred.
- JavaScript validation schema changes: B3 proves existing `input.capabilityHandles` data flow rather than changing the language.

### What Already Exists

- `AutomationInput.capability_handles` already exists in `rollshot-automation`; B3 reuses it rather than adding a new input shape.
- `DryRunTool` already constructs `AutomationInput`; B3 changes only the handle map source.
- `inspect_image_context` already reports capability status; B3 extends that result instead of adding another inspection tool.
- `rollshot.templateMatch` already exists in the QuickJS bridge; B3 does not rebuild or widen that capability.
- Product workbench already has two empty `capability_handles` construction sites; B3 makes the empty state explicit through one helper.

### Failure Modes

| Codepath | Production failure | Test coverage | Handling / user visibility |
|---|---|---|---|
| `inspect_image_context` handle serialization | Too many handles could bloat tool results | Task 1 / Step 2 bounds output to 16 | Bounded list remains visible; no silent unbounded result |
| `inspect_image_context` template status | Empty product map could look available | Task 1 / Step 2 and Task 5 / Step 4 | Structured `unavailable/no_capability_handles` in tool result |
| `dry_run` handle propagation | Inspection sees a handle but JS sees empty `input.capabilityHandles` | Task 3 / Step 2 uses real `QuickJsExecutor` | Dry-run candidate count exposes mismatch |
| Product workbench handle source | Future code could reintroduce scattered empty maps | Task 5 / Step 1 and Step 4 | `product_capability_handles()` is the single edit point |
| Prompt guidance | Model invents handles despite empty map | Task 4 / Step 2 | Provider prompt contract fails before runtime |

Critical gaps after review: none.

### Test Coverage Table

| Task / behavior | Unit | Integ | E2E / smoke | Manual only |
|---|---:|---:|---:|---:|
| Task 1 / empty handle inspection result | ✓ | — | — | no |
| Task 1 / populated handle inspection result | ✓ | — | — | no |
| Task 1 / bounded handle inspection result | ✓ | — | — | no |
| Task 2 / `ToolContext` stores handles and keeps default constructor | ✓ | — | — | no |
| Task 3 / dry-run passes handles into real QuickJS input | ✓ | ✓ | — | no |
| Task 4 / prompt instructs inspected handles before templateMatch | ✓ | — | — | no |
| Task 5 / product workbench remains empty and truthful | ✓ | ✓ | — | no |

### Parallelization

Sequential execution, no parallelization opportunity. Tasks 1-3 all touch
`crates/rollshot-agent/src/tools.rs`, Task 4 depends on the same agent context,
and Task 5 depends on the constructor added earlier.

### Auto Decisions Applied

Auto decision D1 — Preserve a default constructor and add a handle-aware constructor.
Context: The original plan changed `ToolContext::new` directly, which would force every caller to update at once.
ELI10: If we change the only door into a shared object, every room that uses that door breaks until updated. Keeping the old door and adding a new explicit door lets the work land in smaller, compiling commits.
Stakes if we pick wrong: Intermediate task commits can leave `rollshot-app` or unrelated tests uncompilable.
Recommendation: 1A because it keeps the diff right-sized and preserves existing behavior.
Note: options differ in kind, not coverage — no completeness score.
Pros / cons:
1A) Keep `new` and add `new_with_capability_handles` (recommended) - human: ~20 min / AI: ~5 min, low risk, low maintenance.
  ✅ Existing empty-handle callers keep compiling.
  ❌ Adds one extra constructor to maintain.
1B) Change `ToolContext::new` directly - human: ~45 min / AI: ~10 min, medium risk, low maintenance.
  ✅ Only one constructor exists.
  ❌ Every caller must change in the same task or intermediate commits break.
Net: The extra constructor buys smaller, safer commits.

Auto decision D2 — Use real QuickJS for the dry-run propagation test.
Context: Capturing Rust `AutomationInput` proves only that the executor received the map, not that JavaScript can read `input.capabilityHandles`.
ELI10: We need to test the thing the model will rely on, not just the plumbing before it. If JS cannot read the handle, the agent may write correct-looking code that never works.
Stakes if we pick wrong: Inspection and dry-run can appear aligned while generated JavaScript still sees no handles.
Recommendation: 2A because it tests the real contract at the user-facing boundary.
Completeness: A=10/10, B=6/10
Pros / cons:
2A) Real `QuickJsExecutor` dry-run test (recommended) - human: ~30 min / AI: ~10 min, low risk, low maintenance.
  ✅ Proves `input.capabilityHandles.logo` is visible inside JS.
  ❌ Slightly more integration-heavy than a fake executor.
2B) Keep `CapturingExecutor` only - human: ~10 min / AI: ~3 min, low risk, medium maintenance.
  ✅ Very focused Rust-side plumbing test.
  ❌ Misses the actual JS boundary.
Net: B3 is about agent-visible context, so the JS boundary is the right test boundary.

Auto decision D3 — Add bounded-output coverage for handle summaries.
Context: The spec says inspection output is bounded, but the original plan did not test the 16-entry cap.
ELI10: A tool result can become too large if a future run has many handles. The model only needs a compact list, and the plan should prove it stays compact.
Stakes if we pick wrong: A large handle map can exhaust tool result budgets or make prompt context noisy.
Recommendation: 3A because completeness is cheap and the bound is part of the contract.
Completeness: A=10/10, B=7/10
Pros / cons:
3A) Add a 20-handle test capped to 16 (recommended) - human: ~15 min / AI: ~5 min, low risk, low maintenance.
  ✅ Locks down the bounded result contract.
  ❌ Adds one more test case.
3B) Rely on implementation review - human: ~0 min / AI: ~0 min, medium risk, low maintenance.
  ✅ No extra test code.
  ❌ Future edits can accidentally remove the cap.
Net: The bound is small and important enough to test directly.

Auto decision D4 — Add a product-context test independent of OCR feature mode.
Context: The original product assertion lived inside a default-build OCR-disabled test.
ELI10: Template handles are unrelated to OCR. We should verify product template availability stays truthful in any build, including OCR-enabled builds.
Stakes if we pick wrong: OCR-enabled builds could regress template-match availability without the product test catching it.
Recommendation: 4A because build-feature independence is explicit and cheap.
Completeness: A=9/10, B=6/10
Pros / cons:
4A) Add a standalone product template-handle inspection test (recommended) - human: ~20 min / AI: ~5 min, low risk, low maintenance.
  ✅ Covers default and OCR-enabled builds.
  ❌ Slightly duplicates part of the default OCR-disabled assertion.
4B) Keep only the default-build assertion - human: ~0 min / AI: ~0 min, medium risk, low maintenance.
  ✅ No added test.
  ❌ Leaves OCR-enabled product mode less covered.
Net: Capability-handle behavior should not depend on OCR features.

---

### Task 1: Agent Inspection Contract Tests

**Files:**
- Modify: `crates/rollshot-agent/src/tools.rs`
- Test: `crates/rollshot-agent/src/tools.rs`

- [ ] **Step 1: Add a handle-aware test helper**

In `crates/rollshot-agent/src/tools.rs`, inside `#[cfg(test)] pub(crate) mod tests`, replace the existing `test_context` helper with:

```rust
    fn test_context(source: &str) -> Arc<ToolContext> {
        test_context_with_handles(source, std::collections::BTreeMap::new())
    }

    fn test_context_with_handles(
        source: &str,
        capability_handles: std::collections::BTreeMap<String, String>,
    ) -> Arc<ToolContext> {
        let mut policy = rollshot_automation::ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(5),
            4 * 1024 * 1024,
            1024 * 1024,
        );
        policy.proposal_limits.max_total_area_fraction = 0.5;
        Arc::new(ToolContext::new_with_capability_handles(
            SessionId::new(1),
            source.into(),
            rollshot_automation::ValidationLimits::default(),
            policy,
            (100, 100),
            capability_handles,
            &RunCancellation::new(),
        ))
    }

    fn template_handle_map() -> std::collections::BTreeMap<String, String> {
        std::collections::BTreeMap::from([("logo".into(), "tpl-logo-v1".into())])
    }
```

Leave existing `ToolContext::new(...)` call sites alone in this task. They should continue to mean "empty capability handles".

- [ ] **Step 2: Add failing inspection assertions**

Extend `inspect_image_context_returns_authoring_and_region_context()` with:

```rust
                assert!(result_json["capability_handles"].as_array().unwrap().is_empty());
                assert_eq!(
                    result_json["capabilities"]["template_match"]["reason"].as_str(),
                    Some("no_capability_handles")
                );
```

Add a new test below it:

```rust
    #[tokio::test]
    async fn inspect_image_context_exposes_existing_capability_handles() {
        let ctx = test_context_with_handles("source", template_handle_map());
        let tool = InspectImageContextTool::new(ctx, inspection_context_for_tests());

        let result = tool.call(&serde_json::json!({})).await.unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(
                    result_json["capability_handles"][0]["name"].as_str(),
                    Some("logo")
                );
                assert_eq!(
                    result_json["capability_handles"][0]["handle"].as_str(),
                    Some("tpl-logo-v1")
                );
                assert_eq!(
                    result_json["capability_handles"][0]["capability"].as_str(),
                    Some("template_match")
                );
                assert_eq!(
                    result_json["capabilities"]["template_match"]["status"].as_str(),
                    Some("available")
                );
                assert!(result_json["capabilities"]["template_match"]["reason"].is_null());
            }
            other => panic!("expected success, got {other:?}"),
        }
    }
```

Add a bounded-output test below it:

```rust
    #[tokio::test]
    async fn inspect_image_context_bounds_capability_handle_summaries() {
        let handles = (0..20)
            .map(|i| (format!("handle-{i:02}"), format!("tpl-{i:02}")))
            .collect();
        let ctx = test_context_with_handles("source", handles);
        let tool = InspectImageContextTool::new(ctx, inspection_context_for_tests());

        let result = tool.call(&serde_json::json!({})).await.unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                let handles = result_json["capability_handles"].as_array().unwrap();
                assert_eq!(handles.len(), 16);
                assert_eq!(handles[0]["name"].as_str(), Some("handle-00"));
                assert_eq!(handles[15]["name"].as_str(), Some("handle-15"));
                assert_eq!(
                    result_json["capabilities"]["template_match"]["status"].as_str(),
                    Some("available")
                );
            }
            other => panic!("expected success, got {other:?}"),
        }
    }
```

- [ ] **Step 3: Run tests to verify the contract is red**

Run:

```bash
rtk cargo test -p rollshot-agent inspect_image_context
```

Expected: FAIL to compile because `ToolContext::new_with_capability_handles` and `ImageContextResult.capability_handles` do not exist.

- [ ] **Step 4: Commit the failing contract tests**

```bash
rtk git add crates/rollshot-agent/src/tools.rs
rtk git commit -m "test(agent): define capability handle inspection contract"
```

---

### Task 2: Implement Capability Handle Inspection

**Files:**
- Modify: `crates/rollshot-agent/src/tools.rs`
- Test: `crates/rollshot-agent/src/tools.rs`

- [ ] **Step 1: Add `BTreeMap` import**

At the top of `crates/rollshot-agent/src/tools.rs`, ensure the collections import includes `BTreeMap`:

```rust
use std::collections::{BTreeMap, BTreeSet, HashSet};
```

- [ ] **Step 2: Add capability handles to `ToolContext`**

Add the field to `ToolContext`:

```rust
    pub capability_handles: BTreeMap<String, String>,
```

Add a handle-aware constructor below `ToolContext::new`:

```rust
    pub fn new_with_capability_handles(
        session_id: SessionId,
        initial_source: String,
        validation_limits: rollshot_automation::ValidationLimits,
        execution_policy: rollshot_automation::ExecutionPolicy,
        image_dims: (u32, u32),
        capability_handles: BTreeMap<String, String>,
        cancellation: &RunCancellation,
    ) -> Self {
        Self {
            draft: Mutex::new(DraftState::new(session_id)),
            source: Mutex::new(initial_source),
            validation_limits,
            execution_policy,
            automation_cancellation: cancellation.automation_flag().clone(),
            session_id,
            image_dims,
            capability_handles,
            pending_ready_for_review: Mutex::new(None),
            last_validated: Mutex::new(None),
            last_dry_run_proposal: Mutex::new(None),
            last_dry_run_metrics: Mutex::new(None),
        }
    }
```

Change `ToolContext::new` to delegate to the new constructor with an empty map:

```rust
        Self::new_with_capability_handles(
            session_id,
            initial_source,
            validation_limits,
            execution_policy,
            image_dims,
            BTreeMap::new(),
            cancellation,
        )
```

- [ ] **Step 3: Add the inspection result type**

Near `ImageContextCapabilities`, add:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct CapabilityHandleSummary {
    pub name: String,
    pub handle: String,
    pub capability: String,
}
```

Update `ImageContextResult`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ImageContextResult {
    pub image: ImageContextImage,
    pub source: ImageContextSource,
    pub regions: Vec<CanonicalRegionInspection>,
    pub ocr_regions: Vec<CanonicalOcrInspection>,
    pub capability_handles: Vec<CapabilityHandleSummary>,
    pub capabilities: ImageContextCapabilities,
}
```

- [ ] **Step 4: Serialize bounded handle metadata**

In `InspectImageContextTool::call`, after OCR availability is computed, add:

```rust
            let capability_handles: Vec<CapabilityHandleSummary> = self
                .ctx
                .capability_handles
                .iter()
                .take(16)
                .map(|(name, handle)| CapabilityHandleSummary {
                    name: name.clone(),
                    handle: handle.clone(),
                    capability: "template_match".into(),
                })
                .collect();
            let template_match = if self.ctx.capability_handles.is_empty() {
                CapabilityStatus::unavailable("no_capability_handles")
            } else {
                CapabilityStatus::available()
            };
```

Use those values in `ImageContextResult`:

```rust
                    capability_handles,
                    capabilities: ImageContextCapabilities {
                        region_features,
                        ocr,
                        layout: self.inspection.layout_status.clone(),
                        template_match,
                    },
```

- [ ] **Step 5: Run inspection tests**

Run:

```bash
rtk cargo test -p rollshot-agent inspect_image_context
```

Expected: PASS.

- [ ] **Step 6: Commit inspection implementation**

```bash
rtk git add crates/rollshot-agent/src/tools.rs
rtk git commit -m "feat(agent): expose capability handles in image context"
```

---

### Task 3: Pass Capability Handles Into Dry Run

**Files:**
- Modify: `crates/rollshot-agent/src/tools.rs`
- Test: `crates/rollshot-agent/src/tools.rs`

- [ ] **Step 1: Add the failing real-QuickJS dry-run propagation test**

Near the dry-run tests, add:

```rust
    #[tokio::test]
    async fn dry_run_exposes_capability_handles_to_javascript_input() {
        let ctx = test_context_with_handles("source", template_handle_map());
        let tool = DryRunTool::new(
            ctx,
            Arc::new(rollshot_automation_rquickjs::QuickJsExecutor),
            Arc::new(Mutex::new(
                rollshot_automation::FakeAutomationHost::default(),
            )),
        );

        let source = r#"
function main(input) {
  if (input.capabilityHandles.logo !== "tpl-logo-v1") {
    return { candidates: [] };
  }
  return {
    candidates: [{
      kind: "addRedaction",
      bounds: { x: 0, y: 0, width: 10, height: 10 },
      confidence: 0.9,
      label: "handle-visible"
    }]
  };
}
"#;
        let result = tool
            .call(&serde_json::json!({"source": source, "generation": 0}))
            .await
            .unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(
                    result_json["candidate_count"].as_u64(),
                    Some(1),
                    "expected JavaScript to see input.capabilityHandles.logo"
                );
            }
            other => panic!("expected dry-run success, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run the dry-run test to verify failure**

Run:

```bash
rtk cargo test -p rollshot-agent dry_run_exposes_capability_handles_to_javascript_input
```

Expected: FAIL because JavaScript sees an empty `input.capabilityHandles` map.

- [ ] **Step 3: Implement dry-run propagation**

In `DryRunTool::call`, replace:

```rust
                capability_handles: std::collections::BTreeMap::new(),
```

with:

```rust
                capability_handles: self.ctx.capability_handles.clone(),
```

- [ ] **Step 4: Run dry-run tests**

Run:

```bash
rtk cargo test -p rollshot-agent dry_run
```

Expected: PASS.

- [ ] **Step 5: Commit dry-run propagation**

```bash
rtk git add crates/rollshot-agent/src/tools.rs
rtk git commit -m "feat(agent): pass capability handles into dry run"
```

---

### Task 4: Update Driver Prompt

**Files:**
- Modify: `crates/rollshot-agent/src/driver.rs`
- Test: `crates/rollshot-agent/src/driver.rs`

- [ ] **Step 1: Add prompt contract assertions**

In `second_turn_request_carries_history_and_tool_schemas`, add assertions near the existing inspection-loop checks:

```rust
            assert!(
                system_prompt.contains("Use only template handles listed by inspect_image_context"),
                "system prompt should require inspected template handles before templateMatch, got: {:?}",
                system_prompt
            );
            assert!(
                system_prompt.contains("Do not invent template handles"),
                "system prompt should forbid invented template handles, got: {:?}",
                system_prompt
            );
```

- [ ] **Step 2: Run the prompt test to verify failure**

Run:

```bash
rtk cargo test -p rollshot-agent second_turn_request_carries_history_and_tool_schemas
```

Expected: FAIL because the prompt does not yet contain the B3 handle guidance.

- [ ] **Step 3: Update the Smart Redaction system prompt**

In `SMART_REDACTION_SYSTEM_PROMPT`, replace:

```text
- Supported capability calls are rollshot.ocr(query), rollshot.layout(query) when available, rollshot.regionFeatures(query), and rollshot.templateMatch(query) only when a matching input.capabilityHandles entry exists.
```

with:

```text
- Supported capability calls are rollshot.ocr(query), rollshot.layout(query) when available, rollshot.regionFeatures(query), and rollshot.templateMatch(query) only when a matching input.capabilityHandles entry exists.
- Use only template handles listed by inspect_image_context capability_handles before calling rollshot.templateMatch. Do not invent template handles when that list is empty.
```

Update the inspection loop by inserting this new step after `inspect_image_context`:

```text
2. Check capability_handles before writing source that calls rollshot.templateMatch.
```

Renumber the later inspection-loop steps so the list remains sequential.

- [ ] **Step 4: Run driver tests**

Run:

```bash
rtk cargo test -p rollshot-agent second_turn_request_carries_history_and_tool_schemas
rtk cargo test -p rollshot-agent smart_redaction_prompt_examples_validate
```

Expected: PASS.

- [ ] **Step 5: Commit prompt changes**

```bash
rtk git add crates/rollshot-agent/src/driver.rs
rtk git commit -m "feat(agent): guide template handle visibility"
```

---

### Task 5: Update Product Workbench Call Sites

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`
- Test: `crates/rollshot-app/src/result_workspace/workbench/run.rs`

- [ ] **Step 1: Add the product helper**

In `crates/rollshot-app/src/result_workspace/workbench/run.rs`, add this helper near `authoring_inspection_context`:

```rust
fn product_capability_handles() -> std::collections::BTreeMap<String, String> {
    std::collections::BTreeMap::new()
}
```

- [ ] **Step 2: Update product `ToolContext` construction**

In `start_agent_run`, change the constructor to `ToolContext::new_with_capability_handles` and pass the helper result before `&cancellation_for_task`:

```rust
        let tool_ctx = Arc::new(rollshot_agent::tools::ToolContext::new_with_capability_handles(
            session_id,
            active_source,
            validation_limits,
            policy,
            image_dims,
            product_capability_handles(),
            &cancellation_for_task,
        ));
```

- [ ] **Step 3: Update workbench test `ToolContext` call sites**

For Smart Redaction workbench test helpers in the same file, switch to `ToolContext::new_with_capability_handles` and pass:

```rust
            product_capability_handles(),
```

before the cancellation argument in each `ToolContext::new` call.

- [ ] **Step 4: Add product unavailable assertion**

In `authoring_registry_exposes_truthful_phase_b1_tools`, assert the product handle map remains empty:

```rust
        assert!(product_capability_handles().is_empty());
```

In `default_build_inspection_reports_ocr_disabled`, add:

```rust
                assert!(result_json["capability_handles"].as_array().unwrap().is_empty());
                assert_eq!(
                    result_json["capabilities"]["template_match"]["reason"].as_str(),
                    Some("no_capability_handles")
                );
```

Add a standalone product template-handle inspection test that is not gated on OCR:

```rust
    #[tokio::test]
    async fn product_inspection_reports_template_match_unavailable_without_handles() {
        use rollshot_agent::tools::{InspectImageContextTool, Tool};

        let ctx = tool_context_for_tests();
        let inspection = authoring_inspection_context(
            PayloadMode::FullScreenshot,
            &canonical_region_feature_catalog(64, 64),
            &canonical_ocr_catalog(64, 64),
        );
        let tool = InspectImageContextTool::new(ctx, inspection);

        let result = tool.call(&serde_json::json!({})).await.unwrap();

        match result {
            rollshot_agent::tools::ToolOutcome::Success { result_json } => {
                assert!(result_json["capability_handles"].as_array().unwrap().is_empty());
                assert_eq!(
                    result_json["capabilities"]["template_match"]["status"].as_str(),
                    Some("unavailable")
                );
                assert_eq!(
                    result_json["capabilities"]["template_match"]["reason"].as_str(),
                    Some("no_capability_handles")
                );
            }
            other => panic!("expected inspection success, got {other:?}"),
        }
    }
```

- [ ] **Step 5: Run workbench tests**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::workbench
rtk cargo test -p rollshot-app --features ocr product_inspection_reports_template_match_unavailable_without_handles
```

Expected: PASS.

- [ ] **Step 6: Commit workbench call-site updates**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/run.rs
rtk git commit -m "feat(app): surface empty product capability handles"
```

---

### Task 6: Final Verification

**Files:**
- No source edits unless verification exposes a defect in this phase's changes.

- [ ] **Step 1: Run focused agent inspection tests**

Run:

```bash
rtk cargo test -p rollshot-agent inspect_image_context
```

Expected: PASS.

- [ ] **Step 2: Run focused dry-run tests**

Run:

```bash
rtk cargo test -p rollshot-agent dry_run
```

Expected: PASS.

- [ ] **Step 3: Run prompt contract tests**

Run:

```bash
rtk cargo test -p rollshot-agent second_turn_request_carries_history_and_tool_schemas
rtk cargo test -p rollshot-agent smart_redaction_prompt_examples_validate
```

Expected: PASS.

- [ ] **Step 4: Run workbench tests**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::workbench
rtk cargo test -p rollshot-app --features ocr product_inspection_reports_template_match_unavailable_without_handles
```

Expected: PASS.

- [ ] **Step 5: Run formatting**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 6: Inspect final git status**

Run:

```bash
rtk git status --short
```

Expected: only intentional B3 source changes are present. Ignore unrelated untracked `learn-projects/claude-code-source-code/` if it remains present.

- [ ] **Step 7: Commit verification fixes when required**

If a verification command required a source fix, commit only the files from this phase:

```bash
rtk git add crates/rollshot-agent/src/tools.rs crates/rollshot-agent/src/driver.rs crates/rollshot-app/src/result_workspace/workbench/run.rs
rtk git commit -m "fix(agent): stabilize capability handle visibility"
```

If verification passes without fixes, skip this step.

---

## Plan Self-Review

- Spec coverage: The plan covers run-level handle ownership, inspection output, template-match availability, dry-run consistency, product empty-handle behavior, prompt guidance, and verification.
- Scope guard: The plan does not add template persistence, template creation UI, template-match inspection, or product template preparation.
- Type consistency: `ToolContext.capability_handles` uses the existing `BTreeMap<String, String>` shape from `AutomationInput`; inspection uses a serializable list to keep output deterministic and bounded.
- Testing path: Agent tests prove populated handles work; workbench tests prove current product runs remain empty and truthful.
