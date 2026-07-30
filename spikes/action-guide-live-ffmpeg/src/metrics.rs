// ---------------------------------------------------------------------------
// Metrics collection, hard-gate evaluation, and probe validation
// ---------------------------------------------------------------------------

use serde::{Deserialize, Serialize};

// -- Probe structures (deserialize only the prescribed fields) ---------------

#[derive(Debug, Deserialize)]
pub(crate) struct ProbeStream {
    pub codec_name: Option<String>,
    pub codec_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub avg_frame_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProbeFormat {
    pub duration: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProbeOutput {
    pub streams: Vec<ProbeStream>,
    pub format: ProbeFormat,
}

// -- Probe result (derived, never raw probe output) -------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProbeResult {
    pub video_codec: String,
    pub video_fps: String,
    pub video_width: u32,
    pub video_height: u32,
    pub audio_stream_count: usize,
    pub format_duration_ms: u64,
}

// -- RSS sample result ------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub(crate) enum RssResult {
    Ok { kib: u64 },
    Error { reason: String },
}

impl RssResult {
    #[cfg(test)]
    pub(crate) fn test_result(&self) -> TestResult {
        match self {
            RssResult::Ok { .. } => TestResult::Pass,
            RssResult::Error { .. } => TestResult::Untested,
        }
    }
}

// -- Test result (for memory gate) ------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum TestResult {
    Pass,
    Fail,
    Untested,
}

// -- Memory gate status -----------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MemoryGateStatus {
    pub test_result: TestResult,
    pub peak_to_trough_mib: u64,
    pub slope_mib_per_min: f64,
}

// -- Offer window (5-second bucket) ----------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct OfferWindow {
    pub offered: u64,
    pub replaced_or_dropped: u64,
}

// -- Failure category -------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum FailureCategory {
    Spawn,
    Write,
    Exit,
    Rename,
}

// -- Gate enum --------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Gate {
    ProducerP99,
    QueueSaturation,
    MemoryBound,
    Duration,
    Exit,
    Probe,
}

// -- Gate decision ----------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Decision {
    Go,
    NoGo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GateDecision {
    pub decision: Decision,
    pub failed_gates: Vec<Gate>,
}

// -- Environment info -------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EnvironmentInfo {
    pub os_info: String,
    pub rustc_version: String,
    pub cargo_version: String,
    pub ffmpeg_version: String,
    pub ffprobe_version: String,
}

// -- Run report -------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RunReport {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_secs: u64,
    pub queue_capacity: usize,
    pub encoder_frames_written: u64,
    pub encoder_exit_status: i32,
    pub offer_latencies_us: Vec<u64>,
    pub offer_outcomes: Vec<String>,
    pub windows: Vec<OfferWindow>,
    pub self_rss: Vec<RssResult>,
    pub ffmpeg_rss: Vec<RssResult>,
    pub probe: Option<ProbeResult>,
    pub memory_gate: MemoryGateStatus,
    pub gate_decision: GateDecision,
    pub failure_category: Option<FailureCategory>,
    pub environment: EnvironmentInfo,
}

// -- RSS helper -------------------------------------------------------------

/// Read RSS in KiB for the given PID via `ps`.
pub(crate) fn rss_kib(pid: u32) -> std::io::Result<u64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()?;
    let text = std::str::from_utf8(&output.stdout).map_err(std::io::Error::other)?;
    text.trim().parse::<u64>().map_err(std::io::Error::other)
}

// -- Environment gathering --------------------------------------------------

pub(crate) fn gather_environment(
    ffmpeg: &std::path::Path,
    ffprobe: &std::path::Path,
) -> EnvironmentInfo {
    EnvironmentInfo {
        os_info: cmd_first_line("uname", &["-a"]),
        rustc_version: cmd_first_line("rustc", &["--version"]),
        cargo_version: cmd_first_line("cargo", &["--version"]),
        ffmpeg_version: version_first_line(ffmpeg),
        ffprobe_version: version_first_line(ffprobe),
    }
}

fn cmd_first_line(cmd: &str, args: &[&str]) -> String {
    std::process::Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8(o.stdout)
                .ok()
                .and_then(|s| s.lines().next().map(str::to_owned))
        })
        .unwrap_or_default()
}

fn version_first_line(bin: &std::path::Path) -> String {
    std::process::Command::new(bin)
        .arg("-version")
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8(o.stdout)
                .ok()
                .and_then(|s| s.lines().next().map(str::to_owned))
        })
        .unwrap_or_default()
}

// -- Probe validation -------------------------------------------------------

