---
kind: stitch_sequences_benchmark_compare
schema_version: 1
benchmark_id: 2026-05-28-p4-axis-fast-path
benchmark_scope: p4-axis-fast-path
roadmap_item: P4
status: draft
before:
  short_commit: 3bd1fe4
  commit: 3bd1fe4e6ff453651716e05ad06a0566a16c76a8
  jsonl: bench-results/runs/p4-axis-fast-path/before.jsonl
after:
  short_commit: 0341832
  commit: 03418327f4199b9c4b17356926c48fe09f05537a
  jsonl: bench-results/runs/p4-axis-fast-path/after.jsonl
run:
  date: 2026-05-28
  harness: crates/rollshot-core/benches/stitch_sequences.rs
  command: "rtk cargo bench -p rollshot-core --bench stitch_sequences -- --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter,linear_horizontal_right --repeats 3 --out bench-results/runs/p4-axis-fast-path/after.jsonl"
  fixtures:
    - long_vertical_text
    - long_sticky_header
    - long_vertical_jitter
    - linear_horizontal_right
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

# Benchmark comparison: 3bd1fe4 → 0341832

## Total time per frame (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| linear_horizontal_right | 1,188 | 903 | -285 | -24.0% |
| long_sticky_header | 5,524 | 4,731 | -793 | -14.4% |
| long_vertical_jitter | 5,564 | 4,807 | -757 | -13.6% |
| long_vertical_text | 5,846 | 5,358 | -488 | -8.3% |

## Append time (p95) — P1 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| linear_horizontal_right | 146 | 138 | -8 | -5.5% |
| long_sticky_header | 2,568 | 2,410 | -158 | -6.2% |
| long_vertical_jitter | 4,194 | 4,075 | -119 | -2.8% |
| long_vertical_text | 4,633 | 4,783 | +150 | +3.2% |

## Prepare (p50) — P2 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| linear_horizontal_right | 101 | 99 | -2 | -2.0% |
| long_sticky_header | 785 | 446 | -339 | -43.2% |
| long_vertical_jitter | 447 | 433 | -14 | -3.1% |
| long_vertical_text | 756 | 467 | -289 | -38.2% |

## Coarse (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| linear_horizontal_right | 204 | 151 | -53 | -26.0% |
| long_sticky_header | 956 | 696 | -260 | -27.2% |
| long_vertical_jitter | 948 | 684 | -264 | -27.8% |
| long_vertical_text | 963 | 832 | -131 | -13.6% |

## NCC (p50) — P3 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| linear_horizontal_right | 365 | 204 | -161 | -44.1% |
| long_sticky_header | 1,743 | 1,343 | -400 | -22.9% |
| long_vertical_jitter | 1,573 | 1,291 | -282 | -17.9% |
| long_vertical_text | 1,611 | 1,300 | -311 | -19.3% |

## Verifier (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| linear_horizontal_right | 212 | 204 | -8 | -3.8% |
| long_sticky_header | 918 | 836 | -82 | -8.9% |
| long_vertical_jitter | 961 | 881 | -80 | -8.3% |
| long_vertical_text | 1,150 | 1,140 | -10 | -0.9% |

## Peak RSS Δ (kB)

| scenario | before kB | after kB | Δ kB |
|---|---:|---:|---:|
| linear_horizontal_right | 3,456 | 3,452 | -4 |
| long_sticky_header | 71,276 | 71,264 | -12 |
| long_vertical_jitter | 98,140 | 98,120 | -20 |
| long_vertical_text | 99,264 | 99,252 | -12 |

## Regressions (Δ > +5%)

(none) ✅

## Output correctness drift

| scenario | before hash | after hash | diff? |
|---|---|---|---|
| linear_horizontal_right | `60b2af82e9b36164` | `60b2af82e9b36164` | same |
| long_sticky_header | `fd57675e84120080` | `fd57675e84120080` | same |
| long_vertical_jitter | `420a65270e9feaaf` | `420a65270e9feaaf` | same |
| long_vertical_text | `5c0f3290b99f1b97` | `5c0f3290b99f1b97` | same |
