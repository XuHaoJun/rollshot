# Local Issue Pack Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a GUI-only Local Issue Pack export that writes a review-gated Markdown/manifest evidence folder, with optional ZIP packaging, from Result Workspace screenshots and Action Guide sessions.

**Architecture:** Add `crates/rollshot-app/src/issue_pack.rs` as an app-level composition layer. Keep rendering, manifest modeling, staging/rollback, and ZIP packaging in the module; keep UI state and message wiring in the existing Result Workspace and Timeline Workspace modules. The exporter receives prepared assets and never inspects iced state directly.

**Tech Stack:** Rust, iced 0.14, `image`, `chrono`, `serde`/`serde_json`, `rfd`, `zip`, existing `rollshot-action` export/GIF helpers, existing `rollshot-image-document` flattening.

---

## Assumptions And Boundaries

- The user chooses a destination parent directory; the exporter creates `rollshot-issue-pack-YYYY-MM-DD-HHMM/` inside it.
- Folder export and ZIP export both write the folder first. ZIP export then packages the completed folder as `rollshot-issue-pack-YYYY-MM-DD-HHMM.zip`.
- Result Workspace packs always include `images/final-redacted.png`, produced from `ImageDocument::flatten()`.
- Action Guide-only packs require at least one reviewed step with retained keyframe pixels.
- Action Guide keyframes are reviewed evidence images, not redacted outputs.
- OCR snippets are optional and feature-gated behind `ocr`; missing OCR omits the section.
- This plan intentionally does not add CLI export, tracker API writes, hosted pages, browser logs, AI narrative generation, or automatic keyframe redaction.

## File Map

- Create `crates/rollshot-app/src/issue_pack.rs`: pure Issue Pack model, Markdown renderer, manifest renderer, folder staging/rollback, optional GIF warning handling, ZIP packaging, and core tests.
- Modify `crates/rollshot-app/src/main.rs`: register `mod issue_pack;`.
- Modify `Cargo.toml`: add workspace `zip` dependency.
- Modify `crates/rollshot-app/Cargo.toml`: depend on workspace `zip`.
- Modify `crates/rollshot-app/src/result_workspace/mod.rs`: add `issue_pack` dialog state to `ResultWorkspace`.
- Modify `crates/rollshot-app/src/result_workspace/update.rs`: add Result Workspace export messages, pending-candidate block, folder picker, input preparation, and export completion handling.
- Modify `crates/rollshot-app/src/result_workspace/view.rs`: add `Export Bug Report...` toolbar command and compact review modal.
- Modify `crates/rollshot-app/src/timeline_workspace/mod.rs`: add Issue Pack dialog state to `TimelineWorkspace`.
- Modify `crates/rollshot-app/src/timeline_workspace/update.rs`: add Action Guide Issue Pack export messages, input preparation, and export completion handling.
- Modify `crates/rollshot-app/src/timeline_workspace/view.rs`: add `Export Bug Report...` toolbar command and Action Guide review modal.
- No direct changes are required in `crates/rollshot-app/src/macos_product.rs`; it forwards workspace/timeline messages already. Re-run macOS-gated checks if developing on macOS.

---

### Task 1: Add The Pure Issue Pack Model And Renderer

**Files:**
- Create: `crates/rollshot-app/src/issue_pack.rs`
- Modify: `crates/rollshot-app/src/main.rs`
- Test: `crates/rollshot-app/src/issue_pack.rs`

- [ ] **Step 1: Register the module**

In `crates/rollshot-app/src/main.rs`, add this near the existing module list:

```rust
mod issue_pack;
```

- [ ] **Step 2: Add model types, renderer stubs, and failing renderer tests**

Create `crates/rollshot-app/src/issue_pack.rs` with model types, stub renderer functions, and tests first. The renderer helpers intentionally use `todo!()` in this step so the next run proves the tests are red before implementation:

```rust
use chrono::{DateTime, Local};
use image::RgbaImage;
use serde::Serialize;
use std::path::{Path, PathBuf};

pub(crate) const SCHEMA_VERSION: u32 = 1;
pub(crate) const EXPORT_MODE: &str = "local_issue_pack";
pub(crate) const TARGET_ISSUE_PACK_EXPORT: &str = "rollshot::issue_pack::export";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlatformInfo {
    pub os: String,
    pub arch: String,
}

impl PlatformInfo {
    pub(crate) fn current() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EvidenceReviewSummary {
    pub required: bool,
    pub completed: bool,
    pub result_workspace_images_reviewed: bool,
    pub action_guide_keyframes_reviewed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RedactionSummary {
    pub review_required: bool,
    pub review_completed: bool,
    pub result_workspace_images_are_flattened: bool,
    pub original_pixels_included: bool,
    pub redaction_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SafeImageAsset {
    pub file_name: String,
    pub pixels: RgbaImage,
    pub derived_from_original: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OcrSnippet {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IssuePackStep {
    pub index: usize,
    pub title: String,
    pub keyframe_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActionGuideIssueAssets {
    pub steps: Vec<IssuePackStep>,
    pub include_gif: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IssuePackInput {
    pub title: Option<String>,
    pub created_at: DateTime<Local>,
    pub rollshot_version: String,
    pub platform: PlatformInfo,
    pub final_image: Option<SafeImageAsset>,
    pub action_guide: Option<ActionGuideIssueAssets>,
    pub ocr_snippets: Vec<OcrSnippet>,
    pub evidence_review: EvidenceReviewSummary,
    pub redaction: RedactionSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AssetEntry {
    pub kind: String,
    pub path: String,
}

pub(crate) fn issue_pack_folder_name(created_at: DateTime<Local>) -> String {
    format!("rollshot-issue-pack-{}", created_at.format("%Y-%m-%d-%H%M"))
}

pub(crate) fn render_issue_markdown(input: &IssuePackInput) -> String {
    let _ = input;
    todo!("render issue markdown")
}

pub(crate) fn manifest_assets(input: &IssuePackInput, include_gif: bool) -> Vec<AssetEntry> {
    let _ = (input, include_gif);
    todo!("manifest assets")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use image::{Rgba, RgbaImage};

    pub(super) fn base_input() -> IssuePackInput {
        IssuePackInput {
            title: None,
            created_at: Local.with_ymd_and_hms(2026, 7, 4, 15, 30, 0).unwrap(),
            rollshot_version: "0.1.0".to_string(),
            platform: PlatformInfo {
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
            },
            final_image: Some(SafeImageAsset {
                file_name: "final-redacted.png".to_string(),
                pixels: RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])),
                derived_from_original: true,
            }),
            action_guide: None,
            ocr_snippets: vec![],
            evidence_review: EvidenceReviewSummary {
                required: true,
                completed: true,
                result_workspace_images_reviewed: true,
                action_guide_keyframes_reviewed: false,
            },
            redaction: RedactionSummary {
                review_required: true,
                review_completed: true,
                result_workspace_images_are_flattened: true,
                original_pixels_included: false,
                redaction_count: 0,
            },
        }
    }

    #[test]
    fn folder_name_is_deterministic() {
        assert_eq!(
            issue_pack_folder_name(base_input().created_at),
            "rollshot-issue-pack-2026-07-04-1530"
        );
    }

    #[test]
    fn renders_screenshot_only_markdown_with_relative_link() {
        let md = render_issue_markdown(&base_input());
        assert!(md.contains("![](images/final-redacted.png)"), "md = {md}");
        assert!(!md.contains("/tmp/"), "md must not contain absolute paths: {md}");
        assert!(md.contains("- `manifest.json`"), "md = {md}");
    }

    #[test]
    fn renders_action_guide_steps_and_omits_missing_ocr() {
        let mut input = base_input();
        input.final_image = None;
        input.action_guide = Some(ActionGuideIssueAssets {
            include_gif: false,
            steps: vec![
                IssuePackStep {
                    index: 1,
                    title: "Open Settings".to_string(),
                    keyframe_path: "action-guide/keyframes/001.png".to_string(),
                },
                IssuePackStep {
                    index: 2,
                    title: "Click Save".to_string(),
                    keyframe_path: "action-guide/keyframes/002.png".to_string(),
                },
            ],
        });
        let md = render_issue_markdown(&input);
        assert!(md.contains("1. Open Settings"), "md = {md}");
        assert!(md.contains("![](action-guide/keyframes/001.png)"), "md = {md}");
        assert!(!md.contains("## OCR snippets"), "md = {md}");
    }

    #[test]
    fn renders_ocr_snippets_when_available() {
        let mut input = base_input();
        input.ocr_snippets = vec![OcrSnippet {
            text: "Failed to save settings".to_string(),
        }];
        let md = render_issue_markdown(&input);
        assert!(md.contains("## OCR snippets"), "md = {md}");
        assert!(md.contains("- Failed to save settings"), "md = {md}");
    }

    #[test]
    fn manifest_assets_list_every_expected_relative_path() {
        let mut input = base_input();
        input.action_guide = Some(ActionGuideIssueAssets {
            include_gif: true,
            steps: vec![IssuePackStep {
                index: 1,
                title: "Open Settings".to_string(),
                keyframe_path: "action-guide/keyframes/001.png".to_string(),
            }],
        });
        let assets = manifest_assets(&input, true);
        let paths: Vec<_> = assets.iter().map(|a| a.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "issue.md",
                "manifest.json",
                "images/final-redacted.png",
                "action-guide/steps.md",
                "action-guide/session.json",
                "action-guide/keyframes/001.png",
                "action-guide/guide.gif",
            ]
        );
    }
}
```

- [ ] **Step 3: Run the focused tests and verify they fail**

Run:

```bash
rtk cargo test -p rollshot-app issue_pack -- --nocapture
```

Expected: FAIL with `not yet implemented: render issue markdown` or `not yet implemented: manifest assets`.

- [ ] **Step 4: Implement the renderer helpers**

Replace the stub bodies from Step 2 with:

