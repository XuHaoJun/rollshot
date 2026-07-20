//! Platform-neutral Action Guide engine: frame ingestion, deterministic step
//! detection, the editable guide model, and export. Owns no windows, dialogs,
//! platform permissions, native event APIs, or capture backend — it is driven
//! by *pushed* `image::RgbaImage` frames plus privacy-filtered semantic events,
//! so it is fully fixture-testable on every CI host. Every public type carries
//! only privacy-filtered data: never raw key codes, typed text, device names,
//! or device paths. See `docs/superpowers/specs/2026-06-15-action-guide-capture-design.md`.

pub mod caption_proposal;
mod detector;
mod diagnostics;
mod error;
mod events;
mod export;
mod frame_store;
mod gif;
mod guide;
mod input;
mod metrics;
mod models;
pub mod project;
mod recorder;
#[cfg(test)]
mod semantic_fixture_tests;
pub mod step_frame_source;
mod storyboard;
mod video;
pub mod video_import;
pub mod visual_annotation_proposal;

pub use caption_proposal::{
    CaptionApplyOutcome, CaptionProposal, CaptionProposalId, CaptionProposalProvenance,
    CaptionSuggestion, CaptionSuggestionDraft, CaptionSuggestionId, CaptionSuggestionStatus,
};
pub use detector::{CandidateMarker, Detector, DetectorConfig};
pub use error::{DetectError, ExportError, GifError, StoryboardError, VideoError};
pub use events::EventAggregator;
pub use export::model::{
    GuideHotspot, NormalizedRect, ProjectReviewedImage, ReviewedGuideExportJob, ReviewedGuideStep,
    ReviewedStepImage, GUIDE_SCHEMA_VERSION,
};
pub use export::{model as export_model, render_guide_folder, ManifestStep, SessionManifest};
pub use frame_store::{AnalysisFrame, FrameStore, RetainedFrame, StoreConfig};
pub use gif::{export_gif, export_gif_images, export_reviewed_gif, GifOptions};
pub use guide::{Guide, DEFAULT_GUIDE_TITLE};
pub use input::{SemanticInputSource, StartedSemanticInput, VisualOnlySource};
pub use metrics::{changed_area_ratio, downsample_luma, masked_luma_diff, LumaPlane, Rect};
pub use models::{
    default_title, CandidateId, CandidateKind, CandidateStep, CaptureRegion, DegradedReason,
    DetectReason, FrameId, FrameRef, GuideStep, ImportWarning, InputCapability, InputSourceKind,
    Millis, MouseButton, Point, SemanticAction, SemanticKey, TimedSemanticAction,
};
pub use recorder::{ActionRecorder, Recording};
pub use step_frame_source::{
    load_step_frame, LoadedStepFrame, ProjectFrameSource, StepFrameLoadRequest, StepFrameSource,
    DEFAULT_PROJECT_FRAME_CACHE_BYTES,
};
pub use storyboard::{
    export_reviewed_storyboard_cancellable, export_storyboard, render_reviewed_storyboard,
    render_reviewed_storyboard_cancellable, render_storyboard, render_storyboard_steps,
    StoryboardExportResult, StoryboardOptions, StoryboardRenderResult, StoryboardStep,
};
pub use video::{export_reviewed_video, export_video, VideoOptions};
pub use visual_annotation_proposal::{
    VisualAnnotationApplyOutcome, VisualAnnotationBase, VisualAnnotationPayload,
    VisualAnnotationProposal, VisualAnnotationProposalError, VisualAnnotationProposalId,
    VisualAnnotationProvenance, VisualAnnotationSuggestion, VisualAnnotationSuggestionDraft,
    VisualAnnotationSuggestionId, VisualAnnotationSuggestionStatus, MAX_VISUAL_SUGGESTIONS,
};
