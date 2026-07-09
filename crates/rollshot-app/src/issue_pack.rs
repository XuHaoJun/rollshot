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

pub(crate) fn render_issue_markdown(input: &IssuePackInput, include_storyboard: bool) -> String {
    let mut md = String::from("# Bug Report\n\n");
    md.push_str("## Summary\n\n[Write a short summary]\n\n");
    md.push_str("## Steps to reproduce\n\n");
    if let Some(action) = &input.action_guide {
        if include_storyboard {
            md.push_str("Overview:\n\n");
            md.push_str("![](action-guide/storyboard.png)\n\n");
        }
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

pub(crate) fn manifest_assets(
    input: &IssuePackInput,
    include_gif: bool,
    include_storyboard: bool,
) -> Vec<AssetEntry> {
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
        if include_storyboard {
            assets.push(AssetEntry {
                kind: "action_storyboard".to_string(),
                path: "action-guide/storyboard.png".to_string(),
            });
        }
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
    include_storyboard: bool,
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
        assets: manifest_assets(input, include_gif, include_storyboard),
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

#[allow(clippy::ptr_arg)]
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

        let storyboard_path = tmp_dir.join("action-guide/storyboard.png");
        if let Err(error) = rollshot_action::export_storyboard(
            action.guide,
            action.store,
            rollshot_action::StoryboardOptions::default(),
            &storyboard_path,
        ) {
            warnings.push(IssuePackWarning {
                code: "storyboard_export_failed".to_string(),
                message: format!("Storyboard export failed: {error}"),
            });
        }

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

    let include_gif = tmp_dir.join("action-guide/guide.gif").exists();
    let include_storyboard = tmp_dir.join("action-guide/storyboard.png").exists();
    std::fs::write(
        tmp_dir.join("issue.md"),
        render_issue_markdown(input, include_storyboard),
    )
    .map_err(|e| IssuePackError::Io(e.to_string()))?;
    let manifest = render_manifest_json(input, warnings, include_gif, include_storyboard)?;
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
    writer
        .finish()
        .map_err(|e| IssuePackError::Io(e.to_string()))?;
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
        let md = render_issue_markdown(&base_input(), false);
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
        let md = render_issue_markdown(&input, false);
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
        let md = render_issue_markdown(&input, false);
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
        let assets = manifest_assets(&input, true, false);
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

        assert!(
            names.contains(&"images/final-redacted.png".to_string()),
            "names = {names:?}"
        );
        assert!(names.contains(&"issue.md".to_string()), "names = {names:?}");
        assert!(
            names.contains(&"manifest.json".to_string()),
            "names = {names:?}"
        );
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

    #[test]
    fn manifest_serializes_gif_export_failure_warning() {
        let input = base_input();
        let warnings = vec![IssuePackWarning {
            code: "gif_export_failed".to_string(),
            message: "GIF export failed: disk full".to_string(),
        }];
        let json = render_manifest_json(&input, &warnings, false, false).unwrap();

        assert!(json.contains("\"warnings\""), "json = {json}");
        assert!(
            json.contains("\"code\": \"gif_export_failed\""),
            "json = {json}"
        );
        assert!(
            json.contains("\"message\": \"GIF export failed: disk full\""),
            "json = {json}"
        );
    }

    fn action_guide_input_with_one_step(include_gif: bool) -> IssuePackInput {
        let mut input = base_input();
        input.final_image = None;
        input.action_guide = Some(ActionGuideIssueAssets {
            include_gif,
            steps: vec![IssuePackStep {
                index: 1,
                title: "Open Settings".to_string(),
                keyframe_path: "action-guide/keyframes/001.png".to_string(),
            }],
        });
        input.evidence_review.result_workspace_images_reviewed = false;
        input.evidence_review.action_guide_keyframes_reviewed = true;
        input.redaction.result_workspace_images_are_flattened = false;
        input
    }

    #[test]
    fn renders_storyboard_overview_when_action_storyboard_exists() {
        let input = action_guide_input_with_one_step(false);
        let md = render_issue_markdown(&input, true);

        assert!(
            md.contains("Overview:\n\n![](action-guide/storyboard.png)"),
            "md = {md}"
        );
        assert!(md.contains("1. Open Settings"), "md = {md}");
        assert!(
            md.contains("![](action-guide/keyframes/001.png)"),
            "md = {md}"
        );
    }

    #[test]
    fn omits_storyboard_overview_when_action_storyboard_is_absent() {
        let input = action_guide_input_with_one_step(false);
        let md = render_issue_markdown(&input, false);

        assert!(!md.contains("action-guide/storyboard.png"), "md = {md}");
        assert!(md.contains("1. Open Settings"), "md = {md}");
    }

    #[test]
    fn manifest_assets_include_storyboard_only_when_present() {
        let input = action_guide_input_with_one_step(true);

        let without_storyboard = manifest_assets(&input, true, false);
        assert!(
            !without_storyboard
                .iter()
                .any(|asset| asset.kind == "action_storyboard"),
            "assets = {without_storyboard:?}"
        );

        let with_storyboard = manifest_assets(&input, true, true);
        let paths: Vec<_> = with_storyboard
            .iter()
            .map(|asset| asset.path.as_str())
            .collect();
        assert!(
            paths.contains(&"action-guide/storyboard.png"),
            "paths = {paths:?}"
        );
        assert!(
            with_storyboard
                .iter()
                .any(|asset| asset.kind == "action_storyboard"
                    && asset.path == "action-guide/storyboard.png"),
            "assets = {with_storyboard:?}"
        );
    }
}

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

    fn action_input() -> (
        IssuePackInput,
        Guide,
        FrameStore,
        CaptureRegion,
        InputCapability,
        InputSourceKind,
    ) {
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

        assert!(
            manifest.contains("\"action_keyframe\""),
            "manifest = {manifest}"
        );
        assert!(
            !manifest.contains("redacted_keyframe"),
            "manifest = {manifest}"
        );
    }

    fn many_step_action_input(
        count: usize,
    ) -> (
        IssuePackInput,
        Guide,
        FrameStore,
        CaptureRegion,
        InputCapability,
        InputSourceKind,
    ) {
        let mut store = FrameStore::new(StoreConfig {
            ring_capacity: count + 16,
            analysis_capacity: 8,
            analysis_width: 384,
            window_before: 0,
            window_after: 0,
            nearby_max: 1,
        });
        let mut candidates = Vec::with_capacity(count);
        for i in 0..count {
            let id = store.ingest(quadrant(), i as u64 * 100);
            store.retain_window(id);
            candidates.push(CandidateStep {
                id,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: i as u64 * 100,
                keyframe: id,
                nearby: vec![id],
            });
        }
        let guide = Guide::from_candidates(candidates);
        let mut input = super::tests::base_input();
        input.final_image = None;
        input.evidence_review.result_workspace_images_reviewed = false;
        input.evidence_review.action_guide_keyframes_reviewed = true;
        input.redaction.result_workspace_images_are_flattened = false;
        input.action_guide = Some(ActionGuideIssueAssets::from_guide(&guide, false));
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
        assert!(result
            .directory
            .join("action-guide/keyframes/001.png")
            .exists());
        assert!(result
            .directory
            .join("action-guide/storyboard.png")
            .exists());
        assert!(
            result.warnings.is_empty(),
            "warnings = {:?}",
            result.warnings
        );

        let md = std::fs::read_to_string(result.directory.join("issue.md")).unwrap();
        assert!(
            md.contains("![](action-guide/keyframes/001.png)"),
            "md = {md}"
        );
        assert!(
            md.contains("Overview:\n\n![](action-guide/storyboard.png)"),
            "md = {md}"
        );

        let manifest = std::fs::read_to_string(result.directory.join("manifest.json")).unwrap();
        assert!(
            manifest.contains("\"action_keyframe\""),
            "manifest = {manifest}"
        );
        assert!(
            manifest.contains("\"action_storyboard\""),
            "manifest = {manifest}"
        );
        assert!(
            manifest.contains("\"action-guide/storyboard.png\""),
            "manifest = {manifest}"
        );
    }

    #[test]
    fn storyboard_export_failure_warns_without_blocking_issue_pack() {
        // 260 steps at default keyframe size exceeds StoryboardOptions::default().max_canvas_pixels
        let (input, guide, store, region, capability, source_kind) = many_step_action_input(260);
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
        assert!(result
            .directory
            .join("action-guide/keyframes/001.png")
            .exists());
        assert!(!result
            .directory
            .join("action-guide/storyboard.png")
            .exists());
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0].code, "storyboard_export_failed");

        let md = std::fs::read_to_string(result.directory.join("issue.md")).unwrap();
        assert!(!md.contains("action-guide/storyboard.png"), "md = {md}");

        let manifest = std::fs::read_to_string(result.directory.join("manifest.json")).unwrap();
        assert!(
            manifest.contains("\"storyboard_export_failed\""),
            "manifest = {manifest}"
        );
        assert!(
            !manifest.contains("\"action_storyboard\""),
            "manifest = {manifest}"
        );
    }

    #[test]
    fn combined_screenshot_and_action_guide_includes_storyboard() {
        let (mut input, guide, store, region, capability, source_kind) = action_input();
        // Restore the final_image that action_input() clears
        input.final_image = Some(SafeImageAsset {
            file_name: "final-redacted.png".to_string(),
            pixels: RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])),
            derived_from_original: true,
        });
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

        assert!(result
            .directory
            .join("action-guide/storyboard.png")
            .exists());
        assert!(result.directory.join("images/final-redacted.png").exists());

        let md = std::fs::read_to_string(result.directory.join("issue.md")).unwrap();
        assert!(
            md.contains("Overview:\n\n![](action-guide/storyboard.png)"),
            "md = {md}"
        );
        assert!(md.contains("![](images/final-redacted.png)"), "md = {md}");

        let manifest = std::fs::read_to_string(result.directory.join("manifest.json")).unwrap();
        assert!(
            manifest.contains("\"action_storyboard\""),
            "manifest = {manifest}"
        );
        assert!(
            manifest.contains("\"final_redacted_image\""),
            "manifest = {manifest}"
        );
    }

    #[test]
    fn export_zip_with_action_guide_includes_storyboard() {
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

        let result = export_zip_with_action_guide(&input, Some(action), tmp.path()).unwrap();
        let zip_path = result.zip_path.clone().expect("zip path");
        let file = std::fs::File::open(zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut names = Vec::new();
        for i in 0..archive.len() {
            names.push(archive.by_index(i).unwrap().name().to_string());
        }
        names.sort();

        assert!(
            names.contains(&"action-guide/storyboard.png".to_string()),
            "names = {names:?}"
        );
    }
}
