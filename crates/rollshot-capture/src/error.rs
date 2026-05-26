#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("backend not implemented yet: {backend}")]
    NotImplemented { backend: &'static str },

    #[error("backend unsupported on this host: {message}")]
    Unsupported { message: String },

    #[error("user cancelled capture")]
    UserCancelled,

    #[error("permission denied: {message}")]
    PermissionDenied { message: String },

    #[error("end of frame stream")]
    EndOfStream,

    #[error("frame timeout: {message}")]
    Timeout { message: String },

    #[error("invalid configuration: {message}")]
    InvalidConfig { message: String },

    #[error(transparent)]
    Backend(#[from] anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::CaptureError;

    #[test]
    fn not_implemented_includes_backend_name() {
        let err = CaptureError::NotImplemented {
            backend: "linux-portal",
        };
        let text = err.to_string();
        assert!(text.contains("linux-portal"), "text = {text}");
        assert!(text.contains("not implemented"), "text = {text}");
    }

    #[test]
    fn permission_denied_includes_message() {
        let err = CaptureError::PermissionDenied {
            message: "Screen Recording".to_string(),
        };
        assert!(err.to_string().contains("Screen Recording"));
    }

    #[test]
    fn invalid_config_includes_message() {
        let err = CaptureError::InvalidConfig {
            message: "bad region".to_string(),
        };
        assert!(err.to_string().contains("bad region"));
    }

    #[test]
    fn end_of_stream_renders() {
        let err = CaptureError::EndOfStream;
        assert!(err.to_string().contains("end of frame stream"));
    }
}