```rust
pub(crate) fn render_issue_markdown(input: &IssuePackInput) -> String {
    let mut md = String::from("# Bug Report\n\n");
    md.push_str("## Summary\n\n[Write a short summary]\n\n");
    md.push_str("## Steps to reproduce\n\n");
    if let Some(action) = &input.action_guide {
        for step in &action.steps {
            md.push_str(&format!(
                "{}. {}\n\n   ![]({})\n\n",
                step.index, step.title, step.keyframe_path
            ));
        }
    } else {
        md.push_str("[Write the steps to reproduce]\n\n");
    }
    md.push_str("## Actual result\n\n");
    if let Some(image) = &input.final_image {
        md.push_str("The UI reached this state:\n\n");
        md.push_str(&format!("![](images/{})\n\n", image.file_name));
    } else {
        md.push_str("[Describe what happened]\n\n");
    }
    md.push_str("## Expected result\n\n[Write what should have happened]\n\n");
    if !input.ocr_snippets.is_empty() {
        md.push_str("## OCR snippets\n\n");
        for snippet in &input.ocr_snippets {
            md.push_str(&format!("- {}\n", snippet.text));
        }
        md.push('\n');
    }
    md.push_str("## Environment\n\n");
    md.push_str(&format!("- OS: {}\n", input.platform.os));
    md.push_str(&format!("- Architecture: {}\n", input.platform.arch));
    md.push_str(&format!("- Rollshot version: {}\n\n", input.rollshot_version));
    md.push_str("## Attachments\n\n");
    if input.action_guide.is_some() {
        md.push_str("- `action-guide/steps.md`\n");
        md.push_str("- `action-guide/session.json`\n");
    }
    md.push_str("- `manifest.json`\n");
    md
}

pub(crate) fn manifest_assets(input: &IssuePackInput, include_gif: bool) -> Vec<AssetEntry> {
    let mut assets = vec![
        AssetEntry {
            kind: "issue_markdown".to_string(),
            path: "issue.md".to_string(),
        },
        AssetEntry {
            kind: "manifest".to_string(),
            path: "manifest.json".to_string(),
        },
    ];
    if let Some(image) = &input.final_image {
        assets.push(AssetEntry {
            kind: "final_redacted_image".to_string(),
            path: format!("images/{}", image.file_name),
        });
    }
    if let Some(action) = &input.action_guide {
        assets.push(AssetEntry {
            kind: "action_steps".to_string(),
            path: "action-guide/steps.md".to_string(),
        });
        assets.push(AssetEntry {
            kind: "action_session".to_string(),
            path: "action-guide/session.json".to_string(),
        });
        for step in &action.steps {
            assets.push(AssetEntry {
                kind: "action_keyframe".to_string(),
                path: step.keyframe_path.clone(),
            });
        }
        if include_gif {
            assets.push(AssetEntry {
                kind: "action_gif".to_string(),
                path: "action-guide/guide.gif".to_string(),
            });
        }
    }
    assets
}
```

- [ ] **Step 5: Run the focused tests and verify they pass**

Run:

```bash
rtk cargo test -p rollshot-app issue_pack -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
rtk git add crates/rollshot-app/src/main.rs crates/rollshot-app/src/issue_pack.rs
rtk git commit -m "feat(app): add issue pack render model"
```

---

### Task 2: Add Manifest JSON, Validation, And Folder Staging

**Files:**
- Modify: `crates/rollshot-app/src/issue_pack.rs`
- Test: `crates/rollshot-app/src/issue_pack.rs`

- [ ] **Step 1: Add failing export tests**

Append these tests to the existing `#[cfg(test)] mod tests` in `issue_pack.rs`:

```rust
#[test]
fn export_folder_writes_required_files_and_flattened_image() {
    let input = base_input();
    let tmp = tempfile::tempdir().unwrap();
    let result = export_folder(&input, tmp.path()).unwrap();

    assert!(result.directory.ends_with("rollshot-issue-pack-2026-07-04-1530"));
    assert!(result.markdown_path.exists());
    assert!(result.manifest_path.exists());
    assert!(result.directory.join("images/final-redacted.png").exists());
    assert!(result.zip_path.is_none());
    assert!(result.warnings.is_empty());

    let decoded = image::open(result.directory.join("images/final-redacted.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(decoded.as_raw(), input.final_image.unwrap().pixels.as_raw());
}

#[test]
fn export_blocks_when_evidence_review_is_not_confirmed() {
    let mut input = base_input();
    input.evidence_review.completed = false;
    input.redaction.review_completed = false;
    let tmp = tempfile::tempdir().unwrap();
    let err = export_folder(&input, tmp.path()).unwrap_err();

    assert_eq!(err, IssuePackError::EvidenceReviewRequired);
    assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
}

#[test]
fn export_rejects_pack_without_result_or_action_evidence() {
    let mut input = base_input();
    input.final_image = None;
    input.action_guide = None;
    let tmp = tempfile::tempdir().unwrap();
    let err = export_folder(&input, tmp.path()).unwrap_err();

    assert_eq!(err, IssuePackError::MissingEvidence);
    assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
}

#[test]
fn manifest_json_records_review_redaction_ocr_and_assets() {
    let mut input = base_input();
    input.ocr_snippets = vec![OcrSnippet {
        text: "Visible error".to_string(),
    }];
    input.redaction.redaction_count = 2;
    let tmp = tempfile::tempdir().unwrap();
    let result = export_folder(&input, tmp.path()).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(result.manifest_path).unwrap()).unwrap();

    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["export_mode"], "local_issue_pack");
    assert_eq!(json["redaction"]["original_pixels_included"], false);
    assert_eq!(json["redaction"]["redaction_count"], 2);
    assert_eq!(json["ocr"]["included"], true);
    assert_eq!(json["ocr"]["snippet_count"], 1);
    assert_eq!(json["assets"][0]["path"], "issue.md");
    assert_eq!(json["assets"][1]["path"], "manifest.json");
}
```

- [ ] **Step 2: Add export result, error, manifest structs, and validation**

Add this below the renderer helpers in `issue_pack.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IssuePackExportResult {
    pub directory: PathBuf,
    pub markdown_path: PathBuf,
    pub manifest_path: PathBuf,
    pub zip_path: Option<PathBuf>,
    pub warnings: Vec<IssuePackWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct IssuePackWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IssuePackError {
    EvidenceReviewRequired,
    MissingEvidence,
    Io(String),
    Encode(String),
    Json(String),
}

impl std::fmt::Display for IssuePackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EvidenceReviewRequired => write!(f, "review the included evidence before export"),
            Self::MissingEvidence => write!(f, "nothing to export: add a final image or reviewed Action Guide"),
            Self::Io(e) => write!(f, "issue pack file error: {e}"),
            Self::Encode(e) => write!(f, "issue pack image error: {e}"),
            Self::Json(e) => write!(f, "issue pack manifest error: {e}"),
        }
    }
}

impl std::error::Error for IssuePackError {}

impl IssuePackError {
    fn category(&self) -> &'static str {
        match self {
            Self::EvidenceReviewRequired => "review_required",
            Self::MissingEvidence => "missing_evidence",
            Self::Io(_) => "io",
            Self::Encode(_) => "encode",
            Self::Json(_) => "json",
        }
    }
}

#[derive(Debug, Serialize)]
struct Manifest<'a> {
    schema_version: u32,
    created_at: String,
    rollshot_version: &'a str,
    export_mode: &'static str,
    evidence_review: EvidenceReviewManifest,
    platform: PlatformManifest<'a>,
    redaction: RedactionManifest,
    assets: Vec<AssetEntry>,
    ocr: OcrManifest,
    warnings: &'a [IssuePackWarning],
}

#[derive(Debug, Serialize)]
struct EvidenceReviewManifest {
    required: bool,
    completed: bool,
    result_workspace_images_reviewed: bool,
    action_guide_keyframes_reviewed: bool,
}

#[derive(Debug, Serialize)]
struct PlatformManifest<'a> {
    os: &'a str,
    arch: &'a str,
}

#[derive(Debug, Serialize)]
struct RedactionManifest {
    review_required: bool,
    review_completed: bool,
    result_workspace_images_are_flattened: bool,
    original_pixels_included: bool,
    redaction_count: usize,
}

#[derive(Debug, Serialize)]
struct OcrManifest {
    included: bool,
    snippet_count: usize,
}

fn validate(input: &IssuePackInput) -> Result<(), IssuePackError> {
    if input.evidence_review.required && !input.evidence_review.completed {
        return Err(IssuePackError::EvidenceReviewRequired);
    }
    if input.redaction.review_required && !input.redaction.review_completed {
        return Err(IssuePackError::EvidenceReviewRequired);
    }
    if input.final_image.is_none()
        && input
            .action_guide
            .as_ref()
            .is_none_or(|action| action.steps.is_empty())
    {
        return Err(IssuePackError::MissingEvidence);
    }
    Ok(())
}

fn render_manifest_json(
    input: &IssuePackInput,
    warnings: &[IssuePackWarning],
    include_gif: bool,
) -> Result<String, IssuePackError> {
    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        created_at: input.created_at.to_rfc3339(),
        rollshot_version: &input.rollshot_version,
        export_mode: EXPORT_MODE,
        evidence_review: EvidenceReviewManifest {
            required: input.evidence_review.required,
            completed: input.evidence_review.completed,
            result_workspace_images_reviewed: input.evidence_review.result_workspace_images_reviewed,
            action_guide_keyframes_reviewed: input.evidence_review.action_guide_keyframes_reviewed,
        },
        platform: PlatformManifest {
            os: &input.platform.os,
            arch: &input.platform.arch,
        },
        redaction: RedactionManifest {
            review_required: input.redaction.review_required,
            review_completed: input.redaction.review_completed,
            result_workspace_images_are_flattened: input.redaction.result_workspace_images_are_flattened,
            original_pixels_included: input.redaction.original_pixels_included,
            redaction_count: input.redaction.redaction_count,
        },
        assets: manifest_assets(input, include_gif),
        ocr: OcrManifest {
            included: !input.ocr_snippets.is_empty(),
            snippet_count: input.ocr_snippets.len(),
        },
        warnings,
    };
    serde_json::to_string_pretty(&manifest).map_err(|e| IssuePackError::Json(e.to_string()))
}
```

- [ ] **Step 3: Add folder staging and rollback**

Add this implementation:

