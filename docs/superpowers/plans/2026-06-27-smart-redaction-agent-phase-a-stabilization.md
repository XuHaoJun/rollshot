# Smart Redaction Agent Phase A Stabilization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the current Smart Redaction agent harness reliably produce a reviewable JavaScript preset draft for simple screenshot-redaction intents.

**Architecture:** Keep the existing bounded agent loop and QuickJS runtime. Stabilize the authoring harness by improving the prompt/tool contract, locking the product tool registry to truthful tools, preparing region-feature capabilities before QuickJS, and returning bounded dry-run candidate previews.

**Tech Stack:** Rust, `rollshot-agent`, `rollshot-app`, `rollshot-automation`, `rollshot-automation-rquickjs`, `rollshot-vision`, iced task wiring.

---

## File Structure

- Modify `crates/rollshot-agent/src/driver.rs`
  - Expand the Smart Redaction system prompt into an authoring guide.
  - Extend existing provider-request tests so prompt regressions fail.
- Modify `crates/rollshot-agent/src/tools.rs`
  - Add bounded dry-run candidate preview result types.
  - Extend dry-run tests.
- Modify `crates/rollshot-app/src/result_workspace/workbench/run.rs`
  - Add a small authoring registry helper that exposes only Phase A truthful tools.
  - Add canonical region-feature preparation helpers.
  - Use the preparation helper in both agent dry-run setup and existing-preset execution.
  - Add focused workbench tests.
- Do not modify `crates/rollshot-vision` in Phase A unless tests reveal the existing `RealAutomationHost::prepare_region_features` contract is insufficient.

## Task 1: Expand Smart Redaction Authoring Guide

**Files:**
- Modify: `crates/rollshot-agent/src/driver.rs`

- [ ] **Step 1: Extend the provider-request test first**

In `crates/rollshot-agent/src/driver.rs`, find the test that currently asserts the provider request contains `"hide the URL bar"`, `"already captured the current screenshot"`, and `"do not ask the user to upload"`. Add these assertions inside that same test block:

```rust
            let system_prompt = requests[0]
                .system_prompt
                .as_deref()
                .unwrap_or_default();
            assert!(
                system_prompt.contains("Rollshot JavaScript authoring guide"),
                "system prompt should include authoring guide marker, got: {:?}",
                requests[0].system_prompt
            );
            assert!(
                system_prompt.contains("function main(input)"),
                "system prompt should document required source shape, got: {:?}",
                requests[0].system_prompt
            );
            assert!(
                system_prompt.contains("rollshot.regionFeatures"),
                "system prompt should document region features API, got: {:?}",
                requests[0].system_prompt
            );
            assert!(
                system_prompt.contains("{ candidates:"),
                "system prompt should document output envelope, got: {:?}",
                requests[0].system_prompt
            );
            assert!(
                system_prompt.contains("validate_source"),
                "system prompt should require validation before submit, got: {:?}",
                requests[0].system_prompt
            );
            assert!(
                system_prompt.contains("dry_run"),
                "system prompt should require dry run before submit, got: {:?}",
                requests[0].system_prompt
            );
            assert!(
                system_prompt.contains("submit_for_review"),
                "system prompt should require review submit, got: {:?}",
                requests[0].system_prompt
            );
```

- [ ] **Step 2: Run the failing driver prompt test**

Run:

```bash
rtk cargo test -p rollshot-agent second_turn_request_carries_history_and_tool_schemas -- --nocapture
```

Expected: FAIL because the current prompt does not contain `Rollshot JavaScript authoring guide` and the other new markers.

- [ ] **Step 3: Replace the system prompt with a static authoring guide**

In `crates/rollshot-agent/src/driver.rs`, replace the current `SMART_REDACTION_SYSTEM_PROMPT` constant with:

