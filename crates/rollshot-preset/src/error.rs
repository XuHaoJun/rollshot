use std::path::PathBuf;

/// What kind of entity a [`StoreError::NotFound`] refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Preset,
    Revision,
}

/// Errors returned by the preset store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("{kind:?} not found: {id}")]
    NotFound { kind: EntityKind, id: String },
    #[error("incompatible automation artifact: {0}")]
    Incompatible(#[from] rollshot_automation::CompatibilityError),
    #[error("integrity violation: {0}")]
    Integrity(String),
    #[error("revision already exists: {0}")]
    RevisionExists(String),
    #[error("unsupported store schema version at {path:?}: expected {expected}, found {found}")]
    UnsupportedStoreSchema {
        path: PathBuf,
        expected: u16,
        found: u16,
    },
    #[error("corrupt store entry at {path:?}: {detail}")]
    Corrupt { path: PathBuf, detail: String },
}

/// Convenience alias for store operations.
pub type Result<T> = std::result::Result<T, StoreError>;
