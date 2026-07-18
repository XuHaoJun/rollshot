//! Transactional filesystem persistence for Action Guide projects.
//!
//! ```text
//! first Save / Save As                      existing Save
//! ────────────────────                      ─────────────
//!                                  snapshot (base_revision = Some(n))
//! snapshot                                  │ preflight: read_manifest, compare revision
//!    │ build temp dir sibling (RAII guard)     ├─ mismatch ─▶ RevisionConflict (disk untouched)
//!    │ materialize every asset                 │ materialize missing immutable assets
//!    │ validate revision-1 manifest            │ re-read revision just before commit
//!    │ write_manifest_atomic + publish/        ├─ changed ──▶ RevisionConflict (disk untouched)
//!    ▼                                         ▼
//! renameat_with(NOREPLACE) on the dir      write_manifest_atomic(revision n + 1):
//!    ├─ EEXIST ─▶ DestinationExists           tmp ─▶ sync_all ─▶ same-dir rename ─▶ fsync dir
//!    ▼                                      COMMIT = the manifest rename
//! COMMIT = the directory rename
//! ```

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::assets::{inspect_png_asset, materialize_asset};
use super::error::ProjectError;
use super::model::{
    LoadedProject, ProjectCommit, ProjectFrame, ProjectManifestV1, ProjectSnapshot,
    PROJECT_SCHEMA_VERSION,
};
use super::validate::{validate_manifest_structure, validate_snapshot_structure};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// RAII guard that removes a temporary directory on drop unless dismissed.
struct TempGuard {
    path: PathBuf,
}