```rust
const SMART_REDACTION_SYSTEM_PROMPT: &str = r#"You are Rollshot Smart Redaction Agent.
Your only job is to create editable redaction candidates for the current screenshot.
Rollshot has already captured the current screenshot for this run. Use the provided screenshot attachment, local context, and available tools; do not ask the user to upload, attach, or take another screenshot.

Interpret user requests like "hide the URL bar", "hide emails", or "redact names" as redaction targets.
For common screenshot regions such as a browser URL/address bar, infer the visible target from the current screenshot instead of asking what device or app environment the user is using.
If the request is not about hiding or redacting visible content, refuse briefly and ask for a redaction target.
If the redaction target is ambiguous after inspecting the available screenshot/context, ask one brief clarifying question about what visible content should be redacted.
Do not provide general advice, product support, or workflow guidance.

Rollshot JavaScript authoring guide:
- Write exactly one synchronous function main(input). Do not use async, imports, exports, timers, eval, Function, DOM, filesystem, network, process APIs, dynamic property access, or loops that can run forever.
- Available input fields use camelCase: input.imageWidth, input.imageHeight, input.region, input.annotations, input.capabilityHandles.
- Return an object shaped like { candidates: [...] }.
- Each candidate must be { kind: "addRedaction", bounds, confidence, label } with optional rationale.
- bounds is { x, y, width, height } in image pixels. width and height must be positive.
- confidence must be between 0 and 1. label must be short and non-empty.
- Supported capability calls are rollshot.ocr(query), rollshot.layout(query) when available, rollshot.regionFeatures(query), and rollshot.templateMatch(query) only when a matching input.capabilityHandles entry exists.
- In Phase A, OCR and layout may fail unless dry_run proves they are available. Prefer deterministic regionFeatures strip regions for simple screenshot chrome targets, for example:
  const bounds = { x: 0, y: 0, width: input.imageWidth, height: Math.min(96, input.imageHeight) };
  const features = rollshot.regionFeatures({ region: { kind: "rect", bounds: bounds }, limit: 1 });
- Example empty result: function main(input) { return { candidates: [] }; }
- Example redaction from a strip:
  function main(input) {
    const bounds = { x: 0, y: 0, width: input.imageWidth, height: Math.min(96, input.imageHeight) };
    const features = rollshot.regionFeatures({ region: { kind: "rect", bounds: bounds }, limit: 1 });
    if (features.length === 0) { return { candidates: [] }; }
    return { candidates: [{ kind: "addRedaction", bounds: bounds, confidence: 0.6, label: "top-strip" }] };
  }
- Example OCR redaction when OCR is available:
  function expand(rect, padding) {
    return { x: Math.max(0, rect.x - padding), y: Math.max(0, rect.y - padding), width: rect.width + padding * 2, height: rect.height + padding * 2 };
  }
  function main(input) {
    const matches = rollshot.ocr({ region: input.region, limit: 20 });
    return { candidates: matches.map((match) => ({ kind: "addRedaction", bounds: expand(match.bounds, 6), confidence: match.confidence, label: "ocr-match" })) };
  }

Authoring loop:
1. Use replace_source for a new source generation.
2. Use validate_source on the current generation.
3. Use dry_run on the current generation.
4. If validation or dry_run fails, edit the source and retry from replace_source.
5. Use submit_for_review only after the current generation has successful validate_source and dry_run evidence.
6. A successful dry_run means "ready for user review", not "safe to export"."#;
```

- [ ] **Step 4: Run the driver prompt test again**

Run:

```bash
rtk cargo test -p rollshot-agent second_turn_request_carries_history_and_tool_schemas -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit Task 1**

Run:

```bash
rtk git add crates/rollshot-agent/src/driver.rs
rtk git commit -m "feat(agent): add smart redaction authoring guide"
```

Expected: commit succeeds.

## Task 2: Lock Product Registry To Truthful Phase A Tools

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`

- [ ] **Step 1: Add a failing registry test**

In `crates/rollshot-app/src/result_workspace/workbench/run.rs`, inside the existing `#[cfg(test)] mod prepare_tests`, add this test and helper:

```rust
    fn tool_context_for_tests() -> std::sync::Arc<rollshot_agent::tools::ToolContext> {
        let cancel = rollshot_agent::runtime::RunCancellation::new();
        std::sync::Arc::new(rollshot_agent::tools::ToolContext::new(
            rollshot_agent::domain::SessionId::new(1),
            String::new(),
            rollshot_automation::ValidationLimits::default(),
            rollshot_automation::ExecutionPolicy::smart_redaction_default(
                std::time::Duration::from_secs(5),
                4 * 1024 * 1024,
                1024 * 1024,
            ),
            (64, 64),
            &cancel,
        ))
    }

    #[test]
    fn authoring_registry_exposes_only_truthful_phase_a_tools() {
        let ctx = tool_context_for_tests();
        let executor: std::sync::Arc<dyn rollshot_automation::AutomationExecutor> =
            std::sync::Arc::new(rollshot_automation_rquickjs::QuickJsExecutor);
        let host: std::sync::Arc<
            std::sync::Mutex<dyn rollshot_automation::AutomationHost>,
        > = std::sync::Arc::new(std::sync::Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));

        let registry = build_authoring_tool_registry(ctx, executor, host).unwrap();
        let names = registry.tool_names();

        assert_eq!(
            names,
            vec![
                "replace_source",
                "validate_source",
                "submit_for_review",
                "request_user_input",
                "inspect_context_summary",
                "dry_run",
            ]
        );
        assert!(!names.contains(&"inspect_ocr"));
        assert!(!names.contains(&"inspect_layout"));
        assert!(!names.contains(&"inspect_region_features"));
    }
```

