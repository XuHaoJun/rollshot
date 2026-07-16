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
mod storyboard_copy;
mod update;
mod view;
#[cfg(feature = "action-guide")]
mod visual_annotation_agent;

pub use update::{subscription, update, Message};
pub use view::view;

use rollshot_action::{
    CaptureRegion, FrameId, FrameStore, Guide, GuideStep, InputCapability, InputSourceKind,
    Recording,
};

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
    /// Current visual annotation suggestion state. See [`VisualAnnotationSuggestionState`].
    #[allow(dead_code)] // Read by Task 8's view; only the update path uses it here.
    pub(crate) visual_annotation_suggestion: VisualAnnotationSuggestionState,
    /// Monotonic local run id for visual annotation suggestion provenance.
    #[allow(dead_code)]
    pub(crate) visual_annotation_agent_run_id: u64,
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
}

impl TimelineWorkspace {
    /// Build the workspace from a finished recording. Selects step 1 (if any)
    /// and primes the selection handle cache.
    pub fn new(
        recording: Recording,
        region: CaptureRegion,
        capability: InputCapability,
        source_kind: InputSourceKind,
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
            visual_annotation_suggestion: VisualAnnotationSuggestionState::Idle,
            visual_annotation_agent_run_id: 0,
            storyboard_copy_operation_id: 0,
            export_state: GuideExportState::Idle,
            last_export: None,
            next_export_operation_id: 0,
            next_issue_pack_operation_id: 0,
        };
        ws.rebuild_selection_handles();
        ws
    }

    /// The currently selected step, if any.
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
        (
            TimelineWorkspace::new(recording, region, capability, source_kind),
            iced::Task::none(),
        )
    };

    iced::application(boot, update, view)
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
        VisualAnnotationSuggestionDraft, VisualAnnotationSuggestionId,
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
        rec.ingest_frame(black_32(), 0);
        for i in 1..=6 {
            rec.ingest_frame(white_quadrant_32(), i * 100);
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
            step,
            doc.document.state_id(),
            image.width(),
            image.height(),
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
}