```rust
pub(crate) fn export_folder(
    input: &IssuePackInput,
    destination_parent: &Path,
) -> Result<IssuePackExportResult, IssuePackError> {
    validate(input)?;
    let folder_name = issue_pack_folder_name(input.created_at);
    let final_dir = destination_parent.join(&folder_name);
    let tmp_dir = destination_parent.join(format!(".{folder_name}.tmp"));

    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir).map_err(|e| IssuePackError::Io(e.to_string()))?;
    }

    let warnings = Vec::new();
    let build_result = build_folder(input, &tmp_dir, &warnings)
        .and_then(|()| swap_folder(&tmp_dir, &final_dir));
    if let Err(error) = build_result {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return Err(error);
    }

    Ok(IssuePackExportResult {
        markdown_path: final_dir.join("issue.md"),
        manifest_path: final_dir.join("manifest.json"),
        directory: final_dir,
        zip_path: None,
        warnings,
    })
}

fn build_folder(
    input: &IssuePackInput,
    tmp_dir: &Path,
    warnings: &[IssuePackWarning],
) -> Result<(), IssuePackError> {
    std::fs::create_dir_all(tmp_dir).map_err(|e| IssuePackError::Io(e.to_string()))?;

    if let Some(image) = &input.final_image {
        let images_dir = tmp_dir.join("images");
        std::fs::create_dir_all(&images_dir).map_err(|e| IssuePackError::Io(e.to_string()))?;
        image
            .pixels
            .save_with_format(images_dir.join(&image.file_name), image::ImageFormat::Png)
            .map_err(|e| IssuePackError::Encode(e.to_string()))?;
    }

    std::fs::write(tmp_dir.join("issue.md"), render_issue_markdown(input))
        .map_err(|e| IssuePackError::Io(e.to_string()))?;
    let manifest = render_manifest_json(input, warnings, false)?;
    std::fs::write(tmp_dir.join("manifest.json"), manifest)
        .map_err(|e| IssuePackError::Io(e.to_string()))?;
    Ok(())
}

fn swap_folder(tmp_dir: &Path, final_dir: &Path) -> Result<(), IssuePackError> {
    if final_dir.exists() {
        std::fs::remove_dir_all(final_dir).map_err(|e| IssuePackError::Io(e.to_string()))?;
    }
    std::fs::rename(tmp_dir, final_dir).map_err(|e| IssuePackError::Io(e.to_string()))
}
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
rtk cargo test -p rollshot-app issue_pack -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/issue_pack.rs
rtk git commit -m "feat(app): export issue pack folders"
```

---

### Task 3: Integrate Action Guide Assets In The Exporter

**Files:**
- Modify: `crates/rollshot-app/src/issue_pack.rs`
- Test: `crates/rollshot-app/src/issue_pack.rs`

- [ ] **Step 1: Write failing Action Guide export tests**

Append under `#[cfg(all(test, feature = "action-guide"))]` in `issue_pack.rs`:

```rust
#[cfg(all(test, feature = "action-guide"))]
mod action_guide_tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use rollshot_action::{
        ActionRecorder, CandidateKind, CandidateStep, CaptureRegion, DetectReason, DetectorConfig,
        FrameStore, Guide, InputCapability, InputSourceKind, Recording, StoreConfig,
    };

    fn region() -> CaptureRegion {
        CaptureRegion {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        }
    }

    fn black() -> RgbaImage {
        RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]))
    }

    fn quadrant() -> RgbaImage {
        let mut image = black();
        for y in 0..4 {
            for x in 0..4 {
                image.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        image
    }

    fn recording() -> Recording {
        let detector = DetectorConfig {
            diff_threshold: 0.01,
            area_threshold: 0.05,
            cooldown_ms: 0,
            ..DetectorConfig::default()
        };
        let mut recorder = ActionRecorder::new(region(), StoreConfig::default(), detector);
        recorder.ingest_frame(black(), 0);
        for i in 1..=6 {
            recorder.ingest_frame(quadrant(), i * 100);
        }
        let recording = recorder.finish();
        assert!(!recording.candidates.is_empty());
        recording
    }

    fn action_input() -> (IssuePackInput, Guide, FrameStore, CaptureRegion, InputCapability, InputSourceKind) {
        let recording = recording();
        let guide = Guide::from_candidates(recording.candidates);
        let store = recording.store;
        let mut input = super::tests::base_input();
        input.final_image = None;
        input.evidence_review.result_workspace_images_reviewed = false;
        input.evidence_review.action_guide_keyframes_reviewed = true;
        input.redaction.result_workspace_images_are_flattened = false;
        input.action_guide = Some(ActionGuideIssueAssets::from_guide(&guide, true));
        (
            input,
            guide,
            store,
            region(),
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
        )
    }

    #[test]
    fn export_folder_includes_action_guide_folder() {
        let (input, guide, store, region, capability, source_kind) = action_input();
        let tmp = tempfile::tempdir().unwrap();
        let action = ActionGuideExportSource {
            guide: &guide,
            store: &store,
            region,
            capability,
            source_kind,
            include_gif: false,
        };

        let result = export_folder_with_action_guide(&input, Some(action), tmp.path()).unwrap();

        assert!(result.directory.join("action-guide/steps.md").exists());
        assert!(result.directory.join("action-guide/session.json").exists());
        assert!(result.directory.join("action-guide/keyframes/001.png").exists());
        let md = std::fs::read_to_string(result.directory.join("issue.md")).unwrap();
        assert!(md.contains("![](action-guide/keyframes/001.png)"), "md = {md}");
        let manifest = std::fs::read_to_string(result.directory.join("manifest.json")).unwrap();
        assert!(manifest.contains("\"action_keyframe\""), "manifest = {manifest}");
    }

    #[test]
    fn action_guide_only_missing_keyframe_rolls_back_temp_output() {
        let (mut input, _guide, store, region, capability, source_kind) = action_input();
        let guide = Guide::from_candidates(vec![CandidateStep {
            id: 1,
            kind: CandidateKind::Click,
            reason: DetectReason::ClickConfirmed,
            at_ms: 0,
            keyframe: 9999,
            nearby: vec![9999],
        }]);
        input.action_guide = Some(ActionGuideIssueAssets::from_guide(&guide, false));
        let tmp = tempfile::tempdir().unwrap();
        let action = ActionGuideExportSource {
            guide: &guide,
            store: &store,
            region,
            capability,
            source_kind,
            include_gif: false,
        };

        let err = export_folder_with_action_guide(&input, Some(action), tmp.path()).unwrap_err();

        assert!(err.to_string().contains("export failed"), "err = {err}");
        assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
    }
}
```

- [ ] **Step 2: Add Action Guide source adapter**

Add this feature-gated code to `issue_pack.rs`:

```rust
#[cfg(feature = "action-guide")]
pub(crate) struct ActionGuideExportSource<'a> {
    pub guide: &'a rollshot_action::Guide,
    pub store: &'a rollshot_action::FrameStore,
    pub region: rollshot_action::CaptureRegion,
    pub capability: rollshot_action::InputCapability,
    pub source_kind: rollshot_action::InputSourceKind,
    pub include_gif: bool,
}

#[cfg(feature = "action-guide")]
impl ActionGuideIssueAssets {
    pub(crate) fn from_guide(guide: &rollshot_action::Guide, include_gif: bool) -> Self {
        let steps = guide
            .steps()
            .iter()
            .enumerate()
            .map(|(i, step)| IssuePackStep {
                index: i + 1,
                title: step.title.clone(),
                keyframe_path: format!("action-guide/keyframes/{:03}.png", i + 1),
            })
            .collect();
        Self { steps, include_gif }
    }
}
```

- [ ] **Step 3: Route Action Guide folder creation through `rollshot_action::export_guide`**

Replace `export_folder` with a call to this helper and add the helper:

```rust
pub(crate) fn export_folder(
    input: &IssuePackInput,
    destination_parent: &Path,
) -> Result<IssuePackExportResult, IssuePackError> {
    export_folder_impl(input, None, destination_parent)
}

#[cfg(feature = "action-guide")]
pub(crate) fn export_folder_with_action_guide(
    input: &IssuePackInput,
    action: Option<ActionGuideExportSource<'_>>,
    destination_parent: &Path,
) -> Result<IssuePackExportResult, IssuePackError> {
    export_folder_impl(input, action, destination_parent)
}

#[cfg(not(feature = "action-guide"))]
type ActionGuideExportSource<'a> = std::marker::PhantomData<&'a ()>;

fn export_folder_impl(
    input: &IssuePackInput,
    #[cfg(feature = "action-guide")] action: Option<ActionGuideExportSource<'_>>,
    #[cfg(not(feature = "action-guide"))] _action: Option<ActionGuideExportSource<'_>>,
    destination_parent: &Path,
) -> Result<IssuePackExportResult, IssuePackError> {
    validate(input)?;
    tracing::info!(
        target: TARGET_ISSUE_PACK_EXPORT,
        mode = "folder",
        has_final_image = input.final_image.is_some(),
        has_action_guide = input.action_guide.is_some(),
        "issue pack export start"
    );
    let folder_name = issue_pack_folder_name(input.created_at);
    let final_dir = destination_parent.join(&folder_name);
    let tmp_dir = destination_parent.join(format!(".{folder_name}.tmp"));

    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir).map_err(|e| IssuePackError::Io(e.to_string()))?;
    }

    let mut warnings = Vec::new();
    let build_result = build_folder(
        input,
        &tmp_dir,
        &mut warnings,
        #[cfg(feature = "action-guide")]
        action,
    )
    .and_then(|()| swap_folder(&tmp_dir, &final_dir));

    if let Err(error) = build_result {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        tracing::error!(
            target: TARGET_ISSUE_PACK_EXPORT,
            mode = "folder",
            error_category = error.category(),
            "issue pack export failed"
        );
        return Err(error);
    }

    tracing::info!(
        target: TARGET_ISSUE_PACK_EXPORT,
        mode = "folder",
        warning_count = warnings.len(),
        "issue pack export complete"
    );
    Ok(IssuePackExportResult {
        markdown_path: final_dir.join("issue.md"),
        manifest_path: final_dir.join("manifest.json"),
        directory: final_dir,
        zip_path: None,
        warnings,
    })
}
```