- [ ] **Step 2: Run the failing registry test**

Run:

```bash
rtk cargo test -p rollshot-app authoring_registry_exposes_only_truthful_phase_a_tools -- --nocapture
```

Expected: FAIL because `build_authoring_tool_registry` does not exist.

- [ ] **Step 3: Add the registry helper**

In `crates/rollshot-app/src/result_workspace/workbench/run.rs`, after `impl RunEventSink for ChannelEventSink`, add:

```rust
fn build_authoring_tool_registry(
    tool_ctx: Arc<rollshot_agent::tools::ToolContext>,
    executor: Arc<dyn rollshot_automation::AutomationExecutor>,
    host: Arc<StdMutex<dyn rollshot_automation::AutomationHost>>,
) -> Result<rollshot_agent::tools::ToolRegistry, WorkbenchError> {
    use rollshot_agent::tools::{
        DryRunTool, GetContextSummaryTool, ReplaceSourceTool, RequestUserInputTool,
        SubmitForReviewTool, ToolRegistry, ToolRegistryLimits, ValidateSourceTool,
    };

    let mut registry = ToolRegistry::new(ToolRegistryLimits::permissive());
    let reg = |registry: &mut ToolRegistry,
               tool: Arc<dyn rollshot_agent::tools::Tool>|
     -> Result<(), WorkbenchError> {
        registry
            .register(tool)
            .map_err(|_| WorkbenchError::RuntimeFailure)
    };

    reg(
        &mut registry,
        Arc::new(ReplaceSourceTool::new(tool_ctx.clone())),
    )?;
    reg(
        &mut registry,
        Arc::new(ValidateSourceTool::new(tool_ctx.clone())),
    )?;
    reg(
        &mut registry,
        Arc::new(SubmitForReviewTool::new(tool_ctx.clone())),
    )?;
    reg(
        &mut registry,
        Arc::new(RequestUserInputTool::new(tool_ctx.clone())),
    )?;
    reg(
        &mut registry,
        Arc::new(GetContextSummaryTool::new(tool_ctx.clone())),
    )?;
    reg(
        &mut registry,
        Arc::new(DryRunTool::new(tool_ctx, executor, host)),
    )?;

    Ok(registry)
}
```

- [ ] **Step 4: Use the helper from `start_agent_run`**

In `start_agent_run`, replace the local registry creation and six `reg(...)` calls with:

```rust
        let registry = match build_authoring_tool_registry(
            tool_ctx.clone(),
            Arc::new(vision.executor),
            vision.host.clone() as Arc<StdMutex<dyn rollshot_automation::AutomationHost>>,
        ) {
            Ok(registry) => registry,
            Err(e) => {
                yield crate::result_workspace::Message::Workbench(
                    super::WorkbenchMessage::RunFailed(e),
                );
                return;
            }
        };
```

Also shrink the `use rollshot_agent::{ ... }` block in `start_agent_run` so it imports only:

```rust
    use rollshot_agent::{
        domain::{AttachmentDescriptor, AuthorizedModelInput, MediaType},
        driver::{AgentConfig, AgentRunner},
        runtime::{RunCancellation, RunEvent},
    };
```

- [ ] **Step 5: Run the registry test**

Run:

```bash
rtk cargo test -p rollshot-app authoring_registry_exposes_only_truthful_phase_a_tools -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit Task 2**

Run:

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/run.rs
rtk git commit -m "feat(app): lock smart redaction authoring tools"
```

Expected: commit succeeds.

