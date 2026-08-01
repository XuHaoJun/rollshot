//! Launch teaser domain: plan contract, deterministic seed, derived sidecar,
//! text overlays, and fixed render graph.

pub mod error;
pub mod plan;
pub mod seed;
pub mod persistence;
pub mod overlay;
pub mod graph;

pub use error::{
    LaunchTeaserArtifactV1, LaunchTeaserBindingError, LaunchTeaserError, LaunchTeaserPersistenceError,
    LaunchTeaserRenderError, LaunchTeaserSeedError, LaunchTeaserSidecarLoad,
};
pub use plan::{
    AcceptedEditSourceV1, AcceptedEditV1, AgentProvenanceV1, FocusPathV1,
    LaunchTeaserPlanV1, LaunchTeaserProvenanceV1, LaunchTeaserShotV1,
    LaunchTeaserSourceV1, NormalizedPointV1, RepositoryReadProvenanceV1,
    SpeedV1, TransitionV1, ValidatedLaunchTeaserPlan,
    FINAL_FPS, FINAL_HEIGHT, FINAL_WIDTH, LAUNCH_TEASER_SCHEMA_VERSION,
    MAX_DURATION_MS, MAX_SHOTS, MIN_DURATION_MS, MIN_SHOTS,
    OUTRO_DURATION_MS, PLAN_DOMAIN_SEPARATOR, PREVIEW_HEIGHT, PREVIEW_WIDTH,
};
pub use seed::{seed_launch_teaser, validate_launch_teaser_binding, DETERMINISTIC_SEED_VERSION};
pub use persistence::{write_launch_teaser_sidecar, load_launch_teaser_sidecar};
pub use graph::{compile_ffmpeg_graph, CompiledLaunchTeaserGraph, RenderProfile};
pub use overlay::{prepare_overlay_assets, OverlayAsset};
