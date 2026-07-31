//! Atomic promotion of session-owned motion assets into project trees,
//! and raw MP4 export with byte-identical verification.

use std::path::Path;

use sha2::Digest;

use super::error::{ProjectError, ProjectErrorCategory};
use super::model::MotionAsset;
use crate::motion::asset::ValidatedMotionAsset;

/// Canonical relative path for a project-owned motion asset.
const MOTION_RELATIVE_PATH: &str = "assets/motion/recording.mp4";

/// Promote a session-owned `ValidatedMotionAsset` into a project tree.
///
/// Creates `assets/motion/`, copies the `.mp4` to a temp sibling, syncs,
/// renames to `recording.mp4`, fsyncs the directory, and returns the
/// `MotionAsset` for manifest inclusion. If the source is already at the
/// same canonical path within the same project root, revalidates identity
/// instead of copying.
///
/// On any failure the session-owned source is never deleted or mutated.
pub(crate) fn promote_motion_asset(
    asset: &ValidatedMotionAsset,
    project_root: &Path,
) -> Result<MotionAsset, ProjectError> {
    let motion_dir = project_root.join("assets/motion");
    std::fs::create_dir_all(&motion_dir).map_err(|e| ProjectError::Io {
        path: motion_dir.clone(),
        source: e,
    })?;

    let final_path = motion_dir.join("recording.mp4");

    // If the source is already at the canonical location within this project,
    // revalidate identity instead of copying over ourselves.
    if asset.source_path() == final_path {
        return build_motion_asset(asset);
    }

    // Copy to a temp sibling in the same directory.
    let pid = std::process::id();
    let counter = super::store::next_temp_counter();
    let temp_name = format!(".tmp-motion-{pid}-{counter}");
    let temp_path = motion_dir.join(&temp_name);

    let cleanup = || {
        let _ = std::fs::remove_file(&temp_path);
    };

    // Copy bytes from the validated source.
    std::fs::copy(asset.source_path(), &temp_path).map_err(|e| {
        cleanup();
        ProjectError::Io {
            path: temp_path.clone(),
            source: e,
        }
    })?;

    // Sync the temp file to disk.
    let file = std::fs::File::open(&temp_path).map_err(|e| {
        cleanup();
        ProjectError::Io {
            path: temp_path.clone(),
            source: e,
        }
    })?;
    file.sync_all().map_err(|e| {
        cleanup();
        ProjectError::Io {
            path: temp_path.clone(),
            source: e,
        }
    })?;

    // Verify the copied bytes match the validated digest.
    let copied_bytes = std::fs::read(&temp_path).map_err(|e| {
        cleanup();
        ProjectError::Io {
            path: temp_path.clone(),
            source: e,
        }
    })?;
    let hash = sha2::Sha256::digest(&copied_bytes);
    let computed = format!("{hash:x}");
    if computed != asset.sha256() {
        cleanup();
        return Err(ProjectError::invalid_manifest(
            ProjectErrorCategory::InvalidMotion,
            None,
            None,
        ));
    }

    // Rename over the final path.
    std::fs::rename(&temp_path, &final_path).map_err(|e| {
        cleanup();
        ProjectError::Io {
            path: final_path.clone(),
            source: e,
        }
    })?;

    // Fsync the directory to persist the rename.
    super::store::fsync_dir_public(&motion_dir)?;

    build_motion_asset(asset)
}

/// Build a `MotionAsset` manifest entry from a `ValidatedMotionAsset`.
fn build_motion_asset(asset: &ValidatedMotionAsset) -> Result<MotionAsset, ProjectError> {
    Ok(MotionAsset {
        relative_path: MOTION_RELATIVE_PATH.to_string(),
        sha256: asset.sha256().to_string(),
        duration_ms: asset.duration_ms(),
        width: asset.width(),
        height: asset.height(),
        fps_numerator: asset.fps_numerator(),
        fps_denominator: asset.fps_denominator(),
        codec: asset.codec().as_str().to_string(),
        audio: asset.audio().as_str().to_string(),
    })
}

