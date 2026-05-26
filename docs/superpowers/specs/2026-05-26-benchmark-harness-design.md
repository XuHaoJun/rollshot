# Benchmark Harness Design (P0)

Date: 2026-05-26

## Goal

Establish a repeatable benchmark harness for the rollshot stitching pipeline so
that subsequent optimizations (P1 StripCanvas, P2 PreparedFrame cache, P3 Fast
NCC, etc., as outlined in `docs/stitching-rollshot-optimizations-2.md`) can be
validated with data, not intuition.

Concretely:

- Per-frame stage-level timing breakdown of `Stitcher::push_frame`.
- Per-frame algorithmic counters (`coarse_candidates`, `ncc_offsets_scored`,
  `ncc_pixel_visits`, etc.) so that wall-clock-irrelevant improvements still
  show up.
- Per-run peak resident-set-size (RSS) so canvas memory regressions surface.
- Output-correctness check (byte-similar diff against existing golden fixtures)
  alongside the timing numbers — never compare speed without verifying
  correctness still holds.
- Stable JSONL output that can be diffed across two runs to produce a
  before/after markdown table suitable for pasting into a PR description.

The harness must not change production stitching behavior. Instrumentation
overhead must be negligible (target: ≤1% of `push_frame` total time).

## Non-goals

- ❌ No CI regression gate in this iteration. PR description carries the
  before/after table; the gate can be added later if regressions slip through.
- ❌ No web dashboard / continuous bench tracking / Grafana / external store.
- ❌ No statistical significance tests beyond p50/p95/p99. With 5 repeats × 100+
  frames per scenario, eyeballing ±5% deltas is sufficient at current scale.
- ❌ No criterion micro-benches yet. The end-to-end sequence harness is the
  primary deliverable; criterion-style micro-benches can be added later when
  P3 (Fast NCC) needs them.
- ❌ No GPU profiling, no flamegraph integration, no `perf` wrappers — those
  are local-developer tools, orthogonal to this design.

## Context

Existing test infrastructure (`crates/rollshot-core/tests/golden_fixtures.rs`)
already runs 11 fixture families against `Stitcher::push_frame` and verifies
byte-similar output via `MAX_PIXEL_CHANNEL_DIFF = 4`, `MAX_MISMATCHED_PIXEL_RATIO
= 0.005`. The fixtures cover:

- linear vertical down/up
- linear horizontal left/right
- sticky_header
- repeated_grid, repeated_rows
- low_feature_text
- image_cards
- duplicate_frames, bad_frame

These fixtures validate correctness but are too short (5–20 frames) to expose
scaling behavior of the stitching pipeline. P1 (StripCanvas) specifically
targets append cost that grows with `canvas_h`; you cannot see that growth in a
20-frame fixture.

The optimization roadmap doc (`docs/stitching-rollshot-optimizations-2.md`,
section 2) already sketches the `StitchMetrics` struct and JSONL output. This
design refines and commits to that sketch.

Public `Stitcher` API today:

```rust
impl Stitcher {
    pub fn new(config: StitchConfig) -> Self;
    pub fn push_frame(&mut self, frame: RgbaImage) -> StitchOutcome;
    pub fn full_image(&self) -> Option<&RgbaImage>;
}
```

The harness extends this with a `last_metrics()` accessor — no other public-API
change.

## Architecture

```text
crates/rollshot-core/
├── src/
│   ├── metrics.rs          (new)   StitchMetrics + ScopedTimer
│   ├── stitcher.rs         (mod)   ScopedTimer wraps each stage
│   ├── matcher.rs          (mod)   Takes &mut StitchMetrics, fills sub-stage timings
│   ├── canvas.rs           (mod)   Adds allocated_bytes() and logical_pixels() accessors
│   └── lib.rs              (mod)   Re-export StitchMetrics, StitchOutcomeKind
├── benches/                (new)
│   ├── stitch_sequences.rs         Bench runner binary (orchestrator + worker)
│   ├── synthetic.rs                Synthetic long-sequence generators
│   └── rss.rs                      Peak RSS measurement (Linux/macOS/other)
└── tests/
    └── metrics_population.rs       (new) Integration tests for stage population

scripts/bench/
├── summarize.py            (new)   JSONL → markdown table
├── compare.py              (new)   Two JSONL runs → before/after delta table
└── test_summarize.py       (new)   pytest, edge cases

docs/
└── bench.md                (new)   How to run the bench, interpret output, add fixtures
```

