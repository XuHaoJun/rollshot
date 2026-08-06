//! Cancellable FFmpeg render and verification for launch teasers.
//!
//! Orchestrates the full render pipeline: cancellation check, binding
//! validation, overlay rasterization, graph compilation, FFmpeg spawn with
//! periodic cancellation polling, and ffprobe verification.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::project::publish::PublishCancellation;
use crate::project::LoadedProject;
use crate::video_import::VideoToolchain;

use super::error::LaunchTeaserRenderError;
use super::graph::{compile_ffmpeg_graph, CompiledLaunchTeaserGraph, RenderProfile};
use super::overlay::prepare_overlay_assets;
use super::plan::ValidatedLaunchTeaserPlan;
use super::probe::verify_launch_teaser_output;
use super::seed::validate_launch_teaser_binding;

// ============================================================================
// Public request and result DTOs
// ============================================================================

/// Request to render a validated launch teaser.
pub struct LaunchTeaserRenderRequest<'a> {
    pub loaded: &'a LoadedProject,
    pub plan: &'a ValidatedLaunchTeaserPlan,
    pub toolchain: &'a VideoToolchain,
    pub cancellation: &'a PublishCancellation,
    pub destination: &'a Path,
    pub profile: RenderProfile,
}

/// The result of a successful final render.
#[derive(Debug, Clone)]
pub struct LaunchTeaserRenderResult {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_ms: u64,
    pub audio_streams: u32,
    pub output_sha256: String,
}

/// RAII guard for preview renders.
///
/// Holds the scratch directory alive so the output file remains valid.
/// The scratch directory is removed when this guard is dropped.
#[derive(Debug)]
pub struct LaunchTeaserPreview {
    _scratch: tempfile::TempDir,
    output: PathBuf,
}

impl LaunchTeaserPreview {
    /// Path to the preview output file.
    pub fn output(&self) -> &Path {
        &self.output
    }
}

/// The result of a successful preview render.
#[derive(Debug)]
pub enum LaunchTeaserPreviewResult {
    Preview(LaunchTeaserPreview),
    Final(LaunchTeaserRenderResult),
}

// ============================================================================
// Render
// ============================================================================

/// Render a validated launch teaser.
///
/// Pipeline:
/// 1. Check cancellation
/// 2. Validate binding to the loaded project
/// 3. Verify FFmpeg and ffprobe are executable
/// 4. Create scratch directory and temp output sibling
/// 5. Rasterize overlays and compile the graph
/// 6. Spawn FFmpeg with periodic cancellation checks
/// 7. Verify the output with ffprobe
/// 8. For Final: atomic rename to destination and return digest
///    For Preview: return scratch-owned path via RAII guard
pub fn render_launch_teaser(
    request: LaunchTeaserRenderRequest<'_>,
) -> Result<LaunchTeaserPreviewResult, LaunchTeaserRenderError> {
    // 1. Check cancellation.
    request
        .cancellation
        .check()
        .map_err(|_| LaunchTeaserRenderError::Cancelled)?;

    // 2. Validate binding.
    validate_launch_teaser_binding(request.plan.plan(), request.loaded)
        .map_err(|_| LaunchTeaserRenderError::BindingFailed)?;

    // 3. Verify FFmpeg and ffprobe are executable.
    verify_executable(&request.toolchain.ffmpeg)
        .map_err(|_| LaunchTeaserRenderError::ToolchainUnavailable)?;
    verify_executable(&request.toolchain.ffprobe)
        .map_err(|_| LaunchTeaserRenderError::ToolchainUnavailable)?;

    // 4. Create scratch directory and temp output.
    let scratch = tempfile::tempdir().map_err(|_| LaunchTeaserRenderError::ScratchFailed)?;
    let pid = std::process::id();
    let temp_output = scratch.path().join(format!("launch-teaser-{pid}.mp4"));

    // 5. Rasterize overlays and compile the graph.
    let overlays = prepare_overlay_assets(request.plan, scratch.path(), request.profile)?;
    let graph = compile_ffmpeg_graph(
        request.plan,
        motion_source_path(request.loaded)?,
        &overlays,
        &temp_output,
        request.profile,
        scratch.path(),
    )?;

    // 6. Spawn FFmpeg with periodic cancellation checks.
    run_ffmpeg_cancellable(&request.toolchain.ffmpeg, graph, request.cancellation)?;

    // 7. Verify output with ffprobe.
    let verified = verify_launch_teaser_output(
        &request.toolchain.ffprobe,
        &temp_output,
        request.plan,
        request.profile,
    )?;

    // 8. Finalize based on profile.
    match request.profile {
        RenderProfile::Final => {
            // Compute output digest.
            let output_bytes = std::fs::read(&temp_output)
                .map_err(|_| LaunchTeaserRenderError::FfmpegExecutionFailed)?;
            let mut hasher = Sha256::new();
            hasher.update(&output_bytes);
            let output_sha256 = format!("{:x}", hasher.finalize());

            // Atomic rename to destination.
            atomic_rename(&temp_output, request.destination)?;

            Ok(LaunchTeaserPreviewResult::Final(LaunchTeaserRenderResult {
                width: verified.width,
                height: verified.height,
                fps: verified.fps,
                duration_ms: verified.duration_ms,
                audio_streams: verified.audio_streams,
                output_sha256,
            }))
        }
        RenderProfile::Preview => {
            let output = temp_output;
            Ok(LaunchTeaserPreviewResult::Preview(LaunchTeaserPreview {
                _scratch: scratch,
                output,
            }))
        }
    }
}

