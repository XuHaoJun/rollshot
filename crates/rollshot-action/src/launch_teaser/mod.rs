//! Launch teaser domain: plan contract, deterministic seed, derived sidecar,
//! text overlays, and fixed render graph.

pub mod error;
pub mod graph;
pub mod overlay;
pub mod persistence;
pub mod plan;
pub mod probe;
pub mod render;
pub mod seed;

pub use error::{
    LaunchTeaserArtifactV1, LaunchTeaserBindingError, LaunchTeaserError,
    LaunchTeaserPersistenceError, LaunchTeaserRenderError, LaunchTeaserSeedError,
    LaunchTeaserSidecarLoad,
};
pub use graph::RenderProfile;
pub use persistence::{load_launch_teaser_sidecar, write_launch_teaser_sidecar};
pub use plan::{
    AcceptedEditSourceV1, AcceptedEditV1, AgentProvenanceV1, FocusPathV1, LaunchTeaserPlanV1,
    LaunchTeaserProvenanceV1, LaunchTeaserShotV1, LaunchTeaserSourceV1, NormalizedPointV1,
    RepositoryReadProvenanceV1, SpeedV1, TransitionV1, ValidatedLaunchTeaserPlan, FINAL_FPS,
    FINAL_HEIGHT, FINAL_WIDTH, LAUNCH_TEASER_SCHEMA_VERSION, MAX_DURATION_MS, MAX_SHOTS,
    MIN_DURATION_MS, MIN_SHOTS, OUTRO_DURATION_MS, PLAN_DOMAIN_SEPARATOR, PREVIEW_HEIGHT,
    PREVIEW_WIDTH,
};
pub use probe::{verify_launch_teaser_output, VerifiedLaunchTeaserOutput};
pub use render::{
    render_launch_teaser, LaunchTeaserPreview, LaunchTeaserPreviewResult,
    LaunchTeaserRenderRequest, LaunchTeaserRenderResult,
};
pub use seed::{seed_launch_teaser, validate_launch_teaser_binding, DETERMINISTIC_SEED_VERSION};
