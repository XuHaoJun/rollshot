//! Probe and validate the output of an FFmpeg motion recording session.
//!
//! Uses ffprobe's JSON output to extract stream metadata, then validates
//! codec, framerate, audio, and dimension constraints specific to the
//! motion recording pipeline.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::error::MotionFailureCategory;
use crate::video_import::VideoToolchain;

/// Closed set of video codecs the motion pipeline can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MotionCodec {
    H264,
}

impl MotionCodec {
    /// Return the ffprobe codec name string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::H264 => "h264",
        }
    }
}

/// Closed set of audio codecs. Currently the motion pipeline is always silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MotionAudio {
    None,
}

impl MotionAudio {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
        }
    }
}

/// Metadata extracted from a successfully probed motion recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MotionMetadata {
    /// SHA-256 hex digest of the final `.mp4` file.
    pub sha256: String,
    /// Duration in milliseconds (rounded from the probed float).
    pub duration_ms: u64,
    /// Display width in pixels.
    pub width: u32,
    /// Display height in pixels.
    pub height: u32,
    /// FPS numerator (e.g. 30 for 30/1).
    pub fps_numerator: u32,
    /// FPS denominator (e.g. 1 for 30/1).
    pub fps_denominator: u32,
    /// Video codec.
    pub codec: MotionCodec,
    /// Audio stream presence (always `None` for motion recordings).
    pub audio: MotionAudio,
}

/// Probe a motion recording `.mp4` file via ffprobe and return its metadata.
///
/// Validates:
/// - Exactly one video stream exists
/// - Codec is H.264
/// - Frame rate is exactly 30/1
/// - Zero audio streams
/// - Width and height match expected dimensions (accounting for rotation)
/// - Duration is positive
pub fn probe_motion(
    path: &Path,
    toolchain: &VideoToolchain,
    expected_width: u32,
    expected_height: u32,
) -> Result<MotionMetadata, MotionFailureCategory> {
    let output = Command::new(&toolchain.ffprobe)
        .args([
            "-v", "quiet",
            "-print_format", "json",
            "-show_format",
            "-show_streams",
            &path.to_string_lossy(),
        ])
        .output()
        .map_err(|_| MotionFailureCategory::ToolUnavailable)?;

    if !output.status.success() {
        return Err(MotionFailureCategory::Probe);
    }

    parse_motion_probe_json(
        &output.stdout,
        expected_width,
        expected_height,
        &compute_sha256(path)?,
    )
}

/// Compute the SHA-256 hex digest of a file.
fn compute_sha256(path: &Path) -> Result<String, MotionFailureCategory> {
    use sha2::Digest;
    let bytes = std::fs::read(path).map_err(|_| MotionFailureCategory::Digest)?;
    let hash = sha2::Sha256::digest(&bytes);
    Ok(format!("{hash:x}"))
}

/// Parse ffprobe JSON output and validate motion recording constraints.
///
/// This is the pure parsing/validation core, testable without spawning ffprobe.
pub fn parse_motion_probe_json(
    raw: &[u8],
    expected_width: u32,
    expected_height: u32,
    sha256: &str,
) -> Result<MotionMetadata, MotionFailureCategory> {
    let value: serde_json::Value =
        serde_json::from_slice(raw).map_err(|_| MotionFailureCategory::Probe)?;

    let streams = value
        .get("streams")
        .and_then(|s| s.as_array())
        .ok_or(MotionFailureCategory::Probe)?;

    // Separate video and audio streams.
    let mut video_streams: Vec<&serde_json::Value> = Vec::new();
    let mut has_audio = false;
    for stream in streams {
        match stream.get("codec_type").and_then(|c| c.as_str()) {
            Some("video") => video_streams.push(stream),
            Some("audio") => has_audio = true,
            _ => {}
        }
    }

    // Exactly one video stream.
    if video_streams.len() != 1 {
        return Err(MotionFailureCategory::Probe);
    }

    // No audio streams.
    if has_audio {
        return Err(MotionFailureCategory::Probe);
    }

    let video = video_streams[0];

    // Codec must be h264.
    let codec_name = video
        .get("codec_name")
        .and_then(|c| c.as_str())
        .ok_or(MotionFailureCategory::Probe)?;
    if codec_name != "h264" {
        return Err(MotionFailureCategory::Probe);
    }

    // Dimensions.
    let coded_width = video
        .get("width")
        .and_then(|w| w.as_u64())
        .ok_or(MotionFailureCategory::Probe)? as u32;
    let coded_height = video
        .get("height")
        .and_then(|h| h.as_u64())
        .ok_or(MotionFailureCategory::Probe)? as u32;

    // Check for rotation; if 90/270, display dimensions are swapped.
    let rotation = extract_rotation(video);
    let (display_width, display_height) = if rotation == 90 || rotation == 270 {
        (coded_height, coded_width)
    } else {
        (coded_width, coded_height)
    };

    if display_width != expected_width || display_height != expected_height {
        return Err(MotionFailureCategory::Probe);
    }

    // FPS: must be exactly 30/1.
    let (fps_n, fps_d) = parse_frame_rate(video)?;
    if fps_n != 30 || fps_d != 1 {
        return Err(MotionFailureCategory::Probe);
    }

    // Duration: from stream or format.
    let duration_ms = parse_duration(&value, video)?;

    Ok(MotionMetadata {
        sha256: sha256.to_owned(),
        duration_ms,
        width: display_width,
        height: display_height,
        fps_numerator: fps_n,
        fps_denominator: fps_d,
        codec: MotionCodec::H264,
        audio: MotionAudio::None,
    })
}

