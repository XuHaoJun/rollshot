#!/usr/bin/env python3
"""Compare two stitch_sequences JSONL runs and emit a markdown delta report.

Usage:
    python3 scripts/bench/compare.py <before.jsonl> <after.jsonl>

Raw JSONL inputs normally live under bench-results/runs/<scope>/.
Accepted reports that are worth committing live under bench-results/compare/.
"""

import argparse
import json
import os
import platform
import subprocess
import sys
from collections import defaultdict

REGRESSION_THRESHOLD = 0.05  # ±5%


def load(path):
    frames = defaultdict(list)
    summaries = defaultdict(list)
    git_sha = "unknown"
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                continue
            if r.get("git_sha"):
                git_sha = r["git_sha"]
            if r.get("kind") == "frame":
                frames[r["scenario"]].append(r)
            elif r.get("kind") == "summary":
                summaries[r["scenario"]].append(r)
    return frames, summaries, git_sha


def quantile(values, q):
    if not values:
        return 0
    s = sorted(values)
    idx = max(0, min(len(s) - 1, int(round(q * (len(s) - 1)))))
    return s[idx]


def per_scenario_stats(frames):
    out = {}
    for scn, recs in frames.items():
        out[scn] = {
            "p50_total_us": quantile([r["total_us"] for r in recs], 0.50),
            "p95_total_us": quantile([r["total_us"] for r in recs], 0.95),
            "p95_append_us": quantile([r["append_us"] for r in recs], 0.95),
            "p50_prepare_us": quantile([r["prepare_frame_us"] for r in recs], 0.50),
            "p50_coarse_us": quantile([r["coarse_us"] for r in recs], 0.50),
            "p50_ncc_us": quantile([r["template_ncc_us"] for r in recs], 0.50),
            "p50_verifier_us": quantile([r["verifier_us"] for r in recs], 0.50),
            "p50_coarse_candidates": quantile([r["coarse_candidates"] for r in recs], 0.50),
            "p50_ncc_offsets_scored": quantile([r["ncc_offsets_scored"] for r in recs], 0.50),
            "p50_ncc_pixel_visits": quantile([r["ncc_pixel_visits"] for r in recs], 0.50),
            "p50_verifier_candidates": quantile([r["verifier_candidates"] for r in recs], 0.50),
        }
    return out


def delta_row(before, after):
    if before == 0:
        return ("n/a", None)
    diff = after - before
    pct = diff / before
    return (f"{pct * 100:+.1f}%", pct)


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
        '  - "Raw JSONL files are local benchmark artifacts under bench-results/runs/ and are not intended to be committed."',
        '  - "Peak RSS is allocator- and machine-dependent; compare trends on this machine."',
        "---",
        "",
    ])
    return "\n".join(lines)


def render(before_stats, after_stats, summaries_before, summaries_after, before_sha, after_sha):
    lines = []
    lines.append(f"# Benchmark comparison: {before_sha} → {after_sha}\n")

    keys = sorted(set(before_stats) | set(after_stats))

    def section(title, field, label):
        lines.append(f"## {title}")
        lines.append("")
        lines.append(f"| scenario | before µs | after µs | Δ | Δ% |")
        lines.append("|---|---:|---:|---:|---:|")
        regressions = []
        for scn in keys:
            b = before_stats.get(scn, {}).get(field, 0)
            a = after_stats.get(scn, {}).get(field, 0)
            pct_str, pct = delta_row(b, a)
            diff = a - b
            lines.append(
                f"| {scn} | {b:,} | {a:,} | {diff:+,} | {pct_str} |"
            )
            if pct is not None and pct > REGRESSION_THRESHOLD:
                regressions.append((scn, pct))
        lines.append("")
        return regressions

    all_regressions = []
    all_regressions.extend(section("Total time per frame (p50)", "p50_total_us", "p50 total"))
    all_regressions.extend(section("Append time (p95) — P1 target", "p95_append_us", "p95 append"))
    all_regressions.extend(section("Prepare (p50) — P2 target", "p50_prepare_us", "p50 prepare"))
    all_regressions.extend(section("Coarse (p50)", "p50_coarse_us", "p50 coarse"))
    all_regressions.extend(section("NCC (p50) — P3 target", "p50_ncc_us", "p50 ncc"))
    all_regressions.extend(section("Verifier (p50)", "p50_verifier_us", "p50 verifier"))

    lines.append("## Algorithmic counters (p50)")
    lines.append("")
    lines.append("| scenario | coarse candidates before | after | Δ | Δ% | NCC offsets before | after | Δ | Δ% | NCC pixel visits before | after | Δ | Δ% | verifier candidates before | after | Δ | Δ% |")
    lines.append("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
    counter_fields = [
        "p50_coarse_candidates",
        "p50_ncc_offsets_scored",
        "p50_ncc_pixel_visits",
        "p50_verifier_candidates",
    ]
    for scn in keys:
        cells = [scn]
        for field in counter_fields:
            b = before_stats.get(scn, {}).get(field, 0)
            a = after_stats.get(scn, {}).get(field, 0)
            pct_str, _pct = delta_row(b, a)
            diff = a - b
            cells.extend([f"{b:,}", f"{a:,}", f"{diff:+,}", pct_str])
        lines.append(f"| {' | '.join(cells)} |")
    lines.append("")

    lines.append("## Peak RSS Δ (kB)")
    lines.append("")
    lines.append("| scenario | before kB | after kB | Δ kB |")
    lines.append("|---|---:|---:|---:|")
    rss_keys = sorted(set(summaries_before) | set(summaries_after))
    for scn in rss_keys:
        b = max([s["peak_rss_kb_delta"] for s in summaries_before.get(scn, [])] + [0])
        a = max([s["peak_rss_kb_delta"] for s in summaries_after.get(scn, [])] + [0])
        lines.append(f"| {scn} | {b:,} | {a:,} | {a - b:+,} |")
    lines.append("")

    lines.append(f"## Regressions (Δ > +{REGRESSION_THRESHOLD * 100:.0f}%)")
    lines.append("")
    if not all_regressions:
        lines.append("(none) ✅")
    else:
        for scn, pct in sorted(all_regressions, key=lambda x: -x[1]):
            lines.append(f"- **{scn}**: {pct * 100:+.1f}%")
    lines.append("")

    # Correctness drift.
    lines.append("## Output correctness drift")
    lines.append("")
    lines.append("| scenario | before hash | after hash | diff? |")
    lines.append("|---|---|---|---|")
    for scn in rss_keys:
        b = summaries_before.get(scn, [])
        a = summaries_after.get(scn, [])
        if not b or not a:
            continue
        bh = b[0].get("output_pixel_hash", "")
        ah = a[0].get("output_pixel_hash", "")
        same = "same" if bh == ah else "**DIFFERENT**"
        lines.append(f"| {scn} | `{bh}` | `{ah}` | {same} |")
    lines.append("")
    return "\n".join(lines) + "\n"


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


if __name__ == "__main__":
    sys.stdout.write(main())
