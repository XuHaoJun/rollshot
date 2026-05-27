# Benchmark Report Standardization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a standard YAML-frontmatter mode to benchmark compare reports so future optimization reports are small, committed, and easy to search.

**Architecture:** Keep raw JSONL as local artifacts and make `scripts/bench/compare.py` optionally prepend a fixed YAML frontmatter schema to the existing markdown body. The benchmark harness remains unchanged; docs and the existing P1 report are updated to the schema so P2 can reuse the convention.

**Tech Stack:** Python standard library (`argparse`, `json`, `platform`, `os`, `subprocess`), pytest-style script tests, Markdown docs.

---

## File Structure

- Modify `scripts/bench/compare.py`
  - Add optional frontmatter CLI flags.
  - Add best-effort environment and full-commit metadata helpers.
  - Preserve existing output when `--include-frontmatter` is not passed.
- Modify `scripts/bench/test_summarize.py`
  - Add focused tests for compare frontmatter output.
- Modify `docs/bench.md`
  - Document committed compare reports and the frontmatter workflow.
- Modify `bench-results/2026-05-27-p1-strip-canvas-compare.md`
  - Add `schema_version` and `roadmap_item` so the already committed P1 report matches the new schema.

## Task 1: Add Frontmatter Tests

**Files:**
- Modify: `scripts/bench/test_summarize.py`

- [ ] **Step 1: Add reusable compare fixture helpers**

Add these helpers below `_write_jsonl`:

```python
def _frame_record(git_sha, scenario="x"):
    return {
        "kind": "frame", "scenario": scenario, "run": 0, "frame": 0,
        "git_sha": git_sha, "outcome": "Appended", "no_match_reason": None,
        "total_us": 1000, "duplicate_us": 10, "prepare_frame_us": 100,
        "coarse_us": 50, "template_ncc_us": 600, "edge_projection_us": 0,
        "verifier_us": 100, "fallback_us": 0, "append_us": 140,
        "coarse_candidates": 5, "ncc_offsets_scored": 10, "ncc_pixel_visits": 1000,
        "verifier_candidates": 3, "fallback_features_extracted": 0,
        "canvas_logical_pixels": 100, "canvas_allocated_bytes": 400,
        "append_copied_bytes": 80, "best_dx": 0, "best_dy": 40,
        "best_score": 0.95, "second_best_score": None, "match_method": "Template",
    }


def _summary_record(git_sha, scenario="x"):
    return {
        "kind": "summary", "scenario": scenario, "run": 0, "git_sha": git_sha,
        "peak_rss_kb_delta": 1000, "peak_rss_kb_absolute": 200000,
        "total_frames": 1, "appended": 1, "duplicate": 0, "no_match": 0,
        "no_progress": 0, "axis_changed": 0,
        "final_canvas_logical_pixels": 100, "final_canvas_allocated_bytes": 400,
        "output_pixel_hash": "deadbeef",
        "output_max_channel_diff": 1, "output_mismatch_ratio": 0.001,
    }
```

- [ ] **Step 2: Add a frontmatter mode test**

Add this test near the existing compare tests:

```python
def test_compare_can_emit_frontmatter(tmp_path):
    before = tmp_path / "before.jsonl"
    after = tmp_path / "after.jsonl"
    _write_jsonl(before, [_frame_record("abc1234"), _summary_record("abc1234")])
    _write_jsonl(after, [_frame_record("def5678"), _summary_record("def5678")])

    result = compare.main([
        "--include-frontmatter",
        "--benchmark-id", "2026-05-27-p2-prepared-frame",
        "--benchmark-scope", "p2-prepared-frame",
        "--roadmap-item", "P2",
        "--status", "user_accepted",
        "--date", "2026-05-27",
        "--command", "rtk cargo bench -p rollshot-core --bench stitch_sequences -- --fixtures a,b --repeats 3 --out after.jsonl",
        "--fixtures", "a,b",
        "--repeats", "3",
        str(before),
        str(after),
    ])

    assert result.startswith("---\n")
    assert "kind: stitch_sequences_benchmark_compare\n" in result
    assert "schema_version: 1\n" in result
    assert "benchmark_id: 2026-05-27-p2-prepared-frame\n" in result
    assert "roadmap_item: P2\n" in result
    assert "status: user_accepted\n" in result
    assert "short_commit: abc1234\n" in result
    assert "short_commit: def5678\n" in result
    assert "    - a\n" in result
    assert "    - b\n" in result
    assert "# Benchmark comparison: abc1234 → def5678" in result
```

