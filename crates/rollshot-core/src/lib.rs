mod axis;
mod canvas;
mod diagnostics;
mod duplicate;
mod feature_matcher;
mod matcher;
mod metrics;
mod overlap;
mod stitcher;
mod types;
mod verifier;

pub use canvas::{CanvasAppendError, CanvasViewport, StripCanvas};
pub use metrics::{StitchMetrics, StitchOutcomeKind};
pub use stitcher::Stitcher;
pub use types::{
    AppendDirection, FastHnswConfig, MatchMethod, MatchStrategy, MotionCandidate, MotionEstimate,
    NoMatchReason, OverlapRegion, ScrollAxis, StitchConfig, StitchOutcome, StitchStats,
    VerifierConfig,
};