/// Parse the `r_frame_rate` field (e.g. "30/1").
fn parse_frame_rate(
    stream: &serde_json::Value,
) -> Result<(u32, u32), MotionFailureCategory> {
    let fps_str = stream
        .get("r_frame_rate")
        .and_then(|f| f.as_str())
        .ok_or(MotionFailureCategory::Probe)?;

    let parts: Vec<&str> = fps_str.split('/').collect();
    if parts.len() != 2 {
        return Err(MotionFailureCategory::Probe);
    }

    let n: u32 = parts[0].parse().map_err(|_| MotionFailureCategory::Probe)?;
    let d: u32 = parts[1].parse().map_err(|_| MotionFailureCategory::Probe)?;
    if d == 0 {
        return Err(MotionFailureCategory::Probe);
    }

    Ok((n, d))
}

/// Extract rotation from side_data_list (same logic as video_import/probe.rs).
fn extract_rotation(stream: &serde_json::Value) -> i32 {
    let side_data = stream.get("side_data_list").and_then(|s| s.as_array());
    if let Some(list) = side_data {
        for entry in list {
            if let Some(rotation) = entry.get("rotation").and_then(|r| r.as_i64()) {
                return normalize_rotation(rotation as i32);
            }
        }
    }
    0
}

fn normalize_rotation(raw: i32) -> i32 {
    let raw = raw.rem_euclid(360);
    match raw {
        0..=44 => 0,
        45..=134 => 90,
        135..=224 => 180,
        225..=314 => 270,
        _ => 0,
    }
}