Update `build_folder` so it accepts mutable warnings and feature-gated action:

```rust
fn build_folder(
    input: &IssuePackInput,
    tmp_dir: &Path,
    warnings: &mut Vec<IssuePackWarning>,
    #[cfg(feature = "action-guide")] action: Option<ActionGuideExportSource<'_>>,
) -> Result<(), IssuePackError> {
    std::fs::create_dir_all(tmp_dir).map_err(|e| IssuePackError::Io(e.to_string()))?;

    if let Some(image) = &input.final_image {
        let images_dir = tmp_dir.join("images");
        std::fs::create_dir_all(&images_dir).map_err(|e| IssuePackError::Io(e.to_string()))?;
        image
            .pixels
            .save_with_format(images_dir.join(&image.file_name), image::ImageFormat::Png)
            .map_err(|e| IssuePackError::Encode(e.to_string()))?;
    }

    #[cfg(feature = "action-guide")]
    if let Some(action) = action {
        rollshot_action::export_guide(
            action.guide,
            action.store,
            action.region,
            action.capability,
            action.source_kind,
            tmp_dir,
        )
        .map_err(|e| IssuePackError::Io(format!("export failed: {e}")))?;

        if action.include_gif {
            let gif_path = tmp_dir.join("action-guide/guide.gif");
            if let Err(error) = rollshot_action::export_gif(
                action.guide,
                action.store,
                rollshot_action::GifOptions::default(),
                &gif_path,
            ) {
                warnings.push(IssuePackWarning {
                    code: "gif_export_failed".to_string(),
                    message: format!("GIF export failed: {error}"),
                });
            }
        }
    }

    std::fs::write(tmp_dir.join("issue.md"), render_issue_markdown(input))
        .map_err(|e| IssuePackError::Io(e.to_string()))?;
    let include_gif = tmp_dir.join("action-guide/guide.gif").exists();
    let manifest = render_manifest_json(input, warnings, include_gif)?;
    std::fs::write(tmp_dir.join("manifest.json"), manifest)
        .map_err(|e| IssuePackError::Io(e.to_string()))?;
    Ok(())
}
```

- [ ] **Step 4: Run feature-gated tests**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide issue_pack -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/issue_pack.rs
rtk git commit -m "feat(app): include action guide assets in issue packs"
```

---

### Task 4: Add ZIP Packaging

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/rollshot-app/Cargo.toml`
- Modify: `crates/rollshot-app/src/issue_pack.rs`
- Test: `crates/rollshot-app/src/issue_pack.rs`

- [ ] **Step 1: Add the dependency**

In workspace `Cargo.toml`, add:

```toml
zip = { version = "8.6", default-features = false, features = ["deflate"] }
```

In `crates/rollshot-app/Cargo.toml`, add:

```toml
zip = { workspace = true }
```

- [ ] **Step 2: Add failing ZIP test**

Append to the `issue_pack.rs` tests:

```rust
#[test]
fn export_zip_contains_same_relative_layout_as_folder() {
    let input = base_input();
    let tmp = tempfile::tempdir().unwrap();
    let result = export_zip(&input, tmp.path()).unwrap();
    let zip_path = result.zip_path.clone().expect("zip path");
    assert!(zip_path.exists());

    let file = std::fs::File::open(zip_path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut names = Vec::new();
    for i in 0..archive.len() {
        names.push(archive.by_index(i).unwrap().name().to_string());
    }
    names.sort();

    assert!(names.contains(&"images/final-redacted.png".to_string()), "names = {names:?}");
    assert!(names.contains(&"issue.md".to_string()), "names = {names:?}");
    assert!(names.contains(&"manifest.json".to_string()), "names = {names:?}");
}

#[test]
fn export_zip_replaces_stale_zip_atomically() {
    let input = base_input();
    let tmp = tempfile::tempdir().unwrap();
    let first = export_zip(&input, tmp.path()).unwrap();
    let zip_path = first.zip_path.clone().expect("zip path");
    std::fs::write(&zip_path, b"stale").unwrap();

    let second = export_zip(&input, tmp.path()).unwrap();

    assert_eq!(second.zip_path.as_ref(), Some(&zip_path));
    let file = std::fs::File::open(zip_path).unwrap();
    let archive = zip::ZipArchive::new(file).unwrap();
    assert!(archive.len() >= 3);
}
```

- [ ] **Step 3: Implement ZIP export**

Add these functions to `issue_pack.rs`:

```rust
pub(crate) fn export_zip(
    input: &IssuePackInput,
    destination_parent: &Path,
) -> Result<IssuePackExportResult, IssuePackError> {
    let mut result = export_folder(input, destination_parent)?;
    let zip_path = result.directory.with_extension("zip");
    zip_directory(&result.directory, &zip_path)?;
    result.zip_path = Some(zip_path);
    Ok(result)
}

#[cfg(feature = "action-guide")]
pub(crate) fn export_zip_with_action_guide(
    input: &IssuePackInput,
    action: Option<ActionGuideExportSource<'_>>,
    destination_parent: &Path,
) -> Result<IssuePackExportResult, IssuePackError> {
    let mut result = export_folder_with_action_guide(input, action, destination_parent)?;
    let zip_path = result.directory.with_extension("zip");
    zip_directory(&result.directory, &zip_path)?;
    result.zip_path = Some(zip_path);
    Ok(result)
}

fn zip_directory(source_dir: &Path, zip_path: &Path) -> Result<(), IssuePackError> {
    tracing::info!(
        target: TARGET_ISSUE_PACK_EXPORT,
        mode = "zip",
        "issue pack zip start"
    );
    let result = zip_directory_inner(source_dir, zip_path);
    match &result {
        Ok(()) => tracing::info!(
            target: TARGET_ISSUE_PACK_EXPORT,
            mode = "zip",
            "issue pack zip complete"
        ),
        Err(error) => tracing::error!(
            target: TARGET_ISSUE_PACK_EXPORT,
            mode = "zip",
            error_category = error.category(),
            "issue pack zip failed"
        ),
    }
    result
}

fn zip_directory_inner(source_dir: &Path, zip_path: &Path) -> Result<(), IssuePackError> {
    let tmp_zip = zip_path.with_extension("zip.tmp");
    if tmp_zip.exists() {
        std::fs::remove_file(&tmp_zip).map_err(|e| IssuePackError::Io(e.to_string()))?;
    }
    let file = std::fs::File::create(&tmp_zip).map_err(|e| IssuePackError::Io(e.to_string()))?;
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    add_dir_to_zip(&mut writer, source_dir, source_dir, options)?;
    writer.finish().map_err(|e| IssuePackError::Io(e.to_string()))?;
    if zip_path.exists() {
        std::fs::remove_file(zip_path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp_zip);
            IssuePackError::Io(e.to_string())
        })?;
    }
    std::fs::rename(&tmp_zip, zip_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_zip);
        IssuePackError::Io(e.to_string())
    })?;
    Ok(())
}

fn add_dir_to_zip(
    writer: &mut zip::ZipWriter<std::fs::File>,
    root: &Path,
    dir: &Path,
    options: zip::write::SimpleFileOptions,
) -> Result<(), IssuePackError> {
    let mut entries = std::fs::read_dir(dir)
        .map_err(|e| IssuePackError::Io(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| IssuePackError::Io(e.to_string()))?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            add_dir_to_zip(writer, root, &path, options)?;
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|e| IssuePackError::Io(e.to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        writer
            .start_file(rel, options)
            .map_err(|e| IssuePackError::Io(e.to_string()))?;
        let mut file = std::fs::File::open(&path).map_err(|e| IssuePackError::Io(e.to_string()))?;
        std::io::copy(&mut file, writer).map_err(|e| IssuePackError::Io(e.to_string()))?;
    }
    Ok(())
}
```

- [ ] **Step 4: Run ZIP tests**

Run:

```bash
rtk cargo test -p rollshot-app issue_pack::tests::export_zip_contains_same_relative_layout_as_folder -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add Cargo.toml Cargo.lock crates/rollshot-app/Cargo.toml crates/rollshot-app/src/issue_pack.rs
rtk git commit -m "feat(app): package issue packs as zip"
```

---

### Task 5: Wire Result Workspace Export Review UI

**Files:**
- Modify: `crates/rollshot-app/src/result_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/result_workspace/update.rs`
- Modify: `crates/rollshot-app/src/result_workspace/view.rs`
- Modify: `crates/rollshot-app/src/result_workspace/workbench/state.rs`
- Test: `crates/rollshot-app/src/result_workspace/update.rs`
- Test: `crates/rollshot-app/src/result_workspace/view.rs`

- [ ] **Step 1: Add failing update tests**

In `crates/rollshot-app/src/result_workspace/update.rs`, add tests near the existing update tests:

```rust
#[test]
fn issue_pack_request_blocks_pending_smart_redaction_candidates() {
    let mut state = workspace();
    state.mode = super::workbench::WorkspaceMode::Workbench(
        super::workbench::state::workbench_with_pending_candidate(),
    );

    let _ = update(&mut state, Message::ExportBugReport);

    assert!(state.issue_pack.is_none());
    assert!(state.message.as_ref().unwrap().text().contains("Apply"));
}

#[test]
fn issue_pack_export_requires_review_confirmation() {
    let mut state = workspace();
    let _ = update(&mut state, Message::ExportBugReport);
    assert!(state.issue_pack.as_ref().is_some_and(|dialog| !dialog.review_confirmed));

    let tmp = tempfile::tempdir().unwrap();
    let _ = update(&mut state, Message::IssuePackFolderChosen(Some(tmp.path().to_path_buf())));

    assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
    assert!(state.message.as_ref().unwrap().text().contains("review"));
}

#[test]
fn issue_pack_folder_export_writes_flattened_result_image() {
    let mut state = workspace();
    state.document.image.add_redaction(ImageRect {
        x: 0.0,
        y: 0.0,
        width: 2.0,
        height: 2.0,
    }).unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let _ = update(&mut state, Message::ExportBugReport);
    let _ = update(&mut state, Message::IssuePackReviewChanged(true));
    let _ = update(&mut state, Message::IssuePackFolderChosen(Some(tmp.path().to_path_buf())));

    let final_image = tmp.path()
        .join("rollshot-issue-pack-")
        .parent()
        .unwrap()
        .read_dir()
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.file_name().unwrap().to_string_lossy().starts_with("rollshot-issue-pack-"))
        .unwrap()
        .join("images/final-redacted.png");
    let decoded = image::open(final_image).unwrap().to_rgba8();
    assert_eq!(decoded.get_pixel(0, 0).0, [0, 0, 0, 255]);
}
```

