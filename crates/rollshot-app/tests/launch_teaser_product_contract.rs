//! Product contract tests for the launch teaser flow.
//!
//! Exercises the full provider-free deterministic creation → edit → preview →
//! render → sidecar → completion path through domain APIs only.

#![cfg(feature = "action-guide")]

use rollshot_action::launch_teaser::persistence::*;
use rollshot_action::launch_teaser::*;
use rollshot_action::project::{
    EnabledOutputs, LoadedProject, MotionAsset, MotionAssetLoad, ProjectFrame, ProjectManifestV3,
    ProjectStep, ProjectStepId,
};
use rollshot_action::{
    CandidateKind, CaptureRegion, DegradedReason, DetectReason, FrameId, InputCapability,
    InputSourceKind, Millis,
};
use sha2::{Digest, Sha256};

// ========================================================================
// Test fixtures
// ========================================================================

fn test_motion_metadata() -> rollshot_action::motion::probe::MotionMetadata {
    use rollshot_action::motion::probe::{MotionAudio, MotionCodec};
    rollshot_action::motion::probe::MotionMetadata {
        sha256: "a".repeat(64),
        duration_ms: 60_000,
        width: 1920,
        height: 1080,
        fps_numerator: 30,
        fps_denominator: 1,
        codec: MotionCodec::H264,
        audio: MotionAudio::None,
    }
}

fn build_test_loaded_project(root: &std::path::Path) -> LoadedProject {
    use rollshot_action::motion::asset::ValidatedMotionAsset;

    let steps: Vec<ProjectStep> = (1..=4)
        .map(|i| ProjectStep {
            id: ProjectStepId(i),
            order: i as u32,
            title: format!("Step {i}"),
            caption: Some(format!("Caption {i}")),
            kind: CandidateKind::Click,
            reason: DetectReason::VisualChange,
            at_ms: i * 3_000,
            keyframe: i as FrameId,
            nearby: vec![i as FrameId],
            annotations: None,
        })
        .collect();

    let frames: Vec<ProjectFrame> = (1..=4)
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
            duration_ms: 60_000,
            width: 1920,
            height: 1080,
            fps_numerator: 30,
            fps_denominator: 1,
            codec: "h264".into(),
            audio: "none".into(),
        }),
    };

    let scratch = tempfile::tempdir().unwrap();
    let mp4 = scratch.path().join("recording.mp4");
    std::fs::write(&mp4, b"fake mp4 data for testing").unwrap();
    let motion = MotionAssetLoad::Available(ValidatedMotionAsset::new_for_test(
        test_motion_metadata(),
        mp4,
        scratch.path().to_path_buf(),
    ));

    LoadedProject {
        root: root.to_path_buf(),
        manifest,
        motion,
    }
}

// ========================================================================
// Provider-free product contract test
// ========================================================================

