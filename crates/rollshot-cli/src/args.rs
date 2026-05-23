use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "rollshot", version, about = "rollshot: scrollshot stitcher")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Capture frames from a backend and stitch them into a long PNG.
    Capture(CaptureArgs),

    /// Print diagnostics about the host and known capture backends.
    Probe(ProbeArgs),

    /// Stitch a directory of pre-recorded frames without going through a
    /// capture backend. Useful for iterating on the matcher.
    StitchFolder(StitchFolderArgs),
}

#[derive(Debug, clap::Args)]
pub struct CaptureArgs {
    /// Which capture backend to use.
    #[arg(
        long,
        default_value = "auto",
        value_parser = ["auto", "fixture", "linux-portal", "macos-sck"],
    )]
    pub backend: String,

    /// Region selection mode. Accepts `auto`, `portal`, `full`, or `"X,Y WxH"`.
    #[arg(long, default_value = "auto")]
    pub region: String,

    /// Output PNG path.
    #[arg(long)]
    pub output: PathBuf,

    /// Directory of pre-recorded frames; required with --backend fixture.
    #[arg(long)]
    pub fixture: Option<PathBuf>,

    /// Optional directory where every captured frame is written as PNG.
    #[arg(long)]
    pub dump_frames: Option<PathBuf>,

    /// Maximum number of frames to read before stopping.
    #[arg(long, default_value_t = 200)]
    pub max_frames: u32,

    /// Capture frame rate (used by real backends; ignored by fixture).
    #[arg(long, default_value_t = 5)]
    pub fps: u32,

    /// Include the cursor in captured frames.
    #[arg(long, default_value_t = false)]
    pub show_cursor: bool,

    /// Suppress per-frame capture progress on stderr.
    #[arg(long, default_value_t = false)]
    pub quiet: bool,

    /// Enable the AKAZE feature-based fallback instead of FAST+KNN.
    /// DEPRECATED — AKAZE will be removed in the next minor release.
    /// Kept for parity testing during the FAST migration.
    #[arg(long, default_value_t = false)]
    pub enable_akaze: bool,

    /// Disable the FAST + linear-KNN feature fallback. The fallback only
    /// runs after the regular matchers and the relaxed coarse pass both
    /// miss; disabling is for benchmarking / debugging the matcher path.
    #[arg(long, default_value_t = false)]
    pub disable_feature_fallback: bool,
}

#[derive(Debug, clap::Args)]
pub struct ProbeArgs {
    /// Emit JSON instead of human-readable text.
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, clap::Args)]
pub struct StitchFolderArgs {
    /// Directory of frames to stitch.
    pub frames_dir: PathBuf,

    /// Output PNG path.
    #[arg(long, short)]
    pub output: PathBuf,

    /// Write a JSON report with one match outcome per input frame.
    #[arg(long)]
    pub debug_match_report: Option<PathBuf>,

    /// Write overlap and diff images for frames with estimates.
    #[arg(long)]
    pub dump_overlap_debug: Option<PathBuf>,

    /// Enable the AKAZE feature-based fallback. DEPRECATED — AKAZE will
    /// be removed in the next minor release. Kept for parity testing
    /// during the FAST migration.
    #[arg(long, default_value_t = false)]
    pub enable_akaze: bool,

    /// Disable the FAST + linear-KNN feature fallback. The fallback only
    /// runs after the regular matchers and the relaxed coarse pass both
    /// miss; disabling is for benchmarking / debugging the matcher path.
    #[arg(long, default_value_t = false)]
    pub disable_feature_fallback: bool,
}
