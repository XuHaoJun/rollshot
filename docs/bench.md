# Stitching Bench Harness

End-to-end benchmark for `rollshot-core::Stitcher`. Produces per-frame
stage-level timings, algorithmic counters, peak RSS, and output-correctness
checks as JSONL — designed to be diffed before/after an optimization PR.

See the design spec at
`docs/superpowers/specs/2026-05-26-benchmark-harness-design.md` for the full
rationale.

## How it runs

```text
 cargo bench -p rollshot-core --bench stitch_sequences
        │
        ▼
 orchestrator (this binary)
 ├── enumerate scenarios (11 golden fixtures + 3 synthetic stress)
 ├── for each (scenario, run_index in 0..repeats):
 │       fork: stitch_sequences --run-single-scenario <name> --worker-run <i>
 │              │
 │              ▼ stdout (JSONL)
 │       append to bench-results/runs/stitch_sequences/stitch_sequences-<sha>-<ts>.jsonl
 │
 ▼
 python3 scripts/bench/summarize.py  (per-scenario report)
 python3 scripts/bench/compare.py    (delta vs baseline)
```

Subprocess-per-scenario is intentional: each worker process starts with a
fresh allocator state so `peak_rss_kb_delta` measures **this scenario's**
memory pressure rather than the high-water mark of all prior scenarios.

## What it measures

| Field | What it captures | Tracks which roadmap item |
|---|---|---|
| `total_us` | Wall-clock per `push_frame` | overall regression detector |
| `prepare_frame_us` | `to_grayscale` of prev+curr | **P2** PreparedFrame cache |
| `coarse_us` + `coarse_candidates` | Downsampled MAD search | coarse stage cost |
| `template_ncc_us` + `ncc_offsets_scored` + `ncc_pixel_visits` | NCC refine | **P3** Fast NCC + SIMD |
| `edge_projection_us` | Edge projection candidates | matcher path cost |
| `verifier_us` + `verifier_candidates` | PixelOverlapVerifier | verifier cost |
| `fallback_us` + `fallback_features_extracted` | FAST+KNN fallback | **P6** indexed feature fallback |
| `append_us` + `append_copied_bytes` | Canvas append | **P1** StripCanvas |
| `peak_rss_kb_delta` | Resident memory high-water (per scenario subprocess) | **P1** + **P2** memory targets |
| `output_max_channel_diff` + `output_mismatch_ratio` | Diff vs golden | correctness gate |
| `output_pixel_hash` | FNV-1a hash of full_image | drift detection on synthetic scenarios |

## Running locally

```bash
# Run all 14 scenarios (11 golden fixtures + 3 synthetic stress), 3 repeats each.
rtk cargo bench -p rollshot-core --bench stitch_sequences

# Subset.
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --fixtures sticky_header,long_vertical_text --repeats 3

# Custom output path.
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --out bench-results/runs/p2-prepared-frame/baseline.jsonl

# View the summary as markdown.
rtk python3 scripts/bench/summarize.py \
    bench-results/runs/stitch_sequences/stitch_sequences-*.jsonl
```

By default, JSONL filenames include the short git SHA and a UTC timestamp, e.g.
`bench-results/runs/stitch_sequences/stitch_sequences-a745845-1716732615.jsonl`.
Raw run artifacts live outside `target/` so they survive `cargo clean`, but
`bench-results/runs/` is gitignored.

## PR workflow

```bash
# 1. Capture baseline on main.
git checkout main
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --out bench-results/runs/p2-prepared-frame/before.jsonl

# 2. Switch to your branch and capture again.
git checkout my-branch
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
    --out bench-results/runs/p2-prepared-frame/after.jsonl

# 3. Compare. For normal PRs, paste the markdown into the PR description.
rtk python3 scripts/bench/compare.py \
    bench-results/runs/p2-prepared-frame/before.jsonl \
    bench-results/runs/p2-prepared-frame/after.jsonl

# For accepted optimization reports that should be committed, emit YAML
# frontmatter and write the report under bench-results/compare/.
rtk python3 scripts/bench/compare.py \
    --include-frontmatter \
    --benchmark-id 2026-05-27-p2-prepared-frame \
    --benchmark-scope p2-prepared-frame \
    --roadmap-item P2 \
    --status user_accepted \
    --date 2026-05-27 \
    --command "rtk cargo bench -p rollshot-core --bench stitch_sequences -- --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter --repeats 3 --out bench-results/runs/p2-prepared-frame/after.jsonl" \
    --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter \
    --repeats 3 \
    bench-results/runs/p2-prepared-frame/before-abc1234.jsonl \
    bench-results/runs/p2-prepared-frame/after.jsonl \
    > bench-results/compare/2026-05-27-p2-prepared-frame-compare.md
```

The compare report flags any scenario with Δ > +5% on `total_us` (p50),
`append_us` (p95), `prepare_frame_us` (p50), `template_ncc_us` (p50), or
`verifier_us` (p50). It also flags output-correctness drift via the pixel
hash. It includes p50 algorithmic counter deltas for `coarse_candidates`,
`ncc_offsets_scored`, `ncc_pixel_visits`, and `verifier_candidates`.

Committed benchmark reports use:

```text
bench-results/compare/YYYY-MM-DD-<scope>-compare.md
```

Only commit the compare markdown by default. Raw JSONL and summary files remain
local artifacts under `bench-results/runs/` unless a reviewer explicitly asks
to version them. Committed reports
start with YAML frontmatter containing `kind`, `schema_version`,
`benchmark_id`, `roadmap_item`, before/after commits, fixtures, repeats, and
CPU/OS context so reports can be found quickly with `rg` or parsed later.

## Adding a scenario

Two kinds of scenarios exist:

- **Golden fixtures** — `crates/rollshot-core/tests/fixtures/linearscroll_v2/<family>/`
  contain `frames/*.png` and `expected/output.png`. They double as correctness
  tests (driven by `crates/rollshot-core/tests/golden_fixtures.rs`) and bench
  scenarios. To add one: drop the fixture under the right directory, then add
  an entry in `existing_fixture_scenarios()` in
  `crates/rollshot-core/benches/stitch_sequences.rs` and a corresponding test
  invocation in `golden_fixtures.rs`.

- **Synthetic stress scenarios** — defined in
  `crates/rollshot-core/benches/synthetic.rs::default_specs()`. No fixture
  files; frames are generated at runtime. Used to expose scaling behavior the
  short golden fixtures can't.

## Known limitations

- **RSS is allocator-dependent.** Linux glibc `malloc` doesn't return memory
  to the OS aggressively, so absolute `peak_rss_kb_delta` values are platform-
  and allocator-specific. Trends across PRs on the same machine remain
  meaningful.
- **Windows reports 0 RSS** — Windows isn't currently a measurement target;
  the field is set to 0 as an explicit "not measured" sentinel.
- **Subprocess startup cost is per-scenario.** Each scenario runs in its own
  worker subprocess to get a clean RSS baseline. The orchestrator pays
  ~10 ms × N_scenarios × N_repeats spawn overhead, but workload time
  dominates for any non-trivial scenario.
- **3 repeats is the default tradeoff.** Enough for a usable p50/p95 without
  making local runs painful (the synthetic 200-frame scenarios dominate wall
  time). Override with `--repeats 10` for noisier scenarios, or `--repeats 1`
  for a quick smoke run.
- **No CI gate yet.** The bench is local-developer + PR-description-driven.
  If CI gating becomes necessary, it'd build on the same JSONL output.
