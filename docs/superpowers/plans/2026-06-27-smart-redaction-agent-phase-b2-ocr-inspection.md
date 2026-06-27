# Smart Redaction Agent Phase B2 OCR Inspection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add truthful OCR inspection to Smart Redaction authoring runs, with full OCR text exposed through `inspect_ocr` only when OCR is compiled and prepared.

**Architecture:** Extend the B1 prepared-inspection pattern with a separate OCR catalog in the workbench and separate OCR inspection metadata in `rollshot-agent`. The agent tool contract is testable with `FakeAutomationHost`; product registration and real OCR preparation are compile-gated behind a new `rollshot-app/ocr` feature forwarding to `rollshot-vision/ocr`.

**Tech Stack:** Rust, Cargo features, `rollshot-agent` tool registry, `rollshot-app` workbench, `rollshot-automation::AutomationHost`, `rollshot-vision::RealAutomationHost`, `schemars`, `serde_json`, Tokio tests.

---

## File Structure

- Modify `crates/rollshot-agent/src/tools.rs`
  - Add OCR inspection context entries separate from B1 region-feature entries.
  - Replace the unavailable OCR stub with a prepared-host `inspect_ocr` tool.
  - Extend `inspect_image_context` output with `ocr_regions`.
  - Add focused agent tool tests with `FakeAutomationHost`.
- Modify `crates/rollshot-agent/src/driver.rs`
  - Update Smart Redaction prompt guidance for OCR inspection.
  - Update driver test fixtures that construct `AuthoringInspectionContext`.
  - Add prompt/tool-schema assertions for `inspect_ocr`.
- Modify `crates/rollshot-app/Cargo.toml`
  - Add `ocr = ["rollshot-vision/ocr"]`.
- Modify `crates/rollshot-app/src/result_workspace/workbench/run.rs`
  - Add workbench-owned canonical OCR catalog.
  - Prepare OCR queries behind `#[cfg(feature = "ocr")]`.
  - Pass OCR entries into agent inspection context.
  - Register `inspect_ocr` only behind `#[cfg(feature = "ocr")]`.
  - Add default and OCR-enabled registry/catalog tests.

No other files are required for this phase.

---

### Task 1: Agent OCR Tool Contract Tests

**Files:**
- Modify: `crates/rollshot-agent/src/tools.rs`
- Test: `crates/rollshot-agent/src/tools.rs`

- [ ] **Step 1: Add failing tests for OCR inspection context and tool behavior**

In `crates/rollshot-agent/src/tools.rs`, inside `#[cfg(test)] pub(crate) mod tests`, replace the existing `ocr_returns_unavailable` test with the following tests and update `inspection_context_for_tests()` as shown.

