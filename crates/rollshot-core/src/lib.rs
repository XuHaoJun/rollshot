mod axis;
mod canvas;
mod duplicate;
mod matcher;
mod overlap;
mod stitcher;
mod types;
mod verifier;

pub use canvas::{CanvasAppendError, LinearCanvas};
pub use stitcher::Stitcher;
pub use types::{
    AppendDirection, MatchMethod, MatchStrategy, MotionCandidate, MotionEstimate, NoMatchReason,
    OverlapRegion, ScrollAxis, StitchConfig, StitchOutcome, StitchStats, VerifierConfig,
};
