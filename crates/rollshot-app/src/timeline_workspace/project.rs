use rollshot_action::project::{LoadedProject, ProjectSnapshot, ProjectStepId};
use rollshot_action::{
    FrameId, Guide, GuideStep, ProjectFrameSource, StepFrameSource,
    DEFAULT_PROJECT_FRAME_CACHE_BYTES,
};

use super::annotation::ActionGuidePresentation;
use super::TimelineWorkspace;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ProjectOpenMode {
    Writable,
    ReadOnly,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum ProjectSession {
    Unsaved,
    Saved {
        root: std::path::PathBuf,
        base_revision: u64,
        open_mode: ProjectOpenMode,
    },
}

#[allow(dead_code)]
pub(crate) fn from_loaded_project(
    loaded: LoadedProject,
    open_mode: ProjectOpenMode,
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
            open_mode,
        }),
        enabled_outputs: manifest.enabled_outputs,
    };

    ws.rebuild_selection_handles();
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
            manifest,
        }
    }

    #[test]
    fn from_loaded_project_restores_guide_text_order_and_keyframe() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let ws = from_loaded_project(loaded, ProjectOpenMode::Writable).expect("ok");

        assert_eq!(ws.guide.title(), "Test Guide");
        assert_eq!(ws.guide.steps().len(), 2);
        assert_eq!(ws.guide.steps()[0].index, 1);
        assert_eq!(ws.guide.steps()[0].title, "Click");
        assert_eq!(ws.guide.steps()[0].keyframe, 1);
        assert_eq!(ws.guide.steps()[0].source, 1);
        assert_eq!(ws.guide.steps()[1].index, 2);
        assert_eq!(ws.guide.steps()[1].source, 2);
    }

    #[test]
    fn from_loaded_project_stores_enabled_outputs() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let ws = from_loaded_project(loaded, ProjectOpenMode::Writable).expect("ok");

        assert!(ws.enabled_outputs.storyboard);
        assert!(!ws.enabled_outputs.gif);
        assert!(ws.enabled_outputs.mp4);
    }

    #[test]
    fn from_loaded_project_selects_step_one() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let ws = from_loaded_project(loaded, ProjectOpenMode::Writable).expect("ok");

        assert_eq!(ws.selected, Some(1));
    }

    #[test]
    fn from_loaded_project_installs_pending_annotations() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let ws = from_loaded_project(loaded, ProjectOpenMode::Writable).expect("ok");

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
        let ws = from_loaded_project(loaded, ProjectOpenMode::ReadOnly).expect("ok");

        match ws.project_session {
            Some(ProjectSession::Saved {
                base_revision,
                open_mode,
                ..
            }) => {
                assert_eq!(base_revision, 3);
                assert_eq!(open_mode, ProjectOpenMode::ReadOnly);
            }
            _ => panic!("expected Saved session"),
        }
    }

    #[test]
    fn from_loaded_project_starts_with_empty_undo_history() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let ws = from_loaded_project(loaded, ProjectOpenMode::Writable).expect("ok");

        // Presentation has pending entries, no loaded docs
        assert!(ws.presentation.doc(1).is_none());
        assert!(ws.presentation.doc(2).is_none());
    }

    #[test]
    fn from_loaded_project_rejects_zero_step_source() {
        let mut manifest = manifest_two_steps_with_annotations();
        manifest.steps[0].id = ProjectStepId(0);
        let loaded = loaded_project(manifest);
        let result = from_loaded_project(loaded, ProjectOpenMode::Writable);
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
        let result = from_loaded_project(loaded, ProjectOpenMode::Writable);
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
        let result = from_loaded_project(loaded, ProjectOpenMode::Writable);
        assert!(matches!(
            result,
            Err(ProjectAdapterError::InvalidGuide { .. })
        ));
    }

    #[test]
    fn build_project_snapshot_uses_guide_step_source_as_project_step_id() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let mut ws = from_loaded_project(loaded, ProjectOpenMode::Writable).expect("ok");

        let snap = build_project_snapshot(&mut ws).expect("snapshot");
        assert_eq!(snap.steps.len(), 2);
        assert_eq!(snap.steps[0].id, ProjectStepId(1));
        assert_eq!(snap.steps[1].id, ProjectStepId(2));
    }

    #[test]
    fn build_project_snapshot_preserves_title_and_region() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let mut ws = from_loaded_project(loaded, ProjectOpenMode::Writable).expect("ok");

        let snap = build_project_snapshot(&mut ws).expect("snapshot");
        assert_eq!(snap.title, "Test Guide");
        assert_eq!(snap.capture_region.width, 8);
        assert_eq!(snap.input_source, InputSourceKind::VisualOnly);
    }

    #[test]
    fn build_project_snapshot_sets_base_revision_from_saved_session() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let mut ws = from_loaded_project(loaded, ProjectOpenMode::Writable).expect("ok");

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
        let mut ws = from_loaded_project(loaded, ProjectOpenMode::Writable).expect("ok");

        let snap = build_project_snapshot(&mut ws).expect("snapshot");
        assert_eq!(snap.frames.len(), 2);
        assert!(snap.frames.iter().all(|f| f.id != 99));
    }

    #[test]
    fn build_project_snapshot_persists_pending_annotations() {
        let manifest = manifest_two_steps_with_annotations();
        let loaded = loaded_project(manifest);
        let mut ws = from_loaded_project(loaded, ProjectOpenMode::Writable).expect("ok");

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
        let mut ws = from_loaded_project(loaded, ProjectOpenMode::Writable).expect("ok");

        let snap = build_project_snapshot(&mut ws).expect("snapshot");
        assert!(snap.base_revision.is_some());
        assert_eq!(snap.steps.len(), 2);
        assert!(snap.frames.len() >= 2);
    }
}
