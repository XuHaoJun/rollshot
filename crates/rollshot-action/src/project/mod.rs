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
