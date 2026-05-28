---
kind: stitch_sequences_benchmark_compare
schema_version: 1
benchmark_id: 2026-05-27-p3-fast-ncc
benchmark_scope: p3-fast-ncc
roadmap_item: P3
status: user_accepted
before:
  short_commit: b96c907
  commit: b96c90718be9ca727116417bdb57a5970a80df04
  jsonl: bench-results/runs/p3-fast-ncc/before.jsonl
after:
  short_commit: ae55050
  commit: ae55050cb14007ef6b5687da7cec4addadf335dd
  jsonl: bench-results/runs/p3-fast-ncc/after.jsonl
run:
  date: 2026-05-28
  harness: crates/rollshot-core/benches/stitch_sequences.rs
  command: "rtk cargo bench -p rollshot-core --bench stitch_sequences -- --out bench-results/runs/p3-fast-ncc/after.jsonl"
  fixtures:
    []
  repeats: 0
environment:
  os: Linux-6.8.0-117-generic-x86_64-with-glibc2.39
  architecture: x86_64
  cpu_model: "Intel(R) Core(TM) Ultra 7 265K"
  logical_cpus: 8
notes:
  - "Raw JSONL files are local benchmark artifacts under bench-results/runs/ and are not intended to be committed."
  - "Peak RSS is allocator- and machine-dependent; compare trends on this machine."
---
# Benchmark comparison: b96c907 → ae55050

## Total time per frame (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 2,559 | 1,018 | -1,541 | -60.2% |
| duplicate_frames | 223 | 160 | -63 | -28.3% |
| image_cards | 3,419 | 922 | -2,497 | -73.0% |
| linear_horizontal_left | 3,074 | 1,009 | -2,065 | -67.2% |
| linear_horizontal_right | 3,564 | 989 | -2,575 | -72.3% |
| linear_vertical_down | 1,952 | 834 | -1,118 | -57.3% |
| linear_vertical_up | 2,428 | 953 | -1,475 | -60.7% |
| long_sticky_header | 12,734 | 5,474 | -7,260 | -57.0% |
| long_vertical_jitter | 12,583 | 5,507 | -7,076 | -56.2% |
| long_vertical_text | 12,760 | 5,452 | -7,308 | -57.3% |
| low_feature_text | 2,536 | 875 | -1,661 | -65.5% |
| repeated_grid | 4 | 1 | -3 | -75.0% |
| repeated_rows | 3 | 2 | -1 | -33.3% |
| sticky_header | 3,264 | 1,004 | -2,260 | -69.2% |

## Append time (p95) — P1 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 67 | 55 | -12 | -17.9% |
| duplicate_frames | 122 | 117 | -5 | -4.1% |
| image_cards | 139 | 100 | -39 | -28.1% |
| linear_horizontal_left | 139 | 103 | -36 | -25.9% |
| linear_horizontal_right | 141 | 108 | -33 | -23.4% |
| linear_vertical_down | 132 | 128 | -4 | -3.0% |
| linear_vertical_up | 141 | 132 | -9 | -6.4% |
| long_sticky_header | 2,455 | 2,441 | -14 | -0.6% |
| long_vertical_jitter | 4,092 | 4,167 | +75 | +1.8% |
| long_vertical_text | 5,059 | 4,370 | -689 | -13.6% |
| low_feature_text | 247 | 94 | -153 | -61.9% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 120 | 96 | -24 | -20.0% |

## Prepare (p50) — P2 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 65 | 68 | +3 | +4.6% |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 76 | 76 | +0 | +0.0% |
| linear_horizontal_left | 100 | 76 | -24 | -24.0% |
| linear_horizontal_right | 87 | 76 | -11 | -12.6% |
| linear_vertical_down | 97 | 76 | -21 | -21.6% |
| linear_vertical_up | 86 | 120 | +34 | +39.5% |
| long_sticky_header | 443 | 894 | +451 | +101.8% |
| long_vertical_jitter | 452 | 434 | -18 | -4.0% |
| long_vertical_text | 500 | 438 | -62 | -12.4% |
| low_feature_text | 76 | 74 | -2 | -2.6% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 84 | 75 | -9 | -10.7% |

