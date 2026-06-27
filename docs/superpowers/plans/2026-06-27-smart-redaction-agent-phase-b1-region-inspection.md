# Smart Redaction Agent Phase B1 Region Inspection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build truthful product-visible Smart Redaction inspection tools for image context and prepared canonical region features.

**Architecture:** `rollshot-agent` owns provider-neutral tool contracts and result shapes. `rollshot-app` owns the product workbench canonical-region catalog and passes that bounded context into the agent tools. The QuickJS/runtime path stays unchanged: region features remain prepare-then-cached, and inspection only reads already prepared canonical regions.

**Tech Stack:** Rust, serde/schemars JSON schemas, `rollshot-agent` tool registry, `rollshot-app` Smart Redaction workbench, `rollshot-vision::RealAutomationHost`, existing `rtk cargo test` workflow.

---

## File Structure

- Modify `crates/rollshot-agent/src/tools.rs`
  - Add serializable B1 inspection data types.
  - Add `InspectImageContextTool`.
  - Replace `RegionFeaturesTool` stub behavior with a real host-backed canonical-region tool.
  - Keep `OcrTool` and `LayoutTool` as unavailable stubs for non-product/internal tests.
- Modify `crates/rollshot-agent/src/driver.rs`
  - Update the Smart Redaction authoring guide to call `inspect_image_context` before writing source and use `inspect_region_features` for coarse visual evidence.
  - Update test helpers that register all authoring tools.
- Modify `crates/rollshot-app/src/result_workspace/workbench/run.rs`
  - Replace Phase A query-only helper with a named canonical region catalog.
  - Use the catalog for preparation and for agent inspection context.
  - Register `inspect_image_context` and real `inspect_region_features` in the product registry.
- No new crate is needed. Do not move the catalog into `rollshot-automation`; B1 treats canonical regions as product authoring context, not automation language.

---

### Task 1: Add Agent Inspection Tool Contracts

**Files:**
- Modify: `crates/rollshot-agent/src/tools.rs`

- [ ] **Step 1: Write failing tool schema and image-context tests**

Add these tests inside `#[cfg(test)] pub(crate) mod tests` in `crates/rollshot-agent/src/tools.rs`, near the existing inspection tests:

```rust
#[test]
fn inspect_image_context_schema_is_object() {
    let tool = InspectImageContextTool::new(test_context("source"), inspection_context_for_tests());
    let schema = tool.json_schema();
    assert_eq!(schema["type"].as_str(), Some("object"));
}

#[tokio::test]
async fn inspect_image_context_returns_authoring_and_region_context() {
    let ctx = test_context("hello world");
    let tool = InspectImageContextTool::new(ctx, inspection_context_for_tests());

    let result = tool.call(&serde_json::json!({})).await.unwrap();

    match result {
        ToolOutcome::Success { result_json } => {
            assert_eq!(result_json["image"]["width"].as_u64(), Some(100));
            assert_eq!(result_json["image"]["height"].as_u64(), Some(100));
            assert_eq!(result_json["image"]["payload_mode"].as_str(), Some("full_screenshot"));
            assert_eq!(result_json["source"]["generation"].as_u64(), Some(0));
            assert_eq!(result_json["source"]["source_bytes"].as_u64(), Some(11));
            assert_eq!(result_json["regions"][0]["name"].as_str(), Some("top_strip"));
            assert!(result_json["regions"][0].get("query").is_none());
            assert_eq!(result_json["capabilities"]["region_features"]["status"].as_str(), Some("available"));
            assert_eq!(result_json["capabilities"]["ocr"]["status"].as_str(), Some("unavailable"));
            assert_eq!(result_json["capabilities"]["layout"]["status"].as_str(), Some("unavailable"));
            assert_eq!(result_json["capabilities"]["template_match"]["status"].as_str(), Some("unavailable"));
        }
        other => panic!("expected success, got {other:?}"),
    }
}
```

Add the helper used by those tests in the same test module:

```rust
fn inspection_context_for_tests() -> AuthoringInspectionContext {
    AuthoringInspectionContext {
        payload_mode: "full_screenshot".into(),
        regions: vec![CanonicalRegionInspection {
            name: "top_strip".into(),
            bounds: Some(rollshot_image_document::ImageRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 96.0,
            }),
            query: Some(rollshot_automation::RegionFeaturesQuery {
                region: rollshot_automation::Region::Rect {
                    bounds: rollshot_image_document::ImageRect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 96.0,
                    },
                },
                limit: 1,
            }),
            unavailable_reason: None,
        }],
        ocr_status: CapabilityStatus::unavailable("ocr_disabled"),
        layout_status: CapabilityStatus::unavailable("capability_unavailable"),
        template_match_status: CapabilityStatus::unavailable("no_capability_handles"),
    }
}
```

