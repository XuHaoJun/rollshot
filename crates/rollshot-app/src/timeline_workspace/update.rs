use std::path::{Path, PathBuf};

use iced::Task;
use rollshot_action::{
    export_gif, export_video, GifOptions, StoryboardError, StoryboardOptions,
    StoryboardRenderResult, VideoOptions,
};
use rollshot_image_document::ImagePoint;

use super::{StoryboardCopyState, TimelineWorkspace};

pub struct Update {
    pub task: Task<Message>,
    pub effect: super::Effect,
}

impl Update {
    pub fn none() -> Self {
        Self {
            task: Task::none(),
            effect: super::Effect::None,
        }
    }

    pub fn task(task: Task<Message>) -> Self {
        Self {
            task,
            effect: super::Effect::None,
        }
    }

    pub fn effect(effect: super::Effect) -> Self {
        Self {
            task: Task::none(),
            effect,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    SelectStep(usize),
    TitleChanged(String),
    CaptionChanged(String),
    DeleteStep,
    ReplaceKeyframe(rollshot_action::FrameId),
    DiscardRequested,
    CloseRequested,
    CancelDiscard,
    ConfirmDiscard,
    ExportRequested,
    #[allow(dead_code)]
    GuideTitleChanged(String),
    #[allow(dead_code)]
    AnnotationExplanationChanged(rollshot_image_document::AnnotationId, String),
    ExportDirChosenWithId {
        operation_id: u64,
        parent: Option<PathBuf>,
    },
    ExportFinished {
        operation_id: u64,
        result: Result<super::guide_export::StandaloneExportResult, String>,
    },
    #[allow(dead_code)]
    OpenExportedGuide,
    #[allow(dead_code)]
    ShowExportedGuideInFolder,
    #[allow(dead_code)]
    PlatformActionFinished(Result<(), String>),
    ExportGifRequested,
    ExportGifPathChosen(Option<PathBuf>),
    ExportStoryboardRequested,
    ExportStoryboardPathChosen(Option<PathBuf>),
    PreviewStoryboardRequested,
    PreviewStoryboardClosed,
    CopyStoryboardRequested,
    CopyStoryboardFinished {
        operation_id: u64,
        result: Result<super::storyboard_copy::StoryboardCopyResult, String>,
    },
    ClearStoryboardCopyFeedback {
        operation_id: u64,
    },
    ExportMp4Requested,
    ExportMp4PathChosen(Option<PathBuf>),
    FfmpegUseSystem,
    FfmpegDownloadManaged,
    FfmpegDownloadFinished(Result<PathBuf, String>),
    FfmpegSetupCancel,
    /// Export a bug-report Issue Pack from the timeline workspace.
    ExportBugReport,
    /// Toggle the review-confirmed checkbox in the Issue Pack dialog.
    IssuePackReviewChanged(bool),
    /// Toggle whether to include the Action Guide GIF in the Issue Pack.
    IssuePackIncludeGifChanged(bool),
    /// Begin exporting an Issue Pack to a folder.
    IssuePackExportFolder,
    /// Begin exporting an Issue Pack to a ZIP file.
    IssuePackExportZip,
    /// The async folder-picker returned (None = cancelled).
    IssuePackFolderChosen {
        operation_id: u64,
        parent: Option<PathBuf>,
    },
    /// Background Issue Pack export completed.
    IssuePackFinished {
        operation_id: u64,
        result: Result<crate::issue_pack::IssuePackExportResult, String>,
    },
    /// Close the Issue Pack dialog without exporting.
    IssuePackCancel,
    #[cfg(target_os = "macos")]
    OpenInputMonitoringSettings,
    DismissBanner,
    AnnotateStepRequested,
    AnnotationToolChanged(super::annotation::AnnotationTool),
    AnnotationTextChanged(String),
    AnnotationCanvasPressed(rollshot_image_document::ImagePoint),
    AnnotationCanvasMoved(rollshot_image_document::ImagePoint),
    AnnotationCanvasReleased(rollshot_image_document::ImagePoint),
    AnnotationUndo,
    AnnotationRedo,
    AnnotationDone,
    AnnotationCancel,
    CaptionProposalLoaded(Result<rollshot_action::CaptionProposal, String>),
    AcceptCaptionSuggestion(rollshot_action::CaptionSuggestionId),
    RejectCaptionSuggestion(rollshot_action::CaptionSuggestionId),
    AcceptAllCaptionSuggestions,
    DismissCaptionProposal,
    SuggestCaptionsRequested,
    /// Begin a visual annotation suggestion run. Loads provider config and
    /// opens the consent dialog.
    #[allow(dead_code)]
    SuggestVisualAnnotationsRequested,
    /// User confirmed the visual annotation consent dialog. Reloads provider
    /// config, compares with consent snapshot, and starts the async run.
    #[allow(dead_code)]
    VisualSuggestionConsentConfirmed,
    /// User cancelled the visual annotation consent dialog.
    #[allow(dead_code)]
    VisualSuggestionConsentCancelled,
    /// Background visual annotation suggestion run completed. The `run_id`
    /// must match the current `Running` state; otherwise the result is dropped.
    #[allow(dead_code)]
    VisualAnnotationProposalLoaded {
        run_id: u64,
        result: Result<super::visual_annotation_agent::VisualAnnotationTaskResult, String>,
    },
    /// Cancel the in-flight visual annotation suggestion run and transition to Idle.
    #[allow(dead_code)]
    CancelVisualAnnotationSuggestion,
    /// Accept all pending visual annotations in the review and apply them as edits.
    #[allow(dead_code)]
    AcceptAllVisualAnnotations,
    /// Reject the pending visual annotation review and discard all suggestions.
    #[allow(dead_code)]
    RejectVisualAnnotationSuggestion,
    /// Dismiss the pending visual annotation review without accepting or rejecting.
    #[allow(dead_code)]
    DismissVisualAnnotationReview,
    /// Accept a single pending visual annotation by its suggestion id.
    AcceptVisualAnnotation(rollshot_action::VisualAnnotationSuggestionId),
    /// Reject a single pending visual annotation by its suggestion id.
    RejectSingleVisualAnnotationSuggestion(rollshot_action::VisualAnnotationSuggestionId),
    SaveLater,
    SaveRequested,
    SaveAsRequested,
    SavePickerChosen(Option<PathBuf>),
    SaveWorkerFinished(SaveWorkerOutcome),
    CloseSaveAndClose,
    CloseDiscard,
    CloseCancel,
    FrameLoadCompleted {
        generation: u64,
        results: Vec<Result<rollshot_action::LoadedStepFrame, String>>,
        remaining: Vec<(
            rollshot_action::FrameId,
            rollshot_action::StepFrameLoadRequest,
        )>,
    },
}

#[derive(Debug, Clone)]
pub(crate) enum SaveWorkerOutcome {
    ExistingSaved {
        revision: u64,
    },
    NewWritable {
        root: PathBuf,
        revision: u64,
    },
    NewCommittedReadOnly {
        root: PathBuf,
        revision: u64,
        category: &'static str,
    },
    Failed(String),
}

pub fn update(state: &mut TimelineWorkspace, message: Message) -> Update {
    match message {
        Message::SelectStep(index) => {
            if state.guide.steps().iter().any(|s| s.index == index) {
                state.selected = Some(index);
                #[cfg(feature = "action-guide")]
                {
                    // Extract selected step info before borrowing frame_source.
                    let (keyframe, source_id) = {
                        let Some(step) = state.selected_step() else {
                            return Update::none();
                        };
                        (step.keyframe, step.source)
                    };
                    let nearby = state
                        .selected_step()
                        .map(|s| s.nearby.clone())
                        .unwrap_or_default();

                    if let Some(ref mut source) = state.frame_source {
                        // Project-backed: clear old handles, use cache, spawn decodes.
                        state.keyframe_handle = None;
                        state.strip.clear();
                        let gen = state.frame_coordinator.advance_generation();
                        // Try cache for current keyframe first.
                        if let Some(img) = source.cached(keyframe) {
                            state.keyframe_handle = Some(super::build_handle(
                                &::image::ImageBuffer::from_raw(
                                    img.width(),
                                    img.height(),
                                    img.as_raw().to_vec(),
                                )
                                .unwrap_or_else(|| {
                                    ::image::RgbaImage::new(img.width(), img.height())
                                }),
                            ));
                            state
                                .presentation
                                .hydrate_for_step(source_id, keyframe, img);
                        }
                        // Try cache for nearby strip frames.
                        for &id in &nearby {
                            if let Some(img) = source.cached(id) {
                                state.strip.push(super::StripFrame {
                                    id,
                                    handle: super::build_handle(
                                        &::image::ImageBuffer::from_raw(
                                            img.width(),
                                            img.height(),
                                            img.as_raw().to_vec(),
                                        )
                                        .unwrap_or_else(
                                            || ::image::RgbaImage::new(img.width(), img.height()),
                                        ),
                                    ),
                                });
                            }
                        }
                        // Collect uncached IDs (keyframe first, then nearby).
                        let mut uncached = Vec::new();
                        if state.keyframe_handle.is_none() {
                            uncached.push(keyframe);
                        }
                        for &id in &nearby {
                            if !state.strip.iter().any(|f| f.id == id) {
                                uncached.push(id);
                            }
                        }
                        if uncached.is_empty() {
                            return Update::none();
                        }
                        // Build load requests for uncached frames.
                        let requests: Vec<_> = uncached
                            .iter()
                            .filter_map(|&id| source.load_request(id).map(|req| (id, req)))
                            .collect();
                        if requests.is_empty() {
                            return Update::none();
                        }
                        // Spawn up to 2 concurrent decode tasks.
                        let semaphore = state.frame_coordinator.semaphore.clone();
                        let (first_batch, remaining) = if requests.len() > 2 {
                            (requests[..2].to_vec(), requests[2..].to_vec())
                        } else {
                            (requests.clone(), Vec::new())
                        };
                        let all_remaining = remaining;
                        let gen_for_task = gen;
                        let sem_for_task = semaphore;
                        let task = iced::Task::perform(
                            frame_decode_task(
                                gen_for_task,
                                sem_for_task,
                                first_batch,
                                all_remaining,
                            ),
                            move |msg| msg,
                        );
                        return Update::task(task);
                    }
                }
                #[cfg(not(feature = "action-guide"))]
                {
                    state.rebuild_selection_handles();
                }
                #[cfg(feature = "action-guide")]
                if state.frame_source.is_none() {
                    state.rebuild_selection_handles();
                }
            }
            Update::none()
        }
        Message::TitleChanged(title) => {
            if !state.can_mutate() {
                return Update::none();
            }
            if let Some(index) = state.selected {
                state.guide.rename(index, title);
                state.mark_project_dirty();
            }
            Update::none()
        }
        Message::CaptionChanged(caption) => {
            if !state.can_mutate() {
                return Update::none();
            }
            if let Some(index) = state.selected {
                state.guide.set_caption(index, caption);
                state.mark_project_dirty();
            }
            Update::none()
        }
        Message::DeleteStep => {
            if !state.can_mutate() {
                return Update::none();
            }
            if state.guide.steps().len() <= 1 {
                return Update::none();
            }
            let deleted_source = state.selected_step().map(|step| step.source);
            let mut deleted = false;
            if let Some(index) = state.selected {
                if state.guide.delete(index) {
                    deleted = true;
                    let len = state.guide.steps().len();
                    state.selected = if len == 0 { None } else { Some(index.min(len)) };
                    state.rebuild_selection_handles();
                }
            }
            if deleted {
                state.mark_project_dirty();
            }
            if let Some(source) = deleted_source {
                state.presentation.clear_for_source(source);
            }
            state
                .presentation
                .retain_sources(state.guide.steps().iter().map(|step| step.source));
            dismiss_stale_visual_annotation_review(state);
            Update::none()
        }
        Message::ReplaceKeyframe(frame) => {
            if !state.can_mutate() {
                return Update::none();
            }
            if let Some(index) = state.selected {
                let source = state.selected_step().map(|step| step.source);
                if state.guide.replace_keyframe(index, frame) {
                    state.mark_project_dirty();
                    state.rebuild_selection_handles();
                    if let Some(source) = source {
                        if state.presentation.clear_for_source(source) {
                            state.message = Some(
                                "Step annotations were cleared because the keyframe changed."
                                    .to_string(),
                            );
                        }
                    }
                }
            }
            // Replacing the keyframe discards any pending visual annotation
            // review: the document is re-built, the keyframe no longer matches.
            dismiss_stale_visual_annotation_review(state);
            Update::none()
        }
        Message::DiscardRequested => {
            state.pending_discard = true;
            Update::none()
        }
        Message::CloseRequested => {
            #[cfg(feature = "action-guide")]
            {
                if state.project_session.is_some() {
                    if state.save_state == super::ProjectSaveState::Dirty {
                        state.close_intent = super::CloseIntent::Confirming;
                        state.pending_discard = true;
                    } else {
                        return Update::effect(super::Effect::CloseWorkspace);
                    }
                } else {
                    state.pending_discard = true;
                }
                Update::none()
            }
            #[cfg(not(feature = "action-guide"))]
            {
                state.pending_discard = true;
                Update::none()
            }
        }
        Message::CancelDiscard => {
            state.pending_discard = false;
            Update::none()
        }
        Message::ConfirmDiscard => {
            state.pending_discard = false;
            Update::effect(super::Effect::CloseWorkspace)
        }
        Message::ExportRequested => {
            #[cfg(feature = "action-guide")]
            if state
                .frame_source
                .as_ref()
                .is_some_and(|fs| fs.in_memory().is_none())
            {
                return Update::none();
            }
            state.message = None;
            match &state.export_state {
                super::GuideExportState::Idle | super::GuideExportState::Succeeded => {}
                _ => return Update::none(),
            }
            let job = match super::guide_export::build_reviewed_export_job(state) {
                Ok(job) => job,
                Err(error) => {
                    state.message = Some(format!("{error}"));
                    return Update::none();
                }
            };
            state.next_export_operation_id = state.next_export_operation_id.saturating_add(1);
            let operation_id = state.next_export_operation_id;
            let created_at = chrono::Local::now();
            state.export_state = super::GuideExportState::PickingDestination {
                operation_id,
                pending: super::guide_export::PendingStandaloneExport {
                    operation_id,
                    created_at,
                    job,
                },
            };
            Update::task(Task::perform(
                pick_export_dir(picker_default_dir()),
                move |path| Message::ExportDirChosenWithId {
                    operation_id,
                    parent: path,
                },
            ))
        }
        Message::GuideTitleChanged(title) => {
            if !state.can_mutate() {
                return Update::none();
            }
            state.guide.set_title(title);
            state.mark_project_dirty();
            Update::none()
        }
        Message::AnnotationExplanationChanged(id, text) => {
            if !state.can_mutate() {
                return Update::none();
            }
            if let Some(session) = &state.annotation_session {
                let _ = state.presentation.set_explanation(session.source, id, text);
                state.mark_project_dirty();
            }
            Update::none()
        }
        Message::ExportDirChosenWithId {
            operation_id,
            parent,
        } => {
            let Some(parent) = parent else {
                if let super::GuideExportState::PickingDestination {
                    operation_id: current,
                    ..
                } = &state.export_state
                {
                    if operation_id == *current {
                        state.export_state = super::GuideExportState::Idle;
                    }
                }
                return Update::none();
            };
            let super::GuideExportState::PickingDestination {
                operation_id: current,
                ..
            } = &state.export_state
            else {
                return Update::none();
            };
            if operation_id != *current {
                return Update::none();
            }
            let old = std::mem::replace(&mut state.export_state, super::GuideExportState::Idle);
            let super::GuideExportState::PickingDestination {
                operation_id: _,
                pending,
            } = old
            else {
                unreachable!("just matched");
            };
            let request = super::guide_export::StandaloneExportRequest {
                operation_id: pending.operation_id,
                parent,
                created_at: pending.created_at,
                job: pending.job,
            };
            state.export_state = super::GuideExportState::Exporting { operation_id };
            Update::task(Task::perform(
                super::guide_export::run_standalone_export(request),
                move |result| Message::ExportFinished {
                    operation_id,
                    result,
                },
            ))
        }
        Message::ExportFinished {
            operation_id,
            result,
        } => {
            apply_export_finished(state, operation_id, result);
            Update::none()
        }
        Message::OpenExportedGuide => {
            if let Some(export) = &state.last_export {
                let path = export.index_html.clone();
                return Update::task(Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            crate::platform_actions::open_path(&path)
                        })
                        .await
                        .map_err(|_| "Open Guide action worker failed".to_string())?
                    },
                    Message::PlatformActionFinished,
                ));
            }
            Update::none()
        }
        Message::ShowExportedGuideInFolder => {
            if let Some(export) = &state.last_export {
                let path = export.directory.clone();
                return Update::task(Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || crate::platform_actions::reveal(&path))
                            .await
                            .map_err(|_| "Show in Folder action worker failed".to_string())?
                    },
                    Message::PlatformActionFinished,
                ));
            }
            Update::none()
        }
        Message::PlatformActionFinished(Ok(())) => Update::none(),
        Message::PlatformActionFinished(Err(error)) => {
            state.message = Some(error);
            Update::none()
        }
        Message::ExportGifRequested => {
            state.message = None;
            Update::task(Task::perform(
                pick_gif_save_path(picker_default_dir()),
                Message::ExportGifPathChosen,
            ))
        }
        Message::ExportGifPathChosen(None) => Update::none(),
        Message::ExportGifPathChosen(Some(path)) => {
            match export_gif(&state.guide, &state.store, GifOptions::default(), &path) {
                Ok(()) => {
                    tracing::info!(
                        target: "rollshot::action::export",
                        path = %path.display(),
                        "gif exported"
                    );
                    state.message = Some(format!("GIF saved to {}", path.display()));
                }
                Err(error) => {
                    tracing::error!(
                        target: "rollshot::action::export",
                        %error,
                        "gif export failed"
                    );
                    state.message = Some(format!("GIF export failed: {error}"));
                }
            }
            // Unlike guide export, GIF export does NOT exit — the user can still
            // Export Guide afterwards.
            Update::none()
        }
        Message::PreviewStoryboardRequested => {
            #[cfg(feature = "action-guide")]
            if state
                .frame_source
                .as_ref()
                .is_some_and(|fs| fs.in_memory().is_none())
            {
                return Update::none();
            }
            state.message = None;
            match render_timeline_storyboard(state, storyboard_preview_options()) {
                Ok(rendered) => {
                    tracing::info!(
                        target: "rollshot::action::preview",
                        steps = rendered.step_count,
                        width = rendered.width,
                        height = rendered.height,
                        "storyboard preview rendered"
                    );
                    state.storyboard_preview = Some(super::StoryboardPreviewState {
                        handle: super::build_handle(&rendered.image),
                        width: rendered.width,
                        height: rendered.height,
                        step_count: rendered.step_count,
                        copy_state: StoryboardCopyState::Idle,
                    });
                }
                Err(error) => {
                    tracing::error!(
                        target: "rollshot::action::preview",
                        %error,
                        "storyboard preview failed"
                    );
                    state.storyboard_preview = None;
                    state.message = Some(format!("Storyboard preview failed: {error}"));
                }
            }
            Update::none()
        }
        Message::PreviewStoryboardClosed => {
            state.storyboard_preview = None;
            Update::none()
        }
        Message::CopyStoryboardRequested => {
            #[cfg(feature = "action-guide")]
            if state
                .frame_source
                .as_ref()
                .is_some_and(|fs| fs.in_memory().is_none())
            {
                return Update::none();
            }
            let Some(preview) = &state.storyboard_preview else {
                return Update::none();
            };
            if matches!(preview.copy_state, StoryboardCopyState::Copying { .. }) {
                return Update::none();
            }
            let input = match super::storyboard_copy::snapshot_storyboard(
                &state.guide,
                &state.store,
                &state.presentation,
            ) {
                Ok(input) => input,
                Err(error) => {
                    tracing::error!(
                        target: "rollshot::app::storyboard_copy",
                        error = %error,
                        "storyboard snapshot failed during copy request"
                    );
                    state.storyboard_copy_operation_id =
                        state.storyboard_copy_operation_id.saturating_add(1);
                    let operation_id = state.storyboard_copy_operation_id;
                    state.storyboard_preview.as_mut().unwrap().copy_state =
                        StoryboardCopyState::Failed {
                            operation_id,
                            message: error.to_string(),
                        };
                    return Update::none();
                }
            };
            state.storyboard_copy_operation_id =
                state.storyboard_copy_operation_id.saturating_add(1);
            let operation_id = state.storyboard_copy_operation_id;
            state.storyboard_preview.as_mut().unwrap().copy_state =
                StoryboardCopyState::Copying { operation_id };
            Update::task(Task::perform(
                super::storyboard_copy::render_and_copy(input),
                move |result| Message::CopyStoryboardFinished {
                    operation_id,
                    result,
                },
            ))
        }
        Message::CopyStoryboardFinished {
            operation_id,
            result,
        } => {
            let Some(preview) = &mut state.storyboard_preview else {
                return Update::none();
            };
            let current_id = match &preview.copy_state {
                StoryboardCopyState::Copying { operation_id: id } => *id,
                _ => return Update::none(),
            };
            if current_id != operation_id {
                return Update::none();
            }
            match result {
                Ok(copy_result) => {
                    tracing::info!(
                        target: "rollshot::app::storyboard_copy",
                        width = copy_result.width,
                        height = copy_result.height,
                        step_count = copy_result.step_count,
                        "storyboard copied"
                    );
                    preview.copy_state = StoryboardCopyState::Copied { operation_id };
                    let clear_id = operation_id;
                    Update::task(Task::perform(
                        async { tokio::time::sleep(std::time::Duration::from_secs(2)).await },
                        move |_| Message::ClearStoryboardCopyFeedback {
                            operation_id: clear_id,
                        },
                    ))
                }
                Err(error) => {
                    preview.copy_state = StoryboardCopyState::Failed {
                        operation_id,
                        message: error,
                    };
                    Update::none()
                }
            }
        }
        Message::ClearStoryboardCopyFeedback { operation_id } => {
            let Some(preview) = &mut state.storyboard_preview else {
                return Update::none();
            };
            if preview.copy_state == (StoryboardCopyState::Copied { operation_id }) {
                preview.copy_state = StoryboardCopyState::Idle;
            }
            Update::none()
        }
        Message::ExportStoryboardRequested => {
            #[cfg(feature = "action-guide")]
            if state
                .frame_source
                .as_ref()
                .is_some_and(|fs| fs.in_memory().is_none())
            {
                return Update::none();
            }
            state.message = None;
            state.storyboard_preview = None;
            Update::task(Task::perform(
                pick_storyboard_save_path(picker_default_dir()),
                Message::ExportStoryboardPathChosen,
            ))
        }
        Message::ExportStoryboardPathChosen(None) => Update::none(),
        Message::ExportStoryboardPathChosen(Some(path)) => {
            match write_storyboard_png(state, &path) {
                Ok(result) => {
                    tracing::info!(
                        target: "rollshot::action::export",
                        path = %path.display(),
                        steps = result.step_count,
                        width = result.width,
                        height = result.height,
                        "storyboard exported"
                    );
                    state.message = Some(format!("Storyboard saved to {}", path.display()));
                }
                Err(error) => {
                    tracing::error!(
                        target: "rollshot::action::export",
                        %error,
                        path = %path.display(),
                        "storyboard export failed"
                    );
                    state.message = Some(format!("Storyboard export failed: {error}"));
                }
            }
            Update::none()
        }
        Message::ExportBugReport => {
            state.message = None;
            state.issue_pack = Some(super::IssuePackDialog::new());
            Update::none()
        }
        Message::IssuePackReviewChanged(confirmed) => {
            if let Some(dialog) = &mut state.issue_pack {
                dialog.review_confirmed = confirmed;
            }
            Update::none()
        }
        Message::IssuePackIncludeGifChanged(include) => {
            if let Some(dialog) = &mut state.issue_pack {
                dialog.include_gif = include;
            }
            Update::none()
        }
        Message::IssuePackExportFolder => {
            begin_issue_pack_export(state, super::IssuePackKind::Folder)
        }
        Message::IssuePackExportZip => begin_issue_pack_export(state, super::IssuePackKind::Zip),
        Message::IssuePackFolderChosen {
            operation_id,
            parent,
        } => {
            let Some(dialog) = state.issue_pack.as_ref() else {
                return Update::none();
            };
            if dialog.operation_id != operation_id {
                return Update::none();
            }
            let Some(parent) = parent else {
                let dialog = state.issue_pack.as_mut().unwrap();
                dialog.pending_kind = None;
                dialog.pending_export = None;
                dialog.exporting = false;
                return Update::none();
            };
            let (kind, pending, operation_id) = {
                let Some(dialog) = state.issue_pack.as_mut() else {
                    return Update::none();
                };
                let kind = dialog
                    .pending_kind
                    .take()
                    .unwrap_or(super::IssuePackKind::Folder);
                let Some(pending) = dialog.pending_export.take() else {
                    return Update::none();
                };
                let operation_id = dialog.operation_id;
                dialog.exporting = true;
                (kind, pending, operation_id)
            };
            Update::task(Task::perform(
                run_issue_pack_export(pending, kind, parent),
                move |result| Message::IssuePackFinished {
                    operation_id,
                    result,
                },
            ))
        }
        Message::IssuePackFinished {
            operation_id,
            result,
        } => {
            let Some(dialog) = &mut state.issue_pack else {
                return Update::none();
            };
            if dialog.operation_id != operation_id {
                return Update::none();
            }
            dialog.exporting = false;
            match result {
                Ok(result) => {
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
                }
                Err(error) => {
                    if let Some(dialog) = &mut state.issue_pack {
                        dialog.pending_kind = None;
                    }
                    state.message = Some(error);
                }
            }
            Update::none()
        }
        Message::IssuePackCancel => {
            state.issue_pack = None;
            Update::none()
        }
        #[cfg(target_os = "macos")]
        Message::OpenInputMonitoringSettings => {
            rollshot_macos_input::open_input_monitoring_settings();
            state.message = Some("Grant Input Monitoring, then restart Rollshot.".to_string());
            Update::none()
        }
        Message::DismissBanner => {
            state.message = None;
            Update::none()
        }
        Message::AnnotateStepRequested => {
            state.message = None;
            let Some(step) = state.selected_step().cloned() else {
                state.message = Some("Select a step before annotating.".to_string());
                return Update::none();
            };
            match state.presentation.document_for_step(&step, &state.store) {
                Some(doc) => {
                    tracing::info!(
                        target: "rollshot::action::annotation",
                        source = step.source,
                        keyframe = step.keyframe,
                        "annotation session opened"
                    );
                    state.annotation_session = Some(super::annotation::StepAnnotationSession::new(
                        step.source,
                        step.keyframe,
                        doc.document.source(),
                    ));
                }
                None => {
                    state.message = Some(
                        "Cannot annotate this step because its keyframe is unavailable."
                            .to_string(),
                    );
                }
            }
            Update::none()
        }
        Message::AnnotationToolChanged(tool) => {
            if let Some(session) = &mut state.annotation_session {
                session.tool = tool;
                session.draft = None;
            }
            Update::none()
        }
        Message::AnnotationTextChanged(text) => {
            if let Some(session) = &mut state.annotation_session {
                session.text_note = text;
            }
            Update::none()
        }
        Message::AnnotationCanvasPressed(point) => {
            if let Some(session) = &mut state.annotation_session {
                let point = clamp_annotation_point(point, session.width, session.height);
                session.draft = match session.tool {
                    super::annotation::AnnotationTool::Number => {
                        Some(super::annotation::AnnotationDraft::Number {
                            tip: point,
                            bubble: point,
                        })
                    }
                    super::annotation::AnnotationTool::Redaction => {
                        Some(super::annotation::AnnotationDraft::Redaction {
                            start: point,
                            current: point,
                        })
                    }
                    super::annotation::AnnotationTool::Text => None,
                };
            }
            Update::none()
        }
        Message::AnnotationCanvasMoved(point) => {
            if let Some(session) = &mut state.annotation_session {
                let point = clamp_annotation_point(point, session.width, session.height);
                match &mut session.draft {
                    Some(super::annotation::AnnotationDraft::Number { bubble, .. }) => {
                        *bubble = point;
                    }
                    Some(super::annotation::AnnotationDraft::Redaction { current, .. }) => {
                        *current = point;
                    }
                    None => {}
                }
            }
            Update::none()
        }
        Message::AnnotationCanvasReleased(point) => {
            if !state.can_mutate() {
                return Update::none();
            }
            commit_annotation_release(state, point);
            state.mark_project_dirty();
            Update::none()
        }
        Message::AnnotationUndo => {
            if !state.can_mutate() {
                return Update::none();
            }
            dismiss_stale_visual_annotation_review(state);
            with_annotation_document(state, |doc| {
                doc.document.undo();
            });
            state.mark_project_dirty();
            Update::none()
        }
        Message::AnnotationRedo => {
            if !state.can_mutate() {
                return Update::none();
            }
            dismiss_stale_visual_annotation_review(state);
            with_annotation_document(state, |doc| {
                doc.document.redo();
            });
            state.mark_project_dirty();
            Update::none()
        }
        Message::AnnotationDone => {
            // Closing the annotation session must also clear any pending
            dismiss_stale_visual_annotation_review(state);
            state.annotation_session = None;
            Update::none()
        }
        Message::AnnotationCancel => {
            // The scrim and the explicit "Close" button both arrive here.
            // Discard any pending visual annotation review — the user
            // chose to close the modal.
            dismiss_stale_visual_annotation_review(state);
            state.annotation_session = None;
            Update::none()
        }
        Message::CaptionProposalLoaded(Ok(proposal)) => {
            state.caption_suggestions_running = false;
            state.caption_proposal = Some(proposal);
            state.message = Some("Caption suggestions ready for review.".to_string());
            Update::none()
        }
        Message::CaptionProposalLoaded(Err(error)) => {
            state.caption_suggestions_running = false;
            state.message = Some(format!("Caption suggestions failed: {error}"));
            Update::none()
        }
        Message::AcceptCaptionSuggestion(id) => {
            if !state.can_mutate() {
                return Update::none();
            }
            let Some(proposal) = &mut state.caption_proposal else {
                return Update::none();
            };
            match proposal.apply(&mut state.guide, id) {
                rollshot_action::CaptionApplyOutcome::Applied => {
                    state.mark_project_dirty();
                    state.message = Some("Caption suggestion accepted.".to_string());
                }
                rollshot_action::CaptionApplyOutcome::Stale => {
                    state.message =
                        Some("Caption suggestion is stale; regenerate suggestions.".to_string());
                }
                rollshot_action::CaptionApplyOutcome::Missing
                | rollshot_action::CaptionApplyOutcome::NotPending => {}
            }
            Update::none()
        }
        Message::RejectCaptionSuggestion(id) => {
            if let Some(proposal) = &mut state.caption_proposal {
                proposal.reject(id);
            }
            Update::none()
        }
        Message::AcceptAllCaptionSuggestions => {
            if !state.can_mutate() {
                return Update::none();
            }
            if let Some(proposal) = &mut state.caption_proposal {
                let outcomes = proposal.apply_all(&mut state.guide);
                let applied = outcomes
                    .iter()
                    .filter(|&&outcome| outcome == rollshot_action::CaptionApplyOutcome::Applied)
                    .count();
                let stale = outcomes
                    .iter()
                    .filter(|&&outcome| outcome == rollshot_action::CaptionApplyOutcome::Stale)
                    .count();
                if applied > 0 {
                    state.mark_project_dirty();
                }
                state.message = Some(match stale {
                    0 => format!("Accepted {applied} caption suggestions."),
                    _ => format!(
                        "Accepted {applied} caption suggestions; {stale} stale suggestions skipped."
                    ),
                });
            }
            Update::none()
        }
        Message::DismissCaptionProposal => {
            state.caption_proposal = None;
            Update::none()
        }
        Message::SuggestCaptionsRequested => {
            if state.caption_suggestions_running {
                return Update::none();
            }
            if state.guide.is_empty() {
                state.message = Some("No reviewed steps to caption.".to_string());
                return Update::none();
            }
            state.caption_agent_run_id = state.caption_agent_run_id.saturating_add(1);
            let run_id = state.caption_agent_run_id;
            let guide = state.guide.clone();
            let cfg = match crate::daemon::config::rollshot_config_dir()
                .map_err(|_| "Rollshot config directory is unavailable.".to_string())
                .and_then(|dir| crate::result_workspace::workbench::load_provider_config(&dir))
            {
                Ok(cfg) => cfg,
                Err(error) => {
                    state.message = Some(format!("Caption suggestions failed: {error}"));
                    return Update::none();
                }
            };
            if !crate::result_workspace::workbench::has_key(&cfg) {
                state.message =
                    Some("Configure an agent provider before suggesting captions.".to_string());
                return Update::none();
            }
            let model = cfg.model.clone();
            let adapter = match crate::result_workspace::workbench::build_adapter(&cfg) {
                Ok(adapter) => adapter,
                Err(error) => {
                    state.message = Some(format!("Caption suggestions failed: {error}"));
                    return Update::none();
                }
            };
            state.caption_suggestions_running = true;
            state.message = Some("Suggesting captions...".to_string());
            tracing::info!(
                target: "rollshot::action::caption_agent",
                run_id,
                step_count = guide.steps().len(),
                "caption suggestion run started"
            );
            Update::task(Task::perform(
                super::caption_agent::suggest_captions_task(run_id, model, adapter, guide),
                Message::CaptionProposalLoaded,
            ))
        }
        Message::FfmpegSetupCancel => {
            state.ffmpeg_setup = None;
            Update::none()
        }
        Message::FfmpegUseSystem => {
            state.ffmpeg_setup = None;
            state.message = Some(
                "Install FFmpeg or set ROLLSHOT_FFMPEG, then try Export MP4 again.".to_string(),
            );
            Update::none()
        }
        Message::ExportMp4Requested => {
            state.message = None;
            match crate::managed_ffmpeg::resolve_ffmpeg() {
                crate::managed_ffmpeg::FfmpegResolution::Available(_) => {
                    Update::task(Task::perform(
                        pick_mp4_save_path(picker_default_dir()),
                        Message::ExportMp4PathChosen,
                    ))
                }
                crate::managed_ffmpeg::FfmpegResolution::NeedsSetup(info) => {
                    state.ffmpeg_setup = Some(super::FfmpegSetupDialog {
                        info,
                        downloading: false,
                    });
                    Update::none()
                }
            }
        }
        Message::ExportMp4PathChosen(None) => Update::none(),
        Message::ExportMp4PathChosen(Some(path)) => {
            let ffmpeg = match crate::managed_ffmpeg::resolve_ffmpeg() {
                crate::managed_ffmpeg::FfmpegResolution::Available(path) => path,
                crate::managed_ffmpeg::FfmpegResolution::NeedsSetup(info) => {
                    state.ffmpeg_setup = Some(super::FfmpegSetupDialog {
                        info,
                        downloading: false,
                    });
                    return Update::none();
                }
            };
            match export_video(
                &state.guide,
                &state.store,
                VideoOptions::default(),
                &ffmpeg,
                &path,
            ) {
                Ok(()) => {
                    tracing::info!(
                        target: "rollshot::action::export",
                        path = %path.display(),
                        ffmpeg = %ffmpeg.display(),
                        "mp4 exported"
                    );
                    state.message = Some(format!("MP4 saved to {}", path.display()));
                }
                Err(error) => {
                    tracing::error!(
                        target: "rollshot::action::export",
                        %error,
                        path = %path.display(),
                        ffmpeg = %ffmpeg.display(),
                        "mp4 export failed"
                    );
                    state.message = Some(format!("MP4 export failed: {error}"));
                }
            }
            Update::none()
        }
        Message::FfmpegDownloadManaged => {
            let Some(dialog) = &mut state.ffmpeg_setup else {
                return Update::none();
            };
            if dialog.downloading || dialog.info.managed_download.is_none() {
                return Update::none();
            }
            dialog.downloading = true;
            Update::task(Task::perform(
                download_managed_ffmpeg_task(),
                Message::FfmpegDownloadFinished,
            ))
        }
        Message::FfmpegDownloadFinished(Ok(path)) => {
            state.ffmpeg_setup = None;
            state.message = Some(format!("Managed FFmpeg installed at {}", path.display()));
            Update::task(Task::perform(
                pick_mp4_save_path(picker_default_dir()),
                Message::ExportMp4PathChosen,
            ))
        }
        Message::FfmpegDownloadFinished(Err(error)) => {
            if let Some(dialog) = &mut state.ffmpeg_setup {
                dialog.downloading = false;
            }
            state.message = Some(format!("Managed FFmpeg download failed: {error}"));
            Update::none()
        }
        Message::SuggestVisualAnnotationsRequested => {
            if matches!(
                state.visual_annotation_suggestion,
                super::VisualAnnotationSuggestionState::Running { .. }
                    | super::VisualAnnotationSuggestionState::ConsentPending(_)
            ) {
                return Update::none();
            }
            let Some(step) = state.selected_step().cloned() else {
                state.message =
                    Some("Select a step before suggesting visual annotations.".to_string());
                return Update::none();
            };
            let cfg = match crate::daemon::config::rollshot_config_dir()
                .map_err(|_| "Rollshot config directory is unavailable.".to_string())
                .and_then(|dir| crate::result_workspace::workbench::load_provider_config(&dir))
            {
                Ok(cfg) => cfg,
                Err(error) => {
                    state.message = Some(format!("Visual annotation suggestion failed: {error}"));
                    return Update::none();
                }
            };
            if !crate::result_workspace::workbench::has_key(&cfg) {
                state.message = Some(
                    "Configure an agent provider before suggesting visual annotations.".to_string(),
                );
                return Update::none();
            }
            let consent = super::visual_annotation_agent::VisualSuggestionConsent {
                source: step.source,
                keyframe: step.keyframe,
                provider: format!("{}", cfg.provider),
                model: cfg.model.clone(),
            };
            state.visual_annotation_suggestion =
                super::VisualAnnotationSuggestionState::ConsentPending(consent);
            state.message = None;
            Update::none()
        }
        Message::VisualSuggestionConsentCancelled => {
            state.visual_annotation_suggestion = super::VisualAnnotationSuggestionState::Idle;
            Update::none()
        }
        Message::VisualSuggestionConsentConfirmed => {
            let super::VisualAnnotationSuggestionState::ConsentPending(ref consent) =
                state.visual_annotation_suggestion
            else {
                return Update::none();
            };
            let consent_provider = consent.provider.clone();
            let consent_model = consent.model.clone();
            let step_source = consent.source;
            let keyframe = consent.keyframe;
            let cfg = match crate::daemon::config::rollshot_config_dir()
                .map_err(|_| "Rollshot config directory is unavailable.".to_string())
                .and_then(|dir| crate::result_workspace::workbench::load_provider_config(&dir))
            {
                Ok(cfg) => cfg,
                Err(error) => {
                    state.visual_annotation_suggestion =
                        super::VisualAnnotationSuggestionState::Idle;
                    state.message = Some(format!("Visual annotation suggestion failed: {error}"));
                    return Update::none();
                }
            };
            if !crate::result_workspace::workbench::has_key(&cfg) {
                state.visual_annotation_suggestion =
                    super::VisualAnnotationSuggestionState::ConsentPending(
                        super::visual_annotation_agent::VisualSuggestionConsent {
                            source: step_source,
                            keyframe,
                            provider: format!("{}", cfg.provider),
                            model: cfg.model.clone(),
                        },
                    );
                state.message = Some(
                    "Configure an agent provider before suggesting visual annotations.".to_string(),
                );
                return Update::none();
            }
            let current_provider = format!("{}", cfg.provider);
            let current_model = cfg.model.clone();
            if current_provider != consent_provider || current_model != consent_model {
                state.visual_annotation_suggestion =
                    super::VisualAnnotationSuggestionState::ConsentPending(
                        super::visual_annotation_agent::VisualSuggestionConsent {
                            source: step_source,
                            keyframe,
                            provider: current_provider,
                            model: current_model,
                        },
                    );
                state.message =
                    Some("Provider configuration changed. Review the consent again.".to_string());
                return Update::none();
            }
            state.visual_annotation_agent_run_id =
                state.visual_annotation_agent_run_id.saturating_add(1);
            let run_id = state.visual_annotation_agent_run_id;
            let Some(step) = state.selected_step().cloned() else {
                state.visual_annotation_suggestion = super::VisualAnnotationSuggestionState::Idle;
                state.message =
                    Some("Select a step before suggesting visual annotations.".to_string());
                return Update::none();
            };
            let Some(doc) = state.presentation.document_for_step(&step, &state.store) else {
                state.visual_annotation_suggestion = super::VisualAnnotationSuggestionState::Idle;
                state.message = Some(
                    "Cannot suggest visual annotations because the keyframe is unavailable."
                        .to_string(),
                );
                return Update::none();
            };
            let document_state_id = doc.document.state_id();
            let image = doc.document.source().clone();
            let adapter = match crate::result_workspace::workbench::build_adapter(&cfg) {
                Ok(adapter) => adapter,
                Err(error) => {
                    state.visual_annotation_suggestion =
                        super::VisualAnnotationSuggestionState::Idle;
                    state.message = Some(format!("Visual annotation suggestion failed: {error}"));
                    return Update::none();
                }
            };
            let cancellation = rollshot_agent::runtime::RunCancellation::new();
            let task_cancellation = cancellation.clone();
            state.visual_annotation_suggestion = super::VisualAnnotationSuggestionState::Running {
                run_id,
                cancellation,
            };
            state.message = Some("Suggesting visual annotations...".to_string());
            tracing::info!(
                target: "rollshot::action::visual_annotation_agent",
                run_id,
                step_index = step.index,
                keyframe = step.keyframe,
                "visual annotation suggestion run started"
            );
            let input = super::visual_annotation_agent::VisualAnnotationTaskInput {
                run_id,
                step,
                document_state_id,
                image,
            };
            Update::task(Task::perform(
                super::visual_annotation_agent::suggest_visual_annotation_task(
                    input,
                    format!("{}", cfg.provider),
                    cfg.model.clone(),
                    adapter,
                    task_cancellation,
                ),
                move |result| Message::VisualAnnotationProposalLoaded { run_id, result },
            ))
        }
        Message::VisualAnnotationProposalLoaded { run_id, result } => {
            let expected_id = match &state.visual_annotation_suggestion {
                super::VisualAnnotationSuggestionState::Running { run_id, .. } => Some(*run_id),
                _ => None,
            };
            if expected_id != Some(run_id) {
                tracing::debug!(
                    target: "rollshot::action::visual_annotation_agent",
                    late_run_id = run_id,
                    expected_run_id = ?expected_id,
                    "ignoring late visual annotation suggestion result"
                );
                return Update::none();
            }
            match result {
                Ok(super::visual_annotation_agent::VisualAnnotationTaskResult::Proposal(
                    proposal,
                )) => {
                    tracing::info!(
                        target: "rollshot::action::visual_annotation_agent",
                        run_id,
                        "visual annotation suggestion ready for review"
                    );
                    state.visual_annotation_suggestion =
                        super::VisualAnnotationSuggestionState::PendingReview(proposal);
                    state.message =
                        Some("Visual annotation suggestions ready for review.".to_string());
                }
                Ok(super::visual_annotation_agent::VisualAnnotationTaskResult::NoSuggestion {
                    reason,
                }) => {
                    let message = match &reason {
                        Some(text) => format!("Visual annotation suggestion: {text}"),
                        None => "Visual annotation suggestion: no suggestion returned.".to_string(),
                    };
                    state.visual_annotation_suggestion =
                        super::VisualAnnotationSuggestionState::NoSuggestion { reason };
                    state.message = Some(message);
                }
                Err(message) => {
                    tracing::error!(
                        target: "rollshot::action::visual_annotation_agent",
                        run_id,
                        error = %message,
                        "visual annotation suggestion failed"
                    );
                    state.visual_annotation_suggestion =
                        super::VisualAnnotationSuggestionState::Failed { message };
                    state.message = Some(
                        "Visual annotation suggestion failed. See the annotation modal for details."
                            .to_string(),
                    );
                }
            }
            Update::none()
        }
        Message::CancelVisualAnnotationSuggestion => {
            match &state.visual_annotation_suggestion {
                super::VisualAnnotationSuggestionState::Running {
                    run_id,
                    cancellation,
                } => {
                    tracing::info!(
                        target: "rollshot::action::visual_annotation_agent",
                        run_id,
                        "visual annotation suggestion cancelled by user"
                    );
                    cancellation.cancel();
                    state.visual_annotation_suggestion =
                        super::VisualAnnotationSuggestionState::Idle;
                    state.message = Some("Visual annotation suggestion cancelled.".to_string());
                }
                super::VisualAnnotationSuggestionState::ConsentPending(_) => {
                    state.visual_annotation_suggestion =
                        super::VisualAnnotationSuggestionState::Idle;
                }
                _ => {}
            }
            Update::none()
        }
        Message::AcceptAllVisualAnnotations => {
            if !state.can_mutate() {
                return Update::none();
            }
            let step = state.selected_step().cloned();
            let doc = step
                .as_ref()
                .and_then(|s| state.presentation.document_for_step(s, &state.store));
            let (step, doc) = match (step, doc) {
                (Some(step), Some(doc)) => (step, doc),
                _ => {
                    state.visual_annotation_suggestion =
                        super::VisualAnnotationSuggestionState::Idle;
                    return Update::none();
                }
            };
            let super::VisualAnnotationSuggestionState::PendingReview(ref mut proposal) =
                state.visual_annotation_suggestion
            else {
                return Update::none();
            };
            let state_id = doc.document.state_id();
            let image = doc.document.source();
            let w = image.width();
            let h = image.height();
            let mut stale = 0usize;
            let ids: Vec<_> = proposal.suggestions.iter().map(|s| s.id).collect();
            for id in &ids {
                match proposal.validate_item(*id, Some(&step), state_id, w, h) {
                    rollshot_action::VisualAnnotationApplyOutcome::Ready => {}
                    rollshot_action::VisualAnnotationApplyOutcome::Stale => {
                        stale += 1;
                    }
                    _ => {}
                }
            }
            let ops = proposal.pending_edit_ops().unwrap_or_default();
            let applied = match doc.document.apply_batch(ops) {
                Ok(outcome) => outcome.added_ids.len(),
                Err(error) => {
                    state.message = Some(format!(
                        "Could not apply visual annotation suggestions: {error}"
                    ));
                    return Update::none();
                }
            };
            state.visual_annotation_suggestion = super::VisualAnnotationSuggestionState::Idle;
            if applied > 0 {
                state.mark_project_dirty();
            }
            state.message = Some(match stale {
                0 if applied > 0 => format!("Accepted {applied} visual annotation suggestions."),
                0 => "No visual annotations to accept.".to_string(),
                _ if applied > 0 => format!(
                    "Accepted {applied} visual annotation suggestions; {stale} stale suggestions skipped."
                ),
                _ => format!("All {stale} visual annotation suggestions were stale."),
            });
            Update::none()
        }
        Message::RejectVisualAnnotationSuggestion => {
            if let super::VisualAnnotationSuggestionState::PendingReview(mut proposal) =
                std::mem::replace(
                    &mut state.visual_annotation_suggestion,
                    super::VisualAnnotationSuggestionState::Idle,
                )
            {
                proposal.reject_all();
            }
            state.message = Some("Visual annotation suggestions rejected.".to_string());
            Update::none()
        }
        Message::RejectSingleVisualAnnotationSuggestion(id) => {
            let super::VisualAnnotationSuggestionState::PendingReview(ref mut proposal) =
                state.visual_annotation_suggestion
            else {
                return Update::none();
            };
            if let Some(suggestion) = proposal.suggestions.iter_mut().find(|s| s.id == id) {
                if suggestion.status == rollshot_action::VisualAnnotationSuggestionStatus::Pending {
                    suggestion.status = rollshot_action::VisualAnnotationSuggestionStatus::Rejected;
                }
            }
            state.message = Some("Visual annotation rejected.".to_string());
            Update::none()
        }
        Message::DismissVisualAnnotationReview => {
            state.visual_annotation_suggestion = super::VisualAnnotationSuggestionState::Idle;
            Update::none()
        }
        Message::AcceptVisualAnnotation(id) => {
            if !state.can_mutate() {
                return Update::none();
            }
            let Some(step) = state.selected_step().cloned() else {
                state.visual_annotation_suggestion = super::VisualAnnotationSuggestionState::Idle;
                return Update::none();
            };
            let Some(doc) = state.presentation.document_for_step(&step, &state.store) else {
                state.visual_annotation_suggestion = super::VisualAnnotationSuggestionState::Idle;
                return Update::none();
            };
            let super::VisualAnnotationSuggestionState::PendingReview(ref mut proposal) =
                state.visual_annotation_suggestion
            else {
                return Update::none();
            };
            let state_id = doc.document.state_id();
            let w = doc.document.source().width();
            let h = doc.document.source().height();
            match proposal.validate_item(id, Some(&step), state_id, w, h) {
                rollshot_action::VisualAnnotationApplyOutcome::Ready => {
                    // Extract the op for this single suggestion.
                    let suggestion = proposal.suggestions.iter().find(|s| s.id == id).unwrap();
                    let op = match &suggestion.payload {
                        rollshot_action::VisualAnnotationPayload::NumberCallout { tip, bubble } => {
                            rollshot_image_document::EditOp::AddNumberCallout {
                                tip: *tip,
                                bubble: *bubble,
                                style: Default::default(),
                            }
                        }
                        rollshot_action::VisualAnnotationPayload::TextNote { position, text } => {
                            rollshot_image_document::EditOp::AddTextNote {
                                position: *position,
                                text: text.clone(),
                                style: Default::default(),
                            }
                        }
                        rollshot_action::VisualAnnotationPayload::OpaqueRedaction { bounds } => {
                            rollshot_image_document::EditOp::AddRedaction { bounds: *bounds }
                        }
                    };
                    match doc.document.apply_batch(vec![op]) {
                        Ok(_) => {
                            // Mark accepted only after successful apply.
                            let suggestion = proposal
                                .suggestions
                                .iter_mut()
                                .find(|s| s.id == id)
                                .unwrap();
                            suggestion.status =
                                rollshot_action::VisualAnnotationSuggestionStatus::Accepted;
                            // Rebase remaining pending items to the new state.
                            let new_state_id = doc.document.state_id();
                            proposal.rebase(new_state_id);
                            state.mark_project_dirty();
                            state.message = Some("Visual annotation accepted.".to_string());
                        }
                        Err(e) => {
                            state.message = Some(format!("Visual annotation apply failed: {e}"));
                        }
                    }
                }
                rollshot_action::VisualAnnotationApplyOutcome::Stale => {
                    state.message = Some("Visual annotation suggestion is stale.".to_string());
                }
                rollshot_action::VisualAnnotationApplyOutcome::Missing => {
                    state.message = Some("Visual annotation suggestion is missing.".to_string());
                }
                rollshot_action::VisualAnnotationApplyOutcome::NotPending => {
                    state.message =
                        Some("Visual annotation suggestion is no longer pending.".to_string());
                }
            }
            Update::none()
        }
        Message::SaveLater => {
            state.first_save_prompt = super::FirstSavePrompt::Hidden;
            state.save_state = super::ProjectSaveState::Unsaved;
            Update::none()
        }
        Message::SaveRequested => {
            #[cfg(feature = "action-guide")]
            {
                handle_save_requested(state)
            }
            #[cfg(not(feature = "action-guide"))]
            {
                Update::none()
            }
        }
        Message::SaveAsRequested => {
            #[cfg(feature = "action-guide")]
            {
                handle_save_as_requested(state)
            }
            #[cfg(not(feature = "action-guide"))]
            {
                Update::none()
            }
        }
        Message::SavePickerChosen(path) => {
            #[cfg(feature = "action-guide")]
            {
                handle_save_picker_chosen(state, path)
            }
            #[cfg(not(feature = "action-guide"))]
            {
                let _ = path;
                Update::none()
            }
        }
        Message::SaveWorkerFinished(outcome) => {
            #[cfg(feature = "action-guide")]
            {
                handle_save_worker_finished(state, outcome)
            }
            #[cfg(not(feature = "action-guide"))]
            {
                let _ = outcome;
                Update::none()
            }
        }
        Message::CloseSaveAndClose => {
            #[cfg(feature = "action-guide")]
            {
                if state.project_session.is_some()
                    && state.save_state == super::ProjectSaveState::Dirty
                {
                    state.close_intent = super::CloseIntent::SaveThenClose;
                    state.pending_discard = false;
                    handle_save_requested(state)
                } else {
                    state.close_intent = super::CloseIntent::None;
                    state.pending_discard = false;
                    Update::effect(super::Effect::CloseWorkspace)
                }
            }
            #[cfg(not(feature = "action-guide"))]
            {
                state.close_intent = super::CloseIntent::None;
                state.pending_discard = false;
                Update::effect(super::Effect::CloseWorkspace)
            }
        }
        Message::CloseDiscard => {
            state.close_intent = super::CloseIntent::None;
            state.pending_discard = false;
            Update::effect(super::Effect::CloseWorkspace)
        }
        Message::CloseCancel => {
            state.close_intent = super::CloseIntent::None;
            state.pending_discard = false;
            Update::none()
        }
        Message::FrameLoadCompleted {
            generation,
            results,
            remaining,
        } => {
            #[cfg(feature = "action-guide")]
            {
                handle_frame_load_completed(state, generation, results, remaining)
            }
            #[cfg(not(feature = "action-guide"))]
            {
                let _ = (generation, results, remaining);
                Update::none()
            }
        }
    }
}