## Coarse (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 168 | 174 | +6 | +3.6% |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 197 | 164 | -33 | -16.8% |
| linear_horizontal_left | 189 | 153 | -36 | -19.0% |
| linear_horizontal_right | 195 | 165 | -30 | -15.4% |
| linear_vertical_down | 188 | 155 | -33 | -17.6% |
| linear_vertical_up | 435 | 194 | -241 | -55.4% |
| long_sticky_header | 820 | 940 | +120 | +14.6% |
| long_vertical_jitter | 919 | 946 | +27 | +2.9% |
| long_vertical_text | 888 | 938 | +50 | +5.6% |
| low_feature_text | 174 | 161 | -13 | -7.5% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 244 | 164 | -80 | -32.8% |

## NCC (p50) — P3 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 1,620 | 289 | -1,331 | -82.2% |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 2,255 | 371 | -1,884 | -83.5% |
| linear_horizontal_left | 2,138 | 384 | -1,754 | -82.0% |
| linear_horizontal_right | 2,132 | 328 | -1,804 | -84.6% |
| linear_vertical_down | 956 | 194 | -762 | -79.7% |
| linear_vertical_up | 1,367 | 224 | -1,143 | -83.6% |
| long_sticky_header | 9,113 | 1,637 | -7,476 | -82.0% |
| long_vertical_jitter | 8,427 | 1,516 | -6,911 | -82.0% |
| long_vertical_text | 8,765 | 1,477 | -7,288 | -83.1% |
| low_feature_text | 1,267 | 243 | -1,024 | -80.8% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 2,492 | 372 | -2,120 | -85.1% |

## Verifier (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 68 | 49 | -19 | -27.9% |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 136 | 123 | -13 | -9.6% |
| linear_horizontal_left | 161 | 120 | -41 | -25.5% |
| linear_horizontal_right | 220 | 171 | -49 | -22.3% |
| linear_vertical_down | 102 | 81 | -21 | -20.6% |
| linear_vertical_up | 196 | 157 | -39 | -19.9% |
| long_sticky_header | 876 | 917 | +41 | +4.7% |
| long_vertical_jitter | 998 | 949 | -49 | -4.9% |
| long_vertical_text | 1,043 | 1,010 | -33 | -3.2% |
| low_feature_text | 129 | 103 | -26 | -20.2% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 164 | 153 | -11 | -6.7% |

## Peak RSS Δ (kB)

| scenario | before kB | after kB | Δ kB |
|---|---:|---:|---:|
| bad_frame | 3,440 | 3,436 | -4 |
| duplicate_frames | 3,024 | 3,032 | +8 |
| image_cards | 3,436 | 3,448 | +12 |
| linear_horizontal_left | 3,460 | 3,456 | -4 |
| linear_horizontal_right | 3,464 | 3,448 | -16 |
| linear_vertical_down | 3,452 | 3,448 | -4 |
| linear_vertical_up | 3,444 | 3,380 | -64 |
| long_sticky_header | 71,272 | 71,280 | +8 |
| long_vertical_jitter | 98,132 | 98,144 | +12 |
| long_vertical_text | 99,252 | 99,260 | +8 |
| low_feature_text | 3,432 | 3,492 | +60 |
| repeated_grid | 2,476 | 2,476 | +0 |
| repeated_rows | 2,476 | 2,476 | +0 |
| sticky_header | 3,428 | 3,436 | +8 |

## Regressions (Δ > +5%)

- **long_sticky_header**: +101.8%
- **linear_vertical_up**: +39.5%
- **long_sticky_header**: +14.6%
- **long_vertical_text**: +5.6%

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