## Task 3: Prepare Region Features Before QuickJS

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`

- [ ] **Step 1: Add a failing existing-preset region-feature test**

In `crates/rollshot-app/src/result_workspace/workbench/run.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    fn make_revision_from_source(source: &str) -> AutomationRevision {
        use rollshot_preset::*;
        let limits = rollshot_automation::ValidationLimits::default();
        let validated = rollshot_automation::validate_source(source, &limits).unwrap();
        AutomationRevision {
            store_schema_version: STORE_SCHEMA_VERSION,
            id: RevisionId("rev-1".into()),
            preset_id: PresetId("test".into()),
            parent_id: None,
            created_at: "2026-06-27T00:00:00Z".into(),
            provenance: RevisionProvenance {
                origin: RevisionOrigin::Manual,
                note: None,
                source_run_ref: None,
            },
            artifact: validated,
        }
    }

    #[test]
    fn run_existing_preset_prepares_top_strip_region_features() {
        let source = r#"
function main(input) {
  const bounds = { x: 0, y: 0, width: input.imageWidth, height: Math.min(96, input.imageHeight) };
  const features = rollshot.regionFeatures({ region: { kind: "rect", bounds: bounds }, limit: 1 });
  if (features.length === 0) {
    return { candidates: [] };
  }
  return {
    candidates: [{
      kind: "addRedaction",
      bounds: bounds,
      confidence: 0.6,
      label: "top-strip"
    }]
  };
}
"#;
        let image = image::RgbaImage::from_pixel(160, 120, image::Rgba([30, 30, 30, 255]));
        let revision = make_revision_from_source(source);
        let policy = ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(10),
            100_000_000,
            8_000_000,
        );

        let proposal = run_existing_preset(&image, &revision, &policy).unwrap();

        assert_eq!(proposal.candidates.len(), 1);
    }
```

- [ ] **Step 2: Run the failing test**

Run:

```bash
rtk cargo test -p rollshot-app run_existing_preset_prepares_top_strip_region_features -- --nocapture
```

Expected: FAIL with a runtime failure caused by missing prepared region features.

- [ ] **Step 3: Add canonical region-feature helpers**

In `crates/rollshot-app/src/result_workspace/workbench/run.rs`, near the top after imports, add:

```rust
const PHASE_A_REGION_FEATURE_STRIP_PX: u32 = 96;
const PHASE_A_REGION_FEATURE_LIMIT: u32 = 1;
const PHASE_A_REGION_FEATURE_FULL_AREA_LIMIT: u64 = 8_000_000;
```

After `smart_redaction_budget`, add:

```rust
fn phase_a_region_feature_queries(
    width: u32,
    height: u32,
) -> Vec<rollshot_automation::RegionFeaturesQuery> {
    use rollshot_automation::{Region, RegionFeaturesQuery};
    use rollshot_image_document::ImageRect;

    if width == 0 || height == 0 {
        return Vec::new();
    }

    let mut queries = Vec::new();
    let full_area = width as u64 * height as u64;
    if full_area <= PHASE_A_REGION_FEATURE_FULL_AREA_LIMIT {
        queries.push(RegionFeaturesQuery {
            region: Region::Full,
            limit: PHASE_A_REGION_FEATURE_LIMIT,
        });
    }

    let strip_h = height.min(PHASE_A_REGION_FEATURE_STRIP_PX) as f32;
    let strip_w = width.min(PHASE_A_REGION_FEATURE_STRIP_PX) as f32;
    let width_f = width as f32;
    let height_f = height as f32;

    let push_rect =
        |queries: &mut Vec<RegionFeaturesQuery>, x: f32, y: f32, width: f32, height: f32| {
            if width <= 0.0 || height <= 0.0 {
                return;
            }
            let area = (width.ceil() as u64).saturating_mul(height.ceil() as u64);
            if area > PHASE_A_REGION_FEATURE_FULL_AREA_LIMIT {
                return;
            }
            queries.push(RegionFeaturesQuery {
                region: Region::Rect {
                    bounds: ImageRect {
                        x,
                        y,
                        width,
                        height,
                    },
                },
                limit: PHASE_A_REGION_FEATURE_LIMIT,
            });
        };

    push_rect(&mut queries, 0.0, 0.0, width_f, strip_h);
    push_rect(&mut queries, 0.0, 0.0, strip_w, height_f);
    push_rect(
        &mut queries,
        (width_f - strip_w).max(0.0),
        0.0,
        strip_w,
        height_f,
    );
    push_rect(
        &mut queries,
        0.0,
        (height_f - strip_h).max(0.0),
        width_f,
        strip_h,
    );

    queries
}