- [ ] **Step 3: Add a no-frontmatter compatibility test**

Add this test near the frontmatter test:

```python
def test_compare_without_frontmatter_keeps_original_header(tmp_path):
    before = tmp_path / "before.jsonl"
    after = tmp_path / "after.jsonl"
    _write_jsonl(before, [_frame_record("abc1234"), _summary_record("abc1234")])
    _write_jsonl(after, [_frame_record("def5678"), _summary_record("def5678")])

    result = compare.main([str(before), str(after)])

    assert result.startswith("# Benchmark comparison: abc1234 → def5678\n")
    assert "kind: stitch_sequences_benchmark_compare" not in result
```

- [ ] **Step 4: Run tests and verify RED**

Run:

```bash
rtk python3 -m pytest scripts/bench/test_summarize.py -q
```

Expected: FAIL because `compare.py` does not recognize `--include-frontmatter`.

## Task 2: Implement Compare Frontmatter Mode

**Files:**
- Modify: `scripts/bench/compare.py`

- [ ] **Step 1: Add standard-library imports**

Change the imports at the top of `scripts/bench/compare.py` to include:

```python
import argparse
import json
import os
import platform
import subprocess
import sys
from collections import defaultdict
```

- [ ] **Step 2: Add YAML rendering helpers**

Add these helpers below `delta_row`:

```python
def yaml_quote(value):
    text = str(value)
    escaped = text.replace("\\", "\\\\").replace('"', '\\"')
    return f'"{escaped}"'


def yaml_scalar(value):
    if isinstance(value, int):
        return str(value)
    text = str(value)
    if not text:
        return '""'
    safe = all(c.isalnum() or c in "-_./:" for c in text)
    return text if safe else yaml_quote(text)


def resolve_commit(short_sha):
    try:
        result = subprocess.run(
            ["git", "rev-parse", short_sha],
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return short_sha
    return result.stdout.strip() or short_sha


def cpu_model_name():
    try:
        result = subprocess.run(
            ["lscpu"],
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return "unknown"
    for line in result.stdout.splitlines():
        if line.startswith("Model name:"):
            return line.split(":", 1)[1].strip() or "unknown"
    return "unknown"


def run_environment():
    return {
        "os": platform.platform(),
        "architecture": platform.machine() or "unknown",
        "cpu_model": cpu_model_name(),
        "logical_cpus": os.cpu_count() or 0,
    }


def parse_fixtures(value):
    if not value:
        return []
    return [part.strip() for part in value.split(",") if part.strip()]
```

- [ ] **Step 3: Add `render_frontmatter`**

Add this function below `run_environment`:

