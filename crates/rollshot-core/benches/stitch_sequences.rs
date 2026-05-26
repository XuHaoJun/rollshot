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
    #[arg(long, default_value_t = 3)]
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

fn select_scenarios(args: &Args) -> Vec<Scenario> {
    let all = registered_scenarios();
    match &args.fixtures {
        Some(filter) => {
            let allowed: std::collections::HashSet<&str> =
                filter.split(',').map(str::trim).collect();
            all.into_iter()
                .filter(|s| allowed.contains(s.name.as_str()))
                .collect()
        }
        None => all,
    }
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

    // Orchestrator mode.
    let selected = select_scenarios(&args);
    if selected.is_empty() {
        eprintln!("no scenarios matched the --fixtures filter");
        std::process::exit(2);
    }

    let out_path = args.out.unwrap_or_else(|| default_out_path(now));
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).expect("create bench output dir");
    }

    let mut out_file: Option<BufWriter<fs::File>> = if args.no_jsonl {
        None
    } else {
        Some(BufWriter::new(
            fs::File::create(&out_path).expect("open output JSONL"),
        ))
    };

    let exe = std::env::current_exe().expect("current_exe");
    let mut total_workers = 0usize;
    let mut failed_workers = 0usize;

    for scenario in &selected {
        for run in 0..args.repeats {
            total_workers += 1;
            eprintln!(
                "[orchestrator] scenario={} run={}/{}",
                scenario.name,
                run + 1,
                args.repeats
            );
            let output = std::process::Command::new(&exe)
                .arg("--run-single-scenario")
                .arg(&scenario.name)
                .arg("--worker-run")
                .arg(run.to_string())
                .output();
            match output {
                Ok(o) if o.status.success() => {
                    if let Some(file) = out_file.as_mut() {
                        file.write_all(&o.stdout).expect("write worker stdout");
                    } else {
                        io::stdout().write_all(&o.stdout).ok();
                    }
                }
                Ok(o) => {
                    failed_workers += 1;
                    eprintln!(
                        "[orchestrator] worker failed: {}\nstderr: {}",
                        scenario.name,
                        String::from_utf8_lossy(&o.stderr)
                    );
                    if let Some(file) = out_file.as_mut() {
                        file.write_all(&o.stdout).ok();
                    }
                }
                Err(e) => {
                    failed_workers += 1;
                    eprintln!("[orchestrator] failed to spawn worker: {e}");
                }
            }
        }
    }

    if let Some(mut file) = out_file {
        file.flush().expect("flush output JSONL");
    }

    eprintln!("[orchestrator] done: {total_workers} worker run(s), {failed_workers} failed");
    if !args.no_jsonl {
        eprintln!("[orchestrator] JSONL written to {}", out_path.display());
    }
}

fn run_worker(name: &str, run: usize) {
    let scenario = registered_scenarios()
        .into_iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| {
            eprintln!("unknown scenario: {name}");
            std::process::exit(2);
        });

    let stdout = io::stdout().lock();
    let mut out = BufWriter::new(stdout);

    if let Err(e) = run_scenario_worker(&scenario, run, &mut out) {
        let rec = BenchRecord::Error(ErrorRecord {
            scenario: &scenario.name,
            run,
            git_sha: GIT_SHA,
            frame: None,
            message: format!("{e:?}"),
        });
        let _ = writeln!(out, "{}", serde_json::to_string(&rec).unwrap());
        let _ = out.flush();
        std::process::exit(1);
    }
    let _ = out.flush();
}

#[derive(Default)]
struct OutcomeCounts {
    total: usize,
    appended: usize,
    duplicate: usize,
    no_match: usize,
    no_progress: usize,
    axis_changed: usize,
}

