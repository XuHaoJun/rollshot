//! macOS semantic-input source for Action Guide. Observes global input through
//! a listen-only CoreGraphics `CGEventTap` on a dedicated CFRunLoop thread.
//! Exposes only privacy-filtered semantic actions and explicit failure reasons
//! — no Unicode text extraction, no input injection, no raw key persistence.
//! Implements `rollshot_action::SemanticInputSource`. This is an
//! unsafe-isolation crate (Objective-C / CoreFoundation FFI); its public API is
//! safe and the workspace keeps `unsafe_code = "forbid"` elsewhere, mirroring
//! `rollshot-macos-oneshot`. On non-macOS hosts the source is a stub reporting
//! `DegradedReason::SourceStartFailed`. See
//! `docs/superpowers/specs/2026-06-15-action-guide-capture-design.md`.

mod classify;
mod permission;
mod source;

pub use classify::{classify_cg, RawCgEvent, RawCgKind};
pub use permission::{
    input_monitoring_status, open_input_monitoring_settings, request_input_monitoring,
    InputMonitoringStatus,
};
pub use source::MacosInputSource;

pub use rollshot_action::{
    CaptureRegion, DegradedReason, InputCapability, MouseButton, SemanticAction,
    SemanticInputSource, SemanticKey, TimedSemanticAction,
};
