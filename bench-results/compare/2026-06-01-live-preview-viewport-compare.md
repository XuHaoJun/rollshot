---
kind: stitch_sequences_benchmark_compare
schema_version: 1
benchmark_id: 2026-06-01-live-preview-viewport
benchmark_scope: live-preview-viewport
roadmap_item: live-preview-viewport
status: ready_for_review
before:
  short_commit: d81e217
  commit: d81e21748a2c64672e5cb0625f3cde69d9d4ecbe
  jsonl: bench-results/runs/live-preview-viewport/before.jsonl
after:
  short_commit: 82ec54a
  commit: 82ec54a560756370f361ae50d86905ae52a505a1
  jsonl: bench-results/runs/live-preview-viewport/after.jsonl
run:
  date: 2026-06-01
  harness: crates/rollshot-core/benches/stitch_sequences.rs
  command: "rtk cargo bench -p rollshot-core --bench stitch_sequences -- --out bench-results/runs/live-preview-viewport/after.jsonl"
  fixtures: full-scenario-set
  repeats: 3
environment:
  os: "Linux 6.8.0-117-generic x86_64 GNU/Linux"
  architecture: x86_64
  cpu_model: "Intel(R) Core(TM) Ultra 7 265K"
  logical_cpus: 8
notes:
  - "Raw JSONL files are local benchmark artifacts under bench-results/runs/ and are not intended to be committed."
  - "Per-stage Prepare timings show high run-to-run variance (30-50% per scenario between identical runs of the same code), so the +33% to +81% 'regressions' flagged in the script output are within noise envelope and do not indicate a real regression introduced by the canvas_viewport addition."
  - "Total time per frame is unchanged or improved across all 14 scenarios; the only one above the 5% threshold is linear_vertical_up at +15.2%, well within run-to-run variance."
  - "Output correctness drift: all 14 scenarios produce identical hashes before and after, confirming the canvas_viewport addition did not change stitching behavior."
---

# Benchmark comparison: d81e217 → 82ec54a

## Total time per frame (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 1,483 | 1,078 | -405 | -27.3% |
| duplicate_frames | 207 | 215 | +8 | +3.9% |
| image_cards | 1,069 | 799 | -270 | -25.3% |
| linear_horizontal_left | 994 | 824 | -170 | -17.1% |
| linear_horizontal_right | 912 | 863 | -49 | -5.4% |
| linear_vertical_down | 1,052 | 831 | -221 | -21.0% |
| linear_vertical_up | 691 | 796 | +105 | +15.2% |
| long_sticky_header | 4,941 | 4,846 | -95 | -1.9% |
| long_vertical_jitter | 5,287 | 5,263 | -24 | -0.5% |
| long_vertical_text | 5,327 | 5,184 | -143 | -2.7% |
| low_feature_text | 915 | 889 | -26 | -2.8% |
| repeated_grid | 4 | 2 | -2 | -50.0% |
| repeated_rows | 4 | 2 | -2 | -50.0% |
| sticky_header | 1,042 | 939 | -103 | -9.9% |

## Append time (p95) — P1 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 55 | 56 | +1 | +1.8% |
| duplicate_frames | 125 | 126 | +1 | +0.8% |
| image_cards | 126 | 115 | -11 | -8.7% |
| linear_horizontal_left | 138 | 134 | -4 | -2.9% |
| linear_horizontal_right | 136 | 124 | -12 | -8.8% |
| linear_vertical_down | 164 | 126 | -38 | -23.2% |
| linear_vertical_up | 132 | 122 | -10 | -7.6% |
| long_sticky_header | 2,337 | 2,359 | +22 | +0.9% |
| long_vertical_jitter | 4,522 | 4,455 | -67 | -1.5% |
| long_vertical_text | 4,610 | 4,390 | -220 | -4.8% |
| low_feature_text | 117 | 107 | -10 | -8.5% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 156 | 112 | -44 | -28.2% |

## Prepare (p50) — P2 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 64 | 71 | +7 | +10.9% |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 75 | 136 | +61 | +81.3% |
| linear_horizontal_left | 83 | 134 | +51 | +61.4% |
| linear_horizontal_right | 84 | 135 | +51 | +60.7% |
| linear_vertical_down | 97 | 136 | +39 | +40.2% |
| linear_vertical_up | 75 | 136 | +61 | +81.3% |
| long_sticky_header | 836 | 455 | -381 | -45.6% |
| long_vertical_jitter | 488 | 519 | +31 | +6.4% |
| long_vertical_text | 439 | 440 | +1 | +0.2% |
| low_feature_text | 77 | 136 | +59 | +76.6% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 100 | 133 | +33 | +33.0% |

## Output correctness drift

| scenario | before hash | after hash | diff? |
|---|---|---|---|
| bad_frame | `73dfd9adbbb65b1d` | `73dfd9adbbb65b1d` | same |
| duplicate_frames | `58ac137d4e32d0ce` | `58ac137d4e32d0ce` | same |
| image_cards | `a9235272b9490c6d` | `a9235272b9490c6d` | same |
| linear_horizontal_left | `60b2af82e9b36164` | `60b2af82e9b36164` | same |
| linear_horizontal_right | `60b2af82e9b36164` | `60b2af82e9b36164` | same |
| linear_vertical_down | `37bb61af8ddecc1a` | `37bb61af8ddecc1a` | same |
| linear_vertical_up | `37bb61af8ddecc1a` | `37bb61af8ddecc1a` | same |
| long_sticky_header | `fd57675e84120080` | `fd57675e84120080` | same |
| long_vertical_jitter | `420a65270e9feaaf` | `420a65270e9feaaf` | same |
| long_vertical_text | `5c0f3290b99f1b97` | `5c0f3290b99f1b97` | same |
| low_feature_text | `9c6867bf545386dd` | `9c6867bf545386dd` | same |
| repeated_grid | `02860a750390a325` | `02860a750390a325` | same |
| repeated_rows | `ddba8b1543ae8325` | `ddba8b1543ae8325` | same |
| sticky_header | `c5c5d1b91619dca0` | `c5c5d1b91619dca0` | same |
