//! Platform-neutral Action Guide engine: frame ingestion, deterministic step
//! detection, the editable guide model, and export. Owns no windows, dialogs,
//! platform permissions, native event APIs, or capture backend — it is driven
//! by *pushed* `image::RgbaImage` frames plus privacy-filtered semantic events,
//! so it is fully fixture-testable on every CI host. Every public type carries
//! only privacy-filtered data: never raw key codes, typed text, device names,
//! or device paths. See `docs/superpowers/specs/2026-06-15-action-guide-capture-design.md`.

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
mod recorder;

pub use detector::{CandidateMarker, Detector, DetectorConfig};
pub use error::{DetectError, ExportError, GifError};
pub use events::EventAggregator;
pub use export::{export_guide, ManifestStep, SessionManifest};
pub use frame_store::{AnalysisFrame, FrameStore, RetainedFrame, StoreConfig};
pub use gif::{export_gif, GifOptions};
pub use guide::Guide;
pub use input::{SemanticInputSource, StartedSemanticInput, VisualOnlySource};
pub use metrics::{changed_area_ratio, downsample_luma, masked_luma_diff, LumaPlane, Rect};
pub use models::{
    default_title, CandidateId, CandidateKind, CandidateStep, CaptureRegion, DegradedReason,
    DetectReason, FrameId, FrameRef, GuideStep, InputCapability, InputSourceKind, Millis,
    MouseButton, Point, SemanticAction, SemanticKey, TimedSemanticAction,
};
pub use recorder::{ActionRecorder, Recording};
