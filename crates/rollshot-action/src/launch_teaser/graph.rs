//! Fixed FFmpeg render graph compilation.

use std::ffi::OsString;
use std::path::Path;

use super::error::LaunchTeaserRenderError;
use super::overlay::OverlayAsset;
use super::plan::ValidatedLaunchTeaserPlan;

/// Render output profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderProfile {
    /// Full-resolution final output.
    Final,
    /// Half-resolution preview.
    Preview,
}

/// A compiled FFmpeg graph ready for process spawn.
#[derive(Debug, Clone)]
pub struct CompiledLaunchTeaserGraph {
    args: Vec<OsString>,
}

impl CompiledLaunchTeaserGraph {
    /// The full argument list for FFmpeg invocation.
    pub fn args(&self) -> &[OsString] {
        &self.args
    }
}

/// Compile the fixed FFmpeg render graph.
pub fn compile_ffmpeg_graph(
    _plan: &ValidatedLaunchTeaserPlan,
    _motion_path: &Path,
    _overlays: &[OverlayAsset],
    _output_path: &Path,
    _profile: RenderProfile,
) -> Result<CompiledLaunchTeaserGraph, LaunchTeaserRenderError> {
    todo!("Task 4")
}