fn prepare_phase_a_region_features(
    host: &mut rollshot_vision::RealAutomationHost,
    index: &VisualIndex,
) -> Result<(), WorkbenchError> {
    for query in phase_a_region_feature_queries(index.width(), index.height()) {
        host.prepare_region_features(index, &query)
            .map_err(|e| WorkbenchError::VisionPrepare {
                message: format!("regionFeatures: {e}"),
            })?;
    }
    Ok(())
}
```

- [ ] **Step 4: Use the preparation helper in existing-preset execution**

In `run_existing_preset`, replace:

```rust
    let _index = VisualIndex::build(image.clone()).map_err(|e| WorkbenchError::VisionPrepare {
        message: format!("VisualIndex: {e}"),
    })?;
    let mut host = rollshot_vision::RealAutomationHost::new();
```

with:

```rust
    let index = VisualIndex::build(image.clone()).map_err(|e| WorkbenchError::VisionPrepare {
        message: format!("VisualIndex: {e}"),
    })?;
    let mut host = rollshot_vision::RealAutomationHost::new();
    prepare_phase_a_region_features(&mut host, &index)?;
```

- [ ] **Step 5: Use the preparation helper in agent dry-run setup**

In `prepare_vision_context`, replace:

```rust
    let host = rollshot_vision::RealAutomationHost::new();
```

with:

```rust
    let mut host = rollshot_vision::RealAutomationHost::new();
    prepare_phase_a_region_features(&mut host, &index)?;
```

- [ ] **Step 6: Add canonical query tests**

In `#[cfg(test)] mod prepare_tests`, add:

```rust
    #[test]
    fn phase_a_region_feature_queries_match_prompt_top_strip() {
        let queries = phase_a_region_feature_queries(160, 120);
        assert!(queries.iter().any(|query| {
            matches!(
                query.region,
                rollshot_automation::Region::Rect { bounds }
                    if bounds.x == 0.0
                        && bounds.y == 0.0
                        && bounds.width == 160.0
                        && bounds.height == 96.0
            )
        }));
    }

    #[test]
    fn phase_a_region_feature_queries_skip_oversized_full_image() {
        let queries = phase_a_region_feature_queries(10_000, 10_000);
        assert!(!queries
            .iter()
            .any(|query| matches!(query.region, rollshot_automation::Region::Full)));
    }

    #[test]
    fn phase_a_region_feature_queries_keep_every_region_under_area_cap() {
        fn query_area(
            query: &rollshot_automation::RegionFeaturesQuery,
            image_width: u32,
            image_height: u32,
        ) -> u64 {
            match query.region {
                rollshot_automation::Region::Full => image_width as u64 * image_height as u64,
                rollshot_automation::Region::Rect { bounds } => {
                    (bounds.width.ceil() as u64).saturating_mul(bounds.height.ceil() as u64)
                }
            }
        }

        let queries = phase_a_region_feature_queries(100_000, 100_000);
        assert!(queries.iter().all(|query| {
            query_area(query, 100_000, 100_000)
                <= PHASE_A_REGION_FEATURE_FULL_AREA_LIMIT
        }));
    }
```

- [ ] **Step 7: Run workbench region-feature tests**

Run:

```bash
rtk cargo test -p rollshot-app run_existing_preset_prepares_top_strip_region_features -- --nocapture
rtk cargo test -p rollshot-app phase_a_region_feature_queries -- --nocapture
rtk cargo test -p rollshot-app prepare_vision_context_succeeds_for_valid_image -- --nocapture
```

Expected: all PASS.

- [ ] **Step 8: Commit Task 3**

