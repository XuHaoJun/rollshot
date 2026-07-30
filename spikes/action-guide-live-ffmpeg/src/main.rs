mod metrics;
mod pipeline;
mod workload;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

// Dead code suppression: path fields are consumed by run_encoder (future task).
#[allow(dead_code)]
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
    if !width.is_multiple_of(2) {
        return Err(format!("--width must be even, got {width}"));
    }
    if !height.is_multiple_of(2) {
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
    let config = match parse_args(&args[1..]) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    // Ensure report parent directory exists.
    if let Some(parent) = config.report.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let total_frames = config.duration_secs * config.fps as u64;
    let frame_interval = Duration::from_nanos(1_000_000_000 / config.fps as u64);
    let start = Instant::now();

    // Shared PID for RSS sampling of the FFmpeg child.
    let ffmpeg_child_pid = Arc::new(AtomicU32::new(0));

    // Create mailbox.
    let (sender, receiver) = pipeline::latest_frame_mailbox(config.queue_capacity);

    // Spawn encoder worker thread.
    let encoder_config = config.clone();
    let encoder_pid = Arc::clone(&ffmpeg_child_pid);
    let encoder_handle =
        std::thread::spawn(move || pipeline::run_encoder(encoder_config, receiver, encoder_pid));

    // Metrics accumulators.
    let mut offer_latencies_us: Vec<u64> = Vec::with_capacity(total_frames as usize);
    let mut offer_outcomes: Vec<String> = Vec::with_capacity(total_frames as usize);
    let mut self_rss_samples: Vec<metrics::RssResult> = Vec::new();
    let mut ffmpeg_rss_samples: Vec<metrics::RssResult> = Vec::new();
    let mut windows: Vec<metrics::OfferWindow> = Vec::new();
    let mut current_window_offered: u64 = 0;
    let mut current_window_replaced: u64 = 0;
    let mut last_second: u64 = 0;
    let mut last_window_second: u64 = 0;

    // Producer loop.
    for frame_index in 0..total_frames {
        // Render deterministic frame outside the timed section.
        let frame_image = workload::render_frame(&config, frame_index);

        // Sleep until this frame's absolute deadline.
        let deadline = start + frame_interval * frame_index as u32;
        let now = Instant::now();
        if deadline > now {
            std::thread::sleep(deadline - now);
        }

        // Timed section: clone and offer.
        let offer_start = Instant::now();
        let timed = pipeline::TimedFrame {
            at_ms: frame_index * 1_000 / config.fps as u64,
            image: frame_image.clone(),
        };
        let result = sender.offer(timed);
        let offer_us = offer_start.elapsed().as_micros() as u64;

        offer_latencies_us.push(offer_us);
        offer_outcomes.push(format!("{result:?}"));

        // Track offer outcomes for window.
        current_window_offered += 1;
        match result {
            pipeline::OfferResult::ReplacedOldest | pipeline::OfferResult::Disconnected => {
                current_window_replaced += 1;
            }
            pipeline::OfferResult::Queued => {}
        }

        // Sample RSS on second boundaries.
        let current_second = frame_index / config.fps as u64;
        if current_second > last_second {
            self_rss_samples.push(sample_rss(std::process::id()));
            let ff_pid = ffmpeg_child_pid.load(Ordering::Acquire);
            if ff_pid != 0 {
                ffmpeg_rss_samples.push(sample_rss(ff_pid));
            }
            last_second = current_second;

            // Flush completed 5-second window.
            let window_second = current_second / 5;
            if window_second > last_window_second {
                windows.push(metrics::OfferWindow {
                    offered: current_window_offered,
                    replaced_or_dropped: current_window_replaced,
                });
                current_window_offered = 0;
                current_window_replaced = 0;
                last_window_second = window_second;
            }
        }
    }

    // Flush the final window.
    windows.push(metrics::OfferWindow {
        offered: current_window_offered,
        replaced_or_dropped: current_window_replaced,
    });

    // Disconnect sender; encoder will drain remaining and finish.
    drop(sender);

    // Wait for encoder.
    let encoder_result = encoder_handle.join().expect("encoder thread panicked");

    let summary = match encoder_result {
        Ok(s) => s,
        Err(e) => {
            let category = match &e {
                pipeline::PipelineError::Spawn { .. } => metrics::FailureCategory::Spawn,
                pipeline::PipelineError::Write { .. } => metrics::FailureCategory::Write,
                pipeline::PipelineError::Exit { .. } => metrics::FailureCategory::Exit,
                pipeline::PipelineError::Rename { .. } => metrics::FailureCategory::Rename,
            };
            write_failure_report(
                &config,
                &self_rss_samples,
                &ffmpeg_rss_samples,
                &offer_latencies_us,
                &offer_outcomes,
                &windows,
                category,
            );
            eprintln!("encoder error");
            std::process::exit(1);
        }
    };

    // Validate output exists.
    if !config.output.exists() {
        write_failure_report(
            &config,
            &self_rss_samples,
            &ffmpeg_rss_samples,
            &offer_latencies_us,
            &offer_outcomes,
            &windows,
            metrics::FailureCategory::Exit,
        );
        eprintln!("output file missing after encoding");
        std::process::exit(1);
    }

    // Probe the output.
    let probe = match metrics::run_probe(
        &config.ffprobe,
        &config.output,
        config.width,
        config.height,
        config.fps,
    ) {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("probe error: {e}");
            None
        }
    };

    // Build report.
    let mut final_report = metrics::RunReport {
        width: config.width,
        height: config.height,
        fps: config.fps,
        duration_secs: config.duration_secs,
        queue_capacity: config.queue_capacity,
        encoder_frames_written: summary.frames_written,
        encoder_exit_status: summary.ffmpeg_exit_status,
        offer_latencies_us: offer_latencies_us.clone(),
        offer_outcomes: offer_outcomes.clone(),
        windows: windows.clone(),
        self_rss: self_rss_samples.clone(),
        ffmpeg_rss: ffmpeg_rss_samples.clone(),
        probe: probe.clone(),
        memory_gate: metrics::MemoryGateStatus {
            test_result: metrics::TestResult::Pass,
            peak_to_trough_mib: 0,
            slope_mib_per_min: 0.0,
        },
        gate_decision: metrics::GateDecision {
            decision: metrics::Decision::Go,
            failed_gates: vec![],
        },
        failure_category: None,
        environment: metrics::gather_environment(&config.ffmpeg, &config.ffprobe),
    };

    // Evaluate gates.
    let gate_decision = metrics::evaluate(&mut final_report);

    // Build final report with gate decision.
    final_report.gate_decision = gate_decision.clone();

    // Write JSON report.
    match serde_json::to_string_pretty(&final_report) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&config.report, json) {
                eprintln!("warning: failed to write report: {e}");
            }
        }
        Err(e) => {
            eprintln!("warning: failed to serialize report: {e}");
        }
    }

    // Exit based on gate decision.
    match gate_decision.decision {
        metrics::Decision::Go => std::process::exit(0),
        metrics::Decision::NoGo => {
            eprintln!("NO-GO: failed gates: {:?}", gate_decision.failed_gates);
            std::process::exit(2);
        }
    }
}