```rust
    #[test]
    fn inspect_ocr_schema_is_object() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let tool = OcrTool::new(ctx, host, inspection_context_for_tests().ocr_regions);
        let schema = tool.json_schema();
        assert_eq!(schema["type"].as_str(), Some("object"));
    }

    #[test]
    fn inspect_ocr_schema_advertises_canonical_regions() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let tool = OcrTool::new(ctx, host, inspection_context_for_tests().ocr_regions);
        let schema = tool.json_schema().to_string();
        for name in [
            "full",
            "top_strip",
            "left_strip",
            "right_strip",
            "bottom_strip",
        ] {
            assert!(
                schema.contains(name),
                "schema should advertise canonical OCR region {name}, got: {schema}"
            );
        }
    }

    #[tokio::test]
    async fn inspect_ocr_rejects_unknown_region() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let tool = OcrTool::new(ctx, host, inspection_context_for_tests().ocr_regions);

        let err = tool
            .call(&serde_json::json!({"region": "custom_rect"}))
            .await
            .unwrap_err();

        assert!(matches!(err, ToolError::ArgumentDecode(_)));
    }

    #[tokio::test]
    async fn inspect_ocr_returns_full_text_bounds_and_confidence() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(rollshot_automation::FakeAutomationHost {
            ocr_results: vec![rollshot_automation::OcrMatch {
                bounds: rollshot_image_document::ImageRect {
                    x: 10.0,
                    y: 20.0,
                    width: 120.0,
                    height: 24.0,
                },
                text: "alice@example.com".into(),
                confidence: 0.92,
            }],
            ..Default::default()
        }));
        let tool = OcrTool::new(ctx, host, inspection_context_for_tests().ocr_regions);

        let result = tool
            .call(&serde_json::json!({"region": "full"}))
            .await
            .unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["region"].as_str(), Some("full"));
                assert_eq!(result_json["status"].as_str(), Some("available"));
                assert_eq!(result_json["matches"].as_array().unwrap().len(), 1);
                assert_eq!(
                    result_json["matches"][0]["text"].as_str(),
                    Some("alice@example.com")
                );
                assert_eq!(
                    result_json["matches"][0]["bounds"]["x"].as_f64(),
                    Some(10.0)
                );
                assert_eq!(
                    result_json["matches"][0]["confidence"].as_f64(),
                    Some(0.9200000166893005)
                );
                assert!(result_json["unavailable_reason"].is_null());
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn inspect_ocr_returns_unavailable_for_skipped_region() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(
            rollshot_automation::FakeAutomationHost::default(),
        ));
        let regions = vec![CanonicalOcrInspection {
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
        let tool = OcrTool::new(ctx, host, regions);

        let result = tool
            .call(&serde_json::json!({"region": "full"}))
            .await
            .unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["status"].as_str(), Some("unavailable"));
                assert_eq!(
                    result_json["unavailable_reason"].as_str(),
                    Some("area_limit_exceeded")
                );
                assert!(result_json["matches"].as_array().unwrap().is_empty());
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn inspect_ocr_converts_host_error_to_unavailable() {
        let ctx = test_context("source");
        let host = Arc::new(Mutex::new(rollshot_automation::FakeAutomationHost {
            failure: Some(rollshot_automation::CapabilityError::Failed {
                code: "vision_index_unavailable",
            }),
            ..Default::default()
        }));
        let tool = OcrTool::new(ctx, host, inspection_context_for_tests().ocr_regions);

        let result = tool
            .call(&serde_json::json!({"region": "full"}))
            .await
            .unwrap();

        match result {
            ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["status"].as_str(), Some("unavailable"));
                assert_eq!(
                    result_json["unavailable_reason"].as_str(),
                    Some("vision_index_unavailable")
                );
                assert!(result_json["matches"].as_array().unwrap().is_empty());
            }
            other => panic!("expected success, got {other:?}"),
        }
    }
```

Update `inspection_context_for_tests()` so the returned `AuthoringInspectionContext` includes `ocr_regions`:

```rust
            ocr_regions: vec![CanonicalOcrInspection {
                name: "full".into(),
                bounds: Some(rollshot_image_document::ImageRect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                }),
                query: Some(rollshot_automation::OcrQuery {
                    region: rollshot_automation::Region::Full,
                    limit: 50,
                }),
                unavailable_reason: None,
            }],
```

Also extend `inspect_image_context_returns_authoring_and_region_context()` with OCR region and capability assertions:

```rust
                assert_eq!(
                    result_json["ocr_regions"][0]["name"].as_str(),
                    Some("full")
                );
                assert!(result_json["ocr_regions"][0].get("query").is_none());
                assert_eq!(
                    result_json["capabilities"]["ocr"]["status"].as_str(),
                    Some("available")
                );
```

- [ ] **Step 2: Run tests to verify they fail before implementation**

Run:

```bash
rtk cargo test -p rollshot-agent inspect_ocr
rtk cargo test -p rollshot-agent inspect_image_context_returns_authoring_and_region_context
```

Expected: FAIL to compile with missing `CanonicalOcrInspection`, missing `ocr_regions`, and `OcrTool::new` signature mismatch.

- [ ] **Step 3: Commit the failing contract tests**

```bash
rtk git add crates/rollshot-agent/src/tools.rs
rtk git commit -m "test(agent): define smart redaction OCR inspection contract"
```

---

### Task 2: Implement Agent OCR Inspection Tool

**Files:**
- Modify: `crates/rollshot-agent/src/tools.rs`
- Test: `crates/rollshot-agent/src/tools.rs`

- [ ] **Step 1: Add OCR inspection context/result types**

In `crates/rollshot-agent/src/tools.rs`, update the inspection types near `AuthoringInspectionContext`:

```rust
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AuthoringInspectionContext {
    pub payload_mode: String,
    pub regions: Vec<CanonicalRegionInspection>,
    pub ocr_regions: Vec<CanonicalOcrInspection>,
    pub ocr_status: CapabilityStatus,
    pub layout_status: CapabilityStatus,
    pub template_match_status: CapabilityStatus,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CanonicalOcrInspection {
    pub name: String,
    pub bounds: Option<rollshot_image_document::ImageRect>,
    #[serde(skip_serializing)]
    pub query: Option<rollshot_automation::OcrQuery>,
    pub unavailable_reason: Option<String>,
}
```

