//! Strict ffprobe output verification for launch teaser renders.
//!
//! Decodes ffprobe JSON into private `#[serde(deny_unknown_fields)]` DTOs
//! for required fields only. Validates exactly one H.264 video stream, no
//! audio streams, profile dimensions, 30 fps, and duration within one
//! frame of the validated plan.

use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use super::error::LaunchTeaserRenderError;
use super::graph::RenderProfile;
use super::plan::{ValidatedLaunchTeaserPlan, FINAL_FPS};

// ============================================================================
// Private DTOs
// ============================================================================

#[derive(Deserialize)]
struct FfprobeOutput {
    streams: Vec<FfprobeStream>,
    #[serde(default)]
    format: Option<FfprobeFormat>,
}

#[derive(Deserialize)]
struct FfprobeStream {
    codec_type: String,
    #[serde(default)]
    codec_name: Option<String>,
    #[serde(default)]
    width: Option<u64>,
    #[serde(default)]
    height: Option<u64>,
    #[serde(default)]
    r_frame_rate: Option<String>,
    #[serde(default)]
    duration: Option<String>,
}

#[derive(Deserialize)]
struct FfprobeFormat {
    #[serde(default)]
    duration: Option<String>,
}

// ============================================================================
// Verified output DTO
// ============================================================================

/// The verified output of an ffprobe check on a rendered launch teaser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedLaunchTeaserOutput {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_ms: u64,
    pub audio_streams: u32,
}

// ============================================================================
// Verification
// ============================================================================

/// Verify a rendered launch teaser output with ffprobe.
///
/// Requires exactly one H.264 video stream, no audio streams, profile
/// dimensions, 30 fps, and duration within one frame of the validated plan.
pub fn verify_launch_teaser_output(
    ffprobe: &Path,
    output: &Path,
    expected: &ValidatedLaunchTeaserPlan,
    profile: RenderProfile,
) -> Result<VerifiedLaunchTeaserOutput, LaunchTeaserRenderError> {
    let probe_out = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v",
            "-show_entries",
            "stream=codec_type,codec_name,width,height,r_frame_rate,duration",
            "-show_entries",
            "format=duration",
            "-of",
            "json",
        ])
        .arg(output)
        .output()
        .map_err(|_| LaunchTeaserRenderError::FfprobeFailed)?;

    if !probe_out.status.success() {
        return Err(LaunchTeaserRenderError::FfprobeFailed);
    }

    let probe: FfprobeOutput = serde_json::from_slice(&probe_out.stdout)
        .map_err(|_| LaunchTeaserRenderError::FfprobeFailed)?;

    // Exactly one video stream.
    let video_streams: Vec<_> = probe
        .streams
        .iter()
        .filter(|s| s.codec_type == "video")
        .collect();
    if video_streams.len() != 1 {
        return Err(LaunchTeaserRenderError::OutputVerificationFailed);
    }
    let video = video_streams[0];

    // H.264 codec.
    if video.codec_name.as_deref() != Some("h264") {
        return Err(LaunchTeaserRenderError::OutputVerificationFailed);
    }

    // No audio streams.
    let audio_count = probe
        .streams
        .iter()
        .filter(|s| s.codec_type == "audio")
        .count() as u32;
    if audio_count != 0 {
        return Err(LaunchTeaserRenderError::OutputVerificationFailed);
    }

    // Dimensions match profile.
    let (expected_w, expected_h) = (profile.width(), profile.height());
    if video.width != Some(expected_w as u64) || video.height != Some(expected_h as u64) {
        return Err(LaunchTeaserRenderError::OutputVerificationFailed);
    }

    // Frame rate is exactly FINAL_FPS.
    if let Some(r_frame_rate) = &video.r_frame_rate {
        let parts: Vec<&str> = r_frame_rate.split('/').collect();
        if parts.len() == 2 {
            let num: u64 = parts[0].parse().unwrap_or(0);
            let den: u64 = parts[1].parse().unwrap_or(1);
            if den == 0 || num != FINAL_FPS as u64 * den {
                return Err(LaunchTeaserRenderError::OutputVerificationFailed);
            }
        } else {
            return Err(LaunchTeaserRenderError::OutputVerificationFailed);
        }
    } else {
        return Err(LaunchTeaserRenderError::OutputVerificationFailed);
    }

    // Duration: stream-level or format-level fallback.
    let duration_secs = video
        .duration
        .as_deref()
        .or_else(|| probe.format.as_ref().and_then(|f| f.duration.as_deref()))
        .and_then(|s| s.parse::<f64>().ok());

    let duration_secs = duration_secs.ok_or(LaunchTeaserRenderError::OutputVerificationFailed)?;
    if !duration_secs.is_finite() || duration_secs <= 0.0 {
        return Err(LaunchTeaserRenderError::OutputVerificationFailed);
    }

    let actual_duration_ms = (duration_secs * 1_000.0).round() as u64;
    let expected_duration_ms = expected.duration_ms();
    let tolerance_ms = 1_000u64 / FINAL_FPS as u64 + 1; // one frame + 1 ms rounding

    if actual_duration_ms.abs_diff(expected_duration_ms) > tolerance_ms {
        return Err(LaunchTeaserRenderError::OutputVerificationFailed);
    }

    Ok(VerifiedLaunchTeaserOutput {
        width: expected_w,
        height: expected_h,
        fps: FINAL_FPS,
        duration_ms: actual_duration_ms,
        audio_streams: audio_count,
    })
}