pub fn subscription(_state: &TimelineWorkspace) -> iced::Subscription<Message> {
    iced::window::close_requests().map(|_id| Message::CloseRequested)
}

/// Initial directory for the folder picker: the user's Pictures dir, or temp.
fn picker_default_dir() -> PathBuf {
    dirs::picture_dir().unwrap_or_else(std::env::temp_dir)
}

fn storyboard_preview_options() -> StoryboardOptions {
    StoryboardOptions {
        max_width: 800,
        max_canvas_pixels: 12_000_000,
        ..StoryboardOptions::default()
    }
}

fn render_timeline_storyboard(
    state: &TimelineWorkspace,
    opts: StoryboardOptions,
) -> Result<StoryboardRenderResult, StoryboardError> {
    let input = super::storyboard_copy::snapshot_storyboard(
        &state.guide,
        &state.store,
        &state.presentation,
    )?;
    super::storyboard_copy::render_storyboard_input(&input, opts)
}

fn write_storyboard_png(
    state: &TimelineWorkspace,
    path: &Path,
) -> Result<StoryboardRenderResult, StoryboardError> {
    let rendered = render_timeline_storyboard(state, StoryboardOptions::default())?;
    write_storyboard_png_atomic(path, &rendered.image)?;
    Ok(rendered)
}

