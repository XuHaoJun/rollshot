# Benchmark Report Standardization Design

Date: 2026-05-27

## Goal

Standardize committed benchmark comparison reports so future optimization work
can leave small, searchable performance records in the repository without
committing raw benchmark JSONL artifacts.

This is intended for P2 and later items in
`docs/stitching-rollshot-optimizations-2.md`, starting with prepared-frame cache
work. The standard should make it easy to quickly find reports by roadmap item,
benchmark id, before/after commits, fixture set, and machine context.

## Decision

Commit only the human-readable compare report:

```text
bench-results/YYYY-MM-DD-<scope>-compare.md
```

Examples:

```text
bench-results/2026-05-27-p1-strip-canvas-compare.md
bench-results/2026-05-27-p2-prepared-frame-compare.md
```

Do not commit raw JSONL by default. The report may reference local JSONL paths
for traceability, but those files remain local artifacts unless the user
explicitly asks to version them.

## Report Frontmatter

Every committed compare report should start with YAML frontmatter:

```yaml
---
kind: stitch_sequences_benchmark_compare
schema_version: 1
benchmark_id: 2026-05-27-p2-prepared-frame
benchmark_scope: p2-prepared-frame
roadmap_item: P2
status: user_accepted
before:
  short_commit: abc1234
  commit: abc1234abc1234abc1234abc1234abc1234abc1234
  jsonl: bench-results/2026-05-27-p2-prepared-frame-before-abc1234.jsonl
after:
  short_commit: def5678
  commit: def5678def5678def5678def5678def5678def5678
  jsonl: bench-results/2026-05-27-p2-prepared-frame-after.jsonl
run:
  date: 2026-05-27
  harness: crates/rollshot-core/benches/stitch_sequences.rs
  command: "rtk cargo bench -p rollshot-core --bench stitch_sequences -- --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter --repeats 3 --out bench-results/2026-05-27-p2-prepared-frame-after.jsonl"
  fixtures:
    - long_vertical_text
    - long_sticky_header
    - long_vertical_jitter
  repeats: 3
environment:
  os: "Linux 6.8.0-117-generic x86_64 GNU/Linux"
  architecture: x86_64
  cpu_model: "Intel(R) Core(TM) Ultra 7 265K"
  logical_cpus: 8
notes:
  - "Raw JSONL files are local benchmark artifacts and are not intended to be committed."
  - "Peak RSS is allocator- and machine-dependent; compare trends on this machine."
---
```

Fields:

- `kind`: fixed discriminator for simple search and future parsing.
- `schema_version`: starts at `1`; increment only for incompatible schema
  changes.
- `benchmark_id`: filename stem without `-compare.md`.
- `benchmark_scope`: short stable label for the optimization being measured.
- `roadmap_item`: roadmap priority such as `P1` or `P2`.
- `status`: expected values are `draft`, `reviewed`, or `user_accepted`.
- `before` / `after`: short and full commit ids plus local JSONL paths.
- `run`: benchmark date, harness, command, fixtures, repeats.
- `environment`: enough CPU/OS context to interpret local timings.
- `notes`: short caveats that affect interpretation.

The markdown body after frontmatter remains the existing comparison report body
from `scripts/bench/compare.py`: stage timing tables, RSS table, regression
summary, and output correctness drift.

## Script Behavior

Update `scripts/bench/compare.py` so it can optionally emit this frontmatter.
The benchmark harness itself does not need to change.

Add optional flags:

```text
--include-frontmatter
--benchmark-id <id>
--benchmark-scope <scope>
--roadmap-item <P1|P2|Pn>
--status <draft|reviewed|user_accepted>
--date <YYYY-MM-DD>
--command <command string>
--fixtures <comma-separated list>
--repeats <n>
```

Behavior:

- Without `--include-frontmatter`, output remains byte-for-byte compatible
  except for intentional script fixes.
- With `--include-frontmatter`, prepend the YAML block before the existing
  markdown report.
- The script continues to read before/after short commit ids from JSONL
  `git_sha` fields.
- Full commit ids are resolved with `git rev-parse <short>` when available; if
  resolution fails, use the short id as the `commit` value.
- Environment metadata is captured on the machine running the script:
  - `os`: `platform.platform()` or equivalent stable OS string.
  - `architecture`: `platform.machine()`.
  - `cpu_model`: best-effort Linux `lscpu` model name; fallback `"unknown"`.
  - `logical_cpus`: `os.cpu_count()`; fallback `0`.
- The frontmatter should use `yaml`-safe plain scalars or quoted strings. Do
  not add a PyYAML dependency; render the small fixed schema directly.

## Workflow

For a future P2 run:

```bash
rtk cargo bench -p rollshot-core --bench stitch_sequences -- \
  --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter \
  --repeats 3 \
  --out bench-results/2026-05-27-p2-prepared-frame-after.jsonl

rtk python3 scripts/bench/compare.py \
  --include-frontmatter \
  --benchmark-id 2026-05-27-p2-prepared-frame \
  --benchmark-scope p2-prepared-frame \
  --roadmap-item P2 \
  --status user_accepted \
  --date 2026-05-27 \
  --command "rtk cargo bench -p rollshot-core --bench stitch_sequences -- --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter --repeats 3 --out bench-results/2026-05-27-p2-prepared-frame-after.jsonl" \
  --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter \
  --repeats 3 \
  bench-results/2026-05-27-p2-prepared-frame-before-abc1234.jsonl \
  bench-results/2026-05-27-p2-prepared-frame-after.jsonl \
  > bench-results/2026-05-27-p2-prepared-frame-compare.md
```

Then commit only the compare markdown unless the user explicitly requests raw
artifacts.

## Tests

Extend `scripts/bench/test_summarize.py` or add a focused compare test file for
`scripts/bench/compare.py`.

Minimum coverage:

- Existing compare output without `--include-frontmatter` still includes the
  benchmark tables.
- Frontmatter mode starts with `---`, includes fixed keys, and then preserves
  the existing markdown body.
- `--fixtures a,b,c` renders a YAML list.
- Missing full commit resolution does not crash the script.

## Non-Goals

- No benchmark registry or index file.
- No raw JSONL retention policy beyond "do not commit by default".
- No changes to `crates/rollshot-core/benches/stitch_sequences.rs`.
- No CI benchmark gate.
- No PyYAML or other new dependency.

## Risks

The main risk is metadata drift if users manually edit frontmatter. Generating
frontmatter from `compare.py` reduces that risk while keeping the workflow
lightweight.

The second risk is treating local benchmark reports as universally comparable.
The frontmatter should keep machine context visible, and reports should still
be interpreted as local before/after comparisons, not absolute performance
claims across machines.
