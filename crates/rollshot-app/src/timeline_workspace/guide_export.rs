use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Local};
use rollshot_action::{
    ExportError, GuideHotspot, NormalizedRect, ReviewedGuideExportJob, ReviewedGuideStep,
    ReviewedStepImage,
};
use rollshot_image_document::Annotation;

use super::TimelineWorkspace;

#[derive(Clone)]
pub(crate) struct PendingIssuePackExport {
    pub input: crate::issue_pack::IssuePackInput,
    pub source: crate::issue_pack::ActionGuideExportSource,
}

pub(crate) struct PendingStandaloneExport {
    pub operation_id: u64,
    pub created_at: DateTime<Local>,
    pub job: ReviewedGuideExportJob,
}

impl std::fmt::Debug for PendingStandaloneExport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingStandaloneExport")
            .field("operation_id", &self.operation_id)
            .field("created_at", &self.created_at)
            .finish_non_exhaustive()
    }
}

pub(crate) struct StandaloneExportRequest {
    pub operation_id: u64,
    pub parent: PathBuf,
    pub created_at: DateTime<Local>,
    pub job: ReviewedGuideExportJob,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StandaloneExportResult {
    pub operation_id: u64,
    pub directory: PathBuf,
    pub index_html: PathBuf,
}

pub(crate) async fn run_standalone_export(
    request: StandaloneExportRequest,
) -> Result<StandaloneExportResult, String> {
    tokio::task::spawn_blocking(move || export_standalone(request))
        .await
        .map_err(|_| "Action Guide export worker failed".to_string())?
}

fn safe_slug(title: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    let mut count = 0u32;
    for ch in title.chars() {
        if count >= 80 {
            break;
        }
        if ch.is_alphanumeric() {
            slug.push(ch.to_lowercase().next().unwrap_or(ch));
            prev_dash = false;
            count += 1;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    if slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "action-guide".to_string()
    } else {
        slug
    }
}

fn choose_destination(
    parent: &Path,
    title: &str,
    created_at: DateTime<Local>,
    suffix: u32,
) -> PathBuf {
    let base = format!(
        "{}-{}",
        safe_slug(title),
        created_at.format("%Y-%m-%d-%H%M%S")
    );
    let name = if suffix == 1 {
        base
    } else {
        format!("{base}-{suffix}")
    };
    parent.join(name)
}

fn commit_noreplace(temp: &Path, destination: &Path) -> Result<(), rustix::io::Errno> {
    use rustix::fs::{renameat_with, RenameFlags, CWD};
    renameat_with(CWD, temp, CWD, destination, RenameFlags::NOREPLACE)
}

struct TempGuideGuard {
    path: Option<PathBuf>,
}

impl TempGuideGuard {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn mark_committed(&mut self) {
        self.path = None;
    }
}

impl Drop for TempGuideGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

fn export_standalone(request: StandaloneExportRequest) -> Result<StandaloneExportResult, String> {
    export_standalone_with_commit_hook(request, |_, _| {})
}

fn export_standalone_with_commit_hook(
    request: StandaloneExportRequest,
    mut commit_hook: impl FnMut(u32, &Path),
) -> Result<StandaloneExportResult, String> {
    let StandaloneExportRequest {
        operation_id,
        parent,
        created_at,
        job,
    } = request;

    let tmp_name = format!(".tmp-{}", operation_id);
    let tmp = parent.join(&tmp_name);
    let mut guard = TempGuideGuard::new(tmp.clone());

    rollshot_action::render_guide_folder(&job, &tmp).map_err(|error| format!("{error}"))?;

    let mut suffix = 1u32;
    loop {
        let destination = choose_destination(&parent, &job.title, created_at, suffix);
        commit_hook(suffix, &destination);
        match commit_noreplace(&tmp, &destination) {
            Ok(()) => {
                guard.mark_committed();
                let index_html = destination.join("index.html");
                return Ok(StandaloneExportResult {
                    operation_id,
                    directory: destination,
                    index_html,
                });
            }
            Err(rustix::io::Errno::EXIST) => {
                suffix += 1;
            }
            Err(
                rustix::io::Errno::NOSYS | rustix::io::Errno::INVAL | rustix::io::Errno::NOTSUP,
            ) => {
                return Err(
                    "atomic no-replace commit is unsupported on this filesystem".to_string()
                );
            }
            Err(errno) => {
                return Err(format!("commit failed: {errno}"));
            }
        }
    }
}

pub(crate) fn build_reviewed_export_job(
    state: &TimelineWorkspace,
) -> Result<ReviewedGuideExportJob, ExportError> {
    let mut steps = Vec::new();
    for (i, step) in state.guide.steps().iter().enumerate() {
        let frame = state
            .store
            .retained(step.keyframe)
            .ok_or(ExportError::MissingKeyframe { index: i + 1 })?;
        let (w, h) = frame.image.dimensions();

        let image = match state.presentation.doc(step.source) {
            Some(doc)
                if doc.keyframe == step.keyframe && !doc.document.annotations().is_empty() =>
            {
                ReviewedStepImage::Annotated(doc.document.flatten_snapshot())
            }
            _ => ReviewedStepImage::Retained(Arc::clone(&frame.image)),
        };

        let hotspots = match state.presentation.doc(step.source) {
            Some(doc) if doc.keyframe == step.keyframe => build_hotspots(doc, w, h),
            _ => Vec::new(),
        };

        let caption = {
            let trimmed = step.caption.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        };

        steps.push(ReviewedGuideStep {
            index: i + 1,
            title: step.title.clone(),
            caption,
            kind: step.kind,
            reason: step.reason,
            at_ms: step.at_ms,
            image,
            hotspots,
        });
    }

    let job = ReviewedGuideExportJob {
        title: state.guide.effective_title().to_string(),
        region: state.region,
        input_source: state.source_kind,
        input_capability: state.capability,
        steps,
    };
    job.validate()?;
    Ok(job)
}

pub(crate) fn prepare_issue_pack_export(
    state: &TimelineWorkspace,
) -> Result<PendingIssuePackExport, String> {
    let include_gif = state
        .issue_pack
        .as_ref()
        .is_some_and(|dialog| dialog.include_gif);
    let job = build_reviewed_export_job(state).map_err(|error| error.to_string())?;
    let assets = crate::issue_pack::ActionGuideIssueAssets::from_job(&job, include_gif);
    let input = super::update::timeline_issue_pack_input(state, assets);
    let gif_frames = state
        .guide
        .steps()
        .iter()
        .enumerate()
        .map(|(offset, step)| {
            state
                .store
                .retained(step.keyframe)
                .map(|frame| Arc::clone(&frame.image))
                .ok_or_else(|| format!("step {} keyframe is unavailable", offset + 1))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PendingIssuePackExport {
        input,
        source: crate::issue_pack::ActionGuideExportSource {
            job,
            include_gif,
            gif_frames,
        },
    })
}

fn build_hotspots(
    doc: &super::annotation::StepAnnotationDocument,
    width: u32,
    height: u32,
) -> Vec<GuideHotspot> {
    let mut hotspots = Vec::new();
    for item in doc.document.navigator_items() {
        let Some(annotation) = doc.document.annotation(item.id) else {
            continue;
        };
        let explanation = match annotation {
            Annotation::TextNote { text, .. } => text.trim(),
            Annotation::NumberCallout { id, .. } => doc
                .explanations
                .get(id)
                .map(String::as_str)
                .unwrap_or("")
                .trim(),
            _ => "",
        };
        if explanation.is_empty() {
            continue;
        }
        let bounds = normalize_and_clamp(annotation, width, height);
        hotspots.push(GuideHotspot {
            annotation_id: item.id.0,
            bounds,
            explanation: explanation.to_string(),
        });
    }
    hotspots
}

fn normalize_and_clamp(annotation: &Annotation, width: u32, height: u32) -> NormalizedRect {
    let r = rollshot_image_document::annotation_bounds(annotation);
    let w = width as f32;
    let h = height as f32;
    let x = (r.x / w).clamp(0.0, 1.0);
    let y = (r.y / h).clamp(0.0, 1.0);
    let right = ((r.x + r.width) / w).clamp(0.0, 1.0);
    let bottom = ((r.y + r.height) / h).clamp(0.0, 1.0);
    NormalizedRect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rollshot_action::{CaptureRegion, InputCapability, InputSourceKind, ReviewedStepImage};
    use rollshot_image_document::ImagePoint;

    fn real_workspace() -> TimelineWorkspace {
        TimelineWorkspace::new(
            super::super::tests::recording_from_frames(),
            CaptureRegion {
                x: 0,
                y: 0,
                width: 32,
                height: 32,
            },
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
        )
    }

    fn standalone_request(parent: &std::path::Path) -> StandaloneExportRequest {
        let ws = real_workspace();
        let job = build_reviewed_export_job(&ws).unwrap();
        let naive = chrono::NaiveDate::from_ymd_opt(2026, 7, 16)
            .unwrap()
            .and_hms_opt(9, 8, 7)
            .unwrap();
        let created_at = chrono::Local.from_local_datetime(&naive).unwrap();
        StandaloneExportRequest {
            operation_id: 1,
            parent: parent.to_path_buf(),
            created_at,
            job,
        }
    }

    #[test]
    fn job_contains_text_notes_and_only_explained_callouts_in_navigator_order() {
        let mut state = real_workspace();
        let step = state.guide.steps()[0].clone();
        let doc = state
            .presentation
            .document_for_step(&step, &state.store)
            .unwrap();
        let late = doc
            .document
            .add_number_callout(ImagePoint::new(20.0, 20.0), ImagePoint::new(24.0, 24.0));
        doc.document
            .add_text_note(ImagePoint::new(2.0, 2.0), "First note".into())
            .unwrap();
        let silent = doc
            .document
            .add_number_callout(ImagePoint::new(10.0, 10.0), ImagePoint::new(14.0, 14.0));
        state
            .presentation
            .set_explanation(step.source, late, "Second explanation".into());
        state
            .presentation
            .set_explanation(step.source, silent, "   ".into());

        let job = build_reviewed_export_job(&state).unwrap();

        assert_eq!(job.steps[0].hotspots.len(), 2);
        assert_eq!(job.steps[0].hotspots[0].explanation, "First note");
        assert_eq!(job.steps[0].hotspots[1].explanation, "Second explanation");
        assert!(matches!(
            job.steps[0].image,
            ReviewedStepImage::Annotated(_)
        ));
    }

    #[test]
    fn job_without_matching_annotations_shares_retained_keyframe() {
        let state = real_workspace();
        let frame = Arc::clone(
            &state
                .store
                .retained(state.guide.steps()[0].keyframe)
                .unwrap()
                .image,
        );
        let job = build_reviewed_export_job(&state).unwrap();
        let ReviewedStepImage::Retained(exported) = &job.steps[0].image else {
            panic!("retained")
        };
        assert!(Arc::ptr_eq(exported, &frame));
    }

    #[test]
    fn job_is_isolated_from_edits_after_export_click() {
        let mut state = real_workspace();
        let job = build_reviewed_export_job(&state).unwrap();
        let exported_title = job.title.clone();
        let exported_step_title = job.steps[0].title.clone();

        state.guide.set_title("Edited after click".into());
        assert!(state.guide.rename(1, "Changed later".into()));

        assert_eq!(job.title, exported_title);
        assert_eq!(job.steps[0].title, exported_step_title);
        job.validate().unwrap();
    }

    #[test]
    fn folder_name_uses_title_time_and_numeric_suffix_without_replacing() {
        let parent = tempfile::tempdir().unwrap();
        let naive = chrono::NaiveDate::from_ymd_opt(2026, 7, 16)
            .unwrap()
            .and_hms_opt(9, 8, 7)
            .unwrap();
        let at = chrono::Local.from_local_datetime(&naive).unwrap();
        let first = choose_destination(parent.path(), "Checkout / Failure", at, 1);
        assert_eq!(
            first.file_name().unwrap(),
            "checkout-failure-2026-07-16-090807"
        );
        std::fs::create_dir(&first).unwrap();
        let second = choose_destination(parent.path(), "Checkout / Failure", at, 2);
        assert_eq!(
            second.file_name().unwrap(),
            "checkout-failure-2026-07-16-090807-2"
        );
    }

    #[test]
    fn failed_standalone_export_removes_temp_and_keeps_existing_output() {
        let parent = tempfile::tempdir().unwrap();
        let existing = parent.path().join("action-guide-2026-07-16-090807");
        std::fs::create_dir(&existing).unwrap();
        std::fs::write(existing.join("keep"), "safe").unwrap();
        let mut request = standalone_request(parent.path());
        request.job.steps.clear();
        let result = export_standalone(request);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("steps"));
        assert_eq!(
            std::fs::read_to_string(existing.join("keep")).unwrap(),
            "safe"
        );
        assert!(std::fs::read_dir(parent.path()).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".tmp-")));
    }

