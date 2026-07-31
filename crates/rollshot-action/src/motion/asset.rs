//! Validated motion asset: an opaque, cloneable handle to a session-owned
//! H.264 recording. The scratch directory is deleted when the last clone
//! drops (RAII cleanup).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::probe::MotionMetadata;

/// Inner shared state for a validated motion asset.
///
/// Holds the metadata and the path to the scratch directory that owns
/// the `.mp4` file. When the last `Arc` reference drops, the scratch
/// directory is removed from disk.
struct MotionAssetInner {
    metadata: MotionMetadata,
    /// Path to the final `recording.mp4` inside the scratch directory.
    source_path: PathBuf,
    /// The session scratch directory. Removed on drop.
    scratch_dir: PathBuf,
}

impl Drop for MotionAssetInner {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scratch_dir);
    }
}

/// An opaque, cloneable handle to a validated H.264 motion recording.
///
/// Session-owned: the underlying scratch directory is automatically deleted
/// when the last clone is dropped.
#[derive(Clone)]
pub struct ValidatedMotionAsset {
    inner: Arc<MotionAssetInner>,
}

impl ValidatedMotionAsset {
    /// Create a new validated asset. Crate-visible: only the worker produces these.
    pub(crate) fn new(
        metadata: MotionMetadata,
        source_path: PathBuf,
        scratch_dir: PathBuf,
    ) -> Self {
        Self {
            inner: Arc::new(MotionAssetInner {
                metadata,
                source_path,
                scratch_dir,
            }),
        }
    }

    /// The probe metadata for this recording.
    pub fn metadata(&self) -> &MotionMetadata {
        &self.inner.metadata
    }

    /// SHA-256 hex digest of the final `.mp4`.
    pub fn sha256(&self) -> &str {
        &self.inner.metadata.sha256
    }

    /// Duration in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        self.inner.metadata.duration_ms
    }

    /// Display width in pixels.
    pub fn width(&self) -> u32 {
        self.inner.metadata.width
    }

    /// Display height in pixels.
    pub fn height(&self) -> u32 {
        self.inner.metadata.height
    }

    /// FPS numerator.
    pub fn fps_numerator(&self) -> u32 {
        self.inner.metadata.fps_numerator
    }

    /// FPS denominator.
    pub fn fps_denominator(&self) -> u32 {
        self.inner.metadata.fps_denominator
    }

    /// The video codec.
    pub fn codec(&self) -> super::probe::MotionCodec {
        self.inner.metadata.codec
    }

    /// The audio codec (always `None` for motion recordings).
    pub fn audio(&self) -> super::probe::MotionAudio {
        self.inner.metadata.audio
    }

    /// Path to the source `.mp4` file inside the session scratch directory.
    /// Crate-visible: only the export pipeline reads the file.
    #[allow(dead_code)] // used by future export pipeline
    pub(crate) fn source_path(&self) -> &Path {
        &self.inner.source_path
    }
}

impl std::fmt::Debug for ValidatedMotionAsset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValidatedMotionAsset")
            .field("metadata", self.metadata())
            .field("source_path", &self.inner.source_path)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::probe::{MotionAudio, MotionCodec};

    fn dummy_metadata() -> MotionMetadata {
        MotionMetadata {
            sha256: "abcdef1234567890".into(),
            duration_ms: 1000,
            width: 640,
            height: 480,
            fps_numerator: 30,
            fps_denominator: 1,
            codec: MotionCodec::H264,
            audio: MotionAudio::None,
        }
    }

    #[test]
    fn asset_getters_return_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let mp4 = dir.path().join("recording.mp4");
        std::fs::write(&mp4, b"fake").unwrap();

        let asset = ValidatedMotionAsset::new(dummy_metadata(), mp4.clone(), dir.into_path());
        assert_eq!(asset.sha256(), "abcdef1234567890");
        assert_eq!(asset.duration_ms(), 1000);
        assert_eq!(asset.width(), 640);
        assert_eq!(asset.height(), 480);
        assert_eq!(asset.fps_numerator(), 30);
        assert_eq!(asset.fps_denominator(), 1);
        assert_eq!(asset.codec(), MotionCodec::H264);
        assert_eq!(asset.audio(), MotionAudio::None);
        assert_eq!(asset.source_path(), mp4);
    }

    #[test]
    fn last_clone_drop_removes_scratch() {
        let dir = tempfile::tempdir().unwrap();
        let scratch = dir.path().join("action-motion-test");
        std::fs::create_dir_all(&scratch).unwrap();
        let mp4 = scratch.join("recording.mp4");
        std::fs::write(&mp4, b"fake mp4").unwrap();

        let asset = ValidatedMotionAsset::new(dummy_metadata(), mp4, scratch.clone());
        let clone = asset.clone();

        // Drop one reference — scratch still exists.
        drop(asset);
        assert!(scratch.exists());

        // Drop last reference — scratch is removed.
        drop(clone);
        assert!(!scratch.exists());
    }
}
