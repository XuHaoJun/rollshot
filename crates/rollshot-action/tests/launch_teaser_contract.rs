//! Headless acceptance harness for the launch teaser contract.
//!
//! Imports only public APIs. Creates a real project with persistent
//! synthetic motion and three reviewed steps, seeds a plan, edits one
//! bounded caption, renders a preview and final MP4, verifies output,
//! writes the sidecar, reloads it as current, increments the project
//! revision, and reloads it as stale.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use rollshot_action::launch_teaser::{
    load_launch_teaser_sidecar, render_launch_teaser, seed_launch_teaser,
    validate_launch_teaser_binding, write_launch_teaser_sidecar, AcceptedEditSourceV1,
    AcceptedEditV1, LaunchTeaserPreviewResult, LaunchTeaserRenderRequest, LaunchTeaserSidecarLoad,
    RenderProfile,
};
use rollshot_action::motion::{MotionAudio, MotionCodec, MotionMetadata, ValidatedMotionAsset};
use rollshot_action::project::publish::PublishCancellation;
use rollshot_action::project::{
    EnabledOutputs, LoadedProject, MotionAsset, MotionAssetLoad, ProjectFrame, ProjectManifestV3,
    ProjectStep, ProjectStepId,
};
use rollshot_action::video_import::VideoToolchain;
use rollshot_action::{
    CandidateKind, CaptureRegion, DegradedReason, DetectReason, FrameId, InputCapability,
    InputSourceKind, Millis,
};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Toolchain helpers
// ---------------------------------------------------------------------------

fn ffmpeg_path() -> String {
    std::env::var("ROLLSHOT_FFMPEG").unwrap_or_else(|_| "ffmpeg".to_string())
}

fn ffprobe_path() -> String {
    std::env::var("ROLLSHOT_FFPROBE").unwrap_or_else(|_| "ffprobe".to_string())
}

fn toolchain() -> VideoToolchain {
    VideoToolchain {
        ffmpeg: PathBuf::from(ffmpeg_path()),
        ffprobe: PathBuf::from(ffprobe_path()),
    }
}

fn ffmpeg_available() -> bool {
    Command::new(ffmpeg_path())
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn test_toolchain_available() -> bool {
    ffmpeg_available()
}

// ---------------------------------------------------------------------------
// Fixture builder
// ---------------------------------------------------------------------------

/// Build a real loaded project with synthetic motion and three reviewed steps.
fn public_project_fixture() -> (tempfile::TempDir, LoadedProject) {
    let dir = tempfile::tempdir().unwrap();

    // Generate a 30-second synthetic H.264 video.
    let mp4 = dir.path().join("recording.mp4");
    let status = Command::new(ffmpeg_path())
        .args([
            "-y",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=1920x1080:d=30:r=30,format=yuv420p",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-r",
            "30",
            "-an",
        ])
        .arg(&mp4)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("ffmpeg spawn failed");
    assert!(status.success(), "ffmpeg fixture generation failed");

    // Compute SHA-256.
    let bytes = std::fs::read(&mp4).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sha256 = format!("{:x}", hasher.finalize());

    let steps: Vec<ProjectStep> = (1..=3)
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
            sha256: sha256.clone(),
            duration_ms: 30_000,
            width: 1920,
            height: 1080,
            fps_numerator: 30,
            fps_denominator: 1,
            codec: "h264".into(),
            audio: "none".into(),
        }),
    };

    let motion = MotionAssetLoad::Available(ValidatedMotionAsset::new_for_test(
        MotionMetadata {
            sha256,
            duration_ms: 30_000,
            width: 1920,
            height: 1080,
            fps_numerator: 30,
            fps_denominator: 1,
            codec: MotionCodec::H264,
            audio: MotionAudio::None,
        },
        mp4,
        dir.path().to_path_buf(),
    ));

    let loaded = LoadedProject {
        root: dir.path().to_path_buf(),
        manifest,
        motion,
    };

    (dir, loaded)
}

// ---------------------------------------------------------------------------
// Acceptance test
// ---------------------------------------------------------------------------

#[test]
fn provider_free_launch_teaser_contract() {
    if !test_toolchain_available() {
        eprintln!("SKIP: ffmpeg/ffprobe not available");
        return;
    }

    let (_dir, loaded) = public_project_fixture();

    // Seed a plan from the loaded project.
    let mut plan = seed_launch_teaser(&loaded).unwrap();

    // Edit one bounded caption.
    plan.shots[0].caption = "Review the first step".into();
    plan.provenance.accepted_user_edits.push(AcceptedEditV1 {
        field_path: "shots[0].caption".into(),
        source: AcceptedEditSourceV1::User,
    });

    // Validate the edited plan.
    let validated = plan.validate().unwrap();

    // Verify binding.
    validate_launch_teaser_binding(&plan, &loaded).unwrap();

    // Render preview.
    let preview_dir = tempfile::tempdir().unwrap();
    let preview_dest = preview_dir.path().join("preview.mp4");
    let preview_result = render_launch_teaser(LaunchTeaserRenderRequest {
        loaded: &loaded,
        plan: &validated,
        toolchain: &toolchain(),
        cancellation: &PublishCancellation::new(),
        destination: &preview_dest,
        profile: RenderProfile::Preview,
    })
    .unwrap();

    match &preview_result {
        LaunchTeaserPreviewResult::Preview(preview) => {
            assert!(preview.output().is_file());
        }
        _ => panic!("expected preview result"),
    }

    // Render final.
    let final_dest = _dir.path().join("launch-teaser.mp4");
    let final_result = render_launch_teaser(LaunchTeaserRenderRequest {
        loaded: &loaded,
        plan: &validated,
        toolchain: &toolchain(),
        cancellation: &PublishCancellation::new(),
        destination: &final_dest,
        profile: RenderProfile::Final,
    })
    .unwrap();

    let final_meta = match &final_result {
        LaunchTeaserPreviewResult::Final(meta) => {
            assert_eq!(meta.width, 1920);
            assert_eq!(meta.height, 1080);
            assert_eq!(meta.audio_streams, 0);
            assert!(final_dest.is_file());
            meta.clone()
        }
        _ => panic!("expected final result"),
    };

    // Write sidecar.
    use rollshot_action::launch_teaser::LaunchTeaserArtifactV1;

    let plan_sha256 =
        rollshot_action::launch_teaser::persistence::compute_plan_sha256(&plan).unwrap();

    let artifact = LaunchTeaserArtifactV1 {
        schema_version: 1,
        plan: plan.clone(),
        plan_sha256,
        renderer_version: 1,
        ffmpeg_version: "test".into(),
        ffprobe_version: "test".into(),
        output_sha256: final_meta.output_sha256.clone(),
        rendered_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
    };

    write_launch_teaser_sidecar(&loaded.root, &artifact).unwrap();

    // Reload sidecar as current.
    let load = load_launch_teaser_sidecar(&loaded.root, &loaded);
    match load {
        LaunchTeaserSidecarLoad::Available(a) => {
            assert_eq!(a.plan.shots[0].caption, "Review the first step");
        }
        other => panic!("expected Available, got: {other:?}"),
    }

    // Increment project revision and reload as stale.
    let mut loaded_stale = loaded.clone();
    loaded_stale.manifest.revision += 1;
    let load_stale = load_launch_teaser_sidecar(&loaded.root, &loaded_stale);
    match load_stale {
        LaunchTeaserSidecarLoad::Stale(_) => {}
        other => panic!("expected Stale after revision bump, got: {other:?}"),
    }
}