Add OCR result types near `InspectRegionFeaturesResult`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct OcrMatchSummary {
    pub bounds: rollshot_image_document::ImageRect,
    pub text: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectOcrResult {
    pub region: String,
    pub status: String,
    pub bounds: Option<rollshot_image_document::ImageRect>,
    pub matches: Vec<OcrMatchSummary>,
    pub unavailable_reason: Option<String>,
}
```

- [ ] **Step 2: Extend image context result with OCR regions**

Add `ocr_regions` to `ImageContextResult`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ImageContextResult {
    pub image: ImageContextImage,
    pub source: ImageContextSource,
    pub regions: Vec<CanonicalRegionInspection>,
    pub ocr_regions: Vec<CanonicalOcrInspection>,
    pub capabilities: ImageContextCapabilities,
}
```

In `InspectImageContextTool::call`, compute OCR status from `self.inspection.ocr_regions`:

```rust
            let ocr_prepared = self
                .inspection
                .ocr_regions
                .iter()
                .filter(|region| region.query.is_some())
                .count();
            let ocr_skipped = self
                .inspection
                .ocr_regions
                .len()
                .saturating_sub(ocr_prepared);
            let ocr = if self.inspection.ocr_regions.is_empty() {
                self.inspection.ocr_status.clone()
            } else if ocr_prepared == 0 {
                CapabilityStatus::unavailable("no_prepared_ocr_regions")
            } else if ocr_skipped > 0 {
                CapabilityStatus::partial("some_ocr_regions_unavailable")
            } else {
                CapabilityStatus::available()
            };
```

Use that `ocr` value in the serialized result:

```rust
                    ocr_regions: self.inspection.ocr_regions.clone(),
                    capabilities: ImageContextCapabilities {
                        region_features,
                        ocr,
                        layout: self.inspection.layout_status.clone(),
                        template_match: self.inspection.template_match_status.clone(),
                    },
```

- [ ] **Step 3: Replace the OCR unavailable stub with a prepared-host tool**

Replace the existing `OcrTool` implementation with this structure:

```rust
pub struct OcrTool {
    _ctx: Arc<ToolContext>,
    host: Arc<Mutex<dyn rollshot_automation::AutomationHost>>,
    regions: Vec<CanonicalOcrInspection>,
}

impl OcrTool {
    pub fn new(
        ctx: Arc<ToolContext>,
        host: Arc<Mutex<dyn rollshot_automation::AutomationHost>>,
        regions: Vec<CanonicalOcrInspection>,
    ) -> Self {
        Self {
            _ctx: ctx,
            host,
            regions,
        }
    }
}

impl Tool for OcrTool {
    fn name(&self) -> &str {
        "inspect_ocr"
    }

    fn json_schema(&self) -> Value {
        tool_schema::<InspectRegionFeaturesArgs>()
    }

    fn call<'a>(&'a self, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: InspectRegionFeaturesArgs = serde_json::from_value(arguments.clone())
                .map_err(|e| ToolError::ArgumentDecode(e.to_string()))?;
            let region_name = args.region.as_str();
            let region = self
                .regions
                .iter()
                .find(|region| region.name == region_name)
                .ok_or_else(|| {
                    ToolError::ArgumentDecode(format!("unknown canonical OCR region: {region_name}"))
                })?;

            let Some(query) = region.query.clone() else {
                return Ok(ToolOutcome::Success {
                    result_json: serde_json::to_value(InspectOcrResult {
                        region: region.name.clone(),
                        status: "unavailable".into(),
                        bounds: region.bounds,
                        matches: Vec::new(),
                        unavailable_reason: region
                            .unavailable_reason
                            .clone()
                            .or_else(|| Some("ocr_region_unavailable".into())),
                    })
                    .unwrap_or_default(),
                });
            };

            let matches = {
                let mut host = self.host.lock().unwrap();
                host.ocr(query)
            };

            match matches {
                Ok(matches) => {
                    let summaries = matches
                        .into_iter()
                        .map(|m| OcrMatchSummary {
                            bounds: m.bounds,
                            text: m.text,
                            confidence: m.confidence,
                        })
                        .collect();
                    Ok(ToolOutcome::Success {
                        result_json: serde_json::to_value(InspectOcrResult {
                            region: region.name.clone(),
                            status: "available".into(),
                            bounds: region.bounds,
                            matches: summaries,
                            unavailable_reason: None,
                        })
                        .unwrap_or_default(),
                    })
                }
                Err(error) => Ok(ToolOutcome::Success {
                    result_json: serde_json::to_value(InspectOcrResult {
                        region: region.name.clone(),
                        status: "unavailable".into(),
                        bounds: region.bounds,
                        matches: Vec::new(),
                        unavailable_reason: Some(capability_error_code(error)),
                    })
                    .unwrap_or_default(),
                }),
            }
        })
    }
}
```

Keep `LayoutTool` unchanged.

- [ ] **Step 4: Run agent tool tests**

Run:

```bash
rtk cargo test -p rollshot-agent inspect_ocr
rtk cargo test -p rollshot-agent inspect_image_context
```

Expected: PASS.

- [ ] **Step 5: Commit agent OCR tool implementation**

```bash
rtk git add crates/rollshot-agent/src/tools.rs
rtk git commit -m "feat(agent): inspect prepared OCR regions"
```

---

### Task 3: Add Workbench OCR Feature Flag and Catalog

**Files:**
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`
- Test: `crates/rollshot-app/src/result_workspace/workbench/run.rs`

- [ ] **Step 1: Add failing catalog tests**

In `crates/rollshot-app/src/result_workspace/workbench/run.rs`, inside `mod prepare_tests`, add:

```rust
    #[test]
    fn canonical_ocr_catalog_has_named_entries_for_every_region() {
        let names: Vec<&str> = canonical_ocr_catalog(160, 120)
            .iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(
            names,
            vec![
                "full",
                "top_strip",
                "left_strip",
                "right_strip",
                "bottom_strip"
            ]
        );
    }

    #[test]
    fn canonical_ocr_catalog_prefers_full_region_when_under_cap() {
        let catalog = canonical_ocr_catalog(160, 120);
        let full = catalog
            .iter()
            .find(|entry| entry.name == "full")
            .expect("full OCR entry");
        assert_eq!(
            full.bounds,
            rollshot_image_document::ImageRect {
                x: 0.0,
                y: 0.0,
                width: 160.0,
                height: 120.0,
            }
        );
        assert!(full.query.is_some());
        assert_eq!(full.unavailable_reason, None);
    }

    #[test]
    fn canonical_ocr_catalog_keeps_oversized_regions_with_reason() {
        let catalog = canonical_ocr_catalog(100_000, 100_000);
        let full = catalog
            .iter()
            .find(|entry| entry.name == "full")
            .expect("full OCR entry");
        assert_eq!(full.query, None);
        assert_eq!(full.unavailable_reason, Some("area_limit_exceeded"));
    }
```

- [ ] **Step 2: Run tests to verify missing catalog failure**

Run:

```bash
rtk cargo test -p rollshot-app canonical_ocr_catalog
```

Expected: FAIL to compile because `canonical_ocr_catalog` does not exist.

- [ ] **Step 3: Add the Cargo feature**

In `crates/rollshot-app/Cargo.toml`, update `[features]`:

```toml
[features]
action-guide = ["dep:rollshot-action", "dep:rollshot-linux-input", "dep:rollshot-macos-input", "rollshot-iced-overlay/action-guide"]
ocr = ["rollshot-vision/ocr"]
```

Also extend `[dev-dependencies]` for the OCR-enabled workbench test added in Task 4:

```toml
[dev-dependencies]
ab_glyph = "0.2"
imageproc = { workspace = true, features = ["text"] }
serde_json = { workspace = true }
tempfile = "3"
```

- [ ] **Step 4: Add OCR catalog types and constants**

In `crates/rollshot-app/src/result_workspace/workbench/run.rs`, near the B1 catalog constants and type, add:

```rust
const PHASE_B2_OCR_STRIP_PX: u32 = 96;
const PHASE_B2_OCR_LIMIT: u32 = 50;
const PHASE_B2_OCR_AREA_LIMIT: u64 = 16_000_000;

#[derive(Debug, Clone, PartialEq)]
struct CanonicalOcrEntry {
    name: &'static str,
    bounds: rollshot_image_document::ImageRect,
    query: Option<rollshot_automation::OcrQuery>,
    unavailable_reason: Option<&'static str>,
}
```

Add the catalog function after `canonical_region_feature_catalog`:

```rust
fn canonical_ocr_catalog(width: u32, height: u32) -> Vec<CanonicalOcrEntry> {
    use rollshot_automation::{OcrQuery, Region};
    use rollshot_image_document::ImageRect;

    if width == 0 || height == 0 {
        return Vec::new();
    }

    let width_f = width as f32;
    let height_f = height as f32;
    let strip_h = height.min(PHASE_B2_OCR_STRIP_PX) as f32;
    let strip_w = width.min(PHASE_B2_OCR_STRIP_PX) as f32;

    let make_entry = |name: &'static str, bounds: ImageRect| {
        let area = (bounds.width.ceil() as u64).saturating_mul(bounds.height.ceil() as u64);
        if area > PHASE_B2_OCR_AREA_LIMIT {
            CanonicalOcrEntry {
                name,
                bounds,
                query: None,
                unavailable_reason: Some("area_limit_exceeded"),
            }
        } else {
            CanonicalOcrEntry {
                name,
                bounds,
                query: Some(OcrQuery {
                    region: Region::Rect { bounds },
                    limit: PHASE_B2_OCR_LIMIT,
                }),
                unavailable_reason: None,
            }
        }
    };

    vec![
        CanonicalOcrEntry {
            name: "full",
            bounds: ImageRect {
                x: 0.0,
                y: 0.0,
                width: width_f,
                height: height_f,
            },
            query: if (width as u64 * height as u64) <= PHASE_B2_OCR_AREA_LIMIT {
                Some(OcrQuery {
                    region: Region::Full,
                    limit: PHASE_B2_OCR_LIMIT,
                })
            } else {
                None
            },
            unavailable_reason: if (width as u64 * height as u64) <= PHASE_B2_OCR_AREA_LIMIT {
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

- [ ] **Step 5: Extend authoring inspection context conversion**

Change `authoring_inspection_context` signature:

```rust
fn authoring_inspection_context(
    payload_mode: PayloadMode,
    catalog: &[CanonicalRegionFeatureEntry],
    ocr_catalog: &[CanonicalOcrEntry],
) -> rollshot_agent::tools::AuthoringInspectionContext {
```

Inside it, add OCR region conversion:

```rust
    let ocr_regions = ocr_catalog
        .iter()
        .map(|entry| rollshot_agent::tools::CanonicalOcrInspection {
            name: entry.name.into(),
            bounds: Some(entry.bounds),
            query: entry.query.clone(),
            unavailable_reason: entry.unavailable_reason.map(str::to_string),
        })
        .collect();
```

Return `AuthoringInspectionContext` with:

```rust
        ocr_regions,
        ocr_status: if cfg!(feature = "ocr") {
            rollshot_agent::tools::CapabilityStatus::unavailable("no_prepared_ocr_regions")
        } else {
            rollshot_agent::tools::CapabilityStatus::unavailable("ocr_disabled")
        },
```

Update every existing call to `authoring_inspection_context` in this file:

```rust
        let inspection = authoring_inspection_context(
            PayloadMode::FullScreenshot,
            &canonical_region_feature_catalog(64, 64),
            &canonical_ocr_catalog(64, 64),
        );
```

In `start_agent_run`, construct both catalogs:

```rust
        let region_catalog = canonical_region_feature_catalog(image_dims.0, image_dims.1);
        let ocr_catalog = canonical_ocr_catalog(image_dims.0, image_dims.1);
        let inspection = authoring_inspection_context(
            payload_mode,
            &region_catalog,
            &ocr_catalog,
        );
```

- [ ] **Step 6: Run workbench catalog tests**

Run:

```bash
rtk cargo test -p rollshot-app canonical_ocr_catalog
rtk cargo test -p rollshot-app result_workspace::workbench::run::prepare_tests::authoring_registry_exposes_truthful_phase_b1_tools
```

Expected: PASS. The registry test still excludes `inspect_ocr` in the default build.

- [ ] **Step 7: Commit feature flag and OCR catalog**

```bash
rtk git add crates/rollshot-app/Cargo.toml crates/rollshot-app/src/result_workspace/workbench/run.rs
rtk git commit -m "feat(app): add smart redaction OCR inspection catalog"
```

---

### Task 4: Wire OCR Preparation and Product Registry Behind Feature

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`
- Test: `crates/rollshot-app/src/result_workspace/workbench/run.rs`

- [ ] **Step 1: Add failing registry and dry-run tests**

Inside `mod prepare_tests`, add an OCR-enabled registry test:

```rust
    #[cfg(feature = "ocr")]
    #[test]
    fn authoring_registry_exposes_ocr_tool_when_feature_enabled() {
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
            &canonical_ocr_catalog(64, 64),
        );

        let registry = build_authoring_tool_registry(ctx, executor, host, inspection).unwrap();
        let names = registry.tool_names();

        assert!(names.contains(&"inspect_ocr"));
        assert!(!names.contains(&"inspect_layout"));
    }
```

Add a real-host OCR dry-run test behind the OCR feature. Keep the image small and text explicit:

```rust
    #[cfg(feature = "ocr")]
    #[tokio::test]
    async fn prepared_vision_context_dry_runs_full_ocr_query() {
        use imageproc::drawing::draw_text_mut;
        use rollshot_agent::tools::{DryRunTool, OcrTool, Tool};

        let font = ab_glyph::FontRef::try_from_slice(include_bytes!(
            "../../../../rollshot-image-document/assets/fonts/DejaVuSans.ttf"
        ));
        if font.is_err() {
            return;
        }
        let font = font.unwrap();
        let mut image =
            image::RgbaImage::from_pixel(480, 160, image::Rgba([255, 255, 255, 255]));
        draw_text_mut(
            &mut image,
            image::Rgba([0, 0, 0, 255]),
            20,
            40,
            ab_glyph::PxScale::from(32.0),
            &font,
            "alice@example.com",
        );

        let vision = prepare_vision_context(&image).unwrap();
        let region_catalog = canonical_region_feature_catalog(480, 160);
        let ocr_catalog = canonical_ocr_catalog(480, 160);
        let inspection =
            authoring_inspection_context(PayloadMode::FullScreenshot, &region_catalog, &ocr_catalog);
        let ctx = tool_context_for_tests();
        let host = vision.host.clone()
            as std::sync::Arc<std::sync::Mutex<dyn rollshot_automation::AutomationHost>>;

        let inspect = OcrTool::new(ctx.clone(), host.clone(), inspection.ocr_regions.clone());
        let inspected = inspect
            .call(&serde_json::json!({"region": "full"}))
            .await
            .unwrap();
        match inspected {
            rollshot_agent::tools::ToolOutcome::Success { result_json } => {
                assert_eq!(result_json["status"].as_str(), Some("available"));
            }
            other => panic!("expected OCR inspection success, got {other:?}"),
        }

        let source = r#"
function main(input) {
  const matches = rollshot.ocr({ region: { kind: "full" }, limit: 50 });
  return {
    candidates: matches.map((match) => ({
      kind: "addRedaction",
      bounds: match.bounds,
      confidence: match.confidence,
      label: "ocr-match"
    }))
  };
}
"#;
        let dry_run = DryRunTool::new(
            ctx,
            std::sync::Arc::new(rollshot_automation_rquickjs::QuickJsExecutor),
            host,
        );
        let dry_run_result = dry_run
            .call(&serde_json::json!({"source": source, "generation": 0}))
            .await
            .unwrap();
        match dry_run_result {
            rollshot_agent::tools::ToolOutcome::Success { result_json } => {
                assert!(
                    result_json["candidate_count"].as_u64().unwrap_or(0) > 0,
                    "expected OCR dry-run candidates, got {result_json}"
                );
            }
            other => panic!("expected dry-run success, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run tests to verify registry test fails before registration**

Run:

```bash
rtk cargo test -p rollshot-app --features ocr authoring_registry_exposes_ocr_tool_when_feature_enabled
```

Expected: FAIL because `inspect_ocr` is not registered yet.

- [ ] **Step 3: Add OCR preparation helper**

In `crates/rollshot-app/src/result_workspace/workbench/run.rs`, add:

```rust
#[cfg(feature = "ocr")]
fn prepare_phase_b2_ocr(
    host: &mut rollshot_vision::RealAutomationHost,
    index: &VisualIndex,
) -> Result<(), WorkbenchError> {
    for entry in canonical_ocr_catalog(index.width(), index.height()) {
        let Some(query) = entry.query else {
            continue;
        };
        host.prepare_ocr(index, &query)
            .map_err(|e| WorkbenchError::VisionPrepare {
                message: format!("ocr {}: {e}", entry.name),
            })?;
    }
    Ok(())
}
```

Call it in `prepare_vision_context` after region-feature preparation:

```rust
    prepare_phase_a_region_features(&mut host, &index)?;
    #[cfg(feature = "ocr")]
    prepare_phase_b2_ocr(&mut host, &index)?;
```

Call it in `run_existing_preset` after region-feature preparation:

```rust
    prepare_phase_a_region_features(&mut host, &index)?;
    #[cfg(feature = "ocr")]
    prepare_phase_b2_ocr(&mut host, &index)?;
```

- [ ] **Step 4: Register `inspect_ocr` only in OCR-enabled app builds**

In `build_authoring_tool_registry`, add `OcrTool` to the import list:

```rust
        DryRunTool, GetContextSummaryTool, InspectImageContextTool, OcrTool, RegionFeaturesTool,
```

After `RegionFeaturesTool` registration and before `DryRunTool`, add:

```rust
    #[cfg(feature = "ocr")]
    reg(
        &mut registry,
        Arc::new(OcrTool::new(
            tool_ctx.clone(),
            host.clone(),
            inspection.ocr_regions.clone(),
        )),
    )?;
```

- [ ] **Step 5: Run default and OCR-enabled workbench tests**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace::workbench
rtk cargo test -p rollshot-app --features ocr authoring_registry_exposes_ocr_tool_when_feature_enabled
rtk cargo test -p rollshot-app --features ocr prepared_vision_context_dry_runs_full_ocr_query
```

Expected: PASS. If the OCR dry-run test fails because local OCR runtime dependencies are unavailable, capture the exact error and keep the registry/catalog tests passing; do not replace the test with a fake-host assertion unless the error is an environment setup failure outside the crate.

- [ ] **Step 6: Commit OCR product wiring**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/run.rs
rtk git commit -m "feat(app): wire OCR inspection into smart redaction runs"
```

---

### Task 5: Update Smart Redaction Prompt and Provider Tests

**Files:**
- Modify: `crates/rollshot-agent/src/driver.rs`
- Test: `crates/rollshot-agent/src/driver.rs`

- [ ] **Step 1: Add failing prompt/schema assertions**

In `second_turn_request_carries_history_and_tool_schemas`, update the test inspection context to include `ocr_regions`:

```rust
                ocr_regions: vec![crate::tools::CanonicalOcrInspection {
                    name: "full".into(),
                    bounds: Some(rollshot_image_document::ImageRect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 100.0,
                    }),
                    query: Some(rollshot_automation::OcrQuery {
                        region: rollshot_automation::Region::Full,
                        limit: 50,
                    }),
                    unavailable_reason: None,
                }],
```

Add `OcrTool` to the test import list near line 1212:

```rust
        DryRunTool, GetContextSummaryTool, InspectImageContextTool, OcrTool, RegionFeaturesTool,
```

Register the OCR tool in the same test registry:

```rust
            reg.register(Arc::new(OcrTool::new(
                ctx.clone(),
                host.clone(),
                inspection.ocr_regions.clone(),
            )))
            .unwrap();
```

Add prompt assertions after the existing region-feature assertions:

```rust
            assert!(
                system_prompt.contains("Call inspect_ocr for text-driven redaction requests"),
                "system prompt should guide OCR inspection for text-driven intents, got: {:?}",
                system_prompt
            );
            assert!(
                system_prompt.contains("inspect_ocr returns full recognized text"),
                "system prompt should disclose full OCR text in tool results, got: {:?}",
                system_prompt
            );
```

Add OCR schema assertion after `region_features_def`:

```rust
            let ocr_def = second
                .tool_definitions
                .iter()
                .find(|d| d.name == "inspect_ocr")
                .expect("inspect_ocr tool definition present");
            assert_eq!(ocr_def.parameters["type"].as_str(), Some("object"));
            assert!(
                ocr_def.parameters.to_string().contains("region"),
                "inspect_ocr schema must require a canonical region argument, got: {}",
                ocr_def.parameters
            );
```

- [ ] **Step 2: Run driver prompt test to verify failure**

Run:

```bash
rtk cargo test -p rollshot-agent second_turn_request_carries_history_and_tool_schemas
```

Expected: FAIL because the prompt does not yet contain the new OCR guidance.

- [ ] **Step 3: Update the Smart Redaction system prompt**

In `SMART_REDACTION_SYSTEM_PROMPT`, replace the current OCR/layout guidance:

```text
- In Phase A, OCR and layout may fail unless dry_run proves they are available. Prefer deterministic regionFeatures strip regions for simple screenshot chrome targets, for example:
```

with:

```text
- In OCR-enabled runs, call inspect_ocr for text-driven redaction requests before writing source. inspect_ocr returns full recognized text, bounds, and confidence for canonical regions. Use OCR bounds as evidence for candidate rectangles.
- If OCR is unavailable, treat that as a harness limitation and do not invent text evidence.
- Prefer deterministic regionFeatures strip regions for simple screenshot chrome targets, for example:
```

Replace the inspection loop section with:

```text
Inspection loop:
1. Call inspect_image_context before writing or replacing source.
2. Call inspect_ocr for text-driven redaction requests such as visible words, names, emails, ids, labels, form fields, or account-like strings.
3. Use inspect_region_features with canonical regions when coarse visual evidence is needed.
4. Valid canonical regions are full, top_strip, left_strip, right_strip, bottom_strip.
5. Do not ask for raw pixels or custom crop inspection; use dry_run to verify source behavior.
```

- [ ] **Step 4: Keep prompt examples valid**

Run:

```bash
rtk cargo test -p rollshot-agent smart_redaction_prompt_examples_validate
```

Expected: PASS. If the OCR example extraction now includes the `Inspection loop:` marker incorrectly, adjust the test marker from `"Authoring loop:"` to `"Inspection loop:"` so only the JavaScript example is validated.

- [ ] **Step 5: Run driver tests**

Run:

```bash
rtk cargo test -p rollshot-agent second_turn_request_carries_history_and_tool_schemas
rtk cargo test -p rollshot-agent smart_redaction_prompt_examples_validate
```

Expected: PASS.

- [ ] **Step 6: Commit prompt contract changes**

```bash
rtk git add crates/rollshot-agent/src/driver.rs
rtk git commit -m "feat(agent): guide OCR inspection in smart redaction prompt"
```

---

### Task 6: Final Verification

**Files:**
- No source edits unless verification exposes a defect in this phase's changes.

- [ ] **Step 1: Run narrow default-build checks**

Run:

```bash
rtk cargo test -p rollshot-agent inspect_ocr
rtk cargo test -p rollshot-agent inspect_image_context
rtk cargo test -p rollshot-agent second_turn_request_carries_history_and_tool_schemas
rtk cargo test -p rollshot-app result_workspace::workbench
```

Expected: PASS.

- [ ] **Step 2: Run OCR-enabled checks**

Run:

```bash
rtk cargo test -p rollshot-app --features ocr authoring_registry_exposes_ocr_tool_when_feature_enabled
rtk cargo test -p rollshot-app --features ocr prepared_vision_context_dry_runs_full_ocr_query
rtk cargo test -p rollshot-vision --features ocr ocr
```

Expected: PASS. If `rollshot-ocr` model provisioning or ONNX Runtime setup fails, record the exact failing command and stderr in the final response; do not claim OCR runtime verification passed.

- [ ] **Step 3: Run formatting**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 4: Run broader package tests**

Run:

```bash
rtk cargo test -p rollshot-agent
rtk cargo test -p rollshot-app
```

Expected: PASS.

- [ ] **Step 5: Inspect final git status**

Run:

```bash
rtk git status --short
```

Expected: only intentional tracked changes from this phase are present. Ignore unrelated untracked `learn-projects/claude-code-source-code/` if it is still present.

- [ ] **Step 6: Commit verification fixes if needed**

If any verification step required a source fix, commit only the expected B2 files touched by that fix:

```bash
rtk git add crates/rollshot-agent/src/tools.rs crates/rollshot-agent/src/driver.rs crates/rollshot-app/Cargo.toml crates/rollshot-app/src/result_workspace/workbench/run.rs
rtk git commit -m "fix(agent): stabilize OCR inspection verification"
```

If all previous task commits pass without extra edits, skip this step.

---

## Plan Self-Review

- Spec coverage: The plan covers the OCR app feature, separate OCR catalog, prepared host wiring, full text tool result, OCR-enabled registry, default registry exclusion, prompt guidance, and verification commands.
- Scope guard: The plan does not change `PayloadMode::OcrLayoutOnly`, layout inspection, template inspection, source patching, OCR model packaging, or workbench UI.
- Type consistency: `CanonicalOcrInspection` carries `OcrQuery`; B1 `CanonicalRegionInspection` still carries `RegionFeaturesQuery`; both use the shared `CanonicalRegion` enum for schema and argument decoding.
- Testing path: Agent behavior uses `FakeAutomationHost`; product default behavior uses normal app tests; real OCR behavior is gated behind `--features ocr`.