impl OutcomeCounts {
    fn record(&mut self, outcome: &StitchOutcome) {
        self.total += 1;
        match outcome {
            StitchOutcome::FirstFrame => {}
            StitchOutcome::Appended { .. } => self.appended += 1,
            StitchOutcome::Duplicate => self.duplicate += 1,
            StitchOutcome::NoMatch { .. } => self.no_match += 1,
            StitchOutcome::NoProgress { .. } => self.no_progress += 1,
            StitchOutcome::AxisChanged { .. } => self.axis_changed += 1,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn process_and_emit(
    stitcher: &mut Stitcher,
    scenario_name: &str,
    run: usize,
    idx: usize,
    frame: RgbaImage,
    counts: &mut OutcomeCounts,
    rss_peak: &mut u64,
    out: &mut dyn Write,
) -> io::Result<()> {
    let outcome = stitcher.push_frame(frame);
    counts.record(&outcome);
    let metrics = stitcher.last_metrics();
    let rec = BenchRecord::Frame(make_frame_record(scenario_name, run, metrics));
    writeln!(out, "{}", serde_json::to_string(&rec).unwrap())?;
    if idx % 10 == 0 {
        *rss_peak = (*rss_peak).max(rss::read_rss_kb());
    }
    Ok(())
}

fn run_scenario_worker(scenario: &Scenario, run: usize, out: &mut impl Write) -> io::Result<()> {
    let rss_baseline = rss::read_rss_kb();
    let mut rss_peak = rss_baseline;
    let mut stitcher = Stitcher::new(scenario.config.clone());
    let mut counts = OutcomeCounts::default();

    match &scenario.source {
        ScenarioSource::Fixture { family } => {
            for (idx, frame) in load_fixture_frames(family).into_iter().enumerate() {
                process_and_emit(
                    &mut stitcher,
                    &scenario.name,
                    run,
                    idx,
                    frame,
                    &mut counts,
                    &mut rss_peak,
                    out,
                )?;
            }
        }
        ScenarioSource::Synthetic(spec) => {
            let base = make_scroll_canvas(spec.canvas_width, spec.canvas_height);
            for (idx, frame) in spec.frames(&base).enumerate() {
                process_and_emit(
                    &mut stitcher,
                    &scenario.name,
                    run,
                    idx,
                    frame,
                    &mut counts,
                    &mut rss_peak,
                    out,
                )?;
            }
        }
    }
    rss_peak = rss_peak.max(rss::read_rss_kb());

    let stitched: Option<RgbaImage> = stitcher.full_image().cloned();

    let (final_logical_pixels, final_allocated_bytes) = stitched
        .as_ref()
        .map(|img| {
            (
                img.width() as u64 * img.height() as u64,
                img.as_raw().len() as u64,
            )
        })
        .unwrap_or((0, 0));

    let output_pixel_hash = stitched
        .as_ref()
        .map(pixel_hash)
        .unwrap_or_else(|| "none".to_string());

    let (output_max_channel_diff, output_mismatch_ratio) = if scenario.has_golden {
        let family = match &scenario.source {
            ScenarioSource::Fixture { family } => family.clone(),
            _ => String::new(),
        };
        match (load_golden_image(&family), stitched.as_ref()) {
            (Some(g), Some(s)) => compare_against_golden(s, &g),
            _ => (None, None),
        }
    } else {
        (None, None)
    };

    let summary = BenchRecord::Summary(SummaryRecord {
        scenario: &scenario.name,
        run,
        git_sha: GIT_SHA,
        peak_rss_kb_delta: rss_peak.saturating_sub(rss_baseline),
        peak_rss_kb_absolute: rss_peak,
        total_frames: counts.total,
        appended: counts.appended,
        duplicate: counts.duplicate,
        no_match: counts.no_match,
        no_progress: counts.no_progress,
        axis_changed: counts.axis_changed,
        final_canvas_logical_pixels: final_logical_pixels,
        final_canvas_allocated_bytes: final_allocated_bytes,
        output_pixel_hash,
        output_max_channel_diff,
        output_mismatch_ratio,
    });
    writeln!(out, "{}", serde_json::to_string(&summary).unwrap())?;
    Ok(())
}

fn pixel_hash(img: &RgbaImage) -> String {
    // FNV-1a 64-bit over the raw byte buffer.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in img.as_raw() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn compare_against_golden(actual: &RgbaImage, expected: &RgbaImage) -> (Option<u8>, Option<f32>) {
    if actual.dimensions() != expected.dimensions() {
        return (Some(255), Some(1.0));
    }
    let total = (actual.width() as u64) * (actual.height() as u64);
    let mut mismatched = 0u64;
    let mut max_chan: u8 = 0;
    for (a, e) in actual.pixels().zip(expected.pixels()) {
        let dr = a[0].abs_diff(e[0]);
        let dg = a[1].abs_diff(e[1]);
        let db = a[2].abs_diff(e[2]);
        let da = a[3].abs_diff(e[3]);
        let local_max = dr.max(dg).max(db).max(da);
        if local_max > max_chan {
            max_chan = local_max;
        }
        if local_max > 0 {
            mismatched += 1;
        }
    }
    let ratio = mismatched as f32 / total.max(1) as f32;
    (Some(max_chan), Some(ratio))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    fn args_with_filter(filter: Option<&str>) -> Args {
        Args {
            fixtures: filter.map(|s| s.to_string()),
            out: None,
            repeats: 1,
            no_jsonl: false,
            run_single_scenario: None,
            worker_run: 0,
        }
    }

    #[test]
    fn select_scenarios_no_filter_returns_all_registered() {
        let selected = select_scenarios(&args_with_filter(None));
        assert_eq!(selected.len(), registered_scenarios().len());
    }

    #[test]
    fn select_scenarios_filter_returns_matching_subset() {
        let selected = select_scenarios(&args_with_filter(Some(
            "duplicate_frames,long_vertical_text",
        )));
        let names: Vec<_> = selected.iter().map(|s| s.name.clone()).collect();
        assert_eq!(
            names,
            vec![
                "duplicate_frames".to_string(),
                "long_vertical_text".to_string()
            ]
        );
    }

    #[test]
    fn select_scenarios_filter_unknown_name_returns_empty() {
        let selected = select_scenarios(&args_with_filter(Some("does_not_exist")));
        assert!(selected.is_empty());
    }

    #[test]
    fn pixel_hash_is_deterministic() {
        let img = synthetic::make_scroll_canvas(64, 64);
        assert_eq!(pixel_hash(&img), pixel_hash(&img));
    }

    #[test]
    fn pixel_hash_differs_for_different_inputs() {
        let img1 = synthetic::make_scroll_canvas(64, 64);
        let mut img2 = img1.clone();
        img2.put_pixel(0, 0, image::Rgba([1, 2, 3, 255]));
        assert_ne!(pixel_hash(&img1), pixel_hash(&img2));
    }
}
