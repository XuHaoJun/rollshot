use std::fmt;

use rollshot_capture::CaptureError;

/// Where a `CliError`'s message should be written by `main.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug)]
pub struct CliError {
    pub message: String,
    pub exit_code: u8,
    /// Which stream to write `message` to. `Stdout` is used only for clap's
    /// `--help` / `--version` happy-path output. Every other case (including
    /// `UserCancelled` with `exit_code = 0`) prints to stderr so shell
    /// pipelines do not catch diagnostic text as data.
    pub stream: Stream,
}

impl CliError {
    pub fn new(message: impl Into<String>, exit_code: u8) -> Self {
        Self {
            message: message.into(),
            exit_code,
            stream: Stream::Stderr,
        }
    }

    /// Construct a CliError destined for stdout. Reserved for clap's
    /// `--help` / `--version` exit path.
    pub fn stdout(message: impl Into<String>, exit_code: u8) -> Self {
        Self {
            message: message.into(),
            exit_code,
            stream: Stream::Stdout,
        }
    }

    pub fn from_capture(err: CaptureError) -> Self {
        match err {
            CaptureError::NotImplemented { backend } => CliError::new(
                format!(
                    "backend not implemented yet: {backend}\nhint: use --backend fixture for offline runs"
                ),
                2,
            ),
            CaptureError::PermissionDenied { message } => {
                CliError::new(format!("permission denied: {message}"), 3)
            }
            CaptureError::Unsupported { message } => {
                CliError::new(format!("unsupported: {message}"), 4)
            }
            CaptureError::UserCancelled => CliError::new("user cancelled", 0),
            CaptureError::EndOfStream => {
                CliError::new("frame stream ended before any frame was captured", 1)
            }
            CaptureError::InvalidConfig { message } => {
                CliError::new(format!("invalid configuration: {message}"), 1)
            }
            CaptureError::Timeout { message } => {
                CliError::new(format!("frame timeout: {message}"), 1)
            }
            CaptureError::Backend(err) => CliError::new(format!("{err:#}"), 1),
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<CaptureError> for CliError {
    fn from(err: CaptureError) -> Self {
        CliError::from_capture(err)
    }
}
