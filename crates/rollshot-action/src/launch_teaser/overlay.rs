//! Text overlay rasterization for launch teaser rendering.

use std::path::Path;

use super::error::LaunchTeaserRenderError;
use super::graph::RenderProfile;
use super::plan::ValidatedLaunchTeaserPlan;

/// A rasterized overlay asset ready for FFmpeg consumption.
#[derive(Debug, Clone)]
pub struct OverlayAsset {
    /// Index of the shot this overlay belongs to.
    pub shot_index: usize,
    /// Absolute path to the generated PNG file.
    pub path: std::path::PathBuf,
    /// Start time in milliseconds when this overlay appears.
    pub start_ms: u64,
    /// End time in milliseconds when this overlay disappears.
    pub end_ms: u64,
}

/// Rasterize text overlays for all shots in the plan.
pub fn prepare_overlay_assets(
    _plan: &ValidatedLaunchTeaserPlan,
    _scratch: &Path,
    _profile: RenderProfile,
) -> Result<Vec<OverlayAsset>, LaunchTeaserRenderError> {
    todo!("Task 4")
}
