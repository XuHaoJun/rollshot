//! `rollshot capture` — drives a CaptureBackend, stitches its frames, writes a
//! PNG.
//!
//! Pipeline (async stitch):
//!
//! ```text
//!   args (clap)
//!     │
//!     ▼
//!   backend.start(options) ──► Box<dyn FrameStream>
//!     │
//!     ├── reader thread: stream.next_frame() → FrameSlot (latest-wins)
//!     │
//!     └── main thread (stitch loop):
//!           slot.take_blocking() → dump → stitcher.push_frame()
//!           repeat until EndOfStream / --max-frames
//!     │
//!     ▼
//!   stitcher.full_image() → save PNG → summary
//! ```

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use image::ImageFormat;
use rollshot_capture::{
    BackendKind, CaptureBackend, CaptureError, CaptureOptions, FixtureBackend, Region, RegionMode,
};
use rollshot_core::{MotionEstimate, OverlapRegion, StitchConfig, StitchOutcome, Stitcher};
use serde::Serialize;

use crate::args::CaptureArgs;
use crate::cli_error::CliError;

#[derive(Debug, Serialize)]
struct CaptureMatchReport {
    frames: Vec<CaptureFrameReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<CaptureSummary>,
}

#[derive(Debug, Serialize)]
struct CaptureSummary {
    capture_interval_p50_ms: f64,
    capture_interval_p90_ms: f64,
    capture_interval_max_ms: f64,
    max_accepted_dy: u32,
    longest_no_match_run: u32,
    frames_under_20ms: usize,
    total_frames: usize,
    appended: u32,
    duplicates: u32,
    no_match: u32,
    no_progress: u32,
    frames_read: u32,
}

#[derive(Debug, Serialize)]
struct CaptureFrameReport {
    frame_index: usize,
    outcome: String,
    reason: Option<String>,
    estimate: Option<EstimateReport>,
    capture_interval_ms: Option<f64>,
    stitch_elapsed_ms: f64,
}

#[derive(Debug, Serialize)]
struct EstimateReport {
    dx: i32,
    dy: i32,
    direction: String,
    method: String,
    confidence: f32,
    inliers: Option<usize>,
    raw_matches: Option<usize>,
    overlap: OverlapReport,
}

#[derive(Debug, Serialize)]
struct OverlapReport {
    prev_x: u32,
    prev_y: u32,
    curr_x: u32,
    curr_y: u32,
    width: u32,
    height: u32,
}

pub fn run(args: &CaptureArgs) -> Result<String, CliError> {
    if args.headless {
        run_headless(args)
    } else {
        crate::cmd_capture_launcher::run(args)
    }
}

fn run_headless(args: &CaptureArgs) -> Result<String, CliError> {
    let output = args
        .output
        .as_ref()
        .ok_or_else(|| CliError::new("--output is required with --headless", 1))?;
    let kind = BackendKind::from_cli_flag(&args.backend).map_err(CliError::from_capture)?;
    let mut backend = build_backend(kind, args)?;
    let region = parse_region(&args.region, kind)?;
    let options = CaptureOptions {
        region,
        fps: args.fps,
        show_cursor: args.show_cursor,
        prefer_portal_region: true,
        target_display_id: None,
    };

    let mut stream = backend.start(options).map_err(CliError::from_capture)?;

    if let Some(dir) = args.dump_frames.as_ref() {
        std::fs::create_dir_all(dir).map_err(|err| {
            CliError::new(
                format!("failed to create dump dir {}: {err}", dir.display()),
                1,
            )
        })?;
    }

    let mut config = StitchConfig::default();
    if args.disable_feature_fallback {
        config.fast_hnsw.enabled = false;
    }

    let slot = Arc::new(crate::frame_slot::FrameSlot::new());
    let slot_stitch = Arc::clone(&slot);
    let stitch_done = Arc::new(AtomicBool::new(false));
    let stitch_done_reader = Arc::clone(&stitch_done);

    let max_frames = args.max_frames;
    let quiet = args.quiet;
    let dump_frames: Option<PathBuf> = args.dump_frames.clone();

    let stitch_done_flag = Arc::clone(&stitch_done);
    let stitch_handle = std::thread::spawn(move || {
        let result = stitch_loop(
            slot_stitch,
            config,
            max_frames,
            quiet,
            dump_frames.as_deref(),
        );
        stitch_done_flag.store(true, Ordering::Relaxed);
        result
    });

    loop {
        if stitch_done_reader.load(Ordering::Relaxed) {
            break;
        }
        match stream.next_frame() {
            Ok(frame) => slot.store(frame),
            Err(CaptureError::EndOfStream) => {
                slot.signal_end();
                break;
            }
            Err(CaptureError::Timeout { .. }) => continue,
            Err(err) => {
                slot.signal_error(format!("{err}"));
                break;
            }
        }
    }

    let stitch_result = stitch_handle
        .join()
        .map_err(|_| CliError::new("stitch thread panicked", 1))?;
    let (mut stitcher, mut report, captured, appended, duplicates, no_match, no_progress) =
        stitch_result?;
    let frames_read = slot.total_produced();

    let summary = compute_summary(
        &report,
        appended,
        duplicates,
        no_match,
        no_progress,
        frames_read,
    );
    print_diagnostics_summary(&summary, args.quiet);
    report.summary = Some(summary);

    let stitched = stitcher
        .full_image()
        .ok_or_else(|| CliError::new("no frames produced an output image", 1))?;
    save_png(stitched, output)?;

    if let Some(path) = args.debug_match_report.as_ref() {
        write_report(path, &report)?;
    }

    Ok(format!(
        "captured {captured} frames, appended {appended} \
         (duplicates {duplicates}, no-progress {no_progress}, \
         no-match {no_match}, frames-read {frames_read})\n\
         output: {out} ({w}x{h})\n",
        out = output.display(),
        w = stitched.width(),
        h = stitched.height(),
    ))
}