    #[test]
    fn concurrent_standalone_exports_retain_both_outputs() {
        let parent = tempfile::tempdir().unwrap();

        let naive = chrono::NaiveDate::from_ymd_opt(2026, 7, 16)
            .unwrap()
            .and_hms_opt(9, 8, 7)
            .unwrap();
        let created_at = chrono::Local.from_local_datetime(&naive).unwrap();

        let ws1 = real_workspace();
        let mut ws2 = real_workspace();
        ws2.guide.set_title("Second Guide".into());

        let job1 = build_reviewed_export_job(&ws1).unwrap();
        let job2 = build_reviewed_export_job(&ws2).unwrap();

        let req1 = StandaloneExportRequest {
            operation_id: 100,
            parent: parent.path().to_path_buf(),
            created_at,
            job: job1,
        };
        let req2 = StandaloneExportRequest {
            operation_id: 200,
            parent: parent.path().to_path_buf(),
            created_at,
            job: job2,
        };

        let handle1 = std::thread::spawn(move || export_standalone(req1));
        let handle2 = std::thread::spawn(move || export_standalone(req2));

        let result1 = handle1.join().unwrap().unwrap();
        let result2 = handle2.join().unwrap().unwrap();

        assert_ne!(result1.directory, result2.directory);
        assert!(result1.index_html.exists());
        assert!(result2.index_html.exists());
    }

    #[test]
    fn commit_collision_retries_without_replacing_external_directory() {
        let parent = tempfile::tempdir().unwrap();
        let request = standalone_request(parent.path());
        let first = choose_destination(parent.path(), &request.job.title, request.created_at, 1);
        let result = export_standalone_with_commit_hook(request, |attempt, destination| {
            if attempt == 1 {
                std::fs::create_dir(destination).unwrap();
                std::fs::write(destination.join("external"), "safe").unwrap();
            }
        })
        .unwrap();
        assert!(result
            .directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("-2"));
        assert_eq!(
            std::fs::read_to_string(first.join("external")).unwrap(),
            "safe"
        );
    }
}