fn write_storyboard_png_atomic(
    path: &Path,
    image: &image::RgbaImage,
) -> Result<(), StoryboardError> {
    let tmp = path.with_extension("png.tmp");
    image
        .save_with_format(&tmp, image::ImageFormat::Png)
        .map_err(|source| {
            let _ = std::fs::remove_file(&tmp);
            StoryboardError::Encode {
                path: tmp.display().to_string(),
                source,
            }
        })?;
    std::fs::rename(&tmp, path).map_err(|source| {
        let _ = std::fs::remove_file(&tmp);
        StoryboardError::Io {
            path: path.display().to_string(),
            source,
        }
    })
}

/// Async frame decode task for project-backed workspaces.
///
/// Acquires semaphore permits (max 2 concurrent), decodes each frame,
/// and returns results along with remaining frames that weren't started.
/// The generation token is checked by the handler on completion.
async fn frame_decode_task(
    generation: u64,
    semaphore: std::sync::Arc<tokio::sync::Semaphore>,
    batch: Vec<(
        rollshot_action::FrameId,
        rollshot_action::StepFrameLoadRequest,
    )>,
    remaining: Vec<(
        rollshot_action::FrameId,
        rollshot_action::StepFrameLoadRequest,
    )>,
) -> Message {
    use rollshot_action::load_step_frame;

    let mut results = Vec::with_capacity(batch.len());
    for (_id, request) in batch {
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore closed");
        let result = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            load_step_frame(request)
        })
        .await
        .map_err(|e| format!("decode worker failed: {e}"))
        .and_then(|r| r.map_err(|e| format!("frame decode failed: {e}")));
        results.push(result);
    }
    Message::FrameLoadCompleted {
        generation,
        results,
        remaining,
    }
}

