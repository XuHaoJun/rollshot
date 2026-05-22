use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use rollshot_core::{MotionEstimate, OverlapRegion, StitchConfig, StitchOutcome, Stitcher};
use serde::Serialize;

use crate::args::StitchFolderArgs;
use crate::cli_error::CliError;

#[derive(Debug, Serialize)]
struct MatchReport {
    frames: Vec<FrameReport>,
}

#[derive(Debug, Serialize)]
struct FrameReport {
    frame_index: usize,
    path: String,
    outcome: String,
    reason: Option<String>,
    estimate: Option<EstimateReport>,
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

fn write_report(path: &Path, report: &MatchReport) -> Result<(), CliError> {
    let json = serde_json::to_string_pretty(report)
        .map_err(|err| CliError::new(format!("failed to encode match report: {err}"), 1))?;
    std::fs::write(path, json)
        .map_err(|err| CliError::new(format!("failed to write {}: {err}", path.display()), 1))
}

fn write_overlap_artifacts(
    dir: &Path,
    frame_index: usize,
    prev: &RgbaImage,
    curr: &RgbaImage,
    estimate: &MotionEstimate,
) -> Result<(), CliError> {
    std::fs::create_dir_all(dir).map_err(|err| {
        CliError::new(
            format!("failed to create debug dir {}: {err}", dir.display()),
            1,
        )
    })?;
    let prefix = format!("frame_{frame_index:03}");
    crop_overlap(
        prev,
        estimate.overlap.prev_x,
        estimate.overlap.prev_y,
        estimate.overlap.width,
        estimate.overlap.height,
    )
    .save_with_format(
        dir.join(format!("{prefix}_overlap_prev.png")),
        ImageFormat::Png,
    )
    .map_err(|err| CliError::new(format!("failed to save overlap prev: {err}"), 1))?;
    crop_overlap(
        curr,
        estimate.overlap.curr_x,
        estimate.overlap.curr_y,
        estimate.overlap.width,
        estimate.overlap.height,
    )
    .save_with_format(
        dir.join(format!("{prefix}_overlap_curr.png")),
        ImageFormat::Png,
    )
    .map_err(|err| CliError::new(format!("failed to save overlap curr: {err}"), 1))?;
    diff_overlap(prev, curr, estimate.overlap)
        .save_with_format(dir.join(format!("{prefix}_diff.png")), ImageFormat::Png)
        .map_err(|err| CliError::new(format!("failed to save overlap diff: {err}"), 1))?;
    Ok(())
}

fn crop_overlap(img: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
    image::imageops::crop_imm(img, x, y, w, h).to_image()
}

fn diff_overlap(prev: &RgbaImage, curr: &RgbaImage, overlap: OverlapRegion) -> RgbaImage {
    let mut out = RgbaImage::from_pixel(overlap.width, overlap.height, Rgba([0, 0, 0, 255]));
    for y in 0..overlap.height {
        for x in 0..overlap.width {
            let p = prev.get_pixel(overlap.prev_x + x, overlap.prev_y + y);
            let c = curr.get_pixel(overlap.curr_x + x, overlap.curr_y + y);
            out.put_pixel(
                x,
                y,
                Rgba([
                    p[0].abs_diff(c[0]),
                    p[1].abs_diff(c[1]),
                    p[2].abs_diff(c[2]),
                    255,
                ]),
            );
        }
    }
    out
}

pub fn run(args: &StitchFolderArgs) -> Result<String, CliError> {
    let frames_dir = &args.frames_dir;
    let output = &args.output;

    if !frames_dir.is_dir() {
        return Err(CliError::new(
            format!("frames directory not found: {}", frames_dir.display()),
            1,
        ));
    }

    let frame_paths = collect_frame_paths(frames_dir)?;
    if frame_paths.is_empty() {
        return Err(CliError::new(
            format!(
                "no supported images in {} (expected .png/.jpg/.jpeg)",
                frames_dir.display()
            ),
            1,
        ));
    }

    let mut config = StitchConfig::default();
    if args.disable_akaze {
        config.akaze.enabled = false;
    }
    let mut stitcher = Stitcher::new(config);
    let mut report = MatchReport { frames: Vec::new() };
    let mut last_accepted: Option<RgbaImage> = None;
    let mut appended = 0u32;
    let mut duplicates = 0u32;
    let mut no_match = 0u32;
    let mut no_progress = 0u32;

    for path in &frame_paths {
        let img = image::open(path).map_err(|err| {
            CliError::new(format!("failed to decode {}: {err}", path.display()), 1)
        })?;
        let frame = into_rgba(img);

        let outcome = stitcher.push_frame(frame.clone());
        let mut frame_report = FrameReport {
            frame_index: report.frames.len(),
            path: path.display().to_string(),
            outcome: String::new(),
            reason: None,
            estimate: None,
        };

        match &outcome {
            StitchOutcome::FirstFrame => {
                frame_report.outcome = "FirstFrame".to_string();
                last_accepted = Some(frame);
            }
            StitchOutcome::Appended { estimate, .. } => {
                appended += 1;
                frame_report.outcome = "Appended".to_string();
                frame_report.estimate = Some(estimate_report(estimate));
                if let (Some(dir), Some(prev)) =
                    (args.dump_overlap_debug.as_ref(), last_accepted.as_ref())
                {
                    write_overlap_artifacts(dir, report.frames.len(), prev, &frame, estimate)?;
                }
                last_accepted = Some(frame);
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
                if let (Some(dir), Some(prev), Some(estimate)) = (
                    args.dump_overlap_debug.as_ref(),
                    last_accepted.as_ref(),
                    best_estimate.as_ref(),
                ) {
                    write_overlap_artifacts(dir, report.frames.len(), prev, &frame, estimate)?;
                }
            }
            StitchOutcome::NoProgress { estimate } => {
                no_progress += 1;
                frame_report.outcome = "NoProgress".to_string();
                frame_report.estimate = estimate.as_ref().map(estimate_report);
                if let (Some(dir), Some(prev), Some(estimate)) = (
                    args.dump_overlap_debug.as_ref(),
                    last_accepted.as_ref(),
                    estimate.as_ref(),
                ) {
                    write_overlap_artifacts(dir, report.frames.len(), prev, &frame, estimate)?;
                }
            }
            StitchOutcome::AxisChanged { estimate, .. } => {
                no_match += 1;
                frame_report.outcome = "AxisChanged".to_string();
                frame_report.estimate = Some(estimate_report(estimate));
                if let (Some(dir), Some(prev)) =
                    (args.dump_overlap_debug.as_ref(), last_accepted.as_ref())
                {
                    write_overlap_artifacts(dir, report.frames.len(), prev, &frame, estimate)?;
                }
            }
        }

        report.frames.push(frame_report);
    }

    let stitched = stitcher
        .full_image()
        .ok_or_else(|| CliError::new("no stitched output available", 1))?;
    stitched
        .save_with_format(output, ImageFormat::Png)
        .map_err(|err| CliError::new(format!("failed to save {}: {err}", output.display()), 1))?;

    if let Some(path) = args.debug_match_report.as_ref() {
        write_report(path, &report)?;
    }

    Ok(format!(
        "stitch-folder: {dir}\n\
         input frames: {input}\n\
         appended: {appended}\n\
         duplicates: {duplicates}\n\
         no-progress: {no_progress}\n\
         no-match: {no_match}\n\
         output: {out} ({w}x{h})\n",
        dir = frames_dir.display(),
        input = frame_paths.len(),
        appended = appended,
        duplicates = duplicates,
        no_progress = no_progress,
        no_match = no_match,
        out = output.display(),
        w = stitched.width(),
        h = stitched.height(),
    ))
}

fn collect_frame_paths(dir: &Path) -> Result<Vec<PathBuf>, CliError> {
    let entries = fs::read_dir(dir)
        .map_err(|err| CliError::new(format!("failed to read {}: {err}", dir.display()), 1))?;

    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| {
            CliError::new(
                format!("failed to read entry in {}: {err}", dir.display()),
                1,
            )
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|err| {
            CliError::new(format!("failed to inspect {}: {err}", path.display()), 1)
        })?;
        if !file_type.is_file() {
            continue;
        }
        if matches!(
            path.extension()
                .and_then(OsStr::to_str)
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("png" | "jpg" | "jpeg")
        ) {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn into_rgba(image: DynamicImage) -> image::RgbaImage {
    match image {
        DynamicImage::ImageRgba8(rgba) => rgba,
        other => other.to_rgba8(),
    }
}