Add this test-only helper to `result_workspace/workbench/state.rs` outside the private `tests` modules so `result_workspace/update.rs` tests can use it:

```rust
#[cfg(test)]
pub(crate) fn workbench_with_pending_candidate() -> super::WorkbenchState {
    use rollshot_edit_proposal::{
        ConfidenceSummary, ProposalId, ProposedCandidate, Provenance, ProvenanceSource,
    };

    let id = CandidateId(1);
    let proposal = EditProposal {
        id: ProposalId(1),
        base_document_state_id: 0,
        candidates: vec![ProposedCandidate {
            id,
            edit: ProposedEdit::AddRedaction {
                bounds: ImageRect {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
            },
            confidence: 0.9,
            label: "pending redaction".into(),
            rationale: None,
            provenance: Provenance {
                source: ProvenanceSource::Manual,
            },
        }],
        confidence_summary: ConfidenceSummary::from_confidences(&[0.9]),
        rationale_summary: None,
        provenance: Provenance {
            source: ProvenanceSource::Manual,
        },
    };

    let mut wb = super::WorkbenchState::default();
    wb.pending_proposal = Some(proposal);
    wb.review = CandidateReview::from_candidates(&[id]);
    wb
}
```

- [ ] **Step 2: Add dialog state to `ResultWorkspace`**