Run:

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/run.rs
rtk git commit -m "feat(app): prepare smart redaction region features"
```

Expected: commit succeeds.

## Task 4: Add Bounded Dry-Run Candidate Preview

**Files:**
- Modify: `crates/rollshot-agent/src/tools.rs`

- [ ] **Step 1: Add a failing preview assertion to the dry-run test**

In `crates/rollshot-agent/src/tools.rs`, inside `dry_run_succeeds_with_valid_proposal`, extend the success match arm:

```rust
                let preview = result_json["candidate_preview"].as_array().unwrap();
                assert_eq!(preview.len(), 1);
                assert_eq!(preview[0]["kind"].as_str(), Some("addRedaction"));
                assert_eq!(preview[0]["label"].as_str(), Some("email"));
                assert_eq!(preview[0]["confidence"].as_f64(), Some(0.85));
                assert_eq!(preview[0]["bounds"]["x"].as_f64(), Some(5.0));
                assert_eq!(preview[0]["bounds"]["y"].as_f64(), Some(5.0));
                assert_eq!(preview[0]["bounds"]["width"].as_f64(), Some(20.0));
                assert_eq!(preview[0]["bounds"]["height"].as_f64(), Some(20.0));
```

- [ ] **Step 2: Add a preview cap test**

In the same test module, add:

```rust
    #[tokio::test]
    async fn dry_run_candidate_preview_is_capped() {
        let ctx = test_context(valid_js_source());
        let candidates: Vec<_> = (0..8)
            .map(|i| {
                serde_json::json!({
                    "kind": "addRedaction",
                    "bounds": {"x": i * 2, "y": 0, "width": 1, "height": 1},
                    "confidence": 0.8,
                    "label": format!("candidate-{i}")
                })
            })
            .collect();
        let output = serde_json::json!({ "candidates": candidates });
        let executor = Arc::new(FakeExecutor {
            output_json: serde_json::to_string(&output).unwrap(),
        });
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let tool = DryRunTool::new(ctx, executor, host);

        let result = tool
            .call(&serde_json::json!({"source": valid_js_source(), "generation": 0}))
            .await
            .unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["candidate_count"].as_u64(), Some(8));
                assert_eq!(result_json["candidate_preview"].as_array().unwrap().len(), 5);
            }
            other => panic!("expected success, got {other:?}"),
        }
    }
