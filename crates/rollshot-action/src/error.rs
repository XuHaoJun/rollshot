//! Typed errors for the Action Guide engine. Detection and export return
//! `Result` so the app can preserve the session and surface an actionable error
//! instead of writing a partial export.

/// Detection failure. Reserved so `ActionRecorder::finish`-style entry points
/// can return `Result` in the app integration without a breaking change; the
/// P0a in-process detector does not currently produce these, but the type fixes
/// the seam (spec §Failure Handling: "detection returns a `Result`").
#[derive(Debug, thiserror::Error)]
pub enum DetectError {
    #[error("detection failed: {message}")]
    Failed { message: String },
}

/// Export failure. On any error, the exporter leaves no partial `action-guide/`
/// directory and the editable session stays intact (spec §Export).
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("export I/O error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to encode PNG at {path}: {source}")]
    Encode {
        path: String,
        #[source]
        source: image::ImageError,
    },
    #[error("cannot export a guide with no steps")]
    Empty,
}

/// Summary-GIF export failure. On any error, no file is left at the target path
/// and the editable session stays intact.
#[derive(Debug, thiserror::Error)]
pub enum GifError {
    #[error("cannot export a GIF for a guide with no steps")]
    Empty,
    #[error("step {index} keyframe pixels were not retained")]
    KeyframeMissing { index: usize },
    #[error("failed to encode GIF: {source}")]
    Encode {
        #[source]
        source: image::ImageError,
    },
    #[error("GIF I/O error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Summary-MP4 export failure. On any error, no file is left at the target path
/// and the editable session stays intact.
#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    #[error("cannot export an MP4 for a guide with no steps")]
    Empty,
    #[error("step {index} keyframe pixels were not retained")]
    KeyframeMissing { index: usize },
    #[error("FFmpeg binary is not usable at {path}")]
    InvalidFfmpeg { path: String },
    #[error("failed to spawn FFmpeg at {path}: {source}")]
    Spawn {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write raw frames to FFmpeg stdin: {source}")]
    Stdin {
        #[source]
        source: std::io::Error,
    },
    #[error("FFmpeg exited unsuccessfully with status {status}: {stderr}")]
    Exit { status: String, stderr: String },
    #[error("MP4 I/O error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_error_messages_are_descriptive() {
        let io = ExportError::Io {
            path: "out/steps.md".into(),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        };
        assert!(io.to_string().contains("out/steps.md"), "{io}");
        assert_eq!(
            ExportError::Empty.to_string(),
            "cannot export a guide with no steps"
        );
    }

    #[test]
    fn detect_error_message_is_actionable() {
        let err = DetectError::Failed {
            message: "frame decode failed".to_string(),
        };
        assert_eq!(err.to_string(), "detection failed: frame decode failed");
    }

    #[test]
    fn gif_error_messages_are_descriptive() {
        assert_eq!(
            GifError::Empty.to_string(),
            "cannot export a GIF for a guide with no steps"
        );
        let missing = GifError::KeyframeMissing { index: 2 };
        assert!(missing.to_string().contains("step 2"), "{missing}");
    }

    #[test]
    fn video_error_messages_are_descriptive() {
        assert_eq!(
            VideoError::Empty.to_string(),
            "cannot export an MP4 for a guide with no steps"
        );
        let missing = VideoError::KeyframeMissing { index: 3 };
        assert!(missing.to_string().contains("step 3"), "{missing}");
        let invalid = VideoError::InvalidFfmpeg {
            path: "/missing/ffmpeg".to_string(),
        };
        assert!(invalid.to_string().contains("/missing/ffmpeg"), "{invalid}");
    }
}
