use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "rollshot-dev",
    version,
    about = "Rollshot developer diagnostics and offline stitching"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print diagnostics about the host and known backends.
    Probe(ProbeArgs),

    /// Stitch a directory of pre-recorded frames without a live backend.
    StitchFolder(StitchFolderArgs),
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

    /// Disable the FAST + linear-KNN feature fallback.
    #[arg(long, default_value_t = false)]
    pub disable_feature_fallback: bool,
}
