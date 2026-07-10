use std::path::{Path, PathBuf};

use iced::Task;
use rollshot_action::{
    export_gif, export_guide, export_video, render_storyboard_steps, GifOptions, StoryboardError,
    StoryboardOptions, StoryboardRenderResult, StoryboardStep, VideoOptions,
};
use rollshot_image_document::ImagePoint;

use super::TimelineWorkspace;

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
    ExportDirChosen(Option<PathBuf>),
    ExportGifRequested,
    ExportGifPathChosen(Option<PathBuf>),
    ExportStoryboardRequested,
    ExportStoryboardPathChosen(Option<PathBuf>),
    PreviewStoryboardRequested,
    PreviewStoryboardClosed,
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
    IssuePackFolderChosen(Option<PathBuf>),
    /// Background Issue Pack export completed.
    IssuePackFinished(Result<crate::issue_pack::IssuePackExportResult, String>),
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
    /// Begin an agent callout suggestion run for the currently selected step.
    /// The view wires this in Task 8; Task 7 only verifies the state machine
    /// and the late-result race protection.
    #[allow(dead_code)]
    SuggestCalloutRequested,
    /// Cancel the in-flight callout suggestion run and transition to Idle.
    #[allow(dead_code)]
    CancelCalloutSuggestion,
    /// Reject the pending callout proposal and discard it.
    #[allow(dead_code)]
    RejectCalloutSuggestion,
    /// Accept the pending callout proposal and commit it as an annotation.
    #[allow(dead_code)]
    AcceptCalloutSuggestion,
    /// Background callout suggestion run completed. The `run_id` must match
    /// the current `Running` state; otherwise the result is dropped (late).
    #[allow(dead_code)]
    CalloutSuggestionLoaded {
        run_id: u64,
        result: Result<super::callout_agent::CalloutTaskResult, String>,
    },
}