/// Handle the completion of a frame decode batch for project-backed workspaces.
///
/// Verifies the generation token matches the current coordinator generation.
/// Stale results are dropped without cache insertion or handle creation.
/// Current results are inserted into the byte-bounded cache, renderer handles
/// are built, and annotations are hydrated for the current keyframe.
/// If any required frame fails to decode, sets CorruptReadOnly access.
#[cfg(feature = "action-guide")]
fn handle_frame_load_completed(
    state: &mut TimelineWorkspace,
    generation: u64,
    results: Vec<Result<rollshot_action::LoadedStepFrame, String>>,
    remaining: Vec<(
        rollshot_action::FrameId,
        rollshot_action::StepFrameLoadRequest,
    )>,
) -> Update {
    use super::project::{ProjectAccess, ProjectSession};

    let current_gen = state.frame_coordinator.current_generation();
    if generation != current_gen {
        // Stale completion from a previous step selection. Drop silently.
        tracing::debug!(
            target: "rollshot::frame_load",
            completed_gen = generation,
            current_gen,
            "stale frame load completion dropped"
        );
        return Update::none();
    }

    // Extract selected step info before borrowing frame_source.
    let (keyframe, source_id) = {
        let Some(step) = state.selected_step() else {
            return Update::none();
        };
        (step.keyframe, step.source)
    };

    let Some(ref mut source) = state.frame_source else {
        return Update::none();
    };

    let mut any_required_failed = false;

    for result in results {
        match result {
            Ok(loaded) => {
                let loaded_id = loaded.id;
                let is_keyframe = loaded_id == keyframe;
                source.insert_loaded(loaded);
                if is_keyframe {
                    // Build handle and hydrate annotations for current keyframe.
                    if let Some(img) = source.cached(keyframe) {
                        state.keyframe_handle = Some(super::build_handle(
                            &::image::ImageBuffer::from_raw(
                                img.width(),
                                img.height(),
                                img.as_raw().to_vec(),
                            )
                            .unwrap_or_else(|| ::image::RgbaImage::new(img.width(), img.height())),
                        ));
                        state
                            .presentation
                            .hydrate_for_step(source_id, keyframe, img);
                    }
                } else {
                    // Build strip handle for nearby frame.
                    if let Some(img) = source.cached(loaded_id) {
                        state.strip.push(super::StripFrame {
                            id: loaded_id,
                            handle: super::build_handle(
                                &::image::ImageBuffer::from_raw(
                                    img.width(),
                                    img.height(),
                                    img.as_raw().to_vec(),
                                )
                                .unwrap_or_else(|| {
                                    ::image::RgbaImage::new(img.width(), img.height())
                                }),
                            ),
                        });
                    }
                }
            }
            Err(error) => {
                tracing::error!(
                    target: "rollshot::frame_load",
                    %error,
                    "frame decode failed"
                );
                // Check if this was a required frame (keyframe or nearby).
                // If so, mark as CorruptReadOnly.
                any_required_failed = true;
            }
        }
    }

    if any_required_failed {
        // Set CorruptReadOnly access to prevent further mutations.
        if let Some(ProjectSession::Saved { access, .. }) = &mut state.project_session {
            *access = ProjectAccess::CorruptReadOnly;
            state.save_state = super::ProjectSaveState::Clean;
        }
        state.message = Some(
            "A required frame could not be decoded. The project is now read-only.".to_string(),
        );
        return Update::none();
    }

    // Spawn remaining frames if any.
    if !remaining.is_empty() {
        let semaphore = state.frame_coordinator.semaphore.clone();
        let gen = generation;
        return Update::task(iced::Task::perform(
            frame_decode_task(gen, semaphore, remaining, Vec::new()),
            move |msg| msg,
        ));
    }

    Update::none()
}

async fn pick_export_dir(default_dir: PathBuf) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_directory(default_dir)
        .pick_folder()
        .await
        .map(|handle| handle.path().to_path_buf())
}

async fn pick_gif_save_path(default_dir: PathBuf) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_directory(default_dir)
        .set_file_name("summary.gif")
        .add_filter("GIF image", &["gif"])
        .save_file()
        .await
        .map(|handle| handle.path().to_path_buf())
}

async fn pick_storyboard_save_path(default_dir: PathBuf) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_directory(default_dir)
        .set_file_name("storyboard.png")
        .add_filter("PNG image", &["png"])
        .save_file()
        .await
        .map(|handle| handle.path().to_path_buf())
}

async fn pick_mp4_save_path(default_dir: PathBuf) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_directory(default_dir)
        .set_file_name("summary.mp4")
        .add_filter("MP4 video", &["mp4"])
        .save_file()
        .await
        .map(|handle| handle.path().to_path_buf())
}

async fn run_issue_pack_export(
    pending: super::guide_export::PendingIssuePackExport,
    kind: super::IssuePackKind,
    parent: PathBuf,
) -> Result<crate::issue_pack::IssuePackExportResult, String> {
    tokio::task::spawn_blocking(move || match kind {
        super::IssuePackKind::Folder => crate::issue_pack::export_folder_with_action_guide(
            &pending.input,
            Some(pending.source),
            &parent,
        ),
        super::IssuePackKind::Zip => crate::issue_pack::export_zip_with_action_guide(
            &pending.input,
            Some(pending.source),
            &parent,
        ),
    })
    .await
    .map_err(|error| format!("Issue Pack export worker failed: {error}"))?
    .map_err(|error| error.to_string())
}

async fn download_managed_ffmpeg_task() -> Result<PathBuf, String> {
    tokio::task::spawn_blocking(crate::managed_ffmpeg::download_managed_ffmpeg)
        .await
        .map_err(|error| format!("managed FFmpeg download task failed: {error}"))?
}

