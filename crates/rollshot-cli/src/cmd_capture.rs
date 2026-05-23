//! `rollshot capture` — drives a CaptureBackend, stitches its frames, writes a
//! PNG.
//!
//! Pipeline:
//!
//! ```text
//!   args (clap)
//!     │
//!     ▼
//!   BackendKind::from_cli_flag ─► BackendKind
//!     │                              │
//!     │                              ├── Fixture  → FixtureBackend::new(--fixture)
//!     │                              └── other    → BackendKind::create()
//!     ▼
//!   CaptureOptions { region, fps, show_cursor }
//!     │
//!     ▼
//!   backend.start(options) ──► Box<dyn FrameStream>
//!     │
//!     ▼ loop until EndOfStream / --max-frames
//!     ┌──────────────────────────────────────────┐
//!     │  stream.next_frame()                     │
//!     │     │                                    │
//!     │     ▼                                    │
//!     │  [optional] write_dump_frame(idx, image) │
//!     │     │                                    │
//!     │     ▼                                    │
//!     │  stitcher.push_frame(image)              │
//!     └──────────────────────────────────────────┘
//!     │
//!     ▼
//!   stitcher.full_image() → save PNG → summary
//! ```

use std::path::Path;
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
    let kind = BackendKind::from_cli_flag(&args.backend).map_err(CliError::from_capture)?;
    let mut backend = build_backend(kind, args)?;
    let region = parse_region(&args.region, kind)?;
    let options = CaptureOptions {
        region,
        fps: args.fps,
        show_cursor: args.show_cursor,
        prefer_portal_region: true,
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
    if args.enable_akaze {
        eprintln!(
            "warning: --enable-akaze is deprecated and will be removed in \
             a future release; FAST+KNN is the default feature fallback"
        );
        config.akaze.enabled = true;
    }
    let mut stitcher = Stitcher::new(config);
    let mut captured: u32 = 0;
    let mut appended: u32 = 0;
    let mut duplicates: u32 = 0;
    let mut no_match: u32 = 0;
    let mut no_progress: u32 = 0;
    let mut report = CaptureMatchReport { frames: Vec::new() };
    let mut previous_capture_timestamp = None;

    loop {
        match stream.next_frame() {
            Ok(frame) => {
                let capture_interval_ms = previous_capture_timestamp.and_then(|previous| {
                    frame
                        .timestamp
                        .duration_since(previous)
                        .ok()
                        .map(duration_ms)
                });
                previous_capture_timestamp = Some(frame.timestamp);

                if let Some(dir) = args.dump_frames.as_ref() {
                    write_dump_frame(dir, captured, &frame.image)?;
                }
                captured += 1;
                if !args.quiet {
                    log_capture_start(captured, args.max_frames);
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
                report.frames.push(frame_report);
                if !args.quiet {
                    log_capture_progress(captured, args.max_frames, &outcome, stitch_elapsed);
                }
                if captured >= args.max_frames {
                    break;
                }
            }
            Err(CaptureError::EndOfStream) => break,
            Err(err) => return Err(CliError::from_capture(err)),
        }
    }

    let stitched = stitcher
        .full_image()
        .ok_or_else(|| CliError::new("no frames produced an output image", 1))?;
    save_png(stitched, &args.output)?;

    if let Some(path) = args.debug_match_report.as_ref() {
        write_report(path, &report)?;
    }

    Ok(format!(
        "captured {captured} frames, appended {appended} (duplicates {duplicates}, no-progress {no_progress}, no-match {no_match})\noutput: {out} ({w}x{h})\n",
        out = args.output.display(),
        w = stitched.width(),
        h = stitched.height(),
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
