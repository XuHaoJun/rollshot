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
    BackendKind, CaptureBackend, CaptureError, CaptureOptions, FixtureBackend, RegionMode,
};
use rollshot_core::{StitchConfig, StitchOutcome, Stitcher};

use crate::args::CaptureArgs;
use crate::cli_error::CliError;

pub fn run(args: &CaptureArgs) -> Result<String, CliError> {
    let kind = BackendKind::from_cli_flag(&args.backend).map_err(CliError::from_capture)?;
    let mut backend = build_backend(kind, args)?;
    let options = CaptureOptions {
        region: RegionMode::FullSource,
        fps: args.fps,
        show_cursor: args.show_cursor,
        prefer_portal_region: true,
    };

    let mut stream = backend.start(options).map_err(CliError::from_capture)?;

    let mut stitcher = Stitcher::new(StitchConfig::default());
    let mut captured: u32 = 0;
    let mut appended: u32 = 0;
    let mut duplicates: u32 = 0;
    let mut no_match: u32 = 0;
    let mut no_progress: u32 = 0;

    loop {
        match stream.next_frame() {
            Ok(frame) => {
                captured += 1;
                match stitcher.push_frame(frame.image) {
                    StitchOutcome::FirstFrame => {}
                    StitchOutcome::Appended { .. } => appended += 1,
                    StitchOutcome::Duplicate => duplicates += 1,
                    StitchOutcome::NoMatch { .. } => no_match += 1,
                    StitchOutcome::NoProgress => no_progress += 1,
                }
            }
            Err(CaptureError::EndOfStream) => break,
            Err(err) => return Err(CliError::from_capture(err)),
        }
    }

    let stitched = stitcher
        .full_image()
        .ok_or_else(|| CliError::new("no frames produced an output image", 1))?;
    save_png(&stitched, &args.output)?;

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
            let dir = args.fixture.as_ref().ok_or_else(|| {
                CliError::new("--backend fixture requires --fixture <DIR>", 1)
            })?;
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
