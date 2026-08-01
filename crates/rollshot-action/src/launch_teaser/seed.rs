//! Deterministic seed generation and binding validation.

use crate::project::LoadedProject;

use super::error::{LaunchTeaserBindingError, LaunchTeaserSeedError};
use super::plan::LaunchTeaserPlanV1;

/// Deterministic seed algorithm version.
pub const DETERMINISTIC_SEED_VERSION: u32 = 1;

/// Generate a deterministic launch teaser plan from a loaded project.
pub fn seed_launch_teaser(_loaded: &LoadedProject) -> Result<LaunchTeaserPlanV1, LaunchTeaserSeedError> {
    todo!("Task 2")
}

/// Validate that a plan still binds to the current project state.
pub fn validate_launch_teaser_binding(
    _plan: &LaunchTeaserPlanV1,
    _loaded: &LoadedProject,
) -> Result<(), LaunchTeaserBindingError> {
    todo!("Task 2")
}