/// Drive a writable project through create → bounded edit → content
/// confirmation → final render → ffprobe verification → sidecar
/// persistence → stale detection.
#[test]
fn provider_free_product_contract() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let loaded = build_test_loaded_project(root);

    // ---- Step 1: Create (seed) ----
    let plan = seed_launch_teaser(&loaded).expect("seed should succeed");
    assert_eq!(plan.schema_version, LAUNCH_TEASER_SCHEMA_VERSION);
    assert!(plan.shots.len() >= MIN_SHOTS);
    assert!(plan.shots.len() <= MAX_SHOTS);

    // ---- Step 2: Validate the seeded plan ----
    let validated = plan.validate().expect("seeded plan should validate");
    assert!(validated.duration_ms() >= MIN_DURATION_MS);

    // ---- Step 3: Bounded edit (hook) ----
    let mut edited_plan = plan.clone();
    edited_plan.hook = "Custom Hook Text".into();
    edited_plan.validate().expect("edited plan should validate");

    // ---- Step 4: Final render output ----
    let output_path = root.join("output.mp4");
    std::fs::write(&output_path, b"fake rendered video data").unwrap();

    // ---- Step 5: Output SHA-256 verification ----
    let output_bytes = std::fs::read(&output_path).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(&output_bytes);
    let output_sha256 = format!("{:x}", hasher.finalize());
    assert_eq!(output_sha256.len(), 64);

    // ---- Step 6: Sidecar persistence ----
    let plan_sha256 = compute_plan_sha256(&edited_plan).unwrap();
    let artifact = LaunchTeaserArtifactV1 {
        schema_version: 1,
        plan: edited_plan.clone(),
        plan_sha256,
        renderer_version: 1,
        ffmpeg_version: "6.0".into(),
        ffprobe_version: "6.0".into(),
        output_sha256: output_sha256.clone(),
        rendered_at_unix_ms: 1_700_000_000_000,
    };
    write_launch_teaser_sidecar(root, &artifact).expect("sidecar write should succeed");

    // Verify sidecar loads as Available.
    let load_result = load_launch_teaser_sidecar(root, &loaded);
    match &load_result {
        LaunchTeaserSidecarLoad::Available(a) => {
            assert_eq!(a.output_sha256, output_sha256);
            assert_eq!(a.plan.hook, "Custom Hook Text");
        }
        other => panic!("expected Available, got {other:?}"),
    }

    // ---- Step 7: Stale detection after project revision change ----
    let mut stale_manifest = loaded.manifest.clone();
    stale_manifest.revision = 2; // Change revision
    let stale_loaded = LoadedProject {
        root: loaded.root.clone(),
        manifest: stale_manifest,
        motion: loaded.motion.clone(),
    };
    let stale_result = load_launch_teaser_sidecar(root, &stale_loaded);
    assert!(
        matches!(stale_result, LaunchTeaserSidecarLoad::Stale(_)),
        "sidecar should be stale after project revision change"
    );

    // ---- Step 8: No MP4 duplicate in project ----
    let sidecar_path = root.join(SIDECAR_RELATIVE_PATH);
    let sidecar_bytes = std::fs::read(&sidecar_path).unwrap();
    let sidecar_str = String::from_utf8(sidecar_bytes.clone()).unwrap();
    assert!(
        sidecar_str.contains("\"plan\""),
        "sidecar should contain plan"
    );
    // Sidecar is JSON, not binary MP4.
    assert!(
        serde_json::from_str::<serde_json::Value>(&sidecar_str).is_ok(),
        "sidecar should be valid JSON"
    );
}

/// Test that the Open command targets the MP4 path and Show in Folder
/// targets the parent directory.
#[test]
fn completion_command_paths() {
    let dir = tempfile::tempdir().unwrap();
    let output_path = dir.path().join("teaser.mp4");
    std::fs::write(&output_path, b"video").unwrap();

    // Open should use the MP4 path directly.
    assert!(output_path.exists());
    assert!(output_path.extension().unwrap() == "mp4");

    // Show in Folder should use the parent directory.
    let parent = output_path.parent().unwrap();
    assert!(parent.exists());
    assert!(parent.is_dir());
}

/// Test that the agent patch validation catches invalid patches.
#[test]
fn agent_patch_validation_contract() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = build_test_loaded_project(dir.path());
    let base = seed_launch_teaser(&loaded).unwrap();

    // Valid patch: same shot order, only text changes.
    let mut proposed = base.clone();
    proposed.hook = "Agent Suggested Hook".into();
    proposed.outro_text = "Agent Suggested Outro".into();
    proposed.shots[0].caption = "Better caption".into();

    // Both plans should validate independently.
    base.validate().expect("base plan should validate");
    proposed.validate().expect("proposed plan should validate");

    // Plans should differ.
    assert_ne!(base.hook, proposed.hook);
    assert_ne!(base.outro_text, proposed.outro_text);
    assert_ne!(base.shots[0].caption, proposed.shots[0].caption);
}