/// Parse duration in milliseconds from the stream or format object.
fn parse_duration(
    root: &serde_json::Value,
    stream: &serde_json::Value,
) -> Result<u64, MotionFailureCategory> {
    let duration_secs = stream
        .get("duration")
        .and_then(|d| d.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| {
            root.get("format")
                .and_then(|f| f.get("duration"))
                .and_then(|d| d.as_str())
                .and_then(|s| s.parse::<f64>().ok())
        });

    let duration_secs = duration_secs.ok_or(MotionFailureCategory::Probe)?;
    if !duration_secs.is_finite() || duration_secs <= 0.0 {
        return Err(MotionFailureCategory::Probe);
    }

    Ok((duration_secs * 1000.0).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Valid single H.264 video stream, 30/1 fps, 640×480, 2s, no audio.
    fn valid_motion_json(width: u32, height: u32, fps: &str, duration: &str) -> Vec<u8> {
        format!(
            r#"{{
                "streams": [{{
                    "index": 0,
                    "codec_type": "video",
                    "codec_name": "h264",
                    "width": {width},
                    "height": {height},
                    "r_frame_rate": "{fps}",
                    "duration": "{duration}"
                }}],
                "format": {{
                    "duration": "{duration}"
                }}
            }}"#
        )
        .into_bytes()
    }

    fn valid_motion_json_with_audio(width: u32, height: u32) -> Vec<u8> {
        format!(
            r#"{{
                "streams": [
                    {{
                        "index": 0,
                        "codec_type": "video",
                        "codec_name": "h264",
                        "width": {width},
                        "height": {height},
                        "r_frame_rate": "30/1",
                        "duration": "2.0"
                    }},
                    {{
                        "index": 1,
                        "codec_type": "audio",
                        "codec_name": "aac"
                    }}
                ],
                "format": {{ "duration": "2.0" }}
            }}"#
        )
        .into_bytes()
    }

    fn valid_motion_json_with_rotation(width: u32, height: u32, rotation: i32) -> Vec<u8> {
        format!(
            r#"{{
                "streams": [{{
                    "index": 0,
                    "codec_type": "video",
                    "codec_name": "h264",
                    "width": {width},
                    "height": {height},
                    "r_frame_rate": "30/1",
                    "duration": "1.0",
                    "side_data_list": [{{ "rotation": {rotation} }}]
                }}],
                "format": {{ "duration": "1.0" }}
            }}"#
        )
        .into_bytes()
    }

    #[test]
    fn valid_single_h264_stream_parses() {
        let raw = valid_motion_json(640, 480, "30/1", "2.0");
        let meta = parse_motion_probe_json(&raw, 640, 480, "abc123").unwrap();
        assert_eq!(meta.codec, MotionCodec::H264);
        assert_eq!(meta.audio, MotionAudio::None);
        assert_eq!(meta.fps_numerator, 30);
        assert_eq!(meta.fps_denominator, 1);
        assert_eq!(meta.width, 640);
        assert_eq!(meta.height, 480);
        assert_eq!(meta.duration_ms, 2000);
        assert_eq!(meta.sha256, "abc123");
    }

    #[test]
    fn duration_within_34_ms_tolerance() {
        // 1.017s → 1017ms; within 34ms of 1000ms (but we just check it parses correctly)
        let raw = valid_motion_json(320, 240, "30/1", "1.017");
        let meta = parse_motion_probe_json(&raw, 320, 240, "x").unwrap();
        assert_eq!(meta.duration_ms, 1017);
    }

    #[test]
    fn rejects_h264_with_audio() {
        let raw = valid_motion_json_with_audio(640, 480);
        let err = parse_motion_probe_json(&raw, 640, 480, "x").unwrap_err();
        assert_eq!(err, MotionFailureCategory::Probe);
    }

    #[test]
    fn rejects_29_97_fps() {
        let raw = valid_motion_json(640, 480, "30000/1001", "2.0");
        let err = parse_motion_probe_json(&raw, 640, 480, "x").unwrap_err();
        assert_eq!(err, MotionFailureCategory::Probe);
    }

    #[test]
    fn rejects_wrong_dimensions() {
        let raw = valid_motion_json(640, 480, "30/1", "2.0");
        let err = parse_motion_probe_json(&raw, 1280, 720, "x").unwrap_err();
        assert_eq!(err, MotionFailureCategory::Probe);
    }

    #[test]
    fn rejects_malformed_json() {
        let err = parse_motion_probe_json(b"not json", 640, 480, "x").unwrap_err();
        assert_eq!(err, MotionFailureCategory::Probe);
    }

    #[test]
    fn rejects_empty_streams() {
        let raw = br#"{"streams": [], "format": {"duration": "1.0"}}"#;
        let err = parse_motion_probe_json(raw, 640, 480, "x").unwrap_err();
        assert_eq!(err, MotionFailureCategory::Probe);
    }

    #[test]
    fn rejects_second_video_stream() {
        let raw = br#"{
            "streams": [
                {"index": 0, "codec_type": "video", "codec_name": "h264",
                 "width": 640, "height": 480, "r_frame_rate": "30/1", "duration": "1.0"},
                {"index": 1, "codec_type": "video", "codec_name": "h264",
                 "width": 320, "height": 240, "r_frame_rate": "30/1"}
            ],
            "format": {"duration": "1.0"}
        }"#;
        let err = parse_motion_probe_json(raw, 640, 480, "x").unwrap_err();
        assert_eq!(err, MotionFailureCategory::Probe);
    }

    #[test]
    fn rejects_missing_duration() {
        let raw = br#"{
            "streams": [{"index": 0, "codec_type": "video", "codec_name": "h264",
                         "width": 640, "height": 480, "r_frame_rate": "30/1"}],
            "format": {}
        }"#;
        let err = parse_motion_probe_json(raw, 640, 480, "x").unwrap_err();
        assert_eq!(err, MotionFailureCategory::Probe);
    }

    #[test]
    fn rejects_non_h264_codec() {
        let raw = valid_motion_json(640, 480, "30/1", "2.0")
            .into_iter()
            .collect::<Vec<u8>>();
        // Replace codec_name
        let s = String::from_utf8(raw).unwrap().replace("h264", "vp9");
        let err = parse_motion_probe_json(s.as_bytes(), 640, 480, "x").unwrap_err();
        assert_eq!(err, MotionFailureCategory::Probe);
    }

    #[test]
    fn rotated_90_swap_display_dimensions() {
        let raw = valid_motion_json_with_rotation(480, 640, 90);
        let meta = parse_motion_probe_json(&raw, 640, 480, "x").unwrap();
        assert_eq!(meta.width, 640);
        assert_eq!(meta.height, 480);
    }

    #[test]
    fn rejects_rotation_display_size_mismatch() {
        // 90° rotation on 480×640 → display is 640×480; asking for 320×240 should fail
        let raw = valid_motion_json_with_rotation(480, 640, 90);
        let err = parse_motion_probe_json(&raw, 320, 240, "x").unwrap_err();
        assert_eq!(err, MotionFailureCategory::Probe);
    }

    #[test]
    fn accepts_format_level_duration() {
        let raw = br#"{
            "streams": [{"index": 0, "codec_type": "video", "codec_name": "h264",
                         "width": 640, "height": 480, "r_frame_rate": "30/1"}],
            "format": {"duration": "3.5"}
        }"#;
        let meta = parse_motion_probe_json(raw, 640, 480, "x").unwrap();
        assert_eq!(meta.duration_ms, 3500);
    }
}
