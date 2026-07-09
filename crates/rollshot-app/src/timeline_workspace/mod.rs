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

mod annotation;
mod update;
mod view;

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

#[derive(Debug, Clone)]
pub(crate) struct StoryboardPreviewState {
    pub handle: iced::widget::image::Handle,
    pub width: u32,
    pub height: u32,
    pub step_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IssuePackDialog {
    pub review_confirmed: bool,
    pub pending_kind: Option<IssuePackKind>,
    pub include_gif: bool,
}

impl IssuePackDialog {
    pub(crate) fn new() -> Self {
        Self {
            review_confirmed: false,
            pending_kind: None,
            include_gif: true,
        }
    }
}

/// One nearby-strip thumbnail: a retained frame id and its prebuilt iced handle.
pub(crate) struct StripFrame {
    pub id: FrameId,
    pub handle: iced::widget::image::Handle,
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
        };
        ws.rebuild_selection_handles();
        ws
    }

    /// The currently selected step, if any.
    pub(crate) fn selected_step(&self) -> Option<&GuideStep> {
        let index = self.selected?;
        self.guide.steps().iter().find(|s| s.index == index)
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
}