In `result_workspace/mod.rs`, add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IssuePackKind {
    Folder,
    Zip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IssuePackDialog {
    pub review_confirmed: bool,
    pub pending_kind: Option<IssuePackKind>,
}

impl IssuePackDialog {
    pub(crate) fn new() -> Self {
        Self {
            review_confirmed: false,
            pending_kind: None,
        }
    }
}
```

Add this field to `ResultWorkspace`:

```rust
pub issue_pack: Option<IssuePackDialog>,
```

Initialize it in `with_max_texture_dim`:

```rust
issue_pack: None,
```

- [ ] **Step 3: Add messages and preparation helpers**

In `result_workspace/update.rs`, add variants:

```rust
ExportBugReport,
IssuePackReviewChanged(bool),
IssuePackReviewRedactions,
IssuePackExportFolder,
IssuePackExportZip,
IssuePackFolderChosen(Option<PathBuf>),
IssuePackFinished(Result<crate::issue_pack::IssuePackExportResult, String>),
IssuePackCancel,
```

Update `PartialEq` for these variants.

Add helpers:

```rust
fn block_pending_candidates(state: &mut super::ResultWorkspace) -> bool {
    if let super::workbench::WorkspaceMode::Workbench(ref wb) = state.mode {
        if super::workbench::state::has_pending_candidates(wb) {
            state.message = Some(InlineMessage::Error(format!(
                "{}\nApply them before safe export.",
                super::workbench::state::apply_skip_summary(wb)
            )));
            return true;
        }
    }
    false
}

fn result_issue_pack_input(state: &super::ResultWorkspace) -> crate::issue_pack::IssuePackInput {
    let redaction_count = state
        .document
        .image
        .annotations()
        .iter()
        .filter(|annotation| matches!(annotation, Annotation::OpaqueRedaction { .. }))
        .count();
    crate::issue_pack::IssuePackInput {
        title: None,
        created_at: chrono::Local::now(),
        rollshot_version: env!("CARGO_PKG_VERSION").to_string(),
        platform: crate::issue_pack::PlatformInfo::current(),
        final_image: Some(crate::issue_pack::SafeImageAsset {
            file_name: "final-redacted.png".to_string(),
            pixels: state.document.image.flatten(),
            derived_from_original: true,
        }),
        action_guide: None,
        ocr_snippets: result_ocr_snippets(state),
        evidence_review: crate::issue_pack::EvidenceReviewSummary {
            required: true,
            completed: state.issue_pack.as_ref().is_some_and(|dialog| dialog.review_confirmed),
            result_workspace_images_reviewed: state.issue_pack.as_ref().is_some_and(|dialog| dialog.review_confirmed),
            action_guide_keyframes_reviewed: false,
        },
        redaction: crate::issue_pack::RedactionSummary {
            review_required: true,
            review_completed: state.issue_pack.as_ref().is_some_and(|dialog| dialog.review_confirmed),
            result_workspace_images_are_flattened: true,
            original_pixels_included: false,
            redaction_count,
        },
    }
}

#[cfg(feature = "ocr")]
fn result_ocr_snippets(state: &super::ResultWorkspace) -> Vec<crate::issue_pack::OcrSnippet> {
    state
        .ocr_text
        .document()
        .map(|document| {
            document
                .visible_items()
                .iter()
                .take(12)
                .map(|item| crate::issue_pack::OcrSnippet {
                    text: item.text.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(not(feature = "ocr"))]
fn result_ocr_snippets(_state: &super::ResultWorkspace) -> Vec<crate::issue_pack::OcrSnippet> {
    Vec::new()
}
```

- [ ] **Step 4: Handle Result Workspace Issue Pack messages**

Add match arms in `update_inner`:

```rust
Message::ExportBugReport => {
    if block_pending_candidates(state) {
        return Task::none();
    }
    commit_text_draft(state);
    state.issue_pack = Some(super::IssuePackDialog::new());
    state.message = None;
    Task::none()
}
Message::IssuePackReviewChanged(confirmed) => {
    if let Some(dialog) = &mut state.issue_pack {
        dialog.review_confirmed = confirmed;
    }
    Task::none()
}
Message::IssuePackReviewRedactions => {
    state.issue_pack = None;
    state.editor.tool = Tool::Redact;
    Task::none()
}
Message::IssuePackExportFolder => begin_issue_pack_export(state, super::IssuePackKind::Folder),
Message::IssuePackExportZip => begin_issue_pack_export(state, super::IssuePackKind::Zip),
Message::IssuePackFolderChosen(None) => {
    if let Some(dialog) = &mut state.issue_pack {
        dialog.pending_kind = None;
    }
    Task::none()
}
Message::IssuePackFolderChosen(Some(parent)) => {
    let kind = state
        .issue_pack
        .as_ref()
        .and_then(|dialog| dialog.pending_kind)
        .unwrap_or(super::IssuePackKind::Folder);
    let input = result_issue_pack_input(state);
    let result = match kind {
        super::IssuePackKind::Folder => crate::issue_pack::export_folder(&input, &parent),
        super::IssuePackKind::Zip => crate::issue_pack::export_zip(&input, &parent),
    };
    update_inner(state, Message::IssuePackFinished(result.map_err(|e| e.to_string())))
}
Message::IssuePackFinished(Ok(result)) => {
    let mut text = match result.zip_path.as_ref() {
        Some(path) => format!("Exported bug report ZIP to {}", path.display()),
        None => format!("Exported bug report to {}", result.directory.display()),
    };
    if !result.warnings.is_empty() {
        let warning_text = result
            .warnings
            .iter()
            .map(|warning| warning.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        text = format!("{text}\nWarnings: {warning_text}");
    }
    state.issue_pack = None;
    state.message = Some(InlineMessage::success(text));
    Task::none()
}
Message::IssuePackFinished(Err(error)) => {
    if let Some(dialog) = &mut state.issue_pack {
        dialog.pending_kind = None;
    }
    state.message = Some(InlineMessage::Error(error));
    Task::none()
}
Message::IssuePackCancel => {
    state.issue_pack = None;
    Task::none()
}
```

Add this helper near `result_issue_pack_input`:

```rust
fn begin_issue_pack_export(
    state: &mut super::ResultWorkspace,
    kind: super::IssuePackKind,
) -> Task<Message> {
    let Some(dialog) = &mut state.issue_pack else {
        return Task::none();
    };
    if !dialog.review_confirmed {
        state.message = Some(InlineMessage::Error(
            "Review the images included in this bug report before export.".to_string(),
        ));
        return Task::none();
    }
    dialog.pending_kind = Some(kind);
    let default_dir = crate::storage::Platform::current()
        .and_then(crate::storage::default_output_dir)
        .unwrap_or_else(|_| PathBuf::from("."));
    Task::perform(
        async move {
            rfd::AsyncFileDialog::new()
                .set_directory(default_dir)
                .pick_folder()
                .await
                .map(|h| h.path().to_path_buf())
        },
        Message::IssuePackFolderChosen,
    )
}
```

- [ ] **Step 5: Add toolbar button and modal view**

In `result_workspace/view.rs`, add the toolbar button near copy/save/reveal:

```rust
.push(button(text("Export Bug Report...")).on_press(Message::ExportBugReport))
```

Add modal wrapping in `view()` before the discard/unredacted modal decision:

```rust
let body = if state.issue_pack.is_some() {
    issue_pack_modal(body, state)
} else {
    body
};
```

Add this helper:

```rust
fn issue_pack_modal(base: Element<'_, Message>, state: &ResultWorkspace) -> Element<'_, Message> {
    let dialog = state.issue_pack.as_ref().expect("checked by caller");
    let redactions = state
        .document
        .image
        .annotations()
        .iter()
        .filter(|annotation| matches!(annotation, rollshot_image_document::Annotation::OpaqueRedaction { .. }))
        .count();
    let safety = if redactions > 0 {
        column![
            text("Result Workspace images will be flattened."),
            text("Retained originals will not be included."),
            text("Review redactions before export."),
        ]
    } else {
        column![text("No redactions are currently applied. Review the image before sharing.")]
    };
    let export_enabled = dialog.review_confirmed && dialog.pending_kind.is_none();
    let folder = button(text("Export Folder"))
        .on_press_maybe(export_enabled.then_some(Message::IssuePackExportFolder))
        .style(button::primary);
    let zip = button(text("Export ZIP"))
        .on_press_maybe(export_enabled.then_some(Message::IssuePackExportZip))
        .style(button::secondary);

    let dialog = container(
        column![
            text("Issue Pack Export").size(18),
            text("Included: issue.md, manifest.json, final flattened screenshot"),
            text("Safety:"),
            safety,
            iced::widget::checkbox(
                "I reviewed the images included in this bug report.",
                dialog.review_confirmed,
            )
            .on_toggle(Message::IssuePackReviewChanged),
            row![
                button(text("Review Redactions")).on_press(Message::IssuePackReviewRedactions),
                folder,
                zip,
                button(text("Cancel")).on_press(Message::IssuePackCancel),
            ]
            .spacing(8),
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fixed(460.0))
    .style(container::rounded_box);

    let scrim = opaque(
        mouse_area(
            container(opaque(dialog))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .style(|_theme| container::Style {
                    background: Some(Color { a: 0.8, ..Color::BLACK }.into()),
                    ..container::Style::default()
                }),
        )
        .interaction(mouse::Interaction::Idle)
        .on_press(Message::IssuePackCancel),
    );
    iced::widget::stack![base, scrim].into()
}
```

- [ ] **Step 6: Run Result Workspace checks**

Run:

```bash
rtk cargo test -p rollshot-app result_workspace -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/result_workspace/mod.rs crates/rollshot-app/src/result_workspace/update.rs crates/rollshot-app/src/result_workspace/view.rs crates/rollshot-app/src/result_workspace/workbench/state.rs
rtk git commit -m "feat(app): export issue packs from result workspace"
```

---

### Task 6: Wire Action Guide Review Export UI

**Files:**
- Modify: `crates/rollshot-app/src/timeline_workspace/mod.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/update.rs`
- Test: `crates/rollshot-app/src/timeline_workspace/view.rs`

- [ ] **Step 1: Add failing Timeline Workspace tests**

In `timeline_workspace/update.rs`, add:

```rust
#[test]
fn issue_pack_export_requires_keyframe_review_confirmation() {
    let mut state = ws(recording_from_frames());
    let tmp = tempfile::tempdir().unwrap();

    let _ = update(&mut state, Message::ExportBugReport);
    let _ = update(&mut state, Message::IssuePackFolderChosen(Some(tmp.path().to_path_buf())));

    assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
    assert!(state.message.as_ref().unwrap().contains("review"));
}

#[test]
fn issue_pack_folder_export_uses_reviewed_titles_and_keyframes() {
    let mut state = ws(recording_from_frames());
    let tmp = tempfile::tempdir().unwrap();

    let _ = update(&mut state, Message::TitleChanged("Open Settings".to_string()));
    let _ = update(&mut state, Message::ExportBugReport);
    let _ = update(&mut state, Message::IssuePackReviewChanged(true));
    let _ = update(&mut state, Message::IssuePackFolderChosen(Some(tmp.path().to_path_buf())));

    let pack = std::fs::read_dir(tmp.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.file_name().unwrap().to_string_lossy().starts_with("rollshot-issue-pack-"))
        .unwrap();
    let md = std::fs::read_to_string(pack.join("issue.md")).unwrap();
    assert!(md.contains("Open Settings"), "md = {md}");
    assert!(pack.join("action-guide/steps.md").exists());
    assert!(pack.join("action-guide/session.json").exists());
}

#[test]
fn issue_pack_cancel_writes_nothing() {
    let mut state = ws(recording_from_frames());
    let _ = update(&mut state, Message::ExportBugReport);
    let _ = update(&mut state, Message::IssuePackCancel);

    assert!(state.issue_pack.is_none());
}
```

- [ ] **Step 2: Add Timeline dialog state**

In `timeline_workspace/mod.rs`, add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IssuePackKind {
    Folder,
    Zip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IssuePackDialog {
    pub review_confirmed: bool,
    pub pending_kind: Option<IssuePackKind>,
    pub include_gif: bool,
}

impl IssuePackDialog {
    pub(crate) fn new() -> Self {
        Self {
            review_confirmed: false,
            pending_kind: None,
            include_gif: true,
        }
    }
}
```

Add to `TimelineWorkspace`:

```rust
pub(crate) issue_pack: Option<IssuePackDialog>,
```

Initialize in `TimelineWorkspace::new`:

```rust
issue_pack: None,
```

- [ ] **Step 3: Add Timeline messages and input builder**

In `timeline_workspace/update.rs`, add message variants:

```rust
ExportBugReport,
IssuePackReviewChanged(bool),
IssuePackIncludeGifChanged(bool),
IssuePackExportFolder,
IssuePackExportZip,
IssuePackFolderChosen(Option<PathBuf>),
IssuePackFinished(Result<crate::issue_pack::IssuePackExportResult, String>),
IssuePackCancel,
```

Add helpers:

```rust
fn timeline_issue_pack_input(state: &TimelineWorkspace) -> crate::issue_pack::IssuePackInput {
    let include_gif = state.issue_pack.as_ref().is_some_and(|dialog| dialog.include_gif);
    let reviewed = state.issue_pack.as_ref().is_some_and(|dialog| dialog.review_confirmed);
    crate::issue_pack::IssuePackInput {
        title: None,
        created_at: chrono::Local::now(),
        rollshot_version: env!("CARGO_PKG_VERSION").to_string(),
        platform: crate::issue_pack::PlatformInfo::current(),
        final_image: None,
        action_guide: Some(crate::issue_pack::ActionGuideIssueAssets::from_guide(
            &state.guide,
            include_gif,
        )),
        ocr_snippets: Vec::new(),
        evidence_review: crate::issue_pack::EvidenceReviewSummary {
            required: true,
            completed: reviewed,
            result_workspace_images_reviewed: false,
            action_guide_keyframes_reviewed: reviewed,
        },
        redaction: crate::issue_pack::RedactionSummary {
            review_required: false,
            review_completed: reviewed,
            result_workspace_images_are_flattened: false,
            original_pixels_included: false,
            redaction_count: 0,
        },
    }
}

fn timeline_issue_pack_action(
    state: &TimelineWorkspace,
) -> crate::issue_pack::ActionGuideExportSource<'_> {
    let include_gif = state.issue_pack.as_ref().is_some_and(|dialog| dialog.include_gif);
    crate::issue_pack::ActionGuideExportSource {
        guide: &state.guide,
        store: &state.store,
        region: state.region,
        capability: state.capability,
        source_kind: state.source_kind,
        include_gif,
    }
}
```

- [ ] **Step 4: Handle Timeline Issue Pack messages**

Add match arms:

```rust
Message::ExportBugReport => {
    state.message = None;
    state.issue_pack = Some(super::IssuePackDialog::new());
    Task::none()
}
Message::IssuePackReviewChanged(confirmed) => {
    if let Some(dialog) = &mut state.issue_pack {
        dialog.review_confirmed = confirmed;
    }
    Task::none()
}
Message::IssuePackIncludeGifChanged(include) => {
    if let Some(dialog) = &mut state.issue_pack {
        dialog.include_gif = include;
    }
    Task::none()
}
Message::IssuePackExportFolder => begin_issue_pack_export(state, super::IssuePackKind::Folder),
Message::IssuePackExportZip => begin_issue_pack_export(state, super::IssuePackKind::Zip),
Message::IssuePackFolderChosen(None) => {
    if let Some(dialog) = &mut state.issue_pack {
        dialog.pending_kind = None;
    }
    Task::none()
}
Message::IssuePackFolderChosen(Some(parent)) => {
    let kind = state
        .issue_pack
        .as_ref()
        .and_then(|dialog| dialog.pending_kind)
        .unwrap_or(super::IssuePackKind::Folder);
    let input = timeline_issue_pack_input(state);
    let action = timeline_issue_pack_action(state);
    let result = match kind {
        super::IssuePackKind::Folder => {
            crate::issue_pack::export_folder_with_action_guide(&input, Some(action), &parent)
        }
        super::IssuePackKind::Zip => {
            crate::issue_pack::export_zip_with_action_guide(&input, Some(action), &parent)
        }
    };
    update(state, Message::IssuePackFinished(result.map_err(|e| e.to_string())))
}
Message::IssuePackFinished(Ok(result)) => {
    let mut text = match result.zip_path.as_ref() {
        Some(path) => format!("Bug report ZIP saved to {}", path.display()),
        None => format!("Bug report saved to {}", result.directory.display()),
    };
    if !result.warnings.is_empty() {
        let warning_text = result
            .warnings
            .iter()
            .map(|warning| warning.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        text = format!("{text}\nWarnings: {warning_text}");
    }
    state.issue_pack = None;
    state.message = Some(text);
    Task::none()
}
Message::IssuePackFinished(Err(error)) => {
    if let Some(dialog) = &mut state.issue_pack {
        dialog.pending_kind = None;
    }
    state.message = Some(error);
    Task::none()
}
Message::IssuePackCancel => {
    state.issue_pack = None;
    Task::none()
}
```

Add this helper near `timeline_issue_pack_action`:

```rust
fn begin_issue_pack_export(
    state: &mut TimelineWorkspace,
    kind: super::IssuePackKind,
) -> Task<Message> {
    let Some(dialog) = &mut state.issue_pack else {
        return Task::none();
    };
    if !dialog.review_confirmed {
        state.message = Some("Review every keyframe before sharing.".to_string());
        return Task::none();
    }
    dialog.pending_kind = Some(kind);
    Task::perform(
        pick_export_dir(picker_default_dir()),
        Message::IssuePackFolderChosen,
    )
}
```

- [ ] **Step 5: Add Timeline toolbar button and modal**

In `timeline_workspace/view.rs`, add the toolbar button beside `Export Guide` and `Export GIF`:

```rust
button(text("Export Bug Report..."))
    .on_press(Message::ExportBugReport)
    .style(button::secondary),
```

Wrap `view()` with the Issue Pack modal before discard modal:

```rust
let body = if state.issue_pack.is_some() {
    issue_pack_modal(body, state)
} else {
    body
};
```

Add:

```rust
fn issue_pack_modal(base: Element<'_, Message>, state: &TimelineWorkspace) -> Element<'_, Message> {
    let dialog = state.issue_pack.as_ref().expect("checked by caller");
    let export_enabled = dialog.review_confirmed && dialog.pending_kind.is_none();
    let steps = state.guide.steps().len();

    let dialog_view = container(
        column![
            text("Issue Pack Export").size(18),
            text(format!("Included: issue.md, manifest.json, {steps} Action Guide steps")),
            text("Safety:"),
            column![
                text("Action Guide keyframes are reviewed evidence images."),
                text("Keyframes are not automatically redacted."),
                text("Review every keyframe before sharing."),
            ],
            checkbox("Include guide.gif when GIF export succeeds", dialog.include_gif)
                .on_toggle(Message::IssuePackIncludeGifChanged),
            checkbox(
                "I reviewed the images and keyframes included in this bug report.",
                dialog.review_confirmed,
            )
            .on_toggle(Message::IssuePackReviewChanged),
            row![
                button(text("Export Folder"))
                    .on_press_maybe(export_enabled.then_some(Message::IssuePackExportFolder))
                    .style(button::primary),
                button(text("Export ZIP"))
                    .on_press_maybe(export_enabled.then_some(Message::IssuePackExportZip))
                    .style(button::secondary),
                button(text("Cancel")).on_press(Message::IssuePackCancel),
            ]
            .spacing(8),
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fixed(500.0))
    .style(container::rounded_box);

    let scrim = opaque(
        mouse_area(
            container(opaque(dialog_view))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .style(|_theme: &Theme| container::Style {
                    background: Some(Color { a: 0.8, ..Color::BLACK }.into()),
                    ..container::Style::default()
                }),
        )
        .interaction(mouse::Interaction::Idle)
        .on_press(Message::IssuePackCancel),
    );
    stack![base, scrim].into()
}
```

Add `checkbox` to the existing iced widget imports.

- [ ] **Step 6: Run Action Guide feature tests**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide timeline_workspace -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
rtk git add crates/rollshot-app/src/timeline_workspace/mod.rs crates/rollshot-app/src/timeline_workspace/update.rs crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "feat(app): export issue packs from action guides"
```

---

### Task 7: Tighten Manifest Accuracy, Warnings, And Safety Copy

**Files:**
- Modify: `crates/rollshot-app/src/issue_pack.rs`
- Modify: `crates/rollshot-app/src/result_workspace/view.rs`
- Modify: `crates/rollshot-app/src/timeline_workspace/view.rs`
- Test: `crates/rollshot-app/src/issue_pack.rs`

- [ ] **Step 1: Add warning and copy tests**

Add tests:

```rust
#[test]
fn manifest_never_claims_sensitive_content_was_detected() {
    let input = base_input();
    let tmp = tempfile::tempdir().unwrap();
    let result = export_folder(&input, tmp.path()).unwrap();
    let all_text = format!(
        "{}\n{}",
        std::fs::read_to_string(result.directory.join("issue.md")).unwrap(),
        std::fs::read_to_string(result.directory.join("manifest.json")).unwrap()
    );

    assert!(!all_text.to_lowercase().contains("sensitive-free"));
    assert!(!all_text.to_lowercase().contains("all sensitive"));
    assert!(!all_text.to_lowercase().contains("detected every"));
}
```

Under the Action Guide feature test module, add:

```rust
#[test]
fn action_keyframes_are_listed_as_reviewed_evidence_not_redacted_assets() {
    let (input, guide, store, region, capability, source_kind) = action_input();
    let tmp = tempfile::tempdir().unwrap();
    let action = ActionGuideExportSource {
        guide: &guide,
        store: &store,
        region,
        capability,
        source_kind,
        include_gif: false,
    };

    let result = export_folder_with_action_guide(&input, Some(action), tmp.path()).unwrap();
    let manifest = std::fs::read_to_string(result.directory.join("manifest.json")).unwrap();

    assert!(manifest.contains("\"action_keyframe\""), "manifest = {manifest}");
    assert!(!manifest.contains("redacted_keyframe"), "manifest = {manifest}");
}
```

- [ ] **Step 2: Ensure warning serialization records optional GIF failure**

If `rollshot_action::export_gif` fails in `build_folder`, keep the folder export successful and ensure `warnings` is passed to `render_manifest_json`. The warning must look like:

```json
{
  "code": "gif_export_failed",
  "message": "GIF export failed: ..."
}
```

- [ ] **Step 3: Review UI copy**

Verify these exact strings exist:

```text
No redactions are currently applied. Review the image before sharing.
Action Guide keyframes are reviewed evidence images.
Keyframes are not automatically redacted.
Review every keyframe before sharing.
```

Do not add text claiming that Rollshot found all sensitive regions.

- [ ] **Step 4: Run safety-focused tests**

Run:

```bash
rtk cargo test -p rollshot-app issue_pack -- --nocapture
rtk cargo test -p rollshot-app --features action-guide issue_pack -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/rollshot-app/src/issue_pack.rs crates/rollshot-app/src/result_workspace/view.rs crates/rollshot-app/src/timeline_workspace/view.rs
rtk git commit -m "fix(app): preserve issue pack safety semantics"
```

---

### Task 8: Final Verification And Platform Path Check

**Files:**
- Verify only; no planned edits.

- [ ] **Step 1: Run app tests without features**

Run:

```bash
rtk cargo test -p rollshot-app
```

Expected: PASS.

- [ ] **Step 2: Run app tests with Action Guide**

Run:

```bash
rtk cargo test -p rollshot-app --features action-guide
```

Expected: PASS.

- [ ] **Step 3: Run action crate tests**

Run:

```bash
rtk cargo test -p rollshot-action
```

Expected: PASS.

- [ ] **Step 4: Run formatting**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS.

- [ ] **Step 5: Run clippy if UI/update edits are substantial**

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 6: Manually check both UI paths**

Linux path:

```bash
rtk cargo run -p rollshot-app -- capture --backend auto
rtk cargo run -p rollshot-app --features action-guide -- action-guide --fullscreen
```

Expected:

- Result Workspace toolbar shows `Export Bug Report...`.
- Export buttons stay disabled until the review checkbox is checked.
- Folder export writes `issue.md`, `manifest.json`, and `images/final-redacted.png`.
- ZIP export writes the folder and a sibling `.zip`.
- Action Guide Review toolbar shows `Export Bug Report...` beside `Export Guide` and `Export GIF`.
- Action Guide-only modal says keyframes are reviewed evidence and are not automatically redacted.

macOS path, on a macOS machine:

```bash
rtk cargo test -p rollshot-app --features action-guide
rtk cargo run -p rollshot-app --features action-guide -- capture --backend auto
rtk cargo run -p rollshot-app --features action-guide -- action-guide --fullscreen
```

Expected:

- `macos_product.rs` still forwards `Message::Workspace` and `Message::Timeline` without additional changes.
- Result Workspace and Timeline Workspace Issue Pack UI appears inside the macOS product daemon windows.
- No second iced event loop is introduced.

- [ ] **Step 7: Commit verification-only fixups if needed**

If verification required code changes:

```bash
rtk git add <changed-files>
rtk git commit -m "fix(app): finalize issue pack export"
```

If no code changes were required, do not create an empty commit.

---

## Engineering Review Lock-In

### Step 0 Scope Challenge

- Goal alignment: all tasks contribute to GUI-only Local Issue Pack export. No task is pure ornament; ZIP is retained because the approved design calls it a secondary action after folder export succeeds.
- Minimum viable plan: Tasks 1-6 plus Task 8 ship the feature. Task 7 is retained as a hardening task, not deferred, because safety wording and manifest accuracy are core acceptance criteria.
- Complexity check: 1 net-new Rust module, 0 new crates, 8 tasks. The complexity threshold does not trigger.
- Distribution check: no new binary, library crate, container, or external distribution artifact is introduced. The user-visible artifact is generated by the existing GUI.
- Search check: Rust has no standard-library ZIP writer; the plan uses the established `zip` crate. `zip` docs list `ZipWriter::new()` for archive writing and document `deflate` support, so the dependency is a boring choice. `rfd` docs confirm `AsyncFileDialog` supports native async file dialogs and `set_directory`, matching the existing app pattern.

### Auto Decisions Applied

Auto decision D1 — Keep one app-level issue_pack module
Context: The approved spec asks for `crates/rollshot-app/src/issue_pack.rs`, and the required state already lives in app modules.
ELI10: A separate crate sounds cleaner, but it would force more public APIs before the feature proves it needs reuse. Keeping it in the app keeps the first release smaller and easier to change.
Stakes if we pick wrong: A premature crate boundary would slow every follow-up by making internal app state into public contracts.
Recommendation: 1A because it matches the approved architecture and keeps dependency direction simple.
Note: options differ in kind, not coverage — no completeness score.
Pros / cons:
A) Keep app module (recommended): low effort, low risk, low maintenance.
B) Create reusable crate now: higher effort, medium risk, higher maintenance.
Net: Reuse later is possible; premature reuse now is needless surface area.

Auto decision D2 — Retain ZIP in first release but pin the current major
Context: ZIP packaging is secondary but explicitly in first-release scope.
ELI10: Users can already use the folder, but ZIP makes sharing through chat, email, and trackers much easier. The cost is one dependency and one packaging function.
Stakes if we pick wrong: Deferring ZIP would make the feature feel unfinished for common sharing workflows.
Recommendation: 2A because `zip` is the purpose-built crate and the plan now pins `8.6` with narrow `deflate` features.
Completeness: A=9/10, B=7/10
Pros / cons:
A) Keep ZIP now (recommended): small AI-assisted effort, moderate dependency risk, low maintenance.
B) Defer ZIP: lower dependency risk, but misses approved sharing behavior.
Net: ZIP is cheap enough to include and important enough not to defer.

Auto decision D3 — Repair Task 1 red-green ordering
Context: Task 1 originally pasted working renderer code in the same step labeled “failing tests.”
ELI10: A test-first plan should fail before it passes. Otherwise the executor cannot tell whether the test actually caught the intended behavior.
Stakes if we pick wrong: A broken renderer could slip through because the plan never proves the tests fail against missing behavior.
Recommendation: 3A because explicit stubs make the red step real.
Completeness: A=10/10, B=6/10
Pros / cons:
A) Add stubs, red run, green implementation (recommended): slightly longer task, much stronger signal.
B) Keep combined implementation/test step: shorter, but weaker TDD discipline.
Net: The extra step buys reliable test signal.

Auto decision D4 — Make stale ZIP replacement explicit
Context: ZIP export writes a sibling `.zip`; repeated exports can encounter an existing archive.
ELI10: If a user exports twice to the same destination, the second export should replace the old ZIP cleanly. A stale or half-written ZIP is worse than a clear failure.
Stakes if we pick wrong: Users may attach an old or corrupt pack while the folder contains newer evidence.
Recommendation: 4A because atomic temp-then-replace behavior is already the folder-export model.
Completeness: A=9/10, B=6/10
Pros / cons:
A) Add stale ZIP test and replacement logic (recommended): low effort, low risk, low maintenance.
B) Leave rename behavior platform-dependent: less code, but unclear repeat-export semantics.
Net: Deterministic repeat export is worth the small code.

Auto decision D5 — Add a concrete Smart Redaction fixture helper
Context: The Result Workspace pending-candidate test referenced a helper that did not exist.
ELI10: Tests should tell the executor exactly how to build the state they need. Hand-waving a helper wastes time and invites inconsistent fixtures.
Stakes if we pick wrong: The Smart Redaction block test may be skipped or implemented with a weak state that does not exercise the real gate.
Recommendation: 5A because an explicit `#[cfg(test)]` helper keeps the fixture reusable and private to tests.
Completeness: A=10/10, B=5/10
Pros / cons:
A) Add concrete helper (recommended): moderate snippet size, strong test determinism.
B) Keep “add a small helper if needed”: shorter plan, but too vague.
Net: Explicit fixtures beat implicit test setup.

Auto decision D6 — Add privacy-safe tracing for issue-pack export
Context: Rollshot active product paths require stable tracing targets and structured fields.
ELI10: If export fails on a user machine, logs should say which phase failed without leaking file paths. That makes support possible without exposing private paths or images.
Stakes if we pick wrong: Export failures become opaque, or logs accidentally reveal sensitive destination paths.
Recommendation: 6A because it follows repo diagnostics policy with minimal fields.
Completeness: A=9/10, B=5/10
Pros / cons:
A) Add start/success/failure tracing (recommended): low effort, better supportability.
B) Rely only on UI errors: less code, but poor diagnostics.
Net: Stable, path-free tracing is the right product-path default.

### NOT In Scope

- Hosted issue pages: deferred because the approved first release is local-only.
- GitHub/Jira/Linear API writes: deferred to avoid becoming a cloud bug-reporting service.
- CLI Issue Pack export: deferred until reuse pressure justifies moving the renderer out of `rollshot-app`.
- Browser console/network/DOM capture: deferred because Rollshot’s current capture model is cross-desktop visual evidence.
- Session replay backend and team inbox: deferred because they are a different product surface.
- AI-generated full bug narrative: deferred; deterministic Markdown placeholders keep privacy and review semantics clear.
- Automatic Action Guide keyframe redaction: deferred because the first release only asks for reviewed keyframes, not a redaction pipeline.

### What Already Exists

- Result Workspace safe image policy: reused through `ImageDocument::flatten()` and the existing pending Smart Redaction gate.
- Result Workspace OCR document: reused behind `ocr` for visible snippets; missing OCR remains non-blocking.
- Action Guide portable export: reused via `rollshot_action::export_guide` for `steps.md`, `session.json`, and keyframes.
- Action Guide GIF export: reused via `rollshot_action::export_gif`; failures become Issue Pack warnings.
- Existing folder/save dialogs: reused through `rfd::AsyncFileDialog`, matching current save/export UX.
- macOS product daemon forwarding: reused; no second iced event loop is introduced.

### Test Coverage Table

```text
Task / behavior                                      Unit  Integ  E2E / smoke  Manual only
---------------------------------------------------  ----  -----  -----------  -----------
Task 1 / deterministic folder names                  yes   no     no           no
Task 1 / screenshot Markdown relative links          yes   no     no           no
Task 1 / Action Guide Markdown links                 yes   no     no           no
Task 1 / OCR snippets omitted/included               yes   no     no           no
Task 2 / folder staging and required file writes     yes   yes    no           no
Task 2 / evidence review block                       yes   yes    no           no
Task 2 / missing evidence block                      yes   yes    no           no
Task 2 / manifest review/redaction/OCR/assets        yes   yes    no           no
Task 3 / Action Guide folder inclusion               yes   yes    no           no
Task 3 / missing retained keyframe rollback          yes   yes    no           no
Task 4 / ZIP layout matches folder                   yes   yes    no           no
Task 4 / stale ZIP replacement                       yes   yes    no           no
Task 5 / Result Workspace pending-candidate block    yes   yes    no           no
Task 5 / Result Workspace review gate                yes   yes    no           no
Task 5 / flattened Result Workspace image            yes   yes    no           no
Task 6 / Timeline review gate                        yes   yes    no           no
Task 6 / reviewed titles/keyframes exported          yes   yes    no           no
Task 6 / cancel path writes nothing                  yes   yes    no           no
Task 7 / no overclaiming safety text                 yes   yes    no           no
Task 7 / keyframes not labeled redacted              yes   yes    no           no
Task 8 / Linux GUI smoke                             no    no     yes          yes
Task 8 / macOS product path smoke                    no    no     yes          yes
```

### Failure Modes

| Codepath | Realistic failure | Covered | Handling | User-visible |
|---|---|---|---|---|
| Folder export staging | Destination cannot create temp dir | Task 2 export tests cover rollback shape; IO variant handles it | `IssuePackError::Io` | Error banner |
| Review gate | User bypasses disabled button by message/test path | Task 2 and Task 5/6 review tests | `IssuePackError::EvidenceReviewRequired` | Error banner |
| Missing evidence | No final image and no Action Guide steps | Task 2 missing evidence test | `IssuePackError::MissingEvidence` | Error banner |
| PNG final image write | Encoder/filesystem failure | Not directly forced; `Encode` variant exists | `IssuePackError::Encode` | Error banner |
| Manifest write | JSON or file write failure | Manifest happy path covered; error variant exists | `IssuePackError::Json` / `Io` | Error banner |
| Action Guide keyframe missing | Reviewed keyframe pixels not retained | Task 3 rollback test | `IssuePackError::Io("export failed: ...")` | Error banner |
| GIF optional export | GIF encoder/keyframe failure | Task 7 warning semantics | `IssuePackWarning { code: "gif_export_failed" }` | Success banner with warning |
| ZIP stale file | Existing ZIP at destination | Task 4 stale replacement test | Temp zip then replace | Error banner on failure |
| File picker cancel | User cancels destination picker | Task 5/6 cancel/no-op tests | `IssuePackFolderChosen(None)` | No write, no error |
| macOS forwarding | Product daemon fails to route new messages | Task 8 macOS smoke | Existing `Message::Workspace` / `Message::Timeline` mapping | Runtime verification |

Critical gaps flagged: none. PNG encoder and manifest write failure are represented and surfaced but not forced with a custom failing writer; that is acceptable because Rust std/image error injection would require extra test-only abstraction for little gain.

### Performance And Resource Notes

- Export is user-triggered and not on a capture or stitch hot path.
- The largest allocation is the flattened `RgbaImage`, which already exists for safe copy/save semantics.
- ZIP packaging streams file contents with `std::io::copy` and sorted directory traversal; it does not load the whole ZIP payload into memory.
- Export work is synchronous after destination selection, matching the existing Action Guide export path. If long-screenshot ZIP export visibly blocks the UI, the follow-up is an owned export payload plus `tokio::task::spawn_blocking`, not a new crate.

### Task Dependencies And Parallelization

| Task | Modules touched | Depends on |
|---|---|---|
| Task 1 | `crates/rollshot-app/src/issue_pack.rs`, `crates/rollshot-app/src/main.rs` | none |
| Task 2 | `crates/rollshot-app/src/issue_pack.rs` | Task 1 |
| Task 3 | `crates/rollshot-app/src/issue_pack.rs`, `crates/rollshot-action/` through existing APIs | Task 2 |
| Task 4 | root `Cargo.toml`, `crates/rollshot-app/Cargo.toml`, `crates/rollshot-app/src/issue_pack.rs` | Task 3 |
| Task 5 | `crates/rollshot-app/src/result_workspace/` | Task 4 |
| Task 6 | `crates/rollshot-app/src/timeline_workspace/` | Task 4 |
| Task 7 | `crates/rollshot-app/src/issue_pack.rs`, Result/Timeline views | Tasks 5 and 6 |
| Task 8 | verification only | Tasks 1-7 |

Parallel lanes:

- Lane A: Tasks 1 → 2 → 3 → 4, sequential, because they build the shared exporter and add a root dependency.
- Lane B: Task 5, can start after Task 4.
- Lane C: Task 6, can start after Task 4 and can run in parallel with Task 5.
- Lane D: Task 7 after Tasks 5 and 6.
- Lane E: Task 8 final verification.

Conflict flags:

- Task 4 modifies root `Cargo.toml` and serializes earlier work.
- Tasks 5 and 6 are parallel-safe after Task 4 because they touch different workspace modules.
- Task 7 touches both UI modules and should run after Tasks 5 and 6 to avoid merge churn.

---

## Self-Review

- Spec coverage: The plan covers Result Workspace export, Action Guide export, folder-first output, optional ZIP, deterministic Markdown, manifest schema version 1, flattened final image, Action Guide steps/session/keyframes, optional GIF warnings, OCR snippets, review gating, pending Smart Redaction blocking, and explicit safe/original language.
- Deferred scope remains out: No tracker APIs, hosted pages, CLI path, browser logs, session replay backend, team inbox, or AI-generated narrative.
- Platform split: Linux uses direct workspace windows; macOS is covered through existing `macos_product.rs` message forwarding and final macOS verification.
- Risk to watch during execution: `zip` crate API may need a minor adjustment depending on the resolved version. Keep the dependency pinned at workspace level and fix compile errors locally rather than widening scope.
