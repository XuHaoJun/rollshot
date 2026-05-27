---
kind: stitch_sequences_benchmark_compare
benchmark_id: 2026-05-27-p1-strip-canvas
benchmark_scope: p1-strip-canvas
status: user_accepted
before:
  short_commit: f404e61
  commit: f404e613fa7447a6b04e266b5550501bd0cc5014
  jsonl: bench-results/2026-05-27-p1-strip-canvas-before-f404e61.jsonl
after:
  short_commit: d208cf5
  commit: d208cf5583d485172d24a765601cd3b5482ab248
  jsonl: bench-results/2026-05-27-p1-strip-canvas-after.jsonl
run:
  date: 2026-05-27
  harness: crates/rollshot-core/benches/stitch_sequences.rs
  command: "rtk cargo bench -p rollshot-core --bench stitch_sequences -- --fixtures long_vertical_text,long_sticky_header,long_vertical_jitter --repeats 3 --out bench-results/2026-05-27-p1-strip-canvas-after.jsonl"
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

# Benchmark comparison: f404e61 → d208cf5

## Total time per frame (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| long_sticky_header | 14,905 | 14,995 | +90 | +0.6% |
| long_vertical_jitter | 20,897 | 14,295 | -6,602 | -31.6% |
| long_vertical_text | 21,793 | 15,149 | -6,644 | -30.5% |

## Append time (p95) — P1 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| long_sticky_header | 5,678 | 2,267 | -3,411 | -60.1% |
| long_vertical_jitter | 13,205 | 3,651 | -9,554 | -72.4% |
| long_vertical_text | 13,413 | 4,613 | -8,800 | -65.6% |

## Prepare (p50) — P2 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| long_sticky_header | 943 | 1,440 | +497 | +52.7% |
| long_vertical_jitter | 1,266 | 1,043 | -223 | -17.6% |
| long_vertical_text | 1,258 | 1,480 | +222 | +17.6% |

## Coarse (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| long_sticky_header | 1,321 | 1,586 | +265 | +20.1% |
| long_vertical_jitter | 1,620 | 1,317 | -303 | -18.7% |
| long_vertical_text | 1,640 | 1,610 | -30 | -1.8% |

## NCC (p50) — P3 target

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| long_sticky_header | 9,040 | 9,290 | +250 | +2.8% |
| long_vertical_jitter | 8,434 | 8,462 | +28 | +0.3% |
| long_vertical_text | 8,404 | 8,730 | +326 | +3.9% |

## Verifier (p50)

| scenario | before µs | after µs | Δ | Δ% |
|---|---:|---:|---:|---:|
| long_sticky_header | 823 | 934 | +111 | +13.5% |
| long_vertical_jitter | 965 | 914 | -51 | -5.3% |
| long_vertical_text | 1,024 | 1,166 | +142 | +13.9% |

## Peak RSS Δ (kB)

| scenario | before kB | after kB | Δ kB |
|---|---:|---:|---:|
| long_sticky_header | 56,036 | 70,012 | +13,976 |
| long_vertical_jitter | 71,760 | 96,880 | +25,120 |
| long_vertical_text | 71,784 | 98,012 | +26,228 |

## Regressions (Δ > +5%)

- **long_sticky_header**: +52.7%
- **long_sticky_header**: +20.1%
- **long_vertical_text**: +17.6%
- **long_vertical_text**: +13.9%
- **long_sticky_header**: +13.5%

## Output correctness drift

| scenario | before hash | after hash | diff? |
|---|---|---|---|
| long_sticky_header | `fd57675e84120080` | `fd57675e84120080` | same |
| long_vertical_jitter | `420a65270e9feaaf` | `420a65270e9feaaf` | same |
| long_vertical_text | `5c0f3290b99f1b97` | `5c0f3290b99f1b97` | same |
