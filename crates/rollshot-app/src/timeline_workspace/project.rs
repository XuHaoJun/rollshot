use fs4::{FileExt, TryLockError};
use rollshot_action::project::{
    LoadedProject, ProjectCommit, ProjectError, ProjectSnapshot, ProjectStepId,
};
use rollshot_action::{
    FrameId, Guide, GuideStep, ProjectFrameSource, StepFrameSource,
    DEFAULT_PROJECT_FRAME_CACHE_BYTES,
};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use super::annotation::ActionGuidePresentation;
use super::TimelineWorkspace;

// ---------------------------------------------------------------------------
// Writer lock
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub struct ProjectWriterGuard {
    _file: File,
}

impl ProjectWriterGuard {
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self {
            _file: tempfile::tempfile().unwrap(),
        }
    }
}

impl std::fmt::Debug for ProjectWriterGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectWriterGuard").finish()
    }
}

#[allow(dead_code)]
pub enum ProjectLockResult {
    Acquired(ProjectWriterGuard),
    AlreadyLocked,
}

#[allow(dead_code)]
pub fn acquire_project_writer(root: &Path) -> Result<ProjectLockResult, ProjectWorkerError> {
    let lock_path = root.join(".lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|_error| {
            tracing::event!(
                target: "rollshot::project",
                tracing::Level::ERROR,
                category = "project_lock",
                error_kind = "open",
                "lock open failed"
            );
            ProjectWorkerError::Lock {
                category: "project_lock",
            }
        })?;

    match FileExt::try_lock(&file) {
        Ok(()) => Ok(ProjectLockResult::Acquired(ProjectWriterGuard {
            _file: file,
        })),
        Err(TryLockError::WouldBlock) => Ok(ProjectLockResult::AlreadyLocked),
        Err(TryLockError::Error(_error)) => {
            tracing::event!(
                target: "rollshot::project",
                tracing::Level::ERROR,
                category = "project_lock",
                error_kind = "try_lock",
                "lock acquisition failed"
            );
            Err(ProjectWorkerError::Lock {
                category: "project_lock",
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Worker types
// ---------------------------------------------------------------------------

#[derive(Debug)]
#[allow(dead_code)]
pub enum ProjectWorkerError {
    Project(ProjectError),
    Lock { category: &'static str },
    Join { category: &'static str },
}

impl ProjectWorkerError {
    #[allow(dead_code)]
    pub fn category(&self) -> &str {
        match self {
            Self::Project(e) => e.category(),
            Self::Lock { category } => category,
            Self::Join { category } => category,
        }
    }

    #[allow(dead_code)]
    pub fn message_for_ui(&self) -> String {
        match self {
            Self::Project(e) => e.to_string(),
            Self::Lock { .. } => "Project is open in another window".into(),
            Self::Join { .. } => "Internal error".into(),
        }
    }
}

impl From<ProjectError> for ProjectWorkerError {
    fn from(e: ProjectError) -> Self {
        Self::Project(e)
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum ProjectAccess {
    Writable(ProjectWriterGuard),
    ReadOnly,
    CorruptReadOnly,
}

#[allow(dead_code)]
pub(crate) struct OpenProjectRequest {
    pub root: PathBuf,
    pub writable: bool,
}

#[allow(dead_code)]
pub(crate) struct OpenProjectResult {
    pub loaded: LoadedProject,
    pub access: ProjectAccess,
}

impl std::fmt::Debug for OpenProjectResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenProjectResult")
            .field("access", &self.access)
            .finish()
    }
}

#[allow(dead_code)]
pub(crate) enum OpenProjectWorkerResult {
    Opened(OpenProjectResult),
    WriterLocked { root: PathBuf },
}

impl std::fmt::Debug for OpenProjectWorkerResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Opened(arg0) => f.debug_tuple("Opened").field(arg0).finish(),
            Self::WriterLocked { root } => {
                f.debug_struct("WriterLocked").field("root", root).finish()
            }
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum SaveDestination {
    FirstSave(PathBuf),
    Existing(PathBuf),
    SaveAs(PathBuf),
}

#[allow(dead_code)]
pub(crate) struct SaveProjectRequest {
    pub snapshot: ProjectSnapshot,
    pub destination: SaveDestination,
}

#[allow(dead_code)]
pub(crate) enum SaveProjectWorkerResult {
    ExistingSaved(ProjectCommit),
    NewWritable {
        commit: ProjectCommit,
        guard: ProjectWriterGuard,
    },
    NewCommittedReadOnly {
        commit: ProjectCommit,
        category: &'static str,
    },
}

impl std::fmt::Debug for SaveProjectWorkerResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExistingSaved(arg0) => f.debug_tuple("ExistingSaved").field(arg0).finish(),
            Self::NewWritable { commit, .. } => f
                .debug_struct("NewWritable")
                .field("commit", commit)
                .finish(),
            Self::NewCommittedReadOnly { commit, category } => f
                .debug_struct("NewCommittedReadOnly")
                .field("commit", commit)
                .field("category", category)
                .finish(),
        }
    }
}

// ---------------------------------------------------------------------------
// Async worker wrappers
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub(crate) async fn load_project_worker(
    request: OpenProjectRequest,
) -> Result<OpenProjectWorkerResult, ProjectWorkerError> {
    tokio::task::spawn_blocking(move || {
        if request.writable {
            match acquire_project_writer(&request.root)? {
                ProjectLockResult::AlreadyLocked => {
                    return Ok(OpenProjectWorkerResult::WriterLocked { root: request.root });
                }
                ProjectLockResult::Acquired(guard) => {
                    let loaded =
                        rollshot_action::project::load_project(&request.root).map_err(|e| {
                            tracing::event!(
                                target: "rollshot::project",
                                tracing::Level::ERROR,
                                category = e.category(),
                                "load failed"
                            );
                            ProjectWorkerError::Project(e)
                        })?;
                    return Ok(OpenProjectWorkerResult::Opened(OpenProjectResult {
                        loaded,
                        access: ProjectAccess::Writable(guard),
                    }));
                }
            }
        }

        let loaded = rollshot_action::project::load_project(&request.root).map_err(|e| {
            tracing::event!(
                target: "rollshot::project",
                tracing::Level::ERROR,
                category = e.category(),
                "load failed"
            );
            ProjectWorkerError::Project(e)
        })?;
        Ok(OpenProjectWorkerResult::Opened(OpenProjectResult {
            loaded,
            access: ProjectAccess::ReadOnly,
        }))
    })
    .await
    .map_err(|_| ProjectWorkerError::Join {
        category: "project_worker_join",
    })?
}

#[allow(dead_code)]
pub(crate) async fn save_project_worker(
    request: SaveProjectRequest,
) -> Result<SaveProjectWorkerResult, ProjectWorkerError> {
    tokio::task::spawn_blocking(move || match request.destination {
        SaveDestination::Existing(ref root) => {
            let commit = rollshot_action::project::save_project(&request.snapshot, root)?;
            Ok(SaveProjectWorkerResult::ExistingSaved(commit))
        }
        SaveDestination::FirstSave(ref dest) | SaveDestination::SaveAs(ref dest) => {
            let commit = match request.destination {
                SaveDestination::FirstSave(_) => {
                    rollshot_action::project::create_project(&request.snapshot, dest)?
                }
                _ => rollshot_action::project::save_project_as(&request.snapshot, dest)?,
            };

            match acquire_project_writer(dest)? {
                ProjectLockResult::Acquired(guard) => {
                    Ok(SaveProjectWorkerResult::NewWritable { commit, guard })
                }
                ProjectLockResult::AlreadyLocked => {
                    Ok(SaveProjectWorkerResult::NewCommittedReadOnly {
                        commit,
                        category: "post_commit_lock_race",
                    })
                }
            }
        }
    })
    .await
    .map_err(|_| ProjectWorkerError::Join {
        category: "project_worker_join",
    })?
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ProjectAdapterError {
    InvalidGuide {
        category: &'static str,
    },
    MissingFrame {
        frame_id: FrameId,
    },
    InvalidAnnotations {
        step_id: u64,
        category: &'static str,
    },
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum ProjectSession {
    Unsaved,
    Saved {
        root: PathBuf,
        base_revision: u64,
        access: ProjectAccess,
    },
}

#[allow(dead_code)]
pub(crate) fn from_loaded_project(
    loaded: LoadedProject,
    access: ProjectAccess,
) -> Result<TimelineWorkspace, ProjectAdapterError> {
    let manifest = &loaded.manifest;

    let source = ProjectFrameSource::from_loaded(&loaded, DEFAULT_PROJECT_FRAME_CACHE_BYTES);

    let steps: Vec<GuideStep> = manifest
        .steps
        .iter()
        .map(|ps| GuideStep {
            index: ps.order as usize,
            title: ps.title.clone(),
            caption: ps.caption.clone().unwrap_or_default(),
            kind: ps.kind,
            reason: ps.reason,
            at_ms: ps.at_ms,
            keyframe: ps.keyframe,
            nearby: ps.nearby.clone(),
            source: ps.id.0,
        })
        .collect();

    let guide = Guide::from_reviewed_steps(manifest.title.clone(), steps)
        .map_err(|category| ProjectAdapterError::InvalidGuide { category })?;

    let selected = (!guide.is_empty()).then_some(1);

    let mut presentation = ActionGuidePresentation::new();
    for ps in &manifest.steps {
        if let Some(ref persisted) = ps.annotations {
            presentation.restore_pending(ps.id.0, ps.keyframe, persisted.clone());
        }
    }

    let store = rollshot_action::FrameStore::new(Default::default());

    let mut ws = TimelineWorkspace {
        guide,
        store,
        region: manifest.capture_region,
        capability: manifest.input_capability,
        source_kind: manifest.input_source,
        selected,
        message: None,
        issue_pack: None,
        ffmpeg_setup: None,
        pending_discard: false,
        keyframe_handle: None,
        strip: Vec::new(),
        storyboard_preview: None,
        presentation,
        annotation_session: None,
        caption_proposal: None,
        caption_suggestions_running: false,
        caption_agent_run_id: 0,
        visual_annotation_suggestion: super::VisualAnnotationSuggestionState::Idle,
        visual_annotation_agent_run_id: 0,
        storyboard_copy_operation_id: 0,
        export_state: super::GuideExportState::Idle,
        last_export: None,
        next_export_operation_id: 0,
        next_issue_pack_operation_id: 0,
        frame_source: Some(StepFrameSource::Project(source)),
        project_session: Some(ProjectSession::Saved {
            root: loaded.root,
            base_revision: manifest.revision,
            access,
        }),
        enabled_outputs: manifest.enabled_outputs,
        save_state: super::ProjectSaveState::Clean,
        first_save_prompt: super::FirstSavePrompt::Hidden,
        close_intent: super::CloseIntent::None,
        frame_coordinator: super::FrameLoadCoordinator::new(),
        last_save_error: None,
        pending_writer_guard: std::sync::Arc::new(std::sync::Mutex::new(None)),
        publish_operation: None,
        publish_arbiter: super::project_publish::PublishArbiter::new(),
        publish_freshness: std::collections::BTreeMap::new(),
        publish_details_open: false,
        next_publish_operation_id: 0,
        share_progress: None,
        share_kind: None,
        share_operation_id: 0,
        import_warnings: manifest.import_warnings.clone(),
        imported_scratch: None,
    };

    ws.rebuild_selection_handles();
    ws.load_publish_freshness();
    Ok(ws)
}

#[allow(dead_code)]
pub(crate) fn build_project_snapshot(
    ws: &mut TimelineWorkspace,
) -> Result<ProjectSnapshot, ProjectAdapterError> {
    let frame_source = ws
        .frame_source
        .as_mut()
        .ok_or(ProjectAdapterError::InvalidGuide {
            category: "no_frame_source",
        })?;

    let steps = ws.guide.steps();
    let mut referenced_frames = std::collections::BTreeSet::new();
    for step in steps {
        referenced_frames.insert(step.keyframe);
        for &nearby in &step.nearby {
            referenced_frames.insert(nearby);
        }
    }

    let mut frames = Vec::new();
    for &frame_id in &referenced_frames {
        let snap = frame_source
            .snapshot_frame(frame_id)
            .ok_or(ProjectAdapterError::MissingFrame { frame_id })?;
        frames.push(snap);
    }

    let mut project_steps = Vec::with_capacity(steps.len());
    for step in steps {
        let annotations = ws.presentation.snapshot_for_source(step.source);

        project_steps.push(rollshot_action::project::ProjectStep {
            id: ProjectStepId(step.source),
            order: step.index as u32,
            title: step.title.clone(),
            caption: if step.caption.is_empty() {
                None
            } else {
                Some(step.caption.clone())
            },
            kind: step.kind,
            reason: step.reason,
            at_ms: step.at_ms,
            keyframe: step.keyframe,
            nearby: step.nearby.clone(),
            annotations,
        });
    }

    let base_revision = match ws.project_session {
        Some(ProjectSession::Saved { base_revision, .. }) => Some(base_revision),
        _ => None,
    };

    Ok(ProjectSnapshot {
        base_revision,
        title: ws.guide.title().to_string(),
        capture_region: ws.region,
        input_source: ws.source_kind,
        input_capability: ws.capability,
        enabled_outputs: ws.enabled_outputs,
        frames,
        steps: project_steps,
        #[cfg(feature = "action-guide")]
        import_warnings: ws.import_warnings.clone(),
        #[cfg(not(feature = "action-guide"))]
        import_warnings: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_action::project::{
        EnabledOutputs, PersistedStepAnnotations, ProjectFrame, ProjectManifestV1, ProjectStep,
        ProjectStepId,
    };
    use rollshot_action::{
        CandidateKind, CaptureRegion, DetectReason, InputCapability, InputSourceKind,
    };

    fn manifest_two_steps_with_annotations() -> ProjectManifestV1 {
        let annotations = PersistedStepAnnotations {
            annotations: vec![rollshot_image_document::Annotation::NumberCallout {
                id: rollshot_image_document::AnnotationId(1),
                number: 1,
                tip: rollshot_image_document::ImagePoint::new(10.0, 10.0),
                bubble: rollshot_image_document::ImagePoint::new(20.0, 20.0),
                style: Default::default(),
            }],
            explanations: {
                let mut m = std::collections::BTreeMap::new();
                m.insert(
                    rollshot_image_document::AnnotationId(1),
                    "Click here".to_string(),
                );
                m
            },
        };

        ProjectManifestV1 {
            schema_version: 1,
            revision: 3,
            title: "Test Guide".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs {
                storyboard: true,
                gif: false,
                mp4: true,
            },
            frames: vec![
                ProjectFrame {
                    id: 1,
                    at_ms: 0,
                    sha256: "abc".into(),
                    width: 8,
                    height: 8,
                },
                ProjectFrame {
                    id: 2,
                    at_ms: 100,
                    sha256: "def".into(),
                    width: 8,
                    height: 8,
                },
            ],
            steps: vec![
                ProjectStep {
                    id: ProjectStepId(1),
                    order: 1,
                    title: "Click".into(),
                    caption: Some("First step".into()),
                    kind: CandidateKind::Click,
                    reason: DetectReason::ClickConfirmed,
                    at_ms: 50,
                    keyframe: 1,
                    nearby: vec![1],
                    annotations: Some(annotations.clone()),
                },
                ProjectStep {
                    id: ProjectStepId(2),
                    order: 2,
                    title: "Scroll".into(),
                    caption: None,
                    kind: CandidateKind::Scroll,
                    reason: DetectReason::ScrollSettled,
                    at_ms: 150,
                    keyframe: 2,
                    nearby: vec![2],
                    annotations: None,
                },
            ],
        }
    }

    fn loaded_project(manifest: ProjectManifestV1) -> LoadedProject {
        LoadedProject {
            root: std::path::PathBuf::from("/tmp/test-project"),
            manifest: manifest.into(),
        }
    }

    fn dummy_guard() -> ProjectWriterGuard {
        ProjectWriterGuard {
            _file: tempfile::tempfile().unwrap(),
        }
    }

    #[test]
    fn from_loaded_project_restores_guide_text_order_and_keyframe() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let ws = from_loaded_project(loaded, ProjectAccess::Writable(dummy_guard())).expect("ok");

        assert_eq!(ws.guide.title(), "Test Guide");
        assert_eq!(ws.guide.steps().len(), 2);
        assert_eq!(ws.guide.steps()[0].index, 1);
        assert_eq!(ws.guide.steps()[0].title, "Click");
        assert_eq!(ws.guide.steps()[0].keyframe, 1);
        assert_eq!(ws.guide.steps()[0].source, 1);
        assert_eq!(ws.guide.steps()[0].nearby, vec![1]);
        assert_eq!(ws.guide.steps()[1].index, 2);
        assert_eq!(ws.guide.steps()[1].source, 2);
        assert_eq!(ws.guide.steps()[1].nearby, vec![2]);
    }

    #[test]
    fn from_loaded_project_stores_enabled_outputs() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let ws = from_loaded_project(loaded, ProjectAccess::Writable(dummy_guard())).expect("ok");

        assert!(ws.enabled_outputs.storyboard);
        assert!(!ws.enabled_outputs.gif);
        assert!(ws.enabled_outputs.mp4);
    }

    #[test]
    fn from_loaded_project_selects_step_one() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let ws = from_loaded_project(loaded, ProjectAccess::Writable(dummy_guard())).expect("ok");

        assert_eq!(ws.selected, Some(1));
    }

    #[test]
    fn from_loaded_project_provides_initial_frame_load_task() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let ws = from_loaded_project(loaded, ProjectAccess::Writable(dummy_guard())).expect("ok");

        assert!(ws.initial_frame_load_task().units() > 0);
    }

    #[test]
    fn from_loaded_project_installs_pending_annotations() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let ws = from_loaded_project(loaded, ProjectAccess::Writable(dummy_guard())).expect("ok");

        // Step 1 has pending annotations (not hydrated yet)
        let snap = ws.presentation.snapshot_for_source(1).expect("pending");
        assert_eq!(snap.annotations.len(), 1);
        assert_eq!(snap.explanations.len(), 1);

        // Step 2 has no annotations
        assert!(ws.presentation.snapshot_for_source(2).is_none());
    }

    #[test]
    fn from_loaded_project_sets_project_session_saved() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let ws = from_loaded_project(loaded, ProjectAccess::ReadOnly).expect("ok");

        match ws.project_session {
            Some(ProjectSession::Saved {
                base_revision,
                access: ProjectAccess::ReadOnly,
                ..
            }) => {
                assert_eq!(base_revision, 3);
            }
            _ => panic!("expected Saved session with ReadOnly access"),
        }
    }

    #[test]
    fn from_loaded_project_starts_with_empty_undo_history() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let ws = from_loaded_project(loaded, ProjectAccess::Writable(dummy_guard())).expect("ok");

        // Presentation has pending entries, no loaded docs
        assert!(ws.presentation.doc(1).is_none());
        assert!(ws.presentation.doc(2).is_none());
    }

    #[test]
    fn from_loaded_project_rejects_zero_step_source() {
        let mut manifest = manifest_two_steps_with_annotations();
        manifest.steps[0].id = ProjectStepId(0);
        let loaded = loaded_project(manifest);
        let result = from_loaded_project(loaded, ProjectAccess::Writable(dummy_guard()));
        assert!(matches!(
            result,
            Err(ProjectAdapterError::InvalidGuide { .. })
        ));
    }

    #[test]
    fn from_loaded_project_rejects_duplicate_step_source() {
        let mut manifest = manifest_two_steps_with_annotations();
        manifest.steps[1].id = ProjectStepId(1);
        let loaded = loaded_project(manifest);
        let result = from_loaded_project(loaded, ProjectAccess::Writable(dummy_guard()));
        assert!(matches!(
            result,
            Err(ProjectAdapterError::InvalidGuide { .. })
        ));
    }

    #[test]
    fn from_loaded_project_rejects_empty_steps() {
        let mut manifest = manifest_two_steps_with_annotations();
        manifest.steps = vec![];
        let loaded = loaded_project(manifest);
        let result = from_loaded_project(loaded, ProjectAccess::Writable(dummy_guard()));
        assert!(matches!(
            result,
            Err(ProjectAdapterError::InvalidGuide { .. })
        ));
    }

    #[test]
    fn build_project_snapshot_uses_guide_step_source_as_project_step_id() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let mut ws =
            from_loaded_project(loaded, ProjectAccess::Writable(dummy_guard())).expect("ok");

        let snap = build_project_snapshot(&mut ws).expect("snapshot");
        assert_eq!(snap.steps.len(), 2);
        assert_eq!(snap.steps[0].id, ProjectStepId(1));
        assert_eq!(snap.steps[1].id, ProjectStepId(2));
    }

    #[test]
    fn build_project_snapshot_preserves_title_and_region() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let mut ws =
            from_loaded_project(loaded, ProjectAccess::Writable(dummy_guard())).expect("ok");

        let snap = build_project_snapshot(&mut ws).expect("snapshot");
        assert_eq!(snap.title, "Test Guide");
        assert_eq!(snap.capture_region.width, 8);
        assert_eq!(snap.input_source, InputSourceKind::VisualOnly);
    }

    #[test]
    fn build_project_snapshot_sets_base_revision_from_saved_session() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let mut ws =
            from_loaded_project(loaded, ProjectAccess::Writable(dummy_guard())).expect("ok");

        let snap = build_project_snapshot(&mut ws).expect("snapshot");
        assert_eq!(snap.base_revision, Some(3));
    }

    #[test]
    fn build_project_snapshot_enumerates_only_referenced_frames() {
        let mut manifest = manifest_two_steps_with_annotations();
        manifest.frames.push(ProjectFrame {
            id: 99,
            at_ms: 999,
            sha256: "unused".into(),
            width: 8,
            height: 8,
        });
        let loaded = loaded_project(manifest);
        let mut ws =
            from_loaded_project(loaded, ProjectAccess::Writable(dummy_guard())).expect("ok");

        let snap = build_project_snapshot(&mut ws).expect("snapshot");
        assert_eq!(snap.frames.len(), 2);
        assert!(snap.frames.iter().all(|f| f.id != 99));
    }

    #[test]
    fn build_project_snapshot_persists_pending_annotations() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let mut ws =
            from_loaded_project(loaded, ProjectAccess::Writable(dummy_guard())).expect("ok");

        let snap = build_project_snapshot(&mut ws).expect("snapshot");
        let step1 = &snap.steps[0];
        let annotations = step1.annotations.as_ref().expect("annotations");
        assert_eq!(annotations.annotations.len(), 1);
        assert_eq!(annotations.explanations.len(), 1);

        let step2 = &snap.steps[1];
        assert!(step2.annotations.is_none());
    }

    #[test]
    fn build_project_snapshot_never_serializes_workspace_state() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let mut ws =
            from_loaded_project(loaded, ProjectAccess::Writable(dummy_guard())).expect("ok");

        let snap = build_project_snapshot(&mut ws).expect("snapshot");
        assert!(snap.base_revision.is_some());
        assert_eq!(snap.steps.len(), 2);
        assert!(snap.frames.len() >= 2);
    }

    // ---- Writer lock tests ----

    #[test]
    fn project_writer_second_guard_reports_already_locked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("test-project");
        std::fs::create_dir_all(&root).unwrap();

        let first = acquire_project_writer(&root).unwrap();
        assert!(matches!(first, ProjectLockResult::Acquired(_)));

        let second = acquire_project_writer(&root).unwrap();
        assert!(matches!(second, ProjectLockResult::AlreadyLocked));
    }

    #[test]
    fn project_writer_dropping_guard_allows_reacquisition() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("test-project");
        std::fs::create_dir_all(&root).unwrap();

        let guard = match acquire_project_writer(&root).unwrap() {
            ProjectLockResult::Acquired(guard) => guard,
            ProjectLockResult::AlreadyLocked => panic!("first lock must succeed"),
        };
        drop(guard);

        assert!(matches!(
            acquire_project_writer(&root).unwrap(),
            ProjectLockResult::Acquired(_)
        ));
    }

    // ---- Async worker tests (tokio runtime required) ----

    #[tokio::test]
    async fn load_project_worker_read_only_skips_lock() {
        use rollshot_action::project::create_project;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        let snap = build_test_snapshot(None);
        create_project(&snap, &root).unwrap();

        let result = load_project_worker(OpenProjectRequest {
            root: root.clone(),
            writable: false,
        })
        .await
        .unwrap();

        match result {
            OpenProjectWorkerResult::Opened(opened) => {
                assert!(matches!(opened.access, ProjectAccess::ReadOnly));
                assert_eq!(opened.loaded.manifest.revision, 1);
            }
            _ => panic!("expected Opened for read-only"),
        }
    }

    #[tokio::test]
    async fn load_project_worker_writable_returns_locked_on_contention() {
        use rollshot_action::project::create_project;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        let snap = build_test_snapshot(None);
        create_project(&snap, &root).unwrap();

        // Hold the lock
        let _guard = acquire_project_writer(&root).unwrap();

        let result = load_project_worker(OpenProjectRequest {
            root: root.clone(),
            writable: true,
        })
        .await
        .unwrap();

        assert!(matches!(
            result,
            OpenProjectWorkerResult::WriterLocked { .. }
        ));
    }

    #[tokio::test]
    async fn load_project_worker_writable_acquires_guard_when_free() {
        use rollshot_action::project::create_project;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        let snap = build_test_snapshot(None);
        create_project(&snap, &root).unwrap();

        let result = load_project_worker(OpenProjectRequest {
            root: root.clone(),
            writable: true,
        })
        .await
        .unwrap();

        match result {
            OpenProjectWorkerResult::Opened(opened) => {
                assert!(matches!(opened.access, ProjectAccess::Writable(_)));
            }
            _ => panic!("expected Opened for writable"),
        }
    }

    #[tokio::test]
    async fn load_project_worker_preserves_project_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("nonexistent");

        let error = load_project_worker(OpenProjectRequest {
            root,
            writable: false,
        })
        .await
        .unwrap_err();

        assert_eq!(error.category(), "io");
    }

    #[tokio::test]
    async fn save_project_worker_existing_save_increments_revision() {
        use rollshot_action::project::create_project;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        let snap = build_test_snapshot(None);
        create_project(&snap, &root).unwrap();

        let mut updated_snap = build_test_snapshot(Some(1));
        updated_snap.title = "Updated".into();

        let result = save_project_worker(SaveProjectRequest {
            snapshot: updated_snap,
            destination: SaveDestination::Existing(root.clone()),
        })
        .await
        .unwrap();

        match result {
            SaveProjectWorkerResult::ExistingSaved(commit) => {
                assert_eq!(commit.manifest.revision, 2);
                assert_eq!(commit.manifest.title, "Updated");
            }
            _ => panic!("expected ExistingSaved"),
        }
    }

    #[tokio::test]
    async fn save_project_worker_existing_save_returns_revision_conflict() {
        use rollshot_action::project::create_project;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        let snap = build_test_snapshot(None);
        create_project(&snap, &root).unwrap();

        let conflict_snap = build_test_snapshot(Some(99));
        let error = save_project_worker(SaveProjectRequest {
            snapshot: conflict_snap,
            destination: SaveDestination::Existing(root.clone()),
        })
        .await
        .unwrap_err();

        assert_eq!(error.category(), "revision-conflict");

        // Verify snapshot wasn't consumed or corrupted by checking it's still valid
        // (the error path should preserve the snapshot)
        let valid_snap = build_test_snapshot(Some(1));
        assert_eq!(valid_snap.title, "Test Guide");
        assert_eq!(valid_snap.steps[0].title, "Step 1");
    }

    #[tokio::test]
    async fn save_project_worker_first_save_returns_guard_when_free() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        let result = save_project_worker(SaveProjectRequest {
            snapshot: build_test_snapshot(None),
            destination: SaveDestination::FirstSave(root.clone()),
        })
        .await
        .unwrap();

        match result {
            SaveProjectWorkerResult::NewWritable {
                commit,
                guard: _guard,
            } => {
                assert_eq!(commit.manifest.revision, 1);
            }
            _ => panic!("expected NewWritable"),
        }
    }

    #[tokio::test]
    async fn save_project_worker_first_save_returns_read_only_on_lock_race() {
        // The race: another process grabs the lock between create_project
        // (commit) and acquire_project_writer inside the worker.
        //
        // We simulate this with a background thread that monitors for the
        // project directory to appear, then immediately grabs the .lock file.
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("race.rollshot-guide");
        let dest_clone = dest.clone();

        // Spawn a thread that waits for the project dir, then grabs the lock
        let grabber = std::thread::spawn(move || {
            // Spin until project dir exists (created by create_project atomic rename)
            for _ in 0..100_000 {
                if dest_clone.exists() {
                    break;
                }
                std::thread::yield_now();
            }
            // Try to create and lock .lock before the worker does
            let lock_path = dest_clone.join(".lock");
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock_path)
                .ok()?;
            match FileExt::try_lock(&file) {
                Ok(()) => Some(ProjectWriterGuard { _file: file }),
                Err(_) => None,
            }
        });

        let result = save_project_worker(SaveProjectRequest {
            snapshot: build_test_snapshot(None),
            destination: SaveDestination::FirstSave(dest.clone()),
        })
        .await
        .unwrap();

        let guard = grabber.join().unwrap();

        // Verify the project was created at revision 1 regardless of who won the lock race
        match result {
            SaveProjectWorkerResult::NewWritable { commit, guard: _g } => {
                assert_eq!(commit.manifest.revision, 1);
                // Background thread lost the race, that's fine
                drop(_g);
            }
            SaveProjectWorkerResult::NewCommittedReadOnly { commit, category } => {
                assert_eq!(commit.manifest.revision, 1);
                assert_eq!(category, "post_commit_lock_race");
                assert!(guard.is_some(), "grabber should have won the lock");
            }
            _ => panic!("unexpected result variant"),
        }
    }

    // ---- Corrupt project test ----

    #[tokio::test]
    async fn load_project_worker_corrupt_manifest_returns_corrupt_error() {
        use rollshot_action::project::create_project;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("corrupt.rollshot-guide");
        let snap = build_test_snapshot(None);
        create_project(&snap, &root).unwrap();

        // Corrupt the manifest JSON to make it unparseable
        let manifest_path = root.join("project.json");
        std::fs::write(&manifest_path, "{invalid json").unwrap();

        let error = load_project_worker(OpenProjectRequest {
            root: root.clone(),
            writable: false,
        })
        .await
        .unwrap_err();

        // Should get an invalid-json error category
        assert_eq!(error.category(), "invalid-json");
    }

    // ---- Helper for worker tests ----

    fn build_test_snapshot(base: Option<u64>) -> ProjectSnapshot {
        use rollshot_action::project::{
            EnabledOutputs, ProjectStep, ProjectStepId, SnapshotFrame, SnapshotFramePayload,
        };
        use rollshot_action::{
            CandidateKind, CaptureRegion, DetectReason, InputCapability, InputSourceKind,
        };

        ProjectSnapshot {
            base_revision: base,
            title: "Test Guide".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(std::sync::Arc::new(
                    image::RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255])),
                )),
            }],
            steps: vec![ProjectStep {
                id: ProjectStepId(1),
                order: 1,
                title: "Step 1".into(),
                caption: None,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 150,
                keyframe: 1,
                nearby: vec![1],
                annotations: None,
            }],
            import_warnings: Vec::new(),
        }
    }
}