pub(crate) fn run_probe(
    ffprobe: &std::path::Path,
    output: &std::path::Path,
    width: u32,
    height: u32,
    fps: u32,
) -> Result<ProbeResult, String> {
    if !output.exists() {
        return Err("output file does not exist".into());
    }

    let probe_json = std::process::Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,codec_type,width,height,avg_frame_rate",
            "-show_entries",
            "format=duration",
            "-of",
            "json",
        ])
        .arg(output)
        .output()
        .map_err(|e| format!("failed to run probe: {e}"))?;

    if !probe_json.status.success() {
        return Err("probe exited with non-zero status".into());
    }

    let probe: ProbeOutput = serde_json::from_slice(&probe_json.stdout)
        .map_err(|e| format!("probe parse error: {e}"))?;

    // Exactly one video stream
    let video_streams: Vec<_> = probe
        .streams
        .iter()
        .filter(|s| s.codec_type.as_deref() == Some("video"))
        .collect();
    if video_streams.len() != 1 {
        return Err(format!(
            "expected exactly 1 video stream, found {}",
            video_streams.len()
        ));
    }
    let vs = video_streams[0];

    // H.264 codec
    let codec = vs
        .codec_name
        .as_deref()
        .ok_or("video stream missing codec_name")?;
    if codec != "h264" {
        return Err(format!("expected h264 codec, got {codec}"));
    }

    // Expected dimensions
    let vw = vs.width.ok_or("video stream missing width")?;
    let vh = vs.height.ok_or("video stream missing height")?;
    if vw != width || vh != height {
        return Err(format!("expected {width}x{height}, got {vw}x{vh}"));
    }

    // Average frame rate
    let rate = vs
        .avg_frame_rate
        .as_deref()
        .ok_or("video stream missing avg_frame_rate")?;
    let expected_rate = format!("{fps}/1");
    if rate != expected_rate {
        return Err(format!("expected {expected_rate} fps, got {rate}"));
    }

    // No audio streams
    let audio_count = probe
        .streams
        .iter()
        .filter(|s| s.codec_type.as_deref() == Some("audio"))
        .count();

    // Duration
    let dur_str = probe
        .format
        .duration
        .as_deref()
        .ok_or("format missing duration")?;
    let dur_secs: f64 = dur_str
        .parse()
        .map_err(|_| format!("invalid duration: {dur_str}"))?;
    let duration_ms = (dur_secs * 1000.0).round() as u64;

    Ok(ProbeResult {
        video_codec: codec.to_owned(),
        video_fps: rate.to_owned(),
        video_width: vw,
        video_height: vh,
        audio_stream_count: audio_count,
        format_duration_ms: duration_ms,
    })
}

// -- Gate evaluation --------------------------------------------------------