type StitchLoopResult = Result<(Stitcher, CaptureMatchReport, u32, u32, u32, u32, u32), CliError>;

fn stitch_loop(
    slot: Arc<crate::frame_slot::FrameSlot>,
    config: StitchConfig,
    max_frames: u32,
    quiet: bool,
    dump_frames: Option<&Path>,
) -> StitchLoopResult {
    let mut stitcher = Stitcher::new(config);
    let mut captured: u32 = 0;
    let mut appended: u32 = 0;
    let mut duplicates: u32 = 0;
    let mut no_match: u32 = 0;
    let mut no_progress: u32 = 0;
    let mut report = CaptureMatchReport {
        frames: Vec::new(),
        summary: None,
    };
    let mut previous_capture_timestamp = None;

    loop {
        match slot.take_blocking(Duration::from_secs(5)) {
            Ok(frame) => {
                let capture_interval_ms = previous_capture_timestamp.and_then(|previous| {
                    frame
                        .timestamp
                        .duration_since(previous)
                        .ok()
                        .map(duration_ms)
                });
                previous_capture_timestamp = Some(frame.timestamp);

                if let Some(dir) = dump_frames {
                    write_dump_frame(dir, captured, &frame.image)?;
                }
                captured += 1;
                if !quiet {
                    log_capture_start(captured, max_frames);
                }
                let stitch_started = Instant::now();
                let outcome = stitcher.push_frame(frame.image);
                let stitch_elapsed = stitch_started.elapsed();
                let mut frame_report = CaptureFrameReport {
                    frame_index: report.frames.len(),
                    outcome: String::new(),
                    reason: None,
                    estimate: None,
                    capture_interval_ms,
                    stitch_elapsed_ms: duration_ms(stitch_elapsed),
                };
                match &outcome {
                    StitchOutcome::FirstFrame => {
                        frame_report.outcome = "FirstFrame".to_string();
                    }
                    StitchOutcome::Appended { estimate, .. } => {
                        appended += 1;
                        frame_report.outcome = "Appended".to_string();
                        frame_report.estimate = Some(estimate_report(estimate));
                    }
                    StitchOutcome::Duplicate => {
                        duplicates += 1;
                        frame_report.outcome = "Duplicate".to_string();
                    }
                    StitchOutcome::NoMatch {
                        reason,
                        best_estimate,
                    } => {
                        no_match += 1;
                        frame_report.outcome = "NoMatch".to_string();
                        frame_report.reason = Some(format!("{reason:?}"));
                        frame_report.estimate = best_estimate.as_ref().map(estimate_report);
                    }
                    StitchOutcome::NoProgress { estimate } => {
                        no_progress += 1;
                        frame_report.outcome = "NoProgress".to_string();
                        frame_report.estimate = estimate.as_ref().map(estimate_report);
                    }
                    StitchOutcome::AxisChanged { estimate, .. } => {
                        no_match += 1;
                        frame_report.outcome = "AxisChanged".to_string();
                        frame_report.estimate = Some(estimate_report(estimate));
                    }
                }
                let contributes = matches!(
                    outcome,
                    StitchOutcome::FirstFrame | StitchOutcome::Appended { .. }
                );
                report.frames.push(frame_report);
                if !quiet {
                    log_capture_progress(captured, max_frames, &outcome, stitch_elapsed);
                }
                if contributes && (appended + 1) >= max_frames {
                    break;
                }
            }
            Err(CaptureError::EndOfStream) => break,
            Err(err) => return Err(CliError::from_capture(err)),
        }
    }

    Ok((
        stitcher,
        report,
        captured,
        appended,
        duplicates,
        no_match,
        no_progress,
    ))
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn build_backend(
    kind: BackendKind,
    args: &CaptureArgs,
) -> Result<Box<dyn CaptureBackend>, CliError> {
    match kind {
        BackendKind::Fixture => {
            let dir = args
                .fixture
                .as_ref()
                .ok_or_else(|| CliError::new("--backend fixture requires --fixture <DIR>", 1))?;
            Ok(Box::new(FixtureBackend::new(dir.clone())))
        }
        other => other.create().map_err(CliError::from_capture),
    }
}

fn save_png(image: &image::RgbaImage, path: &Path) -> Result<(), CliError> {
    image
        .save_with_format(path, ImageFormat::Png)
        .map_err(|err| CliError::new(format!("failed to save {}: {err}", path.display()), 1))
}

fn write_dump_frame(dir: &Path, index: u32, image: &image::RgbaImage) -> Result<(), CliError> {
    let path = dir.join(format!("frame_{index:04}.png"));
    image
        .save_with_format(&path, ImageFormat::Png)
        .map_err(|err| CliError::new(format!("failed to save {}: {err}", path.display()), 1))?;
    Ok(())
}

fn write_report(path: &Path, report: &CaptureMatchReport) -> Result<(), CliError> {
    let json = serde_json::to_string_pretty(report)
        .map_err(|err| CliError::new(format!("failed to encode match report: {err}"), 1))?;
    std::fs::write(path, json)
        .map_err(|err| CliError::new(format!("failed to write {}: {err}", path.display()), 1))
}

fn estimate_report(estimate: &MotionEstimate) -> EstimateReport {
    EstimateReport {
        dx: estimate.dx,
        dy: estimate.dy,
        direction: format!("{:?}", estimate.direction),
        method: format!("{:?}", estimate.method),
        confidence: estimate.confidence,
        inliers: estimate.inliers,
        raw_matches: estimate.raw_matches,
        overlap: overlap_report(estimate.overlap),
    }
}

fn overlap_report(overlap: OverlapRegion) -> OverlapReport {
    OverlapReport {
        prev_x: overlap.prev_x,
        prev_y: overlap.prev_y,
        curr_x: overlap.curr_x,
        curr_y: overlap.curr_y,
        width: overlap.width,
        height: overlap.height,
    }
}

fn log_capture_start(index: u32, max_frames: u32) {
    eprintln!("frame {index}/{max_frames}: stitching...");
}

fn log_capture_progress(index: u32, max_frames: u32, outcome: &StitchOutcome, elapsed: Duration) {
    eprintln!(
        "frame {}/{}: {} elapsed={:.3}s",
        index,
        max_frames,
        outcome_label(outcome),
        elapsed.as_secs_f64()
    );
}

fn outcome_label(outcome: &StitchOutcome) -> &'static str {
    match outcome {
        StitchOutcome::FirstFrame => "FirstFrame",
        StitchOutcome::Appended { .. } => "Appended",
        StitchOutcome::Duplicate => "Duplicate",
        StitchOutcome::NoMatch { .. } => "NoMatch",
        StitchOutcome::NoProgress { .. } => "NoProgress",
        StitchOutcome::AxisChanged { .. } => "AxisChanged",
    }
}