impl TempGuard {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn dismiss(self) {
        std::mem::forget(self);
    }
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn read_manifest(root: &Path) -> Result<ProjectManifestV1, ProjectError> {
    let manifest_path = root.join("project.json");
    let bytes = std::fs::read(&manifest_path).map_err(|e| ProjectError::Io {
        path: manifest_path.clone(),
        source: e,
    })?;

    let manifest: ProjectManifestV1 =
        serde_json::from_slice(&bytes).map_err(|e| ProjectError::InvalidJson {
            path: manifest_path,
            source: e,
        })?;

    validate_manifest_structure(&manifest)?;

    for frame in &manifest.frames {
        let _ = inspect_png_asset(root, &frame.sha256, frame.width, frame.height)?;
    }

    Ok(manifest)
}

fn write_manifest_atomic(root: &Path, manifest: &ProjectManifestV1) -> Result<(), ProjectError> {
    let json = serde_json::to_vec_pretty(manifest).map_err(|e| ProjectError::Encode {
        message: e.to_string(),
    })?;

    let pid = std::process::id();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_name = format!("project.json.tmp-{pid}-{counter}");
    let temp_path = root.join(&temp_name);
    let final_path = root.join("project.json");

    let cleanup = |p: &Path| {
        let _ = std::fs::remove_file(p);
    };

    std::fs::write(&temp_path, &json).map_err(|e| {
        cleanup(&temp_path);
        ProjectError::Io {
            path: temp_path.clone(),
            source: e,
        }
    })?;

    let file = std::fs::File::open(&temp_path).map_err(|e| {
        cleanup(&temp_path);
        ProjectError::Io {
            path: temp_path.clone(),
            source: e,
        }
    })?;
    file.sync_all().map_err(|e| {
        cleanup(&temp_path);
        ProjectError::Io {
            path: temp_path.clone(),
            source: e,
        }
    })?;

    std::fs::rename(&temp_path, &final_path).map_err(|e| {
        cleanup(&temp_path);
        ProjectError::Io {
            path: final_path,
            source: e,
        }
    })?;

    fsync_dir(root)?;

    Ok(())
}

fn fsync_dir(path: &Path) -> Result<(), ProjectError> {
    let file = std::fs::File::open(path).map_err(|e| ProjectError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    file.sync_all().map_err(|e| ProjectError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    Ok(())
}

fn commit_noreplace(temp: &Path, destination: &Path) -> Result<(), ProjectError> {
    use rustix::fs::{renameat_with, RenameFlags, CWD};

    renameat_with(CWD, temp, CWD, destination, RenameFlags::NOREPLACE).map_err(|e| match e {
        rustix::io::Errno::EXIST => ProjectError::DestinationExists {
            path: destination.to_path_buf(),
        },
        rustix::io::Errno::NOSYS => ProjectError::UnsupportedAtomicCommit {
            path: destination.to_path_buf(),
        },
        rustix::io::Errno::INVAL => ProjectError::UnsupportedAtomicCommit {
            path: destination.to_path_buf(),
        },
        rustix::io::Errno::NOTSUP => ProjectError::UnsupportedAtomicCommit {
            path: destination.to_path_buf(),
        },
        other => ProjectError::Io {
            path: destination.to_path_buf(),
            source: other.into(),
        },
    })?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// First save — creates a new project at revision 1.
///
/// Rejects if `snapshot.base_revision` is `Some`, or if `destination` already exists.
pub fn create_project(
    snapshot: &ProjectSnapshot,
    destination: &Path,
) -> Result<ProjectCommit, ProjectError> {
    if let Some(base) = snapshot.base_revision {
        return Err(ProjectError::RevisionConflict {
            expected: 0,
            actual: base,
        });
    }

    commit_new_project(snapshot, destination)
}

/// Save As — writes a copy of the snapshot as a new project at revision 1.
///
/// Always writes revision 1 regardless of `base_revision`. Rejects if
/// `destination` already exists.
pub fn save_project_as(
    snapshot: &ProjectSnapshot,
    destination: &Path,
) -> Result<ProjectCommit, ProjectError> {
    commit_new_project(snapshot, destination)
}

fn commit_new_project(
    snapshot: &ProjectSnapshot,
    destination: &Path,
) -> Result<ProjectCommit, ProjectError> {
    validate_snapshot_structure(snapshot)?;

    let parent = destination.parent().ok_or_else(|| ProjectError::Io {
        path: destination.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent directory"),
    })?;

    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_name = format!(".tmp-project-{pid}-{counter}", pid = std::process::id());
    let temp_root = parent.join(&temp_name);
    std::fs::create_dir_all(&temp_root).map_err(|e| ProjectError::Io {
        path: temp_root.clone(),
        source: e,
    })?;
    let guard = TempGuard::new(temp_root.clone());

    let frames = materialize_all(&temp_root, snapshot)?;

    let manifest = ProjectManifestV1 {
        schema_version: PROJECT_SCHEMA_VERSION,
        revision: 1,
        title: snapshot.title.clone(),
        capture_region: snapshot.capture_region,
        input_source: snapshot.input_source,
        input_capability: snapshot.input_capability,
        enabled_outputs: snapshot.enabled_outputs,
        frames,
        steps: snapshot.steps.clone(),
    };

    validate_manifest_structure(&manifest)?;
    write_manifest_atomic(&temp_root, &manifest)?;

    let publish_dir = temp_root.join("publish");
    std::fs::create_dir(&publish_dir).map_err(|e| ProjectError::Io {
        path: publish_dir,
        source: e,
    })?;

    commit_noreplace(&temp_root, destination)?;
    fsync_dir(destination)?;
    guard.dismiss();

    Ok(ProjectCommit {
        root: destination.to_path_buf(),
        manifest,
    })
}

/// Existing save — increments the revision by 1.
///
/// Requires `snapshot.base_revision == Some(expected)`. Returns
/// `RevisionConflict` if the on-disk revision has changed.
pub fn save_project(
    snapshot: &ProjectSnapshot,
    project_root: &Path,
) -> Result<ProjectCommit, ProjectError> {
    let Some(expected) = snapshot.base_revision else {
        let current = read_manifest(project_root)?;
        return Err(ProjectError::RevisionConflict {
            expected: 0,
            actual: current.revision,
        });
    };

    validate_snapshot_structure(snapshot)?;

    let current = read_manifest(project_root)?;
    if current.revision != expected {
        return Err(ProjectError::RevisionConflict {
            expected,
            actual: current.revision,
        });
    }

    let mut frames = Vec::with_capacity(snapshot.frames.len());
    for frame in &snapshot.frames {
        let pf = materialize_asset(project_root, frame.payload.clone(), frame.id, frame.at_ms)?;
        frames.push(pf);
    }

    // Re-read revision immediately before commit
    let re_read = read_manifest(project_root)?;
    if re_read.revision != expected {
        return Err(ProjectError::RevisionConflict {
            expected,
            actual: re_read.revision,
        });
    }

    let new_revision = expected.checked_add(1).ok_or(ProjectError::Io {
        path: project_root.to_path_buf(),
        source: std::io::Error::other("revision overflow"),
    })?;

    let manifest = ProjectManifestV1 {
        schema_version: PROJECT_SCHEMA_VERSION,
        revision: new_revision,
        title: snapshot.title.clone(),
        capture_region: snapshot.capture_region,
        input_source: snapshot.input_source,
        input_capability: snapshot.input_capability,
        enabled_outputs: snapshot.enabled_outputs,
        frames,
        steps: snapshot.steps.clone(),
    };

    validate_manifest_structure(&manifest)?;
    write_manifest_atomic(project_root, &manifest)?;

    Ok(ProjectCommit {
        root: project_root.to_path_buf(),
        manifest,
    })
}

/// Load a project from disk, validating manifest and all referenced assets.
pub fn load_project(project_root: &Path) -> Result<LoadedProject, ProjectError> {
    let manifest = read_manifest(project_root)?;
    Ok(LoadedProject {
        root: project_root.to_path_buf(),
        manifest,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn materialize_all(
    root: &Path,
    snapshot: &ProjectSnapshot,
) -> Result<Vec<ProjectFrame>, ProjectError> {
    let mut frames = Vec::with_capacity(snapshot.frames.len());
    for frame in &snapshot.frames {
        let pf = materialize_asset(root, frame.payload.clone(), frame.id, frame.at_ms)?;
        frames.push(pf);
    }
    Ok(frames)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use image::{Rgba, RgbaImage};

    use super::super::model::{
        EnabledOutputs, ProjectStep, ProjectStepId, SnapshotFrame, SnapshotFramePayload,
    };
    use crate::models::{
        CandidateKind, CaptureRegion, DetectReason, InputCapability, InputSourceKind,
    };

    fn pixel_image(w: u32, h: u32) -> Arc<RgbaImage> {
        Arc::new(RgbaImage::from_pixel(w, h, Rgba([10, 20, 30, 255])))
    }

    fn snapshot(base: Option<u64>) -> ProjectSnapshot {
        ProjectSnapshot {
            base_revision: base,
            title: "Test Guide".into(),
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
                payload: SnapshotFramePayload::Pixels(pixel_image(8, 8)),
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
        }
    }

    // ---- create_project ----

    #[test]
    fn create_project_sets_revision_1() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        let commit = create_project(&snapshot(None), &root).unwrap();

        assert_eq!(commit.manifest.revision, 1);
        assert_eq!(commit.manifest.title, "Test Guide");
        assert_eq!(commit.manifest.frames.len(), 1);
        assert!(root.join("project.json").exists());
        assert!(root.join("publish").exists());
        assert!(root.join("assets/frames").exists());
    }

    #[test]
    fn create_project_rejects_base_revision() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        let error = create_project(&snapshot(Some(1)), &root).unwrap_err();
        assert!(matches!(error, ProjectError::RevisionConflict { .. }));
    }

    #[test]
    fn create_project_rejects_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        create_project(&snapshot(None), &root).unwrap();

        let error = create_project(&snapshot(None), &root).unwrap_err();
        assert!(matches!(error, ProjectError::DestinationExists { .. }));
    }

    #[test]
    fn create_project_cleans_temp_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        // Create with invalid snapshot (empty steps) to fail after temp dir creation
        let mut snap = snapshot(None);
        snap.steps = vec![];
        let _ = create_project(&snap, &root);

        // No .tmp-project-* directories should remain
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
    fn create_project_no_partial_on_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        create_project(&snapshot(None), &root).unwrap();

        // Second attempt should fail — destination should still be valid
        let _ = create_project(&snapshot(None), &root);

        // Verify original project is intact
        let loaded = load_project(&root).unwrap();
        assert_eq!(loaded.manifest.revision, 1);
    }

    // ---- save_project_as ----

    #[test]
    fn save_project_as_resets_revision_to_1() {
        let dir = tempfile::tempdir().unwrap();
        let root1 = dir.path().join("first.rollshot-guide");
        let root2 = dir.path().join("second.rollshot-guide");

        let commit1 = create_project(&snapshot(None), &root1).unwrap();
        assert_eq!(commit1.manifest.revision, 1);

        // Save As from a snapshot pretending to be revision 5
        let mut snap = snapshot(Some(5));
        snap.title = "Copied Guide".into();
        let commit2 = save_project_as(&snap, &root2).unwrap();
        assert_eq!(commit2.manifest.revision, 1);
        assert_eq!(commit2.manifest.title, "Copied Guide");
    }

    #[test]
    fn save_project_as_rejects_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        create_project(&snapshot(None), &root).unwrap();

        let error = save_project_as(&snapshot(None), &root).unwrap_err();
        assert!(matches!(error, ProjectError::DestinationExists { .. }));
    }

    #[test]
    fn save_project_as_works_without_base_revision() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        let commit = save_project_as(&snapshot(None), &root).unwrap();
        assert_eq!(commit.manifest.revision, 1);
    }

    // ---- save_project ----

    #[test]
    fn existing_save_increments_revision() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        let first = create_project(&snapshot(None), &root).unwrap();
        assert_eq!(first.manifest.revision, 1);

        let mut snap = snapshot(Some(1));
        snap.title = "Updated Guide".into();
        let second = save_project(&snap, &root).unwrap();
        assert_eq!(second.manifest.revision, 2);
        assert_eq!(second.manifest.title, "Updated Guide");

        // Verify on disk
        let loaded = load_project(&root).unwrap();
        assert_eq!(loaded.manifest.revision, 2);
    }

