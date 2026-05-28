#!/usr/bin/env python3
"""Summarize a stitch_sequences JSONL into a markdown report.

Usage:
    python3 scripts/bench/summarize.py <jsonl-path>

Raw JSONL files normally live under bench-results/runs/<scope>/.
"""

import argparse
import json
import sys
from collections import defaultdict


def load_records(path):
    frames = defaultdict(list)         # (scenario, run) -> [frame_record]
    summaries = defaultdict(list)      # scenario -> [summary_record]
    errors = []
    with open(path) as f:
        for line_no, line in enumerate(f, 1):
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except json.JSONDecodeError as e:
                print(f"warning: line {line_no} not JSON: {e}", file=sys.stderr)
                continue
            kind = rec.get("kind")
            if kind == "frame":
                frames[(rec["scenario"], rec["run"])].append(rec)
            elif kind == "summary":
                summaries[rec["scenario"]].append(rec)
            elif kind == "error":
                errors.append(rec)
    return frames, summaries, errors


def quantile(values, q):
    if not values:
        return 0
    s = sorted(values)
    idx = max(0, min(len(s) - 1, int(round(q * (len(s) - 1)))))
    return s[idx]


def aggregate_per_scenario(frames):
    """scenario -> dict of aggregated metrics across all runs+frames."""
    out = {}
    by_scenario = defaultdict(list)
    for (scn, _run), recs in frames.items():
        by_scenario[scn].extend(recs)
    for scn, recs in by_scenario.items():
        if not recs:
            continue
        total_us = [r["total_us"] for r in recs]
        out[scn] = {
            "frames": len(recs),
            "appended": sum(1 for r in recs if r["outcome"] == "Appended"),
            "duplicate": sum(1 for r in recs if r["outcome"] == "Duplicate"),
            "no_match": sum(1 for r in recs if r["outcome"] == "NoMatch"),
            "no_progress": sum(1 for r in recs if r["outcome"] == "NoProgress"),
            "axis_changed": sum(1 for r in recs if r["outcome"] == "AxisChanged"),
            "p50_total_us": quantile(total_us, 0.50),
            "p95_total_us": quantile(total_us, 0.95),
            "p99_total_us": quantile(total_us, 0.99),
            "p50_prepare_us": quantile([r["prepare_frame_us"] for r in recs], 0.50),
            "p50_coarse_us": quantile([r["coarse_us"] for r in recs], 0.50),
            "p50_pyramid_us": quantile([r.get("pyramid_us", 0) for r in recs], 0.50),
            "p50_ncc_us": quantile([r["template_ncc_us"] for r in recs], 0.50),
            "p50_edge_us": quantile([r["edge_projection_us"] for r in recs], 0.50),
            "p50_verifier_us": quantile([r["verifier_us"] for r in recs], 0.50),
            "p50_fallback_us": quantile([r["fallback_us"] for r in recs], 0.50),
            "p50_append_us": quantile([r["append_us"] for r in recs], 0.50),
            "p95_append_us": quantile([r["append_us"] for r in recs], 0.95),
        }
    return out


def render_markdown(agg, summaries):
    lines = []
    if not agg:
        return "no records\n"

    # Header line — use the first frame record's git_sha if any.
    any_summary = next(iter(summaries.values()), [])
    git_sha = any_summary[0]["git_sha"] if any_summary else "unknown"
    lines.append(f"# Bench summary — {git_sha}\n")

    lines.append("## Per-scenario totals")
    lines.append("")
    lines.append(
        "| scenario | frames | appended | duplicate | nomatch | p50 µs | p95 µs | p99 µs | peak RSS Δ kB |"
    )
    lines.append("|---|---:|---:|---:|---:|---:|---:|---:|---:|")
    for scn, m in sorted(agg.items()):
        rss_deltas = [s["peak_rss_kb_delta"] for s in summaries.get(scn, [])]
        rss = max(rss_deltas) if rss_deltas else 0
        lines.append(
            f"| {scn} | {m['frames']} | {m['appended']} | {m['duplicate']} "
            f"| {m['no_match']} | {m['p50_total_us']:,} | {m['p95_total_us']:,} "
            f"| {m['p99_total_us']:,} | {rss:,} |"
        )
    lines.append("")

    lines.append("## Stage breakdown (p50 µs)")
    lines.append("")
    lines.append(
        "| scenario | prepare | coarse | pyramid | ncc | edge | verifier | fallback | append |"
    )
    lines.append("|---|---:|---:|---:|---:|---:|---:|---:|---:|")
    for scn, m in sorted(agg.items()):
        lines.append(
            f"| {scn} | {m['p50_prepare_us']:,} | {m['p50_coarse_us']:,} | "
            f"{m['p50_pyramid_us']:,} | {m['p50_ncc_us']:,} | {m['p50_edge_us']:,} | "
            f"{m['p50_verifier_us']:,} | {m['p50_fallback_us']:,} | {m['p50_append_us']:,} |"
        )
    lines.append("")

    lines.append("## Output correctness (golden-fixture scenarios only)")
    lines.append("")
    lines.append("| scenario | max channel diff | mismatch ratio |")
    lines.append("|---|---:|---:|")
    for scn, recs in sorted(summaries.items()):
        if not recs:
            continue
        diffs = [r.get("output_max_channel_diff") for r in recs if r.get("output_max_channel_diff") is not None]
        ratios = [r.get("output_mismatch_ratio") for r in recs if r.get("output_mismatch_ratio") is not None]
        if not diffs:
            continue
        lines.append(
            f"| {scn} | {max(diffs)} | {max(ratios):.4%} |"
        )
    lines.append("")
    return "\n".join(lines) + "\n"


def main(argv=None):
    p = argparse.ArgumentParser()
    p.add_argument("path", help="Path to the JSONL file emitted by stitch_sequences.")
    args = p.parse_args(argv)
    frames, summaries, errors = load_records(args.path)
    if not frames and not summaries:
        return "no records\n"
    output = render_markdown(aggregate_per_scenario(frames), summaries)
    if errors:
        output += "\n## Errors\n\n"
        for e in errors:
            output += f"- `{e['scenario']}` run {e['run']}: {e['message']}\n"
    return output


if __name__ == "__main__":
    sys.stdout.write(main())