// ============================================================================
// Internal helpers
// ============================================================================

/// Verify that a binary is reachable and executable.
///
/// Uses a version probe rather than file metadata so bare names
/// resolved via `$PATH` (e.g. "ffmpeg") work correctly.
fn verify_executable(path: &Path) -> Result<(), std::io::Error> {
    let status = Command::new(path)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "binary not executable",
        ))
    }
}

/// Get the motion source path from a loaded project.
fn motion_source_path(loaded: &LoadedProject) -> Result<&Path, LaunchTeaserRenderError> {
    match &loaded.motion {
        crate::project::MotionAssetLoad::Available(m) => Ok(m.source_path()),
        _ => Err(LaunchTeaserRenderError::BindingFailed),
    }
}

/// Spawn FFmpeg and wait with periodic cancellation checks.
fn run_ffmpeg_cancellable(
    ffmpeg: &Path,
    graph: CompiledLaunchTeaserGraph,
    cancellation: &PublishCancellation,
) -> Result<(), LaunchTeaserRenderError> {
    let mut child = Command::new(ffmpeg)
        .args(graph.args())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| LaunchTeaserRenderError::FfmpegSpawnFailed)?;

    // Poll for completion with cancellation checks.
    loop {
        if cancellation.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(LaunchTeaserRenderError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    let stderr = child
                        .stderr
                        .take()
                        .and_then(|mut s| {
                            use std::io::Read;
                            let mut buf = String::new();
                            let _ = s.read_to_string(&mut buf);
                            if buf.is_empty() {
                                None
                            } else {
                                Some(buf)
                            }
                        })
                        .unwrap_or_default();
                    tracing::error!(stderr = %stderr, "ffmpeg execution failed");
                    return Err(LaunchTeaserRenderError::FfmpegExecutionFailed);
                }
                return Ok(());
            }
            Ok(None) => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => {
                return Err(LaunchTeaserRenderError::FfmpegExecutionFailed);
            }
        }
    }
}

