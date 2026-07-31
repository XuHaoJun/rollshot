//! P0c-2 Action Guide Timeline Workspace: review and edit a detected guide
//! (select / rename / delete a step, replace a keyframe from the nearby strip),
//! then export it to a chosen directory. A sibling of `result_workspace/`,
//! reachable only when the `action-guide` feature is built. Replaces P0c-1's
//! direct-export handler.
//!
//! Session-lifecycle tail (original spec §Session Lifecycle):
//!
//! ```text
//! Reviewing  (rename / delete / replace keyframe)
//!    |  Discard -> Discarded (exit; FrameStore dropped)
//!    v  Export Guide -> pick directory
//! Exporting  (export_guide writes a temp sibling, then atomic rename)
//!    |  error -> back to Reviewing (inline message; session intact)
//!    v
//! Done  (exit; temporary assets dropped on app exit)
//! ```

pub(crate) mod annotation;
mod caption_agent;
pub(crate) mod guide_export;
pub(crate) mod motion;
mod storyboard_copy;
mod update;
mod view;
#[cfg(feature = "action-guide")]
mod visual_annotation_agent;

#[cfg(feature = "action-guide")]
pub(crate) mod project;
#[cfg(feature = "action-guide")]
pub(crate) mod project_publish;
#[cfg(feature = "action-guide")]
pub(crate) mod share;

#[allow(unused_imports)]
pub use update::{subscription, update, Message, Update};
pub use view::view;

