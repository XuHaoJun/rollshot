use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use image::{DynamicImage, ImageFormat};
use rollshot_core::{StitchConfig, StitchOutcome, Stitcher};

use crate::args::StitchFolderArgs;
use crate::cli_error::CliError;

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

    let mut stitcher = Stitcher::new(StitchConfig::default());
    let mut appended = 0u32;
    let mut duplicates = 0u32;
    let mut no_match = 0u32;
    let mut no_progress = 0u32;

    for path in &frame_paths {
        let img = image::open(path).map_err(|err| {
            CliError::new(format!("failed to decode {}: {err}", path.display()), 1)
        })?;
        let frame = into_rgba(img);

        match stitcher.push_frame(frame) {
            StitchOutcome::FirstFrame => {}
            StitchOutcome::Appended { .. } => appended += 1,
            StitchOutcome::Duplicate => duplicates += 1,
            StitchOutcome::NoMatch { .. } => no_match += 1,
            StitchOutcome::NoProgress { .. } => no_progress += 1,
            StitchOutcome::AxisChanged { .. } => no_match += 1,
        }
    }

    let stitched = stitcher
        .full_image()
        .ok_or_else(|| CliError::new("no stitched output available", 1))?;
    stitched
        .save_with_format(output, ImageFormat::Png)
        .map_err(|err| CliError::new(format!("failed to save {}: {err}", output.display()), 1))?;

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