## Components

### `StitchMetrics` (`crates/rollshot-core/src/metrics.rs`)

Per-frame snapshot reset at the start of every `push_frame` call:

```rust
#[derive(Debug, Clone, Default)]
pub struct StitchMetrics {
    pub frame_index: usize,
    pub outcome: StitchOutcomeKind,
    pub total_us: u64,

    // Per-stage timings (µs). 0 if stage skipped (e.g. duplicate frame skips matcher).
    pub duplicate_us: u64,
    pub prepare_frame_us: u64,
    pub coarse_us: u64,
    pub template_ncc_us: u64,
    pub edge_projection_us: u64,
    pub verifier_us: u64,
    pub fallback_us: u64,
    pub append_us: u64,

    // Algorithmic counters (CPU-independent).
    pub coarse_candidates: usize,
    pub ncc_offsets_scored: usize,
    pub ncc_pixel_visits: usize,
    pub verifier_candidates: usize,
    pub fallback_features_extracted: usize,

    // Canvas state after this frame.
    pub canvas_logical_pixels: u64,
    pub canvas_allocated_bytes: u64,
    pub append_copied_bytes: u64,

    // Motion outcome.
    pub best_dx: i32,
    pub best_dy: i32,
    pub best_score: f32,
    pub second_best_score: Option<f32>,
    pub match_method: Option<MatchMethod>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StitchOutcomeKind {
    #[default] None,
    FirstFrame, Appended, Duplicate, NoMatch,
    DimensionMismatch, OverlapVerificationFailed, ReverseDirection,
}

impl From<&StitchOutcome> for StitchOutcomeKind { /* ... */ }
```

**Note on serialization.** `StitchMetrics` itself does not derive
`serde::Serialize` — `rollshot-core` keeps `serde` as a dev-dependency only,
and the production library has no use for JSON. The bench binary defines a
local `BenchRecord` struct (with `serde::Serialize`) that wraps a
`StitchMetrics` plus scenario/run identifiers, converting `MatchMethod` and
`StitchOutcomeKind` to string variants at the serialization boundary. This
keeps the production library lean and avoids polluting the public type
surface with serde derives that downstream consumers may not want.

Exposed on `Stitcher`:

```rust
impl Stitcher {
    /// Per-frame metrics from the most recent push_frame call.
    /// Reset to defaults at the start of each push_frame.
    pub fn last_metrics(&self) -> &StitchMetrics { &self.last_metrics }
}
```

**Why always-on, no feature flag:** Cost is ~10 `Instant::now()` calls per
frame (~100 ns total), well under 1% of any realistic `push_frame` time.
Eliminating the feature-flag combinatorial surface is worth more than the
microscopic savings.

### `ScopedTimer` (`crates/rollshot-core/src/metrics.rs`)

RAII timer used inside each stage:

```rust
pub(crate) struct ScopedTimer<'a> {
    start: Instant,
    target: &'a mut u64,
}

impl<'a> ScopedTimer<'a> {
    pub fn new(target: &'a mut u64) -> Self {
        Self { start: Instant::now(), target }
    }
}

impl Drop for ScopedTimer<'_> {
    fn drop(&mut self) {
        *self.target = self.start.elapsed().as_micros() as u64;
    }
}
```

Drop-based recording means early returns and `?` propagation work correctly —
no risk of forgetting to record a stage.

### Stage instrumentation (`stitcher.rs`, `matcher.rs`)

In `Stitcher::push_frame`:

```rust
pub fn push_frame(&mut self, frame: RgbaImage) -> StitchOutcome {
    self.last_metrics = StitchMetrics::default();
    self.last_metrics.frame_index = self.frame_counter;
    self.frame_counter += 1;

    let _total = ScopedTimer::new(&mut self.last_metrics.total_us);

    // Duplicate detection
    let dup = {
        let _t = ScopedTimer::new(&mut self.last_metrics.duplicate_us);
        self.is_duplicate(&frame)
    };
    if dup {
        self.last_metrics.outcome = StitchOutcomeKind::Duplicate;
        return StitchOutcome::Duplicate;
    }

    // Dimension check (negligible, no timer)
    if frame.dimensions() != self.base_dims {
        self.last_metrics.outcome = StitchOutcomeKind::DimensionMismatch;
        return StitchOutcome::DimensionMismatch { ... };
    }

    // Motion estimation. Matcher fills coarse_us / template_ncc_us /
    // edge_projection_us / verifier_us / fallback_us / prepare_frame_us
    // plus counters, given &mut self.last_metrics.
    let motion = estimate_motion(..., &mut self.last_metrics);

    // Append
    {
        let _t = ScopedTimer::new(&mut self.last_metrics.append_us);
        let copied = self.canvas.append(...)?;
        self.last_metrics.append_copied_bytes = copied;
    }

    // Canvas state snapshot
    self.last_metrics.canvas_logical_pixels = self.canvas.logical_pixels();
    self.last_metrics.canvas_allocated_bytes = self.canvas.allocated_bytes();
    self.last_metrics.outcome = StitchOutcomeKind::Appended;
    StitchOutcome::Appended { ... }
}
```

Matcher sub-stages each get their own `ScopedTimer`. Counters
(`ncc_offsets_scored`, `ncc_pixel_visits`, etc.) are incremented inline at the
relevant loop sites.

### Canvas accessors (`canvas.rs`)

```rust
impl LinearCanvas {
    pub fn allocated_bytes(&self) -> u64 {
        self.canvas.as_raw().len() as u64
    }

    pub fn logical_pixels(&self) -> u64 {
        self.logical_width as u64 * self.logical_height as u64
    }
}
```

`append` is modified to return the count of bytes copied during the paste —
needed for the `append_copied_bytes` field.

### Bench runner binary (`benches/stitch_sequences.rs`)

Cargo configuration:

```toml
[[bench]]
name = "stitch_sequences"
harness = false

[dev-dependencies]
serde_json = { workspace = true }
clap = { version = "4", features = ["derive"] }
```

CLI:

```rust
#[derive(clap::Parser)]
struct Args {
    /// Comma-separated fixture names. Default: all registered scenarios.
    #[arg(long)]
    fixtures: Option<String>,

    /// Output JSONL path. Default: target/bench/stitch_sequences-<git-sha>-<utc>.jsonl
    #[arg(long)]
    out: Option<PathBuf>,

    /// Number of repetitions per fixture.
    #[arg(long, default_value_t = 5)]
    repeats: usize,

    /// Skip writing JSONL, only print summary table.
    #[arg(long)]
    no_jsonl: bool,

    /// Internal: run a single scenario in worker mode (used by orchestrator).
    #[arg(long, hide = true)]
    run_single_scenario: Option<String>,
}
```

Two execution modes:

- **Orchestrator** (default): enumerates scenarios, spawns one subprocess per
  scenario via `Command::new(env::current_exe()?)`, merges stdout JSONL into
  the output file.
- **Worker** (`--run-single-scenario <name>`): runs one scenario, writes JSONL
  to stdout, exits.

Subprocess-per-scenario is required for clean RSS measurement (see RSS
section). Spawn overhead (~10 ms × ~14 scenarios = ~140 ms total) is
negligible against ~30 s of actual bench work.

### Scenario registry

```rust
enum ScenarioSource {
    Fixture(PathBuf),               // existing tests/fixtures/linearscroll_v2/<family>/
    Synthetic(SyntheticSpec),       // long-image stress, generated in-memory
}

struct Scenario {
    name: String,
    source: ScenarioSource,
    config: StitchConfig,
    golden_output: Option<RgbaImage>,
}

fn bench_scenarios() -> Vec<Scenario> {
    let mut v = Vec::new();
    v.extend(existing_golden_scenarios());  // 11 from tests/fixtures/linearscroll_v2/
    v.extend(synthetic_stress_scenarios()); // 3 long-sequence stress scenarios
    v
}
```

