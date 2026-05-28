---
kind: stitch_sequences_benchmark_compare
schema_version: 1
benchmark_id: 2026-05-28-p5-true-image-pyramid
benchmark_scope: p5-true-image-pyramid
roadmap_item: P5
status: draft
before:
  short_commit: 2dc1ecd
  commit: 2dc1ecdcc26c5ea285daee367b01a27b5639b858
  jsonl: bench-results/runs/p5-pyramid/before.jsonl
after:
  short_commit: 0c285c7
  commit: 0c285c78d7b8ad699ae521fd90439bdbf66d4665
  jsonl: bench-results/runs/p5-pyramid/after.jsonl
run:
  date: 2026-05-28
  harness: crates/rollshot-core/benches/stitch_sequences.rs
  command: ""
  fixtures:
    []
  repeats: 3
environment:
  os: Linux-6.8.0-117-generic-x86_64-with-glibc2.39
  architecture: x86_64
  cpu_model: "Intel(R) Core(TM) Ultra 7 265K"
  logical_cpus: 8
notes:
  - "Raw JSONL files are local benchmark artifacts under bench-results/runs/ and are not intended to be committed."
  - "Peak RSS is allocator- and machine-dependent; compare trends on this machine."
---
# Benchmark comparison: 2dc1ecd → 0c285c7

## Total time per frame (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 882 | 2,038 | +1,156 | +131.1% |
| duplicate_frames | 153 | 150 | -3 | -2.0% |
| image_cards | 802 | 660 | -142 | -17.7% |
| linear_horizontal_left | 728 | 664 | -64 | -8.8% |
| linear_horizontal_right | 941 | 739 | -202 | -21.5% |
| linear_vertical_down | 818 | 733 | -85 | -10.4% |
| linear_vertical_up | 696 | 630 | -66 | -9.5% |
| long_sticky_header | 4,757 | 4,920 | +163 | +3.4% |
| long_vertical_jitter | 4,964 | 5,583 | +619 | +12.5% |
| long_vertical_text | 5,120 | 5,243 | +123 | +2.4% |
| low_feature_text | 809 | 628 | -181 | -22.4% |
| repeated_grid | 1 | 1 | +0 | +0.0% |
| repeated_rows | 2 | 1 | -1 | -50.0% |
| sticky_header | 927 | 769 | -158 | -17.0% |

## Append time (p95) — P1 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 46 | 43 | -3 | -6.5% |
| duplicate_frames | 109 | 40 | -69 | -63.3% |
| image_cards | 127 | 43 | -84 | -66.1% |
| linear_horizontal_left | 138 | 49 | -89 | -64.5% |
| linear_horizontal_right | 138 | 58 | -80 | -58.0% |
| linear_vertical_down | 141 | 77 | -64 | -45.4% |
| linear_vertical_up | 135 | 46 | -89 | -65.9% |
| long_sticky_header | 2,472 | 2,286 | -186 | -7.5% |
| long_vertical_jitter | 4,032 | 4,343 | +311 | +7.7% |
| long_vertical_text | 4,600 | 4,553 | -47 | -1.0% |
| low_feature_text | 123 | 37 | -86 | -69.9% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 121 | 40 | -81 | -66.9% |

## Prepare (p50) — P2 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 69 | 61 | -8 | -11.6% |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 83 | 75 | -8 | -9.6% |
| linear_horizontal_left | 85 | 75 | -10 | -11.8% |
| linear_horizontal_right | 101 | 80 | -21 | -20.8% |
| linear_vertical_down | 107 | 77 | -30 | -28.0% |
| linear_vertical_up | 79 | 76 | -3 | -3.8% |
| long_sticky_header | 441 | 849 | +408 | +92.5% |
| long_vertical_jitter | 438 | 869 | +431 | +98.4% |
| long_vertical_text | 440 | 458 | +18 | +4.1% |
| low_feature_text | 101 | 76 | -25 | -24.8% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 97 | 75 | -22 | -22.7% |

