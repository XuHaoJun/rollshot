"""pytest-style tests for summarize.py and compare.py edge cases."""

import json
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

import summarize  # noqa: E402
import compare    # noqa: E402


def _write_jsonl(path, records):
    with open(path, "w") as f:
        for r in records:
            f.write(json.dumps(r) + "\n")


def test_summarize_handles_empty_jsonl(tmp_path):
    p = tmp_path / "empty.jsonl"
    p.write_text("")
    result = summarize.main([str(p)])
    assert "no records" in result


def test_summarize_handles_single_frame(tmp_path):
    p = tmp_path / "single.jsonl"
    _write_jsonl(p, [
        {
            "kind": "frame", "scenario": "x", "run": 0, "frame": 0,
            "git_sha": "abc", "outcome": "Appended", "no_match_reason": None,
            "total_us": 1000, "duplicate_us": 10, "prepare_frame_us": 100,
            "coarse_us": 50, "template_ncc_us": 600, "edge_projection_us": 0,
            "verifier_us": 100, "fallback_us": 0, "append_us": 140,
            "coarse_candidates": 5, "ncc_offsets_scored": 10, "ncc_pixel_visits": 1000,
            "verifier_candidates": 3, "fallback_features_extracted": 0,
            "canvas_logical_pixels": 100, "canvas_allocated_bytes": 400,
            "append_copied_bytes": 80, "best_dx": 0, "best_dy": 40,
            "best_score": 0.95, "second_best_score": None, "match_method": "Template",
        },
        {
            "kind": "summary", "scenario": "x", "run": 0, "git_sha": "abc",
            "peak_rss_kb_delta": 1000, "peak_rss_kb_absolute": 200000,
            "total_frames": 1, "appended": 1, "duplicate": 0, "no_match": 0,
            "no_progress": 0, "axis_changed": 0,
            "final_canvas_logical_pixels": 100, "final_canvas_allocated_bytes": 400,
            "output_pixel_hash": "deadbeef",
            "output_max_channel_diff": 1, "output_mismatch_ratio": 0.001,
        },
    ])
    result = summarize.main([str(p)])
    assert "Bench summary" in result
    assert "abc" in result
    assert "| x |" in result


def test_summarize_skips_malformed_lines(tmp_path):
    p = tmp_path / "malformed.jsonl"
    p.write_text('{"kind":"frame","scenario":"x","run":0,"frame":0,"git_sha":"abc","outcome":"Appended",\n'
                 'not-json-at-all\n')
    # Don't crash; just warn to stderr.
    result = summarize.main([str(p)])
    assert "no records" in result or "Bench summary" in result


def test_compare_no_regressions(tmp_path):
    a = tmp_path / "a.jsonl"
    b = tmp_path / "b.jsonl"
    record = {
        "kind": "frame", "scenario": "x", "run": 0, "frame": 0,
        "git_sha": "abc", "outcome": "Appended", "no_match_reason": None,
        "total_us": 1000, "duplicate_us": 10, "prepare_frame_us": 100,
        "coarse_us": 50, "template_ncc_us": 600, "edge_projection_us": 0,
        "verifier_us": 100, "fallback_us": 0, "append_us": 140,
        "coarse_candidates": 5, "ncc_offsets_scored": 10, "ncc_pixel_visits": 1000,
        "verifier_candidates": 3, "fallback_features_extracted": 0,
        "canvas_logical_pixels": 100, "canvas_allocated_bytes": 400,
        "append_copied_bytes": 80, "best_dx": 0, "best_dy": 40,
        "best_score": 0.95, "second_best_score": None, "match_method": "Template",
    }
    summary = {
        "kind": "summary", "scenario": "x", "run": 0, "git_sha": "abc",
        "peak_rss_kb_delta": 1000, "peak_rss_kb_absolute": 200000,
        "total_frames": 1, "appended": 1, "duplicate": 0, "no_match": 0,
        "no_progress": 0, "axis_changed": 0,
        "final_canvas_logical_pixels": 100, "final_canvas_allocated_bytes": 400,
        "output_pixel_hash": "deadbeef",
        "output_max_channel_diff": 1, "output_mismatch_ratio": 0.001,
    }
    _write_jsonl(a, [record, summary])
    _write_jsonl(b, [record, summary])
    result = compare.main([str(a), str(b)])
    assert "(none) ✅" in result


def test_compare_detects_regression(tmp_path):
    a = tmp_path / "a.jsonl"
    b = tmp_path / "b.jsonl"
    base = {
        "kind": "frame", "scenario": "x", "run": 0, "frame": 0,
        "git_sha": "old", "outcome": "Appended", "no_match_reason": None,
        "total_us": 1000, "duplicate_us": 10, "prepare_frame_us": 100,
        "coarse_us": 50, "template_ncc_us": 600, "edge_projection_us": 0,
        "verifier_us": 100, "fallback_us": 0, "append_us": 140,
        "coarse_candidates": 5, "ncc_offsets_scored": 10, "ncc_pixel_visits": 1000,
        "verifier_candidates": 3, "fallback_features_extracted": 0,
        "canvas_logical_pixels": 100, "canvas_allocated_bytes": 400,
        "append_copied_bytes": 80, "best_dx": 0, "best_dy": 40,
        "best_score": 0.95, "second_best_score": None, "match_method": "Template",
    }
    summary = {
        "kind": "summary", "scenario": "x", "run": 0, "git_sha": "old",
        "peak_rss_kb_delta": 1000, "peak_rss_kb_absolute": 200000,
        "total_frames": 1, "appended": 1, "duplicate": 0, "no_match": 0,
        "no_progress": 0, "axis_changed": 0,
        "final_canvas_logical_pixels": 100, "final_canvas_allocated_bytes": 400,
        "output_pixel_hash": "deadbeef",
        "output_max_channel_diff": 1, "output_mismatch_ratio": 0.001,
    }
    _write_jsonl(a, [base, summary])

    slow = dict(base)
    slow["total_us"] = 1100  # +10%
    slow_summary = dict(summary)
    slow_summary["git_sha"] = "new"
    slow["git_sha"] = "new"
    _write_jsonl(b, [slow, slow_summary])

    result = compare.main([str(a), str(b)])
    assert "+10.0%" in result
    assert "(none) ✅" not in result
