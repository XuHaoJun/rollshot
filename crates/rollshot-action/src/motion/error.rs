//! Failure categories for the managed FFmpeg motion recording worker.

/// Closed set of failure modes surfaced by the motion recording pipeline.
///
/// Each variant names a stable, testable failure class. The `as_str()` method
/// returns a kebab-case identifier suitable for telemetry and logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MotionFailureCategory {
    /// The FFmpeg (or ffprobe) binary could not be located or is not executable.
    ToolUnavailable,
    /// `Command::spawn()` failed (e.g. permission denied, resource limit).
    Spawn,
    /// FFmpeg closed its stdin before the worker finished writing frames.
    BrokenPipe,
    /// A write to FFmpeg's stdin returned an I/O error (disk full, etc.).
    Write,
    /// Scratch directory creation or file rename/remove failed.
    Filesystem,
    /// FFmpeg exited with a non-zero status after all frames were delivered.
    Finalize,
    /// ffprobe returned unparseable, malformed, or semantically invalid JSON.
    Probe,
    /// SHA-256 digest computation or verification failed.
    Digest,
    /// The recording was explicitly cancelled before completion.
    Cancelled,
}

impl MotionFailureCategory {
    /// Stable kebab-case identifier for telemetry and structured logging.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ToolUnavailable => "tool-unavailable",
            Self::Spawn => "spawn",
            Self::BrokenPipe => "broken-pipe",
            Self::Write => "write",
            Self::Filesystem => "filesystem",
            Self::Finalize => "finalize",
            Self::Probe => "probe",
            Self::Digest => "digest",
            Self::Cancelled => "cancelled",
        }
    }
}

impl std::fmt::Display for MotionFailureCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