Per-fixture configs mirror `golden_fixtures.rs` (the small-frame fixtures use
`max_search_ratio = 0.75`; sticky_header uses relaxed verifier thresholds).

### Synthetic stress scenarios (`benches/synthetic.rs`)

| Name | Frames | Frame size | Pattern | Targets |
|---|---:|---|---|---|
| `long_vertical_text` | 200 | 900×700 | Dense stripes via `make_scroll_canvas`, step 40 px | P1 append growth, P2 prepare cache, P3 NCC cost |
| `long_sticky_header` | 200 | 900×700 | Same + 80 px sticky top band | P1 + sticky behavior under long run |
| `long_vertical_jitter` | 200 | 900×700 | Step 40 px ± 2 px deterministic jitter | P7 subpixel later, baseline now |

```rust
struct SyntheticSpec {
    canvas_width: u32,
    canvas_height: u32,
    frame_width: u32,
    frame_height: u32,
    step_px: u32,
    step_jitter_px: i32,
    frame_count: usize,
    sticky_top_band: Option<u32>,
}

impl SyntheticSpec {
    fn frames(&self) -> impl Iterator<Item = RgbaImage> + '_ { /* ... */ }
}

fn deterministic_jitter(seed: u64, idx: usize, max_abs: i32) -> i32 {
    let h = seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(idx as u64);
    let h = (h ^ (h >> 32)) as i32;
    h.rem_euclid(2 * max_abs + 1) - max_abs
}
```

The base canvas is generated once via the existing
`tests::common::make_scroll_canvas`. Each frame is an `imageops::crop_imm` view
of that canvas, computed deterministically from `frame_idx`.

**No committed golden PNGs for synthetic scenarios.** Correctness for them is
asserted by:

1. Length: `canvas.logical_height == frame_count * step_px + frame_height`
   (within ±step_jitter_px slack).
2. No `NoMatch` outcomes for smooth-scroll cases (`long_vertical_text`,
   `long_sticky_header`).
3. Optional: pixel hash of `full_image()` recorded into JSONL summary record.
   Hash drift across PRs flips a single number — visible in `compare.py` but
   not a hard gate.

### Peak RSS measurement (`benches/rss.rs`)

```rust
#[cfg(target_os = "linux")]
pub fn read_rss_kb() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    s.lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.trim().split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
pub fn read_rss_kb() -> u64 {
    // Shell out to `ps` for first version — avoids libproc bindings.
    let pid = std::process::id();
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn read_rss_kb() -> u64 { 0 }  // Windows: explicit "not measured" sentinel
```

Sampling strategy:

- One baseline read at start of `run_scenario_worker`.
- Sample every 10 frames during the run.
- Final sample after the last frame.
- `peak_rss_kb_absolute = max(samples)`; `peak_rss_kb_delta = peak - baseline`.

Each scenario runs in its own subprocess so the baseline is clean.

### JSONL output format

Two record kinds, distinguished by `"kind"`:

```json
{"kind":"frame","scenario":"sticky_header","run":0,"frame":12,
 "git_sha":"76a806c","outcome":"Appended","total_us":3421,
 "duplicate_us":12,"prepare_frame_us":380,"coarse_us":210,
 "template_ncc_us":1880,"edge_projection_us":120,"verifier_us":220,
 "fallback_us":0,"append_us":599,
 "coarse_candidates":48,"ncc_offsets_scored":161,"ncc_pixel_visits":4128768,
 "verifier_candidates":3,"fallback_features_extracted":0,
 "canvas_logical_pixels":2520000,"canvas_allocated_bytes":10080000,
 "append_copied_bytes":1310720,
 "best_dx":0,"best_dy":42,"best_score":0.987,"second_best_score":0.912,
 "match_method":"TemplateNcc"}

{"kind":"summary","scenario":"sticky_header","run":0,"git_sha":"76a806c",
 "peak_rss_kb_delta":228352,"peak_rss_kb_absolute":421888,
 "total_frames":60,"appended":42,"duplicate":12,"nomatch":6,
 "final_canvas_logical_pixels":17640000,"final_canvas_allocated_bytes":70560000,
 "output_pixel_hash":"ab12cd34",
 "output_max_channel_diff":2,"output_mismatch_ratio":0.0003}
```

