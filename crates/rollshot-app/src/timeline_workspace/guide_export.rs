use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Local};
use rollshot_action::{
    ExportError, GuideHotspot, NormalizedRect, ProjectReviewedImage, ReviewedGuideExportJob,
    ReviewedGuideStep, ReviewedStepImage,
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

fn unique_temp_path(parent: &Path, operation_id: u64) -> PathBuf {
    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);
    let temp_id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".tmp-{}-{}-{}",
        std::process::id(),
        operation_id,
        temp_id
    ))
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

    let tmp = unique_temp_path(&parent, operation_id);

    rollshot_action::render_guide_folder(&job, &tmp).map_err(|error| format!("{error}"))?;
    let mut guard = TempGuideGuard::new(tmp.clone());

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
        let (w, h, image) = if let Some(rollshot_action::StepFrameSource::Project(ref src)) =
            state.frame_source
        {
            let frame = src
                .frame(step.keyframe)
                .ok_or(ExportError::MissingKeyframe { index: i + 1 })?;
            let (w, h) = (frame.width, frame.height);

            let annotations = match state.presentation.doc(step.source) {
                Some(doc)
                    if doc.keyframe == step.keyframe && !doc.document.annotations().is_empty() =>
                {
                    Some(doc.document.annotations().to_vec())
                }
                _ => None,
            };

            (
                w,
                h,
                ReviewedStepImage::Project(ProjectReviewedImage {
                    project_root: src.root().to_path_buf(),
                    frame: frame.clone(),
                    annotations,
                    step: i + 1,
                }),
            )
        } else {
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

            (w, h, image)
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

    let title = {
        let effective = state.guide.effective_title();
        if effective == rollshot_action::DEFAULT_GUIDE_TITLE
            && state.guide.title().trim().is_empty()
        {
            rollshot_action::DEFAULT_GUIDE_TITLE.to_string()
        } else {
            effective.to_string()
        }
    };

    let job = ReviewedGuideExportJob {
        title,
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
    let publish_source = match &state.project_session {
        Some(super::project::ProjectSession::Saved {
            root,
            base_revision,
            ..
        }) => Some(crate::issue_pack::PublishSource {
            project_root: root.clone(),
            directory: root.join("publish"),
            revision: *base_revision,
        }),
        _ => None,
    };
    Ok(PendingIssuePackExport {
        input,
        source: crate::issue_pack::ActionGuideExportSource {
            job,
            include_gif,
            publish_source,
        },
    })
}

#[allow(dead_code)]
pub(crate) fn prepare_issue_pack_from_reviewed_job(
    state: &TimelineWorkspace,
    include_gif: bool,
) -> Result<PendingIssuePackExport, String> {
    let job = build_reviewed_export_job(state).map_err(|error| error.to_string())?;
    let assets = crate::issue_pack::ActionGuideIssueAssets::from_job(&job, include_gif);
    let input = super::update::timeline_issue_pack_input(state, assets);
    let publish_source = match &state.project_session {
        Some(super::project::ProjectSession::Saved {
            root,
            base_revision,
            ..
        }) => Some(crate::issue_pack::PublishSource {
            project_root: root.clone(),
            directory: root.join("publish"),
            revision: *base_revision,
        }),
        _ => None,
    };
    Ok(PendingIssuePackExport {
        input,
        source: crate::issue_pack::ActionGuideExportSource {
            job,
            include_gif,
            publish_source,
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
    fn temp_paths_are_unique_even_when_operation_ids_match() {
        let parent = tempfile::tempdir().unwrap();

        let first = unique_temp_path(parent.path(), 1);
        let second = unique_temp_path(parent.path(), 1);

        assert_ne!(first, second);
        assert_eq!(first.parent(), Some(parent.path()));
        assert_eq!(second.parent(), Some(parent.path()));
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

    fn project_workspace() -> (TimelineWorkspace, Vec<u64>) {
        use image::ImageEncoder;
        use image::RgbaImage;
        use rollshot_action::project::{
            EnabledOutputs, LoadedProject, ProjectFrame, ProjectManifestV1,
        };
        use rollshot_action::step_frame_source::ProjectFrameSource;
        use rollshot_action::{
            ActionRecorder, CaptureRegion, DetectorConfig, InputCapability, InputSourceKind,
            StoreConfig,
        };
        use sha2::{Digest, Sha256};

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("assets/frames")).unwrap();

        let mut rec = ActionRecorder::new(
            CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            StoreConfig::default(),
            DetectorConfig {
                diff_threshold: 0.01,
                area_threshold: 0.05,
                cooldown_ms: 0,
                ..DetectorConfig::default()
            },
        );
        let img1 = RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255]));
        let img2 = RgbaImage::from_pixel(8, 8, image::Rgba([40, 50, 60, 255]));
        rec.ingest_frame(RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 0, 255])), 0);
        rec.ingest_frame(img1.clone(), 100);
        rec.ingest_frame(img1.clone(), 200);
        rec.ingest_frame(img1.clone(), 300);
        rec.ingest_frame(img2.clone(), 400);
        rec.ingest_frame(img2.clone(), 500);
        rec.ingest_frame(img2.clone(), 600);
        let recording = rec.finish();
        let guide = rollshot_action::Guide::from_candidates(recording.candidates.clone());
        let kf_ids: Vec<u64> = guide.steps().iter().map(|s| s.keyframe).collect();

        let mut frames = Vec::new();
        for (i, img) in [&img1, &img2].iter().enumerate() {
            let mut buf = Vec::new();
            let encoder = image::codecs::png::PngEncoder::new(&mut buf);
            encoder
                .write_image(
                    img.as_raw(),
                    img.width(),
                    img.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .unwrap();
            let sha256 = format!("{:x}", Sha256::digest(&buf));
            let dest = root.join("assets/frames").join(format!("{sha256}.png"));
            std::fs::write(&dest, &buf).unwrap();
            frames.push(ProjectFrame {
                id: kf_ids[i],
                at_ms: (i as u64 + 1) * 100,
                sha256,
                width: img.width(),
                height: img.height(),
            });
        }

        let manifest = ProjectManifestV1 {
            schema_version: 1,
            revision: 1,
            title: "Test".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::VisualOnly {
                reason: rollshot_action::DegradedReason::SourceStartFailed,
            },
            enabled_outputs: EnabledOutputs::default(),
            frames,
            steps: Vec::new(),
        };
        let loaded = LoadedProject { root, manifest };
        let source = ProjectFrameSource::from_loaded(
            &loaded,
            rollshot_action::DEFAULT_PROJECT_FRAME_CACHE_BYTES,
        );

        let mut ws = TimelineWorkspace::new(
            recording,
            CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            InputCapability::VisualOnly {
                reason: rollshot_action::DegradedReason::SourceStartFailed,
            },
            InputSourceKind::VisualOnly,
        );
        ws.frame_source = Some(rollshot_action::StepFrameSource::Project(source));
        let _ = dir.keep();
        (ws, kf_ids)
    }

    #[test]
    fn build_reviewed_export_job_uses_default_title_when_guide_title_is_empty() {
        let mut ws = real_workspace();
        ws.guide.set_title("   ".into());

        let job = build_reviewed_export_job(&ws).unwrap();

        assert_eq!(job.title, rollshot_action::DEFAULT_GUIDE_TITLE);
        assert_eq!(
            ws.guide.title(),
            "   ",
            "original guide title must not be mutated"
        );
    }

    #[test]
    fn job_from_project_source_creates_lazy_descriptors() {
        let (ws, _kf_ids) = project_workspace();
        let job = build_reviewed_export_job(&ws).unwrap();

        assert_eq!(job.steps.len(), 2);
        assert!(matches!(job.steps[0].image, ReviewedStepImage::Project(_)));
        assert!(matches!(job.steps[1].image, ReviewedStepImage::Project(_)));
        assert_eq!(job.steps[0].image.dimensions(), (8, 8));
    }

    #[test]
    fn job_from_project_source_succeeds_when_asset_deleted_construction_lazy() {
        let (ws, kf_ids) = project_workspace();
        let source = ws.frame_source.as_ref().unwrap();
        let root = match source {
            rollshot_action::StepFrameSource::Project(src) => src.root().to_path_buf(),
            _ => unreachable!(),
        };
        let f2 = match source {
            rollshot_action::StepFrameSource::Project(src) => src.frame(kf_ids[1]).unwrap().clone(),
            _ => unreachable!(),
        };
        let asset2 = root
            .join("assets/frames")
            .join(format!("{}.png", f2.sha256));
        std::fs::remove_file(&asset2).unwrap();

        let job = build_reviewed_export_job(&ws).unwrap();
        assert_eq!(job.steps.len(), 2);
    }
}