fn compute_summary(
    report: &CaptureMatchReport,
    appended: u32,
    duplicates: u32,
    no_match: u32,
    no_progress: u32,
    frames_read: u32,
) -> CaptureSummary {
    let mut intervals: Vec<f64> = report
        .frames
        .iter()
        .filter_map(|f| f.capture_interval_ms)
        .collect();
    intervals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let percentile = |sorted: &[f64], p: f64| -> f64 {
        if sorted.is_empty() {
            return 0.0;
        }
        let idx = (p / 100.0 * (sorted.len() as f64 - 1.0)).round() as usize;
        sorted[idx.min(sorted.len() - 1)]
    };

    let max_accepted_dy = report
        .frames
        .iter()
        .filter(|f| f.outcome == "Appended")
        .filter_map(|f| f.estimate.as_ref())
        .map(|e| e.dy.unsigned_abs())
        .max()
        .unwrap_or(0);

    let mut longest_no_match_run: u32 = 0;
    let mut current_run: u32 = 0;
    for f in &report.frames {
        if f.outcome == "NoMatch" {
            current_run += 1;
            longest_no_match_run = longest_no_match_run.max(current_run);
        } else {
            current_run = 0;
        }
    }

    let frames_under_20ms = intervals.iter().filter(|&&v| v < 20.0).count();

    CaptureSummary {
        capture_interval_p50_ms: percentile(&intervals, 50.0),
        capture_interval_p90_ms: percentile(&intervals, 90.0),
        capture_interval_max_ms: intervals.last().copied().unwrap_or(0.0),
        max_accepted_dy,
        longest_no_match_run,
        frames_under_20ms,
        total_frames: report.frames.len(),
        appended,
        duplicates,
        no_match,
        no_progress,
        frames_read,
    }
}

