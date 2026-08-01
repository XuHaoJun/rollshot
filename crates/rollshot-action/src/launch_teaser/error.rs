//! Privacy-safe error types for launch teaser operations.
//!
//! Every variant maps to exactly one stable category string. Error messages
//! never contain user text, file paths, filter graphs, FFmpeg arguments,
//! model strings, or project content.

/// Error from launch teaser plan validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaunchTeaserError {
    #[error("unsupported launch teaser schema")]
    UnsupportedSchema,
    #[error("invalid launch teaser shot count")]
    ShotCount,
    #[error("invalid launch teaser source binding")]
    SourceBinding,
    #[error("invalid launch teaser source range")]
    SourceRange,
    #[error("invalid launch teaser focus path")]
    FocusPath,
    #[error("invalid launch teaser speed")]
    Speed,
    #[error("invalid launch teaser transition")]
    Transition,
    #[error("invalid launch teaser text")]
    Text,
    #[error("invalid launch teaser duration")]
    Duration,
    #[error("launch teaser arithmetic overflow")]
    ArithmeticOverflow,
}

impl LaunchTeaserError {
    /// Stable category string for programmatic matching.
    ///
    /// Categories are lowercase kebab-case and never carry user content.
    pub fn category(&self) -> &'static str {
        match self {
            Self::UnsupportedSchema => "unsupported-schema",
            Self::ShotCount => "shot-count",
            Self::SourceBinding => "source-binding",
            Self::SourceRange => "source-range",
            Self::FocusPath => "focus-path",
            Self::Speed => "speed",
            Self::Transition => "transition",
            Self::Text => "text",
            Self::Duration => "duration",
            Self::ArithmeticOverflow => "arithmetic-overflow",
        }
    }
}

/// Error from deterministic seed generation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaunchTeaserSeedError {
    #[error("insufficient reviewed steps for launch teaser")]
    InsufficientSteps,
    #[error("insufficient non-overlapping motion windows")]
    InsufficientMotion,
}

impl LaunchTeaserSeedError {
    pub fn category(&self) -> &'static str {
        match self {
            Self::InsufficientSteps => "insufficient-steps",
            Self::InsufficientMotion => "insufficient-motion",
        }
    }
}

/// Error from launch teaser binding validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaunchTeaserBindingError {
    #[error("launch teaser project is stale")]
    StaleProject,
    #[error("launch teaser motion is stale")]
    StaleMotion,
    #[error("launch teaser step is missing")]
    MissingStep,
}

impl LaunchTeaserBindingError {
    pub fn category(&self) -> &'static str {
        match self {
            Self::StaleProject => "stale-project",
            Self::StaleMotion => "stale-motion",
            Self::MissingStep => "missing-step",
        }
    }
}

/// Error from sidecar persistence.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaunchTeaserPersistenceError {
    #[error("launch teaser sidecar io error")]
    Io,
    #[error("launch teaser sidecar encoding error")]
    Encoding,
    #[error("launch teaser sidecar digest mismatch")]
    DigestMismatch,
}

impl LaunchTeaserPersistenceError {
    pub fn category(&self) -> &'static str {
        match self {
            Self::Io => "io",
            Self::Encoding => "encoding",
            Self::DigestMismatch => "digest-mismatch",
        }
    }
}

/// Error from launch teaser rendering.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaunchTeaserRenderError {
    #[error("launch teaser cancelled")]
    Cancelled,
    #[error("launch teaser toolchain unavailable")]
    ToolchainUnavailable,
    #[error("launch teaser binding validation failed")]
    BindingFailed,
    #[error("launch teaser plan validation failed")]
    PlanValidationFailed,
    #[error("launch teaser ffmpeg spawn failed")]
    FfmpegSpawnFailed,
    #[error("launch teaser ffmpeg execution failed")]
    FfmpegExecutionFailed,
    #[error("launch teaser ffprobe failed")]
    FfprobeFailed,
    #[error("launch teaser output verification failed")]
    OutputVerificationFailed,
    #[error("launch teaser overlay rasterization failed")]
    OverlayFailed,
    #[error("launch teaser graph compilation failed")]
    GraphCompilationFailed,
    #[error("launch teaser scratch creation failed")]
    ScratchFailed,
}

