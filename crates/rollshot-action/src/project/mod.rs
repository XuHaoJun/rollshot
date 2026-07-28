mod assets;
pub mod continuity;
mod error;
mod model;
pub mod publish;
mod store;
mod validate;

pub use continuity::{
    ActionGuideContextProjectionV1, ActionGuideProjectedStepV1, ActionGuideProjectionError,
    MAX_PROJECTED_BYTES, MAX_PROJECTED_STEPS, MAX_PROJECTED_TEXT_BYTES,
};
pub use error::{ProjectError, ProjectErrorCategory};
pub use model::{
    EnabledOutputs, LoadedProject, PersistedStepAnnotations, ProjectCommit, ProjectFrame,
    ProjectManifestV1, ProjectManifestV2, ProjectSnapshot, ProjectStep, ProjectStepId,
    SnapshotFrame, SnapshotFramePayload, PROJECT_SCHEMA_VERSION,
};
pub use publish::{
    load_publish_state, write_publish_state, PublishCancellation, PublishCancelled,
    PublishFreshness, PublishOutputKind, PublishStateLoad, PublishStateV1, PublishedOutputV1,
};
pub use store::{create_project, load_project, save_project, save_project_as};
pub use validate::{validate_manifest_structure, validate_snapshot_structure};

#[allow(unused_imports)]
pub(crate) use assets::{
    asset_relative_path, decode_png_asset, encode_png_asset, inspect_png_asset, materialize_asset,
    EncodedAsset, InspectedAsset,
};
