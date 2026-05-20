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

use image::ImageFormat;
use rollshot_capture::{
    BackendKind, CaptureBackend, CaptureError, CaptureOptions, FixtureBackend, Region, RegionMode,
};
use rollshot_core::{StitchConfig, StitchOutcome, Stitcher};

use crate::args::CaptureArgs;
use crate::cli_error::CliError;

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

    let mut stitcher = Stitcher::new(StitchConfig::default());
    let mut captured: u32 = 0;
    let mut appended: u32 = 0;
    let mut duplicates: u32 = 0;
    let mut no_match: u32 = 0;
    let mut no_progress: u32 = 0;

    loop {
        match stream.next_frame() {
            Ok(frame) => {
                if let Some(dir) = args.dump_frames.as_ref() {
                    write_dump_frame(dir, captured, &frame.image)?;
                }
                captured += 1;
                match stitcher.push_frame(frame.image) {
                    StitchOutcome::FirstFrame => {}
                    StitchOutcome::Appended { .. } => appended += 1,
                    StitchOutcome::Duplicate => duplicates += 1,
                    StitchOutcome::NoMatch { .. } => no_match += 1,
                    StitchOutcome::NoProgress => no_progress += 1,
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

    Ok(format!(
        "captured {captured} frames, appended {appended} (duplicates {duplicates}, no-progress {no_progress}, no-match {no_match})\noutput: {out} ({w}x{h})\n",
        out = args.output.display(),
        w = stitched.width(),
        h = stitched.height(),
    ))
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

fn parse_region(flag: &str, kind: BackendKind) -> Result<RegionMode, CliError> {
    match flag {
        "auto" => Ok(match kind {
            BackendKind::LinuxPortalPipeWire => RegionMode::PortalPicker,
            BackendKind::MacosScreenCaptureKit
            | BackendKind::Fixture
            | BackendKind::Unsupported => RegionMode::FullSource,
        }),
        "portal" => Ok(RegionMode::PortalPicker),
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
