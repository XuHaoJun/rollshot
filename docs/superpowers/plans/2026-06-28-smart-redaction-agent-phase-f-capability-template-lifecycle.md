# Smart Redaction Agent Phase F Capability Template Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Smart Redaction template capability handles durable, inspectable, and available to authoring and existing-preset runs through a preset-local lifecycle.

**Architecture:** Keep expensive vision work in `rollshot-vision`, keep durable preset metadata in `rollshot-preset`, and let `rollshot-app` assemble a per-run capability bundle from the active preset. Phase F v1 exposes preset-local template aliases through `AutomationInput.capability_handles`, prepares template matching for canonical regions, and records revision capability metadata so the workbench can explain missing capability data.

**Tech Stack:** Rust, `rollshot-vision::TemplateStore`, `rollshot-preset` JSON store, `rollshot-app` Smart Redaction workbench, `rollshot-agent` authoring tools, QuickJS automation execution.

---

## Scope

Phase F v1 is a lifecycle foundation, not a full template-authoring UI.

Implement:
- Preset-local template store sidecar path and capability metadata.
- Template asset summaries that do not expose raw pixels.
- Product capability bundle loading from the active preset.
- Authoring and existing-preset runs that pass template aliases through `AutomationInput.capability_handles`.
- Canonical-region template preparation for available template aliases.
- Tests proving missing templates are reported as availability problems instead of hidden runtime surprises.

Do not implement:
- Agent-suggested template creation.
- User crop-selection UI for creating templates.
- Marketplace/export format changes beyond existing `TemplateStore::export`.
- Arbitrary template preparation for every possible JS region expression.

## Reference Project Notes

`learn-projects/claude-code-source-code` is used here as an agent harness reference, not as an architecture to copy.

Relevant patterns:
- `src/services/tools/toolOrchestration.ts` partitions tool calls by concurrency safety. Rollshot should keep state-changing source/revision/template work serialized and keep read-only inspection truthful.
- `src/Tool.ts` and concrete tools use explicit schemas and output schemas. Rollshot should keep capability availability and template handle summaries structured, not prose-only.
- `src/tools/EnterPlanModeTool/EnterPlanModeTool.ts` and `ExitPlanModeTool/ExitPlanModeV2Tool.ts` make planning state explicit and reversible. Rollshot should make preset capability state explicit in revision metadata instead of inferring it from screenshots or provider memory.

## File Structure

- Modify `crates/rollshot-vision/src/template.rs`
  - Add non-sensitive `TemplateAssetSummary`.
  - Add `TemplateStore::summaries()` and `TemplateStore::is_empty()`.
- Modify `crates/rollshot-vision/src/lib.rs`
  - Re-export `TemplateAssetSummary`.
- Modify `crates/rollshot-preset/src/domain.rs`
  - Add revision capability metadata structs.
  - Add a serde-defaulted `capabilities` field to `AutomationRevision`.
- Modify `crates/rollshot-preset/src/store.rs`
  - Add preset-local template sidecar path helpers.
  - Add `add_revision_with_capabilities`; keep `add_revision` as a default wrapper.
- Modify `crates/rollshot-preset/src/lib.rs`
  - Re-export new metadata types and update serde round-trip tests.
- Modify `crates/rollshot-app/src/result_workspace/workbench/mod.rs`
  - Carry `preset_id` and template-store root through pending run parameters.
- Modify `crates/rollshot-app/src/result_workspace/workbench/run.rs`
  - Replace the empty `product_capability_handles()` path with a `ProductCapabilityBundle`.
  - Load template stores from the active preset.
  - Prepare canonical template queries before QuickJS dry-runs and existing-preset runs.
  - Surface capability availability in `authoring_inspection_context`.
- Modify `crates/rollshot-app/src/result_workspace/workbench/review.rs`
  - Save revisions with capability metadata.
- Modify `crates/rollshot-app/src/result_workspace/update.rs`
  - Thread preset id and store root into author/improve runs and save actions.
- Modify `crates/rollshot-agent/src/driver.rs`
  - Tighten prompt language: use `input.capabilityHandles.<alias>` for template handles listed by `inspect_image_context`.

## Task 1: Vision Template Store Summaries

**Files:**
- Modify: `crates/rollshot-vision/src/template.rs`
- Modify: `crates/rollshot-vision/src/lib.rs`

- [ ] **Step 1: Add failing tests for privacy-safe template summaries**

Append these tests to the existing `tests` module in `template.rs`:

```rust
#[test]
fn summaries_expose_template_metadata_without_bytes() {
    let mut store = TemplateStore::new();
    store
        .insert(TemplateAsset {
            handle: "toolbar-logo".into(),
            sensitivity: TemplateSensitivity::Chrome,
            source: TemplateSource::UserRect,
            created_at_ms: 42,
            bounds_in_source_image: Some(ImageRect {
                x: 4.0,
                y: 5.0,
                width: 8.0,
                height: 6.0,
            }),
            bytes: TemplateBytes::new(8, 6, vec![120u8; 8 * 6 * 4]).unwrap(),
        })
        .unwrap();

    let summaries = store.summaries();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].handle, "toolbar-logo");
    assert_eq!(summaries[0].width, 8);
    assert_eq!(summaries[0].height, 6);
    assert_eq!(summaries[0].byte_len, 8 * 6 * 4);
    assert_eq!(summaries[0].sensitivity, TemplateSensitivity::Chrome);
    assert_eq!(summaries[0].source, TemplateSource::UserRect);
}

#[test]
fn empty_store_reports_empty() {
    let store = TemplateStore::new();
    assert!(store.is_empty());
    assert!(store.summaries().is_empty());
}
```

- [ ] **Step 2: Run the focused tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-vision template::tests
```

Expected: fail because `TemplateAssetSummary`, `TemplateStore::summaries`, and `TemplateStore::is_empty` do not exist.

- [ ] **Step 3: Add summary API**

In `template.rs`, after `TemplateAsset`, add:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateAssetSummary {
    pub handle: String,
    pub sensitivity: TemplateSensitivity,
    pub source: TemplateSource,
    pub created_at_ms: u64,
    pub bounds_in_source_image: Option<ImageRect>,
    pub width: u32,
    pub height: u32,
    pub byte_len: usize,
}
```

Inside `impl TemplateStore`, add:

```rust
    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    pub fn summaries(&self) -> Vec<TemplateAssetSummary> {
        self.assets
            .values()
            .map(|asset| TemplateAssetSummary {
                handle: asset.handle.clone(),
                sensitivity: asset.sensitivity,
                source: asset.source,
                created_at_ms: asset.created_at_ms,
                bounds_in_source_image: asset.bounds_in_source_image,
                width: asset.bytes.width(),
                height: asset.bytes.height(),
                byte_len: asset.bytes.byte_len(),
            })
            .collect()
    }
```

- [ ] **Step 4: Re-export the summary type**

In `crates/rollshot-vision/src/lib.rs`, add `TemplateAssetSummary` to the existing `pub use template::{ ... }` list:

```rust
pub use template::{
    ExportTemplateAssetRecord, LocalTemplateAssetRecord, TemplateAsset, TemplateAssetSummary,
    TemplateBytes, TemplateBytesRecord, TemplateSensitivity, TemplateSource, TemplateStore,
    MAX_SCORE_POSITIONS, MAX_TEMPLATE_AREA, MAX_TEMPLATE_COUNT, MAX_TEMPLATE_MATCH_PIXEL_VISITS,
    MAX_TEMPLATE_STORE_BYTES,
};
```

- [ ] **Step 5: Run the focused tests and verify they pass**

Run:

```bash
rtk cargo test -p rollshot-vision template::tests
```

Expected: both tests pass.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-vision/src/template.rs crates/rollshot-vision/src/lib.rs
rtk git commit -m "feat(vision): expose template store summaries"
```

## Task 2: Preset Capability Metadata And Template Sidecar Path

**Files:**
- Modify: `crates/rollshot-preset/src/domain.rs`
- Modify: `crates/rollshot-preset/src/store.rs`
- Modify: `crates/rollshot-preset/src/lib.rs`

- [ ] **Step 1: Add failing tests for revision metadata defaults and template sidecar path**

In `crates/rollshot-preset/src/lib.rs`, add this test inside the existing `tests` module:

```rust
#[test]
fn revision_capabilities_default_for_legacy_json() {
    let revision = AutomationRevision {
        store_schema_version: STORE_SCHEMA_VERSION,
        id: RevisionId("rev-1".into()),
        preset_id: PresetId("preset-1".into()),
        parent_id: None,
        created_at: "2026-06-28T00:00:00Z".into(),
        provenance: RevisionProvenance {
            origin: RevisionOrigin::AgentRun,
            note: None,
            source_run_ref: None,
        },
        artifact: sample_artifact(),
        capabilities: RevisionCapabilityMetadata::default(),
    };
    let mut value = serde_json::to_value(&revision).unwrap();
    value.as_object_mut().unwrap().remove("capabilities");

    let decoded: AutomationRevision = serde_json::from_value(value).unwrap();

    assert_eq!(decoded.capabilities, RevisionCapabilityMetadata::default());
}
```

In `crates/rollshot-preset/src/store.rs`, add this test inside the existing `tests` module:

```rust
#[test]
fn template_store_path_is_preset_local_and_validated() {
    let tmp = tempfile::tempdir().unwrap();
    let store = PresetStore::open(tmp.path().to_path_buf());
    let path = store
        .template_store_path(&PresetId("preset-a".into()))
        .unwrap();

    assert_eq!(
        path,
        tmp.path()
            .join("presets")
            .join("preset-a")
            .join("templates.local.json")
    );
    assert!(store
        .template_store_path(&PresetId("../bad".into()))
        .is_err());
}
```

- [ ] **Step 2: Run the focused tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-preset
```

Expected: fail because the metadata types, `AutomationRevision.capabilities`, and `template_store_path` do not exist.

- [ ] **Step 3: Add metadata types and serde default**

In `domain.rs`, add `CapabilityName` to imports:

```rust
use rollshot_automation::{CapabilityName, ValidatedAutomation};
```