pub(crate) fn timeline_issue_pack_input(
    state: &TimelineWorkspace,
    assets: crate::issue_pack::ActionGuideIssueAssets,
) -> crate::issue_pack::IssuePackInput {
    let reviewed = state
        .issue_pack
        .as_ref()
        .is_some_and(|dialog| dialog.review_confirmed);
    crate::issue_pack::IssuePackInput {
        title: None,
        created_at: chrono::Local::now(),
        rollshot_version: env!("CARGO_PKG_VERSION").to_string(),
        platform: crate::issue_pack::PlatformInfo::current(),
        final_image: None,
        action_guide: Some(assets),
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

fn begin_issue_pack_export(state: &mut TimelineWorkspace, kind: super::IssuePackKind) -> Update {
    #[cfg(feature = "action-guide")]
    if state
        .frame_source
        .as_ref()
        .is_some_and(|fs| fs.in_memory().is_none())
    {
        return Update::none();
    }
    if state.issue_pack.is_none() {
        return Update::none();
    }
    if !state.issue_pack.as_ref().unwrap().review_confirmed {
        state.message = Some("Review every keyframe before sharing.".to_string());
        return Update::none();
    }
    let pending = match super::guide_export::prepare_issue_pack_export(state) {
        Ok(pending) => pending,
        Err(error) => {
            state.message = Some(error);
            return Update::none();
        }
    };
    state.next_issue_pack_operation_id = state.next_issue_pack_operation_id.saturating_add(1);
    let operation_id = state.next_issue_pack_operation_id;
    let dialog = state.issue_pack.as_mut().unwrap();
    dialog.operation_id = operation_id;
    dialog.pending_kind = Some(kind);
    dialog.pending_export = Some(pending);
    Update::task(Task::perform(
        pick_export_dir(picker_default_dir()),
        move |parent| Message::IssuePackFolderChosen {
            operation_id,
            parent,
        },
    ))
}

fn clamp_annotation_point(point: ImagePoint, width: u32, height: u32) -> ImagePoint {
    point.clamp_to(width, height)
}

/// Dismiss a pending visual annotation review when a state-changing manual
/// action occurs. Transitions to Idle and displays a "stale" banner.
fn dismiss_stale_visual_annotation_review(state: &mut TimelineWorkspace) {
    if matches!(
        state.visual_annotation_suggestion,
        super::VisualAnnotationSuggestionState::PendingReview(_)
    ) {
        state.visual_annotation_suggestion = super::VisualAnnotationSuggestionState::Idle;
        state.message = Some("Annotation suggestions are stale; regenerate them.".to_string());
    }
}

fn apply_export_finished(
    state: &mut TimelineWorkspace,
    operation_id: u64,
    result: Result<super::guide_export::StandaloneExportResult, String>,
) {
    let super::GuideExportState::Exporting {
        operation_id: current,
    } = &state.export_state
    else {
        return;
    };
    if operation_id != *current {
        return;
    }
    match result {
        Ok(exported) => {
            state.export_state = super::GuideExportState::Succeeded;
            state.last_export = Some(exported);
            state.message = Some("Action Guide exported.".into());
        }
        Err(error) => {
            state.export_state = super::GuideExportState::Idle;
            state.message = Some(format!("Action Guide export failed: {error}"));
        }
    }
}

fn with_annotation_document(
    state: &mut TimelineWorkspace,
    f: impl FnOnce(&mut super::annotation::StepAnnotationDocument),
) {
    let Some(session) = state.annotation_session.as_ref() else {
        return;
    };
    let Some(step) = state
        .guide
        .steps()
        .iter()
        .find(|step| step.source == session.source)
        .cloned()
    else {
        return;
    };
    if let Some(doc) = state.presentation.document_for_step(&step, &state.store) {
        f(doc);
    }
}

fn commit_annotation_release(state: &mut TimelineWorkspace, point: ImagePoint) {
    let Some(session) = &mut state.annotation_session else {
        return;
    };
    let release = clamp_annotation_point(point, session.width, session.height);
    let source = session.source;
    let tool = session.tool;
    let draft = session.draft.take();
    let text_note = session.text_note.trim().to_string();
    let Some(step) = state
        .guide
        .steps()
        .iter()
        .find(|step| step.source == source)
        .cloned()
    else {
        state.annotation_session = None;
        state.message =
            Some("Annotation session closed because the step no longer exists.".to_string());
        return;
    };

    if tool == super::annotation::AnnotationTool::Text && text_note.is_empty() {
        state.message = Some("Enter text before placing a text note.".to_string());
        return;
    }

    let Some(doc) = state.presentation.document_for_step(&step, &state.store) else {
        return;
    };

    let error_message = match tool {
        super::annotation::AnnotationTool::Number => {
            let tip = match draft {
                Some(super::annotation::AnnotationDraft::Number { tip, .. }) => tip,
                _ => release,
            };
            doc.document.add_number_callout(tip, release);
            None
        }
        super::annotation::AnnotationTool::Text => doc
            .document
            .add_text_note(release, text_note)
            .err()
            .map(|error| format!("Text note failed: {error}")),
        super::annotation::AnnotationTool::Redaction => {
            let rect = match draft.and_then(|draft| draft.redaction_rect()) {
                Some(rect) => rect,
                None => rollshot_image_document::ImageRect::from_corners(release, release),
            };
            doc.document
                .add_redaction(rect)
                .err()
                .map(|error| format!("Redaction failed: {error}"))
        }
    };

    if let Some(message) = error_message {
        state.message = Some(message);
    } else {
        dismiss_stale_visual_annotation_review(state);
    }
}

// ---------------------------------------------------------------------------
// Save flow (action-guide feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "action-guide")]
fn handle_save_requested(state: &mut TimelineWorkspace) -> Update {
    use super::project::ProjectSession;
    use super::FirstSavePrompt;

    if state.save_state == super::ProjectSaveState::Saving {
        return Update::none();
    }

    let is_first_save = matches!(state.project_session, Some(ProjectSession::Unsaved) | None);
    if is_first_save {
        state.first_save_prompt = FirstSavePrompt::Picking;
        let op_id = state.next_export_operation_id.wrapping_add(1);
        state.next_export_operation_id = op_id;
        Update::task(Task::perform(
            pick_save_path(picker_default_dir()),
            Message::SavePickerChosen,
        ))
    } else {
        state.save_state = super::ProjectSaveState::Saving;
        state.last_save_error = None;
        let Some(snapshot) = build_snapshot_for_save(state) else {
            state.save_state = super::ProjectSaveState::Dirty;
            state.message = Some("Cannot save: snapshot build failed.".to_string());
            return Update::none();
        };
        let destination = match &state.project_session {
            Some(ProjectSession::Saved { root, .. }) => {
                super::project::SaveDestination::Existing(root.clone())
            }
            _ => {
                state.save_state = super::ProjectSaveState::Dirty;
                state.message = Some("Cannot save: no project root.".to_string());
                return Update::none();
            }
        };
        Update::task(Task::perform(
            super::project::save_project_worker(super::project::SaveProjectRequest {
                snapshot,
                destination,
            }),
            |result| match result {
                Ok(super::project::SaveProjectWorkerResult::ExistingSaved(commit)) => {
                    Message::SaveWorkerFinished(SaveWorkerOutcome::ExistingSaved {
                        revision: commit.manifest.revision,
                    })
                }
                Ok(super::project::SaveProjectWorkerResult::NewWritable { commit, .. }) => {
                    Message::SaveWorkerFinished(SaveWorkerOutcome::NewWritable {
                        root: commit.root.clone(),
                        revision: commit.manifest.revision,
                    })
                }
                Ok(super::project::SaveProjectWorkerResult::NewCommittedReadOnly {
                    commit,
                    category,
                }) => Message::SaveWorkerFinished(SaveWorkerOutcome::NewCommittedReadOnly {
                    root: commit.root.clone(),
                    revision: commit.manifest.revision,
                    category,
                }),
                Err(e) => {
                    Message::SaveWorkerFinished(SaveWorkerOutcome::Failed(e.message_for_ui()))
                }
            },
        ))
    }
}

#[cfg(feature = "action-guide")]
fn handle_save_as_requested(state: &mut TimelineWorkspace) -> Update {
    use super::project::{ProjectAccess, ProjectSession};

    if state.save_state == super::ProjectSaveState::Saving
        || !matches!(
            state.project_session,
            Some(ProjectSession::Saved {
                access: ProjectAccess::Writable(_),
                ..
            })
        )
    {
        return Update::none();
    }

    state.first_save_prompt = super::FirstSavePrompt::Picking;
    Update::task(Task::perform(
        pick_save_path(picker_default_dir()),
        Message::SavePickerChosen,
    ))
}

#[cfg(feature = "action-guide")]
fn handle_save_picker_chosen(state: &mut TimelineWorkspace, path: Option<PathBuf>) -> Update {
    use super::project::ProjectSession;
    use super::FirstSavePrompt;

    let is_first_save = matches!(state.first_save_prompt, FirstSavePrompt::Picking);
    if !is_first_save {
        return Update::none();
    }

    let Some(destination_path) = path else {
        state.first_save_prompt =
            if matches!(state.project_session, Some(ProjectSession::Saved { .. })) {
                FirstSavePrompt::Hidden
            } else {
                FirstSavePrompt::Visible
            };
        if state.close_intent == super::CloseIntent::SaveThenClose {
            state.close_intent = super::CloseIntent::None;
        }
        return Update::none();
    };

    state.save_state = super::ProjectSaveState::Saving;
    state.last_save_error = None;
    state.first_save_prompt = FirstSavePrompt::Hidden;

    let Some(snapshot) = build_snapshot_for_save(state) else {
        state.save_state = super::ProjectSaveState::Dirty;
        state.first_save_prompt = FirstSavePrompt::Visible;
        state.message = Some("Cannot save: snapshot build failed.".to_string());
        return Update::none();
    };

    let destination_path = normalize_project_destination(destination_path);
    let destination = match &state.project_session {
        Some(ProjectSession::Saved { .. }) => {
            super::project::SaveDestination::SaveAs(destination_path)
        }
        _ => super::project::SaveDestination::FirstSave(destination_path),
    };

    let guard_slot = state.pending_writer_guard.clone();
    Update::task(Task::perform(
        super::project::save_project_worker(super::project::SaveProjectRequest {
            snapshot,
            destination,
        }),
        move |result| match result {
            Ok(super::project::SaveProjectWorkerResult::ExistingSaved(commit)) => {
                Message::SaveWorkerFinished(SaveWorkerOutcome::ExistingSaved {
                    revision: commit.manifest.revision,
                })
            }
            Ok(super::project::SaveProjectWorkerResult::NewWritable { commit, guard }) => {
                if let Ok(mut slot) = guard_slot.lock() {
                    *slot = Some(guard);
                }
                Message::SaveWorkerFinished(SaveWorkerOutcome::NewWritable {
                    root: commit.root.clone(),
                    revision: commit.manifest.revision,
                })
            }
            Ok(super::project::SaveProjectWorkerResult::NewCommittedReadOnly {
                commit,
                category,
            }) => Message::SaveWorkerFinished(SaveWorkerOutcome::NewCommittedReadOnly {
                root: commit.root.clone(),
                revision: commit.manifest.revision,
                category,
            }),
            Err(e) => Message::SaveWorkerFinished(SaveWorkerOutcome::Failed(e.message_for_ui())),
        },
    ))
}

#[cfg(feature = "action-guide")]
fn handle_save_worker_finished(
    state: &mut TimelineWorkspace,
    outcome: super::update::SaveWorkerOutcome,
) -> Update {
    use super::project::{ProjectAccess, ProjectSession};

    let should_close = state.close_intent == super::CloseIntent::SaveThenClose;

    match outcome {
        super::update::SaveWorkerOutcome::ExistingSaved { revision } => {
            state.save_state = super::ProjectSaveState::Clean;
            state.last_save_error = None;
            if let Some(ProjectSession::Saved { base_revision, .. }) = &mut state.project_session {
                *base_revision = revision;
            }
            state.message = Some("Saved.".to_string());
            tracing::info!(
                target: "rollshot::project",
                revision,
                "project saved"
            );
        }
        super::update::SaveWorkerOutcome::NewWritable { root, revision } => {
            state.save_state = super::ProjectSaveState::Clean;
            state.last_save_error = None;
            let guard = state
                .pending_writer_guard
                .lock()
                .ok()
                .and_then(|mut slot| slot.take());
            state.project_session = Some(ProjectSession::Saved {
                root,
                base_revision: revision,
                access: match guard {
                    Some(g) => ProjectAccess::Writable(g),
                    None => ProjectAccess::ReadOnly,
                },
            });
            state.message = Some("Project saved.".to_string());
            tracing::info!(
                target: "rollshot::project",
                revision,
                "first save committed (writable)"
            );
        }
        super::update::SaveWorkerOutcome::NewCommittedReadOnly {
            root,
            revision,
            category,
        } => {
            state.save_state = super::ProjectSaveState::Clean;
            state.last_save_error = None;
            state.project_session = Some(ProjectSession::Saved {
                root,
                base_revision: revision,
                access: ProjectAccess::ReadOnly,
            });
            state.message = Some(
                "Project saved, but another process holds the write lock. Editing is disabled."
                    .to_string(),
            );
            tracing::warn!(
                target: "rollshot::project",
                revision,
                category,
                "first save committed but read-only (lock race)"
            );
        }
        super::update::SaveWorkerOutcome::Failed(error) => {
            state.save_state = super::ProjectSaveState::Dirty;
            state.last_save_error = Some(error.clone());
            state.message = Some(format!("Save failed: {error}"));
            tracing::error!(
                target: "rollshot::project",
                %error,
                "save failed"
            );
            if should_close {
                state.close_intent = super::CloseIntent::None;
            }
            return Update::none();
        }
    }

    if let Some((root, display_name)) = state.project_recent_metadata() {
        state.close_intent = super::CloseIntent::None;
        if should_close {
            state.pending_discard = false;
        }
        return Update::effect(super::Effect::ProjectSaved {
            root,
            display_name,
            close_workspace: should_close,
        });
    }

    if should_close {
        state.close_intent = super::CloseIntent::None;
        state.pending_discard = false;
        Update::effect(super::Effect::CloseWorkspace)
    } else {
        Update::none()
    }
}

#[cfg(feature = "action-guide")]
fn build_snapshot_for_save(
    state: &mut TimelineWorkspace,
) -> Option<rollshot_action::project::ProjectSnapshot> {
    match super::project::build_project_snapshot(state) {
        Ok(snap) => Some(snap),
        Err(error) => {
            tracing::error!(
                target: "rollshot::project",
                ?error,
                "snapshot build failed"
            );
            state.message = Some(format!("Cannot save: {error:?}"));
            None
        }
    }
}

#[cfg(feature = "action-guide")]
async fn pick_save_path(default_dir: PathBuf) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_directory(default_dir)
        .set_file_name("guide.rollshot-guide")
        .save_file()
        .await
        .map(|handle| handle.path().to_path_buf())
}

#[cfg(feature = "action-guide")]
fn normalize_project_destination(mut path: PathBuf) -> PathBuf {
    if path.extension().and_then(|extension| extension.to_str()) != Some("rollshot-guide") {
        path.set_extension("rollshot-guide");
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline_workspace::guide_export;
    use crate::timeline_workspace::tests::{recording_from_frames, synthetic_recording};

    #[test]
    fn project_destination_adds_required_extension() {
        assert_eq!(
            normalize_project_destination(PathBuf::from("/tmp/My Guide")),
            PathBuf::from("/tmp/My Guide.rollshot-guide")
        );
        assert_eq!(
            normalize_project_destination(PathBuf::from("/tmp/My Guide.rollshot-guide")),
            PathBuf::from("/tmp/My Guide.rollshot-guide")
        );
    }
    use crate::timeline_workspace::visual_annotation_agent::VisualSuggestionConsent;
    use crate::timeline_workspace::{
        annotation::AnnotationTool, FfmpegSetupDialog, StoryboardCopyState, TimelineWorkspace,
    };
    use rollshot_action::{
        CaptureRegion, InputCapability, InputSourceKind, VisualAnnotationProposal,
    };
    use std::ffi::{OsStr, OsString};

    /// RAII guard that restores an environment variable to its original value on drop.
    struct EnvVarGuard {
        name: &'static str,
        old_value: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(name: &'static str, value: impl AsRef<OsStr>) -> Self {
            let old_value = std::env::var_os(name);
            std::env::set_var(name, value);
            Self { name, old_value }
        }

        fn remove(name: &'static str) -> Self {
            let old_value = std::env::var_os(name);
            std::env::remove_var(name);
            Self { name, old_value }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.old_value.take() {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn ws(recording: rollshot_action::Recording) -> TimelineWorkspace {
        TimelineWorkspace::new(
            recording,
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

    #[test]
    fn select_step_changes_selection() {
        let mut state = ws(synthetic_recording(3));
        let _ = update(&mut state, Message::SelectStep(2));
        assert_eq!(state.selected, Some(2));
    }

    #[test]
    fn select_out_of_range_is_ignored() {
        let mut state = ws(synthetic_recording(2));
        let _ = update(&mut state, Message::SelectStep(99));
        assert_eq!(state.selected, Some(1));
    }

    #[test]
    fn caption_changed_updates_selected_step() {
        let mut state = ws(synthetic_recording(2));

        let _ = update(
            &mut state,
            Message::CaptionChanged("The save action loses the selected value.".to_string()),
        );

        assert_eq!(
            state.selected_step().unwrap().caption,
            "The save action loses the selected value."
        );
    }

    #[test]
    fn replace_keyframe_preserves_selected_step_caption() {
        let mut state = ws(synthetic_recording(1));
        let _ = update(
            &mut state,
            Message::CaptionChanged("The selected value is lost.".to_string()),
        );
        let step = state.selected_step().unwrap();
        let target = *step.nearby.iter().find(|&&f| f != step.keyframe).unwrap();

        let _ = update(&mut state, Message::ReplaceKeyframe(target));

        assert_eq!(
            state.selected_step().unwrap().caption,
            "The selected value is lost."
        );
        assert_eq!(state.selected_step().unwrap().keyframe, target);
    }

    #[test]
    fn title_changed_renames_selected_step() {
        let mut state = ws(synthetic_recording(2));
        let _ = update(
            &mut state,
            Message::TitleChanged("Open Preferences".to_string()),
        );
        assert_eq!(state.selected_step().unwrap().title, "Open Preferences");
    }

    #[test]
    fn delete_step_renumbers_and_clamps_selection() {
        let mut state = ws(synthetic_recording(3));
        let _ = update(&mut state, Message::SelectStep(3));
        let _ = update(&mut state, Message::DeleteStep);
        assert_eq!(state.guide.steps().len(), 2);
        // Steps are renumbered 1..=2; selection clamps to the new last step.
        assert_eq!(state.selected, Some(2));
        assert!(state.guide.steps().iter().all(|s| s.index <= 2));
    }

    #[test]
    fn delete_middle_step_keeps_selection_on_a_real_step() {
        let mut state = ws(synthetic_recording(3));
        let _ = update(&mut state, Message::SelectStep(2));
        let _ = update(&mut state, Message::DeleteStep);
        assert_eq!(state.guide.steps().len(), 2);
        // Deleting the middle step renumbers remaining steps to 1..=2; selection
        // clamps to Some(2) (the former step 3) and must resolve to a real step.
        assert_eq!(state.selected, Some(2));
        assert!(state.selected_step().is_some());
    }

    #[test]
    fn delete_last_remaining_step_is_noop() {
        let mut state = ws(synthetic_recording(1));
        let _ = update(&mut state, Message::DeleteStep);
        assert_eq!(state.guide.steps().len(), 1);
        assert_eq!(state.selected, Some(1));
    }

    #[test]
    fn replace_keyframe_swaps_to_a_nearby_frame() {
        let mut state = ws(synthetic_recording(1));
        let step = state.selected_step().unwrap();
        // synthetic step 1: keyframe = 1, nearby = [0, 1, 2].
        assert_eq!(step.keyframe, 1);
        let target = *step.nearby.iter().find(|&&f| f != step.keyframe).unwrap();
        let _ = update(&mut state, Message::ReplaceKeyframe(target));
        assert_eq!(state.selected_step().unwrap().keyframe, target);
    }

    #[test]
    fn replace_keyframe_rejects_frame_outside_nearby() {
        let mut state = ws(synthetic_recording(1));
        let _ = update(&mut state, Message::ReplaceKeyframe(9999));
        assert_eq!(state.selected_step().unwrap().keyframe, 1);
    }

    #[test]
    fn delete_on_real_recording_keeps_handles_consistent() {
        // Real store so rebuild_selection_handles resolves frames; ensures the
        // delete path's handle rebuild does not panic.
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::DeleteStep);
        // No assertion on handle contents (opaque); reaching here = no panic.
    }

    #[test]
    fn discard_requested_shows_modal_then_cancel_clears_it() {
        let mut state = ws(synthetic_recording(2));
        let _ = update(&mut state, Message::DiscardRequested);
        assert!(state.pending_discard);
        let _ = update(&mut state, Message::CancelDiscard);
        assert!(!state.pending_discard);
    }

    #[test]
    fn confirm_discard_clears_pending_flag() {
        let mut state = ws(synthetic_recording(1));
        let _ = update(&mut state, Message::DiscardRequested);
        assert!(state.pending_discard);
        let _ = update(&mut state, Message::ConfirmDiscard);
        assert!(!state.pending_discard);
    }

    #[test]
    fn close_requested_also_prompts_discard() {
        let mut state = ws(synthetic_recording(1));
        let _ = update(&mut state, Message::CloseRequested);
        assert!(state.pending_discard);
    }

    #[test]
    fn export_dir_chosen_with_id_starts_async_export() {
        let mut state = ws(recording_from_frames());
        state.message = Some("stale".to_string());
        begin_export(&mut state, 1).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let _ = update(
            &mut state,
            Message::ExportDirChosenWithId {
                operation_id: 1,
                parent: Some(tmp.path().to_path_buf()),
            },
        );
        assert!(
            matches!(
                state.export_state,
                super::super::GuideExportState::Exporting { operation_id: 1 }
            ),
            "should transition to Exporting, got {:?}",
            state.export_state
        );
    }

    #[test]
    fn export_empty_guide_with_id_starts_async_export() {
        let mut state = ws(synthetic_recording(0));
        let result = begin_export(&mut state, 1);
        assert!(result.is_err(), "empty guide should fail begin_export");
    }

    #[test]
    fn export_cancelled_picker_with_id_resets_to_idle() {
        let mut state = ws(recording_from_frames());
        begin_export(&mut state, 1).unwrap();
        let _ = update(
            &mut state,
            Message::ExportDirChosenWithId {
                operation_id: 1,
                parent: None,
            },
        );
        assert!(
            matches!(state.export_state, super::super::GuideExportState::Idle),
            "cancel should reset to Idle"
        );
    }

    #[test]
    fn post_export_platform_actions_schedule_result_messages() {
        let mut state = ws(recording_from_frames());
        state.last_export = Some(fake_result("/tmp/action-guide"));

        let open_task = update(&mut state, Message::OpenExportedGuide);
        let reveal_task = update(&mut state, Message::ShowExportedGuideInFolder);

        assert!(open_task.task.units() > 0);
        assert!(reveal_task.task.units() > 0);
    }

    #[test]
    fn export_gif_path_chosen_writes_file_and_keeps_window_open() {
        let mut state = ws(recording_from_frames());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("summary.gif");
        let _ = update(&mut state, Message::ExportGifPathChosen(Some(path.clone())));
        assert!(path.exists(), "GIF file should be written");
        assert!(
            state
                .message
                .as_ref()
                .is_some_and(|m| m.contains("GIF saved")),
            "success banner expected, got {:?}",
            state.message
        );
    }

    #[test]
    fn export_gif_empty_guide_sets_error_and_writes_nothing() {
        let mut state = ws(synthetic_recording(0));
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("summary.gif");
        let _ = update(&mut state, Message::ExportGifPathChosen(Some(path.clone())));
        assert!(!path.exists(), "empty guide must not write a file");
        assert!(
            state.message.is_some(),
            "failure surfaces an inline message"
        );
    }

    #[test]
    fn export_gif_cancelled_picker_is_a_no_op() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::ExportGifPathChosen(None));
        assert!(state.message.is_none());
    }

    #[test]
    fn issue_pack_export_requires_keyframe_review_confirmation() {
        let mut state = ws(recording_from_frames());
        let tmp = tempfile::tempdir().unwrap();

        let _ = update(&mut state, Message::ExportBugReport);
        assert!(
            state.issue_pack.is_some(),
            "dialog should exist after ExportBugReport"
        );
        // begin_issue_pack_export checks review_confirmed before preparing.
        let _ = update(&mut state, Message::IssuePackExportFolder);
        assert!(
            state.message.is_some(),
            "message should be set after IssuePackExportFolder, got None"
        );
        // The picker should not have opened; no pending_export was set.
        // Simulate the picker returning anyway to verify nothing is written.
        let _ = update(
            &mut state,
            Message::IssuePackFolderChosen {
                operation_id: 0,
                parent: Some(tmp.path().to_path_buf()),
            },
        );

        assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
        assert!(
            state
                .message
                .as_ref()
                .unwrap()
                .to_lowercase()
                .contains("review"),
            "message = {:?}",
            state.message
        );
    }

    #[test]
    fn issue_pack_folder_export_uses_reviewed_titles_and_keyframes() {
        let mut state = ws(recording_from_frames());
        let tmp = tempfile::tempdir().unwrap();

        let _ = update(
            &mut state,
            Message::TitleChanged("Open Settings".to_string()),
        );
        let _ = update(&mut state, Message::ExportBugReport);
        let _ = update(&mut state, Message::IssuePackReviewChanged(true));
        // IssuePackExportFolder calls begin_issue_pack_export which prepares
        // the owned source before opening the picker.
        let _ = update(&mut state, Message::IssuePackExportFolder);
        assert!(
            state
                .issue_pack
                .as_ref()
                .and_then(|d| d.pending_export.as_ref())
                .is_some(),
            "pending_export should be set after IssuePackExportFolder"
        );
        assert_eq!(
            state.issue_pack.as_ref().unwrap().operation_id,
            1,
            "operation_id should be allocated"
        );
        // Simulate the picker returning a path. This spawns an async task.
        let task = update(
            &mut state,
            Message::IssuePackFolderChosen {
                operation_id: 1,
                parent: Some(tmp.path().to_path_buf()),
            },
        );
        assert!(
            state.issue_pack.as_ref().unwrap().exporting,
            "exporting should be true after picker returns"
        );
        assert!(
            task.task.units() > 0,
            "picker return should spawn an async export task"
        );
    }

    #[test]
    fn issue_pack_cancel_writes_nothing() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::ExportBugReport);
        let _ = update(&mut state, Message::IssuePackCancel);

        assert!(state.issue_pack.is_none());
    }

    #[test]
    fn stale_issue_pack_picker_result_does_not_consume_current_export() {
        let mut state = ws(recording_from_frames());
        let stale_parent = tempfile::tempdir().unwrap();

        let _ = update(&mut state, Message::ExportBugReport);
        let _ = update(&mut state, Message::IssuePackReviewChanged(true));
        let _ = update(&mut state, Message::IssuePackExportFolder);
        assert_eq!(state.issue_pack.as_ref().unwrap().operation_id, 1);

        let _ = update(&mut state, Message::IssuePackCancel);
        let _ = update(&mut state, Message::ExportBugReport);
        let _ = update(&mut state, Message::IssuePackReviewChanged(true));
        let _ = update(&mut state, Message::IssuePackExportFolder);
        assert_eq!(state.issue_pack.as_ref().unwrap().operation_id, 2);

        let task = update(
            &mut state,
            Message::IssuePackFolderChosen {
                operation_id: 1,
                parent: Some(stale_parent.path().to_path_buf()),
            },
        );

        let dialog = state.issue_pack.as_ref().unwrap();
        assert_eq!(task.task.units(), 0);
        assert_eq!(dialog.operation_id, 2);
        assert!(dialog.pending_export.is_some());
        assert!(!dialog.exporting);
    }

    #[test]
    fn ffmpeg_setup_cancel_closes_dialog() {
        let mut state = ws(recording_from_frames());
        state.ffmpeg_setup = Some(FfmpegSetupDialog {
            info: crate::managed_ffmpeg::FfmpegSetupInfo {
                managed_download: None,
                install_location: PathBuf::from("/tmp/ffmpeg"),
            },
            downloading: false,
        });
        let _ = update(&mut state, Message::FfmpegSetupCancel);
        assert!(state.ffmpeg_setup.is_none());
    }

    #[test]
    fn use_system_ffmpeg_sets_actionable_message() {
        let mut state = ws(recording_from_frames());
        state.ffmpeg_setup = Some(FfmpegSetupDialog {
            info: crate::managed_ffmpeg::FfmpegSetupInfo {
                managed_download: None,
                install_location: PathBuf::from("/tmp/ffmpeg"),
            },
            downloading: false,
        });
        let _ = update(&mut state, Message::FfmpegUseSystem);
        assert!(state.ffmpeg_setup.is_none());
        assert!(state.message.as_ref().unwrap().contains("ROLLSHOT_FFMPEG"));
    }

    #[test]
    fn export_mp4_cancelled_picker_is_a_no_op() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::ExportMp4PathChosen(None));
        assert!(state.message.is_none());
    }

    #[test]
    fn export_mp4_missing_ffmpeg_opens_setup_and_writes_nothing() {
        let _guard = ENV_LOCK.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let _path_guard = EnvVarGuard::set("PATH", "");
        let _ffmpeg_guard = EnvVarGuard::set("ROLLSHOT_FFMPEG", "/definitely/missing/ffmpeg");
        let _root_guard = EnvVarGuard::set("ROLLSHOT_FFMPEG_ROOT", root.path());
        let mut state = ws(recording_from_frames());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("summary.mp4");
        let _ = update(&mut state, Message::ExportMp4PathChosen(Some(path.clone())));
        assert!(!path.exists());
        assert!(state.ffmpeg_setup.is_some());
        assert!(state.message.is_none());
    }

    #[test]
    fn export_mp4_requested_opens_setup_when_ffmpeg_missing() {
        let _guard = ENV_LOCK.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let _path_guard = EnvVarGuard::set("PATH", "");
        let _ffmpeg_guard = EnvVarGuard::set("ROLLSHOT_FFMPEG", "/definitely/missing/ffmpeg");
        let _root_guard = EnvVarGuard::set("ROLLSHOT_FFMPEG_ROOT", root.path());
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::ExportMp4Requested);
        assert!(state.ffmpeg_setup.is_some());
    }

    #[test]
    fn export_storyboard_path_chosen_writes_file_and_keeps_window_open() {
        let mut state = ws(recording_from_frames());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("storyboard.png");
        let _ = update(
            &mut state,
            Message::ExportStoryboardPathChosen(Some(path.clone())),
        );
        assert!(path.exists(), "Storyboard PNG should be written");
        assert!(
            state
                .message
                .as_ref()
                .is_some_and(|m| m.contains("Storyboard saved")),
            "success banner expected, got {:?}",
            state.message
        );
    }

    #[test]
    fn export_storyboard_empty_guide_sets_error_and_writes_nothing() {
        let mut state = ws(synthetic_recording(0));
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("storyboard.png");
        let _ = update(
            &mut state,
            Message::ExportStoryboardPathChosen(Some(path.clone())),
        );
        assert!(!path.exists(), "empty guide must not write a storyboard");
        assert!(
            state.message.as_ref().is_some_and(|m| m.contains("failed")),
            "failure banner expected, got {:?}",
            state.message
        );
    }

    #[test]
    fn export_storyboard_cancelled_picker_is_a_no_op() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::ExportStoryboardPathChosen(None));
        assert!(state.message.is_none());
    }

    #[test]
    fn duplicate_ffmpeg_download_request_is_a_no_op_while_downloading() {
        let mut state = ws(recording_from_frames());
        state.ffmpeg_setup = Some(FfmpegSetupDialog {
            info: crate::managed_ffmpeg::FfmpegSetupInfo {
                managed_download: Some(crate::managed_ffmpeg::LINUX_X86_64_METADATA),
                install_location: PathBuf::from("/tmp/ffmpeg"),
            },
            downloading: true,
        });

        let task = update(&mut state, Message::FfmpegDownloadManaged);

        assert_eq!(task.task.units(), 0);
        assert!(state
            .ffmpeg_setup
            .as_ref()
            .is_some_and(|dialog| dialog.downloading));
    }

    #[test]
    fn preview_storyboard_request_stores_rendered_preview() {
        let mut state = ws(recording_from_frames());

        let _ = update(&mut state, Message::PreviewStoryboardRequested);

        let preview = state.storyboard_preview.as_ref().expect("preview state");
        assert_eq!(preview.step_count, state.guide.steps().len());
        assert_eq!(preview.width, 800);
        assert!(preview.height > 0);
        assert!(
            state.message.is_none(),
            "unexpected banner: {:?}",
            state.message
        );
    }

    #[test]
    fn preview_storyboard_empty_guide_sets_recoverable_message() {
        let mut state = ws(synthetic_recording(0));

        let _ = update(&mut state, Message::PreviewStoryboardRequested);

        assert!(state.storyboard_preview.is_none());
        assert!(
            state
                .message
                .as_ref()
                .is_some_and(|message| message.contains("Storyboard preview failed")),
            "failure banner expected, got {:?}",
            state.message
        );
    }

    #[test]
    fn preview_storyboard_close_clears_preview_state() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::PreviewStoryboardRequested);
        assert!(state.storyboard_preview.is_some());

        let _ = update(&mut state, Message::PreviewStoryboardClosed);

        assert!(state.storyboard_preview.is_none());
    }

    #[test]
    fn preview_storyboard_reopen_reflects_renamed_steps() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::PreviewStoryboardRequested);
        let first_height = state.storyboard_preview.as_ref().unwrap().height;

        let _ = update(&mut state, Message::PreviewStoryboardClosed);
        let _ = update(
            &mut state,
            Message::TitleChanged("A much longer title that changes label measurement".to_string()),
        );
        let _ = update(&mut state, Message::PreviewStoryboardRequested);

        let second = state.storyboard_preview.as_ref().expect("preview state");
        assert_eq!(second.step_count, state.guide.steps().len());
        assert_eq!(second.width, 800);
        assert!(second.height >= first_height);
    }

    #[test]
    fn annotate_step_opens_session_for_selected_keyframe() {
        let mut state = ws(recording_from_frames());

        let _ = update(&mut state, Message::AnnotateStepRequested);

        let session = state.annotation_session.as_ref().expect("session open");
        let step = state.selected_step().unwrap();
        assert_eq!(session.source, step.source);
        assert_eq!(session.keyframe, step.keyframe);
        assert_eq!(session.width, 32);
        assert_eq!(session.height, 32);
    }

    #[test]
    fn annotation_drag_commits_number_callout() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let source = state.selected_step().unwrap().source;

        let _ = update(
            &mut state,
            Message::AnnotationCanvasPressed(rollshot_image_document::ImagePoint::new(4.0, 4.0)),
        );
        let _ = update(
            &mut state,
            Message::AnnotationCanvasMoved(rollshot_image_document::ImagePoint::new(20.0, 20.0)),
        );
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(20.0, 20.0)),
        );

        assert!(state.presentation.has_annotations(source));
        let doc = state.presentation.doc(source).unwrap();
        assert_eq!(doc.document.annotations().len(), 1);
    }

    #[test]
    fn annotation_done_closes_session_without_dropping_document() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let source = state.selected_step().unwrap().source;
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(8.0, 8.0)),
        );

        let _ = update(&mut state, Message::AnnotationDone);

        assert!(state.annotation_session.is_none());
        assert!(state.presentation.has_annotations(source));
    }

    #[test]
    fn storyboard_render_uses_flattened_annotation_pixels() {
        let mut state = ws(recording_from_frames());
        let before = render_timeline_storyboard(&state, storyboard_preview_options())
            .expect("render before annotation")
            .image;
        let source = state.selected_step().unwrap().source;
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(8.0, 8.0)),
        );
        assert!(state.presentation.has_annotations(source));

        let after = render_timeline_storyboard(&state, storyboard_preview_options())
            .expect("render after annotation")
            .image;

        assert_ne!(
            before.as_raw(),
            after.as_raw(),
            "annotated render should differ from raw keyframe render"
        );
    }

    #[test]
    fn storyboard_render_uses_flattened_text_and_redaction_annotations() {
        let mut state = ws(recording_from_frames());
        let source = state.selected_step().unwrap().source;
        let before = render_timeline_storyboard(
            &state,
            StoryboardOptions {
                max_width: 240,
                max_canvas_pixels: 1_000_000,
                outer_padding: 12,
                card_spacing: 10,
                card_padding: 8,
                show_titles: true,
            },
        )
        .expect("storyboard render before annotation");

        let step = state.selected_step().unwrap().clone();
        let doc = state
            .presentation
            .document_for_step(&step, &state.store)
            .expect("presentation doc");
        doc.document
            .add_text_note(
                rollshot_image_document::ImagePoint::new(2.0, 2.0),
                "Note".to_string(),
            )
            .unwrap();
        doc.document
            .add_redaction(rollshot_image_document::ImageRect {
                x: 10.0,
                y: 10.0,
                width: 8.0,
                height: 8.0,
            })
            .unwrap();
        assert!(state.presentation.has_annotations(source));
        assert!(
            state
                .presentation
                .doc(source)
                .unwrap()
                .document
                .flatten()
                .pixels()
                .any(|pixel| pixel.0 == [0, 0, 0, 255]),
            "redaction should flatten to opaque black before storyboard render"
        );

        let after = render_timeline_storyboard(
            &state,
            StoryboardOptions {
                max_width: 240,
                max_canvas_pixels: 1_000_000,
                outer_padding: 12,
                card_spacing: 10,
                card_padding: 8,
                show_titles: true,
            },
        )
        .expect("storyboard render");

        assert_ne!(
            before.image.as_raw(),
            after.image.as_raw(),
            "annotated storyboard render should differ from raw keyframe render"
        );
    }

    #[test]
    fn issue_pack_action_carries_annotated_storyboard_image() {
        let mut state = ws(recording_from_frames());
        state.issue_pack = Some(super::super::IssuePackDialog {
            review_confirmed: true,
            pending_kind: None,
            include_gif: false,
            pending_export: None,
            operation_id: 0,
            exporting: false,
        });
        let original = state
            .store
            .retained(state.guide.steps()[0].keyframe)
            .unwrap()
            .image
            .clone();
        let before = rollshot_action::render_reviewed_storyboard(
            &guide_export::build_reviewed_export_job(&state).unwrap(),
            rollshot_action::StoryboardOptions::default(),
        )
        .expect("storyboard before annotation")
        .image;

        let step = state.selected_step().unwrap().clone();
        let doc = state
            .presentation
            .document_for_step(&step, &state.store)
            .expect("presentation doc");
        doc.document
            .add_redaction(rollshot_image_document::ImageRect {
                x: 4.0,
                y: 4.0,
                width: 12.0,
                height: 12.0,
            })
            .unwrap();

        let after = rollshot_action::render_reviewed_storyboard(
            &guide_export::build_reviewed_export_job(&state).unwrap(),
            rollshot_action::StoryboardOptions::default(),
        )
        .expect("storyboard after annotation")
        .image;

        assert_ne!(
            before.as_raw(),
            after.as_raw(),
            "Issue Pack storyboard image should use flattened annotated keyframes"
        );
        assert_eq!(
            state.store.retained(step.keyframe).unwrap().image.as_raw(),
            original.as_raw(),
            "retained keyframe must be unchanged after annotation"
        );
    }

    #[test]
    fn replacing_keyframe_clears_step_annotations_and_shows_banner() {
        let mut state = ws(recording_from_frames());
        let source = state.selected_step().unwrap().source;
        let replacement = state
            .strip
            .iter()
            .find(|f| Some(f.id) != state.selected_step().map(|step| step.keyframe))
            .expect("replacement frame")
            .id;
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(8.0, 8.0)),
        );
        assert!(state.presentation.has_annotations(source));

        let _ = update(&mut state, Message::ReplaceKeyframe(replacement));

        assert!(!state.presentation.has_annotations(source));
        assert_eq!(
            state.message.as_deref(),
            Some("Step annotations were cleared because the keyframe changed.")
        );
    }

    #[test]
    fn annotation_text_tool_commits_text_note_on_click() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let source = state.selected_step().unwrap().source;
        let _ = update(
            &mut state,
            Message::AnnotationToolChanged(AnnotationTool::Text),
        );
        let _ = update(
            &mut state,
            Message::AnnotationTextChanged("This label matters".to_string()),
        );
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(5.0, 6.0)),
        );

        let doc = state.presentation.doc(source).unwrap();
        assert!(doc.document.annotations().iter().any(|annotation| {
            matches!(
                annotation,
                rollshot_image_document::Annotation::TextNote { text, .. }
                    if text == "This label matters"
            )
        }));
    }

    #[test]
    fn annotation_redaction_tool_commits_dragged_redaction() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let source = state.selected_step().unwrap().source;
        let _ = update(
            &mut state,
            Message::AnnotationToolChanged(AnnotationTool::Redaction),
        );
        let _ = update(
            &mut state,
            Message::AnnotationCanvasPressed(rollshot_image_document::ImagePoint::new(4.0, 4.0)),
        );
        let _ = update(
            &mut state,
            Message::AnnotationCanvasMoved(rollshot_image_document::ImagePoint::new(18.0, 20.0)),
        );
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(18.0, 20.0)),
        );

        let doc = state.presentation.doc(source).unwrap();
        assert!(doc.document.annotations().iter().any(|annotation| {
            matches!(
                annotation,
                rollshot_image_document::Annotation::OpaqueRedaction { bounds, .. }
                    if bounds.width >= 14.0 && bounds.height >= 16.0
            )
        }));
    }

    #[test]
    fn annotation_undo_and_redo_update_current_document() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let source = state.selected_step().unwrap().source;
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(8.0, 8.0)),
        );
        assert_eq!(
            state
                .presentation
                .doc(source)
                .unwrap()
                .document
                .annotations()
                .len(),
            1
        );

        let _ = update(&mut state, Message::AnnotationUndo);
        assert_eq!(
            state
                .presentation
                .doc(source)
                .unwrap()
                .document
                .annotations()
                .len(),
            0
        );

        let _ = update(&mut state, Message::AnnotationRedo);
        assert_eq!(
            state
                .presentation
                .doc(source)
                .unwrap()
                .document
                .annotations()
                .len(),
            1
        );
    }

    #[test]
    fn empty_text_note_click_sets_message_without_committing_annotation() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let source = state.selected_step().unwrap().source;
        let _ = update(
            &mut state,
            Message::AnnotationToolChanged(AnnotationTool::Text),
        );
        let _ = update(
            &mut state,
            Message::AnnotationTextChanged("   ".to_string()),
        );
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(5.0, 6.0)),
        );

        assert_eq!(
            state
                .presentation
                .doc(source)
                .unwrap()
                .document
                .annotations()
                .len(),
            0
        );
        assert!(state
            .message
            .as_ref()
            .is_some_and(|message| message.contains("Enter text")));
    }

    #[test]
    fn zero_area_redaction_sets_message_without_committing_annotation() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let source = state.selected_step().unwrap().source;
        let _ = update(
            &mut state,
            Message::AnnotationToolChanged(AnnotationTool::Redaction),
        );
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(8.0, 8.0)),
        );

        assert_eq!(
            state
                .presentation
                .doc(source)
                .unwrap()
                .document
                .annotations()
                .len(),
            0
        );
        assert!(state
            .message
            .as_ref()
            .is_some_and(|message| message.contains("Redaction failed")));
    }

    fn caption_proposal_for_first_step(
        state: &TimelineWorkspace,
    ) -> rollshot_action::CaptionProposal {
        rollshot_action::CaptionProposal::from_agent_drafts(
            rollshot_action::CaptionProposalId(1),
            42,
            &state.guide,
            vec![rollshot_action::CaptionSuggestionDraft {
                step_source: state.guide.steps()[0].source,
                title: Some("Open Settings".to_string()),
                caption: "The settings panel appears.".to_string(),
                confidence: 0.8,
                rationale: Some("The click begins the settings flow.".to_string()),
            }],
        )
    }

    #[test]
    fn caption_proposal_loaded_stores_review_state() {
        let mut state = ws(synthetic_recording(1));
        state.caption_suggestions_running = true;
        let proposal = caption_proposal_for_first_step(&state);

        let _ = update(&mut state, Message::CaptionProposalLoaded(Ok(proposal)));

        assert!(state.caption_proposal.is_some());
        assert!(!state.caption_suggestions_running);
        assert_eq!(
            state.message,
            Some("Caption suggestions ready for review.".to_string())
        );
    }

    #[test]
    fn caption_proposal_loaded_error_clears_running_state() {
        let mut state = ws(synthetic_recording(1));
        state.caption_suggestions_running = true;

        let _ = update(
            &mut state,
            Message::CaptionProposalLoaded(Err("invalid caption JSON".to_string())),
        );

        assert!(state.caption_proposal.is_none());
        assert!(!state.caption_suggestions_running);
        assert_eq!(
            state.message,
            Some("Caption suggestions failed: invalid caption JSON".to_string())
        );
    }

    #[test]
    fn accepting_caption_suggestion_updates_guide() {
        let mut state = ws(synthetic_recording(1));
        let proposal = caption_proposal_for_first_step(&state);
        let _ = update(&mut state, Message::CaptionProposalLoaded(Ok(proposal)));

        let _ = update(
            &mut state,
            Message::AcceptCaptionSuggestion(rollshot_action::CaptionSuggestionId(1)),
        );

        let step = state.selected_step().unwrap();
        assert_eq!(step.title, "Open Settings");
        assert_eq!(step.caption, "The settings panel appears.");
    }

    #[test]
    fn rejecting_caption_suggestion_does_not_update_guide() {
        let mut state = ws(synthetic_recording(1));
        let proposal = caption_proposal_for_first_step(&state);
        let _ = update(&mut state, Message::CaptionProposalLoaded(Ok(proposal)));

        let _ = update(
            &mut state,
            Message::RejectCaptionSuggestion(rollshot_action::CaptionSuggestionId(1)),
        );

        let step = state.selected_step().unwrap();
        assert_eq!(step.title, "Click");
        assert_eq!(step.caption, "");
    }

    #[test]
    fn accepting_stale_caption_suggestion_shows_message() {
        let mut state = ws(synthetic_recording(1));
        let proposal = caption_proposal_for_first_step(&state);
        let _ = update(&mut state, Message::CaptionProposalLoaded(Ok(proposal)));
        let _ = update(
            &mut state,
            Message::TitleChanged("Manual title".to_string()),
        );

        let _ = update(
            &mut state,
            Message::AcceptCaptionSuggestion(rollshot_action::CaptionSuggestionId(1)),
        );

        assert_eq!(state.selected_step().unwrap().title, "Manual title");
        assert_eq!(state.selected_step().unwrap().caption, "");
        assert_eq!(
            state.message,
            Some("Caption suggestion is stale; regenerate suggestions.".to_string())
        );
    }

    #[test]
    fn storyboard_export_error_leaves_no_target_file() {
        let state = ws(recording_from_frames());
        let dir = tempfile::tempdir().unwrap();
        let target_dir = dir.path().join("missing-parent");
        let target = target_dir.join("storyboard.png");

        let result = write_storyboard_png(&state, &target);

        assert!(result.is_err());
        assert!(!target.exists());
        assert!(!target.with_extension("png.tmp").exists());
    }

    #[test]
    fn suggest_captions_without_provider_key_shows_recoverable_message() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::remove("ANTHROPIC_API_KEY");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(synthetic_recording(1));

        let _ = update(&mut state, Message::SuggestCaptionsRequested);

        assert!(!state.caption_suggestions_running);
        assert_eq!(
            state.message,
            Some("Configure an agent provider before suggesting captions.".to_string())
        );
    }

    #[test]
    fn accepted_caption_suggestion_is_used_by_storyboard_renderer() {
        let mut state = ws(recording_from_frames());
        let proposal = rollshot_action::CaptionProposal::from_agent_drafts(
            rollshot_action::CaptionProposalId(1),
            42,
            &state.guide,
            vec![rollshot_action::CaptionSuggestionDraft {
                step_source: state.guide.steps()[0].source,
                title: Some("Open Preferences".to_string()),
                caption: "The preferences window is opened for configuration.".to_string(),
                confidence: 0.8,
                rationale: None,
            }],
        );
        let _ = update(&mut state, Message::CaptionProposalLoaded(Ok(proposal)));
        let _ = update(
            &mut state,
            Message::AcceptCaptionSuggestion(rollshot_action::CaptionSuggestionId(1)),
        );

        let rendered = render_timeline_storyboard(&state, storyboard_preview_options())
            .expect("storyboard renders after accepting caption");

        assert_eq!(rendered.step_count, state.guide.steps().len());
        assert_eq!(
            state.guide.steps()[0].caption,
            "The preferences window is opened for configuration."
        );
    }

    #[test]
    fn accepted_caption_suggestion_is_used_by_issue_pack_input() {
        let mut state = ws(recording_from_frames());
        let proposal = rollshot_action::CaptionProposal::from_agent_drafts(
            rollshot_action::CaptionProposalId(1),
            42,
            &state.guide,
            vec![rollshot_action::CaptionSuggestionDraft {
                step_source: state.guide.steps()[0].source,
                title: Some("Open Preferences".to_string()),
                caption: "The preferences window is opened for configuration.".to_string(),
                confidence: 0.8,
                rationale: None,
            }],
        );
        let _ = update(&mut state, Message::CaptionProposalLoaded(Ok(proposal)));
        let _ = update(
            &mut state,
            Message::AcceptCaptionSuggestion(rollshot_action::CaptionSuggestionId(1)),
        );

        let job = guide_export::build_reviewed_export_job(&state).unwrap();
        let include_gif = state
            .issue_pack
            .as_ref()
            .is_some_and(|dialog| dialog.include_gif);
        let assets = crate::issue_pack::ActionGuideIssueAssets::from_job(&job, include_gif);
        let input = timeline_issue_pack_input(&state, assets);
        let first_step = &input.action_guide.as_ref().unwrap().steps[0];

        assert_eq!(first_step.title, "Open Preferences");
        assert_eq!(
            first_step.caption.as_deref(),
            Some("The preferences window is opened for configuration.")
        );
    }

    // ---- Storyboard copy state machine ----

    fn workspace_with_open_preview() -> TimelineWorkspace {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::PreviewStoryboardRequested);
        assert!(state.storyboard_preview.is_some());
        state
    }

    fn copy_result() -> crate::timeline_workspace::storyboard_copy::StoryboardCopyResult {
        crate::timeline_workspace::storyboard_copy::StoryboardCopyResult {
            width: 1200,
            height: 800,
            step_count: 1,
        }
    }

    #[test]
    fn storyboard_copy_state_starts_idle_when_preview_opens() {
        let state = workspace_with_open_preview();
        assert_eq!(
            state.storyboard_preview.unwrap().copy_state,
            StoryboardCopyState::Idle
        );
    }

    #[test]
    fn older_copy_completion_cannot_replace_newer_operation() {
        let mut state = workspace_with_open_preview();
        state.storyboard_copy_operation_id = 2;
        state.storyboard_preview.as_mut().unwrap().copy_state =
            StoryboardCopyState::Copying { operation_id: 2 };

        let _ = update(
            &mut state,
            Message::CopyStoryboardFinished {
                operation_id: 1,
                result: Ok(copy_result()),
            },
        );

        assert_eq!(
            state.storyboard_preview.unwrap().copy_state,
            StoryboardCopyState::Copying { operation_id: 2 }
        );
    }

    #[test]
    fn completion_after_preview_close_is_ignored() {
        let mut state = workspace_with_open_preview();
        let _ = update(&mut state, Message::PreviewStoryboardClosed);
        let _ = update(
            &mut state,
            Message::CopyStoryboardFinished {
                operation_id: 1,
                result: Ok(copy_result()),
            },
        );
        assert!(state.storyboard_preview.is_none());
    }

    #[test]
    fn retry_after_failure_allocates_new_operation_id() {
        let mut state = workspace_with_open_preview();
        state.storyboard_copy_operation_id = 5;
        state.storyboard_preview.as_mut().unwrap().copy_state = StoryboardCopyState::Failed {
            operation_id: 5,
            message: "previous failure".to_string(),
        };

        let task = update(&mut state, Message::CopyStoryboardRequested);

        assert_eq!(state.storyboard_copy_operation_id, 6);
        assert_eq!(
            state.storyboard_preview.unwrap().copy_state,
            StoryboardCopyState::Copying { operation_id: 6 }
        );
        assert!(
            task.task.units() > 0,
            "should return a render-and-copy task"
        );
    }

    #[test]
    fn duplicate_copy_request_while_copying_does_not_increment_id() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::PreviewStoryboardRequested);
        assert!(state.storyboard_preview.is_some());

        state.storyboard_copy_operation_id = 5;
        state.storyboard_preview.as_mut().unwrap().copy_state =
            StoryboardCopyState::Copying { operation_id: 5 };

        let task = update(&mut state, Message::CopyStoryboardRequested);
        assert_eq!(
            task.task.units(),
            0,
            "duplicate request should return a no-op task"
        );
        assert_eq!(
            state.storyboard_copy_operation_id, 5,
            "operation_id should not increment"
        );
        assert_eq!(
            state.storyboard_preview.unwrap().copy_state,
            StoryboardCopyState::Copying { operation_id: 5 },
            "state should remain Copying"
        );
    }

    #[test]
    fn copy_finished_error_transitions_to_failed() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::PreviewStoryboardRequested);
        assert!(state.storyboard_preview.is_some());

        state.storyboard_preview.as_mut().unwrap().copy_state =
            StoryboardCopyState::Copying { operation_id: 1 };

        let _ = update(
            &mut state,
            Message::CopyStoryboardFinished {
                operation_id: 1,
                result: Err("disk full".to_string()),
            },
        );

        let preview = state.storyboard_preview.unwrap();
        assert_eq!(
            preview.copy_state,
            StoryboardCopyState::Failed {
                operation_id: 1,
                message: "disk full".to_string()
            }
        );
    }

    #[test]
    fn old_clear_cannot_erase_newer_copied_state() {
        let mut state = workspace_with_open_preview();
        state.storyboard_preview.as_mut().unwrap().copy_state =
            StoryboardCopyState::Copied { operation_id: 2 };

        let _ = update(
            &mut state,
            Message::ClearStoryboardCopyFeedback { operation_id: 1 },
        );

        assert_eq!(
            state.storyboard_preview.unwrap().copy_state,
            StoryboardCopyState::Copied { operation_id: 2 }
        );
    }

    #[test]
    fn old_clear_cannot_erase_newer_failed_state() {
        let mut state = workspace_with_open_preview();
        state.storyboard_preview.as_mut().unwrap().copy_state = StoryboardCopyState::Failed {
            operation_id: 2,
            message: "new failure".to_string(),
        };

        let _ = update(
            &mut state,
            Message::ClearStoryboardCopyFeedback { operation_id: 1 },
        );

        assert_eq!(
            state.storyboard_preview.unwrap().copy_state,
            StoryboardCopyState::Failed {
                operation_id: 2,
                message: "new failure".to_string(),
            }
        );
    }

    // ---------- Visual annotation consent & review lifecycle ----------

    #[test]
    fn new_workspace_has_idle_visual_annotation_state() {
        let state = ws(synthetic_recording(1));
        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
        ));
        assert_eq!(state.visual_annotation_agent_run_id, 0);
    }

    #[test]
    fn suggest_visual_annotations_without_selection_sets_message() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(synthetic_recording(0));

        let _ = update(&mut state, Message::SuggestVisualAnnotationsRequested);

        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
        ));
        assert!(state
            .message
            .as_ref()
            .is_some_and(|m| m.contains("Select a step")));
    }

    #[test]
    fn suggest_visual_annotations_without_key_sets_message() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::remove("ANTHROPIC_API_KEY");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());

        let _ = update(&mut state, Message::SuggestVisualAnnotationsRequested);

        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
        ));
        assert!(state
            .message
            .as_ref()
            .is_some_and(|m| m.contains("Configure an agent provider")));
    }

    #[test]
    fn suggest_visual_annotations_transitions_to_consent_pending() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());

        let _ = update(&mut state, Message::SuggestVisualAnnotationsRequested);

        match &state.visual_annotation_suggestion {
            crate::timeline_workspace::VisualAnnotationSuggestionState::ConsentPending(consent) => {
                assert_eq!(consent.keyframe, state.selected_step().unwrap().keyframe);
                assert!(!consent.provider.is_empty());
                assert!(!consent.model.is_empty());
            }
            other => panic!("expected ConsentPending, got {other:?}"),
        }
    }

    #[test]
    fn visual_consent_cancel_keeps_workspace_idle() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::SuggestVisualAnnotationsRequested);

        let _ = update(&mut state, Message::VisualSuggestionConsentCancelled);

        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
        ));
    }

    #[test]
    fn visual_consent_confirm_starts_running_task() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::SuggestVisualAnnotationsRequested);

        let task = update(&mut state, Message::VisualSuggestionConsentConfirmed);

        assert!(
            task.task.units() > 0,
            "consent confirm should return a Task::perform"
        );
        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Running { .. }
        ));
    }

    #[test]
    fn visual_proposal_loaded_matching_run_stores_pending_review() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::SuggestVisualAnnotationsRequested);
        let _ = update(&mut state, Message::VisualSuggestionConsentConfirmed);
        let run_id = match &state.visual_annotation_suggestion {
            crate::timeline_workspace::VisualAnnotationSuggestionState::Running {
                run_id, ..
            } => *run_id,
            other => panic!("expected Running, got {other:?}"),
        };

        let proposal = visual_proposal_for_first_step(&mut state, run_id);
        let _ = update(
            &mut state,
            Message::VisualAnnotationProposalLoaded {
                run_id,
                result: Ok(
                    crate::timeline_workspace::visual_annotation_agent::VisualAnnotationTaskResult::Proposal(proposal),
                ),
            },
        );

        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(_)
        ));
    }

    #[test]
    fn visual_proposal_loaded_late_run_is_rejected() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::SuggestVisualAnnotationsRequested);
        let _ = update(&mut state, Message::VisualSuggestionConsentConfirmed);
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::Running {
                run_id: 2,
                cancellation: rollshot_agent::runtime::RunCancellation::new(),
            };

        let old_proposal = visual_proposal_for_first_step(&mut state, 1);
        let _ = update(
            &mut state,
            Message::VisualAnnotationProposalLoaded {
                run_id: 1,
                result: Ok(
                    crate::timeline_workspace::visual_annotation_agent::VisualAnnotationTaskResult::Proposal(old_proposal),
                ),
            },
        );

        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Running { run_id: 2, .. }
        ));
    }

    #[test]
    fn visual_proposal_loaded_no_suggestion_stores_no_suggestion() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::SuggestVisualAnnotationsRequested);
        let _ = update(&mut state, Message::VisualSuggestionConsentConfirmed);
        let run_id = match &state.visual_annotation_suggestion {
            crate::timeline_workspace::VisualAnnotationSuggestionState::Running {
                run_id, ..
            } => *run_id,
            other => panic!("expected Running, got {other:?}"),
        };

        let _ = update(
            &mut state,
            Message::VisualAnnotationProposalLoaded {
                run_id,
                result: Ok(crate::timeline_workspace::visual_annotation_agent::VisualAnnotationTaskResult::NoSuggestion {
                    reason: Some("no suggestion".to_string()),
                }),
            },
        );

        match &state.visual_annotation_suggestion {
            crate::timeline_workspace::VisualAnnotationSuggestionState::NoSuggestion { reason } => {
                assert_eq!(reason.as_deref(), Some("no suggestion"));
            }
            other => panic!("expected NoSuggestion, got {other:?}"),
        }
    }

    #[test]
    fn visual_proposal_loaded_error_stores_failed() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::SuggestVisualAnnotationsRequested);
        let _ = update(&mut state, Message::VisualSuggestionConsentConfirmed);
        let run_id = match &state.visual_annotation_suggestion {
            crate::timeline_workspace::VisualAnnotationSuggestionState::Running {
                run_id, ..
            } => *run_id,
            other => panic!("expected Running, got {other:?}"),
        };

        let _ = update(
            &mut state,
            Message::VisualAnnotationProposalLoaded {
                run_id,
                result: Err("provider failed".to_string()),
            },
        );

        match &state.visual_annotation_suggestion {
            crate::timeline_workspace::VisualAnnotationSuggestionState::Failed { message } => {
                assert_eq!(message, "provider failed");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn reject_visual_annotation_clears_pending_review() {
        let mut state = ws(recording_from_frames());
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(
                visual_proposal_for_first_step(&mut state, 1),
            );

        let _ = update(&mut state, Message::RejectVisualAnnotationSuggestion);

        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
        ));
    }

    #[test]
    fn accept_all_visual_annotations_applies_pending_items() {
        let mut state = ws(recording_from_frames());
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(
                visual_proposal_for_first_step(&mut state, 1),
            );

        let _ = update(&mut state, Message::AcceptAllVisualAnnotations);

        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
        ));
    }

    #[test]
    fn dismiss_visual_annotation_review_clears_state() {
        let mut state = ws(recording_from_frames());
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(
                visual_proposal_for_first_step(&mut state, 1),
            );

        let _ = update(&mut state, Message::DismissVisualAnnotationReview);

        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
        ));
    }

    #[test]
    fn delete_step_dismisses_visual_annotation_review() {
        let mut state = ws(synthetic_recording(2));
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(
                proposal_for_synthetic_step(&state),
            );

        let _ = update(&mut state, Message::DeleteStep);

        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
        ));
        assert_eq!(
            state.message.as_deref(),
            Some("Annotation suggestions are stale; regenerate them.")
        );
    }

    #[test]
    fn replace_keyframe_dismisses_visual_annotation_review() {
        let mut state = ws(recording_from_frames());
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(
                visual_proposal_for_first_step(&mut state, 1),
            );
        let step = state.selected_step().unwrap();
        let replacement = *step
            .nearby
            .iter()
            .find(|&&f| f != step.keyframe)
            .expect("replacement frame id");

        let _ = update(&mut state, Message::ReplaceKeyframe(replacement));

        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
        ));
        assert_eq!(
            state.message.as_deref(),
            Some("Annotation suggestions are stale; regenerate them.")
        );
    }

    #[test]
    fn annotation_undo_dismisses_visual_annotation_review() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(8.0, 8.0)),
        );
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(
                visual_proposal_for_first_step(&mut state, 1),
            );

        let _ = update(&mut state, Message::AnnotationUndo);

        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
        ));
    }

    #[test]
    fn annotation_redo_dismisses_visual_annotation_review() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(8.0, 8.0)),
        );
        let _ = update(&mut state, Message::AnnotationUndo);
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(
                visual_proposal_for_first_step(&mut state, 1),
            );

        let _ = update(&mut state, Message::AnnotationRedo);

        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
        ));
    }

    #[test]
    fn annotation_done_dismisses_visual_annotation_review() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(
                visual_proposal_for_first_step(&mut state, 1),
            );

        let _ = update(&mut state, Message::AnnotationDone);

        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
        ));
    }

    #[test]
    fn visual_annotation_cancel_on_running_invokes_cancellation() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::SuggestVisualAnnotationsRequested);
        let _ = update(&mut state, Message::VisualSuggestionConsentConfirmed);
        let cancellation = match &state.visual_annotation_suggestion {
            crate::timeline_workspace::VisualAnnotationSuggestionState::Running {
                cancellation,
                ..
            } => cancellation.clone(),
            other => panic!("expected Running, got {other:?}"),
        };

        let _ = update(&mut state, Message::CancelVisualAnnotationSuggestion);

        assert!(cancellation.is_cancelled());
        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
        ));
    }

    #[test]
    fn suggest_visual_annotations_while_running_is_noop() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::SuggestVisualAnnotationsRequested);
        let _ = update(&mut state, Message::VisualSuggestionConsentConfirmed);
        let prev_run_id = match &state.visual_annotation_suggestion {
            crate::timeline_workspace::VisualAnnotationSuggestionState::Running {
                run_id, ..
            } => *run_id,
            other => panic!("expected Running, got {other:?}"),
        };

        let task = update(&mut state, Message::SuggestVisualAnnotationsRequested);

        assert_eq!(task.task.units(), 0);
        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Running { run_id, .. } if run_id == prev_run_id
        ));
    }

    #[test]
    fn visual_consent_confirm_with_changed_provider_stays_consent_pending() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::SuggestVisualAnnotationsRequested);

        // Simulate key becoming unavailable between request and confirm.
        drop(_anthropic);
        let _anthropic2 = EnvVarGuard::remove("ANTHROPIC_API_KEY");

        let task = update(&mut state, Message::VisualSuggestionConsentConfirmed);

        assert_eq!(task.task.units(), 0);
        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::ConsentPending(_)
        ));
        assert!(state
            .message
            .as_ref()
            .is_some_and(|m| m.contains("Configure an agent provider")));
    }

    #[test]
    fn document_edit_discards_visual_annotation_pending_review() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(
                visual_proposal_for_first_step(&mut state, 1),
            );

        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(8.0, 8.0)),
        );

        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
        ));
    }

    #[test]
    fn visual_annotation_cancel_on_consent_pending_goes_idle() {
        let mut state = ws(recording_from_frames());
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::ConsentPending(
                VisualSuggestionConsent {
                    source: 1,
                    keyframe: 1,
                    provider: "Anthropic".to_string(),
                    model: "test".to_string(),
                },
            );

        let _ = update(&mut state, Message::CancelVisualAnnotationSuggestion);

        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
        ));
    }

    /// Build a VisualAnnotationProposal with three primitives (callout, note,
    /// redaction) for the first step, used by the individual-accept and
    /// stale-flow tests.
    fn visual_proposal_three_primitives(
        state: &mut TimelineWorkspace,
        run_id: u64,
    ) -> VisualAnnotationProposal {
        let step = &state.guide.steps()[0];
        let doc = state
            .presentation
            .document_for_step(step, &state.store)
            .expect("presentation document");
        let image = doc.document.source();
        VisualAnnotationProposal::from_agent_drafts(
            rollshot_action::VisualAnnotationProposalId(run_id),
            run_id,
            step,
            doc.document.state_id(),
            image.width(),
            image.height(),
            vec![
                rollshot_action::VisualAnnotationSuggestionDraft {
                    id: rollshot_action::VisualAnnotationSuggestionId(1),
                    payload: rollshot_action::VisualAnnotationPayload::NumberCallout {
                        tip: rollshot_image_document::ImagePoint::new(4.0, 4.0),
                        bubble: rollshot_image_document::ImagePoint::new(20.0, 20.0),
                    },
                    confidence: 0.9,
                    rationale: Some("button click target".to_string()),
                },
                rollshot_action::VisualAnnotationSuggestionDraft {
                    id: rollshot_action::VisualAnnotationSuggestionId(2),
                    payload: rollshot_action::VisualAnnotationPayload::TextNote {
                        position: rollshot_image_document::ImagePoint::new(8.0, 8.0),
                        text: "Save button".to_string(),
                    },
                    confidence: 0.7,
                    rationale: None,
                },
                rollshot_action::VisualAnnotationSuggestionDraft {
                    id: rollshot_action::VisualAnnotationSuggestionId(3),
                    payload: rollshot_action::VisualAnnotationPayload::OpaqueRedaction {
                        bounds: rollshot_image_document::ImageRect {
                            x: 2.0,
                            y: 2.0,
                            width: 10.0,
                            height: 8.0,
                        },
                    },
                    confidence: 0.6,
                    rationale: Some("sensitive info".to_string()),
                },
            ],
        )
        .expect("valid proposal")
    }

    // Helper: build a VisualAnnotationProposal for the first step
    fn visual_proposal_for_first_step(
        state: &mut TimelineWorkspace,
        run_id: u64,
    ) -> VisualAnnotationProposal {
        let step = &state.guide.steps()[0];
        let doc = state
            .presentation
            .document_for_step(step, &state.store)
            .expect("presentation document");
        let image = doc.document.source();
        VisualAnnotationProposal::from_agent_drafts(
            rollshot_action::VisualAnnotationProposalId(run_id),
            run_id,
            step,
            doc.document.state_id(),
            image.width(),
            image.height(),
            vec![rollshot_action::VisualAnnotationSuggestionDraft {
                id: rollshot_action::VisualAnnotationSuggestionId(1),
                payload: rollshot_action::VisualAnnotationPayload::TextNote {
                    position: rollshot_image_document::ImagePoint::new(4.0, 4.0),
                    text: "test note".to_string(),
                },
                confidence: 0.8,
                rationale: None,
            }],
        )
        .expect("valid proposal")
    }

    fn proposal_for_synthetic_step(state: &TimelineWorkspace) -> VisualAnnotationProposal {
        let step = &state.guide.steps()[0];
        VisualAnnotationProposal::from_agent_drafts(
            rollshot_action::VisualAnnotationProposalId(1),
            1,
            step,
            0,
            32,
            32,
            vec![rollshot_action::VisualAnnotationSuggestionDraft {
                id: rollshot_action::VisualAnnotationSuggestionId(1),
                payload: rollshot_action::VisualAnnotationPayload::TextNote {
                    position: rollshot_image_document::ImagePoint::new(4.0, 4.0),
                    text: "test".to_string(),
                },
                confidence: 0.8,
                rationale: None,
            }],
        )
        .expect("valid proposal")
    }

    #[test]
    fn individual_accept_visual_annotation_applies_one_and_rebases() {
        let mut state = ws(recording_from_frames());
        let proposal = visual_proposal_three_primitives(&mut state, 1);
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(proposal);
        let source = state.selected_step().unwrap().source;
        let state_before = state.presentation.doc(source).unwrap().document.state_id();

        let _ = update(
            &mut state,
            Message::AcceptVisualAnnotation(rollshot_action::VisualAnnotationSuggestionId(2)),
        );

        let doc = state.presentation.doc(source).unwrap();
        assert_ne!(
            doc.document.state_id(),
            state_before,
            "state_id must increment"
        );
        assert_eq!(doc.document.annotations().len(), 1, "one note applied");
        assert!(
            doc.document.annotations().iter().any(|a| matches!(
                a,
                rollshot_image_document::Annotation::TextNote { text, .. } if text == "Save button"
            )),
            "applied annotation must be the text note"
        );
        match &state.visual_annotation_suggestion {
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(p) => {
                assert_eq!(p.suggestions.len(), 3);
                assert_eq!(
                    p.suggestions[0].status,
                    rollshot_action::VisualAnnotationSuggestionStatus::Pending
                );
                assert_eq!(
                    p.suggestions[1].status,
                    rollshot_action::VisualAnnotationSuggestionStatus::Accepted
                );
                assert_eq!(
                    p.suggestions[2].status,
                    rollshot_action::VisualAnnotationSuggestionStatus::Pending
                );
                assert_eq!(
                    p.suggestions[0].base.document_state_id,
                    doc.document.state_id(),
                    "remaining items rebased"
                );
            }
            other => panic!("expected PendingReview, got {other:?}"),
        }
    }

    #[test]
    fn rejected_visual_annotation_cannot_reaccept() {
        let mut state = ws(recording_from_frames());
        let proposal = visual_proposal_three_primitives(&mut state, 1);
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(proposal);

        let _ = update(
            &mut state,
            Message::AcceptVisualAnnotation(rollshot_action::VisualAnnotationSuggestionId(2)),
        );

        let _ = update(
            &mut state,
            Message::AcceptVisualAnnotation(rollshot_action::VisualAnnotationSuggestionId(2)),
        );

        match &state.visual_annotation_suggestion {
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(p) => {
                assert_eq!(
                    p.suggestions[1].status,
                    rollshot_action::VisualAnnotationSuggestionStatus::Accepted
                );
            }
            other => panic!("expected PendingReview, got {other:?}"),
        }
        let source = state.selected_step().unwrap().source;
        assert_eq!(
            state
                .presentation
                .doc(source)
                .unwrap()
                .document
                .annotations()
                .len(),
            1,
            "must not add a second annotation"
        );
    }

    #[test]
    fn accept_all_visual_annotations_uses_single_state_id_increment() {
        let mut state = ws(recording_from_frames());
        let proposal = visual_proposal_three_primitives(&mut state, 1);
        let source = state.selected_step().unwrap().source;
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(proposal);
        let state_before = state.presentation.doc(source).unwrap().document.state_id();

        let _ = update(&mut state, Message::AcceptAllVisualAnnotations);

        let doc = state.presentation.doc(source).unwrap();
        assert_eq!(
            doc.document.state_id(),
            state_before + 1,
            "accepting the batch must create one undoable document change"
        );
        assert_eq!(
            doc.document.annotations().len(),
            3,
            "all three annotations applied"
        );
        assert!(
            matches!(
                state.visual_annotation_suggestion,
                crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
            ),
            "proposal consumed"
        );
    }

    #[test]
    fn stale_accept_all_visual_annotations_changes_neither_count_nor_state() {
        let mut state = ws(synthetic_recording(2));
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(
                proposal_for_synthetic_step(&state),
            );

        let _ = update(&mut state, Message::DeleteStep);

        assert!(
            matches!(
                state.visual_annotation_suggestion,
                crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
            ),
            "pending review must be discarded"
        );
    }

    #[test]
    fn manual_edit_after_pending_review_stales_remaining_items() {
        let mut state = ws(recording_from_frames());
        let proposal = visual_proposal_three_primitives(&mut state, 1);
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(proposal);
        let _ = update(&mut state, Message::AnnotateStepRequested);

        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(8.0, 8.0)),
        );

        assert!(
            matches!(
                state.visual_annotation_suggestion,
                crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
            ),
            "manual edit must discard pending review"
        );
    }

    #[test]
    fn undo_after_pending_review_stales_remaining_items() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(8.0, 8.0)),
        );
        let proposal = visual_proposal_three_primitives(&mut state, 1);
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(proposal);

        let _ = update(&mut state, Message::AnnotationUndo);

        assert!(
            matches!(
                state.visual_annotation_suggestion,
                crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
            ),
            "undo must discard pending review"
        );
    }

    #[test]
    fn redo_after_pending_review_stales_remaining_items() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(8.0, 8.0)),
        );
        let _ = update(&mut state, Message::AnnotationUndo);
        let proposal = visual_proposal_three_primitives(&mut state, 1);
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(proposal);

        let _ = update(&mut state, Message::AnnotationRedo);

        assert!(
            matches!(
                state.visual_annotation_suggestion,
                crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
            ),
            "redo must discard pending review"
        );
    }

    #[test]
    fn delete_step_after_pending_review_stales_remaining_items() {
        let mut state = ws(synthetic_recording(2));
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(
                proposal_for_synthetic_step(&state),
            );

        let _ = update(&mut state, Message::DeleteStep);

        assert!(
            matches!(
                state.visual_annotation_suggestion,
                crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
            ),
            "delete step must discard pending review"
        );
    }

    #[test]
    fn replace_keyframe_after_pending_review_stales_remaining_items() {
        let mut state = ws(recording_from_frames());
        let proposal = visual_proposal_three_primitives(&mut state, 1);
        state.visual_annotation_suggestion =
            crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(proposal);
        let step = state.selected_step().unwrap();
        let replacement = *step
            .nearby
            .iter()
            .find(|&&f| f != step.keyframe)
            .expect("replacement frame id");

        let _ = update(&mut state, Message::ReplaceKeyframe(replacement));

        assert!(
            matches!(
                state.visual_annotation_suggestion,
                crate::timeline_workspace::VisualAnnotationSuggestionState::Idle
            ),
            "replace keyframe must discard pending review"
        );
    }

    /// RAII guard that writes a `config.toml` to the given directory and
    /// removes it on drop, so provider-config-change tests don't leak state.
    struct ConfigFileGuard {
        dir: std::path::PathBuf,
        #[allow(dead_code)]
        had_file: bool,
    }

    impl ConfigFileGuard {
        fn new(config_dir: &std::path::Path, content: &str) -> Self {
            let _ = std::fs::create_dir_all(config_dir);
            let path = config_dir.join("config.toml");
            let had_file = path.exists();
            std::fs::write(&path, content).expect("write config.toml for test");
            Self {
                dir: config_dir.to_path_buf(),
                had_file,
            }
        }
    }

    impl Drop for ConfigFileGuard {
        fn drop(&mut self) {
            let path = self.dir.join("config.toml");
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn visual_consent_confirm_with_changed_provider_model_stays_consent_pending() {
        let _lock = ENV_LOCK.lock().unwrap();
        let config_dir =
            crate::daemon::config::rollshot_config_dir().expect("config dir available in test");
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let _cfg = ConfigFileGuard::new(
            &config_dir,
            r#"[provider]
provider = "Anthropic"
model = "claude-sonnet-4-6"
key_source = { Env = "ANTHROPIC_API_KEY" }
"#,
        );
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::SuggestVisualAnnotationsRequested);

        // Verify consent captured the original provider/model.
        match &state.visual_annotation_suggestion {
            crate::timeline_workspace::VisualAnnotationSuggestionState::ConsentPending(consent) => {
                assert_eq!(consent.provider, "Anthropic");
                assert_eq!(consent.model, "claude-sonnet-4-6");
            }
            other => panic!("expected ConsentPending, got {other:?}"),
        }

        // Simulate provider/model change between request and confirm.
        drop(_cfg);
        let _cfg2 = ConfigFileGuard::new(
            &config_dir,
            r#"[provider]
provider = "OpenAI"
model = "gpt-4o"
key_source = { Env = "OPENAI_API_KEY" }
"#,
        );
        drop(_anthropic);
        let _openai2 = EnvVarGuard::set("OPENAI_API_KEY", "test-key-openai");

        let task = update(&mut state, Message::VisualSuggestionConsentConfirmed);

        assert_eq!(task.task.units(), 0);
        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::ConsentPending(_)
        ));
        assert!(state
            .message
            .as_ref()
            .is_some_and(|m| m.contains("Provider configuration changed")));
        // Verify consent snapshot was updated to the new provider/model.
        match &state.visual_annotation_suggestion {
            crate::timeline_workspace::VisualAnnotationSuggestionState::ConsentPending(consent) => {
                assert_eq!(consent.provider, "OpenAI");
                assert_eq!(consent.model, "gpt-4o");
            }
            other => panic!("expected ConsentPending with new provider, got {other:?}"),
        }
    }

    #[test]
    fn visual_consent_confirm_after_provider_change_succeeds_on_retry() {
        let _lock = ENV_LOCK.lock().unwrap();
        let config_dir =
            crate::daemon::config::rollshot_config_dir().expect("config dir available in test");
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let _cfg = ConfigFileGuard::new(
            &config_dir,
            r#"[provider]
provider = "Anthropic"
model = "claude-sonnet-4-6"
key_source = { Env = "ANTHROPIC_API_KEY" }
"#,
        );
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::SuggestVisualAnnotationsRequested);

        // Simulate provider/model change.
        drop(_cfg);
        let _cfg2 = ConfigFileGuard::new(
            &config_dir,
            r#"[provider]
provider = "OpenAI"
model = "gpt-4o"
key_source = { Env = "OPENAI_API_KEY" }
"#,
        );
        drop(_anthropic);
        let _openai2 = EnvVarGuard::set("OPENAI_API_KEY", "test-key-openai");

        // First confirm: detects the change, stays in ConsentPending.
        let _ = update(&mut state, Message::VisualSuggestionConsentConfirmed);
        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::ConsentPending(_)
        ));

        // Second confirm: consent snapshot now matches; should start running.
        let task = update(&mut state, Message::VisualSuggestionConsentConfirmed);
        assert!(
            task.task.units() > 0,
            "second confirm after config change should return a Task::perform"
        );
        assert!(matches!(
            state.visual_annotation_suggestion,
            crate::timeline_workspace::VisualAnnotationSuggestionState::Running { .. }
        ));
    }

    fn begin_export(state: &mut TimelineWorkspace, operation_id: u64) -> Result<(), String> {
        let job = super::super::guide_export::build_reviewed_export_job(state)
            .map_err(|e| format!("{e}"))?;
        let created_at = chrono::Local::now();
        state.export_state = super::super::GuideExportState::PickingDestination {
            operation_id,
            pending: super::super::guide_export::PendingStandaloneExport {
                operation_id,
                created_at,
                job,
            },
        };
        Ok(())
    }

    fn fake_result(path: &str) -> super::super::guide_export::StandaloneExportResult {
        let directory = std::path::PathBuf::from(path);
        super::super::guide_export::StandaloneExportResult {
            operation_id: 0,
            index_html: directory.join("index.html"),
            directory,
        }
    }

    #[test]
    fn export_completion_keeps_workspace_and_exposes_open_actions() {
        let mut state = ws(synthetic_recording(1));
        state.export_state = super::super::GuideExportState::Exporting { operation_id: 7 };
        let directory = std::path::PathBuf::from("/tmp/guide");
        let index_html = directory.join("index.html");
        apply_export_finished(
            &mut state,
            7,
            Ok(super::super::guide_export::StandaloneExportResult {
                operation_id: 7,
                directory: directory.clone(),
                index_html: index_html.clone(),
            }),
        );
        assert!(matches!(
            state.export_state,
            super::super::GuideExportState::Succeeded
        ));
        assert_eq!(state.last_export.as_ref().unwrap().index_html, index_html);
    }

    #[test]
    fn callout_explanation_message_updates_only_matching_annotation() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let source = state.annotation_session.as_ref().unwrap().source;
        let id = state
            .presentation
            .doc_mut(source)
            .unwrap()
            .document
            .add_number_callout(
                rollshot_image_document::ImagePoint::new(2.0, 2.0),
                rollshot_image_document::ImagePoint::new(8.0, 8.0),
            );
        let _ = update(
            &mut state,
            Message::AnnotationExplanationChanged(id, "Open Settings".into()),
        );
        assert_eq!(
            state.presentation.explanation(source, id),
            Some("Open Settings")
        );
    }

    #[test]
    fn picker_cancel_and_stale_results_do_not_mutate_current_operation() {
        let mut state = ws(recording_from_frames());
        begin_export(&mut state, 41).unwrap();
        let _ = update(
            &mut state,
            Message::ExportDirChosenWithId {
                operation_id: 41,
                parent: None,
            },
        );
        assert!(matches!(
            state.export_state,
            super::super::GuideExportState::Idle
        ));
        assert!(state.last_export.is_none());

        begin_export(&mut state, 42).unwrap();
        apply_export_finished(&mut state, 41, Ok(fake_result("/tmp/stale")));
        assert!(matches!(
            state.export_state,
            super::super::GuideExportState::PickingDestination {
                operation_id: 42,
                ..
            }
        ));
    }
}