`output_max_channel_diff` and `output_mismatch_ratio` are present only for
scenarios with a committed golden (the 11 existing fixture families). Synthetic
stress scenarios omit these fields and rely on `output_pixel_hash` alone for
drift detection. `summarize.py` and `compare.py` treat absent fields as "no
golden comparison" rather than as zero.

`git_sha` injected via `build.rs`:

```rust
// crates/rollshot-core/build.rs
fn main() {
    let sha = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=ROLLSHOT_GIT_SHA={}", sha);
    println!("cargo:rerun-if-changed=.git/HEAD");
}
```

### `summarize.py` (`scripts/bench/summarize.py`)

Input: one JSONL path. Output: markdown to stdout.

Sections:
1. Run header (git SHA, timestamp, repeats).
2. Per-scenario summary table (frames / accept / duplicate / nomatch / p50 /
   p95 / p99 of `total_us` / peak RSS Δ).
3. Stage breakdown (p50 µs per stage, all scenarios in one table).
4. Output correctness (per scenario: byte-identical? max channel diff,
   mismatch ratio).

Implementation: Python stdlib only (`json`, `statistics`, `pathlib`, `sys`,
`collections.defaultdict`). Roughly 150 lines.

### `compare.py` (`scripts/bench/compare.py`)

Input: two JSONL paths (before, after). Output: markdown delta table.

Sections:
1. Comparison header (git SHAs).
2. Total time per frame (p50): before µs / after µs / Δ / Δ%.
3. Append time (p95): same shape — explicit because it's the primary P1 metric.
4. Stage breakdowns: per-stage delta tables (prepare / coarse / NCC / edge /
   verifier / fallback / append).
5. Peak RSS: before MB / after MB / Δ.
6. Regressions (rows with Δ > +5%): explicit list, with ✅ if none.
7. Output correctness: any scenario whose correctness status changed gets
   flagged.

Threshold (default ±5%) is a constant at the top of the script. Adjustable if
the noise floor turns out to be higher.

Implementation: Python stdlib only, ~200 lines.

### Test layer (`tests/metrics_population.rs`)

Integration tests that exercise the production code paths with existing
fixtures to verify metrics fields populate correctly. Critical assertions:

- `Appended` outcome populates `prepare_frame_us`, `coarse_us`,
  `template_ncc_us`, `verifier_us`, `append_us`, `canvas_logical_pixels`,
  `canvas_allocated_bytes`, `best_dy`.
- `Duplicate` outcome populates only `duplicate_us`; all other stage timings
  are 0; `canvas_*` unchanged from previous frame.
- `NoMatch` outcome does not record `append_us`; `canvas_allocated_bytes`
  unchanged.
- Stage-sum invariant: `duplicate_us + prepare_frame_us + coarse_us +
  template_ncc_us + edge_projection_us + verifier_us + fallback_us + append_us
  <= total_us`, and `>= total_us * 0.80` for an `Appended` outcome (at least
  80% of total time accounted for by stages).

Unit tests in `metrics.rs` cover `ScopedTimer` (writes on drop, writes on
early return, default-zero invariant on `StitchMetrics`).

Python tests in `scripts/bench/test_summarize.py` cover edge cases: empty
JSONL, single-record JSONL, missing fields, regression threshold formatting.

### Documentation (`docs/bench.md`)

Single doc page covering:
1. **What it measures** — stage names, what each ties to (P1 → `append_us`,
   P2 → `prepare_frame_us`, P3 → `template_ncc_us` + `ncc_pixel_visits`,
   etc.).