Add these structs before `AutomationRevision`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateHandleMetadata {
    pub alias: String,
    pub handle: String,
    pub display_name: String,
    pub sensitivity_sensitive: bool,
    pub source_agent_suggested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionCapabilityRequirement {
    pub capability: CapabilityName,
    pub alias: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionCapabilityMetadata {
    pub requirements: Vec<RevisionCapabilityRequirement>,
    pub template_handles: Vec<TemplateHandleMetadata>,
}
```

Change `AutomationRevision` to:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationRevision {
    pub store_schema_version: u16,
    pub id: RevisionId,
    pub preset_id: PresetId,
    pub parent_id: Option<RevisionId>,
    pub created_at: String,
    pub provenance: RevisionProvenance,
    pub artifact: ValidatedAutomation,
    #[serde(default)]
    pub capabilities: RevisionCapabilityMetadata,
}
```

- [ ] **Step 4: Add store sidecar path and metadata-aware revision writer**

In `store.rs`, import `RevisionCapabilityMetadata`:

```rust
use crate::domain::{
    AutomationRevision, Preset, PresetId, PresetSummary, RevisionCapabilityMetadata, RevisionId,
    RevisionSummary, STORE_SCHEMA_VERSION,
};
```

Inside `impl PresetStore`, add:

```rust
    pub fn template_store_path(&self, id: &PresetId) -> Result<PathBuf> {
        validate_id(&id.0)?;
        Ok(self.preset_dir(id).join("templates.local.json"))
    }
```

Replace the body of `add_revision` with a wrapper:

```rust
    pub fn add_revision(
        &self,
        preset_id: &PresetId,
        id: RevisionId,
        parent_id: Option<RevisionId>,
        artifact: ValidatedAutomation,
        provenance: crate::domain::RevisionProvenance,
        now: String,
    ) -> Result<AutomationRevision> {
        self.add_revision_with_capabilities(
            preset_id,
            id,
            parent_id,
            artifact,
            provenance,
            now,
            RevisionCapabilityMetadata::default(),
        )
    }
```

Then add:

```rust
    pub fn add_revision_with_capabilities(
        &self,
        preset_id: &PresetId,
        id: RevisionId,
        parent_id: Option<RevisionId>,
        artifact: ValidatedAutomation,
        provenance: crate::domain::RevisionProvenance,
        now: String,
        capabilities: RevisionCapabilityMetadata,
    ) -> Result<AutomationRevision> {
        validate_id(&id.0)?;
        validate_id(&preset_id.0)?;
        ensure_compatible(&artifact)?;
        let _lock = io::lock_dir(&self.preset_dir(preset_id))?;
        let _ = self.load_preset(preset_id)?;
        let path = self.revision_json(preset_id, &id);
        if path.exists() {
            return Err(StoreError::RevisionExists(id.0.clone()));
        }
        let revision = AutomationRevision {
            store_schema_version: STORE_SCHEMA_VERSION,
            id: id.clone(),
            preset_id: preset_id.clone(),
            parent_id,
            created_at: now,
            provenance,
            artifact,
            capabilities,
        };
        let bytes = serde_json::to_vec_pretty(&revision)?;
        io::write_atomic(&path, &bytes)?;
        Ok(revision)
    }
```

- [ ] **Step 5: Re-export types and update existing revision constructors**

In `lib.rs`, extend exports:

```rust
pub use domain::{
    AutomationRevision, Preset, PresetId, PresetSummary, RevisionCapabilityMetadata,
    RevisionCapabilityRequirement, RevisionId, RevisionOrigin, RevisionProvenance,
    RevisionSummary, TemplateHandleMetadata, STORE_SCHEMA_VERSION,
};
```

Add `capabilities: RevisionCapabilityMetadata::default(),` to every hand-written
`AutomationRevision` literal in `rollshot-preset`. There is exactly one such
test literal: `crates/rollshot-preset/src/lib.rs` `revision_serde_round_trip`
(the production builder in `store.rs` is rewritten in Step 4, so it already sets
the field).

> **Cross-crate compile note (do not skip).** `#[serde(default)]` only fills the
> field when *deserializing* JSON; every Rust **construction** literal must add
> the field or the workspace will not compile. Besides the preset crate, there
> are three `AutomationRevision` literals in
> `crates/rollshot-app/src/result_workspace/workbench/run.rs`:
> `make_revision_from_source` (~L744), `make_empty_revision` (~L794), and
> `active_revision_for_reducer_test` (~L1353). These are fixed at the **start of
> Task 3** (the first `rollshot-app` task), each gaining
> `capabilities: rollshot_preset::RevisionCapabilityMetadata::default(),`.
> Consequence for parallel execution: after Task 2's commit the `rollshot-app`
> crate does **not** compile until Task 3 lands, so Task 2 must not be merged
> standalone ahead of Task 3 (see the parallelization note at the end of this
> plan).

- [ ] **Step 6: Run the focused tests and verify they pass**

Run:

```bash
rtk cargo test -p rollshot-preset
```

Expected: pass.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-preset/src/domain.rs crates/rollshot-preset/src/store.rs crates/rollshot-preset/src/lib.rs
rtk git commit -m "feat(preset): store revision capability metadata"
```

## Task 3: Product Capability Bundle

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`

- [ ] **Step 1: Add failing tests for loading template handles into a bundle**

Append to the `prepare_tests` module in `run.rs`:

```rust
fn textured_template_bytes() -> rollshot_vision::TemplateBytes {
    let rgba = image::RgbaImage::from_fn(8, 8, |x, y| {
        let v = ((x * 17 + y * 31 + x * y * 3) % 255) as u8;
        image::Rgba([v, v, v, 255])
    });
    rollshot_vision::TemplateBytes::new(8, 8, rgba.into_raw()).unwrap()
}

#[test]
fn product_capability_bundle_loads_preset_template_handles() {
    let tmp = tempfile::tempdir().unwrap();
    let store = rollshot_preset::PresetStore::open(tmp.path().to_path_buf());
    let preset_id = rollshot_preset::PresetId("preset-a".into());
    store
        .create_preset(
            preset_id.clone(),
            "Preset A".into(),
            "test".into(),
            "2026-06-28T00:00:00Z".into(),
        )
        .unwrap();
    let mut templates = rollshot_vision::TemplateStore::new();
    templates
        .insert(rollshot_vision::TemplateAsset {
            handle: "toolbar-logo".into(),
            sensitivity: rollshot_vision::TemplateSensitivity::Chrome,
            source: rollshot_vision::TemplateSource::UserRect,
            created_at_ms: 1,
            bounds_in_source_image: None,
            bytes: textured_template_bytes(),
        })
        .unwrap();
    templates.save_local(&store.template_store_path(&preset_id).unwrap()).unwrap();

    let bundle = ProductCapabilityBundle::load(&store, Some(&preset_id)).unwrap();

    assert_eq!(
        bundle.capability_handles.get("toolbar-logo").map(String::as_str),
        Some("toolbar-logo")
    );
    assert_eq!(bundle.template_summaries.len(), 1);
    assert!(bundle.availability.template_match.available);
}

#[test]
fn product_capability_bundle_reports_missing_template_store() {
    let tmp = tempfile::tempdir().unwrap();
    let store = rollshot_preset::PresetStore::open(tmp.path().to_path_buf());
    let preset_id = rollshot_preset::PresetId("preset-a".into());
    store
        .create_preset(
            preset_id.clone(),
            "Preset A".into(),
            "test".into(),
            "2026-06-28T00:00:00Z".into(),
        )
        .unwrap();

    let bundle = ProductCapabilityBundle::load(&store, Some(&preset_id)).unwrap();

    assert!(bundle.capability_handles.is_empty());
    assert!(!bundle.availability.template_match.available);
    assert_eq!(bundle.availability.template_match.reason.as_deref(), Some("no_capability_handles"));
}
```

- [ ] **Step 2: Run the focused tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app product_capability_bundle
```

Expected: fail because `ProductCapabilityBundle` does not exist and `tempfile` is not imported in this test module.

- [ ] **Step 3: Add pending run preset context**

First, unblock compilation: add
`capabilities: rollshot_preset::RevisionCapabilityMetadata::default(),` to the
three existing `AutomationRevision` literals in this file
(`make_revision_from_source`, `make_empty_revision`,
`active_revision_for_reducer_test`) — see the cross-crate compile note in
Task 2 Step 5.

In `workbench/mod.rs`, add fields to `PendingRunParams`:

```rust
    pub preset_id: rollshot_preset::PresetId,
    pub preset_store_root: std::path::PathBuf,
```

`PendingRunParams` has no `Default`, so **every** construction literal must add
both fields. There are exactly four sites:

- `update.rs` `SendRequested` handler (~L927, `RunKind::Author`)
- `update.rs` `AskAgentToRevise` handler (~L1052, `RunKind::Improve`)
- `run.rs` reducer test `disclosure_cancelled_clears_pending_run_and_flag` (~L1524)
- `run.rs` reducer test `disclosure_confirmed_blocked_while_running` (~L1721)

For both `update.rs` product sites, set:

```rust
preset_id: rollshot_preset::PresetId("workbench-draft".into()),
preset_store_root: crate::daemon::config::rollshot_config_dir()
    .map(|dir| dir.join("presets"))
    .unwrap_or_default(),
```

> **Do not hard-fail the run on a missing config dir.** Templates are optional
> in Phase F v1 (no UI creates them yet), and `ProductCapabilityBundle::load`
> already returns an empty bundle for a missing/empty path. So resolve the store
> root with `.unwrap_or_default()` (empty `PathBuf` → empty bundle → templates
> simply unavailable) rather than returning `WorkbenchError::Config` and
> blocking the author flow. The config dir is still required at save time, where
> the existing `SavePresetOrRevision` handler already surfaces `Config`.

> **Store-root nesting (matches existing behavior).** `PresetStore::open(root)`
> appends its own `presets/` segment internally, and the existing
> `SavePresetOrRevision` opens with `config_dir.join("presets")`. Using the same
> value here keeps run-time and save-time agreeing on
> `…/presets/presets/workbench-draft/templates.local.json`. This double-`presets`
> nesting is pre-existing; do not "fix" it in this plan (it would relocate
> existing draft presets — out of scope). Note the Task 2/Task 3 *tests* open the
> store at the bare temp root, so their paths are `…/presets/<id>/…` (one level).

For the two `run.rs` reducer tests, use a fixed root (no config-dir access):

```rust
preset_id: rollshot_preset::PresetId("workbench-draft".into()),
preset_store_root: std::path::PathBuf::from("/tmp/rollshot-test-presets"),
```

- [ ] **Step 4: Implement bundle types and loader**

In `run.rs`, replace `product_capability_handles()` with:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CapabilityAvailability {
    pub available: bool,
    pub reason: Option<String>,
}

impl CapabilityAvailability {
    fn available() -> Self {
        Self {
            available: true,
            reason: None,
        }
    }

    fn unavailable(reason: &str) -> Self {
        Self {
            available: false,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProductCapabilityAvailability {
    pub template_match: CapabilityAvailability,
}

#[derive(Debug)]
pub(crate) struct ProductCapabilityBundle {
    pub capability_handles: std::collections::BTreeMap<String, String>,
    pub template_store: rollshot_vision::TemplateStore,
    pub template_summaries: Vec<rollshot_vision::TemplateAssetSummary>,
    pub availability: ProductCapabilityAvailability,
}

impl ProductCapabilityBundle {
    pub(crate) fn empty() -> Self {
        Self {
            capability_handles: std::collections::BTreeMap::new(),
            template_store: rollshot_vision::TemplateStore::new(),
            template_summaries: Vec::new(),
            availability: ProductCapabilityAvailability {
                template_match: CapabilityAvailability::unavailable("no_capability_handles"),
            },
        }
    }

    pub(crate) fn load(
        store: &rollshot_preset::PresetStore,
        preset_id: Option<&rollshot_preset::PresetId>,
    ) -> Result<Self, WorkbenchError> {
        let Some(preset_id) = preset_id else {
            return Ok(Self::empty());
        };
        let path = store
            .template_store_path(preset_id)
            .map_err(|_| WorkbenchError::RuntimeFailure)?;
        if !path.exists() {
            return Ok(Self::empty());
        }
        let template_store = rollshot_vision::TemplateStore::load_local(&path).map_err(|e| {
            WorkbenchError::VisionPrepare {
                message: format!("template store: {e}"),
            }
        })?;
        let template_summaries = template_store.summaries();
        let capability_handles = template_summaries
            .iter()
            .map(|summary| (summary.handle.clone(), summary.handle.clone()))
            .collect();
        let template_match = if template_summaries.is_empty() {
            CapabilityAvailability::unavailable("no_capability_handles")
        } else {
            CapabilityAvailability::available()
        };
        Ok(Self {
            capability_handles,
            template_store,
            template_summaries,
            availability: ProductCapabilityAvailability { template_match },
        })
    }
}
```

Temporary alias rule for Phase F v1: alias equals handle. Later UI template creation can add stable user-facing aliases without changing `AutomationInput`.

- [ ] **Step 5: Run focused tests and fix constructors until they pass**

Run:

```bash
rtk cargo test -p rollshot-app product_capability_bundle
```

Expected: pass.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/run.rs crates/rollshot-app/src/result_workspace/workbench/mod.rs crates/rollshot-app/src/result_workspace/update.rs
rtk git commit -m "feat(app): load preset capability bundles"
```

## Task 4: Template Preparation For Authoring And Existing Presets

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`

- [ ] **Step 1: Add failing tests for canonical template preparation**

Append to the `tests` module in `run.rs`:

```rust
#[test]
fn run_existing_preset_prepares_template_handles_from_bundle() {
    let mut image = image::RgbaImage::from_fn(80, 80, |x, y| {
        let v = 120 + ((x * 3 + y * 5) % 23) as u8;
        image::Rgba([v, v, v, 255])
    });
    for y in 0..8 {
        for x in 0..8 {
            let v = ((x * 17 + y * 31 + x * y * 3) % 255) as u8;
            image.put_pixel(20 + x, 24 + y, image::Rgba([v, v, v, 255]));
        }
    }
    let tpl = image::imageops::crop_imm(&image, 20, 24, 8, 8).to_image();
    let mut store = rollshot_vision::TemplateStore::new();
    store
        .insert(rollshot_vision::TemplateAsset {
            handle: "mark".into(),
            sensitivity: rollshot_vision::TemplateSensitivity::Chrome,
            source: rollshot_vision::TemplateSource::UserRect,
            created_at_ms: 1,
            bounds_in_source_image: None,
            bytes: rollshot_vision::TemplateBytes::new(8, 8, tpl.into_raw()).unwrap(),
        })
        .unwrap();
    let bundle = ProductCapabilityBundle::from_template_store_for_tests(store);
    let source = r#"
function main(input) {
  const matches = rollshot.templateMatch({
    templateHandle: input.capabilityHandles.mark,
    region: { kind: "full" },
    limit: 1
  });
  return {
    candidates: matches.map((match) => ({
      kind: "addRedaction",
      bounds: match.bounds,
      confidence: match.score,
      label: "mark"
    }))
  };
}
"#;
    let revision = make_revision_from_source(source);
    let policy = ExecutionPolicy::smart_redaction_default(
        std::time::Duration::from_secs(10),
        100_000_000,
        8_000_000,
    );

    let proposal = run_existing_preset_with_capabilities(&image, &revision, &policy, &bundle)
        .unwrap();

    assert_eq!(proposal.candidates.len(), 1);
}
```

- [ ] **Step 2: Run the focused test and verify it fails**

Run:

```bash
rtk cargo test -p rollshot-app run_existing_preset_prepares_template_handles_from_bundle
```

Expected: fail because `from_template_store_for_tests`, `run_existing_preset_with_capabilities`, and template preparation are not implemented.

- [ ] **Step 3: Add canonical template query preparation**

In `run.rs`, add:

```rust
const PHASE_F_TEMPLATE_MATCH_LIMIT: u32 = 8;

fn canonical_template_queries(
    image_width: u32,
    image_height: u32,
    handles: &std::collections::BTreeMap<String, String>,
) -> Vec<rollshot_automation::TemplateMatchQuery> {
    let regions = canonical_region_feature_catalog(image_width, image_height);
    handles
        .values()
        .flat_map(|handle| {
            regions.iter().filter_map(move |entry| {
                entry.query.as_ref().map(|region_query| rollshot_automation::TemplateMatchQuery {
                    template_handle: handle.clone(),
                    region: region_query.region,
                    limit: PHASE_F_TEMPLATE_MATCH_LIMIT,
                })
            })
        })
        .collect()
}

fn prepare_phase_f_templates(
    host: &mut rollshot_vision::RealAutomationHost,
    index: &VisualIndex,
    bundle: &ProductCapabilityBundle,
) -> Result<(), WorkbenchError> {
    for query in canonical_template_queries(index.width(), index.height(), &bundle.capability_handles) {
        match host.prepare_template_match(index, &bundle.template_store, &query) {
            Ok(()) => {}
            // Infeasible (handle, region) combinations are EXPECTED, not fatal:
            // a template can exceed the matcher's position/pixel-visit caps on a
            // large region, be larger than the region, or be too low-information
            // for NCC. Skip those and keep preparing the rest, mirroring how the
            // region-feature and OCR canonical catalogs omit over-cap regions
            // instead of failing the whole run. If the JS later calls a skipped
            // (handle, region) it gets `vision_index_unavailable` → the run
            // surfaces `RuntimeFailure` (the empty-bundle case is the one Task 6
            // upgrades to the clearer `CapabilityUnavailable`). Either way the
            // failure is explicit — never a silent prep abort for other handles.
            Err(rollshot_automation::CapabilityError::InvalidInput { code })
                if matches!(
                    code,
                    "region_too_large"
                        | "template_larger_than_region"
                        | "template_low_information"
                ) =>
            {
                tracing::debug!(
                    target: "rollshot::vision::template",
                    template_handle = %query.template_handle,
                    code,
                    "skipped infeasible template preparation"
                );
            }
            Err(e) => {
                return Err(WorkbenchError::VisionPrepare {
                    message: format!("templateMatch {}: {e}", query.template_handle),
                });
            }
        }
    }
    Ok(())
}
```

> **Why skip instead of `?`.** The matcher caps work at `MAX_SCORE_POSITIONS`
> (4M) and `MAX_TEMPLATE_MATCH_PIXEL_VISITS` (250M). A realistically-sized
> template (e.g. a ~100×100 logo) over a full 1920×1080 region needs billions of
> pixel-visits — far over the cap — so an eager `?` would abort *every* run
> before the agent writes any source. Skipping per (handle, region) keeps every
> feasible handle/region usable.
>
> **Prep cost (v1, accepted).** This prepares `O(handles × feasible canonical
> regions)` NCC scans per run (≤ 5 regions in the v1 catalog). `Region::Full` and
> an equal-area `Region::Rect` are distinct cache keys, so on small images the
> same pixels may be scanned more than once — bounded and acceptable for v1. If
> handle counts grow, revisit (dedupe by resolved pixel-rect, or prepare lazily).

- [ ] **Step 4: Add test-only bundle constructor and existing-preset helper**

Inside `impl ProductCapabilityBundle`, add:

```rust
    #[cfg(test)]
    pub(crate) fn from_template_store_for_tests(template_store: rollshot_vision::TemplateStore) -> Self {
        let template_summaries = template_store.summaries();
        let capability_handles = template_summaries
            .iter()
            .map(|summary| (summary.handle.clone(), summary.handle.clone()))
            .collect();
        let template_match = if template_summaries.is_empty() {
            CapabilityAvailability::unavailable("no_capability_handles")
        } else {
            CapabilityAvailability::available()
        };
        Self {
            capability_handles,
            template_store,
            template_summaries,
            availability: ProductCapabilityAvailability { template_match },
        }
    }
```

Split `run_existing_preset`:

```rust
pub fn run_existing_preset(
    image: &image::RgbaImage,
    revision: &AutomationRevision,
    policy: &ExecutionPolicy,
) -> Result<EditProposal, WorkbenchError> {
    run_existing_preset_with_capabilities(image, revision, policy, &ProductCapabilityBundle::empty())
}

pub(crate) fn run_existing_preset_with_capabilities(
    image: &image::RgbaImage,
    revision: &AutomationRevision,
    policy: &ExecutionPolicy,
    bundle: &ProductCapabilityBundle,
) -> Result<EditProposal, WorkbenchError> {
    let (w, h) = image.dimensions();
    let index = VisualIndex::build(image.clone()).map_err(|e| WorkbenchError::VisionPrepare {
        message: format!("VisualIndex: {e}"),
    })?;
    let mut host = rollshot_vision::RealAutomationHost::new();
    prepare_phase_a_region_features(&mut host, &index)?;
    #[cfg(feature = "ocr")]
    prepare_phase_b2_ocr(&mut host, &index)?;
    prepare_phase_f_templates(&mut host, &index, bundle)?;
    let executor = QuickJsExecutor;
    let cancellation = CancellationFlag::default();
    let input = AutomationInput {
        image_width: w,
        image_height: h,
        region: None,
        annotations: vec![],
        capability_handles: bundle.capability_handles.clone(),
    };
    let ctx = ProposalContext {
        proposal_id: ProposalId(1),
        base_document_state_id: 0,
        provenance: Provenance {
            source: ProvenanceSource::Manual,
        },
    };
    let (proposal, _metrics) = execute_to_proposal(
        &executor,
        &revision.artifact,
        &input,
        &ctx,
        &mut host,
        policy,
        &cancellation,
    )
    .map_err(|_| WorkbenchError::RuntimeFailure)?;
    Ok(proposal)
}
```

- [ ] **Step 4b: Add skip-on-cap and disk-load integration tests**

These lock in the skip-on-cap behavior from Step 3 and the `load()`→run seam
(Step 4's helper builds the bundle directly; this proves a bundle read from disk
also drives a real match). Add to the `tests` module in `run.rs`:

```rust
#[test]
fn infeasible_template_handle_is_skipped_not_fatal() {
    // Two handles: a feasible textured 8x8 "mark" and a flat 8x8 "flat"
    // (variance 0 -> template_low_information). The flat handle's preparation
    // must be skipped without aborting the run; the JS uses only "mark".
    let mut image = image::RgbaImage::from_fn(80, 80, |x, y| {
        let v = 120 + ((x * 3 + y * 5) % 23) as u8;
        image::Rgba([v, v, v, 255])
    });
    for y in 0..8 {
        for x in 0..8 {
            let v = ((x * 17 + y * 31 + x * y * 3) % 255) as u8;
            image.put_pixel(20 + x, 24 + y, image::Rgba([v, v, v, 255]));
        }
    }
    let tpl = image::imageops::crop_imm(&image, 20, 24, 8, 8).to_image();
    let mut store = rollshot_vision::TemplateStore::new();
    store
        .insert(rollshot_vision::TemplateAsset {
            handle: "mark".into(),
            sensitivity: rollshot_vision::TemplateSensitivity::Chrome,
            source: rollshot_vision::TemplateSource::UserRect,
            created_at_ms: 1,
            bounds_in_source_image: None,
            bytes: rollshot_vision::TemplateBytes::new(8, 8, tpl.into_raw()).unwrap(),
        })
        .unwrap();
    store
        .insert(rollshot_vision::TemplateAsset {
            handle: "flat".into(),
            sensitivity: rollshot_vision::TemplateSensitivity::Chrome,
            source: rollshot_vision::TemplateSource::UserRect,
            created_at_ms: 2,
            bounds_in_source_image: None,
            bytes: rollshot_vision::TemplateBytes::new(8, 8, vec![128u8; 8 * 8 * 4]).unwrap(),
        })
        .unwrap();
    let bundle = ProductCapabilityBundle::from_template_store_for_tests(store);
    let source = r#"
function main(input) {
  const matches = rollshot.templateMatch({
    templateHandle: input.capabilityHandles.mark,
    region: { kind: "full" },
    limit: 1
  });
  return { candidates: matches.map((match) => ({
    kind: "addRedaction", bounds: match.bounds, confidence: match.score, label: "mark"
  })) };
}
"#;
    let revision = make_revision_from_source(source);
    let policy = ExecutionPolicy::smart_redaction_default(
        std::time::Duration::from_secs(10),
        100_000_000,
        8_000_000,
    );

    // The flat handle is infeasible (skipped); the run still succeeds.
    let proposal =
        run_existing_preset_with_capabilities(&image, &revision, &policy, &bundle).unwrap();

    assert_eq!(proposal.candidates.len(), 1);
}

#[test]
fn bundle_loaded_from_disk_drives_existing_preset_match() {
    let tmp = tempfile::tempdir().unwrap();
    let store = rollshot_preset::PresetStore::open(tmp.path().to_path_buf());
    let preset_id = rollshot_preset::PresetId("preset-a".into());
    store
        .create_preset(
            preset_id.clone(),
            "Preset A".into(),
            "test".into(),
            "2026-06-28T00:00:00Z".into(),
        )
        .unwrap();

    let mut image = image::RgbaImage::from_fn(80, 80, |x, y| {
        let v = 120 + ((x * 3 + y * 5) % 23) as u8;
        image::Rgba([v, v, v, 255])
    });
    for y in 0..8 {
        for x in 0..8 {
            let v = ((x * 17 + y * 31 + x * y * 3) % 255) as u8;
            image.put_pixel(20 + x, 24 + y, image::Rgba([v, v, v, 255]));
        }
    }
    let tpl = image::imageops::crop_imm(&image, 20, 24, 8, 8).to_image();
    let mut templates = rollshot_vision::TemplateStore::new();
    templates
        .insert(rollshot_vision::TemplateAsset {
            handle: "mark".into(),
            sensitivity: rollshot_vision::TemplateSensitivity::Chrome,
            source: rollshot_vision::TemplateSource::UserRect,
            created_at_ms: 1,
            bounds_in_source_image: None,
            bytes: rollshot_vision::TemplateBytes::new(8, 8, tpl.into_raw()).unwrap(),
        })
        .unwrap();
    templates
        .save_local(&store.template_store_path(&preset_id).unwrap())
        .unwrap();

    let bundle = ProductCapabilityBundle::load(&store, Some(&preset_id)).unwrap();
    let source = r#"
function main(input) {
  const matches = rollshot.templateMatch({
    templateHandle: input.capabilityHandles.mark,
    region: { kind: "full" },
    limit: 1
  });
  return { candidates: matches.map((match) => ({
    kind: "addRedaction", bounds: match.bounds, confidence: match.score, label: "mark"
  })) };
}
"#;
    let revision = make_revision_from_source(source);
    let policy = ExecutionPolicy::smart_redaction_default(
        std::time::Duration::from_secs(10),
        100_000_000,
        8_000_000,
    );

    let proposal =
        run_existing_preset_with_capabilities(&image, &revision, &policy, &bundle).unwrap();

    assert_eq!(proposal.candidates.len(), 1);
}
```

Run:

```bash
rtk cargo test -p rollshot-app infeasible_template_handle_is_skipped_not_fatal
rtk cargo test -p rollshot-app bundle_loaded_from_disk_drives_existing_preset_match
```

Expected: both pass (the first proves Step 3's skip; the second proves the
disk round-trip drives a real match).

- [ ] **Step 5: Prepare templates in authoring runs**

Change `prepare_vision_context` to accept a bundle:

```rust
pub fn prepare_vision_context(
    image: &image::RgbaImage,
    bundle: &ProductCapabilityBundle,
) -> Result<super::VisionContext, WorkbenchError> {
    let index = VisualIndex::build(image.clone()).map_err(|e| WorkbenchError::VisionPrepare {
        message: format!("VisualIndex: {e}"),
    })?;
    let mut host = rollshot_vision::RealAutomationHost::new();
    prepare_phase_a_region_features(&mut host, &index)?;
    #[cfg(feature = "ocr")]
    prepare_phase_b2_ocr(&mut host, &index)?;
    prepare_phase_f_templates(&mut host, &index, bundle)?;
    Ok(super::VisionContext {
        index,
        host: Arc::new(StdMutex::new(host)),
        executor: QuickJsExecutor,
        cancellation: rollshot_automation::CancellationFlag::default(),
    })
}
```

In `start_agent_run`, load the bundle before `prepare_vision_context`:

```rust
let preset_store = rollshot_preset::PresetStore::open(params.preset_store_root.clone());
let capability_bundle = match ProductCapabilityBundle::load(&preset_store, Some(&params.preset_id)) {
    Ok(bundle) => bundle,
    Err(e) => {
        yield crate::result_workspace::Message::Workbench(
            super::WorkbenchMessage::RunFailed(e),
        );
        return;
    }
};
let vision = match prepare_vision_context(&image, &capability_bundle) {
    Ok(v) => v,
    Err(e) => {
        yield crate::result_workspace::Message::Workbench(
            super::WorkbenchMessage::RunFailed(e),
        );
        return;
    }
};
```

Pass `capability_bundle.capability_handles.clone()` into `ToolContext::new_with_capability_handles`.

- [ ] **Step 6: Run focused tests**

Run:

```bash
rtk cargo test -p rollshot-app workbench::run
```

Expected: pass.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/run.rs
rtk git commit -m "feat(app): prepare template capabilities for smart redaction"
```

## Task 5: Inspection, Prompt, And Revision Metadata Wiring

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/review.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-agent/src/driver.rs`

- [ ] **Step 1: Add failing tests for inspection and saved metadata**

In `run.rs`, add a test in `prepare_tests`:

```rust
#[tokio::test]
async fn inspect_image_context_reports_template_handles_available() {
    use rollshot_agent::tools::{InspectImageContextTool, Tool};

    let mut handles = std::collections::BTreeMap::new();
    handles.insert("mark".to_string(), "mark".to_string());
    let cancel = rollshot_agent::runtime::RunCancellation::new();
    let ctx = std::sync::Arc::new(rollshot_agent::tools::ToolContext::new_with_capability_handles(
        rollshot_agent::domain::SessionId::new(1),
        String::new(),
        rollshot_automation::ValidationLimits::default(),
        rollshot_automation::ExecutionPolicy::smart_redaction_default(
            std::time::Duration::from_secs(5),
            4 * 1024 * 1024,
            1024 * 1024,
        ),
        (64, 64),
        handles,
        &cancel,
    ));
    let inspection = authoring_inspection_context(
        PayloadMode::FullScreenshot,
        &canonical_region_feature_catalog(64, 64),
        &canonical_ocr_catalog(64, 64),
    );
    let tool = InspectImageContextTool::new(ctx, inspection);

    let result = tool.call(&serde_json::json!({})).await.unwrap();

    let rollshot_agent::tools::ToolOutcome::Success { result_json } = result;
    assert_eq!(
        result_json["capabilities"]["template_match"]["status"].as_str(),
        Some("available")
    );
    assert_eq!(result_json["capability_handles"][0]["name"].as_str(), Some("mark"));
}
```

In `review.rs`, add a test near existing save tests:

```rust
#[test]
fn save_revision_records_capability_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let store = PresetStore::open(tmp.path().to_path_buf());
    let preset_id = PresetId("test-preset".into());
    store
        .create_preset(
            preset_id.clone(),
            "Test".into(),
            "intent".into(),
            "2026-06-28T00:00:00Z".into(),
        )
        .unwrap();
    let metadata = rollshot_preset::RevisionCapabilityMetadata {
        requirements: vec![rollshot_preset::RevisionCapabilityRequirement {
            capability: rollshot_automation::CapabilityName::TemplateMatch,
            alias: Some("mark".into()),
            required: true,
        }],
        template_handles: vec![rollshot_preset::TemplateHandleMetadata {
            alias: "mark".into(),
            handle: "mark".into(),
            display_name: "mark".into(),
            sensitivity_sensitive: false,
            source_agent_suggested: false,
        }],
    };

    let saved = save_revision_with_capabilities(
        &store,
        &preset_id,
        r#"function main(input) { return { candidates: [] }; }"#,
        None,
        None,
        7,
        "2026-06-28T00:00:00Z".into(),
        metadata.clone(),
    )
    .unwrap();

    assert_eq!(saved.capabilities, metadata);
}
```

- [ ] **Step 2: Run the focused tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app inspect_image_context_reports_template_handles_available
rtk cargo test -p rollshot-app save_revision_records_capability_metadata
```

Expected: the inspection test **passes already** — `InspectImageContextTool`
already emits `capability_handles[].name` and flips `template_match` to
"available" when handles exist, so this is a *characterization/regression* test
that guards the existing inspection contract, not a red→green driver. The save
metadata test fails because `save_revision_with_capabilities` does not exist.

- [ ] **Step 3: Add metadata builder in workbench run**

In `run.rs`, add:

```rust
pub(crate) fn revision_capability_metadata(
    validated: &rollshot_automation::ValidatedAutomation,
    bundle: &ProductCapabilityBundle,
) -> rollshot_preset::RevisionCapabilityMetadata {
    let mut requirements = Vec::new();
    for call in &validated.workflow_ir.capability_manifest.calls {
        let exists = requirements.iter().any(|r: &rollshot_preset::RevisionCapabilityRequirement| {
            r.capability == call.capability && r.alias.is_none()
        });
        if !exists {
            requirements.push(rollshot_preset::RevisionCapabilityRequirement {
                capability: call.capability,
                alias: None,
                required: true,
            });
        }
    }
    let template_handles = bundle
        .template_summaries
        .iter()
        .map(|summary| rollshot_preset::TemplateHandleMetadata {
            alias: summary.handle.clone(),
            handle: summary.handle.clone(),
            display_name: summary.handle.clone(),
            sensitivity_sensitive: matches!(
                summary.sensitivity,
                rollshot_vision::TemplateSensitivity::Sensitive
            ),
            source_agent_suggested: matches!(
                summary.source,
                rollshot_vision::TemplateSource::AgentSuggested
            ),
        })
        .collect();
    rollshot_preset::RevisionCapabilityMetadata {
        requirements,
        template_handles,
    }
}
```

This records capability kinds from the validated manifest and the template aliases available at save time. It does not claim static knowledge of which alias a dynamic JS expression will use.

- [ ] **Step 4: Add metadata-aware save helper**

In `review.rs`, keep `save_revision` as a wrapper and add:

```rust
pub fn save_revision_with_capabilities(
    store: &PresetStore,
    preset_id: &PresetId,
    source: &str,
    parent_rev_id: Option<&RevisionId>,
    revision_note: Option<&str>,
    session_id: u64,
    now: String,
    capabilities: rollshot_preset::RevisionCapabilityMetadata,
) -> Result<AutomationRevision, WorkbenchError> {
    let limits = rollshot_automation::ValidationLimits::default();
    let artifact = rollshot_automation::validate_source(source, &limits)
        .map_err(|_| WorkbenchError::RuntimeFailure)?;
    let rev_id = RevisionId(format!("rev-{}", chrono::Utc::now().timestamp_millis()));
    let revision = store
        .add_revision_with_capabilities(
            preset_id,
            rev_id.clone(),
            parent_rev_id.cloned(),
            artifact,
            RevisionProvenance {
                origin: RevisionOrigin::AgentRun,
                note: revision_note.map(str::to_string),
                source_run_ref: Some(format!("session:{session_id}")),
            },
            now.clone(),
            capabilities,
        )
        .map_err(|_| WorkbenchError::RuntimeFailure)?;
    store
        .set_active_revision(preset_id, &rev_id, now)
        .map_err(|_| WorkbenchError::RuntimeFailure)?;
    store
        .load_active_revision(preset_id)
        .map_err(|_| WorkbenchError::RuntimeFailure)?;
    Ok(revision)
}
```

Then make `save_revision` call `save_revision_with_capabilities(..., RevisionCapabilityMetadata::default())`.

- [ ] **Step 5: Thread metadata through save action**

In `update.rs`, inside `SavePresetOrRevision`, load the same product capability bundle used for runs:

```rust
let capability_bundle = super::workbench::run::ProductCapabilityBundle::load(
    &store,
    Some(&preset_id),
)
.unwrap_or_else(|_| super::workbench::run::ProductCapabilityBundle::empty());
let limits = rollshot_automation::ValidationLimits::default();
let metadata = rollshot_automation::validate_source(&draft.source, &limits)
    .map(|validated| {
        super::workbench::run::revision_capability_metadata(&validated, &capability_bundle)
    })
    .unwrap_or_default();
```

Call `save_revision_with_capabilities` instead of `save_revision`.

- [ ] **Step 6: Update system prompt contract**

The prompt **already** covers most of this — `SMART_REDACTION_SYSTEM_PROMPT`
already says "Use only template handles listed by inspect_image_context
capability_handles before calling rollshot.templateMatch. Do not invent template
handles when that list is empty." Do **not** duplicate those lines.

Add only the one genuinely-new fragment — the alias *access pattern* — to
`SMART_REDACTION_SYSTEM_PROMPT` in `driver.rs`, right after the existing
"Do not invent template handles…" line:

```text
- Refer to template handles through input.capabilityHandles.<alias>; do not hard-code raw handle strings.
```

There is **no** `provider_contract` test module. The prompt-contract assertions
live in `mod provider_path` (the test that already asserts "Do not invent
template handles"). Extend that existing test with one assertion:

```rust
assert!(
    system_prompt.contains("Refer to template handles through input.capabilityHandles"),
    "system prompt should teach alias access for template handles, got: {:?}",
    system_prompt
);
```

- [ ] **Step 7: Run focused tests**

Run:

```bash
rtk cargo test -p rollshot-app inspect_image_context_reports_template_handles_available
rtk cargo test -p rollshot-app save_revision_records_capability_metadata
rtk cargo test -p rollshot-agent provider_path
```

Expected: pass. (`provider_path` is the existing prompt-contract test module;
running it exercises the extended alias-access assertion.)

- [ ] **Step 8: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/run.rs crates/rollshot-app/src/result_workspace/workbench/review.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-agent/src/driver.rs
rtk git commit -m "feat(agent): persist smart redaction capability metadata"
```

## Task 6: Capability Availability Errors And Full Verification

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/workbench/state.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/run.rs`

- [ ] **Step 1: Add failing test for missing template availability report**

In `run.rs`, add:

```rust
#[test]
fn template_using_existing_preset_without_handles_reports_capability_unavailable() {
    let source = r#"
function main(input) {
  const matches = rollshot.templateMatch({
    templateHandle: input.capabilityHandles.mark,
    region: { kind: "full" },
    limit: 1
  });
  return { candidates: matches.map((match) => ({
    kind: "addRedaction",
    bounds: match.bounds,
    confidence: match.score,
    label: "mark"
  })) };
}
"#;
    let image = image::RgbaImage::from_pixel(80, 80, image::Rgba([120, 120, 120, 255]));
    let revision = make_revision_from_source(source);
    let policy = ExecutionPolicy::smart_redaction_default(
        std::time::Duration::from_secs(10),
        100_000_000,
        8_000_000,
    );

    let err = run_existing_preset_with_capabilities(
        &image,
        &revision,
        &policy,
        &ProductCapabilityBundle::empty(),
    )
    .unwrap_err();

    assert!(matches!(
        err,
        WorkbenchError::CapabilityUnavailable { .. }
    ));
}
```

- [ ] **Step 2: Run the focused test and verify it fails**

Run:

```bash
rtk cargo test -p rollshot-app template_using_existing_preset_without_handles_reports_capability_unavailable
```

Expected: fail because `WorkbenchError::CapabilityUnavailable` does not exist and runtime errors collapse to `RuntimeFailure`.

- [ ] **Step 3: Add explicit availability error**

In `state.rs`, add a variant:

```rust
    CapabilityUnavailable { message: String },
```

Update any `Display`/view mapping for `WorkbenchError` to display the message. Use existing error-rendering style; do not add a new panel.

In `run.rs`, before executing a template-capability revision with an empty bundle, return:

```rust
fn revision_requires_template_match(revision: &AutomationRevision) -> bool {
    revision
        .artifact
        .workflow_ir
        .capability_manifest
        .calls
        .iter()
        .any(|call| call.capability == rollshot_automation::CapabilityName::TemplateMatch)
}
```

At the start of `run_existing_preset_with_capabilities`, after building the visual index:

```rust
if revision_requires_template_match(revision) && bundle.capability_handles.is_empty() {
    return Err(WorkbenchError::CapabilityUnavailable {
        message: "This preset uses template matching, but no template handles are available for this preset.".into(),
    });
}
```

- [ ] **Step 4: Run app and preset focused tests**

Run:

```bash
rtk cargo test -p rollshot-app workbench::run
rtk cargo test -p rollshot-preset
rtk cargo test -p rollshot-vision template::tests
```

Expected: pass.

- [ ] **Step 5: Run formatting and lint checks**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: pass. If OCR-related compilation paths are not covered by the default workspace, also run:

```bash
rtk cargo test -p rollshot-app --features ocr inspect_image_context_reports_template_handles_available
```

Expected: pass or skip only if local OCR dependencies are unavailable; record the exact failure if unavailable.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/workbench/state.rs crates/rollshot-app/src/result_workspace/workbench/run.rs
rtk git commit -m "feat(app): report missing smart redaction capabilities"
```

## Self-Review Checklist

- Phase F roadmap coverage:
  - Created/named/persisted handles: v1 uses preset-local `TemplateStore` with alias-equals-handle and durable sidecar path.
  - Passed through `AutomationInput.capability_handles`: covered by `ProductCapabilityBundle` and `ToolContext::new_with_capability_handles`.
  - Availability metadata: covered by `RevisionCapabilityMetadata`, inspection status, and `CapabilityUnavailable`.
  - First-class v1 capabilities: region features and OCR stay as existing canonical prepared capabilities; template handles become available only when a preset-local store exists.
- Reference harness coverage:
  - Tool output stays structured.
  - Read-only inspection remains truthful.
  - State-changing template/revision work remains serialized in app update/save flow.
- Explicit non-goals:
  - No agent-created template assets.
  - No crop UI.
  - No arbitrary template region preparation.
- Verification:
  - Focused crate tests for `rollshot-vision`, `rollshot-preset`, `rollshot-app`, and `rollshot-agent`.
  - Workspace format and clippy checks.
- Robustness:
  - Infeasible `(handle, region)` preparations (over-cap, larger-than-region,
    low-information) are skipped per combination, never fatal to the run
    (Task 4 Step 3 + `infeasible_template_handle_is_skipped_not_fatal`).

## Phase F v1 Known Limitations (carry into the next phase)

- **No product caller exercises templates end-to-end yet.** `run_existing_preset`
  is test-only today; the existing-preset run path is not wired into the product
  UI. Tasks 4 and 6 are infrastructure — their value is verified by tests, not by
  a live UI flow.
- **No template-creation UI.** Nothing in product code writes
  `templates.local.json`, so the `workbench-draft` preset always loads an empty
  bundle and the authoring path reports `no_capability_handles` until a later
  phase adds crop-to-template creation. This is by design (see Scope), but means
  the alias plumbing is dormant in the product until then.
- **Canonical-region preparation only.** Template matching is prepared for the
  five canonical regions (full + 4 strips). JS that calls `templateMatch` with an
  arbitrary `rect` region misses the prepared cache and gets
  `vision_index_unavailable` → `CapabilityUnavailable`.

## Parallelization Note (for subagent-driven execution)

| Task | Modules touched | Depends on |
|------|-----------------|------------|
| Task 1 | `crates/rollshot-vision/` | — |
| Task 2 | `crates/rollshot-preset/` (+ workspace `AutomationRevision` literals) | — |
| Task 3 | `crates/rollshot-app/` (`run.rs`, `mod.rs`, `update.rs`) | Task 1, Task 2 |
| Task 4 | `crates/rollshot-app/` (`run.rs`) | Task 3 |
| Task 5 | `crates/rollshot-app/` (`run.rs`, `review.rs`, `update.rs`), `crates/rollshot-agent/` (`driver.rs`) | Task 2, Task 3, Task 4 |
| Task 6 | `crates/rollshot-app/` (`state.rs`, `run.rs`) | Task 4 |

- **Lane A:** Task 1 (independent).
- **Lane B:** Task 2 (independent at the source level).
- **Lane C:** Task 3 → 4 → 5 → 6 — all share `rollshot-app` (`run.rs` in
  particular), so they are strictly sequential.
- **Critical sequencing:** Task 2 adds a required `AutomationRevision` field, so
  after Task 2's commit `rollshot-app` does not compile until Task 3 fixes its
  three literals. **Do not merge Lane B (Task 2) standalone** — land it together
  with the start of Lane C. Effective parallelism is therefore limited to
  Lane A ∥ Lane B, then Lane C sequentially.
- No task modifies the root `Cargo.toml` (no new crate/member) — no
  workspace-root serialization beyond the Task 2 → Task 3 ordering above.