```python
def render_frontmatter(args, before_sha, after_sha):
    env = run_environment()
    fixtures = parse_fixtures(args.fixtures)
    lines = [
        "---",
        "kind: stitch_sequences_benchmark_compare",
        "schema_version: 1",
        f"benchmark_id: {yaml_scalar(args.benchmark_id)}",
        f"benchmark_scope: {yaml_scalar(args.benchmark_scope)}",
        f"roadmap_item: {yaml_scalar(args.roadmap_item)}",
        f"status: {yaml_scalar(args.status)}",
        "before:",
        f"  short_commit: {yaml_scalar(before_sha)}",
        f"  commit: {yaml_scalar(resolve_commit(before_sha))}",
        f"  jsonl: {yaml_scalar(args.before)}",
        "after:",
        f"  short_commit: {yaml_scalar(after_sha)}",
        f"  commit: {yaml_scalar(resolve_commit(after_sha))}",
        f"  jsonl: {yaml_scalar(args.after)}",
        "run:",
        f"  date: {yaml_scalar(args.date)}",
        "  harness: crates/rollshot-core/benches/stitch_sequences.rs",
        f"  command: {yaml_scalar(args.command)}",
        "  fixtures:",
    ]
    if fixtures:
        lines.extend(f"    - {yaml_scalar(fixture)}" for fixture in fixtures)
    else:
        lines.append("    []")
    lines.extend([
        f"  repeats: {args.repeats}",
        "environment:",
        f"  os: {yaml_scalar(env['os'])}",
        f"  architecture: {yaml_scalar(env['architecture'])}",
        f"  cpu_model: {yaml_scalar(env['cpu_model'])}",
        f"  logical_cpus: {env['logical_cpus']}",
        "notes:",
        '  - "Raw JSONL files are local benchmark artifacts and are not intended to be committed."',
        '  - "Peak RSS is allocator- and machine-dependent; compare trends on this machine."',
        "---",
        "",
    ])
    return "\n".join(lines)
```

- [ ] **Step 4: Extend argument parsing**

Replace `main` with:

```python
def main(argv=None):
    p = argparse.ArgumentParser()
    p.add_argument("--include-frontmatter", action="store_true")
    p.add_argument("--benchmark-id", default="unknown")
    p.add_argument("--benchmark-scope", default="unknown")
    p.add_argument("--roadmap-item", default="unknown")
    p.add_argument("--status", default="draft")
    p.add_argument("--date", default="unknown")
    p.add_argument("--command", default="")
    p.add_argument("--fixtures", default="")
    p.add_argument("--repeats", type=int, default=0)
    p.add_argument("before")
    p.add_argument("after")
    args = p.parse_args(argv)
    bf, bs, b_sha = load(args.before)
    af, as_, a_sha = load(args.after)
    body = render(
        per_scenario_stats(bf),
        per_scenario_stats(af),
        bs,
        as_,
        b_sha,
        a_sha,
    )
    if not args.include_frontmatter:
        return body
    return render_frontmatter(args, b_sha, a_sha) + body
```

- [ ] **Step 5: Run tests and verify GREEN**

Run:

```bash
rtk python3 -m pytest scripts/bench/test_summarize.py -q
```

Expected: PASS.

- [ ] **Step 6: Commit script and test changes**

Run:

```bash
rtk git add scripts/bench/compare.py scripts/bench/test_summarize.py
rtk git commit -m "feat(bench): emit compare report metadata"
```

## Task 3: Document The Standard Workflow

**Files:**
- Modify: `docs/bench.md`

- [ ] **Step 1: Update PR workflow docs**

In `docs/bench.md`, replace the current step 3 under `## PR workflow` with this text:

```markdown
# 3. Compare. For normal PRs, paste the markdown into the PR description.
rtk python3 scripts/bench/compare.py \
    target/bench/before.jsonl target/bench/after.jsonl

# For accepted optimization reports that should be committed, emit YAML
# frontmatter and write the report under bench-results/.
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

- [ ] **Step 2: Add committed report convention**

After the paragraph that starts "The compare report flags", add:

````markdown
Committed benchmark reports use:

```text
bench-results/YYYY-MM-DD-<scope>-compare.md
```

Only commit the compare markdown by default. Raw JSONL files remain local
artifacts unless a reviewer explicitly asks to version them. Committed reports
start with YAML frontmatter containing `kind`, `schema_version`,
`benchmark_id`, `roadmap_item`, before/after commits, fixtures, repeats, and
CPU/OS context so reports can be found quickly with `rg` or parsed later.
````

- [ ] **Step 3: Run documentation check**

Run:

```bash
rtk sed -n '45,125p' docs/bench.md
```

Expected: the normal PR paste workflow and committed-report workflow are both documented.

- [ ] **Step 4: Commit docs**

Run:

```bash
rtk git add docs/bench.md
rtk git commit -m "docs(bench): document committed compare reports"
```

## Task 4: Upgrade The P1 Report To Schema v1

**Files:**
- Modify: `bench-results/2026-05-27-p1-strip-canvas-compare.md`

- [ ] **Step 1: Add missing frontmatter fields**

Update the existing frontmatter so the top fields are:

```yaml
---
kind: stitch_sequences_benchmark_compare
schema_version: 1
benchmark_id: 2026-05-27-p1-strip-canvas
benchmark_scope: p1-strip-canvas
roadmap_item: P1
status: user_accepted
```

Leave the existing before/after/run/environment/body content unchanged unless it is malformed.

- [ ] **Step 2: Verify the report is searchable**

Run:

```bash
rtk rg -n "schema_version: 1|roadmap_item: P1|benchmark_id: 2026-05-27-p1-strip-canvas" bench-results/2026-05-27-p1-strip-canvas-compare.md
```

Expected: all three fields are found.

- [ ] **Step 3: Commit report upgrade**

Run:

```bash
rtk git add bench-results/2026-05-27-p1-strip-canvas-compare.md
rtk git commit -m "docs(bench): mark P1 report schema version"
```

## Task 5: Final Verification

**Files:**
- Read only.

- [ ] **Step 1: Run Python tests**

Run:

```bash
rtk python3 -m pytest scripts/bench/test_summarize.py -q
```

Expected: PASS.

- [ ] **Step 2: Smoke-test compare output without frontmatter**

Run:

```bash
rtk python3 scripts/bench/compare.py \
  bench-results/2026-05-27-p1-strip-canvas-before-f404e61.jsonl \
  bench-results/2026-05-27-p1-strip-canvas-after.jsonl \
  | rtk sed -n '1,8p'
```

Expected: output starts with `# Benchmark comparison: f404e61 → d208cf5` and has no YAML frontmatter.

- [ ] **Step 3: Smoke-test compare output with frontmatter**

Run:

```bash
rtk python3 scripts/bench/compare.py \
  --include-frontmatter \
  --benchmark-id 2026-05-27-p1-strip-canvas \
  --benchmark-scope p1-strip-canvas \
  --roadmap-item P1 \
  --status user_accepted \
  --date 2026-05-27 \
  --command "rtk cargo bench -p rollshot-core --bench stitch_sequences -- --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter --repeats 3 --out bench-results/2026-05-27-p1-strip-canvas-after.jsonl" \
  --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter \
  --repeats 3 \
  bench-results/2026-05-27-p1-strip-canvas-before-f404e61.jsonl \
  bench-results/2026-05-27-p1-strip-canvas-after.jsonl \
  | rtk sed -n '1,40p'
```

Expected: output starts with `---`, includes `schema_version: 1`, includes `roadmap_item: P1`, and then reaches `# Benchmark comparison: f404e61 → d208cf5`.

- [ ] **Step 4: Check status**

Run:

```bash
rtk git status --short --untracked-files=all
```

Expected: no tracked-file modifications. Untracked raw JSONL artifacts may remain.

## Plan Self-Review

- Spec coverage:
  - Committed report filename convention: Task 3 docs and Task 4 existing report.
  - YAML frontmatter schema: Task 2 implementation and Task 4 upgrade.
  - Optional compare.py flags: Task 2.
  - No raw JSONL commits by default: Task 3 docs; raw artifacts are never staged in the plan.
  - No bench harness change: no task touches `crates/rollshot-core/benches/stitch_sequences.rs`.
  - Tests: Task 1 and Task 5.
- Placeholder scan:
  - No placeholder markers or incomplete implementation steps.
  - Example placeholder commit ids are confined to docs examples and explicitly shaped as examples.
- Type consistency:
  - CLI flags match the spec.
  - Frontmatter keys match the committed P1 report schema and future P2 workflow.
