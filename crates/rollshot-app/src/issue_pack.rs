use chrono::{DateTime, Local};
use image::RgbaImage;
#[cfg(feature = "action-guide")]
use rollshot_action::project::PublishCancellation;
#[cfg(feature = "action-guide")]
use rollshot_action::project::PublishOutputKind;
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
    pub caption: Option<String>,
    pub keyframe_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActionGuideIssueAssets {
    pub steps: Vec<IssuePackStep>,
    pub include_gif: bool,
}

#[cfg(feature = "action-guide")]
#[derive(Clone)]
pub(crate) struct PublishSource {
    pub project_root: PathBuf,
    pub directory: PathBuf,
    pub revision: u64,
}

#[cfg(feature = "action-guide")]
#[derive(Clone)]
pub(crate) struct ActionGuideExportSource {
    pub job: rollshot_action::ReviewedGuideExportJob,
    pub include_gif: bool,
    pub publish_source: Option<PublishSource>,
}

#[cfg(feature = "action-guide")]
impl ActionGuideIssueAssets {
    pub(crate) fn from_job(
        job: &rollshot_action::ReviewedGuideExportJob,
        include_gif: bool,
    ) -> Self {
        let steps = job
            .steps
            .iter()
            .enumerate()
            .map(|(offset, step)| IssuePackStep {
                index: step.index,
                title: step.title.clone(),
                caption: step.caption.clone(),
                keyframe_path: format!("action-guide/keyframes/{:03}.png", offset + 1),
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
            md.push_str(&format!("{}. {}\n\n", step.index, step.title));
            if let Some(caption) = &step.caption {
                md.push_str(&format!("   {caption}\n\n"));
            }
            md.push_str(&format!("   ![]({})\n\n", step.keyframe_path));
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
        md.push_str("- `action-guide/index.html`\n");
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
            kind: "action_html".to_string(),
            path: "action-guide/index.html".to_string(),
        });
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
    #[allow(dead_code)]
    Cancelled,
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
            Self::Cancelled => write!(f, "issue pack export cancelled"),
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
            Self::Cancelled => "cancelled",
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
    #[cfg(feature = "action-guide")]
    {
        export_folder_impl(input, None, destination_parent, &PublishCancellation::new())
    }
    #[cfg(not(feature = "action-guide"))]
    {
        export_folder_impl(input, None, destination_parent)
    }
}

#[cfg(feature = "action-guide")]
#[allow(dead_code)]
pub(crate) fn export_folder_with_action_guide(
    input: &IssuePackInput,
    action: Option<ActionGuideExportSource>,
    destination_parent: &Path,
) -> Result<IssuePackExportResult, IssuePackError> {
    export_folder_impl(
        input,
        action,
        destination_parent,
        &PublishCancellation::new(),
    )
}

#[cfg(feature = "action-guide")]
pub(crate) fn export_folder_with_action_guide_cancellable(
    input: &IssuePackInput,
    action: Option<ActionGuideExportSource>,
    destination_parent: &Path,
    cancel: &PublishCancellation,
) -> Result<IssuePackExportResult, IssuePackError> {
    export_folder_impl(input, action, destination_parent, cancel)
}

#[cfg(not(feature = "action-guide"))]
type ActionGuideExportSource = std::marker::PhantomData<()>;

fn export_folder_impl(
    input: &IssuePackInput,
    #[cfg(feature = "action-guide")] action: Option<ActionGuideExportSource>,
    #[cfg(not(feature = "action-guide"))] _action: Option<ActionGuideExportSource>,
    destination_parent: &Path,
    #[cfg(feature = "action-guide")] cancel: &PublishCancellation,
) -> Result<IssuePackExportResult, IssuePackError> {
    validate(input)?;
    #[cfg(feature = "action-guide")]
    if cancel.is_cancelled() {
        return Err(IssuePackError::Cancelled);
    }
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
        cancel,
        #[cfg(feature = "action-guide")]
        action,
    );

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