- [ ] **Step 2: Run image-context tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-agent inspect_image_context -- --nocapture
```

Expected: compile failure mentioning missing `InspectImageContextTool`, `AuthoringInspectionContext`, `CanonicalRegionInspection`, or `CapabilityStatus`.

- [ ] **Step 3: Implement the inspection result data types**

In `crates/rollshot-agent/src/tools.rs`, under `// ---------- Inspection types ----------`, add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuthoringInspectionContext {
    pub payload_mode: String,
    pub regions: Vec<CanonicalRegionInspection>,
    pub ocr_status: CapabilityStatus,
    pub layout_status: CapabilityStatus,
    pub template_match_status: CapabilityStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalRegionInspection {
    pub name: String,
    pub bounds: Option<rollshot_image_document::ImageRect>,
    #[serde(skip)]
    pub query: Option<rollshot_automation::RegionFeaturesQuery>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapabilityStatus {
    pub status: String,
    pub reason: Option<String>,
}

impl CapabilityStatus {
    pub fn available() -> Self {
        Self {
            status: "available".into(),
            reason: None,
        }
    }

    pub fn partial(reason: impl Into<String>) -> Self {
        Self {
            status: "partial".into(),
            reason: Some(reason.into()),
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            status: "unavailable".into(),
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageContextImage {
    pub width: u32,
    pub height: u32,
    pub payload_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageContextSource {
    pub generation: u64,
    pub source_bytes: usize,
    pub evidence_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageContextCapabilities {
    pub region_features: CapabilityStatus,
    pub ocr: CapabilityStatus,
    pub layout: CapabilityStatus,
    pub template_match: CapabilityStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageContextResult {
    pub image: ImageContextImage,
    pub source: ImageContextSource,
    pub regions: Vec<CanonicalRegionInspection>,
    pub capabilities: ImageContextCapabilities,
}
```

- [ ] **Step 4: Implement `InspectImageContextTool`**

Add this concrete tool near `GetContextSummaryTool`:

```rust
pub struct InspectImageContextTool {
    ctx: Arc<ToolContext>,
    inspection: AuthoringInspectionContext,
}

impl InspectImageContextTool {
    pub fn new(ctx: Arc<ToolContext>, inspection: AuthoringInspectionContext) -> Self {
        Self { ctx, inspection }
    }
}

impl Tool for InspectImageContextTool {
    fn name(&self) -> &str {
        "inspect_image_context"
    }

    fn json_schema(&self) -> Value {
        tool_schema::<EmptyArgs>()
    }

    fn call<'a>(&'a self, _arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let draft = self.ctx.draft.lock().unwrap();
            let generation = draft.generation();
            let evidence_count = draft.evidence().len();
            drop(draft);

            let source_bytes = self.ctx.source.lock().unwrap().len();
            let prepared = self
                .inspection
                .regions
                .iter()
                .filter(|region| region.query.is_some())
                .count();
            let skipped = self.inspection.regions.len().saturating_sub(prepared);
            let region_features = if prepared == 0 {
                CapabilityStatus::unavailable("no_prepared_regions")
            } else if skipped > 0 {
                CapabilityStatus::partial("some_regions_unavailable")
            } else {
                CapabilityStatus::available()
            };

            Ok(ToolOutcome::Success {
                result_json: serde_json::to_value(ImageContextResult {
                    image: ImageContextImage {
                        width: self.ctx.image_dims.0,
                        height: self.ctx.image_dims.1,
                        payload_mode: self.inspection.payload_mode.clone(),
                    },
                    source: ImageContextSource {
                        generation,
                        source_bytes,
                        evidence_count,
                    },
                    regions: self.inspection.regions.clone(),
                    capabilities: ImageContextCapabilities {
                        region_features,
                        ocr: self.inspection.ocr_status.clone(),
                        layout: self.inspection.layout_status.clone(),
                        template_match: self.inspection.template_match_status.clone(),
                    },
                })
                .unwrap_or_default(),
            })
        })
    }
}
```

- [ ] **Step 5: Run image-context tests and verify they pass**

Run:

```bash
rtk cargo test -p rollshot-agent inspect_image_context -- --nocapture
```

Expected: tests whose names contain `inspect_image_context` pass.

- [ ] **Step 6: Commit Task 1**

```bash
rtk git add crates/rollshot-agent/src/tools.rs
rtk git commit -m "feat(agent): add smart redaction image context tool"
```

---

### Task 2: Implement Host-Backed Canonical Region Inspection

**Files:**
- Modify: `crates/rollshot-agent/src/tools.rs`

- [ ] **Step 1: Write failing region-feature inspection tests**

Replace the existing `region_features_returns_unavailable` test with these tests:

```rust
#[test]
fn inspect_region_features_schema_is_object() {
    let ctx = test_context("source");
    let host = Arc::new(Mutex::new(
        rollshot_automation::FakeAutomationHost::default(),
    ));
    let tool = RegionFeaturesTool::new(ctx, host, inspection_context_for_tests().regions);
    let schema = tool.json_schema();
    assert_eq!(schema["type"].as_str(), Some("object"));
}

#[tokio::test]
async fn inspect_region_features_rejects_unknown_region() {
    let ctx = test_context("source");
    let host = Arc::new(Mutex::new(
        rollshot_automation::FakeAutomationHost::default(),
    ));
    let tool = RegionFeaturesTool::new(ctx, host, inspection_context_for_tests().regions);

    let err = tool
        .call(&serde_json::json!({"region": "custom_rect"}))
        .await
        .unwrap_err();

    assert!(matches!(err, ToolError::ArgumentDecode(_)));
}

#[tokio::test]
async fn inspect_region_features_returns_prepared_feature_summary() {
    let ctx = test_context("source");
    let host = Arc::new(Mutex::new(rollshot_automation::FakeAutomationHost {
        region_feature_results: vec![rollshot_automation::RegionFeatures {
            bounds: rollshot_image_document::ImageRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 96.0,
            },
            dominant_rgba: [10, 20, 30, 255],
            edge_density: 0.25,
        }],
        ..Default::default()
    }));
    let tool = RegionFeaturesTool::new(ctx, host, inspection_context_for_tests().regions);

    let result = tool
        .call(&serde_json::json!({"region": "top_strip"}))
        .await
        .unwrap();

    match result {
        ToolOutcome::Success { result_json } => {
            assert_eq!(result_json["region"].as_str(), Some("top_strip"));
            assert_eq!(result_json["status"].as_str(), Some("available"));
            assert_eq!(result_json["features"].as_array().unwrap().len(), 1);
            assert_eq!(result_json["features"][0]["dominant_rgba"][0].as_u64(), Some(10));
            assert_eq!(result_json["features"][0]["edge_density"].as_f64(), Some(0.25));
        }
        other => panic!("expected success, got {other:?}"),
    }
}

#[tokio::test]
async fn inspect_region_features_returns_unavailable_for_skipped_region() {
    let ctx = test_context("source");
    let host = Arc::new(Mutex::new(
        rollshot_automation::FakeAutomationHost::default(),
    ));
    let regions = vec![CanonicalRegionInspection {
        name: "full".into(),
        bounds: Some(rollshot_image_document::ImageRect {
            x: 0.0,
            y: 0.0,
            width: 100_000.0,
            height: 100_000.0,
        }),
        query: None,
        unavailable_reason: Some("area_limit_exceeded".into()),
    }];
    let tool = RegionFeaturesTool::new(ctx, host, regions);

    let result = tool
        .call(&serde_json::json!({"region": "full"}))
        .await
        .unwrap();

    match result {
        ToolOutcome::Success { result_json } => {
            assert_eq!(result_json["status"].as_str(), Some("unavailable"));
            assert_eq!(result_json["unavailable_reason"].as_str(), Some("area_limit_exceeded"));
            assert!(result_json["features"].as_array().unwrap().is_empty());
        }
        other => panic!("expected success, got {other:?}"),
    }
}

#[tokio::test]
async fn inspect_region_features_converts_host_error_to_unavailable() {
    let ctx = test_context("source");
    let host = Arc::new(Mutex::new(rollshot_automation::FakeAutomationHost {
        failure: Some(rollshot_automation::CapabilityError::Failed {
            code: "vision_index_unavailable",
        }),
        ..Default::default()
    }));
    let tool = RegionFeaturesTool::new(ctx, host, inspection_context_for_tests().regions);

    let result = tool
        .call(&serde_json::json!({"region": "top_strip"}))
        .await
        .unwrap();

    match result {
        ToolOutcome::Success { result_json } => {
            assert_eq!(result_json["status"].as_str(), Some("unavailable"));
            assert_eq!(result_json["unavailable_reason"].as_str(), Some("vision_index_unavailable"));
            assert!(result_json["features"].as_array().unwrap().is_empty());
        }
        other => panic!("expected success, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run region-feature tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-agent inspect_region_features -- --nocapture
```

Expected: compile failure because `RegionFeaturesTool::new` still takes no context/host/regions and argument/result structs do not exist.

- [ ] **Step 3: Add argument/result types and capability-error conversion**

In `crates/rollshot-agent/src/tools.rs`, add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectRegionFeaturesArgs {
    pub region: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionFeatureSummary {
    pub bounds: rollshot_image_document::ImageRect,
    pub dominant_rgba: [u8; 4],
    pub edge_density: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectRegionFeaturesResult {
    pub region: String,
    pub status: String,
    pub bounds: Option<rollshot_image_document::ImageRect>,
    pub features: Vec<RegionFeatureSummary>,
    pub unavailable_reason: Option<String>,
}

fn capability_error_code(error: rollshot_automation::CapabilityError) -> String {
    match error {
        rollshot_automation::CapabilityError::InvalidInput { code } => code.into(),
        rollshot_automation::CapabilityError::LimitExceeded => "limit_exceeded".into(),
        rollshot_automation::CapabilityError::Failed { code } => code.into(),
    }
}
```

- [ ] **Step 4: Replace `RegionFeaturesTool` stub with host-backed implementation**

Replace the current `RegionFeaturesTool` struct and impl with:

```rust
pub struct RegionFeaturesTool {
    _ctx: Arc<ToolContext>,
    host: Arc<Mutex<dyn rollshot_automation::AutomationHost>>,
    regions: Vec<CanonicalRegionInspection>,
}

impl RegionFeaturesTool {
    pub fn new(
        ctx: Arc<ToolContext>,
        host: Arc<Mutex<dyn rollshot_automation::AutomationHost>>,
        regions: Vec<CanonicalRegionInspection>,
    ) -> Self {
        Self {
            _ctx: ctx,
            host,
            regions,
        }
    }
}

impl Tool for RegionFeaturesTool {
    fn name(&self) -> &str {
        "inspect_region_features"
    }

    fn json_schema(&self) -> Value {
        tool_schema::<InspectRegionFeaturesArgs>()
    }

    fn call<'a>(&'a self, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: InspectRegionFeaturesArgs = serde_json::from_value(arguments.clone())
                .map_err(|e| ToolError::ArgumentDecode(e.to_string()))?;
            let region = self
                .regions
                .iter()
                .find(|region| region.name == args.region)
                .ok_or_else(|| ToolError::ArgumentDecode(format!("unknown canonical region: {}", args.region)))?;

            let Some(query) = region.query.clone() else {
                return Ok(ToolOutcome::Success {
                    result_json: serde_json::to_value(InspectRegionFeaturesResult {
                        region: region.name.clone(),
                        status: "unavailable".into(),
                        bounds: region.bounds,
                        features: Vec::new(),
                        unavailable_reason: region.unavailable_reason.clone().or_else(|| Some("region_unavailable".into())),
                    })
                    .unwrap_or_default(),
                });
            };

            let features = {
                let mut host = self.host.lock().unwrap();
                host.region_features(query)
            };

            match features {
                Ok(features) => {
                    let summaries = features
                        .into_iter()
                        .take(1)
                        .map(|feature| RegionFeatureSummary {
                            bounds: feature.bounds,
                            dominant_rgba: feature.dominant_rgba,
                            edge_density: feature.edge_density,
                        })
                        .collect();
                    Ok(ToolOutcome::Success {
                        result_json: serde_json::to_value(InspectRegionFeaturesResult {
                            region: region.name.clone(),
                            status: "available".into(),
                            bounds: region.bounds,
                            features: summaries,
                            unavailable_reason: None,
                        })
                        .unwrap_or_default(),
                    })
                }
                Err(error) => Ok(ToolOutcome::Success {
                    result_json: serde_json::to_value(InspectRegionFeaturesResult {
                        region: region.name.clone(),
                        status: "unavailable".into(),
                        bounds: region.bounds,
                        features: Vec::new(),
                        unavailable_reason: Some(capability_error_code(error)),
                    })
                    .unwrap_or_default(),
                }),
            }
        })
    }
}
```

- [ ] **Step 5: Run region-feature tests and verify they pass**

Run:

```bash
rtk cargo test -p rollshot-agent inspect_region_features -- --nocapture
```

Expected: all tests whose names contain `inspect_region_features` pass.

- [ ] **Step 6: Commit Task 2**

```bash
rtk git add crates/rollshot-agent/src/tools.rs
rtk git commit -m "feat(agent): inspect prepared region features"
```

---

### Task 3: Add Workbench Canonical Region Catalog and Registry Wiring

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`

- [ ] **Step 1: Write failing workbench catalog and registry tests**

In `mod prepare_tests` in `crates/rollshot-app/src/result_workspace/workbench/run.rs`, replace the `phase_a_region_feature_queries_*` tests and update `authoring_registry_exposes_only_truthful_phase_a_tools`.

Use these tests:

```rust
#[test]
fn canonical_region_catalog_matches_prompt_top_strip() {
    let catalog = canonical_region_feature_catalog(160, 120);
    let top = catalog
        .iter()
        .find(|entry| entry.name == "top_strip")
        .expect("top strip entry");
    assert_eq!(
        top.bounds,
        rollshot_image_document::ImageRect {
            x: 0.0,
            y: 0.0,
            width: 160.0,
            height: 96.0,
        }
    );
    assert!(top.query.is_some());
    assert_eq!(top.unavailable_reason, None);
}

#[test]
fn canonical_region_catalog_keeps_skipped_full_region_with_reason() {
    let catalog = canonical_region_feature_catalog(10_000, 10_000);
    let full = catalog
        .iter()
        .find(|entry| entry.name == "full")
        .expect("full entry");
    assert_eq!(full.query, None);
    assert_eq!(full.unavailable_reason, Some("area_limit_exceeded"));
}

#[test]
fn canonical_region_catalog_has_named_entries_for_every_region() {
    let names: Vec<&str> = canonical_region_feature_catalog(160, 120)
        .iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(
        names,
        vec!["full", "top_strip", "left_strip", "right_strip", "bottom_strip"]
    );
}

#[test]
fn authoring_registry_exposes_truthful_phase_b1_tools() {
    let ctx = tool_context_for_tests();
    let executor: std::sync::Arc<dyn rollshot_automation::AutomationExecutor> =
        std::sync::Arc::new(rollshot_automation_rquickjs::QuickJsExecutor);
    let host: std::sync::Arc<std::sync::Mutex<dyn rollshot_automation::AutomationHost>> =
        std::sync::Arc::new(std::sync::Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
    let inspection = authoring_inspection_context(
        PayloadMode::FullScreenshot,
        &canonical_region_feature_catalog(64, 64),
    );

    let registry = build_authoring_tool_registry(ctx, executor, host, inspection).unwrap();
    let names = registry.tool_names();

    assert_eq!(
        names,
        vec![
            "replace_source",
            "validate_source",
            "submit_for_review",
            "request_user_input",
            "inspect_context_summary",
            "inspect_image_context",
            "inspect_region_features",
            "dry_run",
        ]
    );
    assert!(!names.contains(&"inspect_ocr"));
    assert!(!names.contains(&"inspect_layout"));
}
```

- [ ] **Step 2: Run workbench tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::workbench::run::prepare_tests -- --nocapture
```

Expected: compile failure mentioning missing `canonical_region_feature_catalog`, `authoring_inspection_context`, and updated `build_authoring_tool_registry` signature.

- [ ] **Step 3: Add the named catalog type and builder**

In `crates/rollshot-app/src/result_workspace/workbench/run.rs`, replace `phase_a_region_feature_queries` with:

```rust
#[derive(Debug, Clone, PartialEq)]
struct CanonicalRegionFeatureEntry {
    name: &'static str,
    bounds: rollshot_image_document::ImageRect,
    query: Option<rollshot_automation::RegionFeaturesQuery>,
    unavailable_reason: Option<&'static str>,
}

fn canonical_region_feature_catalog(width: u32, height: u32) -> Vec<CanonicalRegionFeatureEntry> {
    use rollshot_automation::{Region, RegionFeaturesQuery};
    use rollshot_image_document::ImageRect;

    if width == 0 || height == 0 {
        return Vec::new();
    }

    let width_f = width as f32;
    let height_f = height as f32;
    let strip_h = height.min(PHASE_A_REGION_FEATURE_STRIP_PX) as f32;
    let strip_w = width.min(PHASE_A_REGION_FEATURE_STRIP_PX) as f32;

    let make_entry = |name: &'static str, bounds: ImageRect| {
        let area = (bounds.width.ceil() as u64).saturating_mul(bounds.height.ceil() as u64);
        if area > PHASE_A_REGION_FEATURE_FULL_AREA_LIMIT {
            CanonicalRegionFeatureEntry {
                name,
                bounds,
                query: None,
                unavailable_reason: Some("area_limit_exceeded"),
            }
        } else {
            CanonicalRegionFeatureEntry {
                name,
                bounds,
                query: Some(RegionFeaturesQuery {
                    region: Region::Rect { bounds },
                    limit: PHASE_A_REGION_FEATURE_LIMIT,
                }),
                unavailable_reason: None,
            }
        }
    };

    vec![
        CanonicalRegionFeatureEntry {
            name: "full",
            bounds: ImageRect {
                x: 0.0,
                y: 0.0,
                width: width_f,
                height: height_f,
            },
            query: if (width as u64 * height as u64) <= PHASE_A_REGION_FEATURE_FULL_AREA_LIMIT {
                Some(RegionFeaturesQuery {
                    region: Region::Full,
                    limit: PHASE_A_REGION_FEATURE_LIMIT,
                })
            } else {
                None
            },
            unavailable_reason: if (width as u64 * height as u64) <= PHASE_A_REGION_FEATURE_FULL_AREA_LIMIT {
                None
            } else {
                Some("area_limit_exceeded")
            },
        },
        make_entry(
            "top_strip",
            ImageRect {
                x: 0.0,
                y: 0.0,
                width: width_f,
                height: strip_h,
            },
        ),
        make_entry(
            "left_strip",
            ImageRect {
                x: 0.0,
                y: 0.0,
                width: strip_w,
                height: height_f,
            },
        ),
        make_entry(
            "right_strip",
            ImageRect {
                x: (width_f - strip_w).max(0.0),
                y: 0.0,
                width: strip_w,
                height: height_f,
            },
        ),
        make_entry(
            "bottom_strip",
            ImageRect {
                x: 0.0,
                y: (height_f - strip_h).max(0.0),
                width: width_f,
                height: strip_h,
            },
        ),
    ]
}
```

- [ ] **Step 4: Update preparation to iterate only query entries**

Replace `prepare_phase_a_region_features` with:

```rust
fn prepare_phase_a_region_features(
    host: &mut rollshot_vision::RealAutomationHost,
    index: &VisualIndex,
) -> Result<(), WorkbenchError> {
    for entry in canonical_region_feature_catalog(index.width(), index.height()) {
        let Some(query) = entry.query else {
            continue;
        };
        host.prepare_region_features(index, &query)
            .map_err(|e| WorkbenchError::VisionPrepare {
                message: format!("regionFeatures {}: {e}", entry.name),
            })?;
    }
    Ok(())
}
```

- [ ] **Step 5: Build the agent inspection context from the catalog**

Add this helper near the catalog:

```rust
fn authoring_inspection_context(
    payload_mode: PayloadMode,
    catalog: &[CanonicalRegionFeatureEntry],
) -> rollshot_agent::tools::AuthoringInspectionContext {
    let regions = catalog
        .iter()
        .map(|entry| rollshot_agent::tools::CanonicalRegionInspection {
            name: entry.name.into(),
            bounds: Some(entry.bounds),
            query: entry.query.clone(),
            unavailable_reason: entry.unavailable_reason.map(str::to_string),
        })
        .collect();

    let payload_mode = match payload_mode {
        PayloadMode::FullScreenshot => "full_screenshot",
        PayloadMode::OcrLayoutOnly => "ocr_layout_only",
    };

    rollshot_agent::tools::AuthoringInspectionContext {
        payload_mode: payload_mode.into(),
        regions,
        ocr_status: rollshot_agent::tools::CapabilityStatus::unavailable("ocr_disabled"),
        layout_status: rollshot_agent::tools::CapabilityStatus::unavailable("capability_unavailable"),
        template_match_status: rollshot_agent::tools::CapabilityStatus::unavailable("no_capability_handles"),
    }
}
```

- [ ] **Step 6: Update registry signature and product call site**

Change `build_authoring_tool_registry` to accept the inspection context:

```rust
fn build_authoring_tool_registry(
    tool_ctx: Arc<rollshot_agent::tools::ToolContext>,
    executor: Arc<dyn rollshot_automation::AutomationExecutor>,
    host: Arc<StdMutex<dyn rollshot_automation::AutomationHost>>,
    inspection: rollshot_agent::tools::AuthoringInspectionContext,
) -> Result<rollshot_agent::tools::ToolRegistry, WorkbenchError> {
```

Update imports inside the function to include the B1 tools:

```rust
use rollshot_agent::tools::{
    DryRunTool, GetContextSummaryTool, InspectImageContextTool, RegionFeaturesTool,
    ReplaceSourceTool, RequestUserInputTool, SubmitForReviewTool, ToolRegistry,
    ToolRegistryLimits, ValidateSourceTool,
};
```

Register the tools between `inspect_context_summary` and `dry_run`:

```rust
reg(
    &mut registry,
    Arc::new(InspectImageContextTool::new(tool_ctx.clone(), inspection.clone())),
)?;
reg(
    &mut registry,
    Arc::new(RegionFeaturesTool::new(
        tool_ctx.clone(),
        host.clone(),
        inspection.regions.clone(),
    )),
)?;
```

In `start_agent_run`, build and pass the context before calling the registry builder:

```rust
let inspection = authoring_inspection_context(
    payload_mode,
    &canonical_region_feature_catalog(image_dims.0, image_dims.1),
);
let registry = match build_authoring_tool_registry(
    tool_ctx.clone(),
    Arc::new(vision.executor),
    vision.host.clone() as Arc<StdMutex<dyn rollshot_automation::AutomationHost>>,
    inspection,
) {
```

Update every test call to `build_authoring_tool_registry` to pass an inspection context.

- [ ] **Step 7: Run workbench tests and verify they pass**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::workbench::run::prepare_tests -- --nocapture
```

Expected: all `prepare_tests` pass.

- [ ] **Step 8: Commit Task 3**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/run.rs
rtk git commit -m "feat(app): expose smart redaction region inspection tools"
```

---

### Task 4: Update Prompt and Driver Tool Registration Tests

**Files:**
- Modify: `crates/rollshot-agent/src/driver.rs`
- Modify: `crates/rollshot-agent/src/tools.rs`

- [ ] **Step 1: Write failing prompt assertions**

In `crates/rollshot-agent/src/driver.rs`, locate `smart_redaction_system_prompt_contains_authoring_guide` or the existing prompt-content test. Add these assertions:

```rust
assert!(prompt.contains("Call inspect_image_context before writing or replacing source"));
assert!(prompt.contains("Use inspect_region_features with canonical regions"));
assert!(prompt.contains("full, top_strip, left_strip, right_strip, bottom_strip"));
```

- [ ] **Step 2: Update test helper registration in driver tests**

In `register_all_tools`, import and register the new tools so provider-contract tests can include the B1 schemas:

```rust
use crate::tools::{
    DryRunTool, GetContextSummaryTool, InspectImageContextTool, RegionFeaturesTool,
    ReplaceSourceTool, SubmitForReviewTool, ToolRegistryLimits, ValidateSourceTool,
};
```

Inside `register_all_tools`, create one inspection context and pass cloned regions to the region tool:

```rust
let inspection = crate::tools::AuthoringInspectionContext {
    payload_mode: "full_screenshot".into(),
    regions: vec![crate::tools::CanonicalRegionInspection {
        name: "top_strip".into(),
        bounds: Some(rollshot_image_document::ImageRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 96.0,
        }),
        query: Some(rollshot_automation::RegionFeaturesQuery {
            region: rollshot_automation::Region::Rect {
                bounds: rollshot_image_document::ImageRect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 96.0,
                },
            },
            limit: 1,
        }),
        unavailable_reason: None,
    }],
    ocr_status: crate::tools::CapabilityStatus::unavailable("ocr_disabled"),
    layout_status: crate::tools::CapabilityStatus::unavailable("capability_unavailable"),
    template_match_status: crate::tools::CapabilityStatus::unavailable("no_capability_handles"),
};
reg.register(Arc::new(InspectImageContextTool::new(
    ctx.clone(),
    inspection.clone(),
)))
.unwrap();
reg.register(Arc::new(RegionFeaturesTool::new(
    ctx.clone(),
    host.clone(),
    inspection.regions,
)))
.unwrap();
```

If another driver test manually constructs a `RegionFeaturesTool::new()` stub, update it to use the same concrete context/host/regions pattern. Do not reintroduce product-visible OCR/layout stubs.

- [ ] **Step 3: Run prompt and driver schema tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-agent smart_redaction_system_prompt_contains_authoring_guide -- --nocapture
rtk cargo test -p rollshot-agent second_turn_request_carries_history_and_tool_schemas -- --nocapture
```

Expected before implementation: prompt assertion failure for the first command, and compile or schema expectation failure for the second if helper registration still needs updates.

- [ ] **Step 4: Update the authoring guide**

In `SMART_REDACTION_SYSTEM_PROMPT`, insert this guidance before the existing authoring loop:

```text
Inspection loop:
1. Call inspect_image_context before writing or replacing source.
2. Use inspect_region_features with canonical regions when coarse visual evidence is needed.
3. Valid canonical regions are full, top_strip, left_strip, right_strip, bottom_strip.
4. Do not ask for raw pixels or custom crop inspection; use dry_run to verify source behavior.
```

- [ ] **Step 5: Run agent tests for B1 prompt/tool contracts**

Run:

```bash
rtk cargo test -p rollshot-agent inspect_image_context -- --nocapture
rtk cargo test -p rollshot-agent inspect_region_features -- --nocapture
rtk cargo test -p rollshot-agent smart_redaction_system_prompt_contains_authoring_guide -- --nocapture
rtk cargo test -p rollshot-agent second_turn_request_carries_history_and_tool_schemas -- --nocapture
```

Expected: all four commands pass.

- [ ] **Step 6: Commit Task 4**

```bash
rtk git add crates/rollshot-agent/src/tools.rs crates/rollshot-agent/src/driver.rs
rtk git commit -m "feat(agent): guide smart redaction inspection loop"
```

---

### Task 5: End-to-End B1 Verification and Cleanup

**Files:**
- Modify only files touched by previous tasks if compile/test output reveals small integration fixes.

- [ ] **Step 1: Run narrow B1 verification**

Run:

```bash
rtk cargo test -p rollshot-agent inspect_image_context -- --nocapture
rtk cargo test -p rollshot-agent inspect_region_features -- --nocapture
rtk cargo test -p rollshot-app result_workspace::workbench::run::prepare_tests -- --nocapture
```

Expected: all commands pass with zero failing tests.

- [ ] **Step 2: Run package verification**

Run:

```bash
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-app result_workspace::workbench
rtk cargo fmt --check
```

Expected: `rollshot-agent` tests pass, workbench-scoped `rollshot-app` tests pass, and rustfmt reports no diffs.

- [ ] **Step 3: Inspect product registry exclusions**

Run:

```bash
rtk rg -n "inspect_ocr|inspect_layout|inspect_image_context|inspect_region_features" crates/rollshot-app/src/result_workspace/workbench/run.rs
```

Expected: `inspect_image_context` and `inspect_region_features` appear in the registry/test assertions; `inspect_ocr` and `inspect_layout` appear only in negative assertions, not in registration calls.

- [ ] **Step 4: Check git diff hygiene**

Run:

```bash
rtk git diff --check
rtk git status --short
```

Expected: no whitespace errors. Status shows only intended tracked modifications plus any pre-existing untracked `learn-projects/claude-code-source-code/`.

- [ ] **Step 5: Commit verification fixes if needed**

If Step 1 or Step 2 required integration fixes, commit only those fixes:

```bash
rtk git add crates/rollshot-agent/src/tools.rs crates/rollshot-agent/src/driver.rs crates/rollshot-app/src/result_workspace/workbench/run.rs
rtk git commit -m "fix(agent): stabilize smart redaction region inspection"
```

If Step 1 and Step 2 pass without additional tracked changes, skip this commit.

---

## Final Acceptance Checklist

- `inspect_image_context` is registered in product Smart Redaction runs.
- `inspect_region_features` is registered in product Smart Redaction runs and accepts only canonical names.
- `inspect_ocr` and `inspect_layout` remain out of the product registry.
- Canonical region preparation and inspection use the same catalog.
- Oversized canonical regions return structured unavailable results.
- Prompt tells the model to inspect context before writing source.
- No raw pixels, thumbnails, OCR text, or arbitrary crop inspection are exposed.
- Verification commands in Task 5 pass.