pub(crate) fn evaluate(report: &mut RunReport) -> GateDecision {
    let mut failed_gates: Vec<Gate> = Vec::new();

    // Gate 1: Producer p99 clone+offer <= 1_000 µs
    if !report.offer_latencies_us.is_empty() {
        let mut sorted = report.offer_latencies_us.clone();
        sorted.sort_unstable();
        let p99_idx = ((sorted.len() as f64 - 1.0) * 0.99).ceil() as usize;
        let p99 = sorted[p99_idx.min(sorted.len() - 1)];
        if p99 > 1_000 {
            failed_gates.push(Gate::ProducerP99);
        }
    }

    // Gate 2: Zero 5-second windows with dropped/replaced ratio > 10%
    let has_saturation = report
        .windows
        .iter()
        .any(|w| w.offered > 0 && (w.replaced_or_dropped as f64 / w.offered as f64) > 0.10);
    if has_saturation {
        failed_gates.push(Gate::QueueSaturation);
    }

    // Gate 3: Post-warm-up self RSS
    // warm-up = first 60 seconds of samples (1 sample per second)
    //   peak-to-trough <= 64 MiB
    //   least-squares slope <= 1 MiB/min
    let warmup_count = 60usize; // first warm-up minute (60 samples at 1/s)
    let post_warmup: Vec<(usize, u64)> = report
        .self_rss
        .iter()
        .enumerate()
        .skip(warmup_count)
        .filter_map(|(i, r)| match r {
            RssResult::Ok { kib } => Some((i, *kib)),
            _ => None,
        })
        .collect();

    if post_warmup.is_empty() {
        // No post-warm-up data → memory gate is UNTESTED → fail
        report.memory_gate = MemoryGateStatus {
            test_result: TestResult::Untested,
            peak_to_trough_mib: 0,
            slope_mib_per_min: 0.0,
        };
        failed_gates.push(Gate::MemoryBound);
    } else {
        let peak = post_warmup.iter().map(|(_, k)| *k).max().unwrap();
        let trough = post_warmup.iter().map(|(_, k)| *k).min().unwrap();
        let delta_mib = (peak - trough) / 1024;

        // Least-squares slope: y = a + b*x
        // x = sample index (seconds), y = RSS in MiB
        let n = post_warmup.len() as f64;
        let sum_x: f64 = post_warmup.iter().map(|(i, _)| *i as f64).sum();
        let sum_y: f64 = post_warmup.iter().map(|(_, k)| *k as f64 / 1024.0).sum();
        let sum_xx: f64 = post_warmup
            .iter()
            .map(|(i, _)| (*i as f64) * (*i as f64))
            .sum();
        let sum_xy: f64 = post_warmup
            .iter()
            .map(|(i, k)| (*i as f64) * (*k as f64 / 1024.0))
            .sum();
        let denom = n * sum_xx - sum_x * sum_x;
        let slope_per_sec = if denom.abs() > 1e-12 {
            (n * sum_xy - sum_x * sum_y) / denom
        } else {
            0.0
        };
        let slope_per_min = slope_per_sec * 60.0;

        let test_result = if delta_mib > 64 || slope_per_min > 1.0 {
            TestResult::Fail
        } else {
            TestResult::Pass
        };

        report.memory_gate = MemoryGateStatus {
            test_result,
            peak_to_trough_mib: delta_mib,
            slope_mib_per_min: slope_per_min,
        };

        if test_result == TestResult::Fail {
            failed_gates.push(Gate::MemoryBound);
        }
    }

    // Gate 4: Duration delta <= 34 ms
    if let Some(probe) = &report.probe {
        let expected_ms = report.duration_secs * 1_000;
        let delta = (probe.format_duration_ms as i64 - expected_ms as i64).unsigned_abs();
        if delta > 34 {
            failed_gates.push(Gate::Duration);
        }
    } else {
        // No probe data → duration gate fails
        failed_gates.push(Gate::Duration);
    }

    // Gate 5: FFmpeg exited successfully and atomic output exists
    if report.encoder_exit_status != 0 {
        failed_gates.push(Gate::Exit);
    }

    // Gate 6: Probe reports H.264, expected fps, expected dimensions, no audio
    if let Some(probe) = &report.probe {
        let expected_fps = format!("{}/1", report.fps);
        if probe.video_codec != "h264"
            || probe.video_fps != expected_fps
            || probe.video_width != report.width
            || probe.video_height != report.height
            || probe.audio_stream_count != 0
        {
            failed_gates.push(Gate::Probe);
        }
    } else {
        failed_gates.push(Gate::Probe);
    }

    let decision = if failed_gates.is_empty() {
        Decision::Go
    } else {
        Decision::NoGo
    };

    GateDecision {
        decision,
        failed_gates,
    }
}