2. **Running locally** — `cargo bench --bench stitch_sequences`,
   `--fixtures sticky_header,long_vertical_text`, `--repeats N`, output
   location.
3. **PR workflow** — bench main, switch branch, bench again, run
   `compare.py`, paste markdown into PR description.
4. **Adding a scenario** — golden fixture → `golden_fixtures.rs`; bench
   scenario → `bench_scenarios()`; synthetic stress → `synthetic.rs`.
5. **Known limitations** — RSS is allocator-dependent; Windows doesn't measure
   RSS; subprocess-per-scenario means orchestrator startup cost is paid once.

`AGENTS.md` gets a short "Performance verification" subsection pointing at
`docs/bench.md`. `crates/rollshot-core/README.md` gets a one-line pointer.

## Data flow

```text
[fixture files or SyntheticSpec]
         │
         ▼
   Scenario { name, source, config, golden? }
         │
         ▼  (orchestrator subprocess per scenario)
   run_scenario_worker(scenario)
         │
         ├─→ Stitcher::push_frame(frame)
         │       └─→ writes self.last_metrics (per-stage timings, counters,
         │           canvas state, motion outcome)
         │
         ├─→ stitcher.last_metrics().clone() → frame record (JSONL stdout)
         ├─→ poll RSS every 10 frames
         │
         └─→ end-of-scenario summary record (JSONL stdout, includes peak RSS,
             output diff vs golden, pixel hash)
         │
         ▼  (orchestrator collects worker stdout)
   target/bench/stitch_sequences-<sha>-<utc>.jsonl
         │
         ▼  (manual)
   python scripts/bench/summarize.py <file>   →  markdown table to stdout
   python scripts/bench/compare.py a b        →  before/after markdown
         │
         ▼
   Paste into PR description.
```

## Edge cases

**Stitcher panics or returns unexpected outcome during a scenario.**
Worker captures the panic, writes a `{"kind":"error","scenario":...,
"frame":...,"message":...}` record, exits non-zero. Orchestrator records the
failure in its merged JSONL and continues with the next scenario. Comparison
script treats missing scenarios as "no data" — they don't silently disappear.

**First frame stage timings.** `FirstFrame` outcome: `duplicate_us`,
`prepare_frame_us` (no matcher invoked), `coarse_us`, `template_ncc_us`,
`edge_projection_us`, `verifier_us`, `fallback_us` are all 0; `append_us`
records the initial canvas allocation cost. Integration test asserts this.

**Synthetic scenario length consistency.** `SyntheticSpec` validates at
construction time that `canvas_height >= frame_height + (frame_count - 1) *
step_px + step_jitter_px.unsigned_abs() as u32`, panicking with a clear message
if not. Prevents silent clipping at the canvas edge that would skew
measurements.

**`build.rs` SHA in non-git working trees.** If `git rev-parse` fails (e.g.,
extracted tarball without `.git`), `ROLLSHOT_GIT_SHA` is set to `"unknown"`.
Comparison script handles `"unknown"` by skipping the SHA header line, not
erroring.

**Concurrent harness runs.** Two `cargo bench` invocations in parallel would
race on the default output path. Default filename includes UTC timestamp
(`stitch_sequences-<sha>-2026-05-26T14-30-15.jsonl`) to make this practically
impossible without explicit `--out` collision.

**RSS reads fail (sandboxed environment, `/proc/self/status` unreadable).**
`read_rss_kb()` returns 0. Summary record's `peak_rss_kb_*` fields are 0;
comparison script treats `0` deltas as "not measured" and omits the row from
the RSS table rather than reporting a misleading "0 MB peak".

**Worker subprocess hangs.** Orchestrator imposes a 5-minute per-scenario
timeout via `Child::wait_timeout` (use `wait-timeout` crate or polling loop).
Timeout produces an error record and continues.

**Stage instrumentation overhead distorts measurements.** Validated by the
integration test that asserts ≥80% of `total_us` is accounted for by named
stages — if instrumentation overhead grows, this invariant breaks and the
test fails. A separate one-off measurement (recorded in `docs/bench.md`)
confirms baseline overhead is ≤1% of `total_us` for the sticky_header
scenario.