pub fn update(state: &mut TimelineWorkspace, message: Message) -> Task<Message> {
    match message {
        Message::SelectStep(index) => {
            if state.guide.steps().iter().any(|s| s.index == index) {
                state.selected = Some(index);
                state.rebuild_selection_handles();
            }
            Task::none()
        }
        Message::TitleChanged(title) => {
            if let Some(index) = state.selected {
                state.guide.rename(index, title);
            }
            Task::none()
        }
        Message::CaptionChanged(caption) => {
            if let Some(index) = state.selected {
                state.guide.set_caption(index, caption);
            }
            Task::none()
        }
        Message::DeleteStep => {
            let deleted_source = state.selected_step().map(|step| step.source);
            if let Some(index) = state.selected {
                if state.guide.delete(index) {
                    let len = state.guide.steps().len();
                    state.selected = if len == 0 { None } else { Some(index.min(len)) };
                    state.rebuild_selection_handles();
                }
            }
            if let Some(source) = deleted_source {
                state.presentation.clear_for_source(source);
            }
            state
                .presentation
                .retain_sources(state.guide.steps().iter().map(|step| step.source));
            Task::none()
        }
        Message::ReplaceKeyframe(frame) => {
            if let Some(index) = state.selected {
                let source = state.selected_step().map(|step| step.source);
                if state.guide.replace_keyframe(index, frame) {
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
            // Replacing the keyframe discards any pending callout: the
            // document is re-built, the keyframe no longer matches, and any
            // user-pending proposal would target a stale frame.
            match &state.callout_suggestion {
                super::CalloutSuggestionState::Running { cancellation, .. } => {
                    cancellation.cancel();
                }
                super::CalloutSuggestionState::Pending(_)
                | super::CalloutSuggestionState::NoSuggestion { .. }
                | super::CalloutSuggestionState::Failed { .. } => {}
                super::CalloutSuggestionState::Idle => {}
            }
            state.callout_suggestion = super::CalloutSuggestionState::Idle;
            state.callout_agent_run_id = 0;
            Task::none()
        }
        Message::DiscardRequested | Message::CloseRequested => {
            state.pending_discard = true;
            Task::none()
        }
        Message::CancelDiscard => {
            state.pending_discard = false;
            Task::none()
        }
        Message::ConfirmDiscard => {
            state.pending_discard = false;
            iced::exit()
        }
        Message::ExportRequested => {
            state.message = None;
            Task::perform(
                pick_export_dir(picker_default_dir()),
                Message::ExportDirChosen,
            )
        }
        Message::ExportDirChosen(None) => Task::none(),
        Message::ExportDirChosen(Some(dir)) => match export_to(state, &dir) {
            Ok(out) => {
                tracing::info!(
                    target: "rollshot::action::export",
                    path = %out.display(),
                    "guide exported"
                );
                state.message = None;
                iced::exit()
            }
            Err(error) => {
                tracing::error!(
                    target: "rollshot::action::export",
                    %error,
                    "guide export failed"
                );
                state.message = Some(error);
                Task::none()
            }
        },
        Message::ExportGifRequested => {
            state.message = None;
            Task::perform(
                pick_gif_save_path(picker_default_dir()),
                Message::ExportGifPathChosen,
            )
        }
        Message::ExportGifPathChosen(None) => Task::none(),
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
            Task::none()
        }
        Message::PreviewStoryboardRequested => {
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
            Task::none()
        }
        Message::PreviewStoryboardClosed => {
            state.storyboard_preview = None;
            Task::none()
        }
        Message::ExportStoryboardRequested => {
            state.message = None;
            state.storyboard_preview = None;
            Task::perform(
                pick_storyboard_save_path(picker_default_dir()),
                Message::ExportStoryboardPathChosen,
            )
        }
        Message::ExportStoryboardPathChosen(None) => Task::none(),
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
            Task::none()
        }
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
        Message::IssuePackExportFolder => {
            begin_issue_pack_export(state, super::IssuePackKind::Folder)
        }
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
                super::IssuePackKind::Folder => crate::issue_pack::export_folder_with_action_guide(
                    &input,
                    Some(action),
                    &parent,
                ),
                super::IssuePackKind::Zip => {
                    crate::issue_pack::export_zip_with_action_guide(&input, Some(action), &parent)
                }
            };
            update(
                state,
                Message::IssuePackFinished(result.map_err(|e| e.to_string())),
            )
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
        #[cfg(target_os = "macos")]
        Message::OpenInputMonitoringSettings => {
            rollshot_macos_input::open_input_monitoring_settings();
            state.message = Some("Grant Input Monitoring, then restart Rollshot.".to_string());
            Task::none()
        }
        Message::DismissBanner => {
            state.message = None;
            Task::none()
        }
        Message::AnnotateStepRequested => {
            state.message = None;
            let Some(step) = state.selected_step().cloned() else {
                state.message = Some("Select a step before annotating.".to_string());
                return Task::none();
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
            Task::none()
        }
        Message::AnnotationToolChanged(tool) => {
            if let Some(session) = &mut state.annotation_session {
                session.tool = tool;
                session.draft = None;
            }
            Task::none()
        }
        Message::AnnotationTextChanged(text) => {
            if let Some(session) = &mut state.annotation_session {
                session.text_note = text;
            }
            Task::none()
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
            Task::none()
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
            Task::none()
        }
        Message::AnnotationCanvasReleased(point) => {
            commit_annotation_release(state, point);
            Task::none()
        }
        Message::AnnotationUndo => {
            with_annotation_document(state, |doc| {
                doc.document.undo();
            });
            Task::none()
        }
        Message::AnnotationRedo => {
            with_annotation_document(state, |doc| {
                doc.document.redo();
            });
            Task::none()
        }
        Message::AnnotationDone => {
            // Closing the annotation session must also clear any pending
            // callout state: an accepted or rejected proposal is no longer
            // reachable, and a still-running run's late result would be
            // dropped by the run_id guard anyway. Reset both the state and
            // the local run id so a fresh request gets a fresh run id.
            state.callout_suggestion = super::CalloutSuggestionState::Idle;
            state.callout_agent_run_id = 0;
            state.annotation_session = None;
            Task::none()
        }
        Message::AnnotationCancel => {
            // The scrim and the explicit "Close" button both arrive here. If
            // a callout run is in flight, cancel it first so the async task
            // exits cleanly. A pending proposal is discarded — the user
            // chose to close the modal, so any in-flight review is dropped.
            match &state.callout_suggestion {
                super::CalloutSuggestionState::Running { cancellation, .. } => {
                    cancellation.cancel();
                }
                super::CalloutSuggestionState::Pending(_)
                | super::CalloutSuggestionState::NoSuggestion { .. }
                | super::CalloutSuggestionState::Failed { .. } => {}
                super::CalloutSuggestionState::Idle => {}
            }
            state.callout_suggestion = super::CalloutSuggestionState::Idle;
            state.callout_agent_run_id = 0;
            state.annotation_session = None;
            Task::none()
        }
        Message::CaptionProposalLoaded(Ok(proposal)) => {
            state.caption_suggestions_running = false;
            state.caption_proposal = Some(proposal);
            state.message = Some("Caption suggestions ready for review.".to_string());
            Task::none()
        }
        Message::CaptionProposalLoaded(Err(error)) => {
            state.caption_suggestions_running = false;
            state.message = Some(format!("Caption suggestions failed: {error}"));
            Task::none()
        }
        Message::AcceptCaptionSuggestion(id) => {
            let Some(proposal) = &mut state.caption_proposal else {
                return Task::none();
            };
            match proposal.apply(&mut state.guide, id) {
                rollshot_action::CaptionApplyOutcome::Applied => {
                    state.message = Some("Caption suggestion accepted.".to_string());
                }
                rollshot_action::CaptionApplyOutcome::Stale => {
                    state.message =
                        Some("Caption suggestion is stale; regenerate suggestions.".to_string());
                }
                rollshot_action::CaptionApplyOutcome::Missing
                | rollshot_action::CaptionApplyOutcome::NotPending => {}
            }
            Task::none()
        }
        Message::RejectCaptionSuggestion(id) => {
            if let Some(proposal) = &mut state.caption_proposal {
                proposal.reject(id);
            }
            Task::none()
        }
        Message::AcceptAllCaptionSuggestions => {
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
                state.message = Some(match stale {
                    0 => format!("Accepted {applied} caption suggestions."),
                    _ => format!(
                        "Accepted {applied} caption suggestions; {stale} stale suggestions skipped."
                    ),
                });
            }
            Task::none()
        }
        Message::DismissCaptionProposal => {
            state.caption_proposal = None;
            Task::none()
        }
        Message::SuggestCaptionsRequested => {
            if state.caption_suggestions_running {
                return Task::none();
            }
            if state.guide.is_empty() {
                state.message = Some("No reviewed steps to caption.".to_string());
                return Task::none();
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
                    return Task::none();
                }
            };
            if !crate::result_workspace::workbench::has_key(&cfg) {
                state.message =
                    Some("Configure an agent provider before suggesting captions.".to_string());
                return Task::none();
            }
            let model = cfg.model.clone();
            let adapter = match crate::result_workspace::workbench::build_adapter(&cfg) {
                Ok(adapter) => adapter,
                Err(error) => {
                    state.message = Some(format!("Caption suggestions failed: {error}"));
                    return Task::none();
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
            Task::perform(
                super::caption_agent::suggest_captions_task(run_id, model, adapter, guide),
                Message::CaptionProposalLoaded,
            )
        }
        Message::SuggestCalloutRequested => {
            if matches!(
                state.callout_suggestion,
                super::CalloutSuggestionState::Running { .. }
            ) {
                return Task::none();
            }
            let Some(step) = state.selected_step().cloned() else {
                state.message = Some("Select a step before suggesting a callout.".to_string());
                return Task::none();
            };
            let Some(doc) = state.presentation.document_for_step(&step, &state.store) else {
                state.message = Some(
                    "Cannot suggest a callout because the keyframe is unavailable.".to_string(),
                );
                return Task::none();
            };
            let image = doc.document.source().clone();
            let document_state_id = doc.document.state_id();
            let cfg = match crate::daemon::config::rollshot_config_dir()
                .map_err(|_| "Rollshot config directory is unavailable.".to_string())
                .and_then(|dir| crate::result_workspace::workbench::load_provider_config(&dir))
            {
                Ok(cfg) => cfg,
                Err(error) => {
                    state.message = Some(format!("Callout suggestion failed: {error}"));
                    return Task::none();
                }
            };
            if !crate::result_workspace::workbench::has_key(&cfg) {
                state.message =
                    Some("Configure an agent provider before suggesting a callout.".to_string());
                return Task::none();
            }
            let provider_name = format!("{}", cfg.provider);
            let model = cfg.model.clone();
            let adapter = match crate::result_workspace::workbench::build_adapter(&cfg) {
                Ok(adapter) => adapter,
                Err(error) => {
                    state.message = Some(format!("Callout suggestion failed: {error}"));
                    return Task::none();
                }
            };
            state.callout_agent_run_id = state.callout_agent_run_id.saturating_add(1);
            let run_id = state.callout_agent_run_id;
            let cancellation = rollshot_agent::runtime::RunCancellation::new();
            let task_cancellation = cancellation.clone();
            state.callout_suggestion = super::CalloutSuggestionState::Running {
                run_id,
                cancellation,
            };
            state.message = Some("Suggesting callout...".to_string());
            tracing::info!(
                target: "rollshot::action::callout_agent",
                run_id,
                step_index = step.index,
                keyframe = step.keyframe,
                "callout suggestion run started"
            );
            let input = super::callout_agent::CalloutTaskInput {
                run_id,
                step,
                document_state_id,
                image,
            };
            Task::perform(
                super::callout_agent::suggest_callout_task(
                    input,
                    provider_name,
                    model,
                    adapter,
                    task_cancellation,
                ),
                move |result| Message::CalloutSuggestionLoaded { run_id, result },
            )
        }
        Message::CalloutSuggestionLoaded { run_id, result } => {
            // Only accept the result if the current state is Running with the
            // matching run_id. This is the late-result race protection: a
            // cancelled or timed-out older run must not overwrite a newer
            // proposal.
            let expected_id = match &state.callout_suggestion {
                super::CalloutSuggestionState::Running { run_id, .. } => Some(*run_id),
                _ => None,
            };
            if expected_id != Some(run_id) {
                tracing::debug!(
                    target: "rollshot::action::callout_agent",
                    late_run_id = run_id,
                    expected_run_id = ?expected_id,
                    "ignoring late callout suggestion result"
                );
                return Task::none();
            }
            match result {
                Ok(super::callout_agent::CalloutTaskResult::Proposal(proposal)) => {
                    tracing::info!(
                        target: "rollshot::action::callout_agent",
                        run_id,
                        "callout suggestion ready for review"
                    );
                    state.callout_suggestion = super::CalloutSuggestionState::Pending(proposal);
                    state.message = Some("Callout suggestion ready for review.".to_string());
                }
                Ok(super::callout_agent::CalloutTaskResult::NoSuggestion { reason }) => {
                    let message = match &reason {
                        Some(text) => format!("Callout suggestion: {text}"),
                        None => "Callout suggestion: no suggestion returned.".to_string(),
                    };
                    state.callout_suggestion =
                        super::CalloutSuggestionState::NoSuggestion { reason };
                    state.message = Some(message);
                }
                Err(message) => {
                    tracing::error!(
                        target: "rollshot::action::callout_agent",
                        run_id,
                        error = %message,
                        "callout suggestion failed"
                    );
                    state.callout_suggestion = super::CalloutSuggestionState::Failed { message };
                    state.message = Some(
                        "Callout suggestion failed. See the annotation modal for details."
                            .to_string(),
                    );
                }
            }
            Task::none()
        }
        Message::CancelCalloutSuggestion => {
            if let super::CalloutSuggestionState::Running {
                run_id,
                cancellation,
            } = &state.callout_suggestion
            {
                tracing::info!(
                    target: "rollshot::action::callout_agent",
                    run_id,
                    "callout suggestion cancelled by user"
                );
                cancellation.cancel();
                state.callout_suggestion = super::CalloutSuggestionState::Idle;
                state.message = Some("Callout suggestion cancelled.".to_string());
            }
            Task::none()
        }
        Message::RejectCalloutSuggestion => {
            if let super::CalloutSuggestionState::Pending(mut proposal) = std::mem::replace(
                &mut state.callout_suggestion,
                super::CalloutSuggestionState::Idle,
            ) {
                // `reject()` returns `false` if the proposal is no longer
                // pending; the user explicitly chose to discard it, so any
                // state mismatch is informational and the tip is dropped.
                let _ = proposal.reject();
            }
            state.message = Some("Callout suggestion rejected.".to_string());
            Task::none()
        }
        Message::AcceptCalloutSuggestion => {
            let Some(step) = state.selected_step().cloned() else {
                state.message = Some("Select a step before accepting a callout.".to_string());
                return Task::none();
            };
            let Some(doc) = state.presentation.document_for_step(&step, &state.store) else {
                state.message = Some(
                    "Cannot accept a callout because the keyframe is unavailable.".to_string(),
                );
                return Task::none();
            };
            let state_id = doc.document.state_id();
            let image = doc.document.source();
            let image_width = image.width();
            let image_height = image.height();
            let super::CalloutSuggestionState::Pending(mut proposal) = std::mem::replace(
                &mut state.callout_suggestion,
                super::CalloutSuggestionState::Idle,
            ) else {
                return Task::none();
            };
            match proposal.validate_acceptance(Some(&step), state_id, image_width, image_height) {
                rollshot_action::CalloutApplyOutcome::Ready => {
                    let tip = proposal.suggestion.tip;
                    let bubble = rollshot_image_document::place_number_callout_bubble(
                        tip,
                        image_width,
                        image_height,
                        doc.document.annotations(),
                    );
                    doc.document.add_number_callout(tip, bubble);
                    proposal.mark_applied();
                    state.message = Some("Callout suggestion accepted.".to_string());
                }
                rollshot_action::CalloutApplyOutcome::Missing => {
                    state.message =
                        Some("Callout suggestion is stale; the step is missing.".to_string());
                }
                rollshot_action::CalloutApplyOutcome::Stale => {
                    state.callout_suggestion = super::CalloutSuggestionState::Failed {
                        message: "Callout suggestion is stale; regenerate it.".to_string(),
                    };
                    state.message = Some("Callout suggestion is stale; regenerate it.".to_string());
                    return Task::none();
                }
                rollshot_action::CalloutApplyOutcome::NotPending => {
                    state.message = Some("Callout suggestion is no longer pending.".to_string());
                }
            }
            Task::none()
        }
        Message::FfmpegSetupCancel => {
            state.ffmpeg_setup = None;
            Task::none()
        }
        Message::FfmpegUseSystem => {
            state.ffmpeg_setup = None;
            state.message = Some(
                "Install FFmpeg or set ROLLSHOT_FFMPEG, then try Export MP4 again.".to_string(),
            );
            Task::none()
        }
        Message::ExportMp4Requested => {
            state.message = None;
            match crate::managed_ffmpeg::resolve_ffmpeg() {
                crate::managed_ffmpeg::FfmpegResolution::Available(_) => Task::perform(
                    pick_mp4_save_path(picker_default_dir()),
                    Message::ExportMp4PathChosen,
                ),
                crate::managed_ffmpeg::FfmpegResolution::NeedsSetup(info) => {
                    state.ffmpeg_setup = Some(super::FfmpegSetupDialog {
                        info,
                        downloading: false,
                    });
                    Task::none()
                }
            }
        }
        Message::ExportMp4PathChosen(None) => Task::none(),
        Message::ExportMp4PathChosen(Some(path)) => {
            let ffmpeg = match crate::managed_ffmpeg::resolve_ffmpeg() {
                crate::managed_ffmpeg::FfmpegResolution::Available(path) => path,
                crate::managed_ffmpeg::FfmpegResolution::NeedsSetup(info) => {
                    state.ffmpeg_setup = Some(super::FfmpegSetupDialog {
                        info,
                        downloading: false,
                    });
                    return Task::none();
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
            Task::none()
        }
        Message::FfmpegDownloadManaged => {
            let Some(dialog) = &mut state.ffmpeg_setup else {
                return Task::none();
            };
            if dialog.downloading || dialog.info.managed_download.is_none() {
                return Task::none();
            }
            dialog.downloading = true;
            Task::perform(
                download_managed_ffmpeg_task(),
                Message::FfmpegDownloadFinished,
            )
        }
        Message::FfmpegDownloadFinished(Ok(path)) => {
            state.ffmpeg_setup = None;
            state.message = Some(format!("Managed FFmpeg installed at {}", path.display()));
            Task::perform(
                pick_mp4_save_path(picker_default_dir()),
                Message::ExportMp4PathChosen,
            )
        }
        Message::FfmpegDownloadFinished(Err(error)) => {
            if let Some(dialog) = &mut state.ffmpeg_setup {
                dialog.downloading = false;
            }
            state.message = Some(format!("Managed FFmpeg download failed: {error}"));
            Task::none()
        }
    }
}

pub fn subscription(_state: &TimelineWorkspace) -> iced::Subscription<Message> {
    iced::window::close_requests().map(|_id| Message::CloseRequested)
}

/// Export the (possibly edited) guide into `out_dir/action-guide/`.
fn export_to(state: &TimelineWorkspace, out_dir: &Path) -> Result<PathBuf, String> {
    export_guide(
        &state.guide,
        &state.store,
        state.region,
        state.capability,
        state.source_kind,
        out_dir,
    )
    .map_err(|e| format!("export failed: {e}"))
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
    if state.guide.is_empty() {
        return Err(StoryboardError::Empty);
    }

    let mut images = Vec::with_capacity(state.guide.steps().len());
    for (i, step) in state.guide.steps().iter().enumerate() {
        let frame = state
            .store
            .retained(step.keyframe)
            .ok_or(StoryboardError::KeyframeMissing { index: i + 1 })?;
        let image = match state.presentation.doc(step.source) {
            Some(doc)
                if doc.keyframe == step.keyframe && !doc.document.annotations().is_empty() =>
            {
                doc.document.flatten()
            }
            _ => frame.image.clone(),
        };
        images.push(image);
    }

    let steps: Vec<_> = state
        .guide
        .steps()
        .iter()
        .zip(images.iter())
        .map(|(step, image)| StoryboardStep {
            index: step.index,
            title: &step.title,
            caption: {
                let caption = step.caption.trim();
                (!caption.is_empty()).then_some(caption)
            },
            image,
        })
        .collect();

    render_storyboard_steps(&steps, opts)
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

async fn download_managed_ffmpeg_task() -> Result<PathBuf, String> {
    tokio::task::spawn_blocking(crate::managed_ffmpeg::download_managed_ffmpeg)
        .await
        .map_err(|error| format!("managed FFmpeg download task failed: {error}"))?
}

fn timeline_issue_pack_input(state: &TimelineWorkspace) -> crate::issue_pack::IssuePackInput {
    let include_gif = state
        .issue_pack
        .as_ref()
        .is_some_and(|dialog| dialog.include_gif);
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
    let include_gif = state
        .issue_pack
        .as_ref()
        .is_some_and(|dialog| dialog.include_gif);
    let storyboard_image = render_timeline_storyboard(state, StoryboardOptions::default())
        .ok()
        .map(|r| r.image);
    crate::issue_pack::ActionGuideExportSource {
        guide: &state.guide,
        store: &state.store,
        region: state.region,
        capability: state.capability,
        source_kind: state.source_kind,
        include_gif,
        storyboard_image,
    }
}

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

fn clamp_annotation_point(point: ImagePoint, width: u32, height: u32) -> ImagePoint {
    point.clamp_to(width, height)
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline_workspace::tests::{recording_from_frames, synthetic_recording};
    use crate::timeline_workspace::{
        annotation::AnnotationTool, FfmpegSetupDialog, TimelineWorkspace,
    };
    use rollshot_action::{CaptureRegion, InputCapability, InputSourceKind};
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
    fn delete_last_remaining_step_clears_selection() {
        let mut state = ws(synthetic_recording(1));
        let _ = update(&mut state, Message::DeleteStep);
        assert!(state.guide.steps().is_empty());
        assert_eq!(state.selected, None);
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
    fn export_dir_chosen_writes_guide_folder_and_clears_message() {
        let mut state = ws(recording_from_frames());
        state.message = Some("stale".to_string());
        let tmp = tempfile::tempdir().unwrap();
        let _ = update(
            &mut state,
            Message::ExportDirChosen(Some(tmp.path().to_path_buf())),
        );
        assert!(tmp.path().join("action-guide/steps.md").exists());
        assert!(tmp.path().join("action-guide/session.json").exists());
        assert!(
            state.message.is_none(),
            "successful export clears the banner"
        );
    }

    #[test]
    fn export_empty_guide_sets_error_and_writes_nothing() {
        let mut state = ws(synthetic_recording(0));
        let tmp = tempfile::tempdir().unwrap();
        let _ = update(
            &mut state,
            Message::ExportDirChosen(Some(tmp.path().to_path_buf())),
        );
        assert!(
            !tmp.path().join("action-guide").exists(),
            "empty guide must not write a folder"
        );
        assert!(
            state.message.is_some(),
            "export failure surfaces an inline message"
        );
    }

    #[test]
    fn export_cancelled_picker_is_a_no_op() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::ExportDirChosen(None));
        assert!(state.message.is_none());
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
        let _ = update(
            &mut state,
            Message::IssuePackFolderChosen(Some(tmp.path().to_path_buf())),
        );

        assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
        assert!(state.message.as_ref().unwrap().contains("review"));
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
        let _ = update(
            &mut state,
            Message::IssuePackFolderChosen(Some(tmp.path().to_path_buf())),
        );

        let pack = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("rollshot-issue-pack-")
            })
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

        assert_eq!(task.units(), 0);
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
        });
        let before = {
            let action = timeline_issue_pack_action(&state);
            action
                .storyboard_image
                .expect("storyboard before annotation")
        };

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

        let action = timeline_issue_pack_action(&state);

        assert!(action.storyboard_image.is_some());
        let image = action.storyboard_image.as_ref().unwrap();
        assert_ne!(
            before.as_raw(),
            image.as_raw(),
            "Issue Pack storyboard image should use flattened annotated keyframes"
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

        let input = timeline_issue_pack_input(&state);
        let first_step = &input.action_guide.as_ref().unwrap().steps[0];

        assert_eq!(first_step.title, "Open Preferences");
        assert_eq!(
            first_step.caption.as_deref(),
            Some("The preferences window is opened for configuration.")
        );
    }

    // ---------- Callout suggestion flow ----------

    /// Build a valid CalloutProposal for the workspace's currently selected
    /// step, using the presentation document's `state_id` and image
    /// dimensions. Used by the focused update tests to drive the late-result
    /// race and accept/reject paths.
    fn callout_proposal(state: &mut TimelineWorkspace) -> rollshot_action::CalloutProposal {
        let tip = callout_default_tip(state);
        callout_proposal_with_run_and_tip(state, 1, tip)
    }

    fn callout_proposal_with_run(
        state: &mut TimelineWorkspace,
        run_id: u64,
    ) -> rollshot_action::CalloutProposal {
        let tip = callout_default_tip(state);
        callout_proposal_with_run_and_tip(state, run_id, tip)
    }

    fn callout_default_tip(state: &mut TimelineWorkspace) -> rollshot_image_document::ImagePoint {
        let step = state.selected_step().cloned().expect("selected step");
        let doc = state
            .presentation
            .document_for_step(&step, &state.store)
            .expect("presentation document");
        let image = doc.document.source();
        rollshot_image_document::ImagePoint::new(
            (image.width() as f32 / 4.0).max(0.5),
            (image.height() as f32 / 4.0).max(0.5),
        )
    }

    fn callout_proposal_with_run_and_tip(
        state: &mut TimelineWorkspace,
        run_id: u64,
        tip: rollshot_image_document::ImagePoint,
    ) -> rollshot_action::CalloutProposal {
        let step = state.selected_step().cloned().expect("selected step");
        let doc = state
            .presentation
            .document_for_step(&step, &state.store)
            .expect("presentation document");
        let image = doc.document.source();
        rollshot_action::CalloutProposal::from_agent_draft(
            rollshot_action::CalloutProposalId(run_id),
            run_id,
            &step,
            doc.document.state_id(),
            image.width(),
            image.height(),
            rollshot_action::CalloutSuggestionDraft {
                tip,
                confidence: 0.75,
                rationale: Some("test rationale".to_string()),
            },
        )
        .expect("valid proposal")
    }

    #[test]
    fn new_workspace_has_idle_callout_state() {
        let state = ws(synthetic_recording(1));
        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Idle
        ));
        assert_eq!(state.callout_agent_run_id, 0);
    }

    #[test]
    fn suggest_callout_without_selection_sets_recoverable_message() {
        let mut state = ws(synthetic_recording(0));
        // Ensure no provider key so the no-selection path is reached first.
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::remove("ANTHROPIC_API_KEY");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");

        let _ = update(&mut state, Message::SuggestCalloutRequested);

        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Idle
        ));
        assert!(state
            .message
            .as_ref()
            .is_some_and(|m| m.contains("Select a step")));
    }

    #[test]
    fn suggest_callout_without_provider_key_shows_recoverable_message() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::remove("ANTHROPIC_API_KEY");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());

        let _ = update(&mut state, Message::SuggestCalloutRequested);

        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Idle
        ));
        assert_eq!(state.callout_agent_run_id, 0);
        assert!(state
            .message
            .as_ref()
            .is_some_and(|m| m.contains("Configure an agent provider")));
    }

    #[test]
    fn suggest_callout_while_running_is_a_no_op() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        state.callout_agent_run_id = 7;
        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Running {
            run_id: 7,
            cancellation: rollshot_agent::runtime::RunCancellation::new(),
        };

        let task = update(&mut state, Message::SuggestCalloutRequested);

        assert_eq!(task.units(), 0);
        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Running { run_id: 7, .. }
        ));
        assert_eq!(state.callout_agent_run_id, 7);
    }

    #[test]
    fn callout_suggestion_loaded_proposal_stores_pending() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        // Replay the SuggestCalloutRequested so state transitions to Running.
        let _ = update(&mut state, Message::SuggestCalloutRequested);
        let run_id = state.callout_agent_run_id;
        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Running { .. }
        ));

        let proposal = callout_proposal(&mut state);
        let result = Ok(super::super::callout_agent::CalloutTaskResult::Proposal(
            proposal,
        ));
        let _ = update(
            &mut state,
            Message::CalloutSuggestionLoaded { run_id, result },
        );

        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Pending(_)
        ));
        assert!(state
            .message
            .as_ref()
            .is_some_and(|m| m.contains("ready for review")));
    }

    #[test]
    fn callout_suggestion_loaded_no_suggestion_stores_no_suggestion() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let _ = update(&mut state, Message::SuggestCalloutRequested);
        let run_id = state.callout_agent_run_id;

        let result = Ok(
            super::super::callout_agent::CalloutTaskResult::NoSuggestion {
                reason: Some("no clear target".to_string()),
            },
        );
        let _ = update(
            &mut state,
            Message::CalloutSuggestionLoaded { run_id, result },
        );

        match &state.callout_suggestion {
            crate::timeline_workspace::CalloutSuggestionState::NoSuggestion { reason } => {
                assert_eq!(reason.as_deref(), Some("no clear target"));
            }
            other => panic!("expected NoSuggestion, got {other:?}"),
        }
    }

    #[test]
    fn callout_suggestion_loaded_failure_stores_failed() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let _ = update(&mut state, Message::SuggestCalloutRequested);
        let run_id = state.callout_agent_run_id;

        let result: Result<_, String> = Err("callout provider failed".to_string());
        let _ = update(
            &mut state,
            Message::CalloutSuggestionLoaded { run_id, result },
        );

        match &state.callout_suggestion {
            crate::timeline_workspace::CalloutSuggestionState::Failed { message } => {
                assert_eq!(message, "callout provider failed");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn cancel_callout_suggestion_resets_state() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let _ = update(&mut state, Message::SuggestCalloutRequested);
        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Running { .. }
        ));

        let _ = update(&mut state, Message::CancelCalloutSuggestion);

        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Idle
        ));
    }

    #[test]
    fn cancel_callout_suggestion_when_idle_is_a_no_op() {
        let mut state = ws(recording_from_frames());

        let _ = update(&mut state, Message::CancelCalloutSuggestion);

        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Idle
        ));
    }

    #[test]
    fn replacing_keyframe_during_running_callout_cancels() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let _ = update(&mut state, Message::SuggestCalloutRequested);
        let run_id = state.callout_agent_run_id;
        let step = state.selected_step().unwrap();
        let replacement = *step
            .nearby
            .iter()
            .find(|&&f| f != step.keyframe)
            .expect("replacement frame id");

        let _ = update(&mut state, Message::ReplaceKeyframe(replacement));

        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Idle
        ));
        assert_eq!(state.callout_agent_run_id, 0);
        // The old run's result is ignored because the state was reset to Idle.
        let proposal = callout_proposal(&mut state);
        let result = Ok(super::super::callout_agent::CalloutTaskResult::Proposal(
            proposal,
        ));
        let _ = update(
            &mut state,
            Message::CalloutSuggestionLoaded {
                run_id,
                result: result.clone(),
            },
        );
        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Idle
        ));
    }

    #[test]
    fn replacing_keyframe_discards_pending_callout() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Pending(
            callout_proposal(&mut state),
        );
        let step = state.selected_step().unwrap();
        let replacement = *step
            .nearby
            .iter()
            .find(|&&f| f != step.keyframe)
            .expect("replacement frame id");

        let _ = update(&mut state, Message::ReplaceKeyframe(replacement));

        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Idle
        ));
    }

    #[test]
    fn stale_callout_run_completion_cannot_replace_newer_run() {
        let mut state = ws(recording_from_frames());
        state.callout_agent_run_id = 2;
        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Running {
            run_id: 2,
            cancellation: rollshot_agent::runtime::RunCancellation::new(),
        };

        let old = Ok(super::super::callout_agent::CalloutTaskResult::Proposal(
            callout_proposal_with_run(&mut state, 1),
        ));
        let _ = update(
            &mut state,
            Message::CalloutSuggestionLoaded {
                run_id: 1,
                result: old,
            },
        );

        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Running { run_id: 2, .. }
        ));
    }

    #[test]
    fn document_edit_makes_pending_callout_stale() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let initial_state_id = state
            .presentation
            .doc(state.selected_step().unwrap().source)
            .unwrap()
            .document
            .state_id();
        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Pending(
            callout_proposal(&mut state),
        );

        // Edit the document so the proposal's captured `state_id` is stale.
        let _ = update(
            &mut state,
            Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(8.0, 8.0)),
        );
        let new_state_id = state
            .presentation
            .doc(state.selected_step().unwrap().source)
            .unwrap()
            .document
            .state_id();
        assert_ne!(initial_state_id, new_state_id);

        let _ = update(&mut state, Message::AcceptCalloutSuggestion);

        // The proposal was stale; the document was not mutated.
        match &state.callout_suggestion {
            crate::timeline_workspace::CalloutSuggestionState::Failed { message } => {
                assert!(message.contains("stale"), "message = {message}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        let doc = state
            .presentation
            .doc(state.selected_step().unwrap().source)
            .unwrap();
        // The manual annotation should be the only one — accept did NOT add a
        // callout on top of the stale proposal.
        assert_eq!(doc.document.annotations().len(), 1);
    }

    #[test]
    fn reject_callout_suggestion_does_not_mutate_document() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let annotations_before = state
            .presentation
            .doc(state.selected_step().unwrap().source)
            .unwrap()
            .document
            .annotations()
            .len();
        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Pending(
            callout_proposal(&mut state),
        );

        let _ = update(&mut state, Message::RejectCalloutSuggestion);

        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Idle
        ));
        let annotations_after = state
            .presentation
            .doc(state.selected_step().unwrap().source)
            .unwrap()
            .document
            .annotations()
            .len();
        assert_eq!(annotations_before, annotations_after);
    }

    #[test]
    fn accept_callout_suggestion_commits_one_undoable_callout() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let source = state.selected_step().unwrap().source;
        let step = state.selected_step().cloned().unwrap();
        let tip = callout_default_tip(&mut state);
        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Pending(
            callout_proposal_with_run_and_tip(&mut state, 1, tip),
        );
        let doc = state
            .presentation
            .document_for_step(&step, &state.store)
            .unwrap();
        let expected_bubble = rollshot_image_document::place_number_callout_bubble(
            tip,
            doc.document.source().width(),
            doc.document.source().height(),
            doc.document.annotations(),
        );

        let _ = update(&mut state, Message::AcceptCalloutSuggestion);

        let doc = state.presentation.doc(source).unwrap();
        assert_eq!(doc.document.annotations().len(), 1);
        match &doc.document.annotations()[0] {
            rollshot_image_document::Annotation::NumberCallout {
                tip: got_tip,
                bubble,
                ..
            } => {
                assert_eq!(*got_tip, tip);
                assert_eq!(*bubble, expected_bubble);
            }
            other => panic!("expected NumberCallout, got {other:?}"),
        }
        let _ = update(&mut state, Message::AnnotationUndo);
        let doc = state.presentation.doc(source).unwrap();
        assert_eq!(doc.document.annotations().len(), 0);
        let _ = update(&mut state, Message::AnnotationRedo);
        let doc = state.presentation.doc(source).unwrap();
        assert_eq!(doc.document.annotations().len(), 1);
    }

    #[test]
    fn accepted_callout_bubble_differs_from_tip_for_central_tip() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let source = state.selected_step().unwrap().source;
        let step = state.selected_step().cloned().unwrap();
        let doc = state
            .presentation
            .document_for_step(&step, &state.store)
            .unwrap();
        let image = doc.document.source();
        let center_x = image.width() as f32 / 2.0;
        let center_y = image.height() as f32 / 2.0;
        let tip = rollshot_image_document::ImagePoint::new(center_x - 1.0, center_y - 1.0);
        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Pending(
            callout_proposal_with_run_and_tip(&mut state, 1, tip),
        );

        let _ = update(&mut state, Message::AcceptCalloutSuggestion);

        let doc = state.presentation.doc(source).unwrap();
        assert_eq!(doc.document.annotations().len(), 1);
        match &doc.document.annotations()[0] {
            rollshot_image_document::Annotation::NumberCallout {
                tip: got_tip,
                bubble,
                ..
            } => {
                assert_ne!(
                    *bubble, *got_tip,
                    "committed bubble must not be drawn on top of the tip"
                );
            }
            other => panic!("expected NumberCallout, got {other:?}"),
        }
    }

    #[test]
    fn suggest_callout_requested_transitions_to_running() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);

        let _ = update(&mut state, Message::SuggestCalloutRequested);

        match &state.callout_suggestion {
            crate::timeline_workspace::CalloutSuggestionState::Running { run_id, .. } => {
                assert_eq!(*run_id, 1);
            }
            other => panic!("expected Running, got {other:?}"),
        }
        assert_eq!(state.callout_agent_run_id, 1);
    }

    // ---- Task 8: view/state predicates and modal close routing ----

    #[test]
    fn callout_state_predicates_cover_running_pending_and_terminal() {
        let mut state = ws(recording_from_frames());
        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Idle;
        assert!(!state.callout_suggestion.is_running());
        assert!(!state.callout_suggestion.is_pending());
        assert!(state.callout_suggestion.proposal().is_none());

        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Running {
            run_id: 1,
            cancellation: rollshot_agent::runtime::RunCancellation::new(),
        };
        assert!(state.callout_suggestion.is_running());
        assert!(!state.callout_suggestion.is_pending());

        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Pending(
            callout_proposal(&mut state),
        );
        assert!(state.callout_suggestion.is_pending());
        assert!(state.callout_suggestion.proposal().is_some());

        state.callout_suggestion =
            crate::timeline_workspace::CalloutSuggestionState::NoSuggestion { reason: None };
        assert!(!state.callout_suggestion.is_pending());
        assert!(state.callout_suggestion.proposal().is_none());

        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Failed {
            message: "boom".to_string(),
        };
        assert!(!state.callout_suggestion.is_pending());
    }

    #[test]
    fn running_callout_state_disables_mutation_predicate() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Running {
            run_id: 1,
            cancellation: rollshot_agent::runtime::RunCancellation::new(),
        };

        // The view predicate: while a callout is running, mutation must be
        // disabled. The canvas enforces this via `mutation_allowed`, so the
        // pure state predicate here is `is_running() || is_pending()`.
        let mutation_allowed =
            !state.callout_suggestion.is_running() && !state.callout_suggestion.is_pending();

        assert!(!mutation_allowed, "Running must disable mutation");
    }

    #[test]
    fn pending_callout_state_enables_accept_and_reject_predicates() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Pending(
            callout_proposal(&mut state),
        );

        assert!(state.callout_suggestion.is_pending());
        assert!(state.callout_suggestion.proposal().is_some());
    }

    #[test]
    fn rejecting_callout_proposal_restores_idle_state() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Pending(
            callout_proposal(&mut state),
        );

        let _ = update(&mut state, Message::RejectCalloutSuggestion);

        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Idle
        ));
    }

    #[test]
    fn accepting_callout_proposal_disappears_from_state() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Pending(
            callout_proposal(&mut state),
        );
        assert!(state.callout_suggestion.is_pending());

        let _ = update(&mut state, Message::AcceptCalloutSuggestion);

        assert!(!state.callout_suggestion.is_pending());
        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Idle
        ));
    }

    #[test]
    fn annotation_done_clears_pending_callout_state() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Pending(
            callout_proposal(&mut state),
        );
        state.callout_agent_run_id = 5;

        let _ = update(&mut state, Message::AnnotationDone);

        assert!(state.annotation_session.is_none());
        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Idle
        ));
        assert_eq!(state.callout_agent_run_id, 0);
    }

    #[test]
    fn annotation_done_clears_no_suggestion_and_failed_states() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        state.callout_suggestion =
            crate::timeline_workspace::CalloutSuggestionState::NoSuggestion { reason: None };
        state.callout_agent_run_id = 3;

        let _ = update(&mut state, Message::AnnotationDone);

        assert!(state.annotation_session.is_none());
        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Idle
        ));
        assert_eq!(state.callout_agent_run_id, 0);
    }

    #[test]
    fn annotation_cancel_during_running_callout_invokes_cancellation() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        let cancellation = rollshot_agent::runtime::RunCancellation::new();
        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Running {
            run_id: 1,
            cancellation: cancellation.clone(),
        };
        state.callout_agent_run_id = 1;

        let _ = update(&mut state, Message::AnnotationCancel);

        assert!(
            cancellation.is_cancelled(),
            "running callout must be cancelled"
        );
        assert!(state.annotation_session.is_none());
        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Idle
        ));
        assert_eq!(state.callout_agent_run_id, 0);
    }

    #[test]
    fn annotation_cancel_discards_pending_proposal() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key");
        let _openai = EnvVarGuard::remove("OPENAI_API_KEY");
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        state.callout_suggestion = crate::timeline_workspace::CalloutSuggestionState::Pending(
            callout_proposal(&mut state),
        );

        let _ = update(&mut state, Message::AnnotationCancel);

        assert!(state.annotation_session.is_none());
        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Idle
        ));
    }

    #[test]
    fn annotation_cancel_when_idle_just_closes_session() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::AnnotateStepRequested);
        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Idle
        ));

        let _ = update(&mut state, Message::AnnotationCancel);

        assert!(state.annotation_session.is_none());
        assert!(matches!(
            state.callout_suggestion,
            crate::timeline_workspace::CalloutSuggestionState::Idle
        ));
    }
}
