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
    md.push_str(&format!(
        "- Rollshot version: {}\n\n",
        input.rollshot_version
    ));
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
            Self::MissingEvidence => write!(
                f,
                "nothing to export: add a final image or reviewed Action Guide"
            ),
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
            result_workspace_images_reviewed: input
                .evidence_review
                .result_workspace_images_reviewed,
            action_guide_keyframes_reviewed: input.evidence_review.action_guide_keyframes_reviewed,
        },
        platform: PlatformManifest {
            os: &input.platform.os,
            arch: &input.platform.arch,
        },
        redaction: RedactionManifest {
            review_required: input.redaction.review_required,
            review_completed: input.redaction.review_completed,
            result_workspace_images_are_flattened: input
                .redaction
                .result_workspace_images_are_flattened,
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
    let build_result =
        build_folder(input, &tmp_dir, &warnings).and_then(|()| swap_folder(&tmp_dir, &final_dir));
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
        assert!(
            !md.contains("/tmp/"),
            "md must not contain absolute paths: {md}"
        );
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
        assert!(
            md.contains("![](action-guide/keyframes/001.png)"),
            "md = {md}"
        );
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

    #[test]
    fn export_folder_writes_required_files_and_flattened_image() {
        let input = base_input();
        let tmp = tempfile::tempdir().unwrap();
        let result = export_folder(&input, tmp.path()).unwrap();

        assert!(result
            .directory
            .ends_with("rollshot-issue-pack-2026-07-04-1530"));
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
}
