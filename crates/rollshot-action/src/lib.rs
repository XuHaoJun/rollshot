//! Platform-neutral Action Guide engine: frame ingestion, deterministic step
//! detection, the editable guide model, and export. Owns no windows, dialogs,
//! platform permissions, native event APIs, or capture backend — it is driven
//! by *pushed* `image::RgbaImage` frames plus privacy-filtered semantic events,
//! so it is fully fixture-testable on every CI host. Every public type carries
//! only privacy-filtered data: never raw key codes, typed text, device names,
//! or device paths. See `docs/superpowers/specs/2026-06-15-action-guide-capture-design.md`.

mod models;

pub use models::{
    default_title, CandidateId, CandidateKind, CandidateStep, CaptureRegion, DegradedReason,
    DetectReason, FrameId, FrameRef, GuideStep, InputCapability, InputSourceKind, Millis,
    MouseButton, Point, SemanticAction, SemanticKey, TimedSemanticAction,
};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        // Scaffold smoke test; modules are added by later tasks.
        assert_eq!(2 + 2, 4);
    }
}
