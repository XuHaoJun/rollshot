mod assets;
mod error;
mod model;
mod publish;
mod store;
mod validate;

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