    #[cfg(feature = "action-guide")]
    if cancel.is_cancelled() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return Err(IssuePackError::Cancelled);
    }

    commit_noreplace_dir(&tmp_dir, &final_dir)?;

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
    #[cfg(feature = "action-guide")] cancel: &PublishCancellation,
    #[cfg(feature = "action-guide")] action: Option<ActionGuideExportSource>,
) -> Result<(), IssuePackError> {
    std::fs::create_dir_all(tmp_dir).map_err(|e| IssuePackError::Io(e.to_string()))?;

    #[cfg(feature = "action-guide")]
    if cancel.is_cancelled() {
        return Err(IssuePackError::Cancelled);
    }

    if let Some(image) = &input.final_image {
        let images_dir = tmp_dir.join("images");
        std::fs::create_dir_all(&images_dir).map_err(|e| IssuePackError::Io(e.to_string()))?;
        image
            .pixels
            .save_with_format(images_dir.join(&image.file_name), image::ImageFormat::Png)
            .map_err(|e| IssuePackError::Encode(e.to_string()))?;
    }

    #[cfg(feature = "action-guide")]
    if cancel.is_cancelled() {
        return Err(IssuePackError::Cancelled);
    }

    #[cfg(feature = "action-guide")]
    if let Some(action) = action {
        let publish_src = action.publish_source.as_ref();
        let ag_dir = tmp_dir.join("action-guide");

        let core_copied = publish_src.is_some_and(|src| {
            try_copy_published_core(&src.project_root, &src.directory, src.revision, &ag_dir)
        });
        if !core_copied {
            rollshot_action::render_guide_folder(&action.job, &ag_dir)
                .map_err(|e| IssuePackError::Io(format!("export failed: {e}")))?;
        }

        if cancel.is_cancelled() {
            return Err(IssuePackError::Cancelled);
        }

        let storyboard_path = ag_dir.join("storyboard.png");
        let storyboard_copied = publish_src.is_some_and(|src| {
            try_copy_published_file(
                &src.project_root,
                &src.directory.join("storyboard.png"),
                src.revision,
                PublishOutputKind::Storyboard,
                &storyboard_path,
            )
        });
        if !storyboard_copied {
            let storyboard_result = rollshot_action::render_reviewed_storyboard_cancellable(
                &action.job,
                rollshot_action::StoryboardOptions::default(),
                cancel,
            );
            if let Err(error) = storyboard_result {
                warnings.push(IssuePackWarning {
                    code: "storyboard_export_failed".to_string(),
                    message: format!("Storyboard export failed: {error}"),
                });
            } else if let Ok(rendered) = storyboard_result {
                if let Err(error) = rendered
                    .image
                    .save_with_format(&storyboard_path, image::ImageFormat::Png)
                {
                    warnings.push(IssuePackWarning {
                        code: "storyboard_export_failed".to_string(),
                        message: format!("Storyboard export failed: {error}"),
                    });
                }
            }
        }

        if cancel.is_cancelled() {
            return Err(IssuePackError::Cancelled);
        }

        if action.include_gif {
            let gif_path = ag_dir.join("guide.gif");
            let gif_copied = publish_src.is_some_and(|src| {
                try_copy_published_file(
                    &src.project_root,
                    &src.directory.join("guide.gif"),
                    src.revision,
                    PublishOutputKind::Gif,
                    &gif_path,
                )
            });
            if !gif_copied {
                if let Err(error) = rollshot_action::export_reviewed_gif(
                    &action.job,
                    rollshot_action::GifOptions::default(),
                    cancel,
                    &gif_path,
                ) {
                    warnings.push(IssuePackWarning {
                        code: "gif_export_failed".to_string(),
                        message: format!("GIF export failed: {error}"),
                    });
                }
            }
        }
    }

    #[cfg(feature = "action-guide")]
    if cancel.is_cancelled() {
        return Err(IssuePackError::Cancelled);
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

#[cfg(feature = "action-guide")]
fn try_copy_published_file(
    project_root: &Path,
    source: &Path,
    current_revision: u64,
    kind: rollshot_action::project::PublishOutputKind,
    destination: &Path,
) -> bool {
    use rollshot_action::project::load_publish_state;
    let state = load_publish_state(project_root);
    if state.freshness(kind, current_revision)
        != rollshot_action::project::PublishFreshness::Current
    {
        return false;
    }
    if !source.exists() {
        return false;
    }
    copy_file_if_exists(source, destination)
}

#[cfg(feature = "action-guide")]
fn try_copy_published_core(
    project_root: &Path,
    publish_dir: &Path,
    current_revision: u64,
    destination: &Path,
) -> bool {
    use rollshot_action::project::load_publish_state;
    let state = load_publish_state(project_root);
    if state.freshness(
        rollshot_action::project::PublishOutputKind::Core,
        current_revision,
    ) != rollshot_action::project::PublishFreshness::Current
    {
        return false;
    }
    let index_html = publish_dir.join("index.html");
    let steps_md = publish_dir.join("steps.md");
    let session_json = publish_dir.join("session.json");
    if !index_html.exists() || !steps_md.exists() || !session_json.exists() {
        return false;
    }
    std::fs::create_dir_all(destination).is_ok()
        && copy_file_if_exists(&index_html, &destination.join("index.html"))
        && copy_file_if_exists(&steps_md, &destination.join("steps.md"))
        && copy_file_if_exists(&session_json, &destination.join("session.json"))
        && copy_dir_if_exists(
            &publish_dir.join("keyframes"),
            &destination.join("keyframes"),
        )
}

#[cfg(feature = "action-guide")]
fn copy_file_if_exists(source: &Path, destination: &Path) -> bool {
    if !source.exists() {
        return true;
    }
    let meta = match std::fs::symlink_metadata(source) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if meta.file_type().is_symlink() {
        return false;
    }
    std::fs::copy(source, destination).is_ok()
}

#[cfg(feature = "action-guide")]
fn copy_dir_if_exists(source: &Path, destination: &Path) -> bool {
    if !source.exists() {
        return true;
    }
    if std::fs::create_dir_all(destination).is_err() {
        return false;
    }
    let entries = match std::fs::read_dir(source) {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        let src = entry.path();
        let dst = destination.join(entry.file_name());
        let meta = match std::fs::symlink_metadata(&src) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.file_type().is_dir() {
            if !copy_dir_if_exists(&src, &dst) {
                return false;
            }
        } else if std::fs::copy(&src, &dst).is_err() {
            return false;
        }
    }
    true
}

fn commit_noreplace_dir(tmp_dir: &Path, final_dir: &Path) -> Result<(), IssuePackError> {
    use rustix::fs::{renameat_with, RenameFlags, CWD};
    match renameat_with(CWD, tmp_dir, CWD, final_dir, RenameFlags::NOREPLACE) {
        Ok(()) => Ok(()),
        Err(rustix::io::Errno::EXIST) => Err(IssuePackError::Io(format!(
            "destination already exists: {}",
            final_dir.display()
        ))),
        Err(rustix::io::Errno::NOSYS | rustix::io::Errno::INVAL | rustix::io::Errno::NOTSUP) => {
            Err(IssuePackError::Io(
                "atomic no-replace commit is unsupported on this filesystem".to_string(),
            ))
        }
        Err(errno) => Err(IssuePackError::Io(format!("commit failed: {errno}"))),
    }
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
#[allow(dead_code)]
pub(crate) fn export_zip_with_action_guide(
    input: &IssuePackInput,
    action: Option<ActionGuideExportSource>,
    destination_parent: &Path,
) -> Result<IssuePackExportResult, IssuePackError> {
    let mut result = export_folder_with_action_guide(input, action, destination_parent)?;
    let zip_path = result.directory.with_extension("zip");
    zip_directory(&result.directory, &zip_path)?;
    result.zip_path = Some(zip_path);
    Ok(result)
}

#[cfg(feature = "action-guide")]
pub(crate) fn export_zip_with_action_guide_cancellable(
    input: &IssuePackInput,
    action: Option<ActionGuideExportSource>,
    destination_parent: &Path,
    cancel: &PublishCancellation,
) -> Result<IssuePackExportResult, IssuePackError> {
    let mut result =
        export_folder_with_action_guide_cancellable(input, action, destination_parent, cancel)?;
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
                    caption: None,
                    keyframe_path: "action-guide/keyframes/001.png".to_string(),
                },
                IssuePackStep {
                    index: 2,
                    title: "Click Save".to_string(),
                    caption: None,
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
                caption: None,
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
                "action-guide/index.html",
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
    fn export_zip_does_not_overwrite_existing_destination() {
        let input = base_input();
        let tmp = tempfile::tempdir().unwrap();
        let first = export_zip(&input, tmp.path()).unwrap();
        let first_dir = first.directory.clone();

        let err = export_zip(&input, tmp.path()).unwrap_err();

        assert!(
            matches!(err, IssuePackError::Io(_)),
            "expected Io error for existing destination, got: {err:?}"
        );
        assert!(first_dir.exists(), "original folder must be preserved");
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
                caption: None,
                keyframe_path: "action-guide/keyframes/001.png".to_string(),
            }],
        });
        input.evidence_review.result_workspace_images_reviewed = false;
        input.evidence_review.action_guide_keyframes_reviewed = true;
        input.redaction.result_workspace_images_are_flattened = false;
        input
    }

    #[test]
    fn issue_markdown_includes_action_step_caption_when_present() {
        let mut input = action_guide_input_with_one_step(false);
        input.action_guide.as_mut().unwrap().steps[0].caption =
            Some("The dialog closes but the setting is not saved.".to_string());

        let md = render_issue_markdown(&input, true);

        assert!(
            md.contains("The dialog closes but the setting is not saved."),
            "md = {md}"
        );
        assert!(
            md.contains("1. Open Settings\n\n   The dialog closes but the setting is not saved.\n\n   ![](action-guide/keyframes/001.png)"),
            "md = {md}"
        );
    }

    #[test]
    fn issue_markdown_omits_empty_action_step_caption() {
        let mut input = action_guide_input_with_one_step(false);
        input.action_guide.as_mut().unwrap().steps[0].caption = None;

        let md = render_issue_markdown(&input, true);

        assert!(md.contains("1. Open Settings\n\n   ![](action-guide/keyframes/001.png)"));
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
        FrameStore, Guide, InputCapability, InputSourceKind, Recording, ReviewedGuideExportJob,
        ReviewedGuideStep, ReviewedStepImage, StoreConfig,
    };
    use std::sync::Arc;

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
        ReviewedGuideExportJob,
        FrameStore,
        CaptureRegion,
    ) {
        let recording = recording();
        let guide = Guide::from_candidates(recording.candidates);
        let store = recording.store;
        let mut input = super::tests::base_input();
        input.final_image = None;
        input.evidence_review.result_workspace_images_reviewed = false;
        input.evidence_review.action_guide_keyframes_reviewed = true;
        input.redaction.result_workspace_images_are_flattened = false;
        let job = build_job(
            &guide,
            &store,
            region(),
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
        )
        .unwrap();
        input.action_guide = Some(ActionGuideIssueAssets::from_job(&job, true));
        (input, job, store, region())
    }

    fn build_job(
        guide: &Guide,
        store: &FrameStore,
        region: CaptureRegion,
        capability: InputCapability,
        source_kind: InputSourceKind,
    ) -> Result<ReviewedGuideExportJob, String> {
        let steps = guide
            .steps()
            .iter()
            .map(|step| {
                let frame = store
                    .retained(step.keyframe)
                    .ok_or_else(|| format!("keyframe {} not retained", step.keyframe))?;
                Ok(ReviewedGuideStep {
                    index: step.index,
                    title: step.title.clone(),
                    caption: {
                        let trimmed = step.caption.trim();
                        if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed.to_string())
                        }
                    },
                    kind: step.kind,
                    reason: step.reason,
                    at_ms: step.at_ms,
                    image: ReviewedStepImage::Retained(Arc::clone(&frame.image)),
                    hotspots: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(ReviewedGuideExportJob {
            title: guide.effective_title().to_string(),
            region,
            input_source: source_kind,
            input_capability: capability,
            steps,
        })
    }

    #[test]
    fn action_guide_issue_assets_maps_non_empty_captions() {
        let recording = recording();
        let mut guide = Guide::from_candidates(recording.candidates);
        assert!(guide.set_caption(1, "The value is lost after Save.".to_string()));
        let store = recording.store;
        let job = build_job(
            &guide,
            &store,
            region(),
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
        )
        .unwrap();

        let assets = ActionGuideIssueAssets::from_job(&job, false);

        assert_eq!(
            assets.steps[0].caption.as_deref(),
            Some("The value is lost after Save.")
        );
    }

    #[test]
    fn action_guide_only_missing_keyframe_rolls_back_temp_output() {
        let (_input, _job, store, _region) = action_input();
        let guide = Guide::from_candidates(vec![CandidateStep {
            id: 1,
            kind: CandidateKind::Click,
            reason: DetectReason::ClickConfirmed,
            at_ms: 0,
            keyframe: 9999,
            nearby: vec![9999],
        }]);
        let tmp = tempfile::tempdir().unwrap();

        // build_job fails because keyframe 9999 is not retained.
        let job_result = build_job(
            &guide,
            &store,
            region(),
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
        );
        assert!(
            job_result.is_err(),
            "build_job should fail for missing keyframe"
        );
        let err = job_result.err().unwrap();
        assert!(
            err.contains("not retained"),
            "error should mention missing keyframe: {err}"
        );
        assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
    }

    #[test]
    fn action_keyframes_are_listed_as_reviewed_evidence_not_redacted_assets() {
        let (input, job, _store, _region) = action_input();
        let tmp = tempfile::tempdir().unwrap();
        let source = ActionGuideExportSource {
            job,
            include_gif: false,
            publish_source: None,
        };

        let result = export_folder_with_action_guide(&input, Some(source), tmp.path()).unwrap();
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
        ReviewedGuideExportJob,
        FrameStore,
        CaptureRegion,
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
        let job = build_job(
            &guide,
            &store,
            region(),
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
        )
        .unwrap();
        input.action_guide = Some(ActionGuideIssueAssets::from_job(&job, false));
        (input, job, store, region())
    }

    #[test]
    fn export_folder_includes_action_guide_folder() {
        let (input, job, _store, _region) = action_input();
        let tmp = tempfile::tempdir().unwrap();
        let source = ActionGuideExportSource {
            job,
            include_gif: false,
            publish_source: None,
        };

        let result = export_folder_with_action_guide(&input, Some(source), tmp.path()).unwrap();

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
        let (input, job, _store, _region) = many_step_action_input(260);
        let tmp = tempfile::tempdir().unwrap();
        let source = ActionGuideExportSource {
            job,
            include_gif: false,
            publish_source: None,
        };

        let result = export_folder_with_action_guide(&input, Some(source), tmp.path()).unwrap();

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
        let (mut input, job, _store, _region) = action_input();
        // Restore the final_image that action_input() clears
        input.final_image = Some(SafeImageAsset {
            file_name: "final-redacted.png".to_string(),
            pixels: RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])),
            derived_from_original: true,
        });
        let tmp = tempfile::tempdir().unwrap();
        let source = ActionGuideExportSource {
            job,
            include_gif: false,
            publish_source: None,
        };

        let result = export_folder_with_action_guide(&input, Some(source), tmp.path()).unwrap();

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
        let (input, job, _store, _region) = action_input();
        let tmp = tempfile::tempdir().unwrap();
        let source = ActionGuideExportSource {
            job,
            include_gif: false,
            publish_source: None,
        };

        let result = export_zip_with_action_guide(&input, Some(source), tmp.path()).unwrap();
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

    #[test]
    fn action_guide_issue_pack_renders_storyboard_from_job_data() {
        let (input, job, _store, _region) = action_input();
        let temp = tempfile::tempdir().unwrap();
        let source = ActionGuideExportSource {
            job,
            include_gif: false,
            publish_source: None,
        };

        let result =
            export_folder_with_action_guide(&input, Some(source), temp.path()).expect("issue pack");

        assert!(result
            .directory
            .join("action-guide/storyboard.png")
            .exists());
        assert!(
            result.warnings.is_empty(),
            "warnings = {:?}",
            result.warnings
        );
        let manifest = std::fs::read_to_string(result.directory.join("manifest.json")).unwrap();
        assert!(
            manifest.contains("\"action_storyboard\""),
            "manifest = {manifest}"
        );
    }

    fn issue_pack_test_job() -> rollshot_action::ReviewedGuideExportJob {
        rollshot_action::ReviewedGuideExportJob {
            title: "Checkout failure".into(),
            region: rollshot_action::CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: rollshot_action::InputSourceKind::LinuxEvdev,
            input_capability: rollshot_action::InputCapability::SemanticEvents,
            steps: vec![rollshot_action::ReviewedGuideStep {
                index: 1,
                title: "Submit order".into(),
                caption: Some("Confirm the request".into()),
                kind: rollshot_action::CandidateKind::Click,
                reason: rollshot_action::DetectReason::ClickConfirmed,
                at_ms: 100,
                image: rollshot_action::ReviewedStepImage::Retained(Arc::new(RgbaImage::new(8, 8))),
                hotspots: vec![rollshot_action::GuideHotspot {
                    annotation_id: 1,
                    bounds: rollshot_action::NormalizedRect {
                        x: 0.1,
                        y: 0.1,
                        width: 0.2,
                        height: 0.2,
                    },
                    explanation: "Open Settings".into(),
                }],
            }],
        }
    }

    fn owned_action_source() -> ActionGuideExportSource {
        ActionGuideExportSource {
            job: issue_pack_test_job(),
            include_gif: false,
            publish_source: None,
        }
    }

    fn reviewed_issue_pack_input() -> IssuePackInput {
        let mut input = super::tests::base_input();
        input.final_image = None;
        input.action_guide = Some(ActionGuideIssueAssets::from_job(
            &issue_pack_test_job(),
            false,
        ));
        input.evidence_review.required = true;
        input.evidence_review.completed = true;
        input.evidence_review.action_guide_keyframes_reviewed = true;
        input
    }

    #[test]
    fn issue_pack_lists_and_writes_interactive_guide() {
        let parent = tempfile::tempdir().unwrap();
        let input = reviewed_issue_pack_input();
        let result =
            export_folder_with_action_guide(&input, Some(owned_action_source()), parent.path())
                .unwrap();
        assert!(result.directory.join("action-guide/index.html").is_file());
        let issue = std::fs::read_to_string(result.directory.join("issue.md")).unwrap();
        assert!(issue.contains("`action-guide/index.html`"));
        let manifest = std::fs::read_to_string(result.directory.join("manifest.json")).unwrap();
        assert!(manifest.contains("\"action_html\""));
    }

    #[test]
    fn issue_pack_html_failure_rolls_back_outer_transaction() {
        let parent = tempfile::tempdir().unwrap();
        let mut source = owned_action_source();
        source.job.steps[0].hotspots[0].explanation.clear();
        assert!(export_folder_with_action_guide(
            &reviewed_issue_pack_input(),
            Some(source),
            parent.path()
        )
        .is_err());
        assert!(std::fs::read_dir(parent.path()).unwrap().next().is_none());
    }

    #[test]
    fn issue_pack_zip_includes_interactive_guide_html() {
        let parent = tempfile::tempdir().unwrap();
        let input = reviewed_issue_pack_input();
        let result =
            export_zip_with_action_guide(&input, Some(owned_action_source()), parent.path())
                .unwrap();
        let zip_path = result.zip_path.clone().expect("zip path");
        let file = std::fs::File::open(zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut names = Vec::new();
        for i in 0..archive.len() {
            names.push(archive.by_index(i).unwrap().name().to_string());
        }
        assert!(
            names.contains(&"action-guide/index.html".to_string()),
            "names = {names:?}"
        );
    }

    #[test]
    fn cancellation_before_export_fails_cleanly() {
        let parent = tempfile::tempdir().unwrap();
        let input = reviewed_issue_pack_input();
        let cancel = PublishCancellation::new();
        cancel.cancel();
        let err = export_folder_with_action_guide_cancellable(
            &input,
            Some(owned_action_source()),
            parent.path(),
            &cancel,
        )
        .unwrap_err();
        assert_eq!(err, IssuePackError::Cancelled);
        assert!(
            std::fs::read_dir(parent.path()).unwrap().next().is_none(),
            "cancelled export must not leave output"
        );
    }

    #[test]
    fn cancellation_during_storyboard_fails_cleanly() {
        let parent = tempfile::tempdir().unwrap();
        let mut input = reviewed_issue_pack_input();
        input.action_guide = Some(ActionGuideIssueAssets::from_job(
            &issue_pack_test_job(),
            true,
        ));
        let cancel = PublishCancellation::new();
        cancel.cancel();
        let err = export_folder_with_action_guide_cancellable(
            &input,
            Some(ActionGuideExportSource {
                job: issue_pack_test_job(),
                include_gif: true,
                publish_source: None,
            }),
            parent.path(),
            &cancel,
        )
        .unwrap_err();
        assert_eq!(err, IssuePackError::Cancelled);
        assert!(
            std::fs::read_dir(parent.path()).unwrap().next().is_none(),
            "cancelled export must not leave output"
        );
    }

    #[test]
    fn no_replace_does_not_overwrite_existing_issue_pack() {
        let parent = tempfile::tempdir().unwrap();
        let input = reviewed_issue_pack_input();
        let first =
            export_folder_with_action_guide(&input, Some(owned_action_source()), parent.path())
                .unwrap();
        let err =
            export_folder_with_action_guide(&input, Some(owned_action_source()), parent.path())
                .unwrap_err();
        assert!(
            matches!(err, IssuePackError::Io(_)),
            "expected Io error for existing destination, got: {err:?}"
        );
        assert!(first.directory.exists(), "original must be preserved");
    }

    #[test]
    fn issue_pack_never_copies_project_assets_or_manifest() {
        let parent = tempfile::tempdir().unwrap();
        let input = reviewed_issue_pack_input();
        let result =
            export_folder_with_action_guide(&input, Some(owned_action_source()), parent.path())
                .unwrap();
        assert!(!result.directory.join("assets").exists());
        assert!(!result.directory.join("project.json").exists());
        assert!(!result.directory.join(".rollshot-guide").exists());
        let all_entries: Vec<String> = walk_dir(&result.directory);
        for entry in &all_entries {
            assert!(
                !entry.contains(".rollshot-guide"),
                "must not contain .rollshot-guide: {entry}"
            );
            assert!(
                !entry.ends_with("project.json"),
                "must not contain project.json: {entry}"
            );
        }
    }

    #[test]
    fn stale_publish_files_are_not_copied() {
        let parent = tempfile::tempdir().unwrap();
        let publish_dir = parent.path().join("publish");
        std::fs::create_dir_all(publish_dir.join("keyframes")).unwrap();
        std::fs::write(publish_dir.join("index.html"), "<stale>").unwrap();
        std::fs::write(publish_dir.join("steps.md"), "stale").unwrap();
        std::fs::write(
            publish_dir.join("session.json"),
            r#"{"schema_version":1,"title":"Stale","region":{"x":0,"y":0,"width":8,"height":8},"input_source":"visual-only","input_capability":"semantic-events","steps":[]}"#,
        )
        .unwrap();
        let mut state = rollshot_action::project::PublishStateV1::default();
        state.outputs.insert(
            rollshot_action::project::PublishOutputKind::Core,
            rollshot_action::project::PublishedOutputV1::new(99),
        );
        rollshot_action::project::write_publish_state(parent.path(), &state).unwrap();

        let input = reviewed_issue_pack_input();
        let mut source = owned_action_source();
        source.publish_source = Some(PublishSource {
            project_root: parent.path().to_path_buf(),
            directory: publish_dir.clone(),
            revision: 1,
        });

        let result =
            export_folder_with_action_guide(&input, Some(source), &parent.path().join("out"))
                .unwrap();

        let html =
            std::fs::read_to_string(result.directory.join("action-guide/index.html")).unwrap();
        assert!(
            !html.contains("<stale>"),
            "must not use stale published files"
        );
    }

    #[test]
    fn current_publish_files_are_copied_without_rebuilding() {
        let parent = tempfile::tempdir().unwrap();
        let project_root = parent.path();
        let publish_dir = project_root.join("publish");
        std::fs::create_dir_all(publish_dir.join("keyframes")).unwrap();
        let html = "<html>cached</html>";
        std::fs::write(publish_dir.join("index.html"), html).unwrap();
        std::fs::write(publish_dir.join("steps.md"), "# Cached Steps").unwrap();
        std::fs::write(
            publish_dir.join("session.json"),
            r#"{"schema_version":1,"title":"Cached","region":{"x":0,"y":0,"width":8,"height":8},"input_source":"visual-only","input_capability":"semantic-events","steps":[{"index":1,"title":"S","kind":"click","reason":"click-confirmed","at_ms":100,"keyframe_file":"keyframes/001.png","hotspots":[]}]}"#,
        )
        .unwrap();
        let png = image::RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        png.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        std::fs::write(publish_dir.join("keyframes/001.png"), buf.into_inner()).unwrap();

        let mut state = rollshot_action::project::PublishStateV1::default();
        state.outputs.insert(
            rollshot_action::project::PublishOutputKind::Core,
            rollshot_action::project::PublishedOutputV1::new(1),
        );
        rollshot_action::project::write_publish_state(project_root, &state).unwrap();

        let input = reviewed_issue_pack_input();
        let mut source = owned_action_source();
        source.publish_source = Some(PublishSource {
            project_root: project_root.to_path_buf(),
            directory: publish_dir.clone(),
            revision: 1,
        });

        let result =
            export_folder_with_action_guide(&input, Some(source), &parent.path().join("out"))
                .unwrap();

        let copied_html =
            std::fs::read_to_string(result.directory.join("action-guide/index.html")).unwrap();
        assert!(
            copied_html.contains("cached"),
            "must use current published core files, got: {copied_html}"
        );
    }

    #[test]
    fn only_successful_derivatives_are_included() {
        let (input, job, _store, _region) = many_step_action_input(260);
        let tmp = tempfile::tempdir().unwrap();
        let source = ActionGuideExportSource {
            job,
            include_gif: false,
            publish_source: None,
        };

        let result = export_folder_with_action_guide(&input, Some(source), tmp.path()).unwrap();

        assert!(result.directory.join("action-guide/steps.md").exists());
        assert!(result
            .directory
            .join("action-guide/keyframes/001.png")
            .exists());
        assert!(
            !result
                .directory
                .join("action-guide/storyboard.png")
                .exists(),
            "failed storyboard must not be included"
        );
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0].code, "storyboard_export_failed");

        let manifest = std::fs::read_to_string(result.directory.join("manifest.json")).unwrap();
        assert!(
            !manifest.contains("\"action_storyboard\""),
            "failed storyboard must not appear in manifest: {manifest}"
        );
    }

    #[test]
    fn render_resolves_one_frame_at_a_time() {
        let recording = recording();
        let guide = Guide::from_candidates(recording.candidates);
        let store = recording.store;
        let job = build_job(
            &guide,
            &store,
            region(),
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
        )
        .unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let ag_dir = tmp.path().join("action-guide");
        rollshot_action::render_guide_folder(&job, &ag_dir).unwrap();

        let keyframes_dir = ag_dir.join("keyframes");
        let count = std::fs::read_dir(&keyframes_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "png"))
            .count();
        assert_eq!(count, guide.steps().len());
        for (i, step) in guide.steps().iter().enumerate() {
            let path = keyframes_dir.join(format!("{:03}.png", i + 1));
            assert!(path.exists(), "keyframe {} must exist", step.index);
            let img = image::open(&path).unwrap();
            assert!(img.width() > 0 && img.height() > 0);
        }
    }

    fn walk_dir(dir: &std::path::Path) -> Vec<String> {
        let mut result = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let rel = path.strip_prefix(dir).unwrap_or(&path);
                result.push(rel.to_string_lossy().to_string());
                if path.is_dir() {
                    result.extend(walk_dir(&path));
                }
            }
        }
        result
    }
}
