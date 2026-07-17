use std::path::PathBuf;

/// Privacy-safe error category for tracing. Never includes file paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectErrorCategory {
    Io,
    InvalidJson,
    UnsupportedSchema,
    InvalidManifest,
    InvalidAsset,
    Encode,
    DestinationExists,
    UnsupportedAtomicCommit,
    RevisionConflict,
}

impl std::fmt::Display for ProjectErrorCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io => write!(f, "io"),
            Self::InvalidJson => write!(f, "invalid-json"),
            Self::UnsupportedSchema => write!(f, "unsupported-schema"),
            Self::InvalidManifest => write!(f, "invalid-manifest"),
            Self::InvalidAsset => write!(f, "invalid-asset"),
            Self::Encode => write!(f, "encode"),
            Self::DestinationExists => write!(f, "destination-exists"),
            Self::UnsupportedAtomicCommit => write!(f, "unsupported-atomic-commit"),
            Self::RevisionConflict => write!(f, "revision-conflict"),
        }
    }
}

/// Project persistence error. Paths may appear in user-facing `Display` but
/// tracing call sites must log only `category()` and structural IDs.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("invalid JSON at {path}: {source}")]
    InvalidJson {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[error("unsupported schema version {version} at {path}")]
    UnsupportedSchema { path: PathBuf, version: u32 },

    #[error("invalid manifest ({category}) step={step_id:?} frame={frame_id:?}")]
    InvalidManifest {
        category: ProjectErrorCategory,
        step_id: Option<u64>,
        frame_id: Option<u64>,
    },

    #[error("invalid asset ({category}) frame={frame_id}")]
    InvalidAsset {
        category: ProjectErrorCategory,
        frame_id: u64,
    },

    #[error("encode error: {message}")]
    Encode { message: String },

    #[error("destination already exists: {path}")]
    DestinationExists { path: PathBuf },

    #[error("atomic commit not supported on this platform at {path}")]
    UnsupportedAtomicCommit { path: PathBuf },

    #[error("revision conflict: expected {expected}, got {actual}")]
    RevisionConflict { expected: u64, actual: u64 },
}

impl ProjectError {
    /// Privacy-safe category for structured logging. Never includes paths.
    pub fn category(&self) -> ProjectErrorCategory {
        match self {
            Self::Io { .. } => ProjectErrorCategory::Io,
            Self::InvalidJson { .. } => ProjectErrorCategory::InvalidJson,
            Self::UnsupportedSchema { .. } => ProjectErrorCategory::UnsupportedSchema,
            Self::InvalidManifest { .. } => ProjectErrorCategory::InvalidManifest,
            Self::InvalidAsset { .. } => ProjectErrorCategory::InvalidAsset,
            Self::Encode { .. } => ProjectErrorCategory::Encode,
            Self::DestinationExists { .. } => ProjectErrorCategory::DestinationExists,
            Self::UnsupportedAtomicCommit { .. } => ProjectErrorCategory::UnsupportedAtomicCommit,
            Self::RevisionConflict { .. } => ProjectErrorCategory::RevisionConflict,
        }
    }
}