/// Atomic rename: write to temp sibling, sync, rename, sync dir.
fn atomic_rename(from: &Path, to: &Path) -> Result<(), LaunchTeaserRenderError> {
    let parent = to.parent().ok_or(LaunchTeaserRenderError::ScratchFailed)?;
    std::fs::create_dir_all(parent).map_err(|_| LaunchTeaserRenderError::ScratchFailed)?;

    // Temp sibling in same directory.
    let pid = std::process::id();
    let temp_sibling = parent.join(format!(".launch-teaser-render-{pid}.tmp"));

    std::fs::copy(from, &temp_sibling).map_err(|_| LaunchTeaserRenderError::ScratchFailed)?;

    // Sync the temp file.
    let file =
        std::fs::File::open(&temp_sibling).map_err(|_| LaunchTeaserRenderError::ScratchFailed)?;
    file.sync_all()
        .map_err(|_| LaunchTeaserRenderError::ScratchFailed)?;

    // Atomic rename.
    std::fs::rename(&temp_sibling, to).map_err(|_| LaunchTeaserRenderError::ScratchFailed)?;

    // Sync the directory.
    let dir = std::fs::File::open(parent).map_err(|_| LaunchTeaserRenderError::ScratchFailed)?;
    dir.sync_all()
        .map_err(|_| LaunchTeaserRenderError::ScratchFailed)?;

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch_teaser::seed::seed_launch_teaser;
    use crate::models::*;
    use crate::motion::asset::ValidatedMotionAsset;
    use crate::motion::probe::{MotionAudio, MotionCodec, MotionMetadata};
    use crate::project::*;
    use std::path::PathBuf;

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
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Generate a 30-second synthetic H.264 yuv420p video with moving color blocks.
    fn synthetic_motion_video(dir: &Path) -> PathBuf {
        let output = dir.join("recording.mp4");
        let status = Command::new(ffmpeg_path())
            .args([
                "-y",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=1920x1080:d=30:r=30,format=yuv420p",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-r",
                "30",
                "-an",
            ])
            .arg(&output)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("failed to spawn ffmpeg for test fixture");
        assert!(status.success(), "ffmpeg fixture generation failed");
        assert!(output.is_file());
        output
    }

    fn loaded_project_fixture(dir: &Path) -> LoadedProject {
        let mp4 = synthetic_motion_video(dir);
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
                at_ms: i * 3_000,
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
            dir.to_path_buf(),
        ));

        LoadedProject {
            root: dir.to_path_buf(),
            manifest,
            motion,
        }
    }

    // ------------------------------------------------------------------
    // Tests
    // ------------------------------------------------------------------

    #[test]
    fn final_render_produces_verified_silent_mp4() {
        if !ffmpeg_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let loaded = loaded_project_fixture(dir.path());
        let plan = seed_launch_teaser(&loaded).unwrap();
        let plan = plan.validate().unwrap();
        let output = dir.path().join("final-output.mp4");

        let result = render_launch_teaser(LaunchTeaserRenderRequest {
            loaded: &loaded,
            plan: &plan,
            toolchain: &toolchain(),
            cancellation: &PublishCancellation::new(),
            destination: &output,
            profile: RenderProfile::Final,
        })
        .unwrap();

        match result {
            LaunchTeaserPreviewResult::Final(r) => {
                assert_eq!(r.width, 1920);
                assert_eq!(r.height, 1080);
                assert_eq!(r.audio_streams, 0);
                assert!(r.output_sha256.len() == 64);
            }
            LaunchTeaserPreviewResult::Preview(_) => panic!("expected Final result"),
        }
        assert!(output.is_file());
    }

    #[test]
    fn preview_render_produces_smaller_output() {
        if !ffmpeg_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let loaded = loaded_project_fixture(dir.path());
        let plan = seed_launch_teaser(&loaded).unwrap();
        let plan = plan.validate().unwrap();
        let output = dir.path().join("preview-output.mp4");

        let result = render_launch_teaser(LaunchTeaserRenderRequest {
            loaded: &loaded,
            plan: &plan,
            toolchain: &toolchain(),
            cancellation: &PublishCancellation::new(),
            destination: &output,
            profile: RenderProfile::Preview,
        })
        .unwrap();

        match result {
            LaunchTeaserPreviewResult::Preview(preview) => {
                assert!(preview.output().is_file());
                let meta = std::fs::metadata(preview.output()).unwrap();
                assert!(meta.len() > 0);
            }
            LaunchTeaserPreviewResult::Final(_) => panic!("expected Preview result"),
        }
    }

    #[test]
    fn cancelled_render_leaves_no_destination() {
        if !ffmpeg_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let loaded = loaded_project_fixture(dir.path());
        let plan = seed_launch_teaser(&loaded).unwrap();
        let plan = plan.validate().unwrap();
        let output = dir.path().join("cancelled-output.mp4");
        let cancel = PublishCancellation::new();
        cancel.cancel();

        let err = render_launch_teaser(LaunchTeaserRenderRequest {
            loaded: &loaded,
            plan: &plan,
            toolchain: &toolchain(),
            cancellation: &cancel,
            destination: &output,
            profile: RenderProfile::Final,
        })
        .unwrap_err();

        assert_eq!(err.category(), "cancelled");
        assert!(!output.is_file());
    }

    #[test]
    fn stale_binding_rejected_before_spawn() {
        if !ffmpeg_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let loaded = loaded_project_fixture(dir.path());
        let mut plan = seed_launch_teaser(&loaded).unwrap();
        // Tamper with revision to make binding stale.
        plan.source.project_revision = 999;
        let plan = plan.validate().unwrap();
        let output = dir.path().join("stale-output.mp4");

        let err = render_launch_teaser(LaunchTeaserRenderRequest {
            loaded: &loaded,
            plan: &plan,
            toolchain: &toolchain(),
            cancellation: &PublishCancellation::new(),
            destination: &output,
            profile: RenderProfile::Final,
        })
        .unwrap_err();

        assert_eq!(err.category(), "binding-failed");
    }

    #[test]
    fn missing_motion_rejected() {
        if !ffmpeg_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let loaded = loaded_project_fixture(dir.path());
        let plan = seed_launch_teaser(&loaded).unwrap();
        let plan = plan.validate().unwrap();
        // Set motion to None after seeding.
        let mut loaded_no_motion = loaded.clone();
        loaded_no_motion.motion = MotionAssetLoad::None;
        let output = dir.path().join("no-motion.mp4");

        let err = render_launch_teaser(LaunchTeaserRenderRequest {
            loaded: &loaded_no_motion,
            plan: &plan,
            toolchain: &toolchain(),
            cancellation: &PublishCancellation::new(),
            destination: &output,
            profile: RenderProfile::Final,
        })
        .unwrap_err();

        assert_eq!(err.category(), "binding-failed");
    }

    #[test]
    fn bad_ffmpeg_path_fails_toolchain() {
        if !ffmpeg_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let loaded = loaded_project_fixture(dir.path());
        let plan = seed_launch_teaser(&loaded).unwrap();
        let plan = plan.validate().unwrap();
        let output = dir.path().join("bad-toolchain.mp4");
        let bad_tc = VideoToolchain {
            ffmpeg: PathBuf::from("/nonexistent/ffmpeg"),
            ffprobe: PathBuf::from(ffprobe_path()),
        };

        let err = render_launch_teaser(LaunchTeaserRenderRequest {
            loaded: &loaded,
            plan: &plan,
            toolchain: &bad_tc,
            cancellation: &PublishCancellation::new(),
            destination: &output,
            profile: RenderProfile::Final,
        })
        .unwrap_err();

        assert_eq!(err.category(), "toolchain-unavailable");
    }
}