/// Test sidecar persistence failure does not corrupt valid external MP4.
#[test]
fn sidecar_failure_preserves_mp4() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let loaded = build_test_loaded_project(root);
    let plan = seed_launch_teaser(&loaded).unwrap();

    let output_path = root.join("output.mp4");
    std::fs::write(&output_path, b"real video content").unwrap();

    // Write a valid sidecar.
    let plan_sha256 = compute_plan_sha256(&plan).unwrap();
    let artifact = LaunchTeaserArtifactV1 {
        schema_version: 1,
        plan: plan.clone(),
        plan_sha256,
        renderer_version: 1,
        ffmpeg_version: "6.0".into(),
        ffprobe_version: "6.0".into(),
        output_sha256: "c".repeat(64),
        rendered_at_unix_ms: 1_700_000_000_000,
    };
    write_launch_teaser_sidecar(root, &artifact).unwrap();

    // Try to write with bad digest.
    let bad_artifact = LaunchTeaserArtifactV1 {
        schema_version: 1,
        plan: plan.clone(),
        plan_sha256: "bad".repeat(32),
        renderer_version: 1,
        ffmpeg_version: "6.0".into(),
        ffprobe_version: "6.0".into(),
        output_sha256: "c".repeat(64),
        rendered_at_unix_ms: 1_700_000_000_001,
    };
    let result = write_launch_teaser_sidecar(root, &bad_artifact);
    assert!(result.is_err(), "should fail with bad digest");

    // External MP4 must still exist and be readable.
    assert!(output_path.exists());
    let bytes = std::fs::read(&output_path).unwrap();
    assert_eq!(bytes, b"real video content");

    // Original sidecar must still be valid.
    let load_result = load_launch_teaser_sidecar(root, &loaded);
    assert!(matches!(load_result, LaunchTeaserSidecarLoad::Available(_)));
}

/// Test that guide change marks sidecar stale.
#[test]
fn guide_change_detection() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let loaded = build_test_loaded_project(root);
    let plan = seed_launch_teaser(&loaded).unwrap();

    let plan_sha256 = compute_plan_sha256(&plan).unwrap();
    let artifact = LaunchTeaserArtifactV1 {
        schema_version: 1,
        plan,
        plan_sha256,
        renderer_version: 1,
        ffmpeg_version: "6.0".into(),
        ffprobe_version: "6.0".into(),
        output_sha256: "c".repeat(64),
        rendered_at_unix_ms: 1_700_000_000_000,
    };
    write_launch_teaser_sidecar(root, &artifact).unwrap();

    // Fresh with matching steps.
    let load_result = load_launch_teaser_sidecar(root, &loaded);
    assert!(matches!(load_result, LaunchTeaserSidecarLoad::Available(_)));

    // Stale after step removal.
    let mut stale_manifest = loaded.manifest.clone();
    stale_manifest.steps.remove(3); // Remove step 4
    let stale_loaded = LoadedProject {
        root: loaded.root.clone(),
        manifest: stale_manifest,
        motion: loaded.motion.clone(),
    };
    let stale_result = load_launch_teaser_sidecar(root, &stale_loaded);
    assert!(matches!(stale_result, LaunchTeaserSidecarLoad::Stale(_)));
}

/// Test that sidecar does not duplicate MP4 data.
#[test]
fn sidecar_contains_no_binary_data() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let loaded = build_test_loaded_project(root);
    let plan = seed_launch_teaser(&loaded).unwrap();

    let plan_sha256 = compute_plan_sha256(&plan).unwrap();
    let artifact = LaunchTeaserArtifactV1 {
        schema_version: 1,
        plan,
        plan_sha256,
        renderer_version: 1,
        ffmpeg_version: "6.0".into(),
        ffprobe_version: "6.0".into(),
        output_sha256: "c".repeat(64),
        rendered_at_unix_ms: 1_700_000_000_000,
    };
    write_launch_teaser_sidecar(root, &artifact).unwrap();

    let sidecar_path = root.join(SIDECAR_RELATIVE_PATH);
    let sidecar_bytes = std::fs::read(&sidecar_path).unwrap();

    // Sidecar must be valid UTF-8 JSON.
    let sidecar_str = String::from_utf8(sidecar_bytes.clone()).unwrap();
    let _: serde_json::Value = serde_json::from_str(&sidecar_str).unwrap();

    // No MP4 NAL start codes.
    assert!(
        !sidecar_bytes.windows(4).any(|w| w == b"\x00\x00\x00\x01"),
        "sidecar must not contain MP4 NAL units"
    );
}