## Coarse (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 150 | 160 | +10 | +6.7% |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 148 | 129 | -19 | -12.8% |
| linear_horizontal_left | 133 | 135 | +2 | +1.5% |
| linear_horizontal_right | 159 | 131 | -28 | -17.6% |
| linear_vertical_down | 160 | 156 | -4 | -2.5% |
| linear_vertical_up | 125 | 120 | -5 | -4.0% |
| long_sticky_header | 682 | 795 | +113 | +16.6% |
| long_vertical_jitter | 693 | 833 | +140 | +20.2% |
| long_vertical_text | 724 | 706 | -18 | -2.5% |
| low_feature_text | 160 | 119 | -41 | -25.6% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 165 | 121 | -44 | -26.7% |

## Pyramid (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 0 | 974 | +974 | n/a |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 0 | 0 | +0 | n/a |
| linear_horizontal_left | 0 | 0 | +0 | n/a |
| linear_horizontal_right | 0 | 0 | +0 | n/a |
| linear_vertical_down | 0 | 0 | +0 | n/a |
| linear_vertical_up | 0 | 0 | +0 | n/a |
| long_sticky_header | 0 | 0 | +0 | n/a |
| long_vertical_jitter | 0 | 0 | +0 | n/a |
| long_vertical_text | 0 | 0 | +0 | n/a |
| low_feature_text | 0 | 0 | +0 | n/a |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 0 | 0 | +0 | n/a |

## NCC (p50) — P3 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 298 | 330 | +32 | +10.7% |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 177 | 165 | -12 | -6.8% |
| linear_horizontal_left | 206 | 150 | -56 | -27.2% |
| linear_horizontal_right | 173 | 163 | -10 | -5.8% |
| linear_vertical_down | 168 | 191 | +23 | +13.7% |
| linear_vertical_up | 128 | 114 | -14 | -10.9% |
| long_sticky_header | 1,405 | 1,293 | -112 | -8.0% |
| long_vertical_jitter | 1,352 | 1,293 | -59 | -4.4% |
| long_vertical_text | 1,438 | 1,412 | -26 | -1.8% |
| low_feature_text | 163 | 155 | -8 | -4.9% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 233 | 211 | -22 | -9.4% |

## Verifier (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 45 | 36 | -9 | -20.0% |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 137 | 123 | -14 | -10.2% |
| linear_horizontal_left | 126 | 124 | -2 | -1.6% |
| linear_horizontal_right | 209 | 172 | -37 | -17.7% |
| linear_vertical_down | 100 | 151 | +51 | +51.0% |
| linear_vertical_up | 158 | 152 | -6 | -3.8% |
| long_sticky_header | 814 | 874 | +60 | +7.4% |
| long_vertical_jitter | 889 | 998 | +109 | +12.3% |
| long_vertical_text | 984 | 984 | +0 | +0.0% |
| low_feature_text | 124 | 104 | -20 | -16.1% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 157 | 153 | -4 | -2.5% |

## Algorithmic counters (p50)