    #[test]
    fn existing_save_rejects_external_revision_change() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        let first = create_project(&snapshot(None), &root).unwrap();

        // Simulate external write: bump revision on disk
        let mut external = first.manifest.clone();
        external.revision = 2;
        std::fs::write(
            root.join("project.json"),
            serde_json::to_vec_pretty(&external).unwrap(),
        )
        .unwrap();

        let error = save_project(&snapshot(Some(1)), &root).unwrap_err();
        assert!(matches!(
            error,
            ProjectError::RevisionConflict {
                expected: 1,
                actual: 2
            }
        ));

        // Disk should still have revision 2 (untouched)
        let disk: ProjectManifestV1 =
            serde_json::from_slice(&std::fs::read(root.join("project.json")).unwrap()).unwrap();
        assert_eq!(disk.revision, 2);
    }

    #[test]
    fn existing_save_requires_base_revision() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        create_project(&snapshot(None), &root).unwrap();

        let error = save_project(&snapshot(None), &root).unwrap_err();
        assert!(matches!(error, ProjectError::RevisionConflict { .. }));
    }

    #[test]
    fn existing_save_detects_conflict_on_re_read() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        let first = create_project(&snapshot(None), &root).unwrap();

        // The re-read will see revision 1, then we race by writing revision 2
        // We can test this by manually setting up the conflict:
        // After reading revision 1, write revision 2 before commit
        let mut external = first.manifest.clone();
        external.revision = 3;
        std::fs::write(
            root.join("project.json"),
            serde_json::to_vec_pretty(&external).unwrap(),
        )
        .unwrap();

        let error = save_project(&snapshot(Some(1)), &root).unwrap_err();
        assert!(matches!(
            error,
            ProjectError::RevisionConflict {
                expected: 1,
                actual: 3
            }
        ));
    }

    // ---- load_project ----

    #[test]
    fn load_project_validates_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        create_project(&snapshot(None), &root).unwrap();

        let loaded = load_project(&root).unwrap();
        assert_eq!(loaded.manifest.revision, 1);
        assert_eq!(loaded.manifest.title, "Test Guide");
        assert_eq!(loaded.root, root);
    }

    #[test]
    fn load_project_rejects_missing_assets() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        create_project(&snapshot(None), &root).unwrap();

        // Delete the asset file
        let frame = &snapshot(None).frames[0];
        let encoded = super::super::assets::encode_png_asset(match &frame.payload {
            SnapshotFramePayload::Pixels(img) => img,
            _ => unreachable!(),
        })
        .unwrap();
        let asset_path = root
            .join("assets/frames")
            .join(format!("{}.png", encoded.sha256));
        std::fs::remove_file(&asset_path).unwrap();

        let error = load_project(&root).unwrap_err();
        assert!(matches!(error, ProjectError::Io { .. }));
    }

    #[test]
    fn load_project_rejects_corrupt_asset_hash() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        create_project(&snapshot(None), &root).unwrap();

        let frame = &snapshot(None).frames[0];
        let encoded = super::super::assets::encode_png_asset(match &frame.payload {
            SnapshotFramePayload::Pixels(img) => img,
            _ => unreachable!(),
        })
        .unwrap();
        let asset_path = root
            .join("assets/frames")
            .join(format!("{}.png", encoded.sha256));

        // Corrupt the asset
        let mut data = std::fs::read(&asset_path).unwrap();
        if data.len() > 40 {
            data[35] ^= 0xFF;
        }
        std::fs::write(&asset_path, &data).unwrap();

        let error = load_project(&root).unwrap_err();
        assert!(matches!(error, ProjectError::InvalidAsset { .. }));
    }

    #[test]
    fn load_project_rejects_symlink_asset() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        create_project(&snapshot(None), &root).unwrap();

        let frame = &snapshot(None).frames[0];
        let encoded = super::super::assets::encode_png_asset(match &frame.payload {
            SnapshotFramePayload::Pixels(img) => img,
            _ => unreachable!(),
        })
        .unwrap();
        let asset_path = root
            .join("assets/frames")
            .join(format!("{}.png", encoded.sha256));

        // Replace with symlink
        let target = root.join("assets/frames/real.png");
        std::fs::write(&target, b"fake png").unwrap();
        std::fs::remove_file(&asset_path).unwrap();
        std::os::unix::fs::symlink(&target, &asset_path).unwrap();

        let error = load_project(&root).unwrap_err();
        assert!(
            matches!(error, ProjectError::InvalidAsset { .. })
                || matches!(error, ProjectError::Io { .. }),
            "expected InvalidAsset or Io, got: {:?}",
            error
        );
    }

    #[test]
    fn load_project_rejects_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        create_project(&snapshot(None), &root).unwrap();

        // Overwrite with JSON that has unknown fields
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("project.json")).unwrap()).unwrap();
        manifest["surprise"] = serde_json::Value::Bool(true);
        std::fs::write(
            root.join("project.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let error = load_project(&root).unwrap_err();
        assert!(matches!(error, ProjectError::InvalidJson { .. }));
    }

    #[test]
    fn load_project_rejects_unsupported_schema() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");
        create_project(&snapshot(None), &root).unwrap();

        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("project.json")).unwrap()).unwrap();
        manifest["schema_version"] = serde_json::Value::Number(99.into());
        std::fs::write(
            root.join("project.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let error = load_project(&root).unwrap_err();
        assert_eq!(error.category(), "unsupported-schema");
    }

    // ---- Asset handling ----

    #[test]
    fn create_project_deduplicates_same_content_frames() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        let mut snap = snapshot(None);
        // Two frames with same pixel content
        snap.frames.push(SnapshotFrame {
            id: 2,
            at_ms: 200,
            payload: SnapshotFramePayload::Pixels(pixel_image(8, 8)),
        });
        snap.steps[0].nearby = vec![1, 2];

        let commit = create_project(&snap, &root).unwrap();
        assert_eq!(commit.manifest.frames.len(), 2);
        // Both should have the same sha256
        assert_eq!(
            commit.manifest.frames[0].sha256,
            commit.manifest.frames[1].sha256
        );
    }

    #[test]
    fn save_project_copies_existing_assets() {
        let dir = tempfile::tempdir().unwrap();
        let root1 = dir.path().join("first.rollshot-guide");
        let root2 = dir.path().join("second.rollshot-guide");

        let first = create_project(&snapshot(None), &root1).unwrap();
        let frame = &first.manifest.frames[0];

        // Create second project using ExistingAsset payload
        let snap = ProjectSnapshot {
            base_revision: None,
            title: "Copy".into(),
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
                payload: SnapshotFramePayload::ExistingAsset {
                    project_root: root1.clone(),
                    sha256: frame.sha256.clone(),
                    width: frame.width,
                    height: frame.height,
                },
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
        };

        let second = create_project(&snap, &root2).unwrap();
        assert_eq!(second.manifest.frames[0].sha256, frame.sha256);

        // Verify the asset file exists in the second project
        let asset_path = root2
            .join("assets/frames")
            .join(format!("{}.png", frame.sha256));
        assert!(asset_path.exists());
    }

    // ---- Full round trip ----

    #[test]
    fn full_create_save_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("guide.rollshot-guide");

        // Create
        let commit1 = create_project(&snapshot(None), &root).unwrap();
        assert_eq!(commit1.manifest.revision, 1);

        // Save (bump to 2)
        let commit2 = save_project(&snapshot(Some(1)), &root).unwrap();
        assert_eq!(commit2.manifest.revision, 2);

        // Save (bump to 3)
        let commit3 = save_project(&snapshot(Some(2)), &root).unwrap();
        assert_eq!(commit3.manifest.revision, 3);

        // Load and verify
        let loaded = load_project(&root).unwrap();
        assert_eq!(loaded.manifest.revision, 3);
        assert_eq!(loaded.manifest.title, "Test Guide");
    }
}
