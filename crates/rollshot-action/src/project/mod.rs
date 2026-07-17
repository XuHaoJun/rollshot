mod assets;
mod error;
mod model;
mod validate;

pub use error::{ProjectError, ProjectErrorCategory};
pub use model::{
    EnabledOutputs, LoadedProject, PersistedStepAnnotations, ProjectCommit, ProjectFrame,
    ProjectManifestV1, ProjectSnapshot, ProjectStep, ProjectStepId, SnapshotFrame,
    SnapshotFramePayload, PROJECT_SCHEMA_VERSION,
};
pub use validate::{validate_manifest_structure, validate_snapshot_structure};

#[allow(unused_imports)]
pub(crate) use assets::{
    asset_relative_path, decode_png_asset, encode_png_asset, inspect_png_asset, materialize_asset,
    EncodedAsset, InspectedAsset,
};