**Output diff comparison cost.** Comparing a 200-frame `long_vertical_text`
final canvas (~140k × 900 = 126 MP) against a golden could itself take
seconds. Synthetic scenarios skip the per-pixel comparison and rely on the
hash; only the existing 11 fixture scenarios do byte-similar comparison.

## Testing strategy

| Layer | Tests | Location |
|---|---|---|
| ScopedTimer / StitchMetrics primitives | Writes on drop, writes on early return, default-zero invariant | `src/metrics.rs` (`#[cfg(test)]`) |
| Stage population | All outcomes correctly populate timings + counters; stage-sum invariant | `tests/metrics_population.rs` |
| Existing golden correctness | No regression in 11 golden fixtures | `tests/golden_fixtures.rs` (unchanged) |
| Bench runner mechanics | Manual: `cargo bench --bench stitch_sequences`, inspect JSONL | (manual) |
| Python tooling | Empty JSONL, single record, missing fields, regression threshold | `scripts/bench/test_summarize.py` |
| Instrumentation overhead | Stage-sum ≥80% of `total_us` for Appended outcomes | `tests/metrics_population.rs` |

All Rust tests run under `cargo test`. Python tests run under `pytest
scripts/bench/`. CI invokes both.

## Implementation order

Approximate sequencing for the implementation plan:

1. `metrics.rs` (`StitchMetrics`, `ScopedTimer`, `StitchOutcomeKind`) — pure
   addition, no production-code change. Unit tests.
2. `canvas.rs` accessors (`allocated_bytes`, `logical_pixels`,
   `append_copied_bytes` return) — pure addition.
3. `stitcher.rs` instrumentation — wires ScopedTimer + StitchMetrics
   population. Production tests (golden_fixtures) must still pass.
4. `matcher.rs` instrumentation — sub-stage timings + counters. Tests pass.
5. `tests/metrics_population.rs` — verifies fields populate.
6. `benches/rss.rs` + `benches/synthetic.rs` — helpers.
7. `benches/stitch_sequences.rs` — orchestrator + worker, scenario registry,
   JSONL output. End-to-end manual test.
8. `scripts/bench/summarize.py` + `compare.py` — Python tooling.
9. `scripts/bench/test_summarize.py` — Python tests.
10. `docs/bench.md` + `AGENTS.md` updates + README pointer.

Each step is independently verifiable and the production-code changes
(steps 1–4) leave production behavior unchanged.

## Estimated scope

| # | Component | Location | Lines |
|---|---|---|---:|
| 1 | StitchMetrics + ScopedTimer | `src/metrics.rs` | ~200 |
| 2 | Stage instrumentation | `src/{stitcher,matcher,canvas}.rs` | ~80 (additive) |
| 3 | Bench runner | `benches/stitch_sequences.rs` | ~400 |
| 4 | Synthetic scenarios | `benches/synthetic.rs` | ~150 |
| 5 | Python tooling | `scripts/bench/{summarize,compare}.py` | ~350 |
| 6 | RSS helper | `benches/rss.rs` | ~50 |
| 7 | Metrics tests | `tests/metrics_population.rs` + unit tests | ~150 |
| 8 | Documentation | `docs/bench.md`, AGENTS.md, README | ~200 |

**Total: ~1,580 lines. No production behavior change.**

## Open questions for future iterations

- **CI gate**: when (and whether) to wire `compare.py` into PR checks.
  Probably after P3 lands and we have a stable baseline to compare against.
- **criterion micro-benches**: useful for guiding P3 Fast NCC SIMD work.
  Add as a separate `benches/ncc_microbench.rs` when needed; not in scope here.
- **Cross-machine comparability**: pinning an allocator (`jemallocator`) for
  bench-only builds. Defer until "my numbers don't match yours" becomes a
  real complaint.
- **Real-capture fixture library**: archive of actual screenshot sequences
  from production users for replay. Out of scope; would require a separate
  storage and privacy plan.