/// Sample RSS for a PID. Returns Error result on failure.
fn sample_rss(pid: u32) -> metrics::RssResult {
    match metrics::rss_kib(pid) {
        Ok(kib) => metrics::RssResult::Ok { kib },
        Err(e) => metrics::RssResult::Error {
            reason: e.to_string(),
        },
    }
}

/// Write a failure report and exit with code 1.
fn write_failure_report(
    config: &RunConfig,
    self_rss: &[metrics::RssResult],
    ffmpeg_rss: &[metrics::RssResult],
    offer_latencies_us: &[u64],
    offer_outcomes: &[String],
    windows: &[metrics::OfferWindow],
    category: metrics::FailureCategory,
) {
    let report = metrics::RunReport {
        width: config.width,
        height: config.height,
        fps: config.fps,
        duration_secs: config.duration_secs,
        queue_capacity: config.queue_capacity,
        encoder_frames_written: 0,
        encoder_exit_status: -1,
        offer_latencies_us: offer_latencies_us.to_vec(),
        offer_outcomes: offer_outcomes.to_vec(),
        windows: windows.to_vec(),
        self_rss: self_rss.to_vec(),
        ffmpeg_rss: ffmpeg_rss.to_vec(),
        probe: None,
        memory_gate: metrics::MemoryGateStatus {
            test_result: metrics::TestResult::Untested,
            peak_to_trough_mib: 0,
            slope_mib_per_min: 0.0,
        },
        gate_decision: metrics::GateDecision {
            decision: metrics::Decision::NoGo,
            failed_gates: vec![
                metrics::Gate::Exit,
                metrics::Gate::Probe,
                metrics::Gate::Duration,
            ],
        },
        failure_category: Some(category),
        environment: metrics::gather_environment(&config.ffmpeg, &config.ffprobe),
    };
    if let Ok(json) = serde_json::to_string_pretty(&report) {
        let _ = std::fs::write(&config.report, json);
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
            "--ffmpeg",
            "/usr/bin/ffmpeg",
            "--ffprobe",
            "/usr/bin/ffprobe",
            "--output",
            "out.mp4",
            "--report",
            "report.json",
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
            "--ffmpeg",
            "ffmpeg",
            "--ffprobe",
            "ffprobe",
            "--output",
            "o.mp4",
            "--report",
            "r.json",
            "--width",
            "1280",
            "--height",
            "720",
            "--fps",
            "60",
            "--duration-secs",
            "120",
            "--queue-capacity",
            "4",
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
            "--ffmpeg",
            "ffmpeg",
            "--ffprobe",
            "ffprobe",
            "--output",
            "o.mp4",
            "--report",
            "r.json",
            "--bogus",
        ]))
        .unwrap_err();
        assert!(err.contains("unknown flag"));
    }

    #[test]
    fn zero_width_is_error() {
        assert!(
            parse_args(&args(&[
                "--ffmpeg",
                "ffmpeg",
                "--ffprobe",
                "ffprobe",
                "--output",
                "o.mp4",
                "--report",
                "r.json",
                "--width",
                "0",
            ]))
            .is_err()
        );
    }

    #[test]
    fn zero_height_is_error() {
        assert!(
            parse_args(&args(&[
                "--ffmpeg",
                "ffmpeg",
                "--ffprobe",
                "ffprobe",
                "--output",
                "o.mp4",
                "--report",
                "r.json",
                "--height",
                "0",
            ]))
            .is_err()
        );
    }

    #[test]
    fn zero_fps_is_error() {
        assert!(
            parse_args(&args(&[
                "--ffmpeg",
                "ffmpeg",
                "--ffprobe",
                "ffprobe",
                "--output",
                "o.mp4",
                "--report",
                "r.json",
                "--fps",
                "0",
            ]))
            .is_err()
        );
    }

    #[test]
    fn zero_duration_is_error() {
        assert!(
            parse_args(&args(&[
                "--ffmpeg",
                "ffmpeg",
                "--ffprobe",
                "ffprobe",
                "--output",
                "o.mp4",
                "--report",
                "r.json",
                "--duration-secs",
                "0",
            ]))
            .is_err()
        );
    }

    #[test]
    fn zero_queue_capacity_is_error() {
        assert!(
            parse_args(&args(&[
                "--ffmpeg",
                "ffmpeg",
                "--ffprobe",
                "ffprobe",
                "--output",
                "o.mp4",
                "--report",
                "r.json",
                "--queue-capacity",
                "0",
            ]))
            .is_err()
        );
    }

    #[test]
    fn odd_width_is_error() {
        let err = parse_args(&args(&[
            "--ffmpeg",
            "ffmpeg",
            "--ffprobe",
            "ffprobe",
            "--output",
            "o.mp4",
            "--report",
            "r.json",
            "--width",
            "1921",
        ]))
        .unwrap_err();
        assert!(err.contains("even"));
    }

    #[test]
    fn odd_height_is_error() {
        let err = parse_args(&args(&[
            "--ffmpeg",
            "ffmpeg",
            "--ffprobe",
            "ffprobe",
            "--output",
            "o.mp4",
            "--report",
            "r.json",
            "--height",
            "1081",
        ]))
        .unwrap_err();
        assert!(err.contains("even"));
    }

    #[test]
    fn missing_value_is_error() {
        assert!(
            parse_args(&args(&[
                "--ffmpeg",
                "ffmpeg",
                "--ffprobe",
                "ffprobe",
                "--output",
                "o.mp4",
                "--report",
            ]))
            .is_err()
        );
    }

    #[test]
    fn invalid_number_is_error() {
        let err = parse_args(&args(&[
            "--ffmpeg",
            "ffmpeg",
            "--ffprobe",
            "ffprobe",
            "--output",
            "o.mp4",
            "--report",
            "r.json",
            "--width",
            "abc",
        ]))
        .unwrap_err();
        assert!(err.contains("u32"));
    }
}