```

- [ ] **Step 3: Run failing dry-run tests**

Run:

```bash
rtk cargo test -p rollshot-agent dry_run_succeeds_with_valid_proposal -- --nocapture
rtk cargo test -p rollshot-agent dry_run_candidate_preview_is_capped -- --nocapture
```

Expected: FAIL because `candidate_preview` does not exist.

- [ ] **Step 4: Add preview result types**

In `crates/rollshot-agent/src/tools.rs`, replace the current `DryRunResult` with:

```rust
const DRY_RUN_CANDIDATE_PREVIEW_LIMIT: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DryRunCandidatePreview {
    pub kind: String,
    pub bounds: rollshot_image_document::ImageRect,
    pub confidence: f32,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DryRunResult {
    pub candidate_count: u32,
    pub affected_area: f32,
    pub capability_calls: u32,
    pub candidate_preview: Vec<DryRunCandidatePreview>,
}
```

- [ ] **Step 5: Populate preview in `DryRunTool`**

In `DryRunTool::call`, after `affected_area` is computed and before recording evidence, add:

```rust
            let candidate_preview: Vec<DryRunCandidatePreview> = proposal
                .candidates
                .iter()
                .take(DRY_RUN_CANDIDATE_PREVIEW_LIMIT)
                .filter_map(|candidate| match &candidate.edit {
                    rollshot_edit_proposal::ProposedEdit::AddRedaction { bounds } => {
                        Some(DryRunCandidatePreview {
                            kind: "addRedaction".into(),
                            bounds: *bounds,
                            confidence: candidate.confidence,
                            label: candidate.label.clone(),
                        })
                    }
                    _ => None,
                })
                .collect();
```

Then replace the `DryRunResult` construction with:

```rust
                result_json: serde_json::to_value(DryRunResult {
                    candidate_count: proposal.candidates.len() as u32,
                    affected_area,
                    capability_calls,
                    candidate_preview,
                })
                .unwrap_or_default(),
```

- [ ] **Step 6: Run dry-run tests**

Run:

```bash
rtk cargo test -p rollshot-agent dry_run_succeeds_with_valid_proposal -- --nocapture
rtk cargo test -p rollshot-agent dry_run_candidate_preview_is_capped -- --nocapture
rtk cargo test -p rollshot-agent dry_run_reports_affected_area_from_redactions -- --nocapture
```

Expected: all PASS.

- [ ] **Step 7: Commit Task 4**

Run:

```bash
rtk git add crates/rollshot-agent/src/tools.rs
rtk git commit -m "feat(agent): return dry run candidate previews"
```

Expected: commit succeeds.

## Task 5: Full Phase A Verification

**Files:**
- No code changes unless verification exposes a failure.

- [ ] **Step 1: Run focused package tests**

Run:

```bash
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-app result_workspace::workbench
```

Expected: both PASS.

- [ ] **Step 2: Run workspace formatting check**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 3: Run workspace tests**

Run:

```bash
rtk cargo test
```

Expected: PASS.

- [ ] **Step 4: Run workspace clippy**

Task 4 changes the public `rollshot-agent` dry-run result shape, and Task 3 exercises the `rollshot-automation` / `rollshot-vision` capability contract through the app path. Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Confirm git status**

Run:

```bash
rtk git status --short
```

Expected: only unrelated pre-existing untracked files may remain, such as `learn-projects/claude-code-source-code/`.

- [ ] **Step 6: Commit verification-only fixes if any were needed**

If verification required formatting or test-fix changes, commit them:

```bash
rtk git add crates/rollshot-agent/src/driver.rs crates/rollshot-agent/src/tools.rs crates/rollshot-app/src/result_workspace/workbench/run.rs
rtk git commit -m "fix(agent): stabilize smart redaction phase a checks"
```

Expected: commit succeeds when there are staged fixes. If there are no changes, skip this step.

## Spec Coverage

- Authoring guide: Task 1.
- Truthful product registry: Task 2.
- Region-feature capability preparation before QuickJS: Task 3.
- Existing-preset and agent dry-run shared preparation: Task 3 updates both `run_existing_preset` and `prepare_vision_context`.
- Bounded dry-run feedback: Task 4.
- Tests for prompt/tool contract and prepared region-feature path: Tasks 1, 2, 3, and 4.

## Engineering Review Addendum

This section was added by `plan-eng-review` auto mode on 2026-06-27. It is part
of the live implementation plan for Phase A.

### NOT in Scope

- Real OCR/layout inspection tools: Phase A deliberately exposes no stub
  inspection tools in product runs; real tools belong to Phase B.
- Template-handle persistence: `AutomationInput.capability_handles` remains
  empty in the workbench path until Phase F defines handle lifecycle.
- Arbitrary region preparation during QuickJS: the prepare-then-cached-callback
  contract stays intact to avoid hidden expensive work in the JS runtime.
- Giant-image strip fallback beyond the region-feature area cap: Phase A skips
  canonical regions whose area exceeds the existing vision cap rather than
  inventing downsampling or paging.
- Source patch editing: whole-source replacement remains the Phase A authoring
  primitive; smaller code edits are Phase C.
- Broad UI redesign or unattended export: the output remains reviewable draft
  candidates only.

### What Already Exists

- `rollshot-agent::driver::SMART_REDACTION_SYSTEM_PROMPT` already prevents
  upload requests and carries into provider requests; Task 1 expands and tests
  it instead of creating a new prompt channel.
- `rollshot-agent::tools::{replace_source, validate_source, dry_run,
  submit_for_review, request_user_input, inspect_context_summary}` already
  implement the bounded authoring loop; Task 2 locks the product registry to
  these truthful tools.
- `rollshot-agent::tools::DryRunTool` already validates, executes QuickJS, runs
  proposal policy validation, records evidence, and stores the last proposal;
  Task 4 only adds bounded candidate preview fields to the existing result.
- `rollshot-vision::RealAutomationHost::prepare_region_features` already owns
  expensive region-feature preparation; Task 3 reuses it from the workbench
  instead of adding a second vision path.
- `run_existing_preset` and `prepare_vision_context` already centralize the two
  product execution paths that need prepared capabilities; Task 3 keeps the
  fix scoped to those helpers.

### Test Coverage Diagram

```text
User intent
   |
   v
Provider prompt contract -- Task 1 unit/contract test
   |
   v
Workbench tool registry -- Task 2 unit test
   |
   v
Vision prep helpers ---- Task 3 unit tests
   |                         |
   |                         +--> oversize region cap test
   v
QuickJS dry run ------- Task 3 workbench integration test
   |
   v
Dry-run feedback ------ Task 4 unit tests
   |
   v
Full workspace checks - Task 5 fmt/test/clippy
```

| Task / behavior | Unit | Integration | Smoke | Manual only |
|---|---:|---:|---:|---:|
| Task 1 / authoring-guide prompt markers | yes | no | no | no |
| Task 2 / product registry exposes only truthful tools | yes | no | no | no |
| Task 3 / canonical top strip matches prompt example | yes | no | no | no |
| Task 3 / oversized full image is skipped | yes | no | no | no |
| Task 3 / every prepared canonical region stays under cap | yes | no | no | no |
| Task 3 / existing preset prepares region features before QuickJS | no | yes | no | no |
| Task 3 / valid image vision context still builds | yes | no | no | no |
| Task 4 / dry-run preview includes first candidate geometry | yes | no | no | no |
| Task 4 / dry-run preview is capped at five candidates | yes | no | no | no |
| Task 5 / package and workspace verification | no | yes | yes | no |

### Failure Modes

| Codepath | Realistic failure | Covered by | Handling in plan | User-visible result |
|---|---|---|---|---|
| Provider prompt assembly | Prompt loses required JS/API guidance | Task 1 Step 2/4 | Test fails before runtime | Build/test failure, not silent |
| Registry construction | Duplicate or unavailable tool registration | Task 2 Step 1/5 | `build_authoring_tool_registry` returns `WorkbenchError::RuntimeFailure` | RunFailed event, not panic |
| Empty image vision prep | `VisualIndex::build` rejects zero dimensions | Existing tests plus Task 3 Step 7 | `WorkbenchError::VisionPrepare` | RunFailed with vision message |
| Region-feature prep | Requested canonical region exceeds area cap | Task 3 Step 6/7 | Helper skips oversized regions before prepare | Dry run may fail only if JS asks for skipped region |
| Region-feature dry run | JS asks for unprepared region | Task 3 Step 1/7 covers prepared top strip; arbitrary regions are NOT in scope | `execute_to_proposal` maps capability error to runtime failure | Recoverable dry_run error |
| Existing-preset execution | Fresh host lacks prepared region features | Task 3 Step 1/7 | `prepare_phase_a_region_features` runs before QuickJS | Proposal or explicit runtime error |
| Dry-run result size | Too many candidates make tool output noisy | Task 4 Step 2/6 | Preview capped by `DRY_RUN_CANDIDATE_PREVIEW_LIMIT` | Bounded evidence |
| Policy rejection | Candidate area/count violates policy | Existing `dry_run_fails_on_policy_violation` | `ToolError::ArgumentDecode` recoverable failure | Model receives dry_run failure |

No critical silent gaps remain in this Phase A plan. The remaining uncovered
case is arbitrary model-generated region queries outside canonical prepared
regions; that is intentionally explicit runtime feedback, not silent success.

### Subagent and Execution Strategy

Do not create git worktrees for this branch. The project rule forbids worktrees
unless the user explicitly asks for them.

| Task | Modules touched | Depends on |
|---|---|---|
| Task 1: Authoring guide | `crates/rollshot-agent/` | none |
| Task 2: Registry helper | `crates/rollshot-app/` | none |
| Task 3: Region-feature preparation | `crates/rollshot-app/` | Task 2 optional, same file |
| Task 4: Dry-run preview | `crates/rollshot-agent/` | none |
| Task 5: Verification | workspace | Tasks 1-4 |

Parallel lanes:

- Lane A: Task 1, isolated to `crates/rollshot-agent/src/driver.rs`.
- Lane B: Task 2 -> Task 3 sequential, both touch
  `crates/rollshot-app/src/result_workspace/workbench/run.rs`.
- Lane C: Task 4, isolated to `crates/rollshot-agent/src/tools.rs`.
- Lane D: Task 5 after A/B/C land.

Execution order:

1. If using `superpowers:subagent-driven-development`, use subagents for
   isolated implementation/review chunks, but apply and commit changes
   sequentially on this branch.
2. Run Task 2 before Task 3 if both are assigned to the same worker, because
   both edit `run.rs` and Task 3 benefits from the registry helper already
   existing.
3. Run Task 5 only after Tasks 1-4 are committed.

Conflict flags:

- Task 2 and Task 3 both edit `crates/rollshot-app/src/result_workspace/workbench/run.rs`;
  keep them in the same lane.
- Task 1 and Task 4 both touch `crates/rollshot-agent/` but different files;
  merge risk is low, but commits should still be sequential on the shared
  branch.
- No task modifies root `Cargo.toml`; there is no workspace-root serialization
  point.
