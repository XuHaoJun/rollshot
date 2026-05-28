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
  short_commit: 33ff116
  commit: 33ff116eecfc4366d5c5cd0628f021e372f54113
  jsonl: bench-results/runs/p3-fast-ncc/after.jsonl
run:
  date: 2026-05-27
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
# Benchmark comparison: b96c907 → 33ff116

## Total time per frame (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 2,559 | 997 | -1,562 | -61.0% |
| duplicate_frames | 223 | 222 | -1 | -0.4% |
| image_cards | 3,419 | 1,493 | -1,926 | -56.3% |
| linear_horizontal_left | 3,074 | 1,017 | -2,057 | -66.9% |
| linear_horizontal_right | 3,564 | 983 | -2,581 | -72.4% |
| linear_vertical_down | 1,952 | 1,115 | -837 | -42.9% |
| linear_vertical_up | 2,428 | 937 | -1,491 | -61.4% |
| long_sticky_header | 12,734 | 5,465 | -7,269 | -57.1% |
| long_vertical_jitter | 12,583 | 5,153 | -7,430 | -59.0% |
| long_vertical_text | 12,760 | 5,290 | -7,470 | -58.5% |
| low_feature_text | 2,536 | 1,107 | -1,429 | -56.3% |
| repeated_grid | 4 | 1 | -3 | -75.0% |
| repeated_rows | 3 | 1 | -2 | -66.7% |
| sticky_header | 3,264 | 1,296 | -1,968 | -60.3% |

## Append time (p95) — P1 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 67 | 55 | -12 | -17.9% |
| duplicate_frames | 122 | 117 | -5 | -4.1% |
| image_cards | 139 | 107 | -32 | -23.0% |
| linear_horizontal_left | 139 | 136 | -3 | -2.2% |
| linear_horizontal_right | 141 | 132 | -9 | -6.4% |
| linear_vertical_down | 132 | 137 | +5 | +3.8% |
| linear_vertical_up | 141 | 134 | -7 | -5.0% |
| long_sticky_header | 2,455 | 2,365 | -90 | -3.7% |
| long_vertical_jitter | 4,092 | 4,022 | -70 | -1.7% |
| long_vertical_text | 5,059 | 4,737 | -322 | -6.4% |
| low_feature_text | 247 | 124 | -123 | -49.8% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 120 | 116 | -4 | -3.3% |

## Prepare (p50) — P2 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 65 | 69 | +4 | +6.2% |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 76 | 73 | -3 | -3.9% |
| linear_horizontal_left | 100 | 81 | -19 | -19.0% |
| linear_horizontal_right | 87 | 81 | -6 | -6.9% |
| linear_vertical_down | 97 | 103 | +6 | +6.2% |
| linear_vertical_up | 86 | 78 | -8 | -9.3% |
| long_sticky_header | 443 | 858 | +415 | +93.7% |
| long_vertical_jitter | 452 | 450 | -2 | -0.4% |
| long_vertical_text | 500 | 444 | -56 | -11.2% |
| low_feature_text | 76 | 97 | +21 | +27.6% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 84 | 84 | +0 | +0.0% |

## Coarse (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 168 | 181 | +13 | +7.7% |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 197 | 157 | -40 | -20.3% |
| linear_horizontal_left | 189 | 163 | -26 | -13.8% |
| linear_horizontal_right | 195 | 192 | -3 | -1.5% |
| linear_vertical_down | 188 | 209 | +21 | +11.2% |
| linear_vertical_up | 435 | 231 | -204 | -46.9% |
| long_sticky_header | 820 | 932 | +112 | +13.7% |
| long_vertical_jitter | 919 | 819 | -100 | -10.9% |
| long_vertical_text | 888 | 875 | -13 | -1.5% |
| low_feature_text | 174 | 166 | -8 | -4.6% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 244 | 277 | +33 | +13.5% |

## NCC (p50) — P3 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 1,620 | 283 | -1,337 | -82.5% |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 2,255 | 513 | -1,742 | -77.3% |
| linear_horizontal_left | 2,138 | 368 | -1,770 | -82.8% |
| linear_horizontal_right | 2,132 | 307 | -1,825 | -85.6% |
| linear_vertical_down | 956 | 193 | -763 | -79.8% |
| linear_vertical_up | 1,367 | 208 | -1,159 | -84.8% |
| long_sticky_header | 9,113 | 1,606 | -7,507 | -82.4% |
| long_vertical_jitter | 8,427 | 1,521 | -6,906 | -82.0% |
| long_vertical_text | 8,765 | 1,516 | -7,249 | -82.7% |
| low_feature_text | 1,267 | 352 | -915 | -72.2% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 2,492 | 418 | -2,074 | -83.2% |

## Verifier (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| bad_frame | 68 | 66 | -2 | -2.9% |
| duplicate_frames | 0 | 0 | +0 | n/a |
| image_cards | 136 | 124 | -12 | -8.8% |
| linear_horizontal_left | 161 | 138 | -23 | -14.3% |
| linear_horizontal_right | 220 | 185 | -35 | -15.9% |
| linear_vertical_down | 102 | 101 | -1 | -1.0% |
| linear_vertical_up | 196 | 162 | -34 | -17.3% |
| long_sticky_header | 876 | 917 | +41 | +4.7% |
| long_vertical_jitter | 998 | 882 | -116 | -11.6% |
| long_vertical_text | 1,043 | 958 | -85 | -8.1% |
| low_feature_text | 129 | 135 | +6 | +4.7% |
| repeated_grid | 0 | 0 | +0 | n/a |
| repeated_rows | 0 | 0 | +0 | n/a |
| sticky_header | 164 | 153 | -11 | -6.7% |

## Peak RSS Δ (kB)

| scenario | before kB | after kB | Δ kB |
|---|---:|---:|---:|
| bad_frame | 3,440 | 3,436 | -4 |
| duplicate_frames | 3,024 | 3,008 | -16 |
| image_cards | 3,436 | 3,504 | +68 |
| linear_horizontal_left | 3,460 | 3,532 | +72 |
| linear_horizontal_right | 3,464 | 3,464 | +0 |
| linear_vertical_down | 3,452 | 3,452 | +0 |
| linear_vertical_up | 3,444 | 3,516 | +72 |
| long_sticky_header | 71,272 | 71,276 | +4 |
| long_vertical_jitter | 98,132 | 98,136 | +4 |
| long_vertical_text | 99,252 | 99,264 | +12 |
| low_feature_text | 3,432 | 3,512 | +80 |
| repeated_grid | 2,476 | 2,476 | +0 |
| repeated_rows | 2,476 | 2,476 | +0 |
| sticky_header | 3,428 | 3,424 | -4 |

## Regressions (Δ > +5%)

- **long_sticky_header**: +93.7%
- **low_feature_text**: +27.6%
- **long_sticky_header**: +13.7%
- **sticky_header**: +13.5%
- **linear_vertical_down**: +11.2%
- **bad_frame**: +7.7%
- **linear_vertical_down**: +6.2%
- **bad_frame**: +6.2%

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

