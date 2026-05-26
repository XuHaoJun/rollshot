//! End-to-end bench harness for rollshot stitching.
//!
//! Modes:
//! - Default (orchestrator): enumerate scenarios, spawn one subprocess per
//!   scenario (Task 12), merge their JSONL stdout into the output file.
//! - `--run-single-scenario <name>` (worker, Task 11): run one scenario and
//!   emit JSONL records to stdout. Used by the orchestrator.

mod rss;
mod synthetic;

use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use image::RgbaImage;
use rollshot_core::{
    MatchMethod, NoMatchReason, StitchConfig, StitchMetrics, StitchOutcome, StitchOutcomeKind,
    Stitcher,
};
use serde::Serialize;

use synthetic::{make_scroll_canvas, SyntheticSpec};

const GIT_SHA: &str = env!("ROLLSHOT_GIT_SHA");
const FIXTURE_ROOT: &str = "tests/fixtures/linearscroll_v2";

#[derive(Parser, Debug)]
#[command(about = "rollshot stitch sequence bench harness")]
struct Args {
    /// Comma-separated scenario names. Default: all registered scenarios.
    #[arg(long)]
    fixtures: Option<String>,

    /// Output JSONL path. Default: target/bench/stitch_sequences-<sha>-<utc>.jsonl
    #[arg(long)]
    out: Option<PathBuf>,

    /// Number of repetitions per scenario.
    #[arg(long, default_value_t = 5)]
    repeats: usize,

    /// Skip writing JSONL, only print summary to stdout.
    #[arg(long)]
    no_jsonl: bool,

    /// Internal: run one scenario in worker mode. Used by the orchestrator.
    #[arg(long, hide = true)]
    run_single_scenario: Option<String>,

    /// Internal: which run index this worker invocation should record.
    #[arg(long, hide = true, default_value_t = 0)]
    worker_run: usize,
}

#[derive(Debug, Clone)]
enum ScenarioSource {
    Fixture { family: String },
    Synthetic(SyntheticSpec),
}

#[derive(Debug, Clone)]
struct Scenario {
    name: String,
    source: ScenarioSource,
    config: StitchConfig,
    has_golden: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum BenchRecord<'a> {
    Frame(FrameRecord<'a>),
    Summary(SummaryRecord<'a>),
    Error(ErrorRecord<'a>),
}

#[derive(Debug, Serialize)]
struct FrameRecord<'a> {
    scenario: &'a str,
    run: usize,
    frame: usize,
    git_sha: &'a str,
    outcome: &'static str,
    no_match_reason: Option<&'static str>,
    total_us: u64,
    duplicate_us: u64,
    prepare_frame_us: u64,
    coarse_us: u64,
    template_ncc_us: u64,
    edge_projection_us: u64,
    verifier_us: u64,
    fallback_us: u64,
    append_us: u64,
    coarse_candidates: usize,
    ncc_offsets_scored: usize,
    ncc_pixel_visits: usize,
    verifier_candidates: usize,
    fallback_features_extracted: usize,
    canvas_logical_pixels: u64,
    canvas_allocated_bytes: u64,
    append_copied_bytes: u64,
    best_dx: i32,
    best_dy: i32,
    best_score: f32,
    second_best_score: Option<f32>,
    match_method: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct SummaryRecord<'a> {
    scenario: &'a str,
    run: usize,
    git_sha: &'a str,
    peak_rss_kb_delta: u64,
    peak_rss_kb_absolute: u64,
    total_frames: usize,
    appended: usize,
    duplicate: usize,
    no_match: usize,
    no_progress: usize,
    axis_changed: usize,
    final_canvas_logical_pixels: u64,
    final_canvas_allocated_bytes: u64,
    output_pixel_hash: String,
    output_max_channel_diff: Option<u8>,
    output_mismatch_ratio: Option<f32>,
}

#[derive(Debug, Serialize)]
struct ErrorRecord<'a> {
    scenario: &'a str,
    run: usize,
    git_sha: &'a str,
    frame: Option<usize>,
    message: String,
}

fn outcome_str(kind: StitchOutcomeKind) -> &'static str {
    match kind {
        StitchOutcomeKind::None => "None",
        StitchOutcomeKind::FirstFrame => "FirstFrame",
        StitchOutcomeKind::Appended => "Appended",
        StitchOutcomeKind::Duplicate => "Duplicate",
        StitchOutcomeKind::NoMatch => "NoMatch",
        StitchOutcomeKind::NoProgress => "NoProgress",
        StitchOutcomeKind::AxisChanged => "AxisChanged",
    }
}

fn no_match_reason_str(reason: NoMatchReason) -> &'static str {
    match reason {
        NoMatchReason::LowConfidence => "LowConfidence",
        NoMatchReason::AmbiguousAxis => "AmbiguousAxis",
        NoMatchReason::CrossAxisTooLarge => "CrossAxisTooLarge",
        NoMatchReason::InsufficientOverlap => "InsufficientOverlap",
        NoMatchReason::OverlapVerificationFailed => "OverlapVerificationFailed",
        NoMatchReason::NotEnoughFeatures => "NotEnoughFeatures",
        NoMatchReason::MotionTooSmall => "MotionTooSmall",
        NoMatchReason::DimensionMismatch => "DimensionMismatch",
        NoMatchReason::FeatureFallbackDisabled => "FeatureFallbackDisabled",
        NoMatchReason::FeatureLowInliers => "FeatureLowInliers",
        NoMatchReason::ReverseDirection => "ReverseDirection",
        _ => "Unknown",
    }
}

