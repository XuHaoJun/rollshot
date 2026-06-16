//! Linux semantic-input source for Action Guide. Observes global input through
//! read-only evdev access to `/dev/input/event*` (works under KDE Wayland
//! because it reads kernel input devices, not a compositor API). Exposes only
//! privacy-filtered semantic actions and explicit startup/runtime failure
//! reasons — never device paths, device names, or raw key codes. Implements
//! `rollshot_action::SemanticInputSource`. On non-Linux hosts the source is a
//! stub that reports `DegradedReason::SourceStartFailed` so the crate still
//! compiles in the workspace build. See
//! `docs/superpowers/specs/2026-06-15-action-guide-capture-design.md`.

mod classify;
mod source;

pub use classify::{EvdevClassifier, RawEvdevEvent};
pub use source::EvdevInputSource;

pub use rollshot_action::{
    CaptureRegion, DegradedReason, InputCapability, SemanticAction, SemanticInputSource,
    TimedSemanticAction,
};
