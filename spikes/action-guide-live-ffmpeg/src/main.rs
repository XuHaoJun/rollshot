mod workload;

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub(crate) struct RunConfig {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub output: PathBuf,
    pub report: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_secs: u64,
    pub queue_capacity: usize,
}

impl RunConfig {
    #[cfg(test)]
    pub(crate) fn for_test(width: u32, height: u32, fps: u32) -> Self {
        Self {
            ffmpeg: "ffmpeg".into(),
            ffprobe: "ffprobe".into(),
            output: "out.mp4".into(),
            report: "report.json".into(),
            width,
            height,
            fps,
            duration_secs: 2,
            queue_capacity: 2,
        }
    }
}

/// Parse CLI arguments into a RunConfig. Pure function — no side effects.
/// Returns `Err(message)` on invalid input.
pub(crate) fn parse_args(args: &[String]) -> Result<RunConfig, String> {
    let mut ffmpeg: Option<PathBuf> = None;
    let mut ffprobe: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut report: Option<PathBuf> = None;
    let mut width: u32 = 1920;
    let mut height: u32 = 1080;
    let mut fps: u32 = 30;
    let mut duration_secs: u64 = 600;
    let mut queue_capacity: usize = 2;

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--ffmpeg" => {
                ffmpeg = Some(PathBuf::from(
                    iter.next().ok_or("--ffmpeg requires a value")?,
                ));
            }
            "--ffprobe" => {
                ffprobe = Some(PathBuf::from(
                    iter.next().ok_or("--ffprobe requires a value")?,
                ));
            }
            "--output" => {
                output = Some(PathBuf::from(
                    iter.next().ok_or("--output requires a value")?,
                ));
            }
            "--report" => {
                report = Some(PathBuf::from(
                    iter.next().ok_or("--report requires a value")?,
                ));
            }
            "--width" => {
                let v = iter.next().ok_or("--width requires a value")?;
                width = v
                    .parse::<u32>()
                    .map_err(|_| format!("--width must be a u32, got {v}"))?;
            }
            "--height" => {
                let v = iter.next().ok_or("--height requires a value")?;
                height = v
                    .parse::<u32>()
                    .map_err(|_| format!("--height must be a u32, got {v}"))?;
            }
            "--fps" => {
                let v = iter.next().ok_or("--fps requires a value")?;
                fps = v
                    .parse::<u32>()
                    .map_err(|_| format!("--fps must be a u32, got {v}"))?;
            }
            "--duration-secs" => {
                let v = iter.next().ok_or("--duration-secs requires a value")?;
                duration_secs = v
                    .parse::<u64>()
                    .map_err(|_| format!("--duration-secs must be a u64, got {v}"))?;
            }
            "--queue-capacity" => {
                let v = iter.next().ok_or("--queue-capacity requires a value")?;
                queue_capacity = v
                    .parse::<usize>()
                    .map_err(|_| format!("--queue-capacity must be a usize, got {v}"))?;
            }
            other => {
                return Err(format!("unknown flag: {other}"));
            }
        }
    }

    let ffmpeg = ffmpeg.ok_or("--ffmpeg is required")?;
    let ffprobe = ffprobe.ok_or("--ffprobe is required")?;
    let output = output.ok_or("--output is required")?;
    let report = report.ok_or("--report is required")?;

    if width == 0 {
        return Err("--width must be non-zero".into());
    }
    if height == 0 {
        return Err("--height must be non-zero".into());
    }
    if fps == 0 {
        return Err("--fps must be non-zero".into());
    }
    if duration_secs == 0 {
        return Err("--duration-secs must be non-zero".into());
    }
    if queue_capacity == 0 {
        return Err("--queue-capacity must be non-zero".into());
    }
    if width % 2 != 0 {
        return Err(format!("--width must be even, got {width}"));
    }
    if height % 2 != 0 {
        return Err(format!("--height must be even, got {height}"));
    }

    Ok(RunConfig {
        ffmpeg,
        ffprobe,
        output,
        report,
        width,
        height,
        fps,
        duration_secs,
        queue_capacity,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Skip binary name
    match parse_args(&args[1..]) {
        Ok(config) => {
            // Task 3 will replace this with the actual run.
            // Print validated numeric fields only — no path fields.
            println!("validated: {}x{} @{} fps, {}s, queue={}", config.width, config.height, config.fps, config.duration_secs, config.queue_capacity);
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_valid_args_with_defaults() {
        let cfg = parse_args(&args(&[
            "--ffmpeg", "/usr/bin/ffmpeg",
            "--ffprobe", "/usr/bin/ffprobe",
            "--output", "out.mp4",
            "--report", "report.json",
        ]))
        .unwrap();
        assert_eq!(cfg.ffmpeg, PathBuf::from("/usr/bin/ffmpeg"));
        assert_eq!(cfg.ffprobe, PathBuf::from("/usr/bin/ffprobe"));
        assert_eq!(cfg.output, PathBuf::from("out.mp4"));
        assert_eq!(cfg.report, PathBuf::from("report.json"));
        assert_eq!(cfg.width, 1920);
        assert_eq!(cfg.height, 1080);
        assert_eq!(cfg.fps, 30);
        assert_eq!(cfg.duration_secs, 600);
        assert_eq!(cfg.queue_capacity, 2);
    }

    #[test]
    fn parse_custom_numeric_values() {
        let cfg = parse_args(&args(&[
            "--ffmpeg", "ffmpeg",
            "--ffprobe", "ffprobe",
            "--output", "o.mp4",
            "--report", "r.json",
            "--width", "1280",
            "--height", "720",
            "--fps", "60",
            "--duration-secs", "120",
            "--queue-capacity", "4",
        ]))
        .unwrap();
        assert_eq!(cfg.width, 1280);
        assert_eq!(cfg.height, 720);
        assert_eq!(cfg.fps, 60);
        assert_eq!(cfg.duration_secs, 120);
        assert_eq!(cfg.queue_capacity, 4);
    }

    #[test]
    fn missing_required_flag_is_error() {
        assert!(parse_args(&args(&["--ffmpeg", "ffmpeg"])).is_err());
    }

    #[test]
    fn unknown_flag_is_error() {
        let err = parse_args(&args(&[
            "--ffmpeg", "ffmpeg",
            "--ffprobe", "ffprobe",
            "--output", "o.mp4",
            "--report", "r.json",
            "--bogus",
        ]))
        .unwrap_err();
        assert!(err.contains("unknown flag"));
    }

    #[test]
    fn zero_width_is_error() {
        assert!(parse_args(&args(&[
            "--ffmpeg", "ffmpeg",
            "--ffprobe", "ffprobe",
            "--output", "o.mp4",
            "--report", "r.json",
            "--width", "0",
        ]))
        .is_err());
    }

    #[test]
    fn zero_fps_is_error() {
        assert!(parse_args(&args(&[
            "--ffmpeg", "ffmpeg",
            "--ffprobe", "ffprobe",
            "--output", "o.mp4",
            "--report", "r.json",
            "--fps", "0",
        ]))
        .is_err());
    }

    #[test]
    fn zero_duration_is_error() {
        assert!(parse_args(&args(&[
            "--ffmpeg", "ffmpeg",
            "--ffprobe", "ffprobe",
            "--output", "o.mp4",
            "--report", "r.json",
            "--duration-secs", "0",
        ]))
        .is_err());
    }

    #[test]
    fn zero_queue_capacity_is_error() {
        assert!(parse_args(&args(&[
            "--ffmpeg", "ffmpeg",
            "--ffprobe", "ffprobe",
            "--output", "o.mp4",
            "--report", "r.json",
            "--queue-capacity", "0",
        ]))
        .is_err());
    }

    #[test]
    fn odd_width_is_error() {
        let err = parse_args(&args(&[
            "--ffmpeg", "ffmpeg",
            "--ffprobe", "ffprobe",
            "--output", "o.mp4",
            "--report", "r.json",
            "--width", "1921",
        ]))
        .unwrap_err();
        assert!(err.contains("even"));
    }

    #[test]
    fn odd_height_is_error() {
        let err = parse_args(&args(&[
            "--ffmpeg", "ffmpeg",
            "--ffprobe", "ffprobe",
            "--output", "o.mp4",
            "--report", "r.json",
            "--height", "1081",
        ]))
        .unwrap_err();
        assert!(err.contains("even"));
    }

    #[test]
    fn missing_value_is_error() {
        assert!(parse_args(&args(&[
            "--ffmpeg", "ffmpeg",
            "--ffprobe", "ffprobe",
            "--output", "o.mp4",
            "--report",
        ]))
        .is_err());
    }

    #[test]
    fn invalid_number_is_error() {
        let err = parse_args(&args(&[
            "--ffmpeg", "ffmpeg",
            "--ffprobe", "ffprobe",
            "--output", "o.mp4",
            "--report", "r.json",
            "--width", "abc",
        ]))
        .unwrap_err();
        assert!(err.contains("u32"));
    }
}
