use std::path::{Path, PathBuf};

use super::VideoImportError;

pub(super) const MAX_PROBE_JSON_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoToolchain {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeMetadata {
    pub duration_ms: u64,
    pub display_width: u32,
    pub display_height: u32,
    pub rotation_degrees: i32,
}

pub fn probe_args(source: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-nostdin".into(),
        "-v".into(),
        "error".into(),
        "-an".into(),
        "-sn".into(),
        "-dn".into(),
        "-select_streams".into(),
        "v:0".into(),
        "-show_entries".into(),
        "stream=width,height,duration,side_data_list:format=duration".into(),
        "-of".into(),
        "json".into(),
    ];
    args.push(source.to_string_lossy().into_owned());
    args
}

pub fn parse_probe_json(raw: &[u8]) -> Result<ProbeMetadata, VideoImportError> {
    if raw.len() > MAX_PROBE_JSON_BYTES {
        return Err(VideoImportError::ProbeFailed);
    }

    let value: serde_json::Value =
        serde_json::from_slice(raw).map_err(|_| VideoImportError::ProbeFailed)?;

    let streams = value
        .get("streams")
        .and_then(|s| s.as_array())
        .ok_or(VideoImportError::ProbeFailed)?;

    if streams.is_empty() {
        return Err(VideoImportError::MissingVideoStream);
    }

    let stream = &streams[0];

    let width = stream
        .get("width")
        .and_then(|w| w.as_u64())
        .ok_or(VideoImportError::InvalidVideoMetadata)? as u32;
    let height = stream
        .get("height")
        .and_then(|h| h.as_u64())
        .ok_or(VideoImportError::InvalidVideoMetadata)? as u32;

    if width == 0 || height == 0 {
        return Err(VideoImportError::InvalidVideoMetadata);
    }

    let duration_secs = stream
        .get("duration")
        .and_then(|d| d.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| {
            value
                .get("format")
                .and_then(|f| f.get("duration"))
                .and_then(|d| d.as_str())
                .and_then(|s| s.parse::<f64>().ok())
        });

    let duration_secs = duration_secs.ok_or(VideoImportError::InvalidVideoMetadata)?;

    if !duration_secs.is_finite() || duration_secs <= 0.0 {
        return Err(VideoImportError::InvalidVideoMetadata);
    }

    let duration_ms = (duration_secs * 1000.0).round() as u64;

    let rotation = extract_rotation(stream);

    let (display_width, display_height) = if rotation == 90 || rotation == 270 {
        (height, width)
    } else {
        (width, height)
    };

    Ok(ProbeMetadata {
        duration_ms,
        display_width,
        display_height,
        rotation_degrees: rotation,
    })
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_command_requests_only_required_video_metadata() {
        let args = probe_args(Path::new("sentinel-source.mp4"));
        assert!(args.windows(2).any(|w| w == ["-select_streams", "v:0"]));
        assert!(args.windows(2).any(|w| w == ["-of", "json"]));
        assert!(args.iter().any(|arg| arg == "-an"));
        assert!(args.iter().any(|arg| arg == "-sn"));
    }

    #[test]
    fn metadata_rejects_missing_stream_and_invalid_dimensions() {
        assert_eq!(
            parse_probe_json(br#"{"streams":[],"format":{"duration":"2.0"}}"#)
                .unwrap_err()
                .category(),
            "missing_video_stream"
        );
        assert_eq!(
            parse_probe_json(invalid_dimensions_json())
                .unwrap_err()
                .category(),
            "invalid_video_metadata"
        );
    }

    fn invalid_dimensions_json() -> &'static [u8] {
        br#"{"streams":[{"width":0,"height":480,"duration":"1.0"}],"format":{"duration":"1.0"}}"#
    }

    #[test]
    fn valid_metadata_parses_width_height_duration() {
        let json = br#"{"streams":[{"width":1920,"height":1080,"duration":"10.5"}],"format":{"duration":"10.5"}}"#;
        let meta = parse_probe_json(json).unwrap();
        assert_eq!(meta.display_width, 1920);
        assert_eq!(meta.display_height, 1080);
        assert_eq!(meta.duration_ms, 10500);
        assert_eq!(meta.rotation_degrees, 0);
    }

    #[test]
    fn rotation_90_swaps_dimensions() {
        let json = br#"{"streams":[{"width":1920,"height":1080,"duration":"5.0","side_data_list":[{"rotation":90}]}],"format":{"duration":"5.0"}}"#;
        let meta = parse_probe_json(json).unwrap();
        assert_eq!(meta.display_width, 1080);
        assert_eq!(meta.display_height, 1920);
        assert_eq!(meta.rotation_degrees, 90);
    }

    #[test]
    fn rotation_270_swaps_dimensions() {
        let json = br#"{"streams":[{"width":1920,"height":1080,"duration":"5.0","side_data_list":[{"rotation":-90}]}],"format":{"duration":"5.0"}}"#;
        let meta = parse_probe_json(json).unwrap();
        assert_eq!(meta.display_width, 1080);
        assert_eq!(meta.display_height, 1920);
        assert_eq!(meta.rotation_degrees, 270);
    }

    #[test]
    fn format_duration_fallback() {
        let json = br#"{"streams":[{"width":640,"height":480}],"format":{"duration":"3.0"}}"#;
        let meta = parse_probe_json(json).unwrap();
        assert_eq!(meta.duration_ms, 3000);
    }

    #[test]
    fn zero_duration_rejected() {
        let json = br#"{"streams":[{"width":640,"height":480,"duration":"0.0"}],"format":{"duration":"0.0"}}"#;
        assert_eq!(
            parse_probe_json(json).unwrap_err().category(),
            "invalid_video_metadata"
        );
    }

    #[test]
    fn malformed_json_rejected() {
        assert_eq!(
            parse_probe_json(b"not json").unwrap_err().category(),
            "probe_failed"
        );
    }

    #[test]
    fn oversized_probe_json_rejected() {
        let big = vec![b'x'; MAX_PROBE_JSON_BYTES + 1];
        assert_eq!(
            parse_probe_json(&big).unwrap_err().category(),
            "probe_failed"
        );
    }
}