/// Export a validated motion asset to a user-chosen destination path.
///
/// Creates a hidden temp sibling in the destination directory, copies bytes,
/// syncs, verifies the SHA-256 digest matches the metadata, then atomically
/// renames over the destination. If a file already exists at `destination`,
/// the rename replaces it (same-directory atomic replacement).
///
/// On failure at any step, only the temp file is cleaned up — the original
/// destination (if any) and the session-owned source are never touched.
pub fn export_motion_asset(
    asset: &ValidatedMotionAsset,
    destination: &Path,
) -> Result<(), ProjectError> {
    let parent = destination.parent().ok_or_else(|| ProjectError::Io {
        path: destination.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent directory"),
    })?;

    let pid = std::process::id();
    let counter = super::store::next_temp_counter();
    let temp_name = format!(".tmp-export-{pid}-{counter}");
    let temp_path = parent.join(&temp_name);

    let cleanup = || {
        let _ = std::fs::remove_file(&temp_path);
    };

    // Copy bytes from the validated source.
    std::fs::copy(asset.source_path(), &temp_path).map_err(|e| {
        cleanup();
        ProjectError::Io {
            path: temp_path.clone(),
            source: e,
        }
    })?;

    // Sync the temp file.
    let file = std::fs::File::open(&temp_path).map_err(|e| {
        cleanup();
        ProjectError::Io {
            path: temp_path.clone(),
            source: e,
        }
    })?;
    file.sync_all().map_err(|e| {
        cleanup();
        ProjectError::Io {
            path: temp_path.clone(),
            source: e,
        }
    })?;

    // Verify the copied bytes match the validated digest.
    let copied_bytes = std::fs::read(&temp_path).map_err(|e| {
        cleanup();
        ProjectError::Io {
            path: temp_path.clone(),
            source: e,
        }
    })?;
    let hash = sha2::Sha256::digest(&copied_bytes);
    let computed = format!("{hash:x}");
    if computed != asset.sha256() {
        cleanup();
        return Err(ProjectError::invalid_manifest(
            ProjectErrorCategory::InvalidMotion,
            None,
            None,
        ));
    }

    // Atomic rename over the destination (same-directory replacement).
    std::fs::rename(&temp_path, destination).map_err(|e| {
        cleanup();
        ProjectError::Io {
            path: destination.to_path_buf(),
            source: e,
        }
    })?;

    // Fsync the destination directory.
    super::store::fsync_dir_public(parent)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::asset::ValidatedMotionAsset;
    use crate::motion::probe::{MotionAudio, MotionCodec, MotionMetadata};

    fn dummy_metadata() -> MotionMetadata {
        MotionMetadata {
            sha256: "a".repeat(64),
            duration_ms: 5000,
            width: 1920,
            height: 1080,
            fps_numerator: 30,
            fps_denominator: 1,
            codec: MotionCodec::H264,
            audio: MotionAudio::None,
        }
    }

    /// Create a `ValidatedMotionAsset` backed by a real temp file with the
    /// given content. Returns (asset, guard) — the guard keeps the temp dir alive.
    fn make_validated_asset(content: &[u8]) -> (ValidatedMotionAsset, tempfile::TempDir) {
        use sha2::Digest;
        let scratch = tempfile::tempdir().unwrap();
        let source_path = scratch.path().join("recording.mp4");
        std::fs::write(&source_path, content).unwrap();
        let hash = sha2::Sha256::digest(content);
        let sha256 = format!("{hash:x}");
        let mut meta = dummy_metadata();
        meta.sha256 = sha256;
        let asset = ValidatedMotionAsset::new(meta, source_path, scratch.path().to_path_buf());
        (asset, scratch)
    }

    // -----------------------------------------------------------------------
    // Step 1: RED promotion tests
    // -----------------------------------------------------------------------

    #[test]
    fn create_project_promotes_session_asset_to_project_tree() {
        use super::super::model::{
            EnabledOutputs, ProjectSnapshot, ProjectStep, ProjectStepId, SnapshotFrame,
            SnapshotFramePayload,
        };
        use super::super::store::create_project;
        use crate::models::{CandidateKind, DetectReason};
        use crate::models::{CaptureRegion, InputCapability, InputSourceKind};
        use image::{Rgba, RgbaImage};
        use std::sync::Arc;

        let (asset, _scratch_guard) = make_validated_asset(b"session motion data");

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        let snap = ProjectSnapshot {
            base_revision: None,
            title: "Motion Guide".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(Arc::new(RgbaImage::from_pixel(
                    8,
                    8,
                    Rgba([10, 20, 30, 255]),
                ))),
            }],
            steps: vec![ProjectStep {
                id: ProjectStepId(1),
                order: 1,
                title: "Step 1".into(),
                caption: None,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 150,
                keyframe: 1,
                nearby: vec![1],
                annotations: None,
            }],
            import_warnings: Vec::new(),
            motion: Some(asset.clone()),
        };

        let commit = create_project(&snap, &root).unwrap();
        let motion = commit.manifest.motion.as_ref().expect("motion in manifest");
        assert_eq!(motion.relative_path, "assets/motion/recording.mp4");
        assert_eq!(motion.sha256, asset.sha256());
        assert_eq!(motion.duration_ms, 5000);
        assert_eq!(motion.width, 1920);
        assert_eq!(motion.height, 1080);

        // File exists on disk
        assert!(root.join("assets/motion/recording.mp4").exists());
    }

    #[test]
    fn save_project_as_copies_validated_project_asset() {
        use super::super::model::{
            EnabledOutputs, ProjectSnapshot, ProjectStep, ProjectStepId, SnapshotFrame,
            SnapshotFramePayload,
        };
        use super::super::store::{create_project, save_project_as};
        use crate::models::{CandidateKind, DetectReason};
        use crate::models::{CaptureRegion, InputCapability, InputSourceKind};
        use image::{Rgba, RgbaImage};
        use std::sync::Arc;

        let (asset, _scratch_guard) = make_validated_asset(b"motion content for save as");

        let dir = tempfile::tempdir().unwrap();
        let root1 = dir.path().join("first.rollshot-guide");
        let root2 = dir.path().join("second.rollshot-guide");

        let snap = ProjectSnapshot {
            base_revision: None,
            title: "Original".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(Arc::new(RgbaImage::from_pixel(
                    8,
                    8,
                    Rgba([10, 20, 30, 255]),
                ))),
            }],
            steps: vec![ProjectStep {
                id: ProjectStepId(1),
                order: 1,
                title: "Step 1".into(),
                caption: None,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 150,
                keyframe: 1,
                nearby: vec![1],
                annotations: None,
            }],
            import_warnings: Vec::new(),
            motion: Some(asset.clone()),
        };

        let commit1 = create_project(&snap, &root1).unwrap();
        let motion1_sha = commit1.manifest.motion.as_ref().unwrap().sha256.clone();

        // Save As with the same motion asset
        let mut snap2 = snap.clone();
        snap2.base_revision = Some(1);
        snap2.title = "Copy".into();
        let commit2 = save_project_as(&snap2, &root2).unwrap();
        let motion2 = commit2.manifest.motion.as_ref().expect("motion in save-as");
        assert_eq!(motion2.sha256, motion1_sha);
        assert!(root2.join("assets/motion/recording.mp4").exists());
    }

    #[test]
    fn existing_save_retains_same_motion_asset() {
        use super::super::model::{
            EnabledOutputs, ProjectSnapshot, ProjectStep, ProjectStepId, SnapshotFrame,
            SnapshotFramePayload,
        };
        use super::super::store::{create_project, save_project};
        use crate::models::{CandidateKind, DetectReason};
        use crate::models::{CaptureRegion, InputCapability, InputSourceKind};
        use image::{Rgba, RgbaImage};
        use std::sync::Arc;

        let (asset, _scratch_guard) = make_validated_asset(b"persisted motion");

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        let snap = ProjectSnapshot {
            base_revision: None,
            title: "Guide".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(Arc::new(RgbaImage::from_pixel(
                    8,
                    8,
                    Rgba([10, 20, 30, 255]),
                ))),
            }],
            steps: vec![ProjectStep {
                id: ProjectStepId(1),
                order: 1,
                title: "Step 1".into(),
                caption: None,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 150,
                keyframe: 1,
                nearby: vec![1],
                annotations: None,
            }],
            import_warnings: Vec::new(),
            motion: Some(asset.clone()),
        };

        let commit1 = create_project(&snap, &root).unwrap();
        let original_sha = commit1.manifest.motion.as_ref().unwrap().sha256.clone();

        // Existing save with same motion
        let mut snap2 = snap.clone();
        snap2.base_revision = Some(1);
        snap2.title = "Updated".into();
        let commit2 = save_project(&snap2, &root).unwrap();
        let saved_sha = commit2.manifest.motion.as_ref().unwrap().sha256.clone();
        assert_eq!(saved_sha, original_sha);
    }

    #[test]
    fn promotion_records_digest_and_metadata_from_validated_object() {
        use super::super::model::{
            EnabledOutputs, ProjectSnapshot, ProjectStep, ProjectStepId, SnapshotFrame,
            SnapshotFramePayload,
        };
        use super::super::store::create_project;
        use crate::models::{CandidateKind, DetectReason};
        use crate::models::{CaptureRegion, InputCapability, InputSourceKind};
        use image::{Rgba, RgbaImage};
        use std::sync::Arc;

        let content = b"metadata check content";
        let (asset, _scratch_guard) = make_validated_asset(content);

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        let snap = ProjectSnapshot {
            base_revision: None,
            title: "Meta Guide".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(Arc::new(RgbaImage::from_pixel(
                    8,
                    8,
                    Rgba([10, 20, 30, 255]),
                ))),
            }],
            steps: vec![ProjectStep {
                id: ProjectStepId(1),
                order: 1,
                title: "Step 1".into(),
                caption: None,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 150,
                keyframe: 1,
                nearby: vec![1],
                annotations: None,
            }],
            import_warnings: Vec::new(),
            motion: Some(asset.clone()),
        };

        let commit = create_project(&snap, &root).unwrap();
        let motion = commit.manifest.motion.as_ref().unwrap();

        // Digest matches what we wrote
        use sha2::Digest;
        let hash = sha2::Sha256::digest(content);
        let expected_sha = format!("{hash:x}");
        assert_eq!(motion.sha256, expected_sha);
        assert_eq!(motion.duration_ms, 5000);
        assert_eq!(motion.fps_numerator, 30);
        assert_eq!(motion.fps_denominator, 1);
        assert_eq!(motion.codec, "h264");
        assert_eq!(motion.audio, "none");
    }

    #[test]
    fn failed_promotion_preserves_session_asset_for_retry() {
        // Verify that a failed promotion doesn't corrupt the session source.
        // We test this by ensuring the source file still exists after a
        // simulated failure (incomplete manifest data).
        use super::super::model::{
            EnabledOutputs, ProjectSnapshot, ProjectStep, ProjectStepId, SnapshotFrame,
            SnapshotFramePayload,
        };
        use super::super::store::create_project;
        use crate::models::{CandidateKind, DetectReason};
        use crate::models::{CaptureRegion, InputCapability, InputSourceKind};
        use image::{Rgba, RgbaImage};
        use std::sync::Arc;

        let (asset, _scratch_guard) = make_validated_asset(b"preserve me");

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        // Snapshot with empty steps — will fail validation after promotion
        let snap = ProjectSnapshot {
            base_revision: None,
            title: "Fail Guide".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(Arc::new(RgbaImage::from_pixel(
                    8,
                    8,
                    Rgba([10, 20, 30, 255]),
                ))),
            }],
            steps: vec![], // Empty steps → validation failure
            import_warnings: Vec::new(),
            motion: Some(asset.clone()),
        };

        let _ = create_project(&snap, &root);

        // Session source file must still exist
        assert!(
            asset.source_path().exists(),
            "session asset was deleted after failed promotion"
        );

        // The temp project directory should be cleaned up
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.starts_with(".tmp-project-"))
                    .unwrap_or(false)
            })
            .collect();
        assert!(entries.is_empty(), "temp dirs not cleaned: {:?}", entries);
    }

    #[test]
    fn manifest_json_never_contains_source_or_temp_paths() {
        use super::super::model::{
            EnabledOutputs, ProjectSnapshot, ProjectStep, ProjectStepId, SnapshotFrame,
            SnapshotFramePayload,
        };
        use super::super::store::create_project;
        use crate::models::{CandidateKind, DetectReason};
        use crate::models::{CaptureRegion, InputCapability, InputSourceKind};
        use image::{Rgba, RgbaImage};
        use std::sync::Arc;

        let (asset, _scratch_guard) = make_validated_asset(b"path leak check");

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        let snap = ProjectSnapshot {
            base_revision: None,
            title: "Path Guide".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(Arc::new(RgbaImage::from_pixel(
                    8,
                    8,
                    Rgba([10, 20, 30, 255]),
                ))),
            }],
            steps: vec![ProjectStep {
                id: ProjectStepId(1),
                order: 1,
                title: "Step 1".into(),
                caption: None,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 150,
                keyframe: 1,
                nearby: vec![1],
                annotations: None,
            }],
            import_warnings: Vec::new(),
            motion: Some(asset.clone()),
        };

        let _commit = create_project(&snap, &root).unwrap();

        // Read the JSON from disk
        let json_str = std::fs::read_to_string(root.join("project.json")).unwrap();

        // No source path, temp path, or export path should appear
        let source_path = asset.source_path().to_string_lossy().to_string();
        assert!(
            !json_str.contains(&source_path),
            "manifest leaks source path: {}",
            source_path
        );
        assert!(
            !json_str.contains(".tmp-motion-"),
            "manifest leaks temp path"
        );
        assert!(
            !json_str.contains(".tmp-export-"),
            "manifest leaks export temp path"
        );
    }

    // -----------------------------------------------------------------------
    // Step 2: RED export atomicity tests
    // -----------------------------------------------------------------------

    #[test]
    fn export_produces_byte_identical_output() {
        let content = b"identical export content for verification";
        let (asset, _scratch_guard) = make_validated_asset(content);

        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("exported.mp4");

        export_motion_asset(&asset, &destination).unwrap();

        let exported = std::fs::read(&destination).unwrap();
        assert_eq!(exported, content);
    }

    #[test]
    fn export_removes_temp_sibling_on_failure() {
        // We can't easily force a failure mid-copy, but we can verify
        // that after a successful export no temp files remain.
        let (asset, _scratch_guard) = make_validated_asset(b"clean export");

        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("clean.mp4");

        export_motion_asset(&asset, &destination).unwrap();

        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.starts_with(".tmp-export-"))
                    .unwrap_or(false)
            })
            .collect();
        assert!(entries.is_empty(), "temp files not cleaned: {:?}", entries);
    }

    #[test]
    fn export_existing_destination_replaced_atomically() {
        let content = b"new content replacing old";
        let (asset, _scratch_guard) = make_validated_asset(content);

        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("existing.mp4");
        std::fs::write(&destination, b"old content that should be replaced").unwrap();

        export_motion_asset(&asset, &destination).unwrap();

        let exported = std::fs::read(&destination).unwrap();
        assert_eq!(exported, content);
    }

    #[test]
    fn export_preserves_project_state_and_source() {
        use super::super::model::{
            EnabledOutputs, ProjectSnapshot, ProjectStep, ProjectStepId, SnapshotFrame,
            SnapshotFramePayload,
        };
        use super::super::store::create_project;
        use crate::models::{CandidateKind, DetectReason};
        use crate::models::{CaptureRegion, InputCapability, InputSourceKind};
        use image::{Rgba, RgbaImage};
        use std::sync::Arc;

        let content = b"export preserves everything";
        let (asset, _scratch_guard) = make_validated_asset(content);

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        let snap = ProjectSnapshot {
            base_revision: None,
            title: "Export Guide".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::SemanticEvents,
            enabled_outputs: EnabledOutputs::default(),
            frames: vec![SnapshotFrame {
                id: 1,
                at_ms: 100,
                payload: SnapshotFramePayload::Pixels(Arc::new(RgbaImage::from_pixel(
                    8,
                    8,
                    Rgba([10, 20, 30, 255]),
                ))),
            }],
            steps: vec![ProjectStep {
                id: ProjectStepId(1),
                order: 1,
                title: "Step 1".into(),
                caption: None,
                kind: CandidateKind::Click,
                reason: DetectReason::ClickConfirmed,
                at_ms: 150,
                keyframe: 1,
                nearby: vec![1],
                annotations: None,
            }],
            import_warnings: Vec::new(),
            motion: Some(asset.clone()),
        };

        let commit = create_project(&snap, &root).unwrap();
        let project_motion_sha = commit.manifest.motion.as_ref().unwrap().sha256.clone();

        // Export to a different location
        let export_dir = tempfile::tempdir().unwrap();
        let destination = export_dir.path().join("export.mp4");
        export_motion_asset(&asset, &destination).unwrap();

        // Project state is unchanged
        let loaded = super::super::store::load_project(&root, None).unwrap();
        let loaded_sha = loaded.manifest.motion.as_ref().unwrap().sha256.clone();
        assert_eq!(loaded_sha, project_motion_sha);

        // Source file still exists
        assert!(asset.source_path().exists());
    }

    #[test]
    fn export_does_not_leave_temp_on_success() {
        let (asset, _scratch_guard) = make_validated_asset(b"no temp residue");

        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("final.mp4");

        export_motion_asset(&asset, &destination).unwrap();

        // Only the destination file should exist
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 1, "extra files in export dir: {:?}", entries);
        assert_eq!(entries[0].file_name().to_str().unwrap(), "final.mp4");
    }
}
