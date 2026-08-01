//! Derived sidecar persistence for launch teaser plans.

use std::path::Path;

use super::error::{LaunchTeaserArtifactV1, LaunchTeaserPersistenceError, LaunchTeaserSidecarLoad};

/// Canonical sidecar relative path.
pub const SIDECAR_RELATIVE_PATH: &str = "publish/launch-teaser-plan-v1.json";

/// Write the launch teaser artifact as an atomic sidecar.
pub fn write_launch_teaser_sidecar(
    _project_root: &Path,
    _artifact: &LaunchTeaserArtifactV1,
) -> Result<(), LaunchTeaserPersistenceError> {
    todo!("Task 3")
}

/// Load the launch teaser sidecar from a project root.
pub fn load_launch_teaser_sidecar(_project_root: &Path) -> LaunchTeaserSidecarLoad {
    todo!("Task 3")
}