impl LaunchTeaserRenderError {
    pub fn category(&self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::ToolchainUnavailable => "toolchain-unavailable",
            Self::BindingFailed => "binding-failed",
            Self::PlanValidationFailed => "plan-validation-failed",
            Self::FfmpegSpawnFailed => "ffmpeg-spawn-failed",
            Self::FfmpegExecutionFailed => "ffmpeg-execution-failed",
            Self::FfprobeFailed => "ffprobe-failed",
            Self::OutputVerificationFailed => "output-verification-failed",
            Self::OverlayFailed => "overlay-failed",
            Self::GraphCompilationFailed => "graph-compilation-failed",
            Self::ScratchFailed => "scratch-failed",
        }
    }
}

/// Sidecar load state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchTeaserSidecarLoad {
    /// No sidecar file exists.
    Missing,
    /// Sidecar exists, is valid, and is current.
    Available(LaunchTeaserArtifactV1),
    /// Sidecar exists but is stale relative to the current project.
    Stale(LaunchTeaserArtifactV1),
    /// Sidecar exists but could not be parsed or validated.
    Unavailable,
}

/// The full persisted artifact DTO.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchTeaserArtifactV1 {
    pub schema_version: u32,
    pub plan: crate::launch_teaser::plan::LaunchTeaserPlanV1,
    pub plan_sha256: String,
    pub renderer_version: u32,
    pub ffmpeg_version: String,
    pub ffprobe_version: String,
    pub output_sha256: String,
    pub rendered_at_unix_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_categories_are_stable() {
        assert_eq!(LaunchTeaserError::UnsupportedSchema.category(), "unsupported-schema");
        assert_eq!(LaunchTeaserError::ShotCount.category(), "shot-count");
        assert_eq!(LaunchTeaserError::SourceBinding.category(), "source-binding");
        assert_eq!(LaunchTeaserError::SourceRange.category(), "source-range");
        assert_eq!(LaunchTeaserError::FocusPath.category(), "focus-path");
        assert_eq!(LaunchTeaserError::Speed.category(), "speed");
        assert_eq!(LaunchTeaserError::Transition.category(), "transition");
        assert_eq!(LaunchTeaserError::Text.category(), "text");
        assert_eq!(LaunchTeaserError::Duration.category(), "duration");
        assert_eq!(LaunchTeaserError::ArithmeticOverflow.category(), "arithmetic-overflow");
    }

    #[test]
    fn seed_error_categories_are_stable() {
        assert_eq!(LaunchTeaserSeedError::InsufficientSteps.category(), "insufficient-steps");
        assert_eq!(LaunchTeaserSeedError::InsufficientMotion.category(), "insufficient-motion");
    }

    #[test]
    fn binding_error_categories_are_stable() {
        assert_eq!(LaunchTeaserBindingError::StaleProject.category(), "stale-project");
        assert_eq!(LaunchTeaserBindingError::StaleMotion.category(), "stale-motion");
        assert_eq!(LaunchTeaserBindingError::MissingStep.category(), "missing-step");
    }

    #[test]
    fn persistence_error_categories_are_stable() {
        assert_eq!(LaunchTeaserPersistenceError::Io.category(), "io");
        assert_eq!(LaunchTeaserPersistenceError::Encoding.category(), "encoding");
        assert_eq!(LaunchTeaserPersistenceError::DigestMismatch.category(), "digest-mismatch");
    }

    #[test]
    fn render_error_categories_are_stable() {
        assert_eq!(LaunchTeaserRenderError::Cancelled.category(), "cancelled");
        assert_eq!(LaunchTeaserRenderError::ToolchainUnavailable.category(), "toolchain-unavailable");
        assert_eq!(LaunchTeaserRenderError::BindingFailed.category(), "binding-failed");
        assert_eq!(LaunchTeaserRenderError::PlanValidationFailed.category(), "plan-validation-failed");
        assert_eq!(LaunchTeaserRenderError::FfmpegSpawnFailed.category(), "ffmpeg-spawn-failed");
        assert_eq!(LaunchTeaserRenderError::FfmpegExecutionFailed.category(), "ffmpeg-execution-failed");
        assert_eq!(LaunchTeaserRenderError::FfprobeFailed.category(), "ffprobe-failed");
        assert_eq!(LaunchTeaserRenderError::OutputVerificationFailed.category(), "output-verification-failed");
        assert_eq!(LaunchTeaserRenderError::OverlayFailed.category(), "overlay-failed");
        assert_eq!(LaunchTeaserRenderError::GraphCompilationFailed.category(), "graph-compilation-failed");
        assert_eq!(LaunchTeaserRenderError::ScratchFailed.category(), "scratch-failed");
    }
}