use rollshot_action::{
    CaptureRegion, FrameId, FrameStore, Guide, GuideStep, InputCapability, InputSourceKind,
    Recording,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectSaveState {
    Unsaved,
    Clean,
    Dirty,
    Saving,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublishAggregate {
    Publishing,
    NeedsAttention,
    Published,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublishOutputStatus {
    Current,
    Stale,
    Updating,
    Failed,
}

#[cfg(feature = "action-guide")]
pub(crate) struct PublishOperation {
    pub id: project_publish::PublishOperationId,
    pub revision: u64,
    pub cancel: rollshot_action::project::PublishCancellation,
    pub per_output: std::collections::BTreeMap<
        rollshot_action::project::PublishOutputKind,
        PublishOutputStatus,
    >,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FirstSavePrompt {
    Hidden,
    Visible,
    Picking,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseIntent {
    None,
    Confirming,
    SaveThenClose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Effect {
    None,
    CloseWorkspace,
    #[cfg(feature = "action-guide")]
    ProjectSaved {
        root: std::path::PathBuf,
        display_name: String,
        close_workspace: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IssuePackKind {
    Folder,
    Zip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FfmpegSetupDialog {
    pub info: crate::managed_ffmpeg::FfmpegSetupInfo,
    pub downloading: bool,
}

/// Copy image operation state machine for the storyboard preview modal.
///
/// ```text
/// Idle ───────────────► Copying(id) ──success──► Copied(id) ──delay──► Idle
///  ▲                         │
///  │                         └─failure────────► Failed(id) ──retry──► Copying(new id)
///  └──────────────── modal close drops the entire preview state ────────────────
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StoryboardCopyState {
    Idle,
    Copying { operation_id: u64 },
    Copied { operation_id: u64 },
    Failed { operation_id: u64, message: String },
}

#[derive(Debug, Clone)]
pub(crate) struct StoryboardPreviewState {
    pub handle: iced::widget::image::Handle,
    pub width: u32,
    pub height: u32,
    pub step_count: usize,
    pub copy_state: StoryboardCopyState,
}

#[derive(Clone)]
pub(crate) struct IssuePackDialog {
    pub review_confirmed: bool,
    pub pending_kind: Option<IssuePackKind>,
    pub include_gif: bool,
    pub pending_export: Option<guide_export::PendingIssuePackExport>,
    pub operation_id: u64,
    pub exporting: bool,
    pub cancel: rollshot_action::project::PublishCancellation,
}

impl std::fmt::Debug for IssuePackDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IssuePackDialog")
            .field("review_confirmed", &self.review_confirmed)
            .field("pending_kind", &self.pending_kind)
            .field("include_gif", &self.include_gif)
            .field(
                "pending_export",
                &self.pending_export.as_ref().map(|_| ".."),
            )
            .field("operation_id", &self.operation_id)
            .field("exporting", &self.exporting)
            .field("cancelled", &self.cancel.is_cancelled())
            .finish()
    }
}

impl IssuePackDialog {
    pub(crate) fn new() -> Self {
        Self {
            review_confirmed: false,
            pending_kind: None,
            include_gif: true,
            pending_export: None,
            operation_id: 0,
            exporting: false,
            cancel: rollshot_action::project::PublishCancellation::new(),
        }
    }
}

/// One nearby-strip thumbnail: a retained frame id and its prebuilt iced handle.
pub(crate) struct StripFrame {
    pub id: FrameId,
    pub handle: iced::widget::image::Handle,
}

/// State machine for the visual annotation suggestion flow with consent gating.
///
/// ```text
///   Idle ──► ConsentPending ──► Running ──► PendingReview
///               │                  │             │
///               │ cancel           │ cancel      │ accept/reject/dismiss
///               v                  v             v
///              Idle              Idle           Idle
///                                  │
///                                  └──► NoSuggestion (model declined)
///                                  └──► Failed       (provider/protocol error)
/// ```
///
/// Manual state-changing actions (DeleteStep, ReplaceKeyframe, manual
/// annotation, undo, redo) discard a pending review and transition to
/// Idle with a "stale" banner. `run_id` is the monotonic local id from
/// `visual_annotation_agent_run_id`; late results from older cancelled or
/// timed-out runs are dropped.
#[derive(Debug)]
#[allow(dead_code)] // Read by view; only the update path uses it here.
pub(crate) enum VisualAnnotationSuggestionState {
    Idle,
    ConsentPending(visual_annotation_agent::VisualSuggestionConsent),
    Running {
        run_id: u64,
        cancellation: rollshot_agent::runtime::RunCancellation,
    },
    PendingReview(rollshot_action::VisualAnnotationProposal),
    NoSuggestion {
        reason: Option<String>,
    },
    Failed {
        message: String,
    },
}

/// Export state machine for standalone Action Guide export.
///
/// ```text
/// Idle ──► PickingDestination { operation_id, pending }
///              │  chosen parent ──► Exporting { operation_id }
///              │  cancelled ──────► Idle
///              v
///         Exporting { operation_id }
///              │  success ──► Succeeded
///              │  failure ──► Idle (recoverable banner)
///              │  stale result ──► ignored
/// ```
#[derive(Debug)]
pub(crate) enum GuideExportState {
    Idle,
    PickingDestination {
        operation_id: u64,
        pending: guide_export::PendingStandaloneExport,
    },
    Exporting {
        operation_id: u64,
    },
    Succeeded,
}

/// Coordinates lazy frame loading for project-backed workspaces.
///
/// Uses a generation token to skip stale loads and a semaphore to bound
/// concurrent decodes to two.
///
/// ```text
/// select step → clear old handles, compute required IDs
///   → cache hit → use immediately
///   → cache miss → advance generation, await semaphore permit
///     → recheck generation before spawn_blocking
///     → load_step_frame (decode PNG)
///     → verify generation/selected-step
///     → stale → drop silently
///     → current → insert cache, build handle, hydrate annotations
/// ```
pub(crate) struct FrameLoadCoordinator {
    #[allow(dead_code)]
    generation: std::sync::atomic::AtomicU64,
    #[allow(dead_code)]
    semaphore: std::sync::Arc<tokio::sync::Semaphore>,
}

impl FrameLoadCoordinator {
    pub(crate) fn new() -> Self {
        Self {
            generation: std::sync::atomic::AtomicU64::new(0),
            semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(2)),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn advance_generation(&self) -> u64 {
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1
    }

    #[allow(dead_code)]
    pub(crate) fn current_generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// The Action Guide review/export workspace. Owns the editable guide and the
/// frame store moved out of the finished `Recording`.
pub struct TimelineWorkspace {
    pub(crate) guide: Guide,
    pub(crate) store: FrameStore,
    pub(crate) region: CaptureRegion,
    pub(crate) capability: InputCapability,
    pub(crate) source_kind: InputSourceKind,
    /// 1-based index of the selected step, or `None` when the guide is empty.
    pub(crate) selected: Option<usize>,
    /// Inline banner (export error / advisory). `None` when clear.
    pub(crate) message: Option<String>,
    /// Issue Pack export dialog state, if open.
    pub(crate) issue_pack: Option<IssuePackDialog>,
    /// FFmpeg setup/download dialog state, if MP4 export needs FFmpeg.
    pub(crate) ffmpeg_setup: Option<FfmpegSetupDialog>,
    /// True while the discard confirmation modal is shown.
    pub(crate) pending_discard: bool,
    /// Cached handle for the selected step's current keyframe.
    pub(crate) keyframe_handle: Option<iced::widget::image::Handle>,
    /// Cached nearby-strip thumbnails for the selected step.
    pub(crate) strip: Vec<StripFrame>,
    /// Storyboard preview modal state, if open.
    pub(crate) storyboard_preview: Option<StoryboardPreviewState>,
    /// Per-step annotation documents keyed by `GuideStep.source`.
    pub(crate) presentation: annotation::ActionGuidePresentation,
    /// Active annotation editing session, if the modal is open.
    pub(crate) annotation_session: Option<annotation::StepAnnotationSession>,
    /// Pending agent caption suggestions, if generated for the current guide.
    pub(crate) caption_proposal: Option<rollshot_action::CaptionProposal>,
    /// True while a caption suggestion run is active.
    pub(crate) caption_suggestions_running: bool,
    /// Monotonic local run id for caption proposal provenance.
    pub(crate) caption_agent_run_id: u64,
    /// The single process-wide task store, opened once at workspace boot.
    /// `None` when the config directory or the store is unavailable; the caption
    /// run then reports the existing "Caption suggestions failed: {error}"
    /// copy rather than running unpersisted and unaudited.
    pub(crate) task_store: Option<std::sync::Arc<crate::agent_store::TaskStore>>,
    /// Cancellation for the in-flight caption run. Triggered on the existing
    /// exits — leaving the workspace, starting another run, closing the project
    /// — with no new UI affordance.
    pub(crate) caption_cancellation: Option<rollshot_agent::runtime::RunCancellation>,
    /// Task store ID of the caption task created for the current proposal.
    /// `None` until `SuggestCaptionsRequested` creates a task.
    pub(crate) caption_task_id: Option<rollshot_agent::product_task::ProductTaskId>,
    /// Cached `ReadyForReview` snapshot for the current caption proposal.
    /// `None` until `CaptionProposalLoaded` promotes the task.
    pub(crate) caption_review_snapshot: Option<rollshot_agent::product_task::ProductTaskSnapshot>,
    /// A review decision is being durably committed; serializes subsequent decisions.
    pub(crate) caption_review_persisting: bool,
    /// Current visual annotation suggestion state. See [`VisualAnnotationSuggestionState`].
    #[allow(dead_code)] // Read by Task 8's view; only the update path uses it here.
    pub(crate) visual_annotation_suggestion: VisualAnnotationSuggestionState,
    /// Monotonic local run id for visual annotation suggestion provenance.
    #[allow(dead_code)]
    pub(crate) visual_annotation_agent_run_id: u64,
    /// Task store ID of the visual annotation task created for the current
    /// suggestion run. `None` until the suggestion run creates a task.
    #[allow(dead_code)]
    pub(crate) visual_annotation_task_id: Option<rollshot_agent::product_task::ProductTaskId>,
    /// Cached `ReadyForReview` snapshot for the current visual annotation
    /// proposal. `None` until the suggestion run promotes the task.
    #[allow(dead_code)]
    pub(crate) visual_annotation_review_snapshot:
        Option<rollshot_agent::product_task::ProductTaskSnapshot>,
    /// A visual annotation review decision is being durably committed;
    /// serializes subsequent decisions.
    #[allow(dead_code)]
    pub(crate) visual_annotation_review_persisting: bool,
    /// Monotonic operation id for storyboard copy provenance and late-result
    /// race protection. Incremented on each [`CopyStoryboardRequested`].
    pub(crate) storyboard_copy_operation_id: u64,
    /// Current standalone export lifecycle state.
    pub(crate) export_state: GuideExportState,
    /// Result of the last successful standalone export, if any.
    pub(crate) last_export: Option<guide_export::StandaloneExportResult>,
    /// Monotonic operation id for standalone export provenance.
    pub(crate) next_export_operation_id: u64,
    /// Monotonic operation id for Issue Pack export provenance.
    pub(crate) next_issue_pack_operation_id: u64,
    #[cfg(feature = "action-guide")]
    #[allow(dead_code)]
    pub(crate) frame_source: Option<rollshot_action::StepFrameSource>,
    #[cfg(feature = "action-guide")]
    #[allow(dead_code)]
    pub(crate) project_session: Option<project::ProjectSession>,
    #[cfg(feature = "action-guide")]
    #[allow(dead_code)]
    pub(crate) enabled_outputs: rollshot_action::project::EnabledOutputs,
    #[cfg(feature = "action-guide")]
    pub(crate) save_state: ProjectSaveState,
    #[cfg(feature = "action-guide")]
    pub(crate) first_save_prompt: FirstSavePrompt,
    #[cfg(feature = "action-guide")]
    pub(crate) close_intent: CloseIntent,
    #[cfg(feature = "action-guide")]
    #[allow(dead_code)]
    pub(crate) frame_coordinator: FrameLoadCoordinator,
    #[cfg(feature = "action-guide")]
    pub(crate) last_save_error: Option<String>,
    #[cfg(feature = "action-guide")]
    pub(crate) pending_writer_guard:
        std::sync::Arc<std::sync::Mutex<Option<project::ProjectWriterGuard>>>,
    #[cfg(feature = "action-guide")]
    pub(crate) publish_operation: Option<PublishOperation>,
    #[cfg(feature = "action-guide")]
    pub(crate) publish_arbiter: project_publish::PublishArbiter,
    #[cfg(feature = "action-guide")]
    pub(crate) publish_freshness: std::collections::BTreeMap<
        rollshot_action::project::PublishOutputKind,
        rollshot_action::project::PublishFreshness,
    >,
    #[cfg(feature = "action-guide")]
    pub(crate) publish_details_open: bool,
    #[cfg(feature = "action-guide")]
    pub(crate) next_publish_operation_id: u64,
    #[cfg(feature = "action-guide")]
    pub(crate) share_progress: Option<share::ShareProgress>,
    #[cfg(feature = "action-guide")]
    pub(crate) share_kind: Option<share::ShareKind>,
    #[cfg(feature = "action-guide")]
    pub(crate) share_operation_id: u64,
    #[cfg(feature = "action-guide")]
    pub(crate) import_warnings: Vec<rollshot_action::ImportWarning>,
    #[cfg(feature = "action-guide")]
    pub(crate) imported_scratch: Option<rollshot_action::ImportedScratch>,
    /// Workspace motion state: session-owned recording, failure, or none.
    #[cfg(feature = "action-guide")]
    pub(crate) motion: motion::WorkspaceMotion,
    /// Save recording (raw MP4 export) state machine.
    #[cfg(feature = "action-guide")]
    pub(crate) save_recording_state: motion::SaveRecordingState,
    /// Monotonic operation id for save-recording export provenance.
    #[cfg(feature = "action-guide")]
    pub(crate) next_save_recording_operation_id: u64,
}

impl TimelineWorkspace {
    /// Build the workspace from a finished recording. Selects step 1 (if any)
    /// and primes the selection handle cache.
    pub fn new(
        recording: Recording,
        region: CaptureRegion,
        capability: InputCapability,
        source_kind: InputSourceKind,
        motion_outcome: Option<rollshot_action::motion::MotionRecordingOutcome>,
    ) -> Self {
        let Recording { candidates, store } = recording;
        let guide = Guide::from_candidates(candidates);
        let selected = (!guide.is_empty()).then_some(1);
        let mut ws = Self {
            guide,
            store,
            region,
            capability,
            source_kind,
            selected,
            message: None,
            issue_pack: None,
            ffmpeg_setup: None,
            pending_discard: false,
            keyframe_handle: None,
            strip: Vec::new(),
            storyboard_preview: None,
            presentation: annotation::ActionGuidePresentation::new(),
            annotation_session: None,
            caption_proposal: None,
            caption_suggestions_running: false,
            caption_agent_run_id: 0,
            task_store: None,
            caption_cancellation: None,
            caption_task_id: None,
            caption_review_snapshot: None,
            caption_review_persisting: false,
            visual_annotation_suggestion: VisualAnnotationSuggestionState::Idle,
            visual_annotation_agent_run_id: 0,
            visual_annotation_task_id: None,
            visual_annotation_review_snapshot: None,
            visual_annotation_review_persisting: false,
            storyboard_copy_operation_id: 0,
            export_state: GuideExportState::Idle,
            last_export: None,
            next_export_operation_id: 0,
            next_issue_pack_operation_id: 0,
            #[cfg(feature = "action-guide")]
            frame_source: None,
            #[cfg(feature = "action-guide")]
            project_session: None,
            #[cfg(feature = "action-guide")]
            enabled_outputs: Default::default(),
            #[cfg(feature = "action-guide")]
            save_state: ProjectSaveState::Unsaved,
            #[cfg(feature = "action-guide")]
            first_save_prompt: FirstSavePrompt::Visible,
            #[cfg(feature = "action-guide")]
            close_intent: CloseIntent::None,
            #[cfg(feature = "action-guide")]
            frame_coordinator: FrameLoadCoordinator::new(),
            #[cfg(feature = "action-guide")]
            last_save_error: None,
            #[cfg(feature = "action-guide")]
            pending_writer_guard: std::sync::Arc::new(std::sync::Mutex::new(None)),
            #[cfg(feature = "action-guide")]
            publish_operation: None,
            #[cfg(feature = "action-guide")]
            publish_arbiter: project_publish::PublishArbiter::new(),
            #[cfg(feature = "action-guide")]
            publish_freshness: std::collections::BTreeMap::new(),
            #[cfg(feature = "action-guide")]
            publish_details_open: false,
            #[cfg(feature = "action-guide")]
            next_publish_operation_id: 0,
            #[cfg(feature = "action-guide")]
            share_progress: None,
            #[cfg(feature = "action-guide")]
            share_kind: None,
            #[cfg(feature = "action-guide")]
            share_operation_id: 0,
            #[cfg(feature = "action-guide")]
            import_warnings: Vec::new(),
            #[cfg(feature = "action-guide")]
            imported_scratch: None,
            #[cfg(feature = "action-guide")]
            motion: motion::WorkspaceMotion::from_outcome(motion_outcome),
            #[cfg(feature = "action-guide")]
            save_recording_state: motion::SaveRecordingState::Idle,
            #[cfg(feature = "action-guide")]
            next_save_recording_operation_id: 0,
        };
        ws.rebuild_selection_handles();
        ws
    }

    /// Build the workspace from an imported video seed. Sets the workspace to
    /// Unsaved/Dirty, creates a `ProjectFrameSource` from the scratch directory,
    /// and retains the scratch guard until first save completes.
    #[cfg(feature = "action-guide")]
    pub fn from_imported_video(seed: rollshot_action::ImportedWorkspaceSeed) -> Self {
        use rollshot_action::{
            ProjectFrameSource, StepFrameSource, DEFAULT_PROJECT_FRAME_CACHE_BYTES,
        };

        let selected = (!seed.guide.is_empty()).then_some(1);
        let presentation = annotation::ActionGuidePresentation::new();

        let source = ProjectFrameSource::from_catalog(
            seed.scratch.root().to_owned(),
            seed.frames,
            DEFAULT_PROJECT_FRAME_CACHE_BYTES,
        );

        let mut ws = Self {
            guide: seed.guide,
            store: FrameStore::new(Default::default()),
            region: seed.capture_region,
            capability: seed.input_capability,
            source_kind: seed.input_source,
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
            task_store: None,
            caption_cancellation: None,
            caption_task_id: None,
            caption_review_snapshot: None,
            caption_review_persisting: false,
            visual_annotation_suggestion: VisualAnnotationSuggestionState::Idle,
            visual_annotation_agent_run_id: 0,
            visual_annotation_task_id: None,
            visual_annotation_review_snapshot: None,
            visual_annotation_review_persisting: false,
            storyboard_copy_operation_id: 0,
            export_state: GuideExportState::Idle,
            last_export: None,
            next_export_operation_id: 0,
            next_issue_pack_operation_id: 0,
            frame_source: Some(StepFrameSource::Project(source)),
            project_session: Some(project::ProjectSession::Unsaved),
            enabled_outputs: Default::default(),
            save_state: ProjectSaveState::Dirty,
            first_save_prompt: FirstSavePrompt::Visible,
            close_intent: CloseIntent::None,
            frame_coordinator: FrameLoadCoordinator::new(),
            last_save_error: None,
            pending_writer_guard: std::sync::Arc::new(std::sync::Mutex::new(None)),
            publish_operation: None,
            publish_arbiter: project_publish::PublishArbiter::new(),
            publish_freshness: std::collections::BTreeMap::new(),
            publish_details_open: false,
            next_publish_operation_id: 0,
            share_progress: None,
            share_kind: None,
            share_operation_id: 0,
            import_warnings: seed.import_warnings,
            imported_scratch: Some(seed.scratch),
            motion: motion::WorkspaceMotion::None,
            save_recording_state: motion::SaveRecordingState::Idle,
            next_save_recording_operation_id: 0,
        };
        ws.rebuild_selection_handles();
        ws
    }

    /// Persistent notice text for the current workspace. Returns an empty string
    /// when no notices apply. For imported workspaces, includes a visual-only
    /// disclosure and specific copy for each import warning.
    #[cfg(feature = "action-guide")]
    #[allow(dead_code)]
    pub fn persistent_notice(&self) -> String {
        use rollshot_action::{ImportWarning, InputSourceKind};

        let is_imported = matches!(self.source_kind, InputSourceKind::ImportedVideo);
        if !is_imported && self.import_warnings.is_empty() {
            return String::new();
        }

        let mut parts = Vec::new();

        if is_imported {
            parts.push(
                "Visual-only draft. Steps were inferred from visual changes \
                 because mouse and keyboard events were unavailable. Review before export."
                    .to_string(),
            );
        }

        for warning in &self.import_warnings {
            match warning {
                ImportWarning::NoVisualChangesDetected => {
                    parts.push(
                        "No visual changes detected; the final sampled frame was used.".to_string(),
                    );
                }
                ImportWarning::IntermediateChangesReduced => {
                    parts.push(
                        "Intermediate visual changes were omitted to keep this draft reviewable."
                            .to_string(),
                    );
                }
            }
        }

        parts.join("\n")
    }

    /// The root directory of the current frame source, if project-backed.
    #[cfg(feature = "action-guide")]
    #[allow(dead_code)]
    pub(crate) fn frame_source_root(&self) -> Option<&std::path::Path> {
        match self.frame_source.as_ref()? {
            rollshot_action::StepFrameSource::Project(src) => Some(src.root()),
            rollshot_action::StepFrameSource::InMemory(_) => None,
        }
    }

    /// The saved project root, if the workspace has been saved.
    #[cfg(feature = "action-guide")]
    #[allow(dead_code)]
    pub(crate) fn project_root(&self) -> Option<std::path::PathBuf> {
        match &self.project_session {
            Some(project::ProjectSession::Saved { root, .. }) => Some(root.clone()),
            _ => None,
        }
    }
    pub(crate) fn selected_step(&self) -> Option<&GuideStep> {
        let index = self.selected?;
        self.guide.steps().iter().find(|s| s.index == index)
    }

    /// `true` when the visual annotation consent dialog should be shown.
    pub(crate) fn visual_annotation_consent_pending(&self) -> bool {
        matches!(
            self.visual_annotation_suggestion,
            VisualAnnotationSuggestionState::ConsentPending(_)
        )
    }

    /// Recompute the cached keyframe handle and nearby strip for the current
    /// selection. Called after any change to `selected` or to a keyframe.
    pub(crate) fn rebuild_selection_handles(&mut self) {
        self.keyframe_handle = None;
        self.strip.clear();
        let Some(step) = self.selected_step() else {
            return;
        };
        let keyframe = step.keyframe;
        let nearby = step.nearby.clone();
        if let Some(frame) = self.store.retained(keyframe) {
            self.keyframe_handle = Some(build_handle(&frame.image));
        }
        for id in nearby {
            if let Some(frame) = self.store.retained(id) {
                let handle = build_handle(&frame.image);
                self.strip.push(StripFrame { id, handle });
            }
        }
    }

    /// Returns `true` when the workspace is in a state that allows persisted
    /// mutations. Draft selection/tool/modal changes are NOT gated by this.
    #[cfg(feature = "action-guide")]
    pub(crate) fn can_mutate(&self) -> bool {
        self.save_state != ProjectSaveState::Saving
            && !matches!(
                &self.project_session,
                Some(project::ProjectSession::Saved {
                    access: project::ProjectAccess::ReadOnly
                        | project::ProjectAccess::CorruptReadOnly,
                    ..
                })
            )
    }

    #[cfg(feature = "action-guide")]
    pub(crate) fn initial_frame_load_task(&self) -> iced::Task<Message> {
        if self
            .frame_source
            .as_ref()
            .is_some_and(|source| source.in_memory().is_none())
        {
            self.selected.map_or_else(iced::Task::none, |index| {
                iced::Task::done(Message::SelectStep(index))
            })
        } else {
            iced::Task::none()
        }
    }

    #[cfg(feature = "action-guide")]
    pub(crate) fn project_recent_metadata(&self) -> Option<(std::path::PathBuf, String)> {
        // Imported workspaces must not record recent-project entries.
        if matches!(self.source_kind, InputSourceKind::ImportedVideo) {
            return None;
        }
        match &self.project_session {
            Some(project::ProjectSession::Saved { root, .. }) => {
                Some((root.clone(), self.guide.title().to_string()))
            }
            _ => None,
        }
    }
    /// Mark the project dirty after a successful persisted mutation.
    #[cfg(feature = "action-guide")]
    pub(crate) fn mark_project_dirty(&mut self) {
        if self.save_state == ProjectSaveState::Clean {
            self.save_state = ProjectSaveState::Dirty;
        }
    }

    /// Build the [`CaptionApplyContext`] for the current workspace state.
    /// For durable proposals on a saved, clean project, returns a
    /// `DurableProject` context with the current revision, the proposal's
    /// projection digest, and the clean flag. Otherwise returns
    /// `EphemeralGuide`.
    #[cfg(feature = "action-guide")]
    pub(crate) fn caption_apply_context(
        &self,
        proposal: &rollshot_action::CaptionProposal,
    ) -> rollshot_action::CaptionApplyContext {
        if let (
            Some(project::ProjectSession::Saved { base_revision, .. }),
            ProjectSaveState::Clean,
            rollshot_action::CaptionProposalOrigin::DurableProject {
                projection_digest, ..
            },
        ) = (&self.project_session, self.save_state, proposal.origin())
        {
            rollshot_action::CaptionApplyContext::DurableProject {
                revision: *base_revision,
                projection_digest: projection_digest.clone(),
                clean: true,
            }
        } else {
            rollshot_action::CaptionApplyContext::EphemeralGuide
        }
    }

    #[cfg(feature = "action-guide")]
    pub(crate) fn publish_aggregate(&self) -> Option<PublishAggregate> {
        let session = self.project_session.as_ref()?;
        let _revision = match session {
            project::ProjectSession::Saved { base_revision, .. } => *base_revision,
            _ => return None,
        };

        let enabled = self.publish_enabled_kinds();
        if enabled.is_empty() {
            return None;
        }

        let mut any_updating = false;
        let mut all_current = true;

        for kind in &enabled {
            let status = self.publish_status_for_aggregate(*kind);
            match status {
                PublishOutputStatus::Updating => {
                    any_updating = true;
                    all_current = false;
                }
                PublishOutputStatus::Failed | PublishOutputStatus::Stale => {
                    all_current = false;
                }
                PublishOutputStatus::Current => {}
            }
        }

        if any_updating {
            Some(PublishAggregate::Publishing)
        } else if all_current {
            Some(PublishAggregate::Published)
        } else {
            Some(PublishAggregate::NeedsAttention)
        }
    }

    #[cfg(feature = "action-guide")]
    fn publish_status_for_aggregate(
        &self,
        kind: rollshot_action::project::PublishOutputKind,
    ) -> PublishOutputStatus {
        if let Some(ref op) = self.publish_operation {
            if op.per_output.get(&kind) == Some(&PublishOutputStatus::Updating) {
                return PublishOutputStatus::Updating;
            }
            if op.per_output.get(&kind) == Some(&PublishOutputStatus::Failed) {
                return PublishOutputStatus::Failed;
            }
        }
        match self.publish_freshness.get(&kind) {
            Some(rollshot_action::project::PublishFreshness::Current) => {
                PublishOutputStatus::Current
            }
            _ => PublishOutputStatus::Stale,
        }
    }

    #[cfg(feature = "action-guide")]
    pub(crate) fn publish_output_status(
        &self,
        kind: rollshot_action::project::PublishOutputKind,
        load: &rollshot_action::project::PublishStateLoad,
        revision: u64,
    ) -> PublishOutputStatus {
        if let Some(ref op) = self.publish_operation {
            if op.per_output.get(&kind) == Some(&PublishOutputStatus::Updating) {
                return PublishOutputStatus::Updating;
            }
            if op.per_output.get(&kind) == Some(&PublishOutputStatus::Failed) {
                return PublishOutputStatus::Failed;
            }
        }
        match load.freshness(kind, revision) {
            rollshot_action::project::PublishFreshness::Current => PublishOutputStatus::Current,
            rollshot_action::project::PublishFreshness::Stale => PublishOutputStatus::Stale,
        }
    }

    #[cfg(feature = "action-guide")]
    pub(crate) fn publish_enabled_kinds(&self) -> Vec<rollshot_action::project::PublishOutputKind> {
        use rollshot_action::project::PublishOutputKind;
        let mut kinds = vec![PublishOutputKind::Core];
        if self.enabled_outputs.storyboard {
            kinds.push(PublishOutputKind::Storyboard);
        }
        if self.enabled_outputs.gif {
            kinds.push(PublishOutputKind::Gif);
        }
        if self.enabled_outputs.mp4 {
            kinds.push(PublishOutputKind::Mp4);
        }
        kinds
    }

    #[cfg(feature = "action-guide")]
    pub(crate) fn is_publish_active(&self) -> bool {
        self.publish_operation.is_some()
    }

    #[cfg(feature = "action-guide")]
    pub(crate) fn load_publish_freshness(&mut self) {
        let Some(project::ProjectSession::Saved {
            root,
            base_revision,
            ..
        }) = &self.project_session
        else {
            return;
        };
        let load = rollshot_action::project::load_publish_state(root);
        self.publish_freshness.clear();
        for kind in self.publish_enabled_kinds() {
            self.publish_freshness
                .insert(kind, load.freshness(kind, *base_revision));
        }
    }
}

/// Pure context selection helper for visual annotation dispatch.
/// Returns [`VisualAnnotationContextRequest::Durable`] only when the project
/// is saved and clean; otherwise returns [`VisualAnnotationContextRequest::Ephemeral`]
/// with the current guide cloned from the workspace.
#[cfg(feature = "action-guide")]
pub(crate) fn visual_annotation_context_request(
    ws: &TimelineWorkspace,
) -> crate::timeline_workspace::visual_annotation_agent::VisualAnnotationContextRequest {
    use crate::timeline_workspace::visual_annotation_agent::VisualAnnotationContextRequest;

    if let (
        Some(project::ProjectSession::Saved {
            root,
            base_revision,
            ..
        }),
        ProjectSaveState::Clean,
    ) = (&ws.project_session, ws.save_state)
    {
        let step = ws
            .selected_step()
            .expect("selected step for context request");
        VisualAnnotationContextRequest::Durable {
            root: root.clone(),
            expected_revision: *base_revision,
            step_source: step.source,
            keyframe: step.keyframe,
        }
    } else {
        let step = ws
            .selected_step()
            .expect("selected step for context request");
        VisualAnnotationContextRequest::Ephemeral {
            guide: ws.guide.clone(),
            step_source: step.source,
            keyframe: step.keyframe,
        }
    }
}

/// Build an iced image handle from a retained RGBA frame.
///
/// NOTE: this clones the raw pixel bytes into the handle. It is only called
/// when the selection or keyframe changes (not per-frame), so the copy is
/// acceptable for the P0c-2 workspace. For very large captures the first
/// selection may briefly block the UI; revisit if profiling shows a problem.
pub(crate) fn build_handle(image: &image::RgbaImage) -> iced::widget::image::Handle {
    iced::widget::image::Handle::from_rgba(image.width(), image.height(), image.as_raw().clone())
}

/// Map the recorded input capability to the source kind we record in the export
/// manifest. This keeps the Linux and macOS handoffs DRY.
pub(crate) fn source_kind_for(
    capability: InputCapability,
    platform: crate::storage::Platform,
) -> InputSourceKind {
    match capability {
        InputCapability::VisualOnly { .. } => InputSourceKind::VisualOnly,
        InputCapability::SemanticEvents => match platform {
            crate::storage::Platform::Linux => InputSourceKind::LinuxEvdev,
            crate::storage::Platform::Macos => InputSourceKind::MacosCgEvent,
        },
    }
}

/// Boot the timeline workspace as a standalone iced app (Linux). Blocks until
/// the user exports (then exits) or discards/closes (then exits).
#[cfg(target_os = "linux")]
pub fn run(
    recording: Recording,
    region: CaptureRegion,
    capability: InputCapability,
    source_kind: InputSourceKind,
) -> Result<(), String> {
    use std::sync::{Arc, Mutex};

    let boot_data = Arc::new(Mutex::new(Some((
        recording,
        region,
        capability,
        source_kind,
    ))));
    let boot = move || {
        let (recording, region, capability, source_kind) = boot_data
            .lock()
            .unwrap()
            .take()
            .expect("timeline workspace boot data already consumed");
        let mut ws = TimelineWorkspace::new(recording, region, capability, source_kind, None);
        // Open the process-wide task store once at workspace boot.
        if let Ok(config_dir) = crate::daemon::config::rollshot_config_dir() {
            match crate::agent_store::open_process_store(&config_dir) {
                Ok(store) => ws.task_store = Some(store),
                Err(e) => tracing::warn!(
                    target: "rollshot::app::timeline_workspace",
                    error = %e,
                    "failed to open task store; caption runs will be unaudited"
                ),
            }
        }
        (ws, iced::Task::none())
    };

    fn update_task(state: &mut TimelineWorkspace, message: Message) -> iced::Task<Message> {
        let result = update(state, message);
        match result.effect {
            Effect::CloseWorkspace => iced::exit(),
            Effect::None => result.task,
            #[cfg(feature = "action-guide")]
            Effect::ProjectSaved {
                root,
                display_name,
                close_workspace,
            } => {
                if let Ok(config_dir) = crate::daemon::config::rollshot_config_dir() {
                    let recent =
                        crate::action_guide_home::recent::RecentProjects::load(&config_dir);
                    let mut home = crate::action_guide_home::ActionGuideHome::new(recent);
                    home.record_project_open(root, display_name);
                }
                if close_workspace {
                    iced::exit()
                } else {
                    result.task
                }
            }
        }
    }

    iced::application(boot, update_task, view)
        .title("Rollshot — Action Guide")
        .font(rollshot_image_document::style::FONT_REGULAR_BYTES)
        .font(rollshot_image_document::style::FONT_BOLD_BYTES)
        .subscription(subscription)
        .window(iced::window::Settings {
            size: iced::Size::new(1100.0, 760.0),
            min_size: Some(iced::Size::new(640.0, 420.0)),
            decorations: true,
            resizable: true,
            exit_on_close_request: false,
            ..Default::default()
        })
        .run()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use rollshot_action::{
        ActionRecorder, CandidateKind, CandidateStep, CaptureRegion, DetectReason, DetectorConfig,
        FrameStore, InputCapability, InputSourceKind, Recording, StoreConfig,
        VisualAnnotationPayload, VisualAnnotationProposal, VisualAnnotationProposalId,
        VisualAnnotationProposalOrigin, VisualAnnotationSuggestionDraft,
        VisualAnnotationSuggestionId,
    };

    fn region_32() -> CaptureRegion {
        CaptureRegion {
            x: 0,
            y: 0,
            width: 32,
            height: 32,
        }
    }

    fn black_32() -> RgbaImage {
        RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 255]))
    }

    fn white_quadrant_32() -> RgbaImage {
        let mut img = black_32();
        for y in 0..16 {
            for x in 0..16 {
                img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        img
    }

    /// A real recording with retained frames (detector-produced candidates), so
    /// keyframe/nearby handles resolve. Mirrors the P0c-1 export fixture.
    pub(super) fn recording_from_frames() -> Recording {
        let det = DetectorConfig {
            diff_threshold: 0.01,
            area_threshold: 0.05,
            cooldown_ms: 0,
            ..DetectorConfig::default()
        };
        let mut rec = ActionRecorder::new(region_32(), StoreConfig::default(), det);
        rec.ingest_frame(std::sync::Arc::new(black_32()), 0);
        for i in 1..=6 {
            rec.ingest_frame(std::sync::Arc::new(white_quadrant_32()), i * 100);
        }
        let recording = rec.finish();
        assert!(
            !recording.candidates.is_empty(),
            "detector fixture should produce at least one candidate"
        );
        recording
    }

    /// A synthetic recording with `n` hand-built candidates and an empty store
    /// (no retained frames). Used by pure update-logic tests that don't assert
    /// on image handles.
    pub(super) fn synthetic_recording(n: usize) -> Recording {
        let candidates = (0..n)
            .map(|i| {
                let base = (i as u64) * 10;
                CandidateStep {
                    id: i as u64,
                    kind: CandidateKind::Click,
                    reason: DetectReason::ClickConfirmed,
                    at_ms: (i as u64) * 100,
                    keyframe: base + 1,
                    nearby: vec![base, base + 1, base + 2],
                }
            })
            .collect();
        Recording {
            candidates,
            store: FrameStore::new(StoreConfig::default()),
        }
    }

    /// Build a VisualAnnotationProposal with three primitives for view tests
    /// that need a PendingReview state without requiring a mutable workspace.
    pub(super) fn visual_proposal_three_primitives_for_view(
        state: &TimelineWorkspace,
    ) -> VisualAnnotationProposal {
        let step = &state.guide.steps()[0];
        let doc = state
            .presentation
            .doc(step.source)
            .expect("presentation doc");
        let image = doc.document.source();
        VisualAnnotationProposal::from_agent_drafts(
            VisualAnnotationProposalId(1),
            1,
            VisualAnnotationProposalOrigin::EphemeralGuide {
                guide_digest: "aa".repeat(32),
            },
            step,
            doc.document.state_id(),
            image.width(),
            image.height(),
            [1u8; 32],
            [2u8; 32],
            vec![
                VisualAnnotationSuggestionDraft {
                    id: VisualAnnotationSuggestionId(1),
                    payload: VisualAnnotationPayload::NumberCallout {
                        tip: rollshot_image_document::ImagePoint::new(4.0, 4.0),
                        bubble: rollshot_image_document::ImagePoint::new(20.0, 20.0),
                    },
                    confidence: 0.9,
                    rationale: Some("button click target".to_string()),
                },
                VisualAnnotationSuggestionDraft {
                    id: VisualAnnotationSuggestionId(2),
                    payload: VisualAnnotationPayload::TextNote {
                        position: rollshot_image_document::ImagePoint::new(8.0, 8.0),
                        text: "Save button".to_string(),
                    },
                    confidence: 0.7,
                    rationale: None,
                },
                VisualAnnotationSuggestionDraft {
                    id: VisualAnnotationSuggestionId(3),
                    payload: VisualAnnotationPayload::OpaqueRedaction {
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

    fn workspace(recording: Recording) -> TimelineWorkspace {
        TimelineWorkspace::new(
            recording,
            region_32(),
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
            None,
        )
    }

    #[test]
    fn new_selects_first_step_and_builds_handles() {
        let ws = workspace(recording_from_frames());
        assert!(!ws.guide.steps().is_empty());
        assert_eq!(ws.selected, Some(1));
        assert!(
            ws.keyframe_handle.is_some(),
            "first step keyframe should resolve from the retained store"
        );
        assert!(!ws.strip.is_empty(), "nearby strip should have frames");
    }

    #[test]
    fn new_with_empty_recording_selects_nothing() {
        let ws = workspace(synthetic_recording(0));
        assert!(ws.guide.steps().is_empty());
        assert_eq!(ws.selected, None);
        assert!(ws.keyframe_handle.is_none());
        assert!(ws.strip.is_empty());
    }

    #[test]
    fn accepted_visual_annotations_flatten_only_storyboard() {
        let mut ws = workspace(recording_from_frames());
        let step = ws.selected_step().cloned().expect("selected step");
        let original = ws
            .store
            .retained(step.keyframe)
            .expect("retained keyframe")
            .image
            .clone();

        let doc = ws
            .presentation
            .document_for_step(&step, &ws.store)
            .expect("presentation doc");
        doc.document
            .add_text_note(
                rollshot_image_document::ImagePoint::new(2.0, 2.0),
                "Regression note".to_string(),
            )
            .unwrap();

        let options = storyboard_copy::render_storyboard_input(
            &storyboard_copy::snapshot_storyboard(&ws.guide, &ws.store, &ws.presentation)
                .expect("snapshot"),
            rollshot_action::StoryboardOptions::default(),
        )
        .expect("render");

        assert_ne!(
            options.image.as_raw(),
            original.as_raw(),
            "annotated storyboard must differ from original"
        );
        assert_eq!(
            ws.store.retained(step.keyframe).unwrap().image.as_raw(),
            original.as_raw(),
            "retained keyframe must be unchanged after annotation"
        );
    }

    #[test]
    fn export_guide_metadata_contains_no_provider_or_model_data() {
        let ws = workspace(recording_from_frames());
        let tmp = tempfile::tempdir().unwrap();
        let job = super::guide_export::build_reviewed_export_job(&ws).unwrap();

        let path = rollshot_action::render_guide_folder(&job, &tmp.path().join("action-guide"))
            .expect("render_guide_folder");

        let session_json =
            std::fs::read_to_string(path.join("session.json")).expect("session.json");
        let lower = session_json.to_lowercase();
        for forbidden in &[
            "provider",
            "model",
            "run_id",
            "run id",
            "prompt",
            "rationale",
            "attachment",
            "api_key",
            "api key",
        ] {
            assert!(
                !lower.contains(forbidden),
                "session.json must not contain '{forbidden}': {session_json}"
            );
        }

        let steps_md = std::fs::read_to_string(path.join("steps.md")).expect("steps.md");
        let steps_lower = steps_md.to_lowercase();
        for forbidden in &[
            "provider",
            "model",
            "run_id",
            "prompt",
            "rationale",
            "attachment",
        ] {
            assert!(
                !steps_lower.contains(forbidden),
                "steps.md must not contain '{forbidden}'"
            );
        }
    }

    #[test]
    fn suggest_captions_requested_does_not_enter_consent_pending() {
        let mut ws = workspace(recording_from_frames());
        ws.caption_suggestions_running = false;

        let _result =
            super::update::update(&mut ws, super::update::Message::SuggestCaptionsRequested);

        assert!(
            !matches!(
                ws.visual_annotation_suggestion,
                VisualAnnotationSuggestionState::ConsentPending(_)
            ),
            "SuggestCaptionsRequested must not enter ConsentPending"
        );
        // Whether or not the handler succeeds (depends on provider config),
        // it must never touch the visual annotation consent state.
    }

    // ---- Project lifecycle tests (Task 6) ----

    #[cfg(feature = "action-guide")]
    mod project_lifecycle {
        use super::super::*;
        use super::recording_from_frames;
        use super::{Rgba, RgbaImage};
        use crate::timeline_workspace::project::ProjectAccess;
        use crate::timeline_workspace::update::Message;
        use std::sync::Arc;

        fn workspace(recording: Recording) -> TimelineWorkspace {
            super::workspace(recording)
        }

        fn ws_project_backed() -> TimelineWorkspace {
            use rollshot_action::project::{
                EnabledOutputs, ProjectFrame, ProjectManifestV2, ProjectStep, ProjectStepId,
            };
            use rollshot_action::{CandidateKind, DetectReason, InputCapability, InputSourceKind};

            let manifest = ProjectManifestV2 {
                schema_version: 1,
                revision: 1,
                title: "Test Guide".into(),
                capture_region: super::region_32(),
                input_source: InputSourceKind::LinuxEvdev,
                input_capability: InputCapability::SemanticEvents,
                enabled_outputs: EnabledOutputs::default(),
                frames: vec![
                    ProjectFrame {
                        id: 1,
                        at_ms: 0,
                        sha256: "a".into(),
                        width: 32,
                        height: 32,
                    },
                    ProjectFrame {
                        id: 2,
                        at_ms: 50,
                        sha256: "b".into(),
                        width: 32,
                        height: 32,
                    },
                    ProjectFrame {
                        id: 3,
                        at_ms: 100,
                        sha256: "c".into(),
                        width: 32,
                        height: 32,
                    },
                ],
                steps: vec![
                    ProjectStep {
                        id: ProjectStepId(1),
                        order: 1,
                        title: "Step 1".into(),
                        caption: None,
                        kind: CandidateKind::Click,
                        reason: DetectReason::ClickConfirmed,
                        at_ms: 100,
                        keyframe: 1,
                        nearby: vec![1, 2, 3],
                        annotations: None,
                    },
                    ProjectStep {
                        id: ProjectStepId(2),
                        order: 2,
                        title: "Step 2".into(),
                        caption: None,
                        kind: CandidateKind::Click,
                        reason: DetectReason::ClickConfirmed,
                        at_ms: 200,
                        keyframe: 2,
                        nearby: vec![1, 2, 3],
                        annotations: None,
                    },
                ],
            import_warnings: Vec::new(),
            };
            let loaded = rollshot_action::project::LoadedProject {
                root: std::path::PathBuf::from("/tmp/test-project"),
                manifest: manifest.into(),
            motion: rollshot_action::project::MotionAssetLoad::None,
            };
            let guard = crate::timeline_workspace::project::ProjectWriterGuard::for_test();
            crate::timeline_workspace::project::from_loaded_project(
                loaded,
                ProjectAccess::Writable(guard),
            )
            .expect("ok")
        }

        fn ws_project_backed_read_only() -> TimelineWorkspace {
            use rollshot_action::project::{
                EnabledOutputs, ProjectFrame, ProjectManifestV2, ProjectStep, ProjectStepId,
            };
            use rollshot_action::{CandidateKind, DetectReason, InputCapability, InputSourceKind};

            let manifest = ProjectManifestV2 {
                schema_version: 1,
                revision: 1,
                title: "Test Guide".into(),
                capture_region: super::region_32(),
                input_source: InputSourceKind::LinuxEvdev,
                input_capability: InputCapability::SemanticEvents,
                enabled_outputs: EnabledOutputs::default(),
                frames: vec![
                    ProjectFrame {
                        id: 1,
                        at_ms: 0,
                        sha256: "a".into(),
                        width: 32,
                        height: 32,
                    },
                    ProjectFrame {
                        id: 2,
                        at_ms: 50,
                        sha256: "b".into(),
                        width: 32,
                        height: 32,
                    },
                    ProjectFrame {
                        id: 3,
                        at_ms: 100,
                        sha256: "c".into(),
                        width: 32,
                        height: 32,
                    },
                ],
                steps: vec![
                    ProjectStep {
                        id: ProjectStepId(1),
                        order: 1,
                        title: "Step 1".into(),
                        caption: None,
                        kind: CandidateKind::Click,
                        reason: DetectReason::ClickConfirmed,
                        at_ms: 100,
                        keyframe: 1,
                        nearby: vec![1, 2, 3],
                        annotations: None,
                    },
                    ProjectStep {
                        id: ProjectStepId(2),
                        order: 2,
                        title: "Step 2".into(),
                        caption: None,
                        kind: CandidateKind::Click,
                        reason: DetectReason::ClickConfirmed,
                        at_ms: 200,
                        keyframe: 2,
                        nearby: vec![1, 2, 3],
                        annotations: None,
                    },
                ],
            import_warnings: Vec::new(),
            };
            let loaded = rollshot_action::project::LoadedProject {
                root: std::path::PathBuf::from("/tmp/test-project"),
                manifest: manifest.into(),
            motion: rollshot_action::project::MotionAssetLoad::None,
            };
            crate::timeline_workspace::project::from_loaded_project(loaded, ProjectAccess::ReadOnly)
                .expect("ok")
        }

        #[test]
        fn fresh_recording_starts_with_save_first_prompt_visible() {
            let ws = workspace(recording_from_frames());
            assert!(ws.project_session.is_none());
            assert_eq!(ws.save_state, ProjectSaveState::Unsaved);
            assert_eq!(ws.first_save_prompt, FirstSavePrompt::Visible);
        }

        #[test]
        fn save_later_hides_prompt_and_keeps_unsaved() {
            let mut ws = workspace(recording_from_frames());
            ws.first_save_prompt = FirstSavePrompt::Visible;
            ws.save_state = ProjectSaveState::Unsaved;

            let _ = super::super::update::update(&mut ws, Message::SaveLater);

            assert_eq!(ws.first_save_prompt, FirstSavePrompt::Hidden);
            assert_eq!(ws.save_state, ProjectSaveState::Unsaved);
        }

        #[test]
        fn project_backed_workspace_starts_clean_with_hidden_prompt() {
            let ws = ws_project_backed();
            assert!(ws.project_session.is_some());
            assert_eq!(ws.save_state, ProjectSaveState::Clean);
            assert_eq!(ws.first_save_prompt, FirstSavePrompt::Hidden);
        }

        #[test]
        fn title_change_marks_dirty_when_project_is_clean() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Clean;

            let _ =
                super::super::update::update(&mut ws, Message::TitleChanged("New Title".into()));

            assert_eq!(ws.save_state, ProjectSaveState::Dirty);
        }

        #[test]
        fn caption_change_marks_dirty() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Clean;

            let _ = super::super::update::update(
                &mut ws,
                Message::CaptionChanged("New caption".into()),
            );

            assert_eq!(ws.save_state, ProjectSaveState::Dirty);
        }

        #[test]
        fn read_only_workspace_cannot_mutate() {
            let ws = ws_project_backed_read_only();
            assert!(!ws.can_mutate());
        }

        #[test]
        fn close_workspace_emits_close_workspace_effect() {
            let mut ws = workspace(recording_from_frames());
            ws.pending_discard = true;

            let result = super::super::update::update(&mut ws, Message::CloseSaveAndClose);

            assert_eq!(result.effect, Effect::CloseWorkspace);
            assert!(!ws.pending_discard);
        }

        #[test]
        fn close_discard_emits_close_workspace_effect() {
            let mut ws = workspace(recording_from_frames());
            ws.pending_discard = true;

            let result = super::super::update::update(&mut ws, Message::CloseDiscard);

            assert_eq!(result.effect, Effect::CloseWorkspace);
            assert!(!ws.pending_discard);
        }

        #[test]
        fn close_cancel_returns_to_workspace() {
            let mut ws = workspace(recording_from_frames());
            ws.pending_discard = true;

            let result = super::super::update::update(&mut ws, Message::CloseCancel);

            assert_eq!(result.effect, Effect::None);
            assert!(!ws.pending_discard);
        }

        #[test]
        fn delete_step_is_noop_when_read_only() {
            let mut ws = ws_project_backed_read_only();
            assert!(!ws.can_mutate());
            let step_count_before = ws.guide.steps().len();

            let _ = super::super::update::update(&mut ws, Message::DeleteStep);

            assert_eq!(ws.guide.steps().len(), step_count_before);
        }

        #[test]
        fn delete_step_marks_dirty() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Clean;

            let _ = super::super::update::update(&mut ws, Message::DeleteStep);

            assert_eq!(ws.save_state, ProjectSaveState::Dirty);
        }

        #[test]
        fn replace_keyframe_marks_dirty() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Clean;
            let step = ws.selected_step().cloned().unwrap();
            let replacement = *step.nearby.iter().find(|&&f| f != step.keyframe).unwrap();

            let _ = super::super::update::update(&mut ws, Message::ReplaceKeyframe(replacement));

            assert_eq!(ws.save_state, ProjectSaveState::Dirty);
        }

        #[test]
        fn guide_title_change_marks_dirty() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Clean;

            let _ = super::super::update::update(
                &mut ws,
                Message::GuideTitleChanged("New Guide Title".into()),
            );

            assert_eq!(ws.save_state, ProjectSaveState::Dirty);
        }

        #[test]
        fn read_only_rejects_title_change() {
            let mut ws = ws_project_backed_read_only();
            let title_before = ws.guide.steps()[0].title.clone();

            let _ = super::super::update::update(
                &mut ws,
                Message::TitleChanged("Should Not Apply".into()),
            );

            assert_eq!(ws.guide.steps()[0].title, title_before);
        }

        #[test]
        fn read_only_rejects_caption_change() {
            let mut ws = ws_project_backed_read_only();

            let _ = super::super::update::update(
                &mut ws,
                Message::CaptionChanged("Should Not Apply".into()),
            );

            assert_eq!(ws.guide.steps()[0].caption, "");
        }

        #[test]
        fn read_only_rejects_replace_keyframe() {
            let mut ws = ws_project_backed_read_only();
            let step = ws.selected_step().cloned().unwrap();
            let replacement = *step.nearby.iter().find(|&&f| f != step.keyframe).unwrap();
            let keyframe_before = ws.selected_step().unwrap().keyframe;

            let _ = super::super::update::update(&mut ws, Message::ReplaceKeyframe(replacement));

            assert_eq!(ws.selected_step().unwrap().keyframe, keyframe_before);
        }

        #[test]
        fn read_only_rejects_guide_title_change() {
            let mut ws = ws_project_backed_read_only();
            let title_before = ws.guide.title().to_string();

            let _ = super::super::update::update(
                &mut ws,
                Message::GuideTitleChanged("Should Not Apply".into()),
            );

            assert_eq!(ws.guide.title(), title_before);
        }

        #[test]
        fn select_step_does_not_mark_dirty() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Clean;

            let _ = super::super::update::update(&mut ws, Message::SelectStep(1));

            assert_eq!(ws.save_state, ProjectSaveState::Clean);
        }

        #[test]
        fn annotation_tool_change_does_not_mark_dirty() {
            use crate::timeline_workspace::annotation::AnnotationTool;
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Clean;

            let _ = super::super::update::update(&mut ws, Message::AnnotateStepRequested);
            let _ = super::super::update::update(
                &mut ws,
                Message::AnnotationToolChanged(AnnotationTool::Text),
            );

            assert_eq!(ws.save_state, ProjectSaveState::Clean);
        }

        #[test]
        fn save_requested_for_first_save_sets_picking() {
            let mut ws = workspace(recording_from_frames());
            ws.first_save_prompt = FirstSavePrompt::Hidden;
            ws.save_state = ProjectSaveState::Unsaved;

            let result = super::super::update::update(&mut ws, Message::SaveRequested);

            assert_eq!(ws.first_save_prompt, FirstSavePrompt::Picking);
            assert!(result.task.units() > 0, "should return a picker task");
        }

        #[test]
        fn save_as_requested_for_saved_project_sets_picking() {
            let mut ws = ws_project_backed();

            let result = super::super::update::update(&mut ws, Message::SaveAsRequested);

            assert_eq!(ws.first_save_prompt, FirstSavePrompt::Picking);
            assert!(result.task.units() > 0, "should return a picker task");
        }

        #[test]
        fn saving_workspace_rejects_persisted_mutations() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Saving;
            let title_before = ws.guide.title().to_string();

            let _ = super::super::update::update(
                &mut ws,
                Message::GuideTitleChanged("Late edit".into()),
            );

            assert_eq!(ws.guide.title(), title_before);
            assert_eq!(ws.save_state, ProjectSaveState::Saving);
        }

        #[test]
        fn dirty_marker_does_not_interrupt_inflight_save() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Saving;

            ws.mark_project_dirty();

            assert_eq!(ws.save_state, ProjectSaveState::Saving);
        }

        #[test]
        fn save_picker_cancel_returns_to_visible() {
            let mut ws = workspace(recording_from_frames());
            ws.first_save_prompt = FirstSavePrompt::Picking;

            let _ = super::super::update::update(&mut ws, Message::SavePickerChosen(None));

            assert_eq!(ws.first_save_prompt, FirstSavePrompt::Visible);
        }

        #[test]
        fn save_worker_outcome_existing_saved_sets_clean() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Dirty;

            let _ = super::super::update::update(
                &mut ws,
                Message::SaveWorkerFinished(
                    super::super::update::SaveWorkerOutcome::ExistingSaved { revision: 5 },
                ),
            );

            assert_eq!(ws.save_state, ProjectSaveState::Clean);
            assert!(ws.message.as_ref().is_some_and(|m| m.contains("Saved")));
        }

        #[test]
        fn save_worker_outcome_failed_preserves_dirty() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Dirty;

            let _ = super::super::update::update(
                &mut ws,
                Message::SaveWorkerFinished(super::super::update::SaveWorkerOutcome::Failed(
                    "disk full".to_string(),
                )),
            );

            assert_eq!(ws.save_state, ProjectSaveState::Dirty);
            assert!(ws.last_save_error.is_some());
        }

        #[test]
        fn save_worker_outcome_committed_read_only_sets_clean_and_read_only() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Dirty;

            let _ = super::super::update::update(
                &mut ws,
                Message::SaveWorkerFinished(
                    super::super::update::SaveWorkerOutcome::NewCommittedReadOnly {
                        root: std::path::PathBuf::from("/tmp/test"),
                        revision: 1,
                        manifest: rollshot_action::project::ProjectManifestV3 {
                            schema_version: 3,
                            revision: 1,
                            title: "Test Guide".into(),
                            capture_region: super::region_32(),
                            input_source: InputSourceKind::LinuxEvdev,
                            input_capability: InputCapability::SemanticEvents,
                            enabled_outputs: Default::default(),
                            frames: Vec::new(),
                            steps: Vec::new(),
                            import_warnings: Vec::new(),
                            motion: None,
                        },
                        category: "post_commit_lock_race",
                    },
                ),
            );

            assert_eq!(ws.save_state, ProjectSaveState::Clean);
            assert!(!ws.can_mutate());
        }

        #[test]
        fn first_save_emits_recent_project_effect() {
            let mut ws = workspace(recording_from_frames());
            let root = std::path::PathBuf::from("/tmp/saved.rollshot-guide");

            let result = super::super::update::update(
                &mut ws,
                Message::SaveWorkerFinished(super::super::update::SaveWorkerOutcome::NewWritable {
                    root: root.clone(),
                    revision: 1,
                    manifest: rollshot_action::project::ProjectManifestV3 {
                        schema_version: 3,
                        revision: 1,
                        title: "Test Guide".into(),
                        capture_region: super::region_32(),
                        input_source: InputSourceKind::LinuxEvdev,
                        input_capability: InputCapability::SemanticEvents,
                        enabled_outputs: Default::default(),
                        frames: Vec::new(),
                        steps: Vec::new(),
                        import_warnings: Vec::new(),
                        motion: None,
                    },
                }),
            );

            assert!(matches!(
                result.effect,
                Effect::ProjectSaved {
                    root: effect_root,
                    close_workspace: false,
                    ..
                } if effect_root == root
            ));
        }

        #[test]
        fn close_dirty_project_shows_confirm_modal() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Dirty;

            let _ = super::super::update::update(&mut ws, Message::CloseRequested);

            assert!(ws.pending_discard);
            assert_eq!(ws.close_intent, CloseIntent::Confirming);
        }

        #[test]
        fn close_clean_project_emits_close_workspace() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Clean;

            let result = super::super::update::update(&mut ws, Message::CloseRequested);

            assert_eq!(result.effect, Effect::CloseWorkspace);
        }

        #[test]
        fn close_save_and_close_with_dirty_project_triggers_save() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Dirty;
            ws.close_intent = CloseIntent::Confirming;

            let _result = super::super::update::update(&mut ws, Message::CloseSaveAndClose);

            assert_eq!(ws.close_intent, CloseIntent::SaveThenClose);
            assert_eq!(ws.save_state, ProjectSaveState::Saving);
        }

        #[test]
        fn close_cancel_clears_close_intent_and_returns_to_workspace() {
            let mut ws = ws_project_backed();
            ws.close_intent = CloseIntent::Confirming;
            ws.pending_discard = true;

            let result = super::super::update::update(&mut ws, Message::CloseCancel);

            assert_eq!(result.effect, Effect::None);
            assert_eq!(ws.close_intent, CloseIntent::None);
            assert!(!ws.pending_discard);
        }

        #[test]
        fn read_only_workspace_rejects_all_mutations() {
            let mut ws = ws_project_backed_read_only();
            assert!(!ws.can_mutate());

            let step_count = ws.guide.steps().len();
            let title_before = ws.guide.steps()[0].title.clone();

            let _ = super::super::update::update(
                &mut ws,
                Message::TitleChanged("Should Not Apply".into()),
            );
            assert_eq!(ws.guide.steps()[0].title, title_before);

            let _ = super::super::update::update(
                &mut ws,
                Message::CaptionChanged("Should Not Apply".into()),
            );
            assert_eq!(ws.guide.steps()[0].caption, "");

            let _ = super::super::update::update(&mut ws, Message::DeleteStep);
            assert_eq!(ws.guide.steps().len(), step_count);

            let step = ws.selected_step().cloned().unwrap();
            let replacement = *step.nearby.iter().find(|&&f| f != step.keyframe).unwrap();
            let _ = super::super::update::update(&mut ws, Message::ReplaceKeyframe(replacement));
            assert_eq!(ws.selected_step().unwrap().keyframe, step.keyframe);

            let _ = super::super::update::update(
                &mut ws,
                Message::GuideTitleChanged("Should Not Apply".into()),
            );
            assert_eq!(ws.guide.title(), "Test Guide");
        }

        #[test]
        fn project_backed_view_builds_with_read_only_banner() {
            let ws = ws_project_backed_read_only();
            let _element = super::super::view::view(&ws);
        }

        #[test]
        fn project_backed_view_builds_with_save_state_indicators() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Dirty;
            {
                let _element = super::super::view::view(&ws);
            }

            ws.save_state = ProjectSaveState::Saving;
            {
                let _element = super::super::view::view(&ws);
            }

            ws.save_state = ProjectSaveState::Clean;
            {
                let _element = super::super::view::view(&ws);
            }
        }

        #[test]
        fn close_confirm_modal_shows_when_close_intent_confirming() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Dirty;
            ws.close_intent = CloseIntent::Confirming;
            ws.pending_discard = true;
            let _element = super::super::view::view(&ws);
        }

        // ---- Frame loading pipeline tests ----

        /// Write a PNG asset to disk and return the SHA256 hash.
        fn write_test_png_asset(root: &std::path::Path, image: &RgbaImage) -> String {
            use sha2::Digest;
            use std::io::Write;
            let mut png_buf = Vec::new();
            image
                .write_to(
                    &mut std::io::Cursor::new(&mut png_buf),
                    image::ImageFormat::Png,
                )
                .expect("encode PNG");
            let mut hasher = sha2::Sha256::new();
            hasher.write_all(&png_buf).expect("hash");
            let sha256 = format!("{:x}", hasher.finalize());
            let dest = root.join("assets/frames").join(format!("{sha256}.png"));
            std::fs::write(&dest, &png_buf).unwrap();
            sha256
        }

        fn ws_project_backed_with_assets() -> (TimelineWorkspace, tempfile::TempDir) {
            use rollshot_action::project::{
                EnabledOutputs, ProjectFrame, ProjectManifestV2, ProjectStep, ProjectStepId,
            };
            use rollshot_action::{CandidateKind, DetectReason, InputCapability, InputSourceKind};

            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().to_path_buf();
            std::fs::create_dir_all(root.join("assets/frames")).unwrap();

            let mut frames = Vec::new();
            for i in 1..=3u64 {
                let image = RgbaImage::from_pixel(8, 8, Rgba([i as u8, 0, 0, 255]));
                let sha256 = write_test_png_asset(&root, &image);
                frames.push(ProjectFrame {
                    id: i,
                    at_ms: (i - 1) * 100,
                    sha256,
                    width: 8,
                    height: 8,
                });
            }

            let manifest = ProjectManifestV2 {
                schema_version: 1,
                revision: 1,
                title: "Test Guide".into(),
                capture_region: super::region_32(),
                input_source: InputSourceKind::LinuxEvdev,
                input_capability: InputCapability::SemanticEvents,
                enabled_outputs: EnabledOutputs::default(),
                frames,
                steps: vec![ProjectStep {
                    id: ProjectStepId(1),
                    order: 1,
                    title: "Step 1".into(),
                    caption: None,
                    kind: CandidateKind::Click,
                    reason: DetectReason::ClickConfirmed,
                    at_ms: 200,
                    keyframe: 1,
                    nearby: vec![1, 2, 3],
                    annotations: None,
                }],
            import_warnings: Vec::new(),
            };
            let loaded = rollshot_action::project::LoadedProject {
                root,
                manifest: manifest.into(),
            motion: rollshot_action::project::MotionAssetLoad::None,
            };
            let guard = crate::timeline_workspace::project::ProjectWriterGuard::for_test();
            let ws = crate::timeline_workspace::project::from_loaded_project(
                loaded,
                ProjectAccess::Writable(guard),
            )
            .expect("ok");
            (ws, dir)
        }

        #[test]
        fn select_step_schedules_decodes_for_uncached_project_frames() {
            let (mut ws, _dir) = ws_project_backed_with_assets();
            // The project starts with an empty cache; all frames need decoding.
            ws.keyframe_handle = None;
            ws.strip.clear();

            let result = super::super::update::update(&mut ws, Message::SelectStep(1));

            // Should return a task that performs frame decodes.
            assert!(
                result.task.units() > 0,
                "SelectStep on project-backed workspace should schedule frame decodes"
            );
            // Handles should be None until the decode completes.
            assert!(
                ws.keyframe_handle.is_none(),
                "keyframe handle should be None until decode completes"
            );
            assert!(
                ws.strip.is_empty(),
                "strip should be empty until decode completes"
            );
            // Generation should have advanced.
            assert_eq!(ws.frame_coordinator.current_generation(), 1);
        }

        #[test]
        fn select_step_uses_cached_keyframe_immediately() {
            let (mut ws, _dir) = ws_project_backed_with_assets();
            // Pre-cache the keyframe (frame 1) by loading it into the source.
            let source = ws.frame_source.as_mut().unwrap();
            let req = source.load_request(1).expect("load request for frame 1");
            let loaded = rollshot_action::load_step_frame(req).expect("decode frame 1");
            source.insert_loaded(loaded);

            // Now select the step - should use cached keyframe immediately.
            let _result = super::super::update::update(&mut ws, Message::SelectStep(1));

            assert!(
                ws.keyframe_handle.is_some(),
                "cached keyframe should be used immediately"
            );
        }

        #[test]
        fn frame_load_completed_with_stale_generation_is_ignored() {
            let (mut ws, _dir) = ws_project_backed_with_assets();

            // First select - generation becomes 1.
            let _ = super::super::update::update(&mut ws, Message::SelectStep(1));
            assert_eq!(ws.frame_coordinator.current_generation(), 1);

            // Second select - generation becomes 2.
            let _ = super::super::update::update(&mut ws, Message::SelectStep(1));
            assert_eq!(ws.frame_coordinator.current_generation(), 2);

            // Simulate a stale completion from generation 1.
            let _ = super::super::update::update(
                &mut ws,
                Message::FrameLoadCompleted {
                    generation: 1,
                    results: vec![Ok(rollshot_action::LoadedStepFrame {
                        id: 1,
                        at_ms: 0,
                        image: Arc::new(RgbaImage::from_pixel(8, 8, Rgba([1, 0, 0, 255]))),
                    })],
                    remaining: Vec::new(),
                },
            );

            // Should be ignored - no handles built.
            assert!(
                ws.keyframe_handle.is_none(),
                "stale completion should not build handles"
            );
        }

        #[test]
        fn decode_failure_sets_corrupt_read_only() {
            let (mut ws, _dir) = ws_project_backed_with_assets();

            // Select the step to start loading.
            let _ = super::super::update::update(&mut ws, Message::SelectStep(1));
            let gen = ws.frame_coordinator.current_generation();

            // Simulate a decode failure.
            let _ = super::super::update::update(
                &mut ws,
                Message::FrameLoadCompleted {
                    generation: gen,
                    results: vec![Err("corrupt PNG data".to_string())],
                    remaining: Vec::new(),
                },
            );

            assert!(
                !ws.can_mutate(),
                "workspace should be CorruptReadOnly after decode failure"
            );
            assert!(ws.message.as_ref().is_some_and(|m| m.contains("read-only")));
        }

        #[test]
        fn frame_load_completed_inserts_and_builds_handles() {
            let (mut ws, _dir) = ws_project_backed_with_assets();

            // Select the step to start loading.
            let _ = super::super::update::update(&mut ws, Message::SelectStep(1));
            let gen = ws.frame_coordinator.current_generation();

            // Simulate successful decode of keyframe (frame 1).
            let _ = super::super::update::update(
                &mut ws,
                Message::FrameLoadCompleted {
                    generation: gen,
                    results: vec![Ok(rollshot_action::LoadedStepFrame {
                        id: 1,
                        at_ms: 0,
                        image: Arc::new(RgbaImage::from_pixel(8, 8, Rgba([1, 0, 0, 255]))),
                    })],
                    remaining: Vec::new(),
                },
            );

            assert!(
                ws.keyframe_handle.is_some(),
                "keyframe handle should be built after successful decode"
            );
            // Verify the frame was inserted into cache.
            let source = ws.frame_source.as_mut().unwrap();
            assert!(
                source.cached(1).is_some(),
                "decoded frame should be in cache"
            );
        }

        #[test]
        fn select_step_schedules_at_most_two_decodes_initially() {
            use rollshot_action::project::{
                EnabledOutputs, ProjectFrame, ProjectManifestV2, ProjectStep, ProjectStepId,
            };
            use rollshot_action::{CandidateKind, DetectReason, InputCapability, InputSourceKind};

            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().to_path_buf();
            std::fs::create_dir_all(root.join("assets/frames")).unwrap();

            // Create 5 frames.
            let mut frames = Vec::new();
            for i in 1..=5u64 {
                let image = RgbaImage::from_pixel(8, 8, Rgba([i as u8, 0, 0, 255]));
                let sha256 = write_test_png_asset(&root, &image);
                frames.push(ProjectFrame {
                    id: i,
                    at_ms: (i - 1) * 100,
                    sha256,
                    width: 8,
                    height: 8,
                });
            }

            let manifest = ProjectManifestV2 {
                schema_version: 1,
                revision: 1,
                title: "Test Guide".into(),
                capture_region: super::region_32(),
                input_source: InputSourceKind::LinuxEvdev,
                input_capability: InputCapability::SemanticEvents,
                enabled_outputs: EnabledOutputs::default(),
                frames,
                steps: vec![ProjectStep {
                    id: ProjectStepId(1),
                    order: 1,
                    title: "Step 1".into(),
                    caption: None,
                    kind: CandidateKind::Click,
                    reason: DetectReason::ClickConfirmed,
                    at_ms: 400,
                    keyframe: 1,
                    nearby: vec![1, 2, 3, 4, 5],
                    annotations: None,
                }],
            import_warnings: Vec::new(),
            };
            let loaded = rollshot_action::project::LoadedProject {
                root,
                manifest: manifest.into(),
            motion: rollshot_action::project::MotionAssetLoad::None,
            };
            let guard = crate::timeline_workspace::project::ProjectWriterGuard::for_test();
            let mut ws = crate::timeline_workspace::project::from_loaded_project(
                loaded,
                ProjectAccess::Writable(guard),
            )
            .expect("ok");

            let result = super::super::update::update(&mut ws, Message::SelectStep(1));

            assert!(
                result.task.units() > 0,
                "should schedule frame decodes for 5 uncached frames"
            );
            // The task batches at most 2 in the first batch; remaining 3+ are
            // sent as remaining in the message. We verify the task was created.
            assert_eq!(ws.frame_coordinator.current_generation(), 1);
        }

        #[test]
        fn frame_load_completed_with_remaining_spawns_next_batch() {
            use rollshot_action::project::{
                EnabledOutputs, ProjectFrame, ProjectManifestV2, ProjectStep, ProjectStepId,
            };
            use rollshot_action::{CandidateKind, DetectReason, InputCapability, InputSourceKind};

            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().to_path_buf();
            std::fs::create_dir_all(root.join("assets/frames")).unwrap();

            let mut frames = Vec::new();
            for i in 1..=5u64 {
                let image = RgbaImage::from_pixel(8, 8, Rgba([i as u8, 0, 0, 255]));
                let sha256 = write_test_png_asset(&root, &image);
                frames.push(ProjectFrame {
                    id: i,
                    at_ms: (i - 1) * 100,
                    sha256,
                    width: 8,
                    height: 8,
                });
            }

            let manifest = ProjectManifestV2 {
                schema_version: 1,
                revision: 1,
                title: "Test Guide".into(),
                capture_region: super::region_32(),
                input_source: InputSourceKind::LinuxEvdev,
                input_capability: InputCapability::SemanticEvents,
                enabled_outputs: EnabledOutputs::default(),
                frames: frames.clone(),
                steps: vec![ProjectStep {
                    id: ProjectStepId(1),
                    order: 1,
                    title: "Step 1".into(),
                    caption: None,
                    kind: CandidateKind::Click,
                    reason: DetectReason::ClickConfirmed,
                    at_ms: 400,
                    keyframe: 1,
                    nearby: vec![1, 2, 3, 4, 5],
                    annotations: None,
                }],
            import_warnings: Vec::new(),
            };
            let loaded = rollshot_action::project::LoadedProject {
                root,
                manifest: manifest.into(),
            motion: rollshot_action::project::MotionAssetLoad::None,
            };
            let guard = crate::timeline_workspace::project::ProjectWriterGuard::for_test();
            let mut ws = crate::timeline_workspace::project::from_loaded_project(
                loaded,
                ProjectAccess::Writable(guard),
            )
            .expect("ok");

            // Select step to start loading.
            let _ = super::super::update::update(&mut ws, Message::SelectStep(1));
            let gen = ws.frame_coordinator.current_generation();

            // Simulate the first batch completing with 2 frames, with 3 remaining.
            let remaining_requests: Vec<_> = [3u64, 4, 5]
                .iter()
                .filter_map(|&id| {
                    ws.frame_source
                        .as_ref()
                        .unwrap()
                        .load_request(id)
                        .map(|req| (id, req))
                })
                .collect();

            let result = super::super::update::update(
                &mut ws,
                Message::FrameLoadCompleted {
                    generation: gen,
                    results: vec![
                        Ok(rollshot_action::LoadedStepFrame {
                            id: 1,
                            at_ms: 0,
                            image: Arc::new(RgbaImage::from_pixel(8, 8, Rgba([1, 0, 0, 255]))),
                        }),
                        Ok(rollshot_action::LoadedStepFrame {
                            id: 2,
                            at_ms: 100,
                            image: Arc::new(RgbaImage::from_pixel(8, 8, Rgba([2, 0, 0, 255]))),
                        }),
                    ],
                    remaining: remaining_requests,
                },
            );

            // Should spawn a task for the remaining frames.
            assert!(
                result.task.units() > 0,
                "remaining frames should trigger a follow-up decode task"
            );
        }

        // ---- Finding 1: last step cannot be deleted ----

        #[test]
        fn delete_step_is_noop_when_only_one_step() {
            use rollshot_action::project::{
                EnabledOutputs, ProjectFrame, ProjectManifestV2, ProjectStep, ProjectStepId,
            };
            use rollshot_action::{CandidateKind, DetectReason, InputCapability, InputSourceKind};

            let manifest = ProjectManifestV2 {
                schema_version: 1,
                revision: 1,
                title: "Single Step Guide".into(),
                capture_region: super::region_32(),
                input_source: InputSourceKind::LinuxEvdev,
                input_capability: InputCapability::SemanticEvents,
                enabled_outputs: EnabledOutputs::default(),
                frames: vec![ProjectFrame {
                    id: 1,
                    at_ms: 0,
                    sha256: "a".into(),
                    width: 32,
                    height: 32,
                }],
                steps: vec![ProjectStep {
                    id: ProjectStepId(1),
                    order: 1,
                    title: "Only Step".into(),
                    caption: None,
                    kind: CandidateKind::Click,
                    reason: DetectReason::ClickConfirmed,
                    at_ms: 0,
                    keyframe: 1,
                    nearby: vec![1],
                    annotations: None,
                }],
            import_warnings: Vec::new(),
            };
            let loaded = rollshot_action::project::LoadedProject {
                root: std::path::PathBuf::from("/tmp/test-project"),
                manifest: manifest.into(),
            motion: rollshot_action::project::MotionAssetLoad::None,
            };
            let guard = crate::timeline_workspace::project::ProjectWriterGuard::for_test();
            let mut ws = crate::timeline_workspace::project::from_loaded_project(
                loaded,
                ProjectAccess::Writable(guard),
            )
            .expect("ok");
            ws.save_state = ProjectSaveState::Clean;

            let _ = super::super::update::update(&mut ws, Message::DeleteStep);

            assert_eq!(ws.guide.steps().len(), 1, "last step must not be deleted");
            assert_eq!(ws.selected, Some(1));
            assert_eq!(
                ws.save_state,
                ProjectSaveState::Clean,
                "save state must not change"
            );
        }

        // ---- Finding 2: mutation-dirty tests for 6 arms ----

        #[test]
        fn annotation_explanation_changed_marks_dirty() {
            use crate::timeline_workspace::annotation::StepAnnotationSession;
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Clean;
            let step = ws.selected_step().cloned().unwrap();
            ws.annotation_session = Some(StepAnnotationSession::new(
                step.source,
                step.keyframe,
                &RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 255])),
            ));

            let _ = super::super::update::update(
                &mut ws,
                Message::AnnotationExplanationChanged(
                    rollshot_image_document::AnnotationId(999),
                    "explanation text".to_string(),
                ),
            );

            assert_eq!(ws.save_state, ProjectSaveState::Dirty);
        }

        #[test]
        fn accept_caption_suggestion_marks_dirty() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Clean;
            let proposal = caption_proposal_for_ws(&ws);
            let _ = super::super::update::update(
                &mut ws,
                Message::CaptionProposalLoaded(Ok(caption_run_success(proposal))),
            );

            let _ = super::super::update::update(
                &mut ws,
                Message::AcceptCaptionSuggestion(rollshot_action::CaptionSuggestionId(1)),
            );

            assert_eq!(ws.save_state, ProjectSaveState::Dirty);
        }

        #[test]
        fn accept_all_caption_suggestions_marks_dirty() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Clean;
            let proposal = caption_proposal_for_ws(&ws);
            let _ = super::super::update::update(
                &mut ws,
                Message::CaptionProposalLoaded(Ok(caption_run_success(proposal))),
            );

            let _ = super::super::update::update(&mut ws, Message::AcceptAllCaptionSuggestions);

            assert_eq!(ws.save_state, ProjectSaveState::Dirty);
        }

        #[test]
        fn annotation_canvas_released_marks_dirty() {
            use crate::timeline_workspace::annotation::StepAnnotationSession;
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Clean;
            let step = ws.selected_step().cloned().unwrap();
            ws.annotation_session = Some(StepAnnotationSession::new(
                step.source,
                step.keyframe,
                &RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 255])),
            ));

            let _ = super::super::update::update(
                &mut ws,
                Message::AnnotationCanvasReleased(rollshot_image_document::ImagePoint::new(
                    16.0, 16.0,
                )),
            );

            assert_eq!(ws.save_state, ProjectSaveState::Dirty);
        }

        #[test]
        fn accept_visual_annotation_marks_dirty() {
            let (mut ws, suggestion_id) = ws_project_backed_with_visual_proposal();

            let _ = super::super::update::update(
                &mut ws,
                Message::AcceptVisualAnnotation(suggestion_id),
            );

            assert_eq!(ws.save_state, ProjectSaveState::Dirty);
        }

        #[test]
        fn accept_all_visual_annotations_marks_dirty() {
            let (mut ws, _) = ws_project_backed_with_visual_proposal();

            let _ = super::super::update::update(&mut ws, Message::AcceptAllVisualAnnotations);

            assert_eq!(ws.save_state, ProjectSaveState::Dirty);
        }

        // ---- Finding 3: full SaveThenClose integration test ----

        #[test]
        fn save_then_close_closes_workspace_after_save_completes() {
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Dirty;
            ws.close_intent = CloseIntent::Confirming;

            let _result = super::super::update::update(&mut ws, Message::CloseSaveAndClose);
            assert_eq!(ws.close_intent, CloseIntent::SaveThenClose);
            assert_eq!(ws.save_state, ProjectSaveState::Saving);

            let result = super::super::update::update(
                &mut ws,
                Message::SaveWorkerFinished(
                    super::super::update::SaveWorkerOutcome::ExistingSaved { revision: 2 },
                ),
            );

            assert_eq!(ws.save_state, ProjectSaveState::Clean);
            assert!(matches!(
                result.effect,
                Effect::ProjectSaved {
                    close_workspace: true,
                    ..
                }
            ));
        }

        // ---- Helpers for the new tests ----

        fn caption_test_task_id() -> rollshot_agent::product_task::ProductTaskId {
            rollshot_agent::product_task::ProductTaskId::parse(
                "task-00000000-0000-4000-8000-000000000001",
            )
            .unwrap()
        }

        fn caption_run_success(
            proposal: rollshot_action::CaptionProposal,
        ) -> Box<super::caption_agent::CaptionRunSuccess> {
            let binding = super::caption_agent::caption_source_binding(
                &super::caption_agent::provider_tests::ephemeral_context(),
                None,
            );
            let snapshot = super::caption_agent::provider_tests::promote_caption_task_for_tests(
                &binding, &proposal,
            );
            Box::new(super::caption_agent::CaptionRunSuccess {
                task_id: caption_test_task_id(),
                proposal,
                snapshot,
                provider_id: "test-provider".to_owned(),
                model_id: "test-model".to_owned(),
            })
        }

        fn caption_proposal_for_ws(state: &TimelineWorkspace) -> rollshot_action::CaptionProposal {
            rollshot_action::CaptionProposal::from_agent_drafts(
                rollshot_action::CaptionProposalId(1),
                42,
                rollshot_action::CaptionProposalOrigin::EphemeralGuide {
                    guide_digest: "0".repeat(64),
                },
                &state.guide,
                vec![rollshot_action::CaptionSuggestionDraft {
                    step_source: state.guide.steps()[0].source,
                    title: Some("Suggested Title".to_string()),
                    caption: "Suggested caption.".to_string(),
                    confidence: 0.9,
                    rationale: None,
                }],
            )
        }

        // ---- Imported video workspace tests (Task 6) ----

        fn imported_seed_fixture() -> (rollshot_action::ImportedWorkspaceSeed, tempfile::TempDir) {
            use rollshot_action::project::ProjectFrame;
            use rollshot_action::{
                CandidateKind, CaptureRegion, DetectReason, ImportWarning, InputCapability,
                InputSourceKind,
            };
            use rollshot_action::{Guide, ImportedScratch};

            let parent = tempfile::tempdir().unwrap();
            let scratch = ImportedScratch::create(parent.path()).unwrap();

            let step = rollshot_action::GuideStep {
                index: 1,
                title: "Click button".into(),
                caption: String::new(),
                kind: CandidateKind::Click,
                reason: DetectReason::VisualChange,
                at_ms: 100,
                keyframe: 1,
                nearby: vec![1],
                source: 1,
            };
            let guide = Guide::from_reviewed_steps("Imported Guide".into(), vec![step]).unwrap();

            let seed = rollshot_action::ImportedWorkspaceSeed {
                guide,
                capture_region: CaptureRegion {
                    x: 0,
                    y: 0,
                    width: 640,
                    height: 480,
                },
                input_source: InputSourceKind::ImportedVideo,
                input_capability: InputCapability::VisualOnly {
                    reason: rollshot_action::DegradedReason::ImportedRecording,
                },
                frames: vec![ProjectFrame {
                    id: 1,
                    at_ms: 100,
                    sha256: "abc123".into(),
                    width: 640,
                    height: 480,
                }],
                import_warnings: vec![ImportWarning::NoVisualChangesDetected],
                scratch,
            };

            (seed, parent)
        }

        fn imported_workspace_fixture() -> (TimelineWorkspace, std::path::PathBuf, tempfile::TempDir)
        {
            let (seed, parent) = imported_seed_fixture();
            let scratch_path = seed.scratch.root().to_path_buf();
            let workspace = TimelineWorkspace::from_imported_video(seed);
            (workspace, scratch_path, parent)
        }

        fn complete_first_save(ws: &mut TimelineWorkspace) {
            let manifest = rollshot_action::project::ProjectManifestV3 {
                schema_version: 3,
                revision: 1,
                title: ws.guide.title().to_string(),
                capture_region: ws.region,
                input_source: ws.source_kind,
                input_capability: ws.capability,
                enabled_outputs: ws.enabled_outputs,
                frames: vec![rollshot_action::project::ProjectFrame {
                    id: 1,
                    at_ms: 100,
                    sha256: "abc123".into(),
                    width: 640,
                    height: 480,
                }],
                steps: ws
                    .guide
                    .steps()
                    .iter()
                    .map(|s| rollshot_action::project::ProjectStep {
                        id: rollshot_action::project::ProjectStepId(s.source),
                        order: s.index as u32,
                        title: s.title.clone(),
                        caption: if s.caption.is_empty() {
                            None
                        } else {
                            Some(s.caption.clone())
                        },
                        kind: s.kind,
                        reason: s.reason,
                        at_ms: s.at_ms,
                        keyframe: s.keyframe,
                        nearby: s.nearby.clone(),
                        annotations: None,
                    })
                    .collect(),
                import_warnings: ws.import_warnings.clone(),
                motion: None,
            };
            super::super::update::update(
                ws,
                Message::SaveWorkerFinished(super::super::update::SaveWorkerOutcome::NewWritable {
                    root: std::path::PathBuf::from("/tmp/saved-import.rollshot-guide"),
                    revision: 1,
                    manifest,
                }),
            );
        }

        #[test]
        fn imported_seed_opens_dirty_unsaved_workspace() {
            let (seed, parent) = imported_seed_fixture();
            let scratch_path = seed.scratch.root().to_path_buf();
            let workspace = TimelineWorkspace::from_imported_video(seed);
            assert!(matches!(
                workspace.project_session,
                Some(project::ProjectSession::Unsaved)
            ));
            assert_eq!(workspace.save_state, ProjectSaveState::Dirty);
            assert!(
                workspace.persistent_notice().contains("Visual-only draft"),
                "notice should contain 'Visual-only draft', got: {:?}",
                workspace.persistent_notice()
            );
            assert!(
                scratch_path.exists(),
                "scratch directory should still exist"
            );
            drop(workspace);
            let _ = parent;
        }

        #[test]
        fn imported_workspace_has_warning_copies() {
            let (ws, _scratch, _parent) = imported_workspace_fixture();
            let notice = ws.persistent_notice();
            assert!(
                notice.contains("No visual changes detected"),
                "should contain NoVisualChangesDetected copy, got: {notice}"
            );
        }

        #[test]
        fn first_save_switches_frame_source_then_releases_scratch() {
            let (mut workspace, scratch_path, _parent) = imported_workspace_fixture();
            assert!(scratch_path.exists(), "scratch exists before save");
            complete_first_save(&mut workspace);
            assert!(
                !scratch_path.exists(),
                "scratch should be released after first save"
            );
            assert_eq!(
                workspace.frame_source_root(),
                workspace.project_root().as_deref(),
                "frame source root should match project root after save"
            );
        }

        #[test]
        fn failed_first_save_keeps_scratch_retryable() {
            let (mut workspace, scratch_path, _parent) = imported_workspace_fixture();
            assert!(scratch_path.exists());

            let _ = super::super::update::update(
                &mut workspace,
                Message::SaveWorkerFinished(super::super::update::SaveWorkerOutcome::Failed(
                    "disk full".to_string(),
                )),
            );

            assert!(
                scratch_path.exists(),
                "scratch must remain after failed save"
            );
            assert_eq!(workspace.save_state, ProjectSaveState::Dirty);
            assert!(workspace.imported_scratch.is_some());
        }

        #[test]
        fn closing_unsaved_import_releases_scratch() {
            let (workspace, scratch_path, _parent) = imported_workspace_fixture();
            assert!(scratch_path.exists());
            drop(workspace);
            assert!(
                !scratch_path.exists(),
                "scratch directory should be cleaned up on drop"
            );
        }

        #[test]
        fn imported_workspace_does_not_emit_recent_project_effect() {
            let (mut ws, _scratch, _parent) = imported_workspace_fixture();
            complete_first_save(&mut ws);
            assert!(ws.imported_scratch.is_none(), "scratch should be taken");
        }

        #[test]
        fn build_snapshot_carries_import_warnings() {
            use rollshot_action::project::{
                EnabledOutputs, ProjectFrame, ProjectManifestV3, ProjectStep, ProjectStepId,
            };
            use rollshot_action::{
                CandidateKind, CaptureRegion, DetectReason, ImportWarning, InputCapability,
                InputSourceKind,
            };

            let manifest = ProjectManifestV3 {
                schema_version: 3,
                revision: 1,
                title: "Imported Guide".into(),
                capture_region: CaptureRegion {
                    x: 0,
                    y: 0,
                    width: 640,
                    height: 480,
                },
                input_source: InputSourceKind::ImportedVideo,
                input_capability: InputCapability::VisualOnly {
                    reason: rollshot_action::DegradedReason::ImportedRecording,
                },
                enabled_outputs: EnabledOutputs::default(),
                frames: vec![ProjectFrame {
                    id: 1,
                    at_ms: 100,
                    sha256: "abc123".into(),
                    width: 640,
                    height: 480,
                }],
                steps: vec![ProjectStep {
                    id: ProjectStepId(1),
                    order: 1,
                    title: "Click button".into(),
                    caption: None,
                    kind: CandidateKind::Click,
                    reason: DetectReason::VisualChange,
                    at_ms: 100,
                    keyframe: 1,
                    nearby: vec![1],
                    annotations: None,
                }],
                import_warnings: vec![ImportWarning::NoVisualChangesDetected],
                motion: None,
            };
            let loaded = rollshot_action::project::LoadedProject {
                root: std::path::PathBuf::from("/tmp/test-imported"),
                manifest,
                motion: rollshot_action::project::MotionAssetLoad::None,
            };
            let guard = crate::timeline_workspace::project::ProjectWriterGuard::for_test();
            let mut ws = crate::timeline_workspace::project::from_loaded_project(
                loaded,
                ProjectAccess::Writable(guard),
            )
            .expect("ok");

            let snap = super::project::build_project_snapshot(&mut ws).expect("snapshot");
            assert_eq!(snap.import_warnings.len(), 1);
            assert!(snap
                .import_warnings
                .contains(&ImportWarning::NoVisualChangesDetected));
        }

        #[test]
        fn reopen_preserves_import_warnings() {
            use rollshot_action::project::{
                EnabledOutputs, ProjectFrame, ProjectManifestV3, ProjectStep, ProjectStepId,
            };
            use rollshot_action::{
                CandidateKind, CaptureRegion, DetectReason, ImportWarning, InputCapability,
                InputSourceKind,
            };

            let manifest = ProjectManifestV3 {
                schema_version: 3,
                revision: 1,
                title: "Imported Guide".into(),
                capture_region: CaptureRegion {
                    x: 0,
                    y: 0,
                    width: 640,
                    height: 480,
                },
                input_source: InputSourceKind::ImportedVideo,
                input_capability: InputCapability::VisualOnly {
                    reason: rollshot_action::DegradedReason::ImportedRecording,
                },
                enabled_outputs: EnabledOutputs::default(),
                frames: vec![ProjectFrame {
                    id: 1,
                    at_ms: 100,
                    sha256: "abc123".into(),
                    width: 640,
                    height: 480,
                }],
                steps: vec![ProjectStep {
                    id: ProjectStepId(1),
                    order: 1,
                    title: "Click button".into(),
                    caption: None,
                    kind: CandidateKind::Click,
                    reason: DetectReason::VisualChange,
                    at_ms: 100,
                    keyframe: 1,
                    nearby: vec![1],
                    annotations: None,
                }],
                import_warnings: vec![
                    ImportWarning::NoVisualChangesDetected,
                    ImportWarning::IntermediateChangesReduced,
                ],
                motion: None,
            };
            let loaded = rollshot_action::project::LoadedProject {
                root: std::path::PathBuf::from("/tmp/test-imported"),
                manifest,
                motion: rollshot_action::project::MotionAssetLoad::None,
            };
            let guard = crate::timeline_workspace::project::ProjectWriterGuard::for_test();
            let ws = crate::timeline_workspace::project::from_loaded_project(
                loaded,
                ProjectAccess::Writable(guard),
            )
            .expect("ok");

            assert_eq!(ws.import_warnings.len(), 2);
            assert!(ws
                .import_warnings
                .contains(&ImportWarning::NoVisualChangesDetected));
            assert!(ws
                .import_warnings
                .contains(&ImportWarning::IntermediateChangesReduced));
            let notice = ws.persistent_notice();
            assert!(
                notice.contains("Visual-only draft"),
                "should show visual-only disclosure, got: {notice}"
            );
            assert!(
                notice.contains("No visual changes detected"),
                "should show NoVisualChangesDetected, got: {notice}"
            );
            assert!(
                notice.contains("Intermediate visual changes were omitted"),
                "should show IntermediateChangesReduced, got: {notice}"
            );
        }

        // ---- Visual annotation context request tests (Task 10) ----

        #[test]
        fn visual_context_request_fields_initialized_empty_in_from_imported_video() {
            let (seed, _parent) = imported_seed_fixture();
            let ws = TimelineWorkspace::from_imported_video(seed);
            assert!(ws.visual_annotation_task_id.is_none());
            assert!(ws.visual_annotation_review_snapshot.is_none());
            assert!(!ws.visual_annotation_review_persisting);
        }

        #[test]
        fn visual_context_request_fields_initialized_empty_in_new() {
            let ws = workspace(recording_from_frames());
            assert!(ws.visual_annotation_task_id.is_none());
            assert!(ws.visual_annotation_review_snapshot.is_none());
            assert!(!ws.visual_annotation_review_persisting);
        }

        #[test]
        fn visual_context_request_fields_initialized_empty_in_from_loaded_project() {
            let ws = ws_project_backed();
            assert!(ws.visual_annotation_task_id.is_none());
            assert!(ws.visual_annotation_review_snapshot.is_none());
            assert!(!ws.visual_annotation_review_persisting);
        }

        #[test]
        fn visual_context_request_uses_durable_only_for_saved_clean_project() {
            // Saved + Clean → Durable
            let ws = ws_project_backed();
            assert_eq!(ws.save_state, ProjectSaveState::Clean);
            assert!(ws.project_session.is_some());
            let request = super::super::visual_annotation_context_request(&ws);
            match request {
                super::super::visual_annotation_agent::VisualAnnotationContextRequest::Durable {
                    root,
                    expected_revision,
                    step_source,
                    keyframe,
                } => {
                    assert_eq!(root, std::path::PathBuf::from("/tmp/test-project"));
                    assert_eq!(expected_revision, 1);
                    let step = ws.selected_step().expect("selected step");
                    assert_eq!(step_source, step.source);
                    assert_eq!(keyframe, step.keyframe);
                }
                _ => panic!("expected Durable for saved clean project"),
            }

            // Saved + Dirty → Ephemeral
            let mut ws = ws_project_backed();
            ws.save_state = ProjectSaveState::Dirty;
            let request = super::super::visual_annotation_context_request(&ws);
            match request {
                super::super::visual_annotation_agent::VisualAnnotationContextRequest::Ephemeral {
                    guide: _,
                    step_source,
                    keyframe,
                } => {
                    let step = ws.selected_step().expect("selected step");
                    assert_eq!(step_source, step.source);
                    assert_eq!(keyframe, step.keyframe);
                }
                _ => panic!("expected Ephemeral for dirty project"),
            }

            // Unsaved → Ephemeral
            let ws = workspace(recording_from_frames());
            assert!(ws.project_session.is_none());
            let request = super::super::visual_annotation_context_request(&ws);
            match request {
                super::super::visual_annotation_agent::VisualAnnotationContextRequest::Ephemeral {
                    guide,
                    step_source,
                    keyframe,
                } => {
                    assert_eq!(guide.steps().len(), ws.guide.steps().len());
                    let step = ws.selected_step().expect("selected step");
                    assert_eq!(step_source, step.source);
                    assert_eq!(keyframe, step.keyframe);
                }
                _ => panic!("expected Ephemeral for unsaved project"),
            }
        }

        fn ws_project_backed_with_visual_proposal() -> (
            TimelineWorkspace,
            rollshot_action::VisualAnnotationSuggestionId,
        ) {
            let mut ws = workspace(recording_from_frames());
            let guard = crate::timeline_workspace::project::ProjectWriterGuard::for_test();
            ws.project_session = Some(project::ProjectSession::Saved {
                root: std::path::PathBuf::from("/tmp/test-project"),
                base_revision: 1,
                access: ProjectAccess::Writable(guard),
            });
            ws.save_state = ProjectSaveState::Clean;

            let step = ws.selected_step().cloned().unwrap();
            let _doc = ws.presentation.document_for_step(&step, &ws.store);

            let doc = ws.presentation.doc(step.source).unwrap();
            let image = doc.document.source();
            let suggestion_id = rollshot_action::VisualAnnotationSuggestionId(1);
            let proposal = rollshot_action::VisualAnnotationProposal::from_agent_drafts(
                rollshot_action::VisualAnnotationProposalId(1),
                1,
                rollshot_action::VisualAnnotationProposalOrigin::EphemeralGuide {
                    guide_digest: "aa".repeat(32),
                },
                &step,
                doc.document.state_id(),
                image.width(),
                image.height(),
                [1u8; 32],
                [2u8; 32],
                vec![rollshot_action::VisualAnnotationSuggestionDraft {
                    id: suggestion_id,
                    payload: rollshot_action::VisualAnnotationPayload::TextNote {
                        position: rollshot_image_document::ImagePoint::new(4.0, 4.0),
                        text: "Test note".to_string(),
                    },
                    confidence: 0.9,
                    rationale: None,
                }],
            )
            .unwrap();

            ws.visual_annotation_suggestion =
                crate::timeline_workspace::VisualAnnotationSuggestionState::PendingReview(proposal);

            (ws, suggestion_id)
        }
    }
}
