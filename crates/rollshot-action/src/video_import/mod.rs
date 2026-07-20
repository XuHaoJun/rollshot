pub mod selection;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub use selection::{evidence_sample_indices, CandidateSelector, SelectionResult};

pub const ANALYSIS_FPS: u64 = 2;
pub const ANALYSIS_WIDTH: u32 = 384;
pub const MAX_GENERATED_STEPS: usize = 200;
pub const REDUCTION_BUCKETS: usize = 198;
pub const EVIDENCE_MAX_LONG_EDGE: u32 = 1920;
pub const MAX_EVIDENCE_FRAMES: usize = 600;
pub const MAX_ANALYSIS_FRAME_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_SCRATCH_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoImportPass {
    Preflight,
    Analyze,
    Extract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoImportProgress {
    pub pass: VideoImportPass,
    pub processed_ms: u64,
    pub total_ms: u64,
    pub retained_candidates: usize,
}

#[derive(Clone, Default)]
pub struct VideoImportCancellation(Arc<AtomicBool>);

impl VideoImportCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