| scenario | coarse candidates before | after | Δ | Δ% | NCC offsets before | after | Δ | Δ% | NCC pixel visits before | after | Δ | Δ% | verifier candidates before | after | Δ | Δ% |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| bad_frame | 2 | 2 | +0 | +0.0% | 194 | 162 | -32 | -16.5% | 11,168,192 | 9,326,016 | -1,842,176 | -16.5% | 6 | 6 | +0 | +0.0% |
| duplicate_frames | 0 | 0 | +0 | n/a | 0 | 0 | +0 | n/a | 0 | 0 | +0 | n/a | 0 | 0 | +0 | n/a |
| image_cards | 1 | 1 | +0 | +0.0% | 164 | 164 | +0 | +0.0% | 9,441,152 | 9,441,152 | +0 | +0.0% | 3 | 3 | +0 | +0.0% |
| linear_horizontal_left | 1 | 1 | +0 | +0.0% | 154 | 154 | +0 | +0.0% | 8,865,472 | 8,865,472 | +0 | +0.0% | 3 | 3 | +0 | +0.0% |
| linear_horizontal_right | 1 | 1 | +0 | +0.0% | 154 | 154 | +0 | +0.0% | 8,865,472 | 8,865,472 | +0 | +0.0% | 3 | 3 | +0 | +0.0% |
| linear_vertical_down | 1 | 1 | +0 | +0.0% | 154 | 154 | +0 | +0.0% | 8,865,472 | 8,865,472 | +0 | +0.0% | 3 | 3 | +0 | +0.0% |
| linear_vertical_up | 1 | 1 | +0 | +0.0% | 154 | 154 | +0 | +0.0% | 8,865,472 | 8,865,472 | +0 | +0.0% | 3 | 3 | +0 | +0.0% |
| long_sticky_header | 1 | 1 | +0 | +0.0% | 174 | 174 | +0 | +0.0% | 49,889,280 | 49,889,280 | +0 | +0.0% | 3 | 3 | +0 | +0.0% |
| long_vertical_jitter | 1 | 1 | +0 | +0.0% | 174 | 174 | +0 | +0.0% | 49,889,280 | 49,889,280 | +0 | +0.0% | 3 | 3 | +0 | +0.0% |
| long_vertical_text | 1 | 1 | +0 | +0.0% | 174 | 174 | +0 | +0.0% | 49,889,280 | 49,889,280 | +0 | +0.0% | 3 | 3 | +0 | +0.0% |
| low_feature_text | 1 | 1 | +0 | +0.0% | 174 | 174 | +0 | +0.0% | 10,016,832 | 10,016,832 | +0 | +0.0% | 3 | 3 | +0 | +0.0% |
| repeated_grid | 0 | 0 | +0 | n/a | 0 | 0 | +0 | n/a | 0 | 0 | +0 | n/a | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a | 0 | 0 | +0 | n/a | 0 | 0 | +0 | n/a | 0 | 0 | +0 | n/a |
| sticky_header | 1 | 1 | +0 | +0.0% | 152 | 152 | +0 | +0.0% | 8,750,336 | 8,750,336 | +0 | +0.0% | 3 | 3 | +0 | +0.0% |

## Peak RSS Δ (kB)

| scenario | before kB | after kB | Δ kB |
|---|---:|---:|---:|
| bad_frame | 3,424 | 3,916 | +492 |
| duplicate_frames | 3,000 | 3,500 | +500 |
| image_cards | 3,372 | 3,908 | +536 |
| linear_horizontal_left | 3,452 | 3,900 | +448 |
| linear_horizontal_right | 3,448 | 3,844 | +396 |
| linear_vertical_down | 3,444 | 3,908 | +464 |
| linear_vertical_up | 3,436 | 3,840 | +404 |
| long_sticky_header | 71,260 | 70,564 | -696 |
| long_vertical_jitter | 98,124 | 97,436 | -688 |
| long_vertical_text | 99,248 | 98,560 | -688 |
| low_feature_text | 3,428 | 3,840 | +412 |
| repeated_grid | 2,476 | 2,476 | +0 |
| repeated_rows | 2,476 | 2,476 | +0 |
| sticky_header | 3,436 | 3,904 | +468 |

## Regressions (Δ > +5%)

- **bad_frame**: +131.1%
- **long_vertical_jitter**: +98.4%
- **long_sticky_header**: +92.5%
- **linear_vertical_down**: +51.0%
- **long_vertical_jitter**: +20.2%
- **long_sticky_header**: +16.6%
- **linear_vertical_down**: +13.7%
- **long_vertical_jitter**: +12.5%
- **long_vertical_jitter**: +12.3%
- **bad_frame**: +10.7%
- **long_vertical_jitter**: +7.7%
- **long_sticky_header**: +7.4%
- **bad_frame**: +6.7%

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