fn print_diagnostics_summary(summary: &CaptureSummary, quiet: bool) {
    if quiet {
        return;
    }
    eprintln!(
        "--- capture diagnostics ---\n\
         capture_interval_ms: p50={:.1} p90={:.1} max={:.1}\n\
         max_accepted_dy: {}\n\
         longest_no_match_run: {}",
        summary.capture_interval_p50_ms,
        summary.capture_interval_p90_ms,
        summary.capture_interval_max_ms,
        summary.max_accepted_dy,
        summary.longest_no_match_run,
    );
    if summary.frames_read > summary.total_frames as u32 {
        eprintln!(
            "frames_read: {} (stitched {})",
            summary.frames_read, summary.total_frames
        );
    }
    if summary.longest_no_match_run >= 5 {
        eprintln!(
            "warning: {} consecutive NoMatch frames — \
             scroll may be too fast for the capture cadence",
            summary.longest_no_match_run
        );
    }
}

fn parse_region(flag: &str, kind: BackendKind) -> Result<RegionMode, CliError> {
    match flag {
        "auto" => Ok(match kind {
            BackendKind::LinuxPortalPipeWire => RegionMode::PortalPicker,
            BackendKind::MacosScreenCaptureKit
            | BackendKind::Fixture
            | BackendKind::Unsupported => RegionMode::FullSource,
        }),
        "portal" => match kind {
            BackendKind::LinuxPortalPipeWire => Ok(RegionMode::PortalPicker),
            _ => Err(CliError::new(
                "--region portal is only supported with --backend linux-portal",
                1,
            )),
        },
        "full" => Ok(RegionMode::FullSource),
        other => parse_manual_region(other).map(RegionMode::Manual),
    }
}

fn parse_manual_region(s: &str) -> Result<Region, CliError> {
    let invalid = || {
        CliError::new(
            format!("invalid --region '{s}'; expected auto|portal|full|\"X,Y WxH\""),
            1,
        )
    };

    let mut parts = s.split_whitespace();
    let origin = parts.next().ok_or_else(invalid)?;
    let size = parts.next().ok_or_else(invalid)?;
    if parts.next().is_some() {
        return Err(invalid());
    }

    let (x, y) = origin.split_once(',').ok_or_else(invalid)?;
    let (w, h) = size.split_once('x').ok_or_else(invalid)?;
    let x: i32 = x.parse().map_err(|_| invalid())?;
    let y: i32 = y.parse().map_err(|_| invalid())?;
    let width: u32 = w.parse().map_err(|_| invalid())?;
    let height: u32 = h.parse().map_err(|_| invalid())?;
    if width == 0 || height == 0 {
        return Err(invalid());
    }
    Ok(Region {
        x,
        y,
        width,
        height,
    })
}
