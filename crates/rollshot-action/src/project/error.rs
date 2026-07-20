use std::path::PathBuf;

/// Privacy-safe error category for tracing. Never includes file paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectErrorCategory {
    Io,
    InvalidJson,
    UnsupportedSchema,
    UnsupportedVersion,
    InvalidManifest,
    InvalidAsset,
    Encode,
    DestinationExists,
    UnsupportedAtomicCommit,
    RevisionConflict,
    ZeroRevision,
    EmptySteps,
    EmptyFrames,
    DuplicateFrameId,
    DuplicateStepId,
    NonContiguousOrder,
    MissingKeyframe,
    KeyframeNotNearby,
    DuplicateNearbyId,
    MissingNearbyFrame,
    FrameDimensionMismatch,
    ZeroCaptureRegion,
    MissingExplanationAnnotation,
    AnnotationValidationFailed,
    DuplicateImportWarning,
}

impl ProjectErrorCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Io => "io",
            Self::InvalidJson => "invalid-json",
            Self::UnsupportedSchema => "unsupported-schema",
            Self::UnsupportedVersion => "unsupported_version",
            Self::InvalidManifest => "invalid-manifest",
            Self::InvalidAsset => "invalid-asset",
            Self::Encode => "encode",
            Self::DestinationExists => "destination-exists",
            Self::UnsupportedAtomicCommit => "unsupported-atomic-commit",
            Self::RevisionConflict => "revision-conflict",
            Self::ZeroRevision => "zero-revision",
            Self::EmptySteps => "empty-steps",
            Self::EmptyFrames => "empty-frames",
            Self::DuplicateFrameId => "duplicate-frame-id",
            Self::DuplicateStepId => "duplicate-step-id",
            Self::NonContiguousOrder => "non-contiguous-order",
            Self::MissingKeyframe => "missing-keyframe",
            Self::KeyframeNotNearby => "keyframe-not-nearby",
            Self::DuplicateNearbyId => "duplicate-nearby-id",
            Self::MissingNearbyFrame => "missing-nearby-frame",
            Self::FrameDimensionMismatch => "frame-dimension-mismatch",
            Self::ZeroCaptureRegion => "zero-capture-region",
            Self::MissingExplanationAnnotation => "missing-explanation-annotation",
            Self::AnnotationValidationFailed => "annotation-validation-failed",
            Self::DuplicateImportWarning => "duplicate-import-warning",
        }
    }
}

impl std::fmt::Display for ProjectErrorCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
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

    #[error("unsupported schema version {version}")]
    UnsupportedSchema { path: Option<PathBuf>, version: u32 },

    #[error("unsupported version {version}")]
    UnsupportedVersion { version: u32 },

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
    pub fn category(&self) -> &str {
        match self {
            Self::Io { .. } => "io",
            Self::InvalidJson { .. } => "invalid-json",
            Self::UnsupportedSchema { .. } => "unsupported-schema",
            Self::UnsupportedVersion { .. } => "unsupported_version",
            Self::InvalidManifest { category, .. } => category.as_str(),
            Self::InvalidAsset { .. } => "invalid-asset",
            Self::Encode { .. } => "encode",
            Self::DestinationExists { .. } => "destination-exists",
            Self::UnsupportedAtomicCommit { .. } => "unsupported-atomic-commit",
            Self::RevisionConflict { .. } => "revision-conflict",
        }
    }

    pub(crate) fn invalid_manifest(
        category: ProjectErrorCategory,
        step_id: Option<u64>,
        frame_id: Option<u64>,
    ) -> Self {
        Self::InvalidManifest {
            category,
            step_id,
            frame_id,
        }
    }
}