fn match_method_str(method: MatchMethod) -> &'static str {
    match method {
        MatchMethod::Template => "Template",
        MatchMethod::Coarse => "Coarse",
        MatchMethod::Edge => "Edge",
        MatchMethod::FastHnsw => "FastHnsw",
        _ => "Unknown",
    }
}

fn make_frame_record<'a>(
    scenario: &'a str,
    run: usize,
    metrics: &StitchMetrics,
) -> FrameRecord<'a> {
    FrameRecord {
        scenario,
        run,
        frame: metrics.frame_index,
        git_sha: GIT_SHA,
        outcome: outcome_str(metrics.outcome),
        no_match_reason: metrics.no_match_reason.map(no_match_reason_str),
        total_us: metrics.total_us,
        duplicate_us: metrics.duplicate_us,
        prepare_frame_us: metrics.prepare_frame_us,
        coarse_us: metrics.coarse_us,
        template_ncc_us: metrics.template_ncc_us,
        edge_projection_us: metrics.edge_projection_us,
        verifier_us: metrics.verifier_us,
        fallback_us: metrics.fallback_us,
        append_us: metrics.append_us,
        coarse_candidates: metrics.coarse_candidates,
        ncc_offsets_scored: metrics.ncc_offsets_scored,
        ncc_pixel_visits: metrics.ncc_pixel_visits,
        verifier_candidates: metrics.verifier_candidates,
        fallback_features_extracted: metrics.fallback_features_extracted,
        canvas_logical_pixels: metrics.canvas_logical_pixels,
        canvas_allocated_bytes: metrics.canvas_allocated_bytes,
        append_copied_bytes: metrics.append_copied_bytes,
        best_dx: metrics.best_dx,
        best_dy: metrics.best_dy,
        best_score: metrics.best_score,
        second_best_score: metrics.second_best_score,
        match_method: metrics.match_method.map(match_method_str),
    }
}

fn registered_scenarios() -> Vec<Scenario> {
    let mut out = Vec::new();
    out.extend(existing_fixture_scenarios());
    out.extend(synthetic::default_specs().into_iter().map(|spec| {
        spec.validate();
        Scenario {
            name: spec.name.clone(),
            source: ScenarioSource::Synthetic(spec),
            config: synthetic_default_config(),
            has_golden: false,
        }
    }));
    out
}

fn existing_fixture_scenarios() -> Vec<Scenario> {
    let mut large_search_cfg = StitchConfig::default();
    large_search_cfg.max_search_ratio = 0.75;
    let mut sticky_cfg = StitchConfig::default();
    sticky_cfg.verifier.downsample_max_mad = 40.0 / 255.0;
    sticky_cfg.verifier.full_res_max_mad = 30.0 / 255.0;

    vec![
        ("repeated_rows", StitchConfig::default()),
        ("repeated_grid", StitchConfig::default()),
        ("bad_frame", StitchConfig::default()),
        ("duplicate_frames", StitchConfig::default()),
        ("linear_vertical_down", large_search_cfg.clone()),
        ("linear_vertical_up", large_search_cfg.clone()),
        ("linear_horizontal_right", large_search_cfg.clone()),
        ("linear_horizontal_left", large_search_cfg.clone()),
        ("low_feature_text", large_search_cfg.clone()),
        ("image_cards", large_search_cfg),
        ("sticky_header", sticky_cfg),
    ]
    .into_iter()
    .map(|(family, config)| Scenario {
        name: family.to_string(),
        source: ScenarioSource::Fixture {
            family: family.to_string(),
        },
        config,
        has_golden: true,
    })
    .collect()
}

fn synthetic_default_config() -> StitchConfig {
    let mut cfg = StitchConfig::default();
    cfg.max_search_ratio = 0.75;
    cfg
}

fn load_fixture_frames(family: &str) -> Vec<RgbaImage> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURE_ROOT)
        .join(family)
        .join("frames");
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .expect("read frames dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("png"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|p| image::open(p).expect("decode").to_rgba8())
        .collect()
}

fn load_golden_image(family: &str) -> Option<RgbaImage> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURE_ROOT)
        .join(family)
        .join("expected/output.png");
    image::open(path).ok().map(|i| i.to_rgba8())
}

fn default_out_path(now: u64) -> PathBuf {
    PathBuf::from(format!(
        "target/bench/stitch_sequences-{GIT_SHA}-{now}.jsonl"
    ))
}

fn main() {
    let args = Args::parse();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if let Some(name) = &args.run_single_scenario {
        // Worker mode (Task 11 implements run_scenario_worker).
        run_worker(name, args.worker_run);
        return;
    }

    // Orchestrator mode (Task 12 implements spawn-and-merge).
    eprintln!("(orchestrator not yet implemented — see Tasks 11–12)");
    let _ = (args, now);
    eprintln!("Registered scenarios:");
    for s in registered_scenarios() {
        eprintln!("  - {} (has_golden={})", s.name, s.has_golden);
    }
}

fn run_worker(_name: &str, _run: usize) {
    eprintln!("(worker mode not yet implemented — see Task 11)");
}
