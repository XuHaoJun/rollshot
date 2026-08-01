//! Derived sidecar persistence for launch teaser plans.
//!
//! The sidecar is written atomically and never increments the project
//! revision. It stores the validated plan, plan digest, renderer metadata,
//! and output digest.

use std::path::Path;

use sha2::{Digest, Sha256};

use crate::project::LoadedProject;

use super::error::{
    LaunchTeaserArtifactV1, LaunchTeaserBindingError, LaunchTeaserPersistenceError,
    LaunchTeaserSidecarLoad,
};
use super::plan::{LaunchTeaserPlanV1, PLAN_DOMAIN_SEPARATOR};
use super::seed::validate_launch_teaser_binding;

/// Canonical sidecar relative path within the project root.
pub const SIDECAR_RELATIVE_PATH: &str = "publish/launch-teaser-plan-v1.json";

/// Compute the canonical plan digest with domain separator.
pub fn compute_plan_sha256(plan: &LaunchTeaserPlanV1) -> Result<String, LaunchTeaserPersistenceError> {
    let canonical =
        serde_json::to_vec(plan).map_err(|_| LaunchTeaserPersistenceError::Encoding)?;
    let mut hasher = Sha256::new();
    hasher.update(PLAN_DOMAIN_SEPARATOR);
    hasher.update(&canonical);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Verify a stored plan digest matches the canonical recomputation.
pub fn verify_plan_sha256(
    plan: &LaunchTeaserPlanV1,
    expected: &str,
) -> Result<(), LaunchTeaserPersistenceError> {
    let actual = compute_plan_sha256(plan)?;
    if actual != expected {
        return Err(LaunchTeaserPersistenceError::DigestMismatch);
    }
    Ok(())
}

/// Write the launch teaser artifact as an atomic sidecar.
///
/// Creates `publish/` if needed, writes to a temp sibling, syncs, and
/// renames. Does not increment the project revision.
pub fn write_launch_teaser_sidecar(
    project_root: &Path,
    artifact: &LaunchTeaserArtifactV1,
) -> Result<(), LaunchTeaserPersistenceError> {
    // Verify stored digest.
    verify_plan_sha256(&artifact.plan, &artifact.plan_sha256)?;

    let publish_dir = project_root.join("publish");
    std::fs::create_dir_all(&publish_dir).map_err(|_| LaunchTeaserPersistenceError::Io)?;

    let final_path = publish_dir.join("launch-teaser-plan-v1.json");

    // Write to a unique temp sibling.
    let pid = std::process::id();
    let temp_name = format!("launch-teaser-plan-v1.json.tmp-{pid}");
    let temp_path = publish_dir.join(&temp_name);

    let cleanup = |p: &Path| {
        let _ = std::fs::remove_file(p);
    };

    let json =
        serde_json::to_vec_pretty(artifact).map_err(|_| LaunchTeaserPersistenceError::Encoding)?;

    std::fs::write(&temp_path, &json).map_err(|e| {
        cleanup(&temp_path);
        let _ = e;
        LaunchTeaserPersistenceError::Io
    })?;

    // Sync the temp file.
    let file = std::fs::File::open(&temp_path).map_err(|e| {
        cleanup(&temp_path);
        let _ = e;
        LaunchTeaserPersistenceError::Io
    })?;
    file.sync_all().map_err(|e| {
        cleanup(&temp_path);
        let _ = e;
        LaunchTeaserPersistenceError::Io
    })?;

    // Atomic rename.
    std::fs::rename(&temp_path, &final_path).map_err(|e| {
        cleanup(&temp_path);
        let _ = e;
        LaunchTeaserPersistenceError::Io
    })?;

    // Sync the directory.
    let dir = std::fs::File::open(&publish_dir).map_err(|_| LaunchTeaserPersistenceError::Io)?;
    dir.sync_all().map_err(|_| LaunchTeaserPersistenceError::Io)?;

    Ok(())
}

/// Load the launch teaser sidecar from a project root.
///
/// Returns `Missing` if the file doesn't exist, `Unavailable` if it can't
/// be parsed, `Available` if current, or `Stale` if the project has moved on.
pub fn load_launch_teaser_sidecar(
    project_root: &Path,
    loaded: &LoadedProject,
) -> LaunchTeaserSidecarLoad {
    let path = project_root.join(SIDECAR_RELATIVE_PATH);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return LaunchTeaserSidecarLoad::Missing,
    };

    let artifact: LaunchTeaserArtifactV1 = match serde_json::from_slice(&bytes) {
        Ok(a) => a,
        Err(_) => return LaunchTeaserSidecarLoad::Unavailable,
    };

    // Verify plan digest.
    if verify_plan_sha256(&artifact.plan, &artifact.plan_sha256).is_err() {
        return LaunchTeaserSidecarLoad::Unavailable;
    }

    // Check freshness.
    match validate_launch_teaser_binding(&artifact.plan, loaded) {
        Ok(()) => LaunchTeaserSidecarLoad::Available(artifact),
        Err(LaunchTeaserBindingError::MissingStep) => LaunchTeaserSidecarLoad::Stale(artifact),
        Err(_) => LaunchTeaserSidecarLoad::Stale(artifact),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        CandidateKind, CaptureRegion, DetectReason, DegradedReason, FrameId, InputCapability,
        InputSourceKind, Millis,
    };
    use crate::motion::asset::ValidatedMotionAsset;
    use crate::motion::probe::{MotionAudio, MotionCodec, MotionMetadata};
    use crate::project::{
    EnabledOutputs, MotionAsset, MotionAssetLoad, ProjectFrame, ProjectManifestV3, ProjectStep,
    ProjectStepId,
};
    use crate::launch_teaser::plan::*;
    use crate::launch_teaser::seed::seed_launch_teaser;

    fn test_motion_metadata() -> MotionMetadata {
        MotionMetadata {
            sha256: "a".repeat(64),
            duration_ms: 30_000,
            width: 1920,
            height: 1080,
            fps_numerator: 30,
            fps_denominator: 1,
            codec: MotionCodec::H264,
            audio: MotionAudio::None,
        }
    }

    fn valid_plan_fixture() -> LaunchTeaserPlanV1 {
        LaunchTeaserPlanV1 {
            schema_version: LAUNCH_TEASER_SCHEMA_VERSION,
            source: LaunchTeaserSourceV1 {
                project_revision: 1,
                projection_digest: "a".repeat(64),
                motion_sha256: "b".repeat(64),
                motion_duration_ms: 30_000,
                motion_width: 1920,
                motion_height: 1080,
            },
            hook: "Test Hook".into(),
            shots: vec![
                LaunchTeaserShotV1 {
                    reviewed_step_id: ProjectStepId(1),
                    source_start_ms: 0,
                    source_end_ms: 5_000,
                    focus_path: FocusPathV1 {
                        start: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        end: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        zoom_permille: 1_000,
                    },
                    speed: SpeedV1::P1000,
                    caption: "First step".into(),
                    transition: TransitionV1::Cut,
                },
                LaunchTeaserShotV1 {
                    reviewed_step_id: ProjectStepId(2),
                    source_start_ms: 5_000,
                    source_end_ms: 10_000,
                    focus_path: FocusPathV1 {
                        start: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        end: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        zoom_permille: 1_000,
                    },
                    speed: SpeedV1::P1000,
                    caption: "Second step".into(),
                    transition: TransitionV1::Cut,
                },
                LaunchTeaserShotV1 {
                    reviewed_step_id: ProjectStepId(3),
                    source_start_ms: 10_000,
                    source_end_ms: 15_000,
                    focus_path: FocusPathV1 {
                        start: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        end: NormalizedPointV1 { x: 5_000, y: 5_000 },
                        zoom_permille: 1_000,
                    },
                    speed: SpeedV1::P1000,
                    caption: "Third step".into(),
                    transition: TransitionV1::Cut,
                },
            ],
            outro_text: "Made with Rollshot".into(),
            provenance: LaunchTeaserProvenanceV1 {
                deterministic_seed_version: 1,
                agent: None,
                repository_reads: Vec::new(),
                accepted_user_edits: Vec::new(),
            },
        }
    }

    fn artifact_fixture() -> LaunchTeaserArtifactV1 {
        let plan = valid_plan_fixture();
        let plan_sha256 = compute_plan_sha256(&plan).unwrap();
        LaunchTeaserArtifactV1 {
            schema_version: 1,
            plan,
            plan_sha256,
            renderer_version: 1,
            ffmpeg_version: "test".into(),
            ffprobe_version: "test".into(),
            output_sha256: "c".repeat(64),
            rendered_at_unix_ms: 1_700_000_000_000,
        }
    }

    fn loaded_project_fixture() -> (tempfile::TempDir, LoadedProject) {
        let dir = tempfile::tempdir().unwrap();

        // Create a project by writing the manifest directly.
        let steps: Vec<ProjectStep> = (1..=3)
            .map(|i| ProjectStep {
                id: ProjectStepId(i),
                order: i as u32,
                title: format!("Step {i}"),
                caption: Some(format!("Caption {i}")),
                kind: CandidateKind::Click,
                reason: DetectReason::VisualChange,
                at_ms: (i as u64) * 3_000,
                keyframe: i as FrameId,
                nearby: vec![i as FrameId],
                annotations: None,
            })
            .collect();

        let frames: Vec<ProjectFrame> = (1..=3)
            .map(|i| ProjectFrame {
                id: i as FrameId,
                at_ms: (i as u64 * 3_000) as Millis,
                sha256: "b".repeat(64),
                width: 1920,
                height: 1080,
            })
            .collect();

        let manifest = ProjectManifestV3 {
            schema_version: 3,
            revision: 1,
            title: "Test Guide".into(),
            capture_region: CaptureRegion {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            input_source: InputSourceKind::VisualOnly,
            input_capability: InputCapability::VisualOnly {
                reason: DegradedReason::SourceStartFailed,
            },
            enabled_outputs: EnabledOutputs::default(),
            frames,
            steps,
            import_warnings: Vec::new(),
            motion: Some(MotionAsset {
                relative_path: MotionAsset::CANONICAL_PATH.into(),
                sha256: "a".repeat(64),
                duration_ms: 30_000,
                width: 1920,
                height: 1080,
                fps_numerator: 30,
                fps_denominator: 1,
                codec: "h264".into(),
                audio: "none".into(),
            }),
        };

        // Write the manifest directly.
        let manifest_json = serde_json::to_vec_pretty(&manifest).unwrap();
        std::fs::write(dir.path().join("project.json"), &manifest_json).unwrap();

        let scratch = tempfile::tempdir().unwrap();
        let mp4 = scratch.path().join("recording.mp4");
        std::fs::write(&mp4, b"fake mp4").unwrap();
        let motion = MotionAssetLoad::Available(ValidatedMotionAsset::new_for_test(
            test_motion_metadata(),
            mp4,
            scratch.path().to_path_buf(),
        ));

        let loaded = LoadedProject {
            root: dir.path().to_path_buf(),
            manifest,
            motion,
        };

        (dir, loaded)
    }

    #[test]
    fn plan_sha256_is_deterministic() {
        let plan = valid_plan_fixture();
        let d1 = compute_plan_sha256(&plan).unwrap();
        let d2 = compute_plan_sha256(&plan).unwrap();
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 64);
        assert!(d1.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
    }

    #[test]
    fn plan_sha256_includes_domain_separator() {
        let plan = valid_plan_fixture();
        let with_sep = compute_plan_sha256(&plan).unwrap();
        // Compute without separator for comparison.
        let canonical = serde_json::to_vec(&plan).unwrap();
        let without_sep = format!("{:x}", Sha256::digest(&canonical));
        assert_ne!(with_sep, without_sep);
    }

    #[test]
    fn verify_plan_sha256_passes_for_correct_digest() {
        let plan = valid_plan_fixture();
        let digest = compute_plan_sha256(&plan).unwrap();
        assert!(verify_plan_sha256(&plan, &digest).is_ok());
    }

    #[test]
    fn verify_plan_sha256_rejects_wrong_digest() {
        let plan = valid_plan_fixture();
        assert_eq!(
            verify_plan_sha256(&plan, &"f".repeat(64)).unwrap_err(),
            LaunchTeaserPersistenceError::DigestMismatch
        );
    }

    #[test]
    fn sidecar_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let artifact = artifact_fixture();
        write_launch_teaser_sidecar(dir.path(), &artifact).unwrap();

        // Verify file exists.
        let path = dir.path().join(SIDECAR_RELATIVE_PATH);
        assert!(path.is_file());

        // Verify content matches.
        let bytes = std::fs::read(&path).unwrap();
        let loaded_artifact: LaunchTeaserArtifactV1 = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(loaded_artifact, artifact);
    }

    #[test]
    fn sidecar_rejects_mismatched_plan_digest() {
        let dir = tempfile::tempdir().unwrap();
        let mut artifact = artifact_fixture();
        artifact.plan_sha256 = "f".repeat(64);
        assert_eq!(
            write_launch_teaser_sidecar(dir.path(), &artifact).unwrap_err(),
            LaunchTeaserPersistenceError::DigestMismatch
        );
    }

    #[test]
    fn sidecar_rejects_unknown_json_fields() {
        let dir = tempfile::tempdir().unwrap();
        let artifact = artifact_fixture();
        let json = serde_json::to_string(&artifact).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), serde_json::json!(true));
        let bad_json = serde_json::to_vec_pretty(&value).unwrap();
        let sidecar_path = dir.path().join("publish").join("launch-teaser-plan-v1.json");
        std::fs::create_dir_all(dir.path().join("publish")).unwrap();
        std::fs::write(&sidecar_path, &bad_json).unwrap();

        let loaded = LoadedProject {
            root: dir.path().to_path_buf(),
            manifest: ProjectManifestV3 {
                schema_version: 3,
                revision: 1,
                title: "".into(),
                capture_region: CaptureRegion { x: 0, y: 0, width: 1, height: 1 },
                input_source: InputSourceKind::VisualOnly,
                input_capability: InputCapability::VisualOnly {
                    reason: DegradedReason::SourceStartFailed,
                },
                enabled_outputs: EnabledOutputs::default(),
                frames: vec![],
                steps: vec![],
                import_warnings: vec![],
                motion: None,
            },
            motion: MotionAssetLoad::None,
        };
        assert!(matches!(
            load_launch_teaser_sidecar(dir.path(), &loaded),
            LaunchTeaserSidecarLoad::Unavailable
        ));
    }

    #[test]
    fn sidecar_load_missing_when_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = LoadedProject {
            root: dir.path().to_path_buf(),
            manifest: ProjectManifestV3 {
                schema_version: 3,
                revision: 1,
                title: "".into(),
                capture_region: CaptureRegion { x: 0, y: 0, width: 1, height: 1 },
                input_source: InputSourceKind::VisualOnly,
                input_capability: InputCapability::VisualOnly {
                    reason: DegradedReason::SourceStartFailed,
                },
                enabled_outputs: EnabledOutputs::default(),
                frames: vec![],
                steps: vec![],
                import_warnings: vec![],
                motion: None,
            },
            motion: MotionAssetLoad::None,
        };
        assert!(matches!(
            load_launch_teaser_sidecar(dir.path(), &loaded),
            LaunchTeaserSidecarLoad::Missing
        ));
    }

    #[test]
    fn sidecar_load_returns_unavailable_for_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let publish = dir.path().join("publish");
        std::fs::create_dir_all(&publish).unwrap();
        std::fs::write(publish.join("launch-teaser-plan-v1.json"), b"not json").unwrap();

        let loaded = LoadedProject {
            root: dir.path().to_path_buf(),
            manifest: ProjectManifestV3 {
                schema_version: 3,
                revision: 1,
                title: "".into(),
                capture_region: CaptureRegion { x: 0, y: 0, width: 1, height: 1 },
                input_source: InputSourceKind::VisualOnly,
                input_capability: InputCapability::VisualOnly {
                    reason: DegradedReason::SourceStartFailed,
                },
                enabled_outputs: EnabledOutputs::default(),
                frames: vec![],
                steps: vec![],
                import_warnings: vec![],
                motion: None,
            },
            motion: MotionAssetLoad::None,
        };
        assert!(matches!(
            load_launch_teaser_sidecar(dir.path(), &loaded),
            LaunchTeaserSidecarLoad::Unavailable
        ));
    }

    #[test]
    fn sidecar_write_does_not_change_project_revision() {
        let (_dir, loaded) = loaded_project_fixture();
        let before = loaded.manifest.revision;
        let plan = seed_launch_teaser(&loaded).unwrap();
        let plan_sha256 = compute_plan_sha256(&plan).unwrap();
        let artifact = LaunchTeaserArtifactV1 {
            schema_version: 1,
            plan,
            plan_sha256,
            renderer_version: 1,
            ffmpeg_version: "test".into(),
            ffprobe_version: "test".into(),
            output_sha256: "c".repeat(64),
            rendered_at_unix_ms: 1_700_000_000_000,
        };
        write_launch_teaser_sidecar(&loaded.root, &artifact).unwrap();

        // Re-read the manifest directly to check revision unchanged.
        let manifest_bytes = std::fs::read(loaded.root.join("project.json")).unwrap();
        let reloaded_manifest: ProjectManifestV3 = serde_json::from_slice(&manifest_bytes).unwrap();
        assert_eq!(reloaded_manifest.revision, before);
    }
}