// -- Tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Base fixture with all gates passing.
    fn fixture_report() -> RunReport {
        RunReport {
            width: 1920,
            height: 1080,
            fps: 30,
            duration_secs: 5,
            queue_capacity: 2,
            encoder_frames_written: 150,
            encoder_exit_status: 0,
            offer_latencies_us: vec![100; 100],
            offer_outcomes: vec!["Queued".into(); 100],
            windows: vec![OfferWindow {
                offered: 30,
                replaced_or_dropped: 0,
            }],
            self_rss: std::iter::repeat_with(|| RssResult::Ok { kib: 100 * 1024 })
                .take(62)
                .collect(),
            ffmpeg_rss: vec![RssResult::Ok { kib: 50 * 1024 }; 40],
            probe: Some(ProbeResult {
                video_codec: "h264".into(),
                video_fps: "30/1".into(),
                video_width: 1920,
                video_height: 1080,
                audio_stream_count: 0,
                format_duration_ms: 5_000,
            }),
            memory_gate: MemoryGateStatus {
                test_result: TestResult::Pass,
                peak_to_trough_mib: 0,
                slope_mib_per_min: 0.0,
            },
            gate_decision: GateDecision {
                decision: Decision::Go,
                failed_gates: vec![],
            },
            failure_category: None,
            environment: EnvironmentInfo {
                os_info: "test".into(),
                rustc_version: "test".into(),
                cargo_version: "test".into(),
                ffmpeg_version: "test".into(),
                ffprobe_version: "test".into(),
            },
        }
    }

    // -- Builder helpers for fixture_report ---------------------------------

    trait ReportBuilder {
        fn with_offer_latencies_us(self, us: Vec<u64>) -> Self;
        fn with_window(self, window: OfferWindow) -> Self;
        fn with_post_warmup_rss_mib(self, mib: Vec<u64>) -> Self;
        fn with_durations(self, source_ms: u64, encoded_ms: u64) -> Self;
        fn with_encoder_exit(self, status: i32) -> Self;
        fn with_no_probe(self) -> Self;
        fn with_probe(self, probe: ProbeResult) -> Self;
    }

    impl ReportBuilder for RunReport {
        fn with_offer_latencies_us(mut self, us: Vec<u64>) -> Self {
            self.offer_latencies_us = us;
            self
        }

        fn with_window(mut self, window: OfferWindow) -> Self {
            self.windows = vec![window];
            self
        }

        fn with_post_warmup_rss_mib(mut self, mib: Vec<u64>) -> Self {
            // Prepend 60 warm-up samples (1 second each = first warm-up minute)
            let mut samples: Vec<RssResult> = vec![RssResult::Ok { kib: 100 * 1024 }; 60];
            samples.extend(mib.into_iter().map(|m| RssResult::Ok { kib: m * 1024 }));
            self.self_rss = samples;
            self
        }

        fn with_durations(mut self, source_ms: u64, encoded_ms: u64) -> Self {
            self.duration_secs = source_ms / 1000;
            self.encoder_frames_written = (encoded_ms * self.fps as u64) / 1000;
            self.probe = Some(ProbeResult {
                video_codec: "h264".into(),
                video_fps: format!("{}/1", self.fps),
                video_width: self.width,
                video_height: self.height,
                audio_stream_count: 0,
                format_duration_ms: encoded_ms,
            });
            // Store source_ms in offer_outcomes for reference (not a gate input)
            self.offer_outcomes = vec![format!("source_ms={source_ms}")];
            self
        }

        fn with_encoder_exit(mut self, status: i32) -> Self {
            self.encoder_exit_status = status;
            self
        }

        fn with_no_probe(mut self) -> Self {
            self.probe = None;
            self
        }

        fn with_probe(mut self, probe: ProbeResult) -> Self {
            self.probe = Some(probe);
            self
        }
    }

    // -- Gate tests ---------------------------------------------------------

    #[test]
    fn gate_rejects_slow_producer_p99() {
        let mut report = fixture_report()
            .with_offer_latencies_us(vec![200; 99].into_iter().chain([1_100]).collect());
        let decision = evaluate(&mut report);
        assert_eq!(decision.decision, Decision::NoGo);
        assert!(decision.failed_gates.contains(&Gate::ProducerP99));
    }

    #[test]
    fn gate_rejects_persistent_five_second_saturation() {
        let mut report = fixture_report().with_window(OfferWindow {
            offered: 150,
            replaced_or_dropped: 16,
        });
        let decision = evaluate(&mut report);
        assert!(decision.failed_gates.contains(&Gate::QueueSaturation));
    }

    #[test]
    fn gate_rejects_unbounded_memory_growth() {
        let mut report = fixture_report().with_post_warmup_rss_mib(vec![100, 120, 145, 170]);
        let decision = evaluate(&mut report);
        assert!(decision.failed_gates.contains(&Gate::MemoryBound));
    }

    #[test]
    fn gate_accepts_duration_difference_of_one_frame() {
        let mut report = fixture_report().with_durations(10_000, 10_034);
        let decision = evaluate(&mut report);
        assert!(!decision.failed_gates.contains(&Gate::Duration));
    }

    #[test]
    fn gate_accepts_good_report() {
        let mut report = fixture_report();
        let decision = evaluate(&mut report);
        assert_eq!(decision.decision, Decision::Go);
        assert!(decision.failed_gates.is_empty());
    }

    #[test]
    fn gate_rejects_ffmpeg_nonzero_exit() {
        let mut report = fixture_report().with_encoder_exit(1);
        let decision = evaluate(&mut report);
        assert!(decision.failed_gates.contains(&Gate::Exit));
    }

    #[test]
    fn gate_rejects_missing_probe() {
        let mut report = fixture_report().with_no_probe();
        let decision = evaluate(&mut report);
        assert!(decision.failed_gates.contains(&Gate::Probe));
        assert!(decision.failed_gates.contains(&Gate::Duration));
    }

    #[test]
    fn gate_rejects_wrong_codec() {
        let mut report = fixture_report().with_probe(ProbeResult {
            video_codec: "vp9".into(),
            video_fps: "30/1".into(),
            video_width: 1920,
            video_height: 1080,
            audio_stream_count: 0,
            format_duration_ms: 5_000,
        });
        let decision = evaluate(&mut report);
        assert!(decision.failed_gates.contains(&Gate::Probe));
    }

    #[test]
    fn gate_rejects_audio_stream_present() {
        let mut report = fixture_report().with_probe(ProbeResult {
            video_codec: "h264".into(),
            video_fps: "30/1".into(),
            video_width: 1920,
            video_height: 1080,
            audio_stream_count: 1,
            format_duration_ms: 5_000,
        });
        let decision = evaluate(&mut report);
        assert!(decision.failed_gates.contains(&Gate::Probe));
    }

    #[test]
    fn gate_rejects_wrong_dimensions() {
        let mut report = fixture_report().with_probe(ProbeResult {
            video_codec: "h264".into(),
            video_fps: "30/1".into(),
            video_width: 1280,
            video_height: 720,
            audio_stream_count: 0,
            format_duration_ms: 5_000,
        });
        let decision = evaluate(&mut report);
        assert!(decision.failed_gates.contains(&Gate::Probe));
    }

    #[test]
    fn gate_rejects_wrong_frame_rate() {
        let mut report = fixture_report().with_probe(ProbeResult {
            video_codec: "h264".into(),
            video_fps: "60/1".into(),
            video_width: 1920,
            video_height: 1080,
            audio_stream_count: 0,
            format_duration_ms: 5_000,
        });
        let decision = evaluate(&mut report);
        assert!(decision.failed_gates.contains(&Gate::Probe));
    }

    #[test]
    fn gate_rejects_duration_delta_over_34ms() {
        let mut report = fixture_report().with_durations(10_000, 10_035);
        let decision = evaluate(&mut report);
        assert!(decision.failed_gates.contains(&Gate::Duration));
    }

    #[test]
    fn untested_memory_gate_yields_nogo() {
        let mut report = fixture_report();
        report.self_rss = vec![
            RssResult::Error {
                reason: "test".into()
            };
            (report.fps as usize) + 2
        ];
        let decision = evaluate(&mut report);
        assert!(decision.failed_gates.contains(&Gate::MemoryBound));
    }

    #[test]
    fn gate_accepts_flat_memory_profile() {
        let mut report = fixture_report().with_post_warmup_rss_mib(vec![100, 100, 100, 100]);
        let decision = evaluate(&mut report);
        assert!(!decision.failed_gates.contains(&Gate::MemoryBound));
    }

    #[test]
    fn gate_rejects_slope_over_one_mib_per_min() {
        // 5 post-warm-up samples, increasing by ~30 MiB each → slope > 1 MiB/min
        let mut report = fixture_report().with_post_warmup_rss_mib(vec![100, 130, 160, 190, 220]);
        let decision = evaluate(&mut report);
        assert!(decision.failed_gates.contains(&Gate::MemoryBound));
    }

    // -- RssResult tests ----------------------------------------------------

    #[test]
    fn rss_ok_maps_to_pass() {
        assert_eq!(RssResult::Ok { kib: 1024 }.test_result(), TestResult::Pass);
    }

    #[test]
    fn rss_error_maps_to_untested() {
        assert_eq!(
            RssResult::Error {
                reason: "no such process".into()
            }
            .test_result(),
            TestResult::Untested
        );
    }

    // -- Probe parser tests -------------------------------------------------

    #[test]
    fn probe_parses_valid_h264_output() {
        let json = serde_json::json!({
            "streams": [{
                "codec_name": "h264",
                "codec_type": "video",
                "width": 1920,
                "height": 1080,
                "avg_frame_rate": "30/1"
            }],
            "format": {
                "duration": "5.033333"
            }
        });
        let probe: ProbeOutput = serde_json::from_value(json).unwrap();
        assert_eq!(probe.streams.len(), 1);
        assert_eq!(probe.streams[0].codec_name.as_deref(), Some("h264"));
        assert_eq!(probe.streams[0].width, Some(1920));
    }

    #[test]
    fn probe_parses_format_without_duration() {
        let json = serde_json::json!({
            "streams": [],
            "format": {}
        });
        let probe: ProbeOutput = serde_json::from_value(json).unwrap();
        assert!(probe.format.duration.is_none());
    }

    // -- Report serialization test ------------------------------------------

    #[test]
    fn report_json_never_contains_file_paths() {
        let report = fixture_report();
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("/usr/bin"));
        assert!(!json.contains(".mp4"));
        assert!(!json.contains("pipe:0"));
    }

    #[test]
    fn report_json_never_contains_ffmpeg_command() {
        let report = fixture_report();
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("-c:v"));
        assert!(!json.contains("libx264"));
        assert!(!json.contains("rawvideo"));
    }
}
